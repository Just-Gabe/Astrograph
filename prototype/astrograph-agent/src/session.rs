//! TraduÃ§Ã£o ACP <-> editor.
//!
//! O agente fala ACP (JSON-RPC); o editor recebe eventos simples, um JSON por
//! linha, com o campo `ev`:
//!
//!   ready       sessÃ£o aberta (modos disponÃ­veis e o atual)
//!   text        pedaÃ§o da resposta (`kind`: agent | thought)
//!   tool        ferramenta nova ou atualizada (tÃ­tulo, tipo, estado, locais)
//!   permission  o agente pede para agir; traz o diff proposto e as opÃ§Ãµes
//!   edited      um arquivo foi alterado pelo agente: recarregar e destacar
//!   plan        plano de passos do agente
//!   mode        modo atual mudou
//!   done        a resposta terminou (`stop`)
//!   error       algo falhou (`message`)
//!
//! O Claude Code grava os arquivos direto no disco (nÃ£o usa o fs do cliente),
//! mas pede permissÃ£o com o diff antes e informa o local de cada ferramenta:
//! Ã© isso que deixa o editor mostrar a ediÃ§Ã£o antes e depois de acontecer.

use crate::rpc::{Incoming, Rpc};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type Events = mpsc::UnboundedSender<Value>;

#[derive(Default)]
struct State {
    session_id: Option<String>,
    /// estado acumulado de cada ferramenta (as atualizaÃ§Ãµes vÃªm parciais)
    tools: HashMap<String, Map<String, Value>>,
    /// arquivos jÃ¡ avisados como editados, por ferramenta
    edited: HashMap<String, HashSet<String>>,
    /// pedidos de permissÃ£o ainda sem resposta (id do agente, em JSON)
    permissions: HashSet<String>,
}

#[derive(Clone)]
pub struct Session {
    rpc: Rpc,
    events: Events,
    state: Arc<Mutex<State>>,
}

fn text_of(content: &Value) -> Option<&str> {
    (content.get("type")?.as_str()? == "text").then(|| content.get("text")?.as_str()).flatten()
}

/// Diffs propostos numa ferramenta (`content` com `type: diff`).
fn diffs_of(tool: &Map<String, Value>) -> Vec<Value> {
    let Some(items) = tool.get("content").and_then(Value::as_array) else { return vec![] };
    items
        .iter()
        .filter(|c| c.get("type").and_then(Value::as_str) == Some("diff"))
        .map(|c| json!({ "path": c.get("path"), "old": c.get("oldText"), "new": c.get("newText") }))
        .collect()
}

fn paths_of(tool: &Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let locs = tool.get("locations").and_then(Value::as_array).into_iter().flatten();
    let diffs = tool.get("content").and_then(Value::as_array).into_iter().flatten();
    for v in locs.chain(diffs.filter(|c| c.get("type").and_then(Value::as_str) == Some("diff"))) {
        if let Some(p) = v.get("path").and_then(Value::as_str) {
            if !out.iter().any(|o| o == p) {
                out.push(p.to_owned());
            }
        }
    }
    out
}

impl Session {
    pub fn new(rpc: Rpc, events: Events) -> Self {
        Session { rpc, events, state: Arc::default() }
    }

    fn emit(&self, ev: Value) {
        let _ = self.events.send(ev);
    }

    pub fn error(&self, message: impl Into<String>) {
        self.emit(json!({ "ev": "error", "message": message.into() }));
    }

    /// `initialize` + `session/new`. O cliente anuncia leitura/escrita de
    /// arquivos: se um agente usar o fs do cliente, a escrita vira `edited`.
    pub async fn open(&self, cwd: &str) -> Result<(), String> {
        let caps = json!({ "fs": { "readTextFile": true, "writeTextFile": true }, "terminal": false });
        self.rpc
            .request("initialize", json!({ "protocolVersion": 1, "clientCapabilities": caps }))
            .await
            .map_err(|e| format!("initialize: {e}"))?;
        let res = self
            .rpc
            .request("session/new", json!({ "cwd": cwd, "mcpServers": [] }))
            .await
            .map_err(|e| format!("session/new: {e}"))?;
        let id = res.get("sessionId").and_then(Value::as_str).ok_or("session/new sem sessionId")?;
        self.state.lock().unwrap().session_id = Some(id.to_owned());
        let modes = res.get("modes").cloned().unwrap_or(Value::Null);
        self.emit(json!({
            "ev": "ready",
            "session": id,
            "mode": modes.get("currentModeId"),
            "modes": modes.get("availableModes"),
        }));
        Ok(())
    }

