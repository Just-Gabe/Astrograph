//! UI em Rust do Astrograph sobre um Neovim embutido.
//!
//! A UI roda no thread principal (winit). Um runtime tokio num thread separado
//! fala com o `nvim --embed` via msgpack-rpc. O handler aplica os eventos numa
//! grade de trabalho e, a cada `flush` (quadro completo), publica as linhas
//! alteradas na grade que a UI desenha e pede um quadro via `EventLoopProxy`.

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
use softbuffer::{Context, Rect, Surface};
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
/// Intervalo entre quadros enquanto o cursor desliza (~144 Hz).
const ANIM_FRAME: Duration = Duration::from_millis(7);
/// Se o Neovim nao desenhar nada ate aqui, a janela aparece mesmo assim.
const SHOW_TIMEOUT: Duration = Duration::from_millis(600);

#[derive(Clone, Copy)]
enum UserEvent {
    Redraw,
    Quit,
}

enum UiCommand {
    Keys(String),
    Resize(i64, i64),
    Focus(bool),
    Mouse {
        button: &'static str,
        action: &'static str,
        modifier: String,
        row: i64,
        col: i64,
    },
}

/// Ultima copia feita no Neovim: o sistema guarda so o texto, aqui fica o
/// tipo do registro (linha inteira vs. trecho) para a colagem sair igual.
type LastCopy = Arc<Mutex<Option<(String, String)>>>;

#[derive(Clone)]
struct UiHandler {
    /// Grade de trabalho: recebe os eventos ate o `flush`.
    back: Arc<Mutex<Grid>>,
    /// Grade publicada, que a UI desenha.
    front: Arc<Mutex<Grid>>,
    proxy: EventLoopProxy<UserEvent>,
    last_copy: LastCopy,
}

#[async_trait::async_trait]
impl Handler for UiHandler {
    type Writer = Compat<ChildStdin>;

    async fn handle_notify(&self, name: String, args: Vec<Value>, _nvim: Neovim<Self::Writer>) {
        match name.as_str() {
            "redraw" => {
                let mut back = self.back.lock().unwrap();
                back.apply_redraw(&args);
                if back.flushed {
                    back.publish(&mut self.front.lock().unwrap());
                    drop(back);
                    let _ = self.proxy.send_event(UserEvent::Redraw);
                }
            }
            "astrograph_clipboard_copy" => self.copy(&args),
            _ => {}
        }
    }

    async fn handle_request(
        &self,
        name: String,
        _args: Vec<Value>,
        _nvim: Neovim<Self::Writer>,
    ) -> Result<Value, Value> {
        match name.as_str() {
            "astrograph_clipboard_paste" => Ok(self.paste()),
            _ => Err(Value::from(format!("metodo desconhecido: {name}"))),
        }
    }
}

impl UiHandler {
    /// `copy` do `g:clipboard`: chega como notificacao, o Neovim nao espera.
    fn copy(&self, args: &[Value]) {
        let mut lines: Vec<&str> = args
            .first()
            .and_then(|v| v.as_array())
            .map(|ls| ls.iter().filter_map(|l| l.as_str()).collect())
            .unwrap_or_default();
        let regtype = args.get(1).and_then(|v| v.as_str()).unwrap_or("v").to_string();
        let linewise = regtype.starts_with('V');
        // o Neovim manda uma linha vazia final nas copias de linha inteira
        if linewise && lines.len() > 1 && lines.last() == Some(&"") {
            lines.pop();
        }
        let mut text = lines.join("\n");
        if linewise {
            text.push('\n');
        }
        let system = if cfg!(windows) { text.replace('\n', "\r\n") } else { text.clone() };
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(system);
        }
        *self.last_copy.lock().unwrap() = Some((text, regtype));
    }

    /// `paste` do `g:clipboard`: devolve `[linhas, tipo]`.
    fn paste(&self) -> Value {
        let mut text = arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get_text())
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let cached = self.last_copy.lock().unwrap().clone();
        let regtype = match cached {
            Some((copied, regtype)) if copied == text => regtype,
            _ if text.ends_with('\n') => "V".to_string(),
            _ => "v".to_string(),
        };
        if regtype.starts_with('V') && text.ends_with('\n') {
            text.pop();
        }
        let lines = text.split('\n').map(|l| Value::from(l.to_string())).collect();
        Value::Array(vec![Value::Array(lines), Value::from(regtype)])
    }
}

