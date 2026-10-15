//! Agente falso por um duplex, repetindo a sequência real do claude-agent-acp
//! (capturada numa sonda: ler, pedir permissão com diff, editar, responder).

use crate::rpc::Rpc;
use crate::session::Session;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf};
use tokio::sync::mpsc;
use tokio::time::timeout;

struct FakeAgent {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    w: WriteHalf<DuplexStream>,
}

impl FakeAgent {
    async fn recv(&mut self) -> Value {
        let line = timeout(Duration::from_secs(2), self.lines.next_line()).await.unwrap().unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }
    async fn send(&mut self, v: Value) {
        self.w.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    }
    async fn update(&mut self, u: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": u } })).await;
    }
    /// Responde ao próximo pedido do cliente, conferindo o método.
    async fn reply(&mut self, method: &str, result: Value) -> Value {
        let req = self.recv().await;
        assert_eq!(req["method"], method);
        self.send(json!({ "jsonrpc": "2.0", "id": req["id"], "result": result })).await;
        req
    }
}

fn setup() -> (Session, FakeAgent, mpsc::UnboundedReceiver<Value>) {
    let (client_io, agent_io) = duplex(1 << 16);
    let (cr, cw) = split(client_io);
    let (rpc, mut incoming) = Rpc::spawn(cr, cw);
    let (tx, rx) = mpsc::unbounded_channel();
    let session = Session::new(rpc, tx);
    let s = session.clone();
    tokio::spawn(async move {
        while let Some(m) = incoming.recv().await {
            s.handle(m).await;
        }
    });
    let (ar, aw) = split(agent_io);
    (session, FakeAgent { lines: BufReader::new(ar).lines(), w: aw }, rx)
}

async fn next_ev(rx: &mut mpsc::UnboundedReceiver<Value>) -> Value {
    timeout(Duration::from_secs(2), rx.recv()).await.expect("evento não chegou").unwrap()
}

async fn open(session: &Session, agent: &mut FakeAgent, rx: &mut mpsc::UnboundedReceiver<Value>) {
    let s = session.clone();
    let opening = tokio::spawn(async move { s.open("C:/proj").await });
    let init = agent.reply("initialize", json!({ "protocolVersion": 1 })).await;
    assert_eq!(init["params"]["clientCapabilities"]["fs"]["writeTextFile"], true);
    let new = agent
        .reply("session/new", json!({ "sessionId": "s1", "modes": { "currentModeId": "default", "availableModes": [{ "id": "default" }, { "id": "acceptEdits" }] } }))
        .await;
    assert_eq!(new["params"]["cwd"], "C:/proj");
    opening.await.unwrap().unwrap();
    let ready = next_ev(rx).await;
    assert_eq!(ready["ev"], "ready");
    assert_eq!(ready["mode"], "default");
}

#[tokio::test]
async fn edicao_com_permissao_vira_eventos_do_editor() {
    let (session, mut agent, mut rx) = setup();
    open(&session, &mut agent, &mut rx).await;

    session.prompt(json!([{ "type": "text", "text": "troque" }]));
    let prompt = agent.recv().await;
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(prompt["params"]["sessionId"], "s1");

    // ferramenta nasce sem local; o local chega numa atualização parcial
    agent.update(json!({ "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit", "kind": "edit", "status": "pending", "locations": [] })).await;
    let ev = next_ev(&mut rx).await;
    assert_eq!((ev["ev"].as_str(), ev["kind"].as_str()), (Some("tool"), Some("edit")));
    agent.update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": "t1", "title": "Edit a.txt", "locations": [{ "path": "C:/proj/a.txt" }] })).await;
    let ev = next_ev(&mut rx).await;
    assert_eq!(ev["title"], "Edit a.txt");
    assert_eq!(ev["kind"], "edit", "kind da primeira mensagem se mantém");
    assert_eq!(ev["locations"][0]["path"], "C:/proj/a.txt");

    // pedido de permissão com o diff
    agent
        .send(json!({ "jsonrpc": "2.0", "id": 7, "method": "session/request_permission", "params": {
            "sessionId": "s1",
            "toolCall": { "toolCallId": "t1", "title": "Edit a.txt", "kind": "edit",
                "content": [{ "type": "diff", "path": "C:/proj/a.txt", "oldText": "linha dois", "newText": "linha 2" }],
                "locations": [{ "path": "C:/proj/a.txt" }] },
            "options": [{ "optionId": "allow-once", "name": "Yes", "kind": "allow_once" }, { "optionId": "reject", "name": "No", "kind": "reject_once" }]
        } }))
        .await;
    let perm = next_ev(&mut rx).await;
    assert_eq!(perm["ev"], "permission");
    assert_eq!(perm["diffs"][0]["old"], "linha dois");
    assert_eq!(perm["diffs"][0]["new"], "linha 2");
    assert_eq!(perm["options"][1]["optionId"], "reject");

    session.answer_permission(perm["id"].clone(), Some("allow-once"));
    let answer = agent.recv().await;
    assert_eq!(answer["id"], 7);
    assert_eq!(answer["result"]["outcome"], json!({ "outcome": "selected", "optionId": "allow-once" }));

    // responder de novo (clique duplo) não manda segunda resposta
    session.answer_permission(json!(7), Some("allow-once"));

    // concluída: avisa a edição uma vez só, mesmo com atualização repetida
    for _ in 0..2 {
        agent.update(json!({ "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed" })).await;
    }
    // o texto serve de marcador: conta todos os `edited` que vieram antes dele
    agent.update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Feito." } })).await;
    let mut edited = 0;
    loop {
        let ev = next_ev(&mut rx).await;
        if ev["ev"] == "edited" {
            assert_eq!(ev["path"], "C:/proj/a.txt");
            edited += 1;
        }
        if ev["ev"] == "text" {
            assert_eq!(ev["text"], "Feito.");
            break;
        }
    }
    assert_eq!(edited, 1);

    agent.send(json!({ "jsonrpc": "2.0", "id": prompt["id"], "result": { "stopReason": "end_turn" } })).await;
    let ev = next_ev(&mut rx).await;
    assert_eq!((ev["ev"].as_str(), ev["stop"].as_str()), (Some("done"), Some("end_turn")));
}

