//! Agentes de IA no motor nativo: qual está escolhido, em que modo de permissão abre, as conversas
//! guardadas (Claude Code) e os pedidos prontos (corrigir o erro da linha, explicar, revisar, mensagem
//! de commit). A escolha do agente e do modo fica no mesmo `config.json` do Astrograph que o Neovim
//! usa (`agent` e `agent_mode`), então vale nos dois lados.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::proc::SemJanela;

pub struct Agente {
    pub bin: &'static str,
    pub label: &'static str,
    /// pacote npm do CLI (para instalar)
    pub npm: &'static str,
}

/// Na ordem em que aparecem nos menus.
pub const AGENTES: &[Agente] = &[
    Agente { bin: "claude", label: "Claude Code", npm: "@anthropic-ai/claude-code" },
    Agente { bin: "opencode", label: "opencode", npm: "opencode-ai" },
    Agente { bin: "codex", label: "Codex CLI", npm: "@openai/codex" },
    Agente { bin: "gemini", label: "Gemini CLI", npm: "@google/gemini-cli" },
    // IA local: sem login nem chave; o modelo vem de `ollama_model` no config.json ou do primeiro de `ollama list`
    Agente { bin: "ollama", label: "Ollama (local)", npm: "" },
];

/// O modelo do Ollama: `ollama_model` do config ou o primeiro de `ollama list`.
pub fn modelo_ollama(pasta: &Path) -> Option<String> {
    if let Some(m) = ler_config(pasta)["ollama_model"].as_str().filter(|m| !m.is_empty()) {
        return Some(m.to_string());
    }
    let o = std::process::Command::new("ollama").arg("list").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).sem_janela().output().ok()?;
    primeiro_modelo(&String::from_utf8_lossy(&o.stdout))
}

/// Primeiro nome da saída de `ollama list` (a primeira linha é o cabeçalho).
pub fn primeiro_modelo(saida: &str) -> Option<String> {
    saida.lines().skip(1).find_map(|l| l.split_whitespace().next().map(str::to_string))
}

pub struct Modo {
    pub id: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
}

pub const MODOS: &[Modo] = &[
    Modo { id: "manual", label: "Manual", desc: "pergunta antes de editar ou executar" },
    Modo { id: "acceptEdits", label: "Aceitar edições", desc: "edita sem perguntar; comandos ainda pedem" },
    Modo { id: "plan", label: "Plano", desc: "só lê e planeja, não altera nada" },
    Modo { id: "auto", label: "Auto", desc: "o agente decide o que precisa de você" },
    Modo { id: "bypass", label: "Sem permissões", desc: "aceita tudo, sem perguntar (cuidado)" },
];

/// Pasta do `config.json` do Astrograph (a mesma do Neovim).
pub fn pasta_config() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("astrograph")
}

pub fn ler_config(pasta: &Path) -> Value {
    std::fs::read_to_string(pasta.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Grava chaves no `config.json`, mantendo as que já existem.
pub fn gravar_config(pasta: &Path, patch: &[(&str, Value)]) -> bool {
    let mut c = ler_config(pasta);
    for (k, v) in patch {
        c[*k] = v.clone();
    }
    let _ = std::fs::create_dir_all(pasta);
    std::fs::write(pasta.join("config.json"), c.to_string()).is_ok()
}

pub fn instalado(bin: &str) -> bool {
    crate::term::em_path(bin)
}

pub fn instalados() -> Vec<&'static Agente> {
    AGENTES.iter().filter(|a| instalado(a.bin)).collect()
}

/// O agente escolhido (se ainda estiver instalado) ou o primeiro disponível.
pub fn atual_em(pasta: &Path, tem: impl Fn(&str) -> bool) -> Option<&'static Agente> {
    let escolhido = ler_config(pasta)["agent"].as_str().map(str::to_string);
    escolhido.and_then(|b| AGENTES.iter().find(|a| a.bin == b && tem(a.bin))).or_else(|| AGENTES.iter().find(|a| tem(a.bin)))
}

pub fn atual() -> Option<&'static Agente> {
    atual_em(&pasta_config(), instalado)
}

pub fn definir_atual(pasta: &Path, bin: &str) -> bool {
    AGENTES.iter().any(|a| a.bin == bin) && gravar_config(pasta, &[("agent", json!(bin))])
}

pub fn modo_atual_em(pasta: &Path) -> &'static Modo {
    let id = ler_config(pasta)["agent_mode"].as_str().map(str::to_string);
    id.and_then(|i| MODOS.iter().find(|m| m.id == i)).unwrap_or(&MODOS[0])
}

