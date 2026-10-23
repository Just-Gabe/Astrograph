//! Controle do app aberto por agentes: um servidor local (o app), o cliente `astrograph --ctl` e o
//! servidor MCP `astrograph --mcp` (que o Claude Code lançado pelo painel do agente já recebe).
//!
//! Protocolo: TCP em 127.0.0.1, porta aleatória; uma linha JSON por pedido
//! `{"token": "...", "comando": "estado"}` e uma linha de resposta `{"ok": true, "resultado": ...}` ou
//! `{"ok": false, "erro": "..."}`. Os comandos são os do `harness.rs`.
//!
//! Segurança: só aceita conexões locais e com o token da sessão (256 bits, comparado em tempo
//! constante). O token vai para o arquivo `controle.json` na pasta de estado do usuário e para o
//! ambiente dos terminais que o app abre (e do agente que roda neles): `ASTROGRAPH_CONTROLE` e
//! `ASTROGRAPH_TOKEN`. Pedido em HTTP (um navegador tentando a porta) fecha a conexão sem resposta.
//! Desligado (`[agentes] controle = false`, Configurações ou `<Leader>AM`), não há servidor, token,
//! arquivo nem variável: religar sobe tudo de novo, com outro token.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

static LIGADO: AtomicBool = AtomicBool::new(true);
/// Sobe a cada servidor novo: conexões de uma subida anterior (com o token velho) não executam mais nada.
static GERACAO: AtomicU64 = AtomicU64::new(0);

pub fn ligado() -> bool {
    LIGADO.load(Ordering::SeqCst)
}

/// Liga ou desliga na hora: desligar derruba o servidor e apaga o endereço e o token; ligar sobe outro.
pub fn ligar(sim: bool) {
    LIGADO.store(sim, Ordering::SeqCst);
    if sim {
        subir();
    } else {
        descer();
    }
}

/// Maior pedido aceito (uma linha).
const MAX_LINHA: u64 = 1 << 20;
/// Conexões ao mesmo tempo.
const MAX_CONEXOES: usize = 16;

pub fn pasta() -> PathBuf {
    crate::harness::pasta_estado()
}

fn arquivo_atual() -> PathBuf {
    pasta().join("controle.json")
}

fn arquivo_do_processo() -> PathBuf {
    pasta().join("controle").join(format!("{}.json", std::process::id()))
}

/// Token novo: 32 bytes do gerador do sistema, em hexadecimal.
fn token_novo() -> Option<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn mesmo_token(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Grava o arquivo só para o usuário (no Windows a pasta do perfil já é dele).
fn gravar_privado(caminho: &std::path::Path, texto: &str) -> std::io::Result<()> {
    if let Some(p) = caminho.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(caminho)?.write_all(texto.as_bytes())
}

/// Apaga os arquivos de endereço se ainda forem deste processo.
fn apagar_arquivos() {
    for a in [arquivo_do_processo(), arquivo_atual()] {
        let meu = std::fs::read_to_string(&a).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["pid"] == json!(std::process::id()));
        if meu {
            let _ = std::fs::remove_file(&a);
        }
    }
}

/// Um pedido que chegou: a linha de comando e para onde vai a resposta.
pub type Pedido = (String, tokio::sync::oneshot::Sender<Result<Value, String>>);
type Enviar = Arc<dyn Fn(Pedido) -> bool + Send + Sync>;

struct Servidor {
    porta: u16,
    token: String,
    tarefa: tokio::task::AbortHandle,
}

/// Para onde os pedidos vão (a task dona do editor) e o runtime onde o servidor roda.
static DESTINO: Mutex<Option<(Enviar, tokio::runtime::Handle)>> = Mutex::new(None);
static SERVIDOR: Mutex<Option<Servidor>> = Mutex::new(None);

/// Ao sair do editor: derruba o servidor e apaga os arquivos.
pub struct Encerrar;

