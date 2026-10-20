//! Cliente LSP nativo (JSON-RPC sobre stdio): diagnósticos, hover e ir para a
//! definição. Usa os servidores que já existem na máquina (PATH ou o Mason do
//! Neovim); sem servidor para a linguagem, simplesmente não faz nada.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub row: usize,
    pub col: usize,
    pub end_row: usize,
    pub end_col: usize,
    /// 1 erro, 2 aviso, 3 informação, 4 dica
    pub severity: u8,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub arquivo: PathBuf,
    pub row: usize,
    pub col: usize,
}

/// Uma sugestão de completar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// texto usado para filtrar (`filterText` ou o rótulo)
    pub filtro: String,
    /// o que entra no buffer (já sem a sintaxe de snippet)
    pub texto: String,
    pub detalhe: String,
    /// 1 texto, 2 método, 3 função, 4 construtor, 5 campo, 6 variável, 7 classe, 8 interface, 9 módulo, 10 propriedade, 14 palavra-chave, 15 snippet, 22 struct...
    pub tipo: u8,
    /// `textEdit.range` inicial (linha, coluna UTF-16), se houver
    pub inicio: Option<(usize, usize)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tipo {
    Init,
    Hover,
    Definicao,
    Completar,
}

/// O que o servidor já contou (lido pela UI).
#[derive(Default)]
pub struct Estado {
    pub diagnosticos: HashMap<String, Vec<Diag>>,
    pub hover: Option<(i64, String)>,
    pub definicao: Option<(i64, Vec<Local>)>,
    pub completar: Option<(i64, Vec<Item>)>,
    pub pronto: bool,
}

pub struct Cliente {
    pub nome: String,
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next: AtomicI64,
    kinds: Arc<Mutex<HashMap<i64, Tipo>>>,
    pub estado: Arc<Mutex<Estado>>,
    /// mensagens que esperam o `initialize` terminar
    fila: Arc<Mutex<Vec<Value>>>,
    pronto: Arc<AtomicBool>,
    pub versao: i64,
}

/// `(programa, argumentos, languageId)` para a extensão do arquivo.
pub fn servidor_para(arquivo: &Path) -> Option<(String, Vec<String>, &'static str)> {
    // ASTROGRAPH_LSP_CMD="programa args...": força um servidor (testes e servidores personalizados)
    if let Ok(cmd) = std::env::var("ASTROGRAPH_LSP_CMD") {
        let mut it = cmd.split_whitespace().map(str::to_string);
        let prog = it.next()?;
        return Some((prog, it.collect(), "plaintext"));
    }
    let ext = arquivo.extension()?.to_string_lossy().to_lowercase();
    let (cmds, lang): (&[(&str, &[&str])], &'static str) = match ext.as_str() {
        "rs" => (&[("rust-analyzer", &[])], "rust"),
        "py" | "pyi" => (&[("pyright-langserver", &["--stdio"]), ("basedpyright-langserver", &["--stdio"]), ("pylsp", &[])], "python"),
        "ts" | "tsx" => (&[("typescript-language-server", &["--stdio"])], "typescript"),
        "js" | "jsx" | "mjs" => (&[("typescript-language-server", &["--stdio"])], "javascript"),
        "go" => (&[("gopls", &[])], "go"),
        "lua" => (&[("lua-language-server", &[])], "lua"),
        "c" | "h" => (&[("clangd", &[])], "c"),
        "cpp" | "cc" | "hpp" => (&[("clangd", &[])], "cpp"),
        "sh" | "bash" => (&[("bash-language-server", &["start"])], "shellscript"),
        _ => return None,
    };
    cmds.iter().find_map(|(nome, args)| achar(nome).map(|p| (p.to_string_lossy().into_owned(), args.iter().map(|a| a.to_string()).collect(), lang)))
}

/// Procura no PATH e na pasta de binários do Mason (a que o Neovim já usa).
fn achar(nome: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("nvim-data/mason/bin"));
    }
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/nvim/mason/bin"));
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    dirs.iter().find_map(|d| exts.iter().map(|e| d.join(format!("{nome}{e}"))).find(|p| p.is_file()))
}

pub fn uri(p: &Path) -> String {
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf()) };
    let s = abs.to_string_lossy().replace('\\', "/");
    let enc: String = s
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | '-' | '_' | '.' | '~' | ':' => c.to_string(),
            c => c.to_string().bytes().map(|b| format!("%{b:02X}")).collect(),
        })
        .collect();
    if enc.starts_with('/') { format!("file://{enc}") } else { format!("file:///{enc}") }
}

