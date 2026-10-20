//! Agentes de IA no motor nativo: qual está escolhido, em que modo de permissão abre, as conversas
//! guardadas (Claude Code) e os pedidos prontos (corrigir o erro da linha, explicar, revisar, mensagem
//! de commit). A escolha do agente e do modo fica no mesmo `config.json` do Astrograph que o Neovim
//! usa (`agent` e `agent_mode`), então vale nos dois lados.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

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
];

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

#[cfg(test)]
mod testes {
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
