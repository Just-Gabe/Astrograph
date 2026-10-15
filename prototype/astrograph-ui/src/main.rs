//! Passos 2-4 do spike: modelo de estado da tela, render CPU (softbuffer +
//! atlas de glifos) e input de teclado sobre um Neovim embutido.
//!
//! A UI roda no thread principal (winit). Um runtime tokio num thread separado
//! fala com o `nvim --embed` via msgpack-rpc; o handler atualiza a grade
//! compartilhada e avisa a UI via `EventLoopProxy` para redesenhar.

mod grid;
mod render;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, UiAttachOptions, Value};
use softbuffer::{Context, Surface};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use grid::Grid;
use render::Renderer;

const INITIAL_COLS: usize = 100;
const INITIAL_ROWS: usize = 30;
const FONT_PX: f32 = 18.0;

#[derive(Clone, Copy)]
enum UserEvent {
    Redraw,
    Quit,
}

enum UiCommand {
    Keys(String),
    Resize(i64, i64),
}

#[derive(Clone)]
struct UiHandler {
    grid: Arc<Mutex<Grid>>,
    proxy: EventLoopProxy<UserEvent>,
}

#[async_trait::async_trait]
impl Handler for UiHandler {
    type Writer = Compat<ChildStdin>;

    async fn handle_notify(&self, name: String, args: Vec<Value>, _nvim: Neovim<Self::Writer>) {
        if name != "redraw" {
            return;
        }
        if let Ok(mut grid) = self.grid.lock() {
            grid.apply_redraw(&args);
        }
        let _ = self.proxy.send_event(UserEvent::Redraw);
    }
}