pub fn de_uri(u: &str) -> Option<PathBuf> {
    let r = u.strip_prefix("file://")?;
    let r = if r.len() > 2 && r.as_bytes()[0] == b'/' && r.as_bytes()[2] == b':' { &r[1..] } else { r };
    let mut out = Vec::new();
    let b = r.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 + 0 {
            if let Ok(v) = u8::from_str_radix(&r[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

/// Coluna em caracteres -> unidades UTF-16 (a medida do LSP).
pub fn col_utf16(linha: &str, col: usize) -> usize {
    linha.chars().take(col).map(char::len_utf16).sum()
}

/// Unidades UTF-16 -> coluna em caracteres.
pub fn col_chars(linha: &str, u16col: usize) -> usize {
    let mut n = 0;
    for (i, c) in linha.chars().enumerate() {
        if n >= u16col {
            return i;
        }
        n += c.len_utf16();
    }
    linha.chars().count()
}

impl Cliente {
    /// Abre o servidor e começa o `initialize`; `notify` roda (em outra thread) quando há novidade.
    pub fn iniciar(prog: &str, args: &[String], raiz: &Path, notify: Arc<dyn Fn() + Send + Sync>) -> Result<Cliente, String> {
        let mut child = Command::new(prog).args(args).current_dir(raiz).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("sem stdin")?));
        let stdout = child.stdout.take().ok_or("sem stdout")?;
        let estado = Arc::new(Mutex::new(Estado::default()));
        let kinds: Arc<Mutex<HashMap<i64, Tipo>>> = Arc::new(Mutex::new(HashMap::new()));
        let fila: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let pronto = Arc::new(AtomicBool::new(false));
        {
            let (stdin, estado, kinds, fila, pronto) = (stdin.clone(), estado.clone(), kinds.clone(), fila.clone(), pronto.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(stdout);
                while let Some(msg) = ler(&mut r) {
                    let id = msg.get("id").and_then(Value::as_i64);
                    if msg.get("method").is_some() && id.is_some() {
                        // pedido do servidor (registro de capacidades, configuração...): responde vazio
                        let resp = json!({"jsonrpc": "2.0", "id": msg["id"], "result": Value::Null});
                        let _ = escrever(&mut *stdin.lock().unwrap(), &resp);
                        continue;
                    }
                    if let Some(m) = msg.get("method").and_then(Value::as_str) {
                        if m == "textDocument/publishDiagnostics" {
                            let p = &msg["params"];
                            if let Some(u) = p["uri"].as_str() {
                                let diags = p["diagnostics"].as_array().map(|a| a.iter().filter_map(diag_de).collect()).unwrap_or_default();
                                estado.lock().unwrap().diagnosticos.insert(u.to_string(), diags);
                                notify();
                            }
                        }
                        continue;
                    }
                    let Some(id) = id else { continue };
                    let tipo = kinds.lock().unwrap().remove(&id);
                    match tipo {
                        Some(Tipo::Init) => {
                            let mut w = stdin.lock().unwrap();
                            let _ = escrever(&mut *w, &json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
                            for m in fila.lock().unwrap().drain(..) {
                                let _ = escrever(&mut *w, &m);
                            }
                            pronto.store(true, Ordering::Release);
                            estado.lock().unwrap().pronto = true;
                            notify();
                        }
                        Some(Tipo::Hover) => {
                            estado.lock().unwrap().hover = Some((id, hover_texto(&msg["result"])));
                            notify();
                        }
                        Some(Tipo::Completar) => {
                            estado.lock().unwrap().completar = Some((id, itens(&msg["result"])));
                            notify();
                        }
                        Some(Tipo::Definicao) => {
                            estado.lock().unwrap().definicao = Some((id, locais(&msg["result"])));
                            notify();
                        }
                        None => {}
                    }
                }
            });
        }
        let c = Cliente { nome: prog.rsplit(['/', '\\']).next().unwrap_or(prog).to_string(), child, stdin, next: AtomicI64::new(1), kinds, estado, fila, pronto, versao: 0 };
        let init = json!({
            "processId": std::process::id(),
            "rootUri": uri(raiz),
            "workspaceFolders": [{"uri": uri(raiz), "name": raiz.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()}],
            "capabilities": {
                "textDocument": {
                    "synchronization": {"didSave": true},
                    "publishDiagnostics": {"relatedInformation": false},
                    "hover": {"contentFormat": ["plaintext", "markdown"]},
                    "definition": {"linkSupport": false},
                    "completion": {"completionItem": {"snippetSupport": false, "documentationFormat": ["plaintext"]}}
                },
                "workspace": {"workspaceFolders": true}
            }
        });
        c.pedido("initialize", init, Tipo::Init);
        Ok(c)
    }

    fn pedido(&self, metodo: &str, params: Value, tipo: Tipo) -> i64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.kinds.lock().unwrap().insert(id, tipo);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": params});
        self.enviar(msg, tipo == Tipo::Init);
        id
    }

    fn notificacao(&self, metodo: &str, params: Value) {
        self.enviar(json!({"jsonrpc": "2.0", "method": metodo, "params": params}), false);
    }

    fn enviar(&self, msg: Value, agora: bool) {
        if agora || self.pronto.load(Ordering::Acquire) {
            let _ = escrever(&mut *self.stdin.lock().unwrap(), &msg);
        } else {
            self.fila.lock().unwrap().push(msg);
        }
    }

    pub fn abrir(&mut self, arquivo: &Path, linguagem: &str, texto: &str) {
        self.versao = 1;
        self.notificacao("textDocument/didOpen", json!({"textDocument": {"uri": uri(arquivo), "languageId": linguagem, "version": self.versao, "text": texto}}));
    }

    pub fn mudou(&mut self, arquivo: &Path, texto: &str) {
        self.versao += 1;
        self.notificacao("textDocument/didChange", json!({"textDocument": {"uri": uri(arquivo), "version": self.versao}, "contentChanges": [{"text": texto}]}));
    }

    pub fn fechar(&mut self, arquivo: &Path) {
        self.notificacao("textDocument/didClose", json!({"textDocument": {"uri": uri(arquivo)}}));
    }

    /// `K`: pede o hover na posição (linha, coluna em unidades UTF-16).
    pub fn hover(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        self.pedido("textDocument/hover", json!({"textDocument": {"uri": uri(arquivo)}, "position": {"line": row, "character": col16}}), Tipo::Hover)
    }

    /// Pede sugestões na posição (linha, coluna UTF-16).
    pub fn completar(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        self.pedido("textDocument/completion", json!({"textDocument": {"uri": uri(arquivo)}, "position": {"line": row, "character": col16}}), Tipo::Completar)
    }

    /// `gd`: pede a definição.
    pub fn definicao(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        self.pedido("textDocument/definition", json!({"textDocument": {"uri": uri(arquivo)}, "position": {"line": row, "character": col16}}), Tipo::Definicao)
    }

    pub fn diagnosticos(&self, arquivo: &Path) -> Vec<Diag> {
        self.estado.lock().unwrap().diagnosticos.get(&uri(arquivo)).cloned().unwrap_or_default()
    }

    pub fn encerrar(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for Cliente {
    fn drop(&mut self) {
        self.encerrar();
    }
}

fn escrever(w: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let corpo = msg.to_string();
    write!(w, "Content-Length: {}\r\n\r\n{}", corpo.len(), corpo)?;
    w.flush()
}

fn ler(r: &mut impl BufRead) -> Option<Value> {
    let mut tam = None;
    loop {
        let mut linha = String::new();
        if r.read_line(&mut linha).ok()? == 0 {
            return None;
        }
        let l = linha.trim();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.strip_prefix("Content-Length:") {
            tam = v.trim().parse::<usize>().ok();
        }
    }
    let mut buf = vec![0u8; tam?];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

fn diag_de(v: &Value) -> Option<Diag> {
    let r = &v["range"];
    Some(Diag {
        row: r["start"]["line"].as_u64()? as usize,
        col: r["start"]["character"].as_u64()? as usize,
        end_row: r["end"]["line"].as_u64()? as usize,
        end_col: r["end"]["character"].as_u64()? as usize,
        severity: v["severity"].as_u64().unwrap_or(1) as u8,
        message: v["message"].as_str()?.to_string(),
    })
}

fn hover_texto(r: &Value) -> String {
    fn parte(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(o) => o.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
            Value::Array(a) => a.iter().map(parte).collect::<Vec<_>>().join("\n"),
            _ => String::new(),
        }
    }
    let t = parte(&r["contents"]);
    // tira as cercas de código do markdown (```rust ... ```)
    t.lines().filter(|l| !l.trim_start().starts_with("```")).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// Tira a sintaxe de snippet (`$1`, `${2:nome}`, `$0`) do texto que entra no buffer.
pub fn sem_snippet(t: &str) -> String {
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            out.push(c[i + 1]);
            i += 2;
        } else if c[i] == '$' && i + 1 < c.len() && c[i + 1].is_ascii_digit() {
            i += 2;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
        } else if c[i] == '$' && i + 1 < c.len() && c[i + 1] == '{' {
            // ${1:padrão} -> padrão ; ${1} -> nada
            let mut j = i + 2;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
            let mut depth = 1;
            let ini = if j < c.len() && c[j] == ':' { j + 1 } else { j };
            let mut k = ini;
            while k < c.len() && depth > 0 {
                if c[k] == '{' {
                    depth += 1;
                } else if c[k] == '}' {
                    depth -= 1;
                }
                k += 1;
            }
            let fim = if depth == 0 { k - 1 } else { k };
            if j < c.len() && c[j] == ':' {
                out.push_str(&sem_snippet(&c[ini..fim.max(ini)].iter().collect::<String>()));
            }
            i = k;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

fn itens(r: &Value) -> Vec<Item> {
    let lista = match r {
        Value::Array(a) => a.as_slice(),
        Value::Object(o) => o.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]),
        _ => &[],
    };
    lista
        .iter()
        .filter_map(|v| {
            let label = v["label"].as_str()?.to_string();
            let snippet = v["insertTextFormat"].as_u64() == Some(2);
            let bruto = v["textEdit"]["newText"].as_str().or_else(|| v["insertText"].as_str()).unwrap_or(&label).to_string();
            let texto = if snippet { sem_snippet(&bruto) } else { bruto };
            let inicio = v["textEdit"]["range"]["start"].as_object().and_then(|s| Some((s.get("line")?.as_u64()? as usize, s.get("character")?.as_u64()? as usize)));
            Some(Item {
                filtro: v["filterText"].as_str().unwrap_or(&label).to_string(),
                detalhe: v["detail"].as_str().or_else(|| v["labelDetails"]["detail"].as_str()).unwrap_or("").to_string(),
                tipo: v["kind"].as_u64().unwrap_or(1) as u8,
                label,
                texto,
                inicio,
            })
        })
        .collect()
}

fn locais(r: &Value) -> Vec<Local> {
    let um = |v: &Value| -> Option<Local> {
        let u = v.get("uri").or_else(|| v.get("targetUri"))?.as_str()?;
        let rg = v.get("range").or_else(|| v.get("targetSelectionRange"))?;
        Some(Local { arquivo: de_uri(u)?, row: rg["start"]["line"].as_u64()? as usize, col: rg["start"]["character"].as_u64()? as usize })
    };
    match r {
        Value::Array(a) => a.iter().filter_map(um).collect(),
        Value::Object(_) => um(r).into_iter().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use std::time::{Duration, Instant};

    /// Testes que mexem em `ASTROGRAPH_LSP_CMD` (env global do processo) não podem rodar juntos.
    pub(crate) static TRAVA_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(crate) const FALSO: &str = r#"
import sys, json
def ler():
    n = 0
    while True:
        l = sys.stdin.buffer.readline()
        if not l: return None
        l = l.strip()
        if not l: break
        if l.lower().startswith(b'content-length:'): n = int(l.split(b':')[1])
    return json.loads(sys.stdin.buffer.read(n))
def enviar(o):
    b = json.dumps(o).encode()
    sys.stdout.buffer.write(b'Content-Length: %d\r\n\r\n' % len(b) + b); sys.stdout.buffer.flush()
while True:
    m = ler()
    if m is None: break
    meth = m.get('method'); i = m.get('id')
    if meth == 'initialize': enviar({'jsonrpc':'2.0','id':i,'result':{'capabilities':{}}})
    elif meth == 'textDocument/didOpen' or meth == 'textDocument/didChange':
        td = m['params']['textDocument']
        enviar({'jsonrpc':'2.0','method':'textDocument/publishDiagnostics','params':{'uri':td['uri'],'diagnostics':[{'range':{'start':{'line':0,'character':1},'end':{'line':0,'character':3}},'severity':2,'message':'cuidado aqui'}]}})
    elif meth == 'textDocument/completion': enviar({'jsonrpc':'2.0','id':i,'result':{'isIncomplete':False,'items':[{'label':'println!','kind':3,'detail':'macro','insertText':'println!("$1")$0','insertTextFormat':2},{'label':'print','kind':3,'detail':'fn print()'},{'label':'private','kind':14},{'label':'zeta','kind':6}]}})
    elif meth == 'textDocument/hover': enviar({'jsonrpc':'2.0','id':i,'result':{'contents':{'kind':'markdown','value':'```rust\nfn x()\n```\ndoc da x'}}})
    elif meth == 'textDocument/definition': enviar({'jsonrpc':'2.0','id':i,'result':[{'uri':'file:///tmp/alvo%20x.rs','range':{'start':{'line':4,'character':2},'end':{'line':4,'character':5}}}]})
"#;

    fn esperar<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
        let fim = Instant::now() + Duration::from_secs(8);
        while Instant::now() < fim {
            if let Some(v) = f() {
                return Some(v);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    #[test]
    fn uris_e_colunas_utf16() {
        assert_eq!(uri(Path::new("/tmp/a b.rs")), "file:///tmp/a%20b.rs");
        assert_eq!(de_uri("file:///tmp/a%20b.rs"), Some(PathBuf::from("/tmp/a b.rs")));
        assert_eq!(de_uri("file:///C:/x/y.rs"), Some(PathBuf::from("C:/x/y.rs")));
        // 😀 vale 2 unidades UTF-16
        assert_eq!(col_utf16("a😀b", 2), 3);
        assert_eq!(col_chars("a😀b", 3), 2);
        assert_eq!(col_chars("abc", 99), 3);
    }

    #[test]
    fn servidor_falso_diagnostica_hover_e_definicao() {
        let Some(py) = ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok()) else { return };
        let dir = std::env::temp_dir().join(format!("astrograph-lsp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("falso.py");
        std::fs::write(&script, FALSO).unwrap();
        let mut c = Cliente::iniciar(py, &[script.to_string_lossy().into_owned()], &dir, Arc::new(|| {})).unwrap();
        let arq = dir.join("a.py");
        // didOpen antes do `initialize` terminar: fica na fila e sai depois
        c.abrir(&arq, "python", "x = 1\n");
        let d = esperar(|| Some(c.diagnosticos(&arq)).filter(|d| !d.is_empty())).expect("diagnóstico");
        assert_eq!((d[0].row, d[0].col, d[0].end_col, d[0].severity, d[0].message.as_str()), (0, 1, 3, 2, "cuidado aqui"));
        let id = c.hover(&arq, 0, 1);
        let h = esperar(|| c.estado.lock().unwrap().hover.clone().filter(|(i, _)| *i == id)).expect("hover");
        assert_eq!(h.1, "fn x()\ndoc da x");
        let id = c.definicao(&arq, 0, 1);
        let l = esperar(|| c.estado.lock().unwrap().definicao.clone().filter(|(i, _)| *i == id)).expect("definição");
        assert_eq!(l.1, vec![Local { arquivo: PathBuf::from("/tmp/alvo x.rs"), row: 4, col: 2 }]);
        let id = c.completar(&arq, 0, 1);
        let it = esperar(|| c.estado.lock().unwrap().completar.clone().filter(|(i, _)| *i == id)).expect("completar");
        assert_eq!(it.1.len(), 4);
        assert_eq!((it.1[0].label.as_str(), it.1[0].texto.as_str(), it.1[0].tipo), ("println!", "println!(\"\")", 3), "snippet sem $1/$0");
        assert_eq!(it.1[1].texto, "print");
        assert_eq!(sem_snippet("foo(${1:a}, ${2:b})$0"), "foo(a, b)");
        assert_eq!(sem_snippet("a\\$b"), "a$b");
        // mudar o texto reenvia (o falso sempre devolve o mesmo aviso)
        c.mudou(&arq, "x = 2\n");
        assert_eq!(c.versao, 2);
        c.encerrar();
    }

    #[test]
    fn extensao_escolhe_o_servidor_se_existir() {
        assert!(servidor_para(Path::new("a.txt")).is_none());
        if let Some((p, _, l)) = servidor_para(Path::new("a.py")) {
            assert!(!p.is_empty() && l == "python");
        }
    }
}
