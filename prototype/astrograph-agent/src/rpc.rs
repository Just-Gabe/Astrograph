//! JSON-RPC 2.0 por linhas (o transporte do ACP): uma mensagem por linha, nos
//! dois sentidos. Genérico em leitor/escritor para os testes usarem um duplex.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

/// O que chega do outro lado e não é resposta a um pedido nosso.
#[derive(Debug)]
pub enum Incoming {
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, Value>>>>>;

#[derive(Clone)]
pub struct Rpc {
    tx: mpsc::UnboundedSender<String>,
    pending: Pending,
    next: Arc<AtomicU64>,
}

impl Rpc {
    /// Liga o RPC a um par leitor/escritor. As mensagens que não são respostas
    /// saem no receptor; ele fecha quando o outro lado fecha a saída.
    pub fn spawn<R, W>(reader: R, mut writer: W) -> (Rpc, mpsc::UnboundedReceiver<Incoming>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::default();

        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if writer.write_all(line.as_bytes()).await.is_err()
                    || writer.write_all(b"\n").await.is_err()
                    || writer.flush().await.is_err()
                {
                    break;
                }
            }
        });

        let pend = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
                match (method, msg.get("id")) {
                    (Some(method), Some(id)) => {
                        let params = msg.get("params").cloned().unwrap_or(Value::Null);
                        let _ = in_tx.send(Incoming::Request { id: id.clone(), method, params });
                    }
                    (Some(method), None) => {
                        let params = msg.get("params").cloned().unwrap_or(Value::Null);
                        let _ = in_tx.send(Incoming::Notification { method, params });
                    }
                    (None, Some(id)) => {
                        let Some(id) = id.as_u64() else { continue };
                        if let Some(waiter) = pend.lock().unwrap().remove(&id) {
                            let res = match msg.get("error") {
                                Some(err) => Err(err.clone()),
                                None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                            };
                            let _ = waiter.send(res);
                        }
                    }
                    (None, None) => {}
                }
            }
            // o outro lado fechou: quem espera resposta recebe erro em vez de travar
            for (_, waiter) in pend.lock().unwrap().drain() {
                let _ = waiter.send(Err(json!({ "message": "o agente encerrou" })));
            }
        });

        (Rpc { tx, pending, next: Arc::new(AtomicU64::new(1)) }, in_rx)
    }

    fn send(&self, msg: Value) {
        let _ = self.tx.send(msg.to_string());
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        rx.await.unwrap_or_else(|_| Err(json!({ "message": "o agente encerrou" })))
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    pub fn respond(&self, id: Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }));
    }
}