async fn nvim_task(
    back: Arc<Mutex<Grid>>,
    front: Arc<Mutex<Grid>>,
    mut rx: UnboundedReceiver<UiCommand>,
    proxy: EventLoopProxy<UserEvent>,
) {
    let handler = UiHandler {
        back,
        front,
        proxy: proxy.clone(),
        last_copy: Arc::new(Mutex::new(None)),
    };
    let nvim_bin = std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".to_string());
    let (nvim, io_handler, _child) = match create::new_child_cmd(
        Command::new(&nvim_bin).arg("--embed").arg("--headless"),
        handler,
    )
    .await
    {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("ASTROGRAPH_UI: nao consegui iniciar {nvim_bin:?}: {err}");
            let _ = proxy.send_event(UserEvent::Quit);
            return;
        }
    };
    // o Neovim saiu (:qa, falha): a janela fecha junto
    let quit = proxy.clone();
    tokio::spawn(async move {
        let _ = io_handler.await;
        let _ = quit.send_event(UserEvent::Quit);
    });

    // antes do ui_attach o Neovim ainda nao leu o init.lua: o g:clipboard daqui
    // vale desde o inicio e a config do usuario o respeita
    setup_ui(&nvim).await;

    let mut opts = UiAttachOptions::new();
    opts.set_linegrid_external(true);
    opts.set_rgb(true);
    if let Err(err) = nvim.ui_attach(INITIAL_COLS as i64, INITIAL_ROWS as i64, &opts).await {
        eprintln!("ASTROGRAPH_UI: ui_attach falhou: {err:?}");
        let _ = proxy.send_event(UserEvent::Quit);
        return;
    }

    while let Some(cmd) = rx.recv().await {
        let _ = match cmd {
            UiCommand::Keys(keys) => nvim.input(&keys).await.map(|_| ()),
            UiCommand::Resize(cols, rows) => nvim.ui_try_resize(cols, rows).await,
            UiCommand::Focus(gained) => nvim.ui_set_focus(gained).await,
            UiCommand::Mouse { button, action, modifier, row, col } => {
                nvim.input_mouse(button, action, &modifier, 0, row, col).await
            }
        };
    }
}

/// Liga a area de transferencia do Neovim ao sistema sem processos externos
/// (arboard na UI) e da a janela um titulo no estilo macOS: "arquivo — pasta".
///
/// As funcoes Lua no `g:clipboard` sao essenciais: valores string seriam
/// executados como comandos de shell.
async fn setup_ui(nvim: &Neovim<Compat<ChildStdin>>) {
    const LUA: &str = r#"
local chan = ...
local function copy(lines, regtype)
  vim.rpcnotify(chan, 'astrograph_clipboard_copy', lines, regtype)
end
local function paste()
  return vim.rpcrequest(chan, 'astrograph_clipboard_paste')
end
vim.g.clipboard = {
  name = 'astrograph-ui',
  copy = { ['+'] = copy, ['*'] = copy },
  paste = { ['+'] = paste, ['*'] = paste },
  cache_enabled = 0,
}
vim.o.title = true
vim.o.titlestring = "%{expand('%:t') !=# '' ? expand('%:t') . ' — ' : ''}%{fnamemodify(getcwd(), ':t')}"
"#;
    let channel = match nvim.get_api_info().await {
        Ok(info) => info.first().and_then(|v| v.as_i64()).unwrap_or(1),
        Err(_) => 1,
    };
    if let Err(err) = nvim.exec_lua(LUA, vec![Value::from(channel)]).await {
        eprintln!("ASTROGRAPH_UI: falha ao configurar clipboard/titulo: {err:?}");
    }
}

