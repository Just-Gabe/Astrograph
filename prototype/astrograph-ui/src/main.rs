//! UI em Rust do Astrograph sobre um Neovim embutido.
//!
//! A UI roda no thread principal (winit). Um runtime tokio num thread separado
//! fala com o `nvim --embed` via msgpack-rpc; o handler atualiza a grade
//! compartilhada e avisa a UI via `EventLoopProxy` para redesenhar.

mod config;
mod grid;
mod render;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, UiAttachOptions, Value};
use softbuffer::{Context, Surface};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Icon, Window, WindowId};

use config::Config;
use grid::Grid;
use render::Renderer;

const INITIAL_COLS: usize = 100;
const INITIAL_ROWS: usize = 30;
const ZOOM_STEP: f32 = 1.0;
const ZOOM_MIN: f32 = 8.0;
const ZOOM_MAX: f32 = 48.0;

#[derive(Clone, Copy)]
enum UserEvent {
    Redraw,
    Quit,
}

enum UiCommand {
    Keys(String),
    Resize(i64, i64),
    Mouse {
        button: String,
        action: String,
        row: i64,
        col: i64,
    },
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
            UiCommand::Mouse {
                button,
                action,
                row,
                col,
            } => {
                let _ = nvim.input_mouse(&button, &action, "", 0, row, col).await;
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
    start: Instant,
    frames: u64,
    render_sum: Duration,
    first_paint: Option<Duration>,
    bench: bool,
    proxy: EventLoopProxy<UserEvent>,
    base_font: f32,
    blink: bool,
    blink_ms: u64,
    cursor_on: bool,
    next_blink: Instant,
    focused: bool,
    cursor_pos: (f64, f64),
    fullscreen: bool,
}

impl App {
    fn new(
        grid: Arc<Mutex<Grid>>,
        tx: UnboundedSender<UiCommand>,
        proxy: EventLoopProxy<UserEvent>,
        cfg: &Config,
    ) -> Self {
        let now = Instant::now();
        App {
            grid,
            tx,
            window: None,
            context: None,
            surface: None,
            renderer: None,
            modifiers: ModifiersState::empty(),
            last_grid: (0, 0),
            start: now,
            frames: 0,
            render_sum: Duration::ZERO,
            first_paint: None,
            bench: std::env::var("SPIKE_BENCH").is_ok(),
            proxy,
            base_font: cfg.font.size,
            blink: cfg.cursor.blink,
            blink_ms: cfg.cursor.blink_ms.max(100),
            cursor_on: true,
            next_blink: now + Duration::from_millis(cfg.cursor.blink_ms.max(100)),
            focused: true,
            cursor_pos: (0.0, 0.0),
            fullscreen: false,
        }
    }

    fn cell_size(&self) -> (f64, f64) {
        match &self.renderer {
            Some(renderer) => (renderer.cell_w as f64, renderer.cell_h as f64),
            None => (1.0, 1.0),
        }
    }