#[tokio::test]
async fn cancelar_responde_permissoes_pendentes() {
    let (session, mut agent, mut rx) = setup();
    open(&session, &mut agent, &mut rx).await;

    agent
        .send(json!({ "jsonrpc": "2.0", "id": "p-9", "method": "session/request_permission", "params": {
            "sessionId": "s1", "toolCall": { "toolCallId": "t2", "title": "Bash" }, "options": [] } }))
        .await;
    assert_eq!(next_ev(&mut rx).await["ev"], "permission");

    session.cancel();
    let first = agent.recv().await;
    let second = agent.recv().await;
    let (resp, cancel) = if first.get("result").is_some() { (first, second) } else { (second, first) };
    assert_eq!(resp["id"], "p-9");
    assert_eq!(resp["result"]["outcome"]["outcome"], "cancelled");
    assert_eq!(cancel["method"], "session/cancel");
}

#[tokio::test]
async fn fs_do_cliente_le_por_linhas_e_escreve_avisando() {
    let (session, mut agent, mut rx) = setup();
    open(&session, &mut agent, &mut rx).await;
    let dir = std::env::temp_dir().join(format!("astrograph-agent-test-{}", std::process::id()));
    let file = dir.join("sub").join("b.txt");
    let path = file.to_string_lossy().into_owned();

    agent.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "fs/write_text_file", "params": { "sessionId": "s1", "path": path, "content": "a\nb\nc\n" } })).await;
    assert_eq!(agent.recv().await["result"], Value::Null);
    let ev = next_ev(&mut rx).await;
    assert_eq!((ev["ev"].as_str(), ev["path"].as_str()), (Some("edited"), Some(path.as_str())));

    agent.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "fs/read_text_file", "params": { "sessionId": "s1", "path": path, "line": 2, "limit": 1 } })).await;
    assert_eq!(agent.recv().await["result"]["content"], "b\n");

    agent.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "terminal/create", "params": {} })).await;
    assert_eq!(agent.recv().await["error"]["code"], -32601);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn agente_que_morre_nao_trava_pedido() {
    let (client_io, agent_io) = duplex(1024);
    let (cr, cw) = split(client_io);
    let (rpc, _incoming) = Rpc::spawn(cr, cw);
    drop(agent_io);
    let res = timeout(Duration::from_secs(2), rpc.request("initialize", json!({}))).await.unwrap();
    assert!(res.is_err());
}

#[test]
fn blocos_do_pedido_trazem_arquivo_e_trecho() {
    let op = json!({ "text": "explique", "files": ["Cargo.toml"], "snippets": [{ "path": "src/x.rs", "start": 3, "end": 5, "text": "fn x() {}" }] });
    let b = crate::prompt_blocks(&op);
    assert_eq!(b[0]["text"], "explique");
    assert_eq!(b[1]["type"], "resource_link");
    let uri = b[1]["uri"].as_str().unwrap();
    assert!(uri.starts_with("file:///") && uri.ends_with("/Cargo.toml") && !uri.contains('\\'), "{uri}");
    assert!(b[2]["text"].as_str().unwrap().starts_with("src/x.rs (linhas 3-5)"));
}