struct App {
    grid: Arc<Mutex<Grid>>,
    tx: UnboundedSender<UiCommand>,
    cfg: Config,
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    surface_size: (u32, u32),
    renderer: Option<Renderer>,
    modifiers: ModifiersState,
    last_grid: (usize, usize),
    start: Instant,
    frames: u64,
    render_full: Duration,
    render_partial: Duration,
    first_paint: Option<Duration>,
    bench: bool,
    proxy: EventLoopProxy<UserEvent>,
    base_font: f32,
    cursor_on: bool,
    next_blink: Instant,
    last_cursor: (usize, usize),
    anim_next: Option<Instant>,
    focused: bool,
    mouse_pos: (f64, f64),
    scroll_acc: (f64, f64),
    fullscreen: bool,
    left_down: bool,
    shown: bool,
    /// Pedimos um quadro; um RedrawRequested sem pedido veio do sistema.
    redraw_pending: bool,
    close_at: Option<Instant>,
    last_redraw: Instant,
}

impl App {
    fn new(
        grid: Arc<Mutex<Grid>>,
        tx: UnboundedSender<UiCommand>,
        proxy: EventLoopProxy<UserEvent>,
        cfg: Config,
    ) -> Self {
        let now = Instant::now();
        App {
            grid,
            tx,
            base_font: cfg.font.size,
            next_blink: now + Duration::from_millis(cfg.cursor.blink_ms.max(100)),
            cfg,
            window: None,
            context: None,
            surface: None,
            surface_size: (0, 0),
            renderer: None,
            modifiers: ModifiersState::empty(),
            last_grid: (0, 0),
            start: now,
            frames: 0,
            render_full: Duration::ZERO,
            render_partial: Duration::ZERO,
            first_paint: None,
            bench: std::env::var("SPIKE_BENCH").is_ok(),
            proxy,
            cursor_on: true,
            last_cursor: (usize::MAX, 0),
            anim_next: None,
            focused: true,
            mouse_pos: (0.0, 0.0),
            scroll_acc: (0.0, 0.0),
            fullscreen: false,
            left_down: false,
            shown: false,
            redraw_pending: false,
            close_at: None,
            last_redraw: now,
        }
    }

    fn blink_ms(&self) -> u64 {
        self.cfg.cursor.blink_ms.max(100)
    }

    /// Cursor aceso e pausa no piscar: digitar ou mover nunca deixa o cursor sumido.
    fn wake_cursor(&mut self) {
        self.cursor_on = true;
        self.next_blink = Instant::now() + Duration::from_millis(self.blink_ms() * 2);
    }

    fn request_redraw(&mut self) {
        if let Some(window) = &self.window {
            self.redraw_pending = true;
            window.request_redraw();
        }
    }

    fn mouse_cell(&self) -> (i64, i64) {
        let (renderer, window) = match (&self.renderer, &self.window) {
            (Some(r), Some(w)) => (r, w),
            _ => return (0, 0),
        };
        let size = window.inner_size();
        let lay = renderer.layout(size.width, size.height);
        let col = ((self.mouse_pos.0 - lay.ox as f64) / renderer.cell_w as f64).floor() as i64;
        let row = ((self.mouse_pos.1 - lay.oy as f64) / renderer.cell_h as f64).floor() as i64;
        let grid = self.grid.lock().unwrap();
        (
            row.clamp(0, grid.height as i64 - 1),
            col.clamp(0, grid.width as i64 - 1),
        )
    }

    fn mouse_modifier(&self) -> String {
        let m = self.modifiers;
        let mut out = String::new();
        if m.control_key() {
            out.push_str("C-");
        }
        if m.shift_key() {
            out.push_str("S-");
        }
        if m.alt_key() {
            out.push_str("A-");
        }
        out
    }