pub fn modo_atual() -> &'static Modo {
    modo_atual_em(&pasta_config())
}

pub fn definir_modo(pasta: &Path, id: &str) -> bool {
    MODOS.iter().any(|m| m.id == id) && gravar_config(pasta, &[("agent_mode", json!(id))])
}

/// Linha de comando do agente já no modo `modo`. O segundo valor é `false` quando o CLI não tem como abrir
/// nesse modo (abre no padrão dele). Só o que foi conferido no CLI: `claude --permission-mode` aceita
/// default/acceptEdits/plan/auto/bypassPermissions; `opencode --auto` aprova o que não for negado.
pub fn linha_de_comando(a: &Agente, modo: &str) -> (String, bool) {
    match a.bin {
        "claude" => {
            let m = match modo {
                "manual" => "default",
                "acceptEdits" => "acceptEdits",
                "plan" => "plan",
                "auto" => "auto",
                "bypass" => "bypassPermissions",
                _ => return (a.bin.to_string(), modo == "manual"),
            };
            (format!("claude --permission-mode {m}"), true)
        }
        "opencode" if modo == "auto" || modo == "bypass" => ("opencode --auto".to_string(), modo == "bypass"),
        b => (b.to_string(), modo == "manual"),
    }
}

/// Uma conversa guardada do Claude Code (`~/.claude/projects/<pasta>/<id>.jsonl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessaoClaude {
    pub id: String,
    pub pasta: PathBuf,
    pub titulo: String,
    pub quando: SystemTime,
    pub bytes: u64,
}

/// Quanto de cada arquivo se lê procurando a primeira fala do usuário (os arquivos passam de 30 MB).
const LER_ATE: usize = 1 << 20;

/// Texto da primeira mensagem do usuário num JSONL do Claude Code, e a pasta (`cwd`) da conversa.
pub fn ler_sessao_claude(caminho: &Path) -> Option<(String, PathBuf)> {
    use std::io::Read;
    let mut f = std::fs::File::open(caminho).ok()?;
    let mut buf = Vec::new();
    f.by_ref().take(LER_ATE as u64).read_to_end(&mut buf).ok()?;
    let texto = String::from_utf8_lossy(&buf);
    let (mut titulo, mut cwd) = (None::<String>, None::<PathBuf>);
    for linha in texto.lines() {
        let Ok(v) = serde_json::from_str::<Value>(linha) else { continue };
        if cwd.is_none() {
            cwd = v["cwd"].as_str().map(PathBuf::from);
        }
        if titulo.is_none() && v["type"] == "user" && v["isMeta"] != true {
            let c = &v["message"]["content"];
            let t = match c {
                Value::String(s) => s.clone(),
                Value::Array(a) => a.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" "),
                _ => String::new(),
            };
            let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
            // mensagens de sistema (`<command-name>`, `<local-command-stdout>`...) não servem de título
            if !t.is_empty() && !t.starts_with('<') {
                titulo = Some(t.chars().take(90).collect());
            }
        }
        if titulo.is_some() && cwd.is_some() {
            break;
        }
    }
    Some((titulo?, cwd?))
}

/// Conversas do Claude Code de todos os projetos em `base` (`~/.claude/projects`), da mais recente.
pub fn sessoes_claude(base: &Path) -> Vec<SessaoClaude> {
    let mut v = Vec::new();
    let Ok(pastas) = std::fs::read_dir(base) else { return v };
    for p in pastas.flatten() {
        let Ok(arqs) = std::fs::read_dir(p.path()) else { continue };
        for a in arqs.flatten() {
            let caminho = a.path();
            if caminho.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Some(id) = caminho.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            let Ok(meta) = a.metadata() else { continue };
            if let Some((titulo, pasta)) = ler_sessao_claude(&caminho) {
                v.push(SessaoClaude { id, pasta, titulo, quando: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), bytes: meta.len() });
            }
        }
    }
    v.sort_by(|a, b| b.quando.cmp(&a.quando));
    v
}

pub fn pasta_do_claude() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(home).join(".claude").join("projects"))
}

/// Há quanto tempo, em português curto (`3 min`, `2 h`, `5 d`).
pub fn ha_quanto_tempo(quando: SystemTime) -> String {
    let s = SystemTime::now().duration_since(quando).map_or(0, |d| d.as_secs());
    match s {
        0..=59 => "agora".into(),
        60..=3599 => format!("{} min", s / 60),
        3600..=86399 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86400),
    }
}

// ---- pedidos prontos ao agente ----