    fn session_id(&self) -> Option<String> {
        self.state.lock().unwrap().session_id.clone()
    }

    /// Manda um pedido e avisa `done` quando a resposta termina.
    pub fn prompt(&self, blocks: Value) {
        let Some(sid) = self.session_id() else { return self.error("sessÃ£o ainda nÃ£o aberta") };
        let me = self.clone();
        tokio::spawn(async move {
            match me.rpc.request("session/prompt", json!({ "sessionId": sid, "prompt": blocks })).await {
                Ok(res) => me.emit(json!({ "ev": "done", "stop": res.get("stopReason"), "usage": res.get("usage") })),
                Err(e) => {
                    me.error(format!("{}", e.get("message").and_then(Value::as_str).unwrap_or("falha no pedido")));
                    me.emit(json!({ "ev": "done", "stop": "error" }));
                }
            }
        });
    }

    /// Interrompe a resposta atual; permissÃµes pendentes saem como canceladas
    /// (o protocolo exige resposta para cada pedido).
    pub fn cancel(&self) {
        let (sid, pending) = {
            let mut st = self.state.lock().unwrap();
            (st.session_id.clone(), st.permissions.drain().collect::<Vec<_>>())
        };
        for id in pending {
            if let Ok(id) = serde_json::from_str::<Value>(&id) {
                self.rpc.respond(id, json!({ "outcome": { "outcome": "cancelled" } }));
            }
        }
        if let Some(sid) = sid {
            self.rpc.notify("session/cancel", json!({ "sessionId": sid }));
        }
    }

    pub fn set_mode(&self, mode: &str) {
        let Some(sid) = self.session_id() else { return };
        let me = self.clone();
        let mode = mode.to_owned();
        tokio::spawn(async move {
            match me.rpc.request("session/set_mode", json!({ "sessionId": sid, "modeId": mode })).await {
                Ok(_) => me.emit(json!({ "ev": "mode", "mode": mode })),
                Err(e) => me.error(format!("nÃ£o consegui trocar o modo: {e}")),
            }
        });
    }

    /// Resposta do editor a um pedido de permissÃ£o (`option` nulo = cancelar).
    pub fn answer_permission(&self, id: Value, option: Option<&str>) {
        if !self.state.lock().unwrap().permissions.remove(&id.to_string()) {
            return; // jÃ¡ respondida (ex.: cancelada)
        }
        let outcome = match option {
            Some(opt) => json!({ "outcome": "selected", "optionId": opt }),
            None => json!({ "outcome": "cancelled" }),
        };
        self.rpc.respond(id, json!({ "outcome": outcome }));
    }

    /// Processa uma mensagem que veio do agente.
    pub async fn handle(&self, msg: Incoming) {
        match msg {
            Incoming::Notification { method, params } if method == "session/update" => {
                if let Some(update) = params.get("update") {
                    self.on_update(update);
                }
            }
            Incoming::Notification { .. } => {}
            Incoming::Request { id, method, params } => match method.as_str() {
                "session/request_permission" => self.on_permission(id, &params),
                "fs/read_text_file" => self.on_read(id, &params).await,
                "fs/write_text_file" => self.on_write(id, &params).await,
                _ => self.rpc.respond_error(id, -32601, "mÃ©todo nÃ£o suportado pelo Astrograph"),
            },
        }
    }