    fn send_mouse(&self, button: &'static str, action: &'static str) {
        let (row, col) = self.mouse_cell();
        let modifier = self.mouse_modifier();
        let _ = self.tx.send(UiCommand::Mouse { button, action, modifier, row, col });
    }

    fn draw(&mut self) {
        let window = match &self.window {
            Some(window) => window.clone(),
            None => return,
        };
        let size = window.inner_size();
        let (w, h) = (size.width, size.height);
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
            return;
        };
        let (Some(surface), Some(renderer)) = (self.surface.as_mut(), self.renderer.as_mut()) else {
            return;
        };
        // redimensionar a superficie descarta o conteudo: so quando o tamanho muda
        if self.surface_size != (w, h) {
            surface.resize(nw, nh).expect("resize");
            self.surface_size = (w, h);
        }

        let now = Instant::now();
        let bench_full = self.bench && self.frames < 240;
        if self.bench && !bench_full {
            // fase incremental do bench: so o cursor muda a cada quadro
            self.cursor_on = !self.cursor_on;
        }
        let render_start = Instant::now();
        let mut buffer = surface.buffer_mut().expect("buffer_mut");
        // idade != 1: o buffer nao tem o quadro anterior, repinta tudo
        let full = bench_full || buffer.age() != 1;
        let spans = {
            let mut grid = self.grid.lock().unwrap();
            if grid.bg_changed {
                grid.bg_changed = false;
                style_window(&window, grid.default_bg, grid.default_fg);
            }
            if let Some(title) = grid.title.take() {
                window.set_title(if title.is_empty() { &self.cfg.window.title } else { &title });
            }
            if grid.cursor != self.last_cursor {
                self.last_cursor = grid.cursor;
                self.cursor_on = true;
                self.next_blink = now + Duration::from_millis(self.cfg.cursor.blink_ms.max(100) * 2);
            }
            let visible = self.cursor_on || !self.focused;
            renderer.render(&mut buffer, w, h, &mut grid, visible, self.focused, now, full)
        };
        if !spans.is_empty() {
            let rects: Vec<Rect> = spans
                .iter()
                .filter_map(|&(y, sh)| {
                    Some(Rect { x: 0, y, width: nw, height: NonZeroU32::new(sh)? })
                })
                .collect();
            buffer.present_with_damage(&rects).expect("present");
        }
        let elapsed = render_start.elapsed();
        if full {
            self.render_full += elapsed;
        } else {
            self.render_partial += elapsed;
        }
        self.anim_next = renderer.animating(now).then(|| now + ANIM_FRAME);

        self.frames += 1;
        if self.first_paint.is_none() {
            let elapsed = self.start.elapsed();
            self.first_paint = Some(elapsed);
            println!("FIRST_PAINT: {} ms desde o inicio", elapsed.as_millis());
        }
        if self.bench {
            if self.frames == 480 {
                println!(
                    "BENCH: tela inteira {:.3} ms/quadro, so o que mudou {:.3} ms/quadro ({}x{})",
                    self.render_full.as_secs_f64() * 1000.0 / 240.0,
                    self.render_partial.as_secs_f64() * 1000.0 / 240.0,
                    w,
                    h
                );
                let _ = self.proxy.send_event(UserEvent::Quit);
            } else {
                self.redraw_pending = true;
                window.request_redraw();
            }
        }

