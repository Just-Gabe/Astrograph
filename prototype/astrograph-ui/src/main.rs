//! Passo 1 do spike: prova de protocolo.
//!
//! Spawna `nvim --embed`, faz `ui_attach` com `ext_linegrid` e reconstroi a
//! grade a partir dos eventos `redraw`, imprimindo-a como texto. Prova que a
//! config e o tema do Astrograph carregam pelo motor embutido.
//!
//! Formato do `redraw` (msgpack): o parametro e um array de entradas no formato
//! `[nome, payload1, payload2, ...]`, onde eventos consecutivos de mesmo nome
//! sao agrupados num unico item.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, UiAttachOptions, Value};
use tokio::process::{ChildStdin, Command};

const COLS: usize = 100;
const ROWS: usize = 30;

struct Screen {
    width: usize,
    height: usize,
    cells: Vec<Vec<char>>,
}

impl Screen {
    fn new(width: usize, height: usize) -> Self {
        Screen {
            width,
            height,
            cells: vec![vec![' '; width]; height],
        }
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells = vec![vec![' '; width]; height];
    }

    fn clear(&mut self) {
        for row in self.cells.iter_mut() {
            for cell in row.iter_mut() {
                *cell = ' ';
            }
        }
    }

    fn put(&mut self, row: usize, col: usize, ch: char) {
        if row < self.height && col < self.width {
            self.cells[row][col] = ch;
        }
    }

    fn print(&self) {
        for row in &self.cells {
            let line: String = row.iter().collect();
            println!("{}", line.trim_end());
        }
    }
}

#[derive(Clone)]
struct UiHandler {
    screen: Arc<Mutex<Screen>>,
    redraws: Arc<AtomicU64>,
    gridlines: Arc<AtomicU64>,
}

impl UiHandler {
    fn apply_redraw(&self, events: &[Value]) {
        let mut screen = self.screen.lock().unwrap();
        for event in events {
            let parts = match event.as_array() {
                Some(parts) => parts,
                None => continue,
            };
            let name = parts.first().and_then(|v| v.as_str()).unwrap_or("");
            for payload in parts.iter().skip(1) {
                let args = match payload.as_array() {
                    Some(args) => args,
                    None => continue,
                };
                self.apply_event(&mut screen, name, args);
            }
        }
    }

    fn apply_event(&self, screen: &mut Screen, name: &str, args: &[Value]) {
        match name {
            "grid_resize" => {
                if args.first().and_then(|v| v.as_i64()) != Some(1) {
                    return;
                }
                let width = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let height = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if width > 0 && height > 0 {
                    screen.resize(width, height);
                }
            }
            "grid_clear" => screen.clear(),
            "grid_line" => {
                self.gridlines.fetch_add(1, Ordering::SeqCst);
                let row = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let mut col = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if let Some(Value::Array(cells)) = args.get(3) {
                    for cell in cells {
                        let cell = match cell.as_array() {
                            Some(cell) => cell,
                            None => continue,
                        };
                        let text = cell.first().and_then(|v| v.as_str()).unwrap_or(" ");
                        let repeat = cell.get(2).and_then(|v| v.as_u64()).unwrap_or(1);
                        let ch = text.chars().next().unwrap_or(' ');
                        for _ in 0..repeat {
                            screen.put(row, col, ch);
                            col += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[async_trait::async_trait]
impl Handler for UiHandler {
    type Writer = Compat<ChildStdin>;

    async fn handle_notify(&self, name: String, args: Vec<Value>, _nvim: Neovim<Self::Writer>) {
        if name != "redraw" {
            return;
        }
        self.apply_redraw(&args);
        self.redraws.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::main]
async fn main() {
    let started = std::time::Instant::now();
    let screen = Arc::new(Mutex::new(Screen::new(COLS, ROWS)));
    let redraws = Arc::new(AtomicU64::new(0));
    let gridlines = Arc::new(AtomicU64::new(0));
    let handler = UiHandler {
        screen: Arc::clone(&screen),
        redraws: Arc::clone(&redraws),
        gridlines: Arc::clone(&gridlines),
    };

    let nvim_bin = std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".to_string());
    let (nvim, io_handler, _child) = create::new_child_cmd(
        Command::new(&nvim_bin).arg("--embed").arg("--headless"),
        handler,
    )
    .await
    .expect("spawn nvim --embed");
    tokio::spawn(async move {
        let _ = io_handler.await;
    });

    let info = nvim.get_api_info().await.expect("get_api_info");
    println!("EMBED_OK: canal={}", info[0]);

    let mut opts = UiAttachOptions::new();
    opts.set_linegrid_external(true);
    opts.set_rgb(true);
    nvim.ui_attach(COLS as i64, ROWS as i64, &opts)
        .await
        .expect("ui_attach");

    let mut last = 0u64;
    let mut stable_ms = 0u64;
    let mut total_ms = 0u64;
    while total_ms < 8_000 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        total_ms += 100;
        let now = redraws.load(Ordering::SeqCst);
        if now == last {
            stable_ms += 100;
            if now > 0 && stable_ms >= 400 {
                break;
            }
        } else {
            last = now;
            stable_ms = 0;
        }
    }

    println!(
        "PROTO_OK: {} redraws, {} grid_line(s), {} ms desde o spawn",
        last,
        gridlines.load(Ordering::SeqCst),
        started.elapsed().as_millis()
    );

    match nvim.eval("get(g:, 'colors_name', 'NENHUM')").await {
        Ok(value) => println!("CONFIG_CHECK: colors_name = {:?}", value),
        Err(err) => println!("CONFIG_CHECK: falhou ({:?})", err),
    }

    println!("--- TELA ---");
    {
        let screen = screen.lock().unwrap();
        screen.print();
    }
    println!("--- FIM ---");

    let _ = nvim.command("qa!").await;
}