impl Drop for Encerrar {
    fn drop(&mut self) {
        descer();
        *DESTINO.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Chamado pela task dona do editor (dentro do runtime tokio): guarda para onde mandar os pedidos e sobe o
/// servidor se o controle estiver ligado. `ASTROGRAPH_CONTROLE=0` no ambiente desliga de vez nesta sessão.
pub fn preparar(enviar: impl Fn(Pedido) -> bool + Send + Sync + 'static, ligado_na_config: bool) -> Encerrar {
    let desligado_no_ambiente = std::env::var("ASTROGRAPH_CONTROLE").is_ok_and(|v| v == "0");
    *DESTINO.lock().unwrap_or_else(|e| e.into_inner()) = Some((Arc::new(enviar), tokio::runtime::Handle::current()));
    ligar(ligado_na_config && !desligado_no_ambiente);
    Encerrar
}

fn subir() -> bool {
    let mut srv = SERVIDOR.lock().unwrap_or_else(|e| e.into_inner());
    if srv.is_some() {
        return true;
    }
    let Some((enviar, rt)) = DESTINO.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return false };
    let Some(token) = token_novo() else { return false };
    let Ok(std_l) = std::net::TcpListener::bind(("127.0.0.1", 0)) else { return false };
    let Some(porta) = std_l.set_nonblocking(true).ok().and_then(|_| std_l.local_addr().ok()).map(|a| a.port()) else { return false };
    let geracao = GERACAO.fetch_add(1, Ordering::SeqCst) + 1;
    let vagas = Arc::new(tokio::sync::Semaphore::new(MAX_CONEXOES));
    let tok = token.clone();
    let tarefa = rt.spawn(async move {
        let Ok(listener) = tokio::net::TcpListener::from_std(std_l) else { return };
        loop {
            let (sock, origem) = match listener.accept().await {
                Ok(c) => c,
                // sem descritores, por exemplo: espera em vez de girar a CPU
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            if !origem.ip().is_loopback() {
                continue;
            }
            let Ok(vaga) = vagas.clone().try_acquire_owned() else { continue };
            let (token, enviar) = (tok.clone(), enviar.clone());
            tokio::spawn(async move {
                atender(sock, &token, geracao, enviar.as_ref()).await;
                drop(vaga);
            });
        }
    });
    let info = json!({ "porta": porta, "token": token, "pid": std::process::id() }).to_string();
    let _ = gravar_privado(&arquivo_do_processo(), &info);
    let _ = gravar_privado(&arquivo_atual(), &info);
    *srv = Some(Servidor { porta, token, tarefa: tarefa.abort_handle() });
    true
}

fn descer() {
    if let Some(s) = SERVIDOR.lock().unwrap_or_else(|e| e.into_inner()).take() {
        s.tarefa.abort();
        GERACAO.fetch_add(1, Ordering::SeqCst);
        apagar_arquivos();
    }
}

/// O próprio app pede um comando à task do editor (ex.: recarregar o git depois de um commit), esperando a vez.
pub fn pedido_interno(linha: &str) {
    let Some((enviar, _)) = DESTINO.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let (tx, rx) = tokio::sync::oneshot::channel();
    if enviar((linha.to_string(), tx)) {
        let _ = rx.blocking_recv();
    }
}

/// Endereço e token do servidor no ar (para os terminais e agentes que o app abre).
pub fn endereco() -> Option<(String, String)> {
    SERVIDOR.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| (format!("127.0.0.1:{}", s.porta), s.token.clone()))
}

/// Põe (ou tira, com o controle desligado) o endereço e o token no ambiente de um processo que o app abre.
pub fn aplicar_ambiente(cmd: &mut portable_pty::CommandBuilder) {
    match endereco() {
        Some((e, t)) => {
            cmd.env("ASTROGRAPH_CONTROLE", e);
            cmd.env("ASTROGRAPH_TOKEN", t);
        }
        None => {
            cmd.env_remove("ASTROGRAPH_CONTROLE");
            cmd.env_remove("ASTROGRAPH_TOKEN");
        }
    }
}

fn parece_http(l: &str) -> bool {
    ["GET ", "POST ", "PUT ", "HEAD ", "OPTIONS ", "DELETE ", "PATCH ", "CONNECT ", "TRACE "].iter().any(|m| l.starts_with(m))
}

async fn atender(sock: tokio::net::TcpStream, token: &str, geracao: u64, enviar: &(dyn Fn(Pedido) -> bool + Send + Sync)) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let (r, mut wr) = sock.into_split();
    let mut leitor = tokio::io::BufReader::new(r);
    loop {
        let mut buf = Vec::new();
        match (&mut leitor).take(MAX_LINHA).read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if !buf.ends_with(b"\n") && buf.len() as u64 >= MAX_LINHA {
            return; // linha grande demais
        }
        let linha = String::from_utf8_lossy(&buf);
        let linha = linha.trim();
        if linha.is_empty() {
            continue;
        }
        if parece_http(linha) {
            return;
        }
        let resposta = match serde_json::from_str::<Value>(linha) {
            Err(_) => json!({ "ok": false, "erro": "pedido inválido: mande uma linha JSON {\"token\", \"comando\"}" }),
            Ok(v) if !mesmo_token(v["token"].as_str().unwrap_or(""), token) => {
                let _ = wr.write_all(b"{\"ok\":false,\"erro\":\"token inv\xc3\xa1lido\"}\n").await;
                return;
            }
            // desligado, ou religado com outro token depois que esta conexão abriu
            Ok(_) if !ligado() || GERACAO.load(Ordering::SeqCst) != geracao => {
                let _ = wr.write_all("{\"ok\":false,\"erro\":\"os agentes estão desligados deste editor (ligue em Configurações ou com o atalho <Líder>AM)\"}\n".as_bytes()).await;
                return;
            }
            Ok(v) => {
                let comando = v["comando"].as_str().unwrap_or("").to_string();
                let (tx, rx) = tokio::sync::oneshot::channel();
                if !enviar((comando, tx)) {
                    return;
                }
                match tokio::time::timeout(Duration::from_secs(60), rx).await {
                    Ok(Ok(Ok(r))) => json!({ "ok": true, "resultado": r }),
                    Ok(Ok(Err(e))) => json!({ "ok": false, "erro": e }),
                    _ => json!({ "ok": false, "erro": "o editor não respondeu" }),
                }
            }
        };
        let mut s = resposta.to_string();
        s.push('\n');
        if wr.write_all(s.as_bytes()).await.is_err() {
            return;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// cliente
// ---------------------------------------------------------------------------------------------

/// Conexão com o app aberto: o do ambiente (quando o processo nasceu dentro do app) ou o do
/// `controle.json` (o app aberto mais recente).
pub struct Cliente {
    leitor: BufReader<std::net::TcpStream>,
    escritor: std::net::TcpStream,
    token: String,
}

impl Cliente {
    pub fn conectar() -> Result<Cliente, String> {
        let (endereco, token) = match (std::env::var("ASTROGRAPH_CONTROLE"), std::env::var("ASTROGRAPH_TOKEN")) {
            (Ok(e), Ok(t)) if e != "0" => (e, t),
            _ => {
                let txt = std::fs::read_to_string(arquivo_atual()).map_err(|_| "nenhum Astrograph aberto com os agentes ligados (só o motor nativo tem o controle; ligue em Configurações ou com <Líder>AM)".to_string())?;
                let v: Value = serde_json::from_str(&txt).map_err(|_| "controle.json inválido".to_string())?;
                (format!("127.0.0.1:{}", v["porta"]), v["token"].as_str().unwrap_or("").to_string())
            }
        };
        let s = std::net::TcpStream::connect_timeout(&endereco.parse().map_err(|_| format!("endereço inválido: {endereco}"))?, Duration::from_secs(3))
            .map_err(|e| format!("o Astrograph não respondeu em {endereco}: {e}"))?;
        s.set_read_timeout(Some(Duration::from_secs(90))).ok();
        let escritor = s.try_clone().map_err(|e| e.to_string())?;
        Ok(Cliente { leitor: BufReader::new(s), escritor, token })
    }

    pub fn pedir(&mut self, comando: &str) -> Result<Value, String> {
        let mut l = json!({ "token": self.token, "comando": comando }).to_string();
        l.push('\n');
        self.escritor.write_all(l.as_bytes()).map_err(|e| format!("enviar: {e}"))?;
        let mut r = String::new();
        self.leitor.read_line(&mut r).map_err(|e| format!("ler: {e}"))?;
        let v: Value = serde_json::from_str(r.trim()).map_err(|_| format!("resposta inválida: {r:?}"))?;
        if v["ok"] == json!(true) {
            Ok(v["resultado"].clone())
        } else {
            Err(v["erro"].as_str().unwrap_or("erro").to_string())
        }
    }
}

/// `astrograph --ctl <comando...>` (ou `--ctl -`: um comando por linha da entrada). `espera <ms>` é
/// do cliente. Imprime o JSON de cada resposta; sai com 1 se algum falhar.
pub fn ctl(args: &[String]) -> i32 {
    let comandos: Vec<String> = if args.first().is_some_and(|a| a == "-") {
        std::io::stdin().lock().lines().map_while(Result::ok).filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).collect()
    } else if args.is_empty() {
        eprintln!("uso: astrograph --ctl <comando>   (estado, teclas :w<CR>, abrir arquivo.rs 10, print, atalhos...)");
        return 2;
    } else {
        vec![args.join(" ")]
    };
    let mut cli = match Cliente::conectar() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let mut falhou = false;
    for c in comandos {
        if let Some(ms) = c.strip_prefix("espera ").and_then(|m| m.trim().parse::<u64>().ok()) {
            std::thread::sleep(Duration::from_millis(ms.min(60_000)));
            continue;
        }
        match cli.pedir(&c) {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
            Err(e) => {
                falhou = true;
                eprintln!("erro: {e}");
            }
        }
    }
    i32::from(falhou)
}

// ---------------------------------------------------------------------------------------------
// servidor MCP (stdio)
// ---------------------------------------------------------------------------------------------

fn base64(dados: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(dados.len().div_ceil(3) * 4);
    for c in dados.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn ferramenta(nome: &str, descricao: &str, props: Value, obrigatorios: &[&str]) -> Value {
    json!({ "name": nome, "description": descricao, "inputSchema": { "type": "object", "properties": props, "required": obrigatorios } })
}

fn ferramentas() -> Vec<Value> {
    let s = |d: &str| json!({ "type": "string", "description": d });
    vec![
        ferramenta("astrograph_estado", "Estado da tela do Astrograph: pasta, arquivo, modo, cursor, trecho em volta do cursor, abas, buscador/menu aberto, explorador, terminal, avisos e diagnósticos.", json!({}), &[]),
        ferramenta("astrograph_print", "Print da janela do Astrograph (PNG). Use para ver menus, popups e o layout.", json!({}), &[]),
        ferramenta("astrograph_teclas", "Manda teclas ao editor na notação do Vim: `:w<CR>`, `<Esc>`, `<C-d>`, `dd`, `ihello<Esc>`. Para os menus, prefira astrograph_acao (a tecla líder é configurável). Devolve o estado depois.", json!({ "teclas": s("teclas em notação do Vim") }), &["teclas"]),
        ferramenta("astrograph_texto", "Digita um texto literal (no modo de inserção, num buscador ou num prompt). Devolve o estado depois.", json!({ "texto": s("texto a digitar") }), &["texto"]),
        ferramenta("astrograph_comando", "Roda um comando `:` do editor (sem os dois-pontos): `w`, `tabnew`, `vsplit`, `%s/a/b/g`, `config`, `Rename novo`, `Format`.", json!({ "comando": s("comando sem o :") }), &["comando"]),
        ferramenta("astrograph_abrir", "Abre um arquivo numa aba (caminho relativo à pasta do projeto ou absoluto), opcionalmente numa linha.", json!({ "caminho": s("arquivo"), "linha": { "type": "integer", "minimum": 1 } }), &["caminho"]),
        ferramenta("astrograph_abrir_pasta", "Abre uma pasta como projeto no Astrograph (buscas, explorador, git e terminal passam a usar ela).", json!({ "caminho": s("pasta") }), &["caminho"]),
        ferramenta("astrograph_salvar", "Grava o arquivo da aba ativa, ou todas as abas modificadas com `todos`.", json!({ "todos": { "type": "boolean" } }), &[]),
        ferramenta("astrograph_buffer", "Texto de um arquivo aberto como está no editor (inclui o que ainda não foi salvo). Sem caminho: a aba ativa.", json!({ "caminho": s("arquivo aberto (opcional)") }), &[]),
        ferramenta("astrograph_atalhos", "Lista os atalhos do menu da tecla líder (o que cada sequência faz).", json!({}), &[]),
        ferramenta("astrograph_acao", "Executa um atalho do menu da líder pela sequência (ex.: `ff` buscar arquivo, `e` explorador, `fc` configurações, `Ag` escolher agente), qualquer que seja a tecla líder. Recusa se o editor estiver no meio de uma inserção ou de um menu. Devolve o estado depois.", json!({ "teclas": s("sequência depois da líder") }), &["teclas"]),
        ferramenta("astrograph_terminal", "Roda um comando de shell num terminal do Astrograph (nunca no do agente), espera e devolve as linhas da tela do terminal.", json!({ "comando": s("comando de shell"), "esperar_ms": { "type": "integer", "minimum": 0, "maximum": 60000 } }), &["comando"]),
        ferramenta("astrograph_git_status", "git status do projeto aberto.", json!({}), &[]),
        ferramenta("astrograph_git_diff", "git diff do projeto aberto (ou de um arquivo).", json!({ "caminho": s("arquivo (opcional)") }), &[]),
        ferramenta("astrograph_git_commit", "Faz o commit do que está no disco (não grava as abas: a resposta lista as que estão sem salvar). Com `tudo`, inclui todas as alterações não ignoradas (git add -A); sem ele, só o que já está no stage. `pasta` escolhe o repositório (padrão: a pasta do projeto aberto).", json!({ "mensagem": s("mensagem do commit"), "tudo": { "type": "boolean" }, "pasta": s("repositório (opcional)") }), &["mensagem"]),
        ferramenta("astrograph_esperar", "Espera alguns milissegundos (resultados de busca, servidor de linguagem, animações) e devolve o estado.", json!({ "ms": { "type": "integer", "minimum": 0, "maximum": 30000 } }), &["ms"]),
    ]
}

/// Traduz a chamada de ferramenta em comandos do harness e monta o conteúdo da resposta MCP.
fn chamar(cli: &mut Cliente, nome: &str, a: &Value) -> Result<Vec<Value>, String> {
    let s = |k: &str| a[k].as_str().unwrap_or("").to_string();
    // JSON compacto: cada ação devolve o estado, e o agente paga tokens por ele
    let texto = |v: Value| json!({ "type": "text", "text": v.to_string() });
    let pausa = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
    // ações que mudam a tela: devolve o estado depois de um instante (buscas e animações chegam)
    let mut acao_e_estado = |cli: &mut Cliente, cmd: String| -> Result<Vec<Value>, String> {
        let r = cli.pedir(&cmd)?;
        pausa(150);
        let e = cli.pedir("estado")?;
        Ok(vec![texto(json!({ "resultado": r, "estado": e }))])
    };
    match nome {
        "astrograph_estado" => Ok(vec![texto(cli.pedir("estado")?)]),
        "astrograph_print" => {
            let r = cli.pedir("print")?;
            let caminho = r["png"].as_str().unwrap_or("");
            let png = std::fs::read(caminho).map_err(|e| format!("ler o print: {e}"))?;
            Ok(vec![json!({ "type": "image", "data": base64(&png), "mimeType": "image/png" }), texto(r)])
        }
        "astrograph_teclas" => acao_e_estado(cli, format!("teclas {}", s("teclas"))),
        "astrograph_texto" => acao_e_estado(cli, format!("texto {}", s("texto"))),
        "astrograph_comando" => acao_e_estado(cli, format!("comando {}", s("comando"))),
        "astrograph_abrir" => {
            let linha = a["linha"].as_u64().map(|l| format!(" {l}")).unwrap_or_default();
            acao_e_estado(cli, format!("abrir {}{linha}", s("caminho")))
        }
        "astrograph_abrir_pasta" => acao_e_estado(cli, format!("pasta {}", s("caminho"))),
        "astrograph_salvar" => Ok(vec![texto(cli.pedir(if a["todos"] == json!(true) { "salvar todos" } else { "salvar" })?)]),
        "astrograph_buffer" => Ok(vec![texto(cli.pedir(&format!("buffer {}", s("caminho")))?)]),
        "astrograph_atalhos" => Ok(vec![texto(cli.pedir("atalhos")?)]),
        "astrograph_acao" => acao_e_estado(cli, format!("acao {}", s("teclas"))),
        "astrograph_terminal" => {
            cli.pedir(&format!("terminal {}", s("comando")))?;
            pausa(a["esperar_ms"].as_u64().unwrap_or(1500).min(60_000));
            let t = cli.pedir("terminal_tela")?;
            Ok(vec![texto(t)])
        }
        "astrograph_git_status" => Ok(vec![texto(cli.pedir("git_status")?)]),
        "astrograph_git_diff" => Ok(vec![texto(cli.pedir(&format!("git_diff {}", s("caminho")))?)]),
        "astrograph_git_commit" => {
            let mut opcoes = String::new();
            if a["tudo"] == json!(true) {
                opcoes.push_str(" --tudo");
            }
            if !s("pasta").is_empty() {
                opcoes.push_str(&format!(" --em {}", s("pasta")));
            }
            Ok(vec![texto(cli.pedir(&format!("git_commit{opcoes} -- {}", s("mensagem")))?)])
        }
        "astrograph_esperar" => {
            pausa(a["ms"].as_u64().unwrap_or(500).min(30_000));
            Ok(vec![texto(cli.pedir("estado")?)])
        }
        outro => Err(format!("ferramenta desconhecida: {outro}")),
    }
}

/// Resposta a uma mensagem JSON-RPC do cliente MCP (`None` para notificações).
pub fn mcp_responder(cli: &mut Option<Cliente>, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let metodo = msg["method"].as_str().unwrap_or("");
    let resultado = match metodo {
        "initialize" => json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "astrograph", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Ferramentas para operar o editor Astrograph aberto (o mesmo onde você está rodando): ver a tela (astrograph_estado, astrograph_print), abrir arquivos e pastas, mandar teclas do Vim, rodar comandos :, usar o menu da líder, rodar comandos no terminal do app, salvar e fazer commit. Depois de editar arquivos no disco, o editor recarrega sozinho; use astrograph_estado para conferir.",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": ferramentas() }),
        "tools/call" => {
            let nome = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"]["arguments"].clone();
            if cli.is_none() {
                *cli = Cliente::conectar().ok();
            }
            let r = match cli.as_mut() {
                None => Err("o Astrograph não está aberto com os agentes ligados (motor nativo; Configurações ou <Líder>AM liga). O agente lançado pelo app já sabe como achá-lo".to_string()),
                Some(c) => chamar(c, nome, &args),
            };
            // conexão caiu (app reiniciado): tenta de novo uma vez, só nas ferramentas que não mudam nada (terminal,
            // commit, teclas... poderiam rodar duas vezes)
            let so_le = matches!(nome, "astrograph_estado" | "astrograph_print" | "astrograph_buffer" | "astrograph_atalhos" | "astrograph_git_status" | "astrograph_git_diff" | "astrograph_esperar");
            let r = match r {
                Err(e) if so_le && (e.starts_with("enviar:") || e.starts_with("ler:")) => {
                    *cli = Cliente::conectar().ok();
                    match cli.as_mut() {
                        Some(c) => chamar(c, nome, &args),
                        None => Err(e),
                    }
                }
                r => r,
            };
            match r {
                Ok(conteudo) => json!({ "content": conteudo, "isError": false }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            }
        }
        _ => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("método desconhecido: {metodo}") } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": resultado }))
}

/// `astrograph --mcp`: servidor MCP na entrada e saída padrão (uma mensagem JSON por linha).
pub fn mcp() -> i32 {
    let mut cli: Option<Cliente> = None;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for linha in stdin.lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(linha.trim()) else { continue };
        if let Some(r) = mcp_responder(&mut cli, &msg) {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
    0
}

/// Config do MCP para o `claude --mcp-config`: este executável com `--mcp`.
pub fn arquivo_mcp() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = pasta().join("mcp-astrograph.json");
    let cfg = json!({ "mcpServers": { "astrograph": { "type": "stdio", "command": exe.to_string_lossy(), "args": ["--mcp"] } } });
    gravar_privado(&p, &serde_json::to_string_pretty(&cfg).ok()?).ok()?;
    Some(p)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn token_comparado_por_inteiro_e_base64() {
        assert!(mesmo_token("abc", "abc"));
        assert!(!mesmo_token("abc", "abd") && !mesmo_token("abc", "ab") && !mesmo_token("", "a"));
        let t = token_novo().unwrap();
        assert_eq!(t.len(), 64);
        assert_ne!(t, token_novo().unwrap());
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert!(parece_http("GET / HTTP/1.1") && parece_http("POST /x HTTP/1.1") && !parece_http("{\"token\":1}"));
    }

    /// O servidor de verdade (TCP, token) sobre um editor de verdade, como no app: um agente abre um
    /// arquivo, edita, grava, lê o buffer, tira print, roda no terminal e faz commit.
    #[test]
    fn agente_opera_o_app_pelo_controle_de_ponta_a_ponta() {
        let _t = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("astrograph-controle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let proj = base.join("proj");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::write(proj.join("src/a.rs"), "fn a() {}\n").unwrap();
        let git = |args: &[&str]| std::process::Command::new("git").args(args).current_dir(&proj).output().map(|o| o.status.success()).unwrap_or(false);
        let tem_git = git(&["init", "-q"]) && git(&["config", "user.email", "t@t"]) && git(&["config", "user.name", "t"]) && git(&["add", "-A"]) && git(&["commit", "-qm", "inicio"]);
        let estado_antes = std::env::var_os("ASTROGRAPH_STATE");
        std::env::set_var("ASTROGRAPH_STATE", base.join("estado"));
        std::env::set_var("ASTROGRAPH_LSP", "0");

        // a "task nativa": dona do editor, atende os pedidos do servidor
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Pedido>();
        let (pronto_tx, pronto_rx) = std::sync::mpsc::channel();
        let raiz = proj.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let _g = preparar(move |p| tx.send(p).is_ok(), true);
                let mut w = crate::native::Workspace::new(None, ' ', Arc::new(|| {}));
                w.resize(100, 30.0);
                w.feed(&format!(":cd {}<CR>", raiz.display()));
                let mut pintor = None;
                pronto_tx.send(()).unwrap();
                while let Some((linha, resp)) = rx.recv().await {
                    crate::harness::executar_ao_vivo(&mut w, &mut pintor, &linha, resp);
                }
            });
        });
        pronto_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        // o app não mexe mais no próprio ambiente (só no dos terminais que abre): o cliente acha pelo controle.json
        assert!(std::env::var("ASTROGRAPH_TOKEN").is_err());
        let mut c = Cliente::conectar().expect("conecta pelo controle.json");