        let lay = renderer.layout(w, h);
        if (lay.cols, lay.rows) != self.last_grid {
            self.last_grid = (lay.cols, lay.rows);
            let _ = self.tx.send(UiCommand::Resize(lay.cols as i64, lay.rows as i64));
        }
    }

    /// Mostra a janela ja com o primeiro quadro pintado: sem flash branco.
    fn show(&mut self) {
        if self.shown {
            return;
        }
        self.shown = true;
        self.draw();
        if let Some(window) = &self.window {
            window.set_visible(true);
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.invalidate();
        }
        self.request_redraw();
    }

    fn apply_zoom(&mut self, size: f32) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_size(size);
        }
        self.request_redraw();
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(window) = &self.window {
            self.fullscreen = !self.fullscreen;
            window.set_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
        }
    }

    /// Fechar a janela pede ao Neovim (`:confirm qa`): arquivos nao salvos
    /// perguntam antes. So fecha a forca se o Neovim parou de responder.
    fn request_close(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some(at) = self.close_at {
            if now.duration_since(at) > Duration::from_secs(2) && self.last_redraw < at {
                event_loop.exit();
                return;
            }
        }
        self.close_at = Some(now);
        let _ = self.tx.send(UiCommand::Keys("<C-\\><C-n><Cmd>confirm qa<CR>".to_string()));
    }
}

