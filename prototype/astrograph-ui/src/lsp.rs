//! Cliente LSP nativo (JSON-RPC sobre stdio): diagnósticos, hover e ir para a
//! definição. Usa os servidores que já existem na máquina (PATH ou o Mason do
//! Neovim); sem servidor para a linguagem, simplesmente não faz nada.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::proc::SemJanela;
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
    /// paradas do snippet (`$1`, `${2:nome}`...) em ordem de uso: (início, fim) em caracteres do `texto`
    pub paradas: Vec<(usize, usize)>,
    /// `additionalTextEdits` (imports automáticos): faixas (linha, coluna UTF-16) e texto
    pub extras: Vec<((usize, usize), (usize, usize), String)>,
}

/// Lê a sintaxe de snippet do LSP: devolve o texto (com os valores padrão) e as paradas em ordem
/// de uso (`$1`, `$2`... e por último `$0`), como (início, fim) em caracteres do texto.
pub fn snippet(t: &str) -> (String, Vec<(usize, usize)>) {
    fn rec(c: &[char], out: &mut String, paradas: &mut Vec<(u32, usize, usize)>) {
        let mut i = 0;
        while i < c.len() {
            let ch = c[i];
            if ch == '\\' && i + 1 < c.len() {
                out.push(c[i + 1]);
                i += 2;
            } else if ch == '$' && i + 1 < c.len() && c[i + 1].is_ascii_digit() {
                let mut j = i + 1;
                let mut n = 0u32;
                while j < c.len() && c[j].is_ascii_digit() {
                    n = n * 10 + c[j].to_digit(10).unwrap();
                    j += 1;
                }
                let at = out.chars().count();
                paradas.push((n, at, at));
                i = j;
            } else if ch == '$' && i + 1 < c.len() && c[i + 1] == '{' {
                // ${1}  ${1:padrão}  ${1|a,b|}  ${VAR:padrão}
                let mut j = i + 2;
                let mut n: Option<u32> = None;
                while j < c.len() && c[j].is_ascii_digit() {
                    n = Some(n.unwrap_or(0) * 10 + c[j].to_digit(10).unwrap());
                    j += 1;
                }
                let mut depth = 1;
                let mut k = j;
                while k < c.len() && depth > 0 {
                    match c[k] {
                        '\\' => k += 1,
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    k += 1;
                }
                let fim = if depth == 0 { k - 1 } else { k.min(c.len()) };
                let corpo: &[char] = &c[j.min(fim)..fim];
                let at = out.chars().count();
                if corpo.first() == Some(&':') {
                    rec(&corpo[1..], out, paradas);
                } else if corpo.first() == Some(&'|') {
                    // escolha: vale a primeira opção
                    let opts: String = corpo[1..].iter().take_while(|&&x| x != '|' && x != ',').collect();
                    out.push_str(&opts);
                }
                let fim_txt = out.chars().count();
                if let Some(n) = n {
                    paradas.push((n, at, fim_txt));
                }
                i = k;
            } else {
                out.push(ch);
                i += 1;
            }
        }
    }
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut ps: Vec<(u32, usize, usize)> = Vec::new();
    rec(&c, &mut out, &mut ps);
    // `$0` é a última parada; as outras seguem a numeração (a mesma número repete: fica a primeira)
    ps.sort_by_key(|p| (p.0 == 0, p.0));
    ps.dedup_by_key(|p| p.0);
    (out, ps.into_iter().map(|(_, a, b)| (a, b)).collect())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tipo {
    Init,
    Hover,
    Definicao,
    Completar,
    /// resposta crua guardada em `Estado::respostas` (renomear, ações, formatar, assinatura...)
    Generico,
}

/// Um trabalho do servidor (`$/progress`): título, mensagem e porcentagem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progresso {
    pub titulo: String,
    pub mensagem: String,
    pub pct: Option<u64>,
}

impl Progresso {
    /// "Indexando - src 40%"
    pub fn texto(&self) -> String {
        let mut t = self.titulo.clone();
        if !self.mensagem.is_empty() {
            if !t.is_empty() {
                t.push_str(" - ");
            }
            t.push_str(&self.mensagem);
        }
        if let Some(p) = self.pct {
            t.push_str(&format!(" {p}%"));
        }
        t
    }
}

/// O que o servidor já contou (lido pela UI).
#[derive(Default)]
pub struct Estado {
    pub diagnosticos: HashMap<String, Vec<Diag>>,
    pub hover: Option<(i64, String)>,
    pub definicao: Option<(i64, Vec<Local>)>,
    pub completar: Option<(i64, Vec<Item>)>,
    /// respostas dos pedidos genéricos, por id (`null` e erro chegam como `Value::Null`)
    pub respostas: HashMap<i64, Value>,
    /// `workspace/applyEdit` pedidos pelo servidor (ações que rodam comandos)
    pub edicoes_externas: Vec<Value>,
    /// `window/showMessage`: (tipo 1 erro..4 log, texto)
    pub mensagens: Vec<(u8, String)>,
    /// trabalhos em andamento (`$/progress`): token -> texto ("Indexando 40%")
    pub progresso: HashMap<String, Progresso>,
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
    /// o servidor aceita `didChange` só com o trecho que mudou (`textDocumentSync` = 2)
    incremental: Arc<AtomicBool>,
    /// por arquivo: quantos painéis o têm aberto, a versão enviada e o hash do último texto
    docs: HashMap<PathBuf, Doc>,
}

#[derive(Default)]
struct Doc {
    abertos: usize,
    versao: i64,
    hash: u64,
    /// último texto enviado (base do `didChange` incremental)
    texto: String,
}

/// Trecho que mudou entre `velho` e `novo` como um único `contentChange` (faixa em UTF-16 + texto novo).
pub fn mudanca_incremental(velho: &str, novo: &str) -> Value {
    let (vb, nb) = (velho.as_bytes(), novo.as_bytes());
    let mut p = vb.iter().zip(nb).take_while(|(a, b)| a == b).count();
    while p > 0 && !(velho.is_char_boundary(p) && novo.is_char_boundary(p)) {
        p -= 1;
    }
    let max_s = vb.len().min(nb.len()) - p;
    let mut s = vb.iter().rev().zip(nb.iter().rev()).take(max_s).take_while(|(a, b)| a == b).count();
    while s > 0 && !(velho.is_char_boundary(vb.len() - s) && novo.is_char_boundary(nb.len() - s)) {
        s -= 1;
    }
    let pos = |t: &str, upto: usize| -> (usize, usize) {
        let antes = &t[..upto];
        let linha = antes.matches('\n').count();
        let ini = antes.rfind('\n').map_or(0, |i| i + 1);
        (linha, antes[ini..].encode_utf16().count())
    };
    let (l0, c0) = pos(velho, p);
    let (l1, c1) = pos(velho, vb.len() - s);
    json!({"range": {"start": {"line": l0, "character": c0}, "end": {"line": l1, "character": c1}}, "text": &novo[p..nb.len() - s]})
}

fn hash_texto(t: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// `(programa, argumentos, languageId)` para a extensão do arquivo.
pub fn servidor_para(arquivo: &Path) -> Option<(String, Vec<String>, &'static str)> {
    // ASTROGRAPH_LSP_CMD="programa args...": força um servidor (testes e servidores personalizados)
    if let Ok(cmd) = std::env::var("ASTROGRAPH_LSP_CMD") {
        let mut it = cmd.split_whitespace().map(str::to_string);
        let prog = it.next()?;
        return Some((prog, it.collect(), "plaintext"));
    }
    servidor_em(arquivo, &dirs_de_servidores(), cfg!(windows))
}

/// `servidor_para` com as pastas e o sistema escolhidos (para testar o Windows em qualquer lugar).
fn servidor_em(arquivo: &Path, dirs: &[PathBuf], windows: bool) -> Option<(String, Vec<String>, &'static str)> {
    let ext = arquivo.extension()?.to_string_lossy().to_lowercase();
    let (cmds, lang): (&[(&str, &[&str])], &'static str) = match ext.as_str() {
        "rs" => (&[("rust-analyzer", &[])], "rust"),
        "py" | "pyi" => (&[("pyright-langserver", &["--stdio"]), ("basedpyright-langserver", &["--stdio"]), ("pylsp", &[])], "python"),
        "ts" | "mts" | "cts" => (&[("typescript-language-server", &["--stdio"])], "typescript"),
        "tsx" => (&[("typescript-language-server", &["--stdio"])], "typescriptreact"),
        "js" | "mjs" | "cjs" => (&[("typescript-language-server", &["--stdio"])], "javascript"),
        "jsx" => (&[("typescript-language-server", &["--stdio"])], "javascriptreact"),
        "go" => (&[("gopls", &[])], "go"),
        "lua" => (&[("lua-language-server", &[])], "lua"),
        "c" | "h" => (&[("clangd", &[])], "c"),
        "cpp" | "cc" | "hpp" => (&[("clangd", &[])], "cpp"),
        "sh" | "bash" => (&[("bash-language-server", &["start"])], "shellscript"),
        "json" | "jsonc" => (&[("vscode-json-language-server", &["--stdio"])], "json"),
        "html" | "htm" => (&[("vscode-html-language-server", &["--stdio"])], "html"),
        "css" | "scss" => (&[("vscode-css-language-server", &["--stdio"])], "css"),
        "yaml" | "yml" => (&[("yaml-language-server", &["--stdio"])], "yaml"),
        "toml" => (&[("taplo", &["lsp", "stdio"])], "toml"),
        "md" | "markdown" => (&[("marksman", &["server"])], "markdown"),
        "zig" => (&[("zls", &[])], "zig"),
        "java" => (&[("jdtls", &[])], "java"),
        "rb" => (&[("ruby-lsp", &[]), ("solargraph", &["stdio"])], "ruby"),
        "php" => (&[("intelephense", &["--stdio"])], "php"),
        "cs" => (&[("csharp-ls", &[])], "csharp"),
        "kt" | "kts" => (&[("kotlin-language-server", &[])], "kotlin"),
        "swift" => (&[("sourcekit-lsp", &[])], "swift"),
        "dart" => (&[("dart", &["language-server"])], "dart"),
        "ex" | "exs" => (&[("elixir-ls", &[])], "elixir"),
        _ => return None,
    };
    cmds.iter().find_map(|(nome, args)| crate::term::achar_programa(nome, dirs, windows).map(|p| (p.to_string_lossy().into_owned(), args.iter().map(|a| a.to_string()).collect(), lang)))
}

/// Procura no PATH, na pasta de binários do Mason (a que o Neovim já usa) e na do npm global. No Windows
/// só vale `.exe`/`.cmd`/`.bat`: o script sem extensão que o npm deixa ao lado do `.cmd` não executa (erro 193),
/// e era ele que o servidor de TypeScript (instalado pelo npm ou pelo Mason) acabava usando, sem subir.
fn achar(nome: &str) -> Option<PathBuf> {
    crate::term::achar_programa(nome, &dirs_de_servidores(), cfg!(windows))
}

fn dirs_de_servidores() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("nvim-data/mason/bin"));
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(a).join("npm"));
    }
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(&h).join(".local/share/nvim/mason/bin"));
        dirs.push(PathBuf::from(&h).join(".npm-global/bin"));
    }
    dirs
}