        let e = c.pedir("estado").unwrap();
        assert_eq!(e["tela_inicial"], true);
        // abrir, digitar, salvar
        c.pedir("abrir src/a.rs 1").unwrap();
        c.pedir("teclas O// do agente<Esc>").unwrap();
        let e = c.pedir("estado").unwrap();
        assert_eq!((e["arquivo"].as_str(), e["modificado"].as_bool()), (Some("a.rs"), Some(true)));
        let b = c.pedir("buffer").unwrap();
        assert_eq!(b["texto"], "// do agente\nfn a() {}\n");
        assert_eq!(std::fs::read_to_string(proj.join("src/a.rs")).unwrap(), "fn a() {}\n", "ainda não gravou");
        c.pedir("salvar").unwrap();
        assert_eq!(std::fs::read_to_string(proj.join("src/a.rs")).unwrap(), "// do agente\nfn a() {}\n");
        // erros viram resposta de erro, não queda
        assert!(c.pedir("abrir nao/existe.rs").unwrap_err().contains("não existe"));
        assert!(c.pedir("acao zz-nao-existe").is_err());
        assert!(c.pedir("comando_inventado").unwrap_err().contains("desconhecido"));
        // menu da líder: lista e executa
        let m = c.pedir("atalhos").unwrap();
        assert!(m["atalhos"].as_array().unwrap().iter().any(|a| a["teclas"] == "ff"));
        c.pedir("acao ff").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(c.pedir("estado").unwrap()["buscador"]["titulo"], "Buscar arquivo");
        c.pedir("teclas <Esc>").unwrap();
        // print: PNG de verdade
        let p = c.pedir("print").unwrap();
        let png = std::fs::read(p["png"].as_str().unwrap()).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // terminal: o comando roda num shell do app
        c.pedir("terminal echo astro-ok-123").unwrap();
        let mut viu = false;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(t) = c.pedir("terminal_tela") {
                if t["linhas"].as_array().unwrap().iter().any(|l| l.as_str() == Some("astro-ok-123")) {
                    viu = true;
                    break;
                }
            }
        }
        assert!(viu, "a saída do echo apareceu no terminal: {:?}", c.pedir("terminal_tela"));
        c.pedir("teclas <C-\\><C-n>").unwrap();
        // commit
        if tem_git {
            c.pedir("teclas ggA!<Esc>").unwrap();
            // não grava a aba por conta própria: avisa; com a aba salva, o commit leva tudo
            let r = c.pedir("git_commit --tudo -- agente: comentário").unwrap();
            assert_eq!(r["abas_nao_salvas"].as_array().unwrap().len(), 1, "{r}");
            c.pedir("salvar").unwrap();
            c.pedir("git_commit --tudo -- agente: o resto").unwrap();
            // mensagem que começa com --tudo é mensagem, não opção
            assert!(c.pedir("git_commit --tudo e mais nada").is_err(), "sem nada no stage o commit falha, sem add -A");
            let log = std::process::Command::new("git").args(["log", "-1", "--format=%s"]).current_dir(&proj).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "agente: o resto");
            assert!(c.pedir("git_status").unwrap()["status"].as_str().unwrap().lines().count() == 1, "árvore limpa depois do commit");
        }

        // CONTROLES de segurança: token errado e pedido HTTP não executam nada
        let (endereco, _) = super::endereco().unwrap();
        let mut s = std::net::TcpStream::connect(&endereco).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).ok();
        s.write_all(b"{\"token\":\"errado\",\"comando\":\"teclas ggdG\"}\n").unwrap();
        let mut r = String::new();
        BufReader::new(&s).read_line(&mut r).unwrap();
        assert!(r.contains("token inv"), "{r}");
        let mut h = std::net::TcpStream::connect(&endereco).unwrap();
        h.set_read_timeout(Some(Duration::from_secs(5))).ok();
        h.write_all(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n{\"comando\":\"teclas ggdG\"}\n").unwrap();
        let mut r = String::new();
        let _ = BufReader::new(&h).read_line(&mut r);
        assert!(r.is_empty(), "HTTP fecha sem resposta: {r:?}");
        assert!(c.pedir("buffer").unwrap()["texto"].as_str().unwrap().contains("fn a()"), "nada foi apagado");
        // print só na pasta de prints: caminho absoluto ou `..` viram só o nome
        let p = c.pedir(&format!("print {}", base.join("fora.png").display())).unwrap();
        assert!(!base.join("fora.png").exists() && p["png"].as_str().unwrap().contains("prints"), "{p}");
        // desligado (atalho ou configuração): servidor cai, arquivos e endereço somem; a conexão aberta é recusada
        ligar(false);
        assert!(super::endereco().is_none() && !arquivo_atual().exists());
        assert!(c.pedir("estado").unwrap_err().contains("desligados"));
        assert!(std::net::TcpStream::connect(&endereco).is_err(), "a porta fechou");
        // religado: outro token; a conexão velha continua recusada, uma nova funciona
        ligar(true);
        assert!(Cliente::conectar().unwrap().pedir("estado").is_ok());
        // o registro guardou as ações
        let log = std::fs::read_to_string(base.join("estado").join("agentes.log")).unwrap();
        assert!(log.contains("abrir src/a.rs 1") && log.contains("git_commit"), "{log}");
        // o arquivo de controle existe e aponta para este processo
        let info: Value = serde_json::from_str(&std::fs::read_to_string(arquivo_atual()).unwrap()).unwrap();
        assert_eq!(info["pid"], json!(std::process::id()));
        ligar(false);
        std::env::remove_var("ASTROGRAPH_LSP");
        match estado_antes {
            Some(v) => std::env::set_var("ASTROGRAPH_STATE", v),
            None => std::env::remove_var("ASTROGRAPH_STATE"),
        }
    }

    #[test]
    fn mcp_lista_ferramentas_e_recusa_metodo_desconhecido() {
        let mut cli = None;
        let r = mcp_responder(&mut cli, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "astrograph");
        assert!(mcp_responder(&mut cli, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let r = mcp_responder(&mut cli, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        let nomes: Vec<&str> = r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for n in ["astrograph_estado", "astrograph_print", "astrograph_teclas", "astrograph_abrir", "astrograph_git_commit", "astrograph_terminal"] {
            assert!(nomes.contains(&n), "{n}");
        }
        let r = mcp_responder(&mut cli, &json!({ "jsonrpc": "2.0", "id": 3, "method": "nada/disso" })).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }
}