pub fn pedido_corrigir(arquivo: &str, linha: usize, gravidade: &str, mensagem: &str) -> String {
    format!("Corrija o {gravidade} na linha {linha} de {arquivo}: {mensagem}\nMexa só no que for preciso para resolver isso.")
}

pub fn pedido_explicar(arquivo: &str, de: usize, ate: usize) -> String {
    if de == ate {
        format!("Explique o código da linha {de} de {arquivo}: o que faz e por quê.")
    } else {
        format!("Explique o código das linhas {de} a {ate} de {arquivo}: o que faz e por quê.")
    }
}

pub fn pedido_revisar() -> String {
    "Revise as alterações ainda não commitadas deste repositório (use `git diff`): aponte bugs, riscos e o que falta testar. Não altere nada.".into()
}

pub fn pedido_commit() -> String {
    "Escreva uma mensagem de commit para as alterações em stage (use `git diff --staged`), no estilo do histórico do repositório. Só mostre a mensagem, não faça o commit.".into()
}

/// Comando (para o shell) que instala o CLI do agente.
pub fn comando_instalar(a: &Agente) -> String {
    if a.bin == "ollama" {
        return if cfg!(windows) { "winget install Ollama.Ollama".into() } else { "curl -fsSL https://ollama.com/install.sh | sh".into() };
    }
    format!("npm install -g {}", a.npm)
}

/// Comando (para o shell) que faz o login do agente.
pub fn comando_login(a: &Agente) -> String {
    match a.bin {
        "claude" => "claude /login".into(),
        "ollama" => "ollama list".into(),
        "opencode" => "opencode auth login".into(),
        "codex" => "codex login".into(),
        _ => a.bin.to_string(),
    }
}

/// Programa e argumentos para rodar o agente uma vez, sem terminal, com `prompt` (a resposta sai no stdout).
pub fn comando_direto(a: &Agente, prompt: &str) -> (String, Vec<String>) {
    let args: Vec<String> = match a.bin {
        "claude" => vec!["-p".into(), prompt.into()],
        "opencode" => vec!["run".into(), prompt.into()],
        "codex" => vec!["exec".into(), "--skip-git-repo-check".into(), prompt.into()],
        "gemini" => vec!["-p".into(), prompt.into()],
        "ollama" => vec!["run".into(), modelo_ollama(&pasta_config()).unwrap_or_else(|| "llama3.2".into()), prompt.into()],
        _ => vec![prompt.into()],
    };
    (a.bin.to_string(), args)
}

/// Pedido de edição do trecho `codigo` (do `arquivo`) seguindo `instrucao`: só o trecho novo na resposta.
pub fn pedido_editar(arquivo: &str, instrucao: &str, codigo: &str) -> String {
    format!(
        "Edite o trecho de código abaixo seguindo a instrução. Responda SOMENTE com o trecho já editado, \
         pronto para substituir o original: sem explicações e sem cercas de markdown. Mantenha o recuo.\n\
         Arquivo: {arquivo}\nInstrução: {instrucao}\n--- CÓDIGO ---\n{codigo}\n--- FIM ---"
    )
}

/// Pergunta sobre o arquivo (e o trecho selecionado, se houver).
pub fn pedido_perguntar(arquivo: &str, pergunta: &str, trecho: Option<&str>) -> String {
    match trecho {
        Some(t) => format!("Arquivo: {arquivo}\nTrecho:\n{t}\n\nPergunta: {pergunta}\nResponda em português, de forma direta. Não altere nenhum arquivo."),
        None => format!("Arquivo: {arquivo}\n\nPergunta: {pergunta}\nResponda em português, de forma direta. Não altere nenhum arquivo."),
    }
}

/// Tira as cercas de markdown (```lang ... ```) da resposta e as linhas em branco das pontas.
pub fn limpar_resposta(t: &str) -> String {
    let t = t.replace("\r\n", "\n");
    let linhas: Vec<&str> = t.lines().collect();
    // se houver um bloco cercado, vale o primeiro
    let corpo: Vec<&str> = match linhas.iter().position(|l| l.trim_start().starts_with("```")) {
        Some(i) => {
            let resto = &linhas[i + 1..];
            let fim = resto.iter().position(|l| l.trim_start().starts_with("```")).unwrap_or(resto.len());
            resto[..fim].to_vec()
        }
        None => linhas,
    };
    let ini = corpo.iter().position(|l| !l.trim().is_empty()).unwrap_or(0);
    let fim = corpo.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
    corpo[ini..fim.max(ini)].join("\n")
}