    fn mouse_cell(&self) -> (i64, i64) {
        let (cw, ch) = self.cell_size();
        let col = (self.cursor_pos.0 / cw).floor() as i64;
        let row = (self.cursor_pos.1 / ch).floor() as i64;
        let grid = self.grid.lock().unwrap();
        (
            row.clamp(0, grid.height as i64 - 1),
            col.clamp(0, grid.width as i64 - 1),
        )
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
            let render_start = Instant::now();
            let mut buffer = surface.buffer_mut().expect("buffer_mut");
            let grid = self.grid.lock().unwrap();
            renderer.draw(&mut buffer, size.width, size.height, &grid, self.cursor_on);
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

    fn apply_zoom(&mut self, size: f32) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_size(size);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(window) = &self.window {
            self.fullscreen = !self.fullscreen;
            let mode = if self.fullscreen {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            };
            window.set_fullscreen(mode);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let cfg = Config::load();
        let (config, path) = cfg;
        if let Some(path) = &path {
            println!("CONFIG: {:?}", path);
        } else {
            println!("CONFIG: usando defaults");
        }
        let attrs = Window::default_attributes()
            .with_title(config.window.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                config.window.width,
                config.window.height,
            ))
            .with_window_icon(Some(make_icon()));
        let window = Rc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        let scale = window.scale_factor() as f32;
        self.renderer = Some(
            Renderer::new(&config.font).expect("renderer"),
        );
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_size(self.base_font * scale);
        }
        window.set_cursor(CursorIcon::Text);
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.blink && self.focused {
            let now = Instant::now();
            if now >= self.next_blink {
                self.cursor_on = !self.cursor_on;
                self.next_blink = now + Duration::from_millis(self.blink_ms);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_blink));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
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
            WindowEvent::Focused(focused) => {
                self.focused = focused;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x, position.y);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (_, row, col) = self.mouse_event_pos();
                let dir = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                };
                let action = if dir > 0.0 {
                    "up"
                } else if dir < 0.0 {
                    "down"
                } else {
                    return;
                };
                let _ = self.tx.send(UiCommand::Mouse {
                    button: "wheel".to_string(),
                    action: action.to_string(),
                    row,
                    col,
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let name = match button {
                    MouseButton::Left => "left",
                    MouseButton::Right => "right",
                    MouseButton::Middle => "middle",
                    _ => return,
                };
                let action = if state == ElementState::Pressed {
                    "press"
                } else {
                    "release"
                };
                let (_, row, col) = self.mouse_event_pos();
                let _ = self.tx.send(UiCommand::Mouse {
                    button: name.to_string(),
                    action: action.to_string(),
                    row,
                    col,
                });
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                self.cursor_on = true;
                self.next_blink = Instant::now() + Duration::from_millis(self.blink_ms);
                if self.handle_shortcut(&event) {
                    return;
                }
                if let Some(keys) = key_to_input(&event, self.modifiers.control_key()) {
                    self.send_keys(keys);
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}

impl App {
    fn mouse_event_pos(&self) -> (i64, i64, i64) {
        let (row, col) = self.mouse_cell();
        (0, row, col)
    }

    /// Retorna true se o atalho foi consumido pela UI (nao vai para o Neovim).
    fn handle_shortcut(&mut self, event: &KeyEvent) -> bool {
        if matches!(event.logical_key, Key::Named(NamedKey::F11)) {
            self.toggle_fullscreen();
            return true;
        }
        if !self.modifiers.control_key() {
            return false;
        }
        let current = match &self.renderer {
            Some(renderer) => renderer.font_size(),
            None => return false,
        };
        let scale = match &self.window {
            Some(window) => window.scale_factor() as f32,
            None => 1.0,
        };
        match &event.logical_key {
            Key::Character(text) => match text.as_str() {
                "+" | "=" => {
                    self.apply_zoom((current + ZOOM_STEP * scale).min(ZOOM_MAX * scale));
                    true
                }
                "-" => {
                    self.apply_zoom((current - ZOOM_STEP * scale).max(ZOOM_MIN * scale));
                    true
                }
                "0" => {
                    self.apply_zoom(self.base_font * scale);
                    true
                }
                _ => false,
            },
            _ => false,
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

fn make_icon() -> Icon {
    const N: u32 = 64;
    let bg = (0x1a_u32, 0x1d_u32, 0x23_u32);
    let accent = (0x7d_u32, 0xd3_u32, 0xfc_u32);
    let cyan = (0x5d_u32, 0xe4_u32, 0xd0_u32);
    let radius = 14.0_f32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let dx = (fx - N as f32 / 2.0).abs();
            let dy = (fy - N as f32 / 2.0).abs();
            let corner = radius - 0.5;
            let inside = dx <= N as f32 / 2.0 - radius
                || dy <= N as f32 / 2.0 - radius
                || {
                    let cxd = (dx - (N as f32 / 2.0 - radius)).max(0.0);
                    let cyd = (dy - (N as f32 / 2.0 - radius)).max(0.0);
                    (cxd * cxd + cyd * cyd).sqrt() <= corner
                };
            let idx = ((y * N + x) * 4) as usize;
            if !inside {
                continue;
            }
            let u = (fx - N as f32 / 2.0) / (N as f32 / 2.0);
            let v = (fy - N as f32 / 2.0) / (N as f32 / 2.0);
            let star = u.abs().powf(0.55) + v.abs().powf(0.55);
            let star_alpha = ((1.05 - star) * 6.0).clamp(0.0, 1.0);
            let t = ((v + 1.0) / 2.0).clamp(0.0, 1.0);
            let sr = (accent.0 as f32 * (1.0 - t) + cyan.0 as f32 * t) as u32;
            let sg = (accent.1 as f32 * (1.0 - t) + cyan.1 as f32 * t) as u32;
            let sb = (accent.2 as f32 * (1.0 - t) + cyan.2 as f32 * t) as u32;
            let (r, g, b) = if star_alpha > 0.0 {
                (
                    (bg.0 as f32 * (1.0 - star_alpha) + sr as f32 * star_alpha) as u32,
                    (bg.1 as f32 * (1.0 - star_alpha) + sg as f32 * star_alpha) as u32,
                    (bg.2 as f32 * (1.0 - star_alpha) + sb as f32 * star_alpha) as u32,
                )
            } else {
                bg
            };
            rgba[idx] = r as u8;
            rgba[idx + 1] = g as u8;
            rgba[idx + 2] = b as u8;
            rgba[idx + 3] = 255;
        }
    }
    Icon::from_rgba(rgba, N, N).expect("icon")
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

    if std::env::var("SPIKE_SMOKE").is_ok()
        || std::env::var("SPIKE_INPUT").is_ok()
        || std::env::var("SPIKE_WIDE").is_ok()
    {
        let grid_for_smoke = Arc::clone(&grid);
        let proxy_smoke = event_loop.create_proxy();
        let tx_smoke = tx.clone();
        let input_test = std::env::var("SPIKE_INPUT").is_ok();
        let wide_test = std::env::var("SPIKE_WIDE").is_ok();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(4000));
            if wide_test {
                let _ = tx_smoke.send(UiCommand::Keys(":echo 'AB你好CD'<CR>".to_string()));
                std::thread::sleep(Duration::from_millis(1500));
                let dump = {
                    let grid = grid_for_smoke.lock().unwrap();
                    grid.to_text()
                };
                let row = dump
                    .lines()
                    .find(|line| line.contains("AB"))
                    .unwrap_or("(nao achei)");
                println!("WIDE_OK: colunas={}", row.chars().count());
                println!("WIDE_ROW: [{}]", row);
            } else if input_test {
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

    let cfg = Config::load().0;
    let mut app = App::new(grid, tx, event_loop.create_proxy(), &cfg);
    event_loop.run_app(&mut app).expect("run_app");
    println!("SPIKE_DONE");
}
