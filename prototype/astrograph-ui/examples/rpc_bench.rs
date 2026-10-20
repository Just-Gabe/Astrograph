//! Mede o custo do RPC entre o Rust e um `nvim --embed`, para decidir que
//! servicos do editor podem sair do processo do Neovim (realce, indent, LSP).
//!
//!   cargo run --release --example rpc_bench
//!
//! Imprime mediana e p99 de:
//! - ida e volta de uma chamada simples (`nvim_get_current_line`);
//! - 200 extmarks de realce: uma chamada atomica x 200 chamadas avulsas;
//! - ciclo de uma tecla: o Rust manda a tecla, o nvim avisa a mudanca por
//!   `rpcnotify` (TextChangedI), o Rust devolve os highlights da linha.

use std::time::{Duration, Instant};

use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, Value};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

const ROUNDS: usize = 500;
const MARKS: usize = 200;

#[derive(Clone)]
struct Bench {
    changed: UnboundedSender<Instant>,
}

#[async_trait::async_trait]
impl Handler for Bench {
    type Writer = Compat<ChildStdin>;

    async fn handle_notify(&self, name: String, _args: Vec<Value>, _nvim: Neovim<Self::Writer>) {
        if name == "bench_changed" {
            let _ = self.changed.send(Instant::now());
        }
    }
}

fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    println!(
        "{label:<44} mediana {:>8.1} us   p99 {:>8.1} us   (n={})",
        us(at(0.5)),
        us(at(0.99)),
        samples.len()
    );
}

fn mark_args(ns: i64, line: i64) -> Vec<Value> {
    // um token por "coluna": o tamanho tipico de uma linha realcada
    (0..MARKS as i64)
        .map(|i| {
            let col = i % 40;
            Value::Array(vec![
                Value::from("nvim_buf_set_extmark"),
                Value::Array(vec![
                    Value::from(0),
                    Value::from(ns),
                    Value::from(line),
                    Value::from(col),
                    Value::Map(vec![
                        (Value::from("end_col"), Value::from(col + 1)),
                        (Value::from("hl_group"), Value::from("Keyword")),
                    ]),
                ]),
            ])
        })
        .collect()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let (tx, mut rx) = unbounded_channel();
    let nvim_bin = std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".to_string());
    let mut cmd = Command::new(&nvim_bin);
    cmd.args(["--embed", "--headless", "--clean", "-n"]);
    let (nvim, _io, _child) = create::new_child_cmd(&mut cmd, Bench { changed: tx })
        .await
        .expect("nvim --embed");
    let chan = match nvim.get_api_info().await.expect("api_info")[0] {
        Value::Integer(ref n) => n.as_i64().unwrap(),
        _ => panic!("canal"),
    };
    let lines: Vec<String> = (0..2000).map(|i| format!("local x{i} = function(a, b) return a + b end")).collect();
    nvim.get_current_buf().await.unwrap().set_lines(0, -1, false, lines).await.unwrap();
    let ns = nvim.create_namespace("bench").await.unwrap();

    // 1) ida e volta simples
    let mut rt = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let t = Instant::now();
        nvim.get_current_line().await.unwrap();
        rt.push(t.elapsed());
    }
    report("ida e volta (get_current_line)", rt);

    // 2) extmarks: lote atomico x avulsos
    let mut atomic = Vec::new();
    let mut single = Vec::new();
    for r in 0..50 {
        let line = (r * 7) as i64;
        let t = Instant::now();
        nvim.call_atomic(mark_args(ns, line)).await.unwrap();
        atomic.push(t.elapsed());
        let buf = nvim.get_current_buf().await.unwrap();
        let t = Instant::now();
        for i in 0..MARKS as i64 {
            let col = i % 40;
            buf.set_extmark(ns, line + 1, col, vec![
                (Value::from("end_col"), Value::from(col + 1)),
                (Value::from("hl_group"), Value::from("Keyword")),
            ])
            .await
            .unwrap();
        }
        single.push(t.elapsed());
    }
    report(&format!("{MARKS} extmarks numa chamada atomica"), atomic);
    report(&format!("{MARKS} extmarks em chamadas avulsas"), single);

    // 3) ciclo de uma tecla no modo de insercao
    nvim.exec_lua(
        &format!(
            "vim.api.nvim_create_autocmd('TextChangedI', {{ callback = function() \
             vim.rpcnotify({chan}, 'bench_changed', vim.b.changedtick) end }})"
        ),
        vec![],
    )
    .await
    .unwrap();
    nvim.command("normal! G").await.unwrap();
    nvim.input("A").await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    while rx.try_recv().is_ok() {}
    let mut to_notify = Vec::new();
    let mut full = Vec::new();
    for _ in 0..ROUNDS {
        let t = Instant::now();
        nvim.input("x").await.unwrap();
        let seen = rx.recv().await.unwrap();
        to_notify.push(seen - t);
        let line = nvim.call_function("line", vec![Value::from(".")]).await.unwrap();
        let line = line.as_i64().unwrap() - 1;
        let mut batch = vec![Value::Array(vec![
            Value::from("nvim_buf_clear_namespace"),
            Value::Array(vec![Value::from(0), Value::from(ns), Value::from(line), Value::from(line + 1)]),
        ])];
        batch.extend(mark_args(ns, line).into_iter().take(20));
        nvim.call_atomic(batch).await.unwrap();
        full.push(t.elapsed());
    }
    report("tecla -> aviso no Rust (TextChangedI)", to_notify);
    report("tecla -> aviso -> 20 highlights aplicados", full);
    let _ = nvim.command("qa!").await;
}