/// Barra de titulo e borda na cor do tema (Windows 11), como uma janela unica.
fn style_window(window: &Window, bg: u32, fg: u32) {
    #[cfg(windows)]
    {
        use winit::platform::windows::{Color, WindowExtWindows};
        let rgb = |c: u32| Color::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
        window.set_title_background_color(Some(rgb(bg)));
        window.set_border_color(Some(rgb(bg)));
        window.set_title_text_color(rgb(fg));
    }
    #[cfg(not(windows))]
    let _ = (window, bg, fg);
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let bg = self.grid.lock().unwrap().default_bg;
        let (mut width, mut height) = (self.cfg.window.width, self.cfg.window.height);
        let mut position = None;
        // cabe na tela (nunca por baixo da barra de tarefas) e abre centralizada,
        // um pouco acima do meio, como as janelas do macOS
        if let Some(monitor) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()) {
            let scale = monitor.scale_factor();
            let screen = monitor.size().to_logical::<f64>(scale);
            let origin = monitor.position().to_logical::<f64>(scale);
            width = width.min(screen.width * 0.9);
            height = height.min((screen.height - 48.0) * 0.88);
            position = Some(winit::dpi::LogicalPosition::new(
                origin.x + (screen.width - width) / 2.0,
                origin.y + ((screen.height - 48.0 - height) / 3.0).max(0.0),
            ));
        }
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(self.cfg.window.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(width, height))
            .with_window_icon(Some(make_icon()))
            .with_visible(false);
        if let Some(position) = position {
            attrs = attrs.with_position(position);
        }
        #[cfg(windows)]
        {
            use winit::platform::windows::{Color, CornerPreference, WindowAttributesExtWindows};
            let c = Color::from_rgb((bg >> 16) as u8, (bg >> 8) as u8, bg as u8);
            attrs = attrs
                .with_title_background_color(Some(c))
                .with_border_color(Some(c))
                .with_corner_preference(CornerPreference::Round);
        }
        let _ = bg;
        let window = Rc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        let scale = window.scale_factor() as f32;
        let mut renderer =
            Renderer::new(&self.cfg.font, self.cfg.cursor.animation_ms).expect("renderer");
        renderer.set_size(self.base_font * scale);
        renderer.set_padding((self.cfg.window.padding.max(0.0) * scale as f64).round() as u32);
        self.renderer = Some(renderer);
        window.set_cursor(CursorIcon::Text);
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Redraw => {
                self.last_redraw = Instant::now();
                if self.shown {
                    self.request_redraw();
                } else if self.window.is_some() {
                    self.show();
                }
            }
            UserEvent::Quit => event_loop.exit(),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let mut wake: Option<Instant> = None;
        let mut soonest = |t: Instant| wake = Some(wake.map_or(t, |w: Instant| w.min(t)));
        if !self.shown && self.window.is_some() {
            let deadline = self.start + SHOW_TIMEOUT;
            if now >= deadline {
                self.show();
            } else {
                soonest(deadline);
            }
        }
        if self.cfg.cursor.blink && self.focused && self.shown {
            if now >= self.next_blink {
                self.cursor_on = !self.cursor_on;
                self.next_blink = now + Duration::from_millis(self.blink_ms());
                self.request_redraw();
            }
            soonest(self.next_blink);
        }
        if let Some(t) = self.anim_next {
            if now >= t {
                self.anim_next = None;
                self.request_redraw();
            } else {
                soonest(t);
            }
        }
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.request_close(event_loop),
            WindowEvent::Resized(_) => self.request_redraw(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let scale = scale_factor as f32;
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.set_size(self.base_font * scale);
                    renderer.set_padding((self.cfg.window.padding.max(0.0) * scale_factor).round() as u32);
                }
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                self.wake_cursor();
                let _ = self.tx.send(UiCommand::Focus(focused));
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let before = self.mouse_cell();
                self.mouse_pos = (position.x, position.y);
                if let Some(window) = &self.window {
                    window.set_cursor_visible(true);
                }
                // arrastar so manda evento quando muda de celula
                if self.left_down && self.mouse_cell() != before {
                    self.send_mouse("left", "drag");
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // touchpad manda pixels: acumula e rola uma linha por altura de celula
                let cell_h = self.renderer.as_ref().map_or(16.0, |r| r.cell_h as f64);
                let cell_w = self.renderer.as_ref().map_or(8.0, |r| r.cell_w as f64);
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(p) => (p.x / cell_w, p.y / cell_h),
                };
                self.scroll_acc.0 += dx;
                self.scroll_acc.1 += dy;
                while self.scroll_acc.1.abs() >= 1.0 {
                    let up = self.scroll_acc.1 > 0.0;
                    self.scroll_acc.1 -= if up { 1.0 } else { -1.0 };
                    self.send_mouse("wheel", if up { "up" } else { "down" });
                }
                while self.scroll_acc.0.abs() >= 1.0 {
                    let right = self.scroll_acc.0 > 0.0;
                    self.scroll_acc.0 -= if right { 1.0 } else { -1.0 };
                    self.send_mouse("wheel", if right { "right" } else { "left" });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let name = match button {
                    MouseButton::Left => "left",
                    MouseButton::Right => "right",
                    MouseButton::Middle => "middle",
                    _ => return,
                };
                let pressed = state == ElementState::Pressed;
                if button == MouseButton::Left {
                    self.left_down = pressed;
                }
                self.send_mouse(name, if pressed { "press" } else { "release" });
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(window) = &self.window {
                    window.set_cursor_visible(false);
                }
                self.wake_cursor();
                if self.handle_shortcut(&event) {
                    return;
                }
                if let Some(keys) = key_to_input(&event, self.modifiers) {
                    let _ = self.tx.send(UiCommand::Keys(keys));
                }
            }
            WindowEvent::RedrawRequested => {
                // pedido do sistema (restaurar, descobrir a janela): repinta tudo
                if !self.redraw_pending {
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.invalidate();
                    }
                }
                self.redraw_pending = false;
                if self.shown {
                    self.draw();
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// Retorna true se o atalho foi consumido pela UI (nao vai para o Neovim).
    fn handle_shortcut(&mut self, event: &KeyEvent) -> bool {
        if matches!(event.logical_key, Key::Named(NamedKey::F11)) {
            self.toggle_fullscreen();
            return true;
        }
        if !self.modifiers.control_key() || self.modifiers.alt_key() {
            return false;
        }
        let current = match &self.renderer {
            Some(renderer) => renderer.font_size(),
            None => return false,
        };
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32);
        let size = match &event.logical_key {
            Key::Character(text) => match text.as_str() {
                "+" | "=" => (current + ZOOM_STEP * scale).min(ZOOM_MAX * scale),
                "-" => (current - ZOOM_STEP * scale).max(ZOOM_MIN * scale),
                "0" => self.base_font * scale,
                _ => return false,
            },
            _ => return false,
        };
        self.apply_zoom(size);
        true
    }
}