async fn nvim_task(
    grid: Arc<Mutex<Grid>>,
    mut rx: UnboundedReceiver<UiCommand>,
    proxy: EventLoopProxy<UserEvent>,
) {
    let handler = UiHandler {
        grid: Arc::clone(&grid),
        proxy,
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

    let mut opts = UiAttachOptions::new();
    opts.set_linegrid_external(true);
    opts.set_rgb(true);
    nvim.ui_attach(INITIAL_COLS as i64, INITIAL_ROWS as i64, &opts)
        .await
        .expect("ui_attach");

    while let Some(cmd) = rx.recv().await {
        match cmd {
            UiCommand::Keys(keys) => {
                let _ = nvim.input(&keys).await;
            }
            UiCommand::Resize(cols, rows) => {
                let _ = nvim.ui_try_resize(cols, rows).await;
            }
        }
    }
    let _ = nvim.command("qa!").await;
}

struct App {
    grid: Arc<Mutex<Grid>>,
    tx: UnboundedSender<UiCommand>,
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    renderer: Option<Renderer>,
    modifiers: ModifiersState,
    last_grid: (usize, usize),
    start: std::time::Instant,
    frames: u64,
    render_sum: Duration,
    first_paint: Option<Duration>,
    bench: bool,
    proxy: EventLoopProxy<UserEvent>,
}

impl App {
    fn new(grid: Arc<Mutex<Grid>>, tx: UnboundedSender<UiCommand>, proxy: EventLoopProxy<UserEvent>) -> Self {
        App {
            grid,
            tx,
            window: None,
            context: None,
            surface: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            last_grid: (0, 0),
            start: std::time::Instant::now(),
            frames: 0,
            render_sum: Duration::ZERO,
            first_paint: None,
            bench: std::env::var("SPIKE_BENCH").is_ok(),
            proxy,
        }
    }

    fn draw(&mut self) {
        let window = match &self.window {
            Some(window) => window.clone(),
            None => return,
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let surface = match self.surface.as_mut() {
            Some(surface) => surface,
            None => return,
        };
        let renderer = match self.renderer.as_mut() {
            Some(renderer) => renderer,
            None => return,
        };
        surface
            .resize(
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            )
            .expect("resize");
        {
            let render_start = std::time::Instant::now();
            let mut buffer = surface.buffer_mut().expect("buffer_mut");
            let grid = self.grid.lock().unwrap();
            renderer.draw(&mut buffer, size.width, size.height, &grid, true);
            buffer.present().expect("present");
            self.render_sum += render_start.elapsed();
        }

        self.frames += 1;
        if self.first_paint.is_none() {
            let elapsed = self.start.elapsed();
            self.first_paint = Some(elapsed);
            println!("FIRST_PAINT: {} ms desde o inicio", elapsed.as_millis());
        }
        if self.bench && self.frames >= 240 {
            let avg = self.render_sum.as_secs_f64() * 1000.0 / self.frames as f64;
            println!(
                "BENCH: {} frames, render medio {:.2} ms/frame (CPU, {}x{})",
                self.frames, avg, size.width, size.height
            );
            let _ = self.proxy.send_event(UserEvent::Quit);
        }

        let cols = (size.width / renderer.cell_w).max(1) as usize;
        let rows = (size.height / renderer.cell_h).max(1) as usize;
        if (cols, rows) != self.last_grid {
            self.last_grid = (cols, rows);
            let _ = self.tx.send(UiCommand::Resize(cols as i64, rows as i64));
        }
        if self.bench && self.frames < 240 {
            window.request_redraw();
        }
    }

    fn send_keys(&self, keys: String) {
        let _ = self.tx.send(UiCommand::Keys(keys));
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Astrograph (spike UI em Rust)")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
        let window = Rc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        let scale = window.scale_factor() as f32;
        self.renderer = Some(Renderer::new(FONT_PX * scale).expect("renderer"));
        window.request_redraw();
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Redraw => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::Quit => event_loop.exit(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                if event.state == ElementState::Pressed {
                    if let Some(keys) = key_to_input(&event, self.modifiers.control_key()) {
                        self.send_keys(keys);
                    }
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}

fn key_to_input(event: &KeyEvent, ctrl: bool) -> Option<String> {
    match &event.logical_key {
        Key::Named(NamedKey::Escape) => Some("<Esc>".to_string()),
        Key::Named(NamedKey::Enter) => Some("<CR>".to_string()),
        Key::Named(NamedKey::Tab) => Some("<Tab>".to_string()),
        Key::Named(NamedKey::Backspace) => Some("<BS>".to_string()),
        Key::Named(NamedKey::Delete) => Some("<Del>".to_string()),
        Key::Named(NamedKey::ArrowLeft) => Some("<Left>".to_string()),
        Key::Named(NamedKey::ArrowRight) => Some("<Right>".to_string()),
        Key::Named(NamedKey::ArrowUp) => Some("<Up>".to_string()),
        Key::Named(NamedKey::ArrowDown) => Some("<Down>".to_string()),
        Key::Named(NamedKey::Home) => Some("<Home>".to_string()),
        Key::Named(NamedKey::End) => Some("<End>".to_string()),
        Key::Named(NamedKey::PageUp) => Some("<PageUp>".to_string()),
        Key::Named(NamedKey::PageDown) => Some("<PageDown>".to_string()),
        Key::Named(NamedKey::Space) => Some(if ctrl { "<C-Space>" } else { " " }.to_string()),
        Key::Character(text) => {
            if ctrl {
                Some(format!("<C-{}>", text.as_str()))
            } else {
                Some(text.to_string())
            }
        }
        _ => event.text.as_ref().map(|text| text.to_string()),
    }
}

fn main() {
    let grid = Arc::new(Mutex::new(Grid::new(INITIAL_COLS, INITIAL_ROWS)));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiCommand>();

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();

    let grid_for_nvim = Arc::clone(&grid);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime tokio");
        runtime.block_on(nvim_task(grid_for_nvim, rx, proxy));
    });

    if std::env::var("SPIKE_SMOKE").is_ok() || std::env::var("SPIKE_INPUT").is_ok() {
        let grid_for_smoke = Arc::clone(&grid);
        let proxy_smoke = event_loop.create_proxy();
        let tx_smoke = tx.clone();
        let input_test = std::env::var("SPIKE_INPUT").is_ok();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(4000));
            if input_test {
                let _ = tx_smoke.send(UiCommand::Keys(":".to_string()));
                std::thread::sleep(Duration::from_millis(1200));
                let (mode, dump) = {
                    let grid = grid_for_smoke.lock().unwrap();
                    (grid.mode.clone(), grid.to_text())
                };
                println!("INPUT_MODE: {:?}", mode);
                println!("INPUT_OK: {}", if mode.starts_with("cmdline") { "sim" } else { "nao" });
                println!("--- TELA ---\n{}--- FIM ---", dump);
            } else {
                let dump = {
                    let grid = grid_for_smoke.lock().unwrap();
                    format!(
                        "SMOKE_OK: grade {}x{}, cursor {:?}\n--- TELA ---\n{}--- FIM ---",
                        grid.width,
                        grid.height,
                        grid.cursor,
                        grid.to_text()
                    )
                };
                println!("{}", dump);
            }
            let _ = proxy_smoke.send_event(UserEvent::Quit);
        });
    }

    let mut app = App::new(grid, tx, event_loop.create_proxy());
    event_loop.run_app(&mut app).expect("run_app");
    println!("SPIKE_DONE");
}