/// O que falta para ter servidor de linguagem num tipo de arquivo e como instalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instalacao {
    /// nome mostrado ("TypeScript/JavaScript")
    pub nome: &'static str,
    /// programa que precisa existir para o comando rodar (npm, rustup, go)
    pub requer: Option<&'static str>,
    /// comando de shell que instala; `None` quando não dá para automatizar (só o texto)
    pub comando: Option<&'static str>,
    /// como instalar, em texto
    pub como: &'static str,
}

/// O servidor que falta para `arquivo`. Só para tipos que o app sabe atender.
pub fn instalacao_para(arquivo: &Path) -> Option<Instalacao> {
    let ext = arquivo.extension()?.to_string_lossy().to_lowercase();
    let auto = |nome, requer, comando| Instalacao { nome, requer: Some(requer), comando: Some(comando), como: comando };
    Some(match ext.as_str() {
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => auto("TypeScript/JavaScript", "npm", "npm i -g typescript typescript-language-server"),
        "rs" => auto("Rust", "rustup", "rustup component add rust-analyzer"),
        "py" | "pyi" => auto("Python", "npm", "npm i -g pyright"),
        "go" => auto("Go", "go", "go install golang.org/x/tools/gopls@latest"),
        "lua" => Instalacao { nome: "Lua", requer: None, comando: None, como: "instale o lua-language-server (ou use o Mason)" },
        "c" | "h" | "cpp" | "cc" | "hpp" => Instalacao { nome: "C/C++", requer: None, comando: None, como: "instale o clangd" },
        _ => return None,
    })
}