/// Nome de tecla especial no formato do Neovim (`<Nome>`).
fn named_key(key: NamedKey) -> Option<&'static str> {
    Some(match key {
        NamedKey::Escape => "Esc",
        NamedKey::Enter => "CR",
        NamedKey::Tab => "Tab",
        NamedKey::Backspace => "BS",
        NamedKey::Delete => "Del",
        NamedKey::Insert => "Insert",
        NamedKey::ArrowLeft => "Left",
        NamedKey::ArrowRight => "Right",
        NamedKey::ArrowUp => "Up",
        NamedKey::ArrowDown => "Down",
        NamedKey::Home => "Home",
        NamedKey::End => "End",
        NamedKey::PageUp => "PageUp",
        NamedKey::PageDown => "PageDown",
        NamedKey::Space => "Space",
        NamedKey::F1 => "F1",
        NamedKey::F2 => "F2",
        NamedKey::F3 => "F3",
        NamedKey::F4 => "F4",
        NamedKey::F5 => "F5",
        NamedKey::F6 => "F6",
        NamedKey::F7 => "F7",
        NamedKey::F8 => "F8",
        NamedKey::F9 => "F9",
        NamedKey::F10 => "F10",
        NamedKey::F12 => "F12",
        _ => return None,
    })
}

/// Texto literal para `nvim_input`: `<` precisa virar `<lt>`.
fn escape_text(text: &str) -> String {
    text.replace('<', "<lt>")
}

/// Caractere dentro de `<C-…>`/`<M-…>`.
fn key_char_name(text: &str) -> String {
    match text {
        "<" => "lt".to_string(),
        "\\" => "Bslash".to_string(),
        "|" => "Bar".to_string(),
        " " => "Space".to_string(),
        other => other.to_string(),
    }
}

fn key_to_input(event: &KeyEvent, mods: ModifiersState) -> Option<String> {
    let (ctrl, alt, shift) = (mods.control_key(), mods.alt_key(), mods.shift_key());
    match &event.logical_key {
        Key::Named(key) => {
            let name = named_key(*key)?;
            // espaco puro (ou com Shift) e texto
            if *key == NamedKey::Space && !ctrl && !alt {
                return Some(" ".to_string());
            }
            let mut prefix = String::new();
            if ctrl {
                prefix.push_str("C-");
            }
            if alt {
                prefix.push_str("M-");
            }
            if shift {
                prefix.push_str("S-");
            }
            Some(format!("<{prefix}{name}>"))
        }
        Key::Character(text) => {
            // AltGr no Windows chega como Ctrl+Alt e ja produz o caractere (@, {, €…)
            if ctrl && alt {
                if let Some(t) = event.text.as_ref().filter(|t| t.chars().all(|c| !c.is_control())) {
                    return Some(escape_text(t));
                }
            }
            if ctrl || alt {
                let mut prefix = String::new();
                if ctrl {
                    prefix.push_str("C-");
                }
                if alt {
                    prefix.push_str("M-");
                }
                return Some(format!("<{prefix}{}>", key_char_name(text.as_str())));
            }
            let typed = event.text.as_ref().map_or(text.as_str(), |t| t.as_str());
            Some(escape_text(typed))
        }
        _ => event
            .text
            .as_ref()
            .filter(|t| t.chars().all(|c| !c.is_control()))
            .map(|t| escape_text(t)),
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
    let cfg = {
        let (cfg, path) = Config::load();
        match &path {
            Some(path) => println!("CONFIG: {:?}", path),
            None => println!("CONFIG: usando defaults"),
        }
        cfg
    };
    let back = Arc::new(Mutex::new(Grid::new(INITIAL_COLS, INITIAL_ROWS)));
    let grid = Arc::new(Mutex::new(Grid::new(INITIAL_COLS, INITIAL_ROWS)));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiCommand>();

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();

    let front_for_nvim = Arc::clone(&grid);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime tokio");
        runtime.block_on(nvim_task(back, front_for_nvim, rx, proxy));
    });

    if std::env::var("SPIKE_SMOKE").is_ok()
        || std::env::var("SPIKE_INPUT").is_ok()
        || std::env::var("SPIKE_WIDE").is_ok()
        || std::env::var("SPIKE_CLIP").is_ok()
        || std::env::var("SPIKE_KEYS").is_ok()
    {
        spawn_smoke(Arc::clone(&grid), tx.clone(), event_loop.create_proxy());
    }

    let mut app = App::new(grid, tx, event_loop.create_proxy(), cfg);
    event_loop.run_app(&mut app).expect("run_app");
    println!("SPIKE_DONE");
}