/// Roda `prog args` em `pasta` e devolve o stdout; para depois de `limite` (mata o processo).
pub fn rodar_direto(prog: &str, args: &[String], pasta: &Path, limite: std::time::Duration) -> Result<String, String> {
    use std::io::Read;
    let mut filho = std::process::Command::new(crate::term::programa(prog))
        .args(args)
        .current_dir(pasta)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .sem_janela()
        .spawn()
        .map_err(|e| format!("não consegui rodar {prog}: {e}"))?;
    let mut so = filho.stdout.take().unwrap();
    let mut se = filho.stderr.take().unwrap();
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let ate = std::time::Instant::now() + limite;
    let status = loop {
        match filho.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if std::time::Instant::now() >= ate => {
                let _ = filho.kill();
                let _ = filho.wait();
                return Err(format!("{prog} demorou mais de {} s", limite.as_secs()));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(40)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let (out, err) = (t_out.join().unwrap_or_default(), t_err.join().unwrap_or_default());
    if !status.success() {
        let msg = String::from_utf8_lossy(&err).trim().to_string();
        return Err(if msg.is_empty() { format!("{prog} terminou com erro") } else { msg.lines().last().unwrap_or("").to_string() });
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod testes {
    #[test]
    fn ollama_modelo_e_comandos() {
        let saida = "NAME            ID            SIZE    MODIFIED\nllama3.2:3b     a80c4f17acd5  2.0 GB  2 weeks ago\nqwen2.5:7b      845dbda0ea48  4.7 GB  3 weeks ago\n";
        assert_eq!(super::primeiro_modelo(saida).as_deref(), Some("llama3.2:3b"));
        assert_eq!(super::primeiro_modelo("NAME ID\n"), None);
        let o = super::AGENTES.iter().find(|a| a.bin == "ollama").unwrap();
        let (p, a) = super::comando_direto(o, "oi");
        assert_eq!(p, "ollama");
        assert_eq!(a[0], "run");
        assert_eq!(a.last().unwrap(), "oi");
    }

    #[test]
    fn comandos_de_instalar_e_login() {
        let a = |b: &str| super::AGENTES.iter().find(|a| a.bin == b).unwrap();
        assert_eq!(super::comando_instalar(a("claude")), "npm install -g @anthropic-ai/claude-code");
        assert_eq!(super::comando_login(a("codex")), "codex login");
        assert_eq!(super::comando_login(a("gemini")), "gemini");
    }

    #[test]
    fn limpa_cercas_e_bordas_da_resposta() {
        use super::limpar_resposta as l;
        assert_eq!(l("```rust\nfn a() {}\n```\n"), "fn a() {}");
        assert_eq!(l("Claro! Aqui está:\n```\n  x = 1\n  y = 2\n```\nPronto."), "  x = 1\n  y = 2");
        assert_eq!(l("\n\n  x = 1\n\n"), "  x = 1");
        assert_eq!(l("sem cercas\nduas linhas"), "sem cercas\nduas linhas");
    }

    #[cfg(unix)]
    #[test]
    fn roda_direto_devolve_stdout_erro_e_limite() {
        let d = std::env::temp_dir();
        let ok = super::rodar_direto("sh", &["-c".into(), "printf 'oi\\n'".into()], &d, std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(ok, "oi\n");
        let erro = super::rodar_direto("sh", &["-c".into(), "echo falhou >&2; exit 3".into()], &d, std::time::Duration::from_secs(5)).unwrap_err();
        assert_eq!(erro, "falhou");
        let lento = super::rodar_direto("sh", &["-c".into(), "sleep 5".into()], &d, std::time::Duration::from_millis(200)).unwrap_err();
        assert!(lento.contains("demorou"), "{lento}");
    }

    use super::*;

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-agentes-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn escolha_do_agente_e_do_modo_ficam_no_config_json_e_mantem_o_resto() {
        let d = tmp("cfg");
        std::fs::write(d.join("config.json"), r#"{"ollama_model": "llama3", "agent": "gemini"}"#).unwrap();
        // gemini não instalado: cai no primeiro disponível
        assert_eq!(atual_em(&d, |b| b == "codex" || b == "claude").map(|a| a.bin), Some("claude"));
        assert_eq!(atual_em(&d, |b| b == "gemini").map(|a| a.bin), Some("gemini"));
        assert!(atual_em(&d, |_| false).is_none());
        assert!(definir_atual(&d, "codex"));
        assert!(!definir_atual(&d, "bobagem"));
        assert_eq!(atual_em(&d, |b| b == "codex").map(|a| a.bin), Some("codex"));
        // modo: padrão Manual; só ids conhecidos
        assert_eq!(modo_atual_em(&d).id, "manual");
        assert!(definir_modo(&d, "plan"));
        assert!(!definir_modo(&d, "voar"));
        assert_eq!(modo_atual_em(&d).id, "plan");
        // as outras chaves do arquivo ficaram
        assert_eq!(ler_config(&d)["ollama_model"], "llama3");
        // arquivo ausente ou quebrado: configuração vazia, sem derrubar
        assert_eq!(ler_config(&d.join("nao-existe")), json!({}));
        std::fs::write(d.join("config.json"), "{ quebrado").unwrap();
        assert_eq!(ler_config(&d), json!({}));
    }

    #[test]
    fn linha_de_comando_por_agente_e_modo() {
        let (cl, op) = (&AGENTES[0], &AGENTES[1]);
        assert_eq!(linha_de_comando(cl, "plan"), ("claude --permission-mode plan".to_string(), true));
        assert_eq!(linha_de_comando(cl, "manual"), ("claude --permission-mode default".to_string(), true));
        assert_eq!(linha_de_comando(cl, "bypass").0, "claude --permission-mode bypassPermissions");
        assert_eq!(linha_de_comando(cl, "acceptEdits").0, "claude --permission-mode acceptEdits");
        // opencode: só o auto; o resto abre no padrão e avisa que não é exato
        assert_eq!(linha_de_comando(op, "auto"), ("opencode --auto".to_string(), false));
        assert_eq!(linha_de_comando(op, "bypass"), ("opencode --auto".to_string(), true));
        assert_eq!(linha_de_comando(op, "plan"), ("opencode".to_string(), false));
        assert_eq!(linha_de_comando(op, "manual"), ("opencode".to_string(), true));
        // agente sem opções conhecidas
        assert_eq!(linha_de_comando(&AGENTES[2], "plan"), ("codex".to_string(), false));
    }

    #[test]
    fn sessoes_do_claude_de_varios_projetos_da_mais_recente_e_com_titulo_do_usuario() {
        let base = tmp("claude");
        let (a, b) = (base.join("-home-a"), base.join("-home-b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // projeto a: linhas de sistema antes da fala do usuário (texto em lista); mensagem de comando não vira título
        std::fs::write(
            a.join("11111111-aaaa.jsonl"),
            [
                r#"{"type":"queue-operation","operation":"enqueue"}"#,
                r#"{"type":"user","cwd":"/home/a","message":{"role":"user","content":"<command-name>/init</command-name>"}}"#,
                r#"{"type":"user","cwd":"/home/a","message":{"role":"user","content":[{"type":"text","text":"conserta   o\nbug do login"}]}}"#,
                "linha quebrada",
                r#"{"type":"assistant","cwd":"/home/a"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(b.join("22222222-bbbb.jsonl"), r#"{"type":"user","cwd":"/home/b","message":{"role":"user","content":"segundo projeto"}}"#).unwrap();
        // sem fala do usuário: não aparece
        std::fs::write(b.join("33333333-vazia.jsonl"), r#"{"type":"system","cwd":"/home/b"}"#).unwrap();
        std::fs::write(b.join("notas.txt"), "x").unwrap();
        let v = sessoes_claude(&base);
        assert_eq!(v.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["22222222-bbbb", "11111111-aaaa"], "mais recente primeiro");
        assert_eq!((v[0].titulo.as_str(), v[0].pasta.clone()), ("segundo projeto", PathBuf::from("/home/b")));
        assert_eq!((v[1].titulo.as_str(), v[1].pasta.clone()), ("conserta o bug do login", PathBuf::from("/home/a")));
        assert!(sessoes_claude(&base.join("nada")).is_empty());
    }

    #[test]
    fn tempo_e_pedidos_prontos() {
        let agora = SystemTime::now();
        assert_eq!(ha_quanto_tempo(agora), "agora");
        assert_eq!(ha_quanto_tempo(agora - std::time::Duration::from_secs(180)), "3 min");
        assert_eq!(ha_quanto_tempo(agora - std::time::Duration::from_secs(7300)), "2 h");
        assert_eq!(ha_quanto_tempo(agora - std::time::Duration::from_secs(3 * 86400)), "3 d");
        assert!(pedido_corrigir("src/a.rs", 12, "erro", "tipo errado").contains("linha 12 de src/a.rs: tipo errado"));
        assert!(pedido_explicar("a.rs", 3, 3).contains("linha 3 de a.rs"));
        assert!(pedido_explicar("a.rs", 3, 9).contains("linhas 3 a 9"));
        assert!(pedido_revisar().contains("git diff"));
        assert!(pedido_commit().contains("git diff --staged") && pedido_commit().contains("não faça o commit"));
    }
}
