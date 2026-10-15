//! astrograph-agent: ponte entre o Neovim e um agente ACP.
//!
//!   astrograph-agent -- <comando do agente> [args...]
//!
//! O editor escreve comandos no stdin (um JSON por linha, campo `op`) e lê
//! eventos no stdout (um JSON por linha, campo `ev`; ver `session.rs`).
//!
//!   {"op":"prompt","text":"...","files":["a.rs"],"snippets":[{"path":..,"start":1,"end":9,"text":..}]}
//!   {"op":"cancel"}
//!   {"op":"mode","mode":"acceptEdits"}
//!   {"op":"permission","id":<id do pedido>,"option":"allow-once"}   (option null = cancelar)
//!
//! stdin fechado (o Neovim saiu) encerra o agente junto.

mod rpc;
mod session;

use rpc::Rpc;
use serde_json::{json, Value};
use session::Session;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// No Windows, `Command::new("claude-agent-acp")` só acha `.exe`; os CLIs do npm
/// são `.cmd`. Procura no PATH com as extensões do PATHEXT.
fn resolve(program: &str) -> PathBuf {
    let p = Path::new(program);
    if !cfg!(windows) || p.extension().is_some() || p.components().count() > 1 {
        return p.to_path_buf();
    }
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let cand = dir.join(format!("{program}{}", ext.to_ascii_lowercase()));
            if cand.is_file() {
                return cand;
            }
        }
    }
    p.to_path_buf()
}

/// Blocos ACP do pedido: o texto, os arquivos citados (resource_link) e os
/// trechos selecionados (com caminho e linhas, para o agente achar o lugar).
fn prompt_blocks(op: &Value) -> Value {
    let mut blocks = vec![json!({ "type": "text", "text": op.get("text").and_then(Value::as_str).unwrap_or("") })];
    for f in op.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let abs = std::fs::canonicalize(f).unwrap_or_else(|_| PathBuf::from(f));
        let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
        let name = Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        blocks.push(json!({ "type": "resource_link", "uri": format!("file:///{}", abs.trim_start_matches('/')), "name": name }));
    }
    for s in op.get("snippets").and_then(Value::as_array).into_iter().flatten() {
        let path = s.get("path").and_then(Value::as_str).unwrap_or("?");
        let (a, b) = (s.get("start").and_then(Value::as_u64).unwrap_or(0), s.get("end").and_then(Value::as_u64).unwrap_or(0));
        let text = s.get("text").and_then(Value::as_str).unwrap_or("");
        blocks.push(json!({ "type": "text", "text": format!("{path} (linhas {a}-{b}):\n```\n{text}\n```") }));
    }
    Value::Array(blocks)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip_while(|a| a != "--").skip(1).collect();
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Value>();

    // eventos -> stdout (uma tarefa só escreve, as linhas nunca se misturam)
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(ev) = events_rx.recv().await {
            let line = format!("{ev}\n");
            if out.write_all(line.as_bytes()).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    });
    let fail = |msg: String| {
        let _ = events_tx.send(json!({ "ev": "error", "message": msg, "fatal": true }));
    };

    let Some((program, rest)) = args.split_first() else {
        fail("uso: astrograph-agent -- <comando do agente> [args...]".into());
        drop(events_tx);
        let _ = writer.await;
        std::process::exit(2);
    };

    let mut cmd = tokio::process::Command::new(resolve(program));
    cmd.args(rest).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: sem janela de console piscando
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            fail(format!("não consegui iniciar {program}: {e}"));
            drop(events_tx);
            let _ = writer.await;
            std::process::exit(1);
        }
    };

    let (rpc, mut incoming) = Rpc::spawn(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let session = Session::new(rpc, events_tx.clone());

    let s = session.clone();
    let agent_loop = tokio::spawn(async move {
        while let Some(msg) = incoming.recv().await {
            s.handle(msg).await;
        }
    });

    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| ".".into());
    if let Err(e) = session.open(&cwd).await {
        fail(format!("o agente não abriu a sessão: {e}"));
    }

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Ok(op) = serde_json::from_str::<Value>(&line) else { continue };
                match op.get("op").and_then(Value::as_str) {
                    Some("prompt") => session.prompt(prompt_blocks(&op)),
                    Some("cancel") => session.cancel(),
                    Some("mode") => if let Some(m) = op.get("mode").and_then(Value::as_str) { session.set_mode(m) },
                    Some("permission") => session.answer_permission(
                        op.get("id").cloned().unwrap_or(Value::Null),
                        op.get("option").and_then(Value::as_str),
                    ),
                    _ => {}
                }
            }
            _ = child.wait() => {
                fail("o agente encerrou".into());
                break;
            }
        }
    }
    agent_loop.abort();
    let _ = child.kill().await;
    drop(session);
    drop(events_tx);
    // pedidos em voo ainda podem segurar o canal: não espera por eles
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), writer).await;
}

#[cfg(test)]
mod tests;