/// Testes de fumaca: dirigem o Neovim pela mesma fila da UI e leem a grade publicada.
fn spawn_smoke(grid: Arc<Mutex<Grid>>, tx: UnboundedSender<UiCommand>, proxy: EventLoopProxy<UserEvent>) {
    let has = |name: &str| std::env::var(name).is_ok();
    let (input, wide, clip, keys) = (has("SPIKE_INPUT"), has("SPIKE_WIDE"), has("SPIKE_CLIP"), has("SPIKE_KEYS"));
    std::thread::spawn(move || {
        let send = |k: &str| {
            let _ = tx.send(UiCommand::Keys(k.to_string()));
        };
        let screen = || grid.lock().unwrap().to_text();
        std::thread::sleep(Duration::from_millis(4000));
        if clip {
            send(":enew<CR>:call setline(1, 'APROVEITADO')<CR>ggV\"+y");
            std::thread::sleep(Duration::from_millis(900));
            let got = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
            // colar de volta: tipo de registro preservado (linha inteira)
            send("\"+p:echo line('$') . getregtype('+')<CR>");
            std::thread::sleep(Duration::from_millis(900));
            let dump = screen();
            println!("CLIP_OK: {}", if got.contains("APROVEITADO") { "sim" } else { "nao" });
            println!("CLIP_VAL: {:?}", got);
            // "2V": a linha colada abaixo, como linha inteira
            let paste_ok = dump.lines().any(|l| l.trim() == "2V");
            println!("CLIP_PASTE_OK: {}", if paste_ok { "sim" } else { "nao" });
        } else if keys {
            // <, \, |, F-keys e Shift+Tab precisam chegar como teclas certas
            send(":enew<CR>ia<lt>b\\c|d<Esc>:echo getline(1)<CR>");
            std::thread::sleep(Duration::from_millis(1200));
            let dump = screen();
            let ok = dump.contains("a<b\\c|d");
            println!("KEYS_OK: {}", if ok { "sim" } else { "nao" });
        } else if wide {
            send(":echo 'AB你好CD'<CR>");
            std::thread::sleep(Duration::from_millis(1500));
            let dump = screen();
            let row = dump.lines().find(|l| l.contains("AB")).unwrap_or("(nao achei)");
            println!("WIDE_OK: {}", if row.contains("AB你好CD") { "sim" } else { "nao" });
            println!("WIDE_ROW: [{}]", row);
        } else if input {
            send(":");
            std::thread::sleep(Duration::from_millis(1200));
            let mode = grid.lock().unwrap().mode.clone();
            println!("INPUT_MODE: {:?}", mode);
            println!("INPUT_OK: {}", if mode.starts_with("cmdline") { "sim" } else { "nao" });
            println!("--- TELA ---\n{}--- FIM ---", screen());
        } else {
            let g = grid.lock().unwrap();
            println!(
                "SMOKE_OK: grade {}x{}, cursor {:?}, quadros {}\n--- TELA ---\n{}--- FIM ---",
                g.width,
                g.height,
                g.cursor,
                g.generation,
                g.to_text()
            );
        }
        let _ = proxy.send_event(UserEvent::Quit);
    });
}