    fn on_update(&self, u: &Value) {
        let kind = u.get("sessionUpdate").and_then(Value::as_str).unwrap_or("");
        match kind {
            "agent_message_chunk" | "agent_thought_chunk" => {
                if let Some(t) = u.get("content").and_then(text_of) {
                    let k = if kind == "agent_message_chunk" { "agent" } else { "thought" };
                    self.emit(json!({ "ev": "text", "kind": k, "text": t }));
                }
            }
            "tool_call" | "tool_call_update" => self.on_tool(u),
            "plan" => self.emit(json!({ "ev": "plan", "entries": u.get("entries") })),
            "current_mode_update" => self.emit(json!({ "ev": "mode", "mode": u.get("currentModeId") })),
            _ => {}
        }
    }

    fn on_tool(&self, u: &Value) {
        let Some(id) = u.get("toolCallId").and_then(Value::as_str) else { return };
        let Some(fields) = u.as_object() else { return };
        let (tool, newly_edited) = {
            let mut st = self.state.lock().unwrap();
            let tool = st.tools.entry(id.to_owned()).or_default();
            for (k, v) in fields {
                // atualizaÃ§Ã£o parcial: sÃ³ sobrescreve o que veio preenchido
                let empty = v.is_null() || v.as_array().is_some_and(Vec::is_empty);
                if k != "sessionUpdate" && !empty {
                    tool.insert(k.clone(), v.clone());
                }
            }
            let tool = tool.clone();
            let done = tool.get("status").and_then(Value::as_str) == Some("completed");
            let is_edit = matches!(tool.get("kind").and_then(Value::as_str), Some("edit" | "delete" | "move"));
            let mut newly = Vec::new();
            if done && is_edit {
                let seen = st.edited.entry(id.to_owned()).or_default();
                for p in paths_of(&tool) {
                    if seen.insert(p.clone()) {
                        newly.push(p);
                    }
                }
            }
            (tool, newly)
        };
        self.emit(json!({
            "ev": "tool",
            "id": id,
            "title": tool.get("title"),
            "kind": tool.get("kind"),
            "status": tool.get("status"),
            "locations": tool.get("locations"),
        }));
        for path in newly_edited {
            self.emit(json!({ "ev": "edited", "path": path, "tool": id }));
        }
    }

    fn on_permission(&self, id: Value, params: &Value) {
        let tool = params.get("toolCall").and_then(Value::as_object).cloned().unwrap_or_default();
        self.state.lock().unwrap().permissions.insert(id.to_string());
        self.emit(json!({
            "ev": "permission",
            "id": id,
            "tool": tool.get("toolCallId"),
            "title": tool.get("title"),
            "kind": tool.get("kind"),
            "locations": tool.get("locations"),
            "diffs": diffs_of(&tool),
            "options": params.get("options"),
        }));
    }

    async fn on_read(&self, id: Value, params: &Value) {
        let Some(path) = params.get("path").and_then(Value::as_str) else {
            return self.rpc.respond_error(id, -32602, "path ausente");
        };
        match tokio::fs::read_to_string(path).await {
            Ok(text) => {
                let line = params.get("line").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
                let limit = params.get("limit").and_then(Value::as_u64).map(|l| l as usize);
                let content = if line == 1 && limit.is_none() {
                    text
                } else {
                    let it = text.split_inclusive('\n').skip(line - 1);
                    match limit {
                        Some(n) => it.take(n).collect(),
                        None => it.collect(),
                    }
                };
                self.rpc.respond(id, json!({ "content": content }));
            }
            Err(e) => self.rpc.respond_error(id, -32002, &format!("{path}: {e}")),
        }
    }

    async fn on_write(&self, id: Value, params: &Value) {
        let (Some(path), Some(content)) = (
            params.get("path").and_then(Value::as_str),
            params.get("content").and_then(Value::as_str),
        ) else {
            return self.rpc.respond_error(id, -32602, "path/content ausentes");
        };
        if let Some(dir) = std::path::Path::new(path).parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        match tokio::fs::write(path, content).await {
            Ok(()) => {
                self.rpc.respond(id, Value::Null);
                self.emit(json!({ "ev": "edited", "path": path }));
            }
            Err(e) => self.rpc.respond_error(id, -32003, &format!("{path}: {e}")),
        }
    }
}