/// Os servidores mais comuns e se estão instalados (PATH ou Mason): para o `:Astrograph saude`.
pub fn servidores_instalados() -> Vec<(&'static str, bool)> {
    ["rust-analyzer", "pyright-langserver", "typescript-language-server", "gopls", "lua-language-server", "clangd", "bash-language-server", "vscode-json-language-server", "yaml-language-server", "taplo", "marksman", "zls"]
        .into_iter()
        .map(|n| (n, achar(n).is_some()))
        .collect()
}

/// Raiz do projeto de `arquivo` para o servidor: a raiz do git; sem git, a pasta mais perto com um
/// marcador de projeto; senão `padrao`.
pub fn raiz_do_projeto(arquivo: &Path, padrao: &Path) -> PathBuf {
    let dir = arquivo.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(padrao);
    if let Some(r) = crate::git::raiz(dir) {
        return r;
    }
    const MARCADORES: &[&str] = &["Cargo.toml", "package.json", "go.mod", "pyproject.toml", "setup.py", "tsconfig.json", ".luarc.json", "compile_commands.json", "Makefile"];
    let mut d = Some(dir);
    while let Some(p) = d {
        if MARCADORES.iter().any(|m| p.join(m).exists()) {
            return p.to_path_buf();
        }
        d = p.parent();
    }
    padrao.to_path_buf()
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
        let mut child = Command::new(prog).args(args).current_dir(raiz).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).sem_janela().spawn().map_err(|e| e.to_string())?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("sem stdin")?));
        let stdout = child.stdout.take().ok_or("sem stdout")?;
        let estado = Arc::new(Mutex::new(Estado::default()));
        let kinds: Arc<Mutex<HashMap<i64, Tipo>>> = Arc::new(Mutex::new(HashMap::new()));
        let fila: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let pronto = Arc::new(AtomicBool::new(false));
        let incremental = Arc::new(AtomicBool::new(false));
        {
            let (stdin, estado, kinds, fila, pronto, incremental) = (stdin.clone(), estado.clone(), kinds.clone(), fila.clone(), pronto.clone(), incremental.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(stdout);
                while let Some(msg) = ler(&mut r) {
                    let id = msg.get("id").and_then(Value::as_i64);
                    if msg.get("method").is_some() && id.is_some() {
                        // pedido do servidor: `applyEdit` vira edição pendente; o resto (registro de
                        // capacidades, configuração...) é respondido vazio
                        let resultado = if msg["method"] == "workspace/applyEdit" {
                            estado.lock().unwrap().edicoes_externas.push(msg["params"]["edit"].clone());
                            notify();
                            json!({"applied": true})
                        } else {
                            Value::Null
                        };
                        let resp = json!({"jsonrpc": "2.0", "id": msg["id"], "result": resultado});
                        let _ = escrever(&mut *stdin.lock().unwrap(), &resp);
                        continue;
                    }
                    if let Some(m) = msg.get("method").and_then(Value::as_str) {
                        if m == "window/showMessage" {
                            let p = &msg["params"];
                            if let Some(t) = p["message"].as_str() {
                                estado.lock().unwrap().mensagens.push((p["type"].as_u64().unwrap_or(3) as u8, t.to_string()));
                                notify();
                            }
                        }
                        if m == "$/progress" {
                            let p = &msg["params"];
                            let token = p["token"].as_str().map(str::to_string).or_else(|| p["token"].as_i64().map(|n| n.to_string())).unwrap_or_default();
                            let v = &p["value"];
                            match v["kind"].as_str() {
                                Some("end") => {
                                    estado.lock().unwrap().progresso.remove(&token);
                                }
                                Some(k) => {
                                    let mut e = estado.lock().unwrap();
                                    let pr = e.progresso.entry(token).or_default();
                                    if k == "begin" || !v["title"].is_null() {
                                        pr.titulo = v["title"].as_str().unwrap_or("").to_string();
                                    }
                                    if let Some(m) = v["message"].as_str() {
                                        pr.mensagem = m.to_string();
                                    }
                                    if let Some(p) = v["percentage"].as_u64() {
                                        pr.pct = Some(p);
                                    }
                                }
                                None => {}
                            }
                            notify();
                        }
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
                            let sync = &msg["result"]["capabilities"]["textDocumentSync"];
                            let modo = sync.as_i64().or_else(|| sync["change"].as_i64()).unwrap_or(1);
                            incremental.store(modo == 2, Ordering::Release);
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
                        Some(Tipo::Generico) => {
                            let r = if msg.get("error").is_some() { Value::Null } else { msg["result"].clone() };
                            estado.lock().unwrap().respostas.insert(id, r);
                            notify();
                        }
                        None => {}
                    }
                }
            });
        }
        let c = Cliente { nome: prog.rsplit(['/', '\\']).next().unwrap_or(prog).to_string(), child, stdin, next: AtomicI64::new(1), kinds, estado, fila, pronto, incremental, docs: HashMap::new() };
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
                    "completion": {"completionItem": {"snippetSupport": false, "documentationFormat": ["plaintext"]}},
                    "rename": {"prepareSupport": false},
                    "codeAction": {"codeActionLiteralSupport": {"codeActionKind": {"valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"]}}, "resolveSupport": {"properties": ["edit"]}},
                    "formatting": {},
                    "rangeFormatting": {},
                    "signatureHelp": {"signatureInformation": {"documentationFormat": ["plaintext"], "parameterInformation": {"labelOffsetSupport": true}}},
                    "references": {},
                    "implementation": {"linkSupport": false},
                    "typeDefinition": {"linkSupport": false},
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true}
                },
                "window": {"showMessage": {}},
                "workspace": {"workspaceFolders": true, "applyEdit": true, "workspaceEdit": {"documentChanges": true}}
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

    /// Abre o arquivo no servidor. Dois painéis com o mesmo arquivo abrem uma vez só (contagem).
    pub fn abrir(&mut self, arquivo: &Path, linguagem: &str, texto: &str) {
        let d = self.docs.entry(arquivo.to_path_buf()).or_default();
        d.abertos += 1;
        if d.abertos > 1 {
            return;
        }
        d.versao = 1;
        d.hash = hash_texto(texto);
        d.texto = texto.to_string();
        let versao = d.versao;
        self.notificacao("textDocument/didOpen", json!({"textDocument": {"uri": uri(arquivo), "languageId": linguagem, "version": versao, "text": texto}}));
    }

    /// Texto novo; igual ao último enviado (o outro painel já mandou) não manda nada.
    pub fn mudou(&mut self, arquivo: &Path, texto: &str) {
        let h = hash_texto(texto);
        let Some(d) = self.docs.get_mut(arquivo) else { return };
        if d.hash == h {
            return;
        }
        d.hash = h;
        d.versao += 1;
        let versao = d.versao;
        let mudanca = if self.incremental.load(Ordering::Acquire) { mudanca_incremental(&d.texto, texto) } else { json!({"text": texto}) };
        d.texto = texto.to_string();
        self.notificacao("textDocument/didChange", json!({"textDocument": {"uri": uri(arquivo), "version": versao}, "contentChanges": [mudanca]}));
    }

    /// Versão do arquivo no servidor (0 = não aberto).
    pub fn versao(&self, arquivo: &Path) -> i64 {
        self.docs.get(arquivo).map_or(0, |d| d.versao)
    }

    /// Fecha só quando o último painel que o tinha aberto fecha.
    pub fn fechar(&mut self, arquivo: &Path) {
        let Some(d) = self.docs.get_mut(arquivo) else { return };
        d.abertos = d.abertos.saturating_sub(1);
        if d.abertos > 0 {
            return;
        }
        self.docs.remove(arquivo);
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

    /// Pedido genérico: a resposta crua aparece em `resposta(id)`.
    pub fn requisitar(&self, metodo: &str, params: Value) -> i64 {
        self.pedido(metodo, params, Tipo::Generico)
    }

    /// Pega (e tira) a resposta de um pedido genérico, se já chegou.
    pub fn resposta(&self, id: i64) -> Option<Value> {
        self.estado.lock().unwrap().respostas.remove(&id)
    }

    fn pos(arquivo: &Path, row: usize, col16: usize) -> Value {
        json!({"textDocument": {"uri": uri(arquivo)}, "position": {"line": row, "character": col16}})
    }

    pub fn renomear(&self, arquivo: &Path, row: usize, col16: usize, novo: &str) -> i64 {
        let mut p = Self::pos(arquivo, row, col16);
        p["newName"] = json!(novo);
        self.requisitar("textDocument/rename", p)
    }

    /// Ações de código para o trecho `(linha, coluna UTF-16)` início–fim, com os diagnósticos dele.
    pub fn acoes(&self, arquivo: &Path, ini: (usize, usize), fim: (usize, usize), diags: Vec<Value>) -> i64 {
        self.requisitar(
            "textDocument/codeAction",
            json!({
                "textDocument": {"uri": uri(arquivo)},
                "range": {"start": {"line": ini.0, "character": ini.1}, "end": {"line": fim.0, "character": fim.1}},
                "context": {"diagnostics": diags}
            }),
        )
    }

    pub fn formatar(&self, arquivo: &Path, tab: usize, espacos: bool) -> i64 {
        self.requisitar("textDocument/formatting", json!({"textDocument": {"uri": uri(arquivo)}, "options": {"tabSize": tab, "insertSpaces": espacos}}))
    }

    pub fn formatar_trecho(&self, arquivo: &Path, ini: (usize, usize), fim: (usize, usize), tab: usize, espacos: bool) -> i64 {
        self.requisitar(
            "textDocument/rangeFormatting",
            json!({
                "textDocument": {"uri": uri(arquivo)},
                "range": {"start": {"line": ini.0, "character": ini.1}, "end": {"line": fim.0, "character": fim.1}},
                "options": {"tabSize": tab, "insertSpaces": espacos}
            }),
        )
    }

    pub fn assinatura(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        self.requisitar("textDocument/signatureHelp", Self::pos(arquivo, row, col16))
    }

    pub fn referencias(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        let mut p = Self::pos(arquivo, row, col16);
        p["context"] = json!({"includeDeclaration": true});
        self.requisitar("textDocument/references", p)
    }

    pub fn implementacao(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        self.requisitar("textDocument/implementation", Self::pos(arquivo, row, col16))
    }

    pub fn tipo_da_definicao(&self, arquivo: &Path, row: usize, col16: usize) -> i64 {
        self.requisitar("textDocument/typeDefinition", Self::pos(arquivo, row, col16))
    }

    pub fn simbolos(&self, arquivo: &Path) -> i64 {
        self.requisitar("textDocument/documentSymbol", json!({"textDocument": {"uri": uri(arquivo)}}))
    }

    /// `workspace/symbol`: símbolos de todo o projeto que casam com `consulta`.
    pub fn simbolos_do_projeto(&self, consulta: &str) -> i64 {
        self.requisitar("workspace/symbol", json!({"query": consulta}))
    }

    pub fn executar(&self, comando: &str, args: Vec<Value>) -> i64 {
        self.requisitar("workspace/executeCommand", json!({"command": comando, "arguments": args}))
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
            let (texto, paradas) = if snippet { self::snippet(&bruto) } else { (bruto, Vec::new()) };
            let extras = v["additionalTextEdits"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| {
                            let g = |p: &Value, k: &str| p[k].as_u64().map(|n| n as usize);
                            let (r, n) = (&e["range"], e["newText"].as_str()?.to_string());
                            Some(((g(&r["start"], "line")?, g(&r["start"], "character")?), (g(&r["end"], "line")?, g(&r["end"], "character")?), n))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let inicio = v["textEdit"]["range"]["start"].as_object().and_then(|s| Some((s.get("line")?.as_u64()? as usize, s.get("character")?.as_u64()? as usize)));
            Some(Item {
                filtro: v["filterText"].as_str().unwrap_or(&label).to_string(),
                detalhe: v["detail"].as_str().or_else(|| v["labelDetails"]["detail"].as_str()).unwrap_or("").to_string(),
                tipo: v["kind"].as_u64().unwrap_or(1) as u8,
                label,
                texto,
                inicio,
                paradas,
                extras,
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

/// Uma troca de texto do servidor: posições (linha, coluna UTF-16) do texto de antes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edicao {
    pub arquivo: PathBuf,
    pub ini: (usize, usize),
    pub fim: (usize, usize),
    pub novo: String,
}

fn edicao_de(arquivo: &Path, v: &Value) -> Option<Edicao> {
    let r = v.get("range")?;
    Some(Edicao {
        arquivo: arquivo.to_path_buf(),
        ini: (r["start"]["line"].as_u64()? as usize, r["start"]["character"].as_u64()? as usize),
        fim: (r["end"]["line"].as_u64()? as usize, r["end"]["character"].as_u64()? as usize),
        novo: v["newText"].as_str()?.to_string(),
    })
}

/// `[TextEdit]` de um arquivo só (formatação).
pub fn edicoes_do_arquivo(arquivo: &Path, r: &Value) -> Vec<Edicao> {
    r.as_array().map(|a| a.iter().filter_map(|v| edicao_de(arquivo, v)).collect()).unwrap_or_default()
}

/// `WorkspaceEdit` (`changes` ou `documentChanges`): as trocas de texto de todos os arquivos.
/// Criar, renomear e apagar arquivos não são tratados.
pub fn edicoes_de(r: &Value) -> Vec<Edicao> {
    let mut out = Vec::new();
    if let Some(ch) = r["changes"].as_object() {
        for (u, lista) in ch {
            if let (Some(p), Some(a)) = (de_uri(u), lista.as_array()) {
                out.extend(a.iter().filter_map(|v| edicao_de(&p, v)));
            }
        }
    }
    if let Some(dc) = r["documentChanges"].as_array() {
        for d in dc {
            let (Some(u), Some(a)) = (d["textDocument"]["uri"].as_str(), d["edits"].as_array()) else { continue };
            if let Some(p) = de_uri(u) {
                out.extend(a.iter().filter_map(|v| edicao_de(&p, v)));
            }
        }
    }
    out
}

/// Uma ação de código: título e o que fazer (edição, comando ou os dois).
#[derive(Clone, Debug, PartialEq)]
pub struct Acao {
    pub titulo: String,
    pub tipo: String,
    pub edicao: Option<Value>,
    pub comando: Option<(String, Vec<Value>)>,
}

pub fn acoes_de(r: &Value) -> Vec<Acao> {
    let Some(a) = r.as_array() else { return Vec::new() };
    a.iter()
        .filter_map(|v| {
            let titulo = v["title"].as_str()?.to_string();
            // `Command` puro: `command` é o nome e `arguments` ficam no mesmo nível
            let comando = match &v["command"] {
                Value::String(c) => Some((c.clone(), v["arguments"].as_array().cloned().unwrap_or_default())),
                Value::Object(o) => o.get("command").and_then(Value::as_str).map(|c| (c.to_string(), o.get("arguments").and_then(Value::as_array).cloned().unwrap_or_default())),
                _ => None,
            };
            let edicao = v.get("edit").filter(|e| !e.is_null()).cloned();
            Some(Acao { titulo, tipo: v["kind"].as_str().unwrap_or("").to_string(), edicao, comando })
        })
        .collect()
}

/// Um símbolo do documento (função, struct...), em ordem de leitura; `nivel` é o aninhamento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simbolo {
    /// arquivo do símbolo (só nas respostas de `workspace/symbol`)
    pub arquivo: Option<PathBuf>,
    pub nome: String,
    pub tipo: u8,
    pub detalhe: String,
    pub row: usize,
    pub col: usize,
    pub nivel: usize,
}

pub fn simbolos_de(r: &Value) -> Vec<Simbolo> {
    fn rec(v: &Value, nivel: usize, out: &mut Vec<Simbolo>) {
        let Some(nome) = v["name"].as_str() else { return };
        // DocumentSymbol: selectionRange; SymbolInformation: location.range
        let rg = v.get("selectionRange").or_else(|| v.get("range")).or_else(|| v["location"].get("range"));
        if let Some(rg) = rg {
            if let (Some(row), Some(col)) = (rg["start"]["line"].as_u64(), rg["start"]["character"].as_u64()) {
                out.push(Simbolo { arquivo: v["location"]["uri"].as_str().and_then(de_uri), nome: nome.to_string(), tipo: v["kind"].as_u64().unwrap_or(0) as u8, detalhe: v["detail"].as_str().unwrap_or("").to_string(), row: row as usize, col: col as usize, nivel });
            }
        }
        if let Some(f) = v["children"].as_array() {
            for c in f {
                rec(c, nivel + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(a) = r.as_array() {
        for v in a {
            rec(v, 0, &mut out);
        }
    }
    out
}

/// A assinatura da função em edição: o texto, o trecho do parâmetro ativo (em caracteres) e a doc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assinatura {
    pub texto: String,
    pub ativo: Option<(usize, usize)>,
    pub doc: String,
}

pub fn assinatura_de(r: &Value) -> Option<Assinatura> {
    let sigs = r["signatures"].as_array()?;
    let i = r["activeSignature"].as_u64().unwrap_or(0) as usize;
    let s = sigs.get(i).or_else(|| sigs.first())?;
    let texto = s["label"].as_str()?.to_string();
    let p = s["activeParameter"].as_u64().or_else(|| r["activeParameter"].as_u64()).unwrap_or(0) as usize;
    let ativo = s["parameters"].as_array().and_then(|ps| ps.get(p)).and_then(|par| match &par["label"] {
        Value::Array(o) => Some((o.first()?.as_u64()? as usize, o.get(1)?.as_u64()? as usize)),
        Value::String(t) => texto.find(t.as_str()).map(|b| (b, b + t.len())),
        _ => None,
    });
    // posições do servidor são UTF-16 (ou bytes, na busca por texto): converte para caracteres
    let ativo = ativo.map(|(a, b)| (col_chars(&texto, a), col_chars(&texto, b)));
    let doc = match &s["documentation"] {
        Value::String(t) => t.clone(),
        Value::Object(o) => o.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
        _ => String::new(),
    };
    Some(Assinatura { texto, ativo, doc: doc.lines().next().unwrap_or("").to_string() })
}

pub fn locais_de(r: &Value) -> Vec<Local> {
    locais(r)
}

#[cfg(test)]
pub(crate) mod testes {
    #[test]
    fn snippet_com_paradas() {
        let (t, p) = super::snippet("foo(${1:a}, ${2:b})$0");
        assert_eq!(t, "foo(a, b)");
        assert_eq!(p, vec![(4, 5), (7, 8), (9, 9)]);
        // $2 antes de $1 no texto: a ordem de uso é a numeração
        let (t, p) = super::snippet("for $2 in ${1:items} {\n\t$0\n}");
        assert_eq!(t, "for  in items {\n\t\n}");
        assert_eq!(p[0], (8, 13));
        assert_eq!(p[1], (4, 4));
        assert_eq!(p.last().unwrap(), &(17, 17));
        // escolha, aninhado e escape
        assert_eq!(super::snippet("${1|um,dois|}").0, "um");
        assert_eq!(super::snippet("${1:f(${2:x})}\\$").0, "f(x)$");
        assert_eq!(super::snippet("a").1, vec![]);
    }

    #[test]
    fn mudanca_incremental_acha_o_trecho_em_utf16() {
        use super::mudanca_incremental as m;
        let r = m("fn a() {}\nlet x = 1;\n", "fn a() {}\nlet xy = 1;\n");
        assert_eq!(r["range"]["start"], serde_json::json!({"line": 1, "character": 5}));
        assert_eq!(r["range"]["end"], serde_json::json!({"line": 1, "character": 5}));
        assert_eq!(r["text"], "y");
        // apagar uma linha inteira
        let r = m("a\nb\nc\n", "a\nc\n");
        assert_eq!((r["range"]["start"]["line"].as_u64(), r["range"]["end"]["line"].as_u64()), (Some(1), Some(2)));
        assert_eq!(r["text"], "");
        // emoji conta 2 em UTF-16 e o corte respeita o limite do caractere
        let r = m("😀a\n", "😀ba\n");
        assert_eq!(r["range"]["start"]["character"], 2);
        assert_eq!(r["text"], "b");
        // texto igual não quebra
        let r = m("x", "x");
        assert_eq!(r["text"], "");
    }

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
ultimo = ''
while True:
    m = ler()
    if m is None: break
    meth = m.get('method'); i = m.get('id')
    if meth == 'textDocument/didOpen': ultimo = m['params']['textDocument']['uri']
    if meth == 'initialize': enviar({'jsonrpc':'2.0','id':i,'result':{'capabilities':{}}})
    if meth == 'initialized':
        enviar({'jsonrpc':'2.0','method':'$/progress','params':{'token':'t1','value':{'kind':'begin','title':'Indexando','percentage':10}}})
        enviar({'jsonrpc':'2.0','method':'$/progress','params':{'token':'t1','value':{'kind':'report','message':'src','percentage':40}}})
    elif meth == 'textDocument/didOpen' or meth == 'textDocument/didChange':
        td = m['params']['textDocument']
        enviar({'jsonrpc':'2.0','method':'textDocument/publishDiagnostics','params':{'uri':td['uri'],'diagnostics':[{'range':{'start':{'line':0,'character':1},'end':{'line':0,'character':3}},'severity':2,'message':'cuidado aqui'}]}})
    elif meth == 'workspace/symbol':
        enviar({'jsonrpc':'2.0','id':i,'result':[{'name':'Foo','kind':5,'location':{'uri':'file:///tmp/sym%20a.py','range':{'start':{'line':2,'character':1},'end':{'line':2,'character':4}}}}]})
    elif meth == 'textDocument/completion': enviar({'jsonrpc':'2.0','id':i,'result':{'isIncomplete':False,'items':[{'label':'println!','kind':3,'detail':'macro','insertText':'println!("$1")$0','insertTextFormat':2},{'label':'print','kind':3,'detail':'fn print()'},{'label':'private','kind':14},{'label':'zeta','kind':6}]}})
    elif meth == 'textDocument/hover': enviar({'jsonrpc':'2.0','id':i,'result':{'contents':{'kind':'markdown','value':'```rust\nfn x()\n```\ndoc da x'}}})
    elif meth == 'textDocument/rename':
        u = m['params']['textDocument']['uri']; n = m['params']['newName']
        enviar({'jsonrpc':'2.0','id':i,'result':{'changes':{u:[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':1}},'newText':n}],'file:///tmp/outro%20arq.py':[{'range':{'start':{'line':1,'character':2},'end':{'line':1,'character':3}},'newText':n}]}}})
    elif meth == 'textDocument/codeAction':
        u = m['params']['textDocument']['uri']
        enviar({'jsonrpc':'2.0','id':i,'result':[{'title':'Trocar x por y','kind':'quickfix','edit':{'documentChanges':[{'textDocument':{'uri':u,'version':1},'edits':[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':1}},'newText':'y'}]}]}},{'title':'Rodar comando','command':{'title':'x','command':'falso.cmd','arguments':[7]}}]})
    elif meth == 'workspace/executeCommand':
        enviar({'jsonrpc':'2.0','id':9001,'method':'workspace/applyEdit','params':{'edit':{'changes':{ultimo:[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':1}},'newText':'z'}]}}}})
        enviar({'jsonrpc':'2.0','id':i,'result':None})
    elif meth == 'textDocument/formatting' or meth == 'textDocument/rangeFormatting':
        enviar({'jsonrpc':'2.0','id':i,'result':[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':5}},'newText':'x = 2'}]})
    elif meth == 'textDocument/signatureHelp':
        enviar({'jsonrpc':'2.0','id':i,'result':{'signatures':[{'label':'soma(a: int, b: int)','parameters':[{'label':[5,11]},{'label':[13,19]}],'documentation':'soma dois numeros\nsegunda linha'}],'activeSignature':0,'activeParameter':1}})
    elif meth == 'textDocument/references':
        u = m['params']['textDocument']['uri']
        enviar({'jsonrpc':'2.0','id':i,'result':[{'uri':u,'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':1}}},{'uri':'file:///tmp/ref%20dois.py','range':{'start':{'line':2,'character':4},'end':{'line':2,'character':5}}}]})
    elif meth == 'textDocument/documentSymbol':
        enviar({'jsonrpc':'2.0','id':i,'result':[{'name':'Foo','kind':5,'range':{'start':{'line':3,'character':0},'end':{'line':9,'character':0}},'selectionRange':{'start':{'line':3,'character':6},'end':{'line':3,'character':9}},'children':[{'name':'bar','kind':6,'detail':'fn','range':{'start':{'line':4,'character':4},'end':{'line':5,'character':0}},'selectionRange':{'start':{'line':4,'character':8},'end':{'line':4,'character':11}}}]}]})
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
        // no Windows `/tmp/..` não é absoluto (falta o drive): o caminho absoluto muda por plataforma
        if cfg!(windows) {
            assert_eq!(uri(Path::new("C:\\tmp\\a b.rs")), "file:///C:/tmp/a%20b.rs");
        } else {
            assert_eq!(uri(Path::new("/tmp/a b.rs")), "file:///tmp/a%20b.rs");
        }
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
        assert_eq!(c.versao(&arq), 2);
        // mesmo texto de novo (o outro painel já mandou): sem versão nova
        c.mudou(&arq, "x = 2\n");
        assert_eq!(c.versao(&arq), 2);
        // dois painéis: abre uma vez, fecha quando o último sai
        c.abrir(&arq, "python", "x = 2\n");
        assert_eq!(c.versao(&arq), 2, "a segunda abertura não reinicia");
        c.fechar(&arq);
        assert_eq!(c.versao(&arq), 2, "ainda tem um painel");
        c.fechar(&arq);
        assert_eq!(c.versao(&arq), 0);
        c.encerrar();
    }

    fn cliente_falso() -> (Cliente, PathBuf, PathBuf) {
        let py = ["python3", "python"].into_iter().find(|p| Command::new(p).arg("--version").output().is_ok()).expect("python");
        let dir = std::env::temp_dir().join(format!("astrograph-lsp2-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("falso.py");
        std::fs::write(&script, FALSO).unwrap();
        let mut c = Cliente::iniciar(py, &[script.to_string_lossy().into_owned()], &dir, Arc::new(|| {})).unwrap();
        let arq = dir.join("a.py");
        c.abrir(&arq, "python", "x = 1\n");
        (c, dir, arq)
    }

    fn resposta(c: &Cliente, id: i64) -> Value {
        esperar(|| c.resposta(id)).expect("resposta do servidor")
    }

    #[test]
    fn progresso_e_simbolos_do_projeto() {
        if ["python3", "python"].into_iter().all(|p| Command::new(p).arg("--version").output().is_err()) {
            return;
        }
        let (mut c, _d, _arq) = cliente_falso();
        // `$/progress` do servidor: begin + report (o título do begin fica)
        let p = esperar(|| {
            let st = c.estado.lock().unwrap();
            st.progresso.values().find(|t| t.pct == Some(40)).cloned()
        });
        assert_eq!(p.map(|t| t.texto()).as_deref(), Some("Indexando - src 40%"));
        let id = c.simbolos_do_projeto("Fo");
        let sims = simbolos_de(&resposta(&c, id));
        assert_eq!(sims.len(), 1);
        assert_eq!((sims[0].nome.as_str(), sims[0].row, sims[0].col), ("Foo", 2, 1));
        assert_eq!(sims[0].arquivo, Some(PathBuf::from("/tmp/sym a.py")));
        c.encerrar();
    }

    #[test]
    fn renomear_devolve_edicoes_em_dois_arquivos() {
        if ["python3", "python"].into_iter().all(|p| Command::new(p).arg("--version").output().is_err()) {
            return;
        }
        let (mut c, _d, arq) = cliente_falso();
        let id = c.renomear(&arq, 0, 0, "novo");
        let ed = edicoes_de(&resposta(&c, id));
        assert_eq!(ed.len(), 2);
        let aqui = ed.iter().find(|e| e.arquivo == arq).expect("edição no arquivo aberto");
        assert_eq!((aqui.ini, aqui.fim, aqui.novo.as_str()), ((0, 0), (0, 1), "novo"));
        let outro = ed.iter().find(|e| e.arquivo != arq).unwrap();
        assert_eq!((outro.arquivo.clone(), outro.ini), (PathBuf::from("/tmp/outro arq.py"), (1, 2)));
        c.encerrar();
    }

    #[test]
    fn acoes_de_codigo_edicao_e_comando_com_applyedit_do_servidor() {
        if ["python3", "python"].into_iter().all(|p| Command::new(p).arg("--version").output().is_err()) {
            return;
        }
        let (mut c, _d, arq) = cliente_falso();
        let id = c.acoes(&arq, (0, 0), (0, 1), vec![]);
        let acoes = acoes_de(&resposta(&c, id));
        assert_eq!(acoes.len(), 2);
        assert_eq!((acoes[0].titulo.as_str(), acoes[0].tipo.as_str()), ("Trocar x por y", "quickfix"));
        let ed = edicoes_de(acoes[0].edicao.as_ref().expect("documentChanges"));
        assert_eq!((ed.len(), ed[0].novo.as_str(), ed[0].arquivo.clone()), (1, "y", arq.clone()));
        let (cmd, args) = acoes[1].comando.clone().expect("comando");
        assert_eq!((cmd.as_str(), args.len()), ("falso.cmd", 1));
        // executar o comando: o servidor responde com um `workspace/applyEdit` que fica pendente
        c.executar(&cmd, args);
        let ext = esperar(|| {
            let mut e = c.estado.lock().unwrap();
            (!e.edicoes_externas.is_empty()).then(|| std::mem::take(&mut e.edicoes_externas))
        })
        .expect("applyEdit do servidor");
        let ed = edicoes_de(&ext[0]);
        assert_eq!((ed[0].novo.as_str(), ed[0].arquivo.clone()), ("z", arq));
        c.encerrar();
    }

    #[test]
    fn formatar_assinatura_referencias_e_simbolos() {
        if ["python3", "python"].into_iter().all(|p| Command::new(p).arg("--version").output().is_err()) {
            return;
        }
        let (mut c, _d, arq) = cliente_falso();
        let id = c.formatar(&arq, 4, true);
        let ed = edicoes_do_arquivo(&arq, &resposta(&c, id));
        assert_eq!((ed[0].ini, ed[0].fim, ed[0].novo.as_str()), ((0, 0), (0, 5), "x = 2"));
        let id = c.formatar_trecho(&arq, (0, 0), (0, 5), 4, true);
        assert_eq!(edicoes_do_arquivo(&arq, &resposta(&c, id)).len(), 1);
        // assinatura: o parâmetro ativo é o segundo (`b: int`, caracteres 13..19)
        let id = c.assinatura(&arq, 0, 5);
        let a = assinatura_de(&resposta(&c, id)).expect("assinatura");
        assert_eq!(a.texto, "soma(a: int, b: int)");
        assert_eq!(a.ativo, Some((13, 19)));
        assert_eq!(&a.texto[13..19], "b: int");
        assert_eq!(a.doc, "soma dois numeros", "só a primeira linha da doc");
        let id = c.referencias(&arq, 0, 0);
        let refs = locais_de(&resposta(&c, id));
        assert_eq!(refs.len(), 2);
        assert_eq!((refs[1].arquivo.clone(), refs[1].row, refs[1].col), (PathBuf::from("/tmp/ref dois.py"), 2, 4));
        let id = c.simbolos(&arq);
        let sim = simbolos_de(&resposta(&c, id));
        assert_eq!(sim.iter().map(|s| (s.nome.as_str(), s.nivel, s.row, s.col)).collect::<Vec<_>>(), vec![("Foo", 0, 3, 6), ("bar", 1, 4, 8)]);
        c.encerrar();
    }

    #[test]
    fn parsers_aceitam_as_variantes_do_protocolo() {
        // WorkspaceEdit com `changes`
        let e = edicoes_de(&json!({"changes": {"file:///tmp/a.rs": [{"range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 5}}, "newText": "xyz"}]}}));
        assert_eq!(e, vec![Edicao { arquivo: PathBuf::from("/tmp/a.rs"), ini: (1, 2), fim: (1, 5), novo: "xyz".into() }]);
        // operações de arquivo (criar/renomear) são ignoradas, sem derrubar o resto
        let e = edicoes_de(&json!({"documentChanges": [{"kind": "create", "uri": "file:///tmp/n.rs"}, {"textDocument": {"uri": "file:///tmp/b.rs"}, "edits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "// oi\n"}]}]}));
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].arquivo, PathBuf::from("/tmp/b.rs"));
        assert!(edicoes_de(&Value::Null).is_empty());
        // `Command` puro numa lista de ações
        let a = acoes_de(&json!([{"title": "Organizar", "command": "organizar", "arguments": ["a"]}, {"semtitulo": 1}]));
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].comando, Some(("organizar".to_string(), vec![json!("a")])));
        // SymbolInformation (plano, com location)
        let s = simbolos_de(&json!([{"name": "f", "kind": 12, "location": {"uri": "file:///x", "range": {"start": {"line": 7, "character": 3}, "end": {"line": 8, "character": 0}}}}]));
        assert_eq!((s[0].nome.as_str(), s[0].row, s[0].col), ("f", 7, 3));
        // assinatura com o parâmetro dado como texto e sem activeSignature
        let a = assinatura_de(&json!({"signatures": [{"label": "f(x, y)", "parameters": [{"label": "x"}, {"label": "y"}]}], "activeParameter": 1})).unwrap();
        assert_eq!(a.ativo, Some((5, 6)));
        assert!(assinatura_de(&json!({"signatures": []})).is_none());
    }

    #[test]
    fn extensao_escolhe_o_servidor_se_existir() {
        assert!(servidor_para(Path::new("a.txt")).is_none());
        if let Some((p, _, l)) = servidor_para(Path::new("a.py")) {
            assert!(!p.is_empty() && l == "python");
        }
    }

    #[test]
    fn typescript_acha_o_cmd_do_npm_no_windows_e_usa_o_idioma_certo() {
        let d = std::env::temp_dir().join(format!("astrograph-ts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // o npm deixa um script sem extensão ao lado do .cmd; só o .cmd executa no Windows
        std::fs::write(d.join("typescript-language-server"), "#!/bin/sh").unwrap();
        let dirs = vec![d.clone()];
        assert!(servidor_em(Path::new("a.ts"), &dirs, true).is_none(), "o script sem extensão não serve no Windows");
        std::fs::write(d.join("typescript-language-server.cmd"), "@echo off").unwrap();
        let (prog, args, lang) = servidor_em(Path::new("a.ts"), &dirs, true).expect("achou o .cmd");
        assert!(prog.ends_with("typescript-language-server.cmd"), "{prog}");
        assert_eq!((args, lang), (vec!["--stdio".to_string()], "typescript"));
        for (arq, idioma) in [("a.tsx", "typescriptreact"), ("a.js", "javascript"), ("a.jsx", "javascriptreact"), ("a.mts", "typescript"), ("a.cjs", "javascript")] {
            assert_eq!(servidor_em(Path::new(arq), &dirs, true).map(|s| s.2), Some(idioma), "{arq}");
        }
        // Linux/macOS: o arquivo com o nome exato
        assert!(servidor_em(Path::new("a.ts"), &dirs, false).is_some());
        // sem servidor instalado há a dica de como instalar
        assert!(servidor_em(Path::new("a.ts"), &[], false).is_none());
        let ts = instalacao_para(Path::new("x.tsx")).expect("TypeScript");
        assert_eq!((ts.nome, ts.requer, ts.comando), ("TypeScript/JavaScript", Some("npm"), Some("npm i -g typescript typescript-language-server")));
        assert!(instalacao_para(Path::new("x.lua")).is_some_and(|i| i.comando.is_none()), "Lua só tem o texto");
        assert!(instalacao_para(Path::new("x.txt")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
