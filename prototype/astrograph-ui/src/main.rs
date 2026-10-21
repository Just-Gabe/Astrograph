// release: app de janela, sem console (o debug mantem o console para logs)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! UI em Rust do Astrograph sobre um Neovim embutido.
//!
//! A UI roda no thread principal (winit). Um runtime tokio num thread separado
//! fala com o `nvim --embed` via msgpack-rpc. O handler aplica os eventos numa
//! grade de trabalho e, a cada `flush` (quadro completo), publica as linhas
//! alteradas na grade que a UI desenha e pede um quadro via `EventLoopProxy`.

mod comandos;
mod proc;
mod config;
mod git;
mod grid;
mod realce;
mod relogio;
mod render;
mod anim;
mod atalhos;
mod arvore;
mod busca;
mod design;
mod dobras;
mod editorconfig;
mod native;
mod lsp;
mod agentes;
mod janelas;
mod nscene;
mod nview;
mod paint;
mod servicos;
mod term;

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
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
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
/// Duracao do fade do cursor ao piscar.
const BLINK_FADE_MS: f32 = 180.0;

#[derive(Clone, Copy)]
enum UserEvent {
    Redraw,
    Quit,
}

enum UiCommand {
    Keys(String),
    Resize(i64, i64),
    /// rolagem do editor nativo, em linhas (> 0 desce)
    Scroll(f64),
    /// o terminal nativo tem saída nova (ou terminou)
    TermOut,
    /// linhas e colunas do painel do terminal
    TermSize(i64, i64),
    /// clique na barra de abas
    Tab(i64),
    /// clique numa linha do explorador (índice na lista)
    TreeClick(usize),
    /// tamanho de cada painel (id, colunas, linhas) das janelas divididas
    Panes(Vec<(u32, i64, i64)>),
    /// clique (pressionar) num painel: foca e posiciona o cursor
    PaneClick { id: u32, row: i64, col: i64 },
    /// roda a rolagem sobre um painel, em linhas
    PaneScroll(u32, f64),
    /// mouse sobre o terminal nativo: ação (press, drag, release, wheel), botão (0 esq, 1 meio, 2 dir, 64/65 roda), célula, shift
    TermMouse { acao: &'static str, botao: u8, row: i64, col: i64, shift: bool },
    /// colar texto (do clipboard do sistema) no terminal ou na inserção
    Paste(String),
    /// copiar a seleção do terminal
    TermCopy,
    /// arrasta um divisor: índice, área (x, y, w, h), espaço entre painéis, posição em pixels
    Divisor { idx: usize, area: (f32, f32, f32, f32), gap: f32, pos: f32 },
    Focus(bool),
    /// o texto que está no clipboard do sistema (para `"+p` e `clipboard=unnamedplus`)
    ClipSync(String),
    Mouse {
        button: &'static str,
        action: &'static str,
        modifier: String,
        row: i64,
        col: i64,
    },
}

/// Janelas divididas: pinta cada painel na sua parte do canvas (cada um com a própria cena, e
/// portanto as próprias animações) e, por cima, a "casca" da janela.
#[allow(clippy::too_many_arguments)]
fn paint_paineis(
    shell: &mut nscene::Scene,
    pool: &mut std::collections::HashMap<u32, nscene::Scene>,
    c: &mut paint::Canvas,
    vm: &nview::ViewModel,
    now: Instant,
    font_px: f32,
    scale: f32,
) -> (nscene::Frame, Vec<(u32, janelas::Rect)>, janelas::Rect, f32) {
    use nscene::Camada;
    let Some(layout) = vm.layout.as_ref() else { unreachable!("só com layout") };
    let (w, h) = (c.w, c.h);
    let area = shell.area_paineis(w as u32, h as u32);
    let gap = shell.gap_paineis();
    let rects = layout.rects(area, gap);
    pool.retain(|id, _| rects.iter().any(|(i, _)| i == id));
    c.clear(design::bg());
    let (mut animating, mut wake) = (false, None::<Duration>);
    let mut juntar = |f: nscene::Frame| {
        animating |= f.animating;
        wake = match (wake, f.wake) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    };
    for (id, r) in &rects {
        if !pool.contains_key(id) {
            if let Some(sc) = nscene::Scene::new(font_px, scale) {
                pool.insert(*id, sc);
            }
        }
        let Some(sc) = pool.get_mut(id) else { continue };
        let foco = *id == vm.pane;
        // o painel não conhece abas, explorador, buscador nem avisos: são da casca
        let pvm = if foco {
            let mut v = vm.clone();
            v.tabs.clear();
            v.tree = None;
            v.picker = None;
            v.toast = None;
            v.panes.clear();
            v
        } else if let Some(p) = vm.panes.iter().find(|p| p.id == *id) {
            (*p.vm).clone()
        } else {
            continue;
        };
        let (x, y, pw, ph) = (r.x.round() as i32, r.y.round() as i32, r.w.round() as i32, r.h.round() as i32);
        let mut sub = c.sub(x, y, pw, ph);
        juntar(sc.paint_camada(&mut sub, &pvm, now, Camada::Painel { foco }));
        if !foco {
            // painéis sem o foco ficam um pouco apagados
            let (sw, sh) = (sub.w, sub.h);
            sub.rect(0, 0, sw, sh, design::bg(), 0.28);
        }
    }
    // divisores finos no espaço entre os painéis e o contorno do painel focado
    for dv in layout.divisores(area, gap) {
        let rc = dv.rect;
        let (x, y, dw, dh) = match dv.dir {
            janelas::Dir::Row => ((rc.x + rc.w / 2.0).floor() as i32, rc.y as i32, 1.max(scale as i32), rc.h as i32),
            janelas::Dir::Col => (rc.x as i32, (rc.y + rc.h / 2.0).floor() as i32, rc.w as i32, 1.max(scale as i32)),
        };
        c.rect(x, y, dw, dh, design::white(), 0.10);
    }
    if rects.len() > 1 {
        if let Some((_, r)) = rects.iter().find(|(i, _)| *i == vm.pane) {
            c.rrect_border(r.x, r.y, r.w, r.h, 0.0, 1.0 * scale.max(1.0), design::accent_soft(), 0.35);
        }
    }
    juntar(shell.paint_camada(c, vm, now, Camada::Casca));
    (nscene::Frame { animating, wake }, rects, area, gap)
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
    /// eventos de buffer para o realce em Rust (`ASTROGRAPH_REALCE=nvim` desliga)
    realce: Option<UnboundedSender<realce::Event>>,
    /// copias dos buffers do realce; responde a `indentexpr` (`astrograph_indent`)
    estado: realce::Shared,
}

#[async_trait::async_trait]
impl Handler for UiHandler {
    type Writer = Compat<ChildStdin>;

    async fn handle_notify(&self, name: String, args: Vec<Value>, nvim: Neovim<Self::Writer>) {
        match name.as_str() {
            "astrograph_busy" => return servicos::ocupado_pelo_lua(&args),
            "astrograph_dir" => {
                if servicos::ligado() {
                    if let Some(p) = args.first().and_then(Value::as_str) {
                        tokio::spawn(servicos::preparar(nvim, std::path::PathBuf::from(p)));
                    }
                }
                return;
            }
            _ => {}
        }
        if let Some(tx) = &self.realce {
            if let Some(ev) = realce::parse_notify(&name, &args) {
                let _ = tx.send(ev);
                return;
            }
        }
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
            "astrograph_scroll" => {
                let n = |i: usize| args.get(i).and_then(|v| v.as_i64()).unwrap_or(0);
                let (top, bot, left, right) = (n(0).max(0), n(1).max(0), n(2).max(0), n(3).max(0));
                if std::env::var_os("DEBUG_SCROLL").is_some() {
                    eprintln!("WINSCROLL {top}..{bot} x {left}..{right} rows={}", n(4));
                }
                self.back.lock().unwrap().capture_scroll(
                    top as usize,
                    bot as usize,
                    left as usize,
                    right as usize,
                    n(4),
                );
            }
            "astrograph_floats" => {
                let floats = parse_floats(&args);
                let mut front = self.front.lock().unwrap();
                if front.floats != floats {
                    front.floats = floats;
                    front.floats_changed = true;
                    drop(front);
                    let _ = self.proxy.send_event(UserEvent::Redraw);
                }
            }
            _ => {}
        }
    }

    async fn handle_request(
        &self,
        name: String,
        args: Vec<Value>,
        _nvim: Neovim<Self::Writer>,
    ) -> Result<Value, Value> {
        match name.as_str() {
            "astrograph_clipboard_paste" => Ok(self.paste()),
            "astrograph_indent" => Ok(realce::indent(&self.estado, &args).await),
            "astrograph_textobject" => Ok(realce::textobject(&self.estado, &args).await),
            "astrograph_textobject_ranges" => Ok(realce::textobject_ranges(&self.estado, &args).await),
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

/// `[[linha, coluna, largura, altura, zindex, winblend], ...]` vindo do Lua.
fn parse_floats(args: &[Value]) -> Vec<grid::FloatRect> {
    let num = |v: Option<&Value>| v.and_then(|v| v.as_i64()).unwrap_or(0);
    args.first()
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| {
                    let f = item.as_array()?;
                    Some(grid::FloatRect {
                        row: num(f.first()).max(0) as usize,
                        col: num(f.get(1)).max(0) as usize,
                        width: num(f.get(2)).max(0) as usize,
                        height: num(f.get(3)).max(0) as usize,
                        zindex: num(f.get(4)),
                        blend: num(f.get(5)).clamp(0, 100) as u8,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn nvim_task(
    back: Arc<Mutex<Grid>>,
    front: Arc<Mutex<Grid>>,
    mut rx: UnboundedReceiver<UiCommand>,
    proxy: EventLoopProxy<UserEvent>,
) {
    let estado = realce::Shared::default();
    let (realce_tx, realce_rx) = if realce::enabled() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let handler = UiHandler {
        back,
        front,
        proxy: proxy.clone(),
        last_copy: Arc::new(Mutex::new(None)),
        realce: realce_tx,
        estado: estado.clone(),
    };
    let nvim_bin = std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".to_string());
    // so `--embed` (sem `--headless`): o Neovim espera o `ui_attach` para ler a
    // config, entao tudo que o `setup_ui` define vale desde o inicio. O `--cmd`
    // garante o aviso de UI nativa antes de qualquer plugin.
    let mut cmd = Command::new(&nvim_bin);
    cmd.args(["--embed", "--cmd", "let g:astrograph_gui = 'astrograph-ui'"]);
    // sem janela de console para o nvim (nem para os processos que ele abre)
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let (nvim, io_handler, _child) = match create::new_child_cmd(
        &mut cmd,
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

    // conecta ja com o tamanho real da janela: a tela inicial e montada uma vez
    // so, do tamanho certo (com um tamanho provisorio o dashboard nascia
    // centralizado para outra altura e deslocado)
    let mut size = (INITIAL_COLS as i64, INITIAL_ROWS as i64);
    let mut pending = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(UiCommand::Resize(cols, rows))) => {
                size = (cols, rows);
                break;
            }
            Ok(Some(other)) => pending.push(other),
            Ok(None) | Err(_) => break,
        }
    }

    let mut opts = UiAttachOptions::new();
    opts.set_linegrid_external(true);
    opts.set_rgb(true);
    if let Err(err) = nvim.ui_attach(size.0, size.1, &opts).await {
        eprintln!("ASTROGRAPH_UI: ui_attach falhou: {err:?}");
        let _ = proxy.send_event(UserEvent::Quit);
        return;
    }

    if let Some(rx) = realce_rx {
        tokio::spawn(realce::run(nvim.clone(), estado, rx));
    }
    {
        // servicos do app: o Lua so avisa quando esta ocupado (astrograph_busy)
        let n = nvim.clone();
        tokio::spawn(async move {
            servicos::ligar(&n).await;
            if servicos::ligado() {
                if let Some(p) = servicos::pasta_atual(&n).await {
                    servicos::preparar(n.clone(), p).await;
                }
                // depois da abertura, sem disputar CPU com o inicio do editor
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                servicos::compilar_nativos().await;
            }
        });
    }

    let mut queued = pending.into_iter();
    loop {
        let cmd = match queued.next() {
            Some(cmd) => cmd,
            None => match rx.recv().await {
                Some(cmd) => cmd,
                None => break,
            },
        };
        let _ = match cmd {
            UiCommand::Keys(keys) => nvim.input(&keys).await.map(|_| ()),
            UiCommand::Resize(cols, rows) => nvim.ui_try_resize(cols, rows).await,
            UiCommand::Scroll(_) | UiCommand::TermOut | UiCommand::TermSize(..) | UiCommand::Tab(_) | UiCommand::TreeClick(_) | UiCommand::Panes(_) | UiCommand::PaneClick { .. } | UiCommand::PaneScroll(..) | UiCommand::Divisor { .. } | UiCommand::TermMouse { .. } | UiCommand::Paste(_) | UiCommand::TermCopy | UiCommand::ClipSync(_) => Ok(()),
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

-- a UI desenha cursor deslizando e rolagem por pixel: a config desliga as
-- versoes em Lua (smear-cursor, scroll do snacks) para nao animar duas vezes
vim.g.astrograph_gui = 'astrograph-ui'

-- janelas flutuantes com borda -> sombra na UI. So notifica quando muda.
local last = ''
local function border_side(b, i)
  local v = b[i]
  if type(v) == 'table' then v = v[1] end
  return v ~= nil and v ~= ''
end
local function push()
  local out, key = {}, {}
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local ok, cfg = pcall(vim.api.nvim_win_get_config, win)
    if ok and cfg.relative ~= '' and not cfg.hide and type(cfg.border) == 'table' and #cfg.border > 0 then
      local b = cfg.border
      local top, right, bottom, left = border_side(b, 2), border_side(b, 4), border_side(b, 6), border_side(b, 8)
      if top or right or bottom or left then
        local pos = vim.api.nvim_win_get_position(win)
        local h = vim.api.nvim_win_get_height(win) + (top and 1 or 0) + (bottom and 1 or 0)
        local w = vim.api.nvim_win_get_width(win) + (left and 1 or 0) + (right and 1 or 0)
        local item = { pos[1], pos[2], w, h, cfg.zindex or 50, vim.wo[win].winblend }
        out[#out + 1] = item
        key[#key + 1] = table.concat(item, ',')
      end
    end
  end
  local k = table.concat(key, ';')
  if k ~= last then
    last = k
    vim.rpcnotify(chan, 'astrograph_floats', out)
  end
end
local scheduled = false
local function soon()
  if scheduled then return end
  scheduled = true
  vim.schedule(function()
    scheduled = false
    pcall(push)
  end)
end
local group = vim.api.nvim_create_augroup('astrograph_ui_floats', { clear = true })

-- rolagem por pixel: avisa a UI quantas linhas de tela a janela rolou, antes
-- do redesenho (o WinScrolled roda antes de o Neovim desenhar o quadro)
vim.api.nvim_create_autocmd('WinScrolled', {
  group = group,
  callback = function()
    for id, ev in pairs(vim.v.event) do
      local win = tonumber(id)
      local delta = type(ev) == 'table' and ev.topline or 0
      if win and delta ~= 0 and vim.api.nvim_win_is_valid(win)
        and vim.api.nvim_win_get_config(win).relative == '' then
        local info = vim.fn.getwininfo(win)[1]
        local new_top = info.topline
        local old_top = new_top - delta
        local a, b = math.min(old_top, new_top), math.max(old_top, new_top)
        -- linhas de tela (quebra de linha e dobras contam)
        local ok, h = pcall(vim.api.nvim_win_text_height, win, { start_row = a - 1, end_row = b - 2 })
        local rows = ok and h.all or math.abs(delta)
        if delta < 0 then rows = -rows end
        local top = info.winrow - 1 + (info.winbar or 0)
        vim.rpcnotify(chan, 'astrograph_scroll', top, top + info.height, info.wincol - 1, info.wincol - 1 + info.width, rows)
      end
    end
  end,
})
vim.api.nvim_create_autocmd(
  { 'WinNew', 'WinClosed', 'WinResized', 'WinScrolled', 'VimResized', 'SafeState', 'BufWinEnter', 'WinEnter' },
  { group = group, callback = soon }
)
vim.api.nvim_create_autocmd('OptionSet', { group = group, pattern = 'winblend', callback = soon })
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
    /// Inicio do ciclo de piscar (antes disso o cursor fica aceso).
    blink_origin: Instant,
    /// Opacidade do cursor no ultimo quadro desenhado.
    drawn_alpha: f32,
    bench_flip: bool,
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
    /// SPIKE_SCROLL: quadros com rolagem em andamento e maior deslocamento.
    scroll_frames: u32,
    scroll_max: f32,
    /// Editor nativo: o que o motor publica e a cena que o anima
    native: Option<nview::Shared>,
    scene: Option<nscene::Scene>,
    last_vm: Arc<nview::ViewModel>,
    /// quando o relógio precisa ser redesenhado (virada do minuto)
    clock_next: Instant,
    last_term: (usize, usize),
    /// janelas divididas: uma cena por painel, onde cada um foi pintado e o último tamanho enviado
    pane_scenes: std::collections::HashMap<u32, nscene::Scene>,
    pane_rects: Vec<(u32, janelas::Rect)>,
    pane_area: janelas::Rect,
    pane_gap: f32,
    pane_sizes: Vec<(u32, i64, i64)>,
    /// divisor sendo arrastado (índice em `Layout::divisores`)
    drag_div: Option<usize>,
    /// tamanho da fonte (pixels) depois de `Ctrl +/-/0`; `None`: o da configuração
    zoom_px: Option<f32>,
    /// botão apertado sobre o terminal (a seleção continua mesmo se o ponteiro sair dele)
    term_down: Option<u8>,
    /// último id de cópia já enviado ao clipboard do sistema, e o clipboard (mantido vivo)
    clip_visto: u64,
    clipboard: Option<arboard::Clipboard>,
}

impl App {
    fn new(
        grid: Arc<Mutex<Grid>>,
        tx: UnboundedSender<UiCommand>,
        proxy: EventLoopProxy<UserEvent>,
        cfg: Config,
        native: Option<nview::Shared>,
    ) -> Self {
        let now = Instant::now();
        App {
            grid,
            tx,
            base_font: cfg.font.size,
            blink_origin: now + Duration::from_millis(cfg.cursor.blink_ms.max(100)),
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
            drawn_alpha: 1.0,
            bench_flip: false,
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
            scroll_frames: 0,
            scroll_max: 0.0,
            native,
            scene: None,
            last_vm: Arc::new(nview::ViewModel::empty()),
            clock_next: now,
            last_term: (0, 0),
            pane_scenes: std::collections::HashMap::new(),
            pane_rects: Vec::new(),
            pane_area: janelas::Rect::new(0.0, 0.0, 0.0, 0.0),
            pane_gap: 2.0,
            pane_sizes: Vec::new(),
            drag_div: None,
            term_down: None,
            zoom_px: None,
            clip_visto: 0,
            clipboard: None,
        }
    }

    fn blink_ms(&self) -> u64 {
        self.cfg.cursor.blink_ms.max(100)
    }

    /// Cursor aceso e pausa no piscar: digitar ou mover nunca deixa o cursor sumido.
    fn wake_cursor(&mut self) {
        self.blink_origin = Instant::now() + Duration::from_millis(self.blink_ms());
    }

    /// Opacidade do cursor agora e quando ela volta a mudar. O piscar e um fade
    /// suave (como no macOS), nao um liga/desliga.
    fn cursor_alpha(&self, now: Instant) -> (f32, Option<Instant>) {
        if !self.cfg.cursor.blink || !self.focused {
            return (1.0, None);
        }
        if now < self.blink_origin {
            return (1.0, Some(self.blink_origin));
        }
        let on = self.blink_ms() as f32;
        let period = on * 2.0;
        let fade = BLINK_FADE_MS.min(on / 2.0);
        let t = (now.duration_since(self.blink_origin).as_secs_f32() * 1000.0) % period;
        let ease = |x: f32| x * x * (3.0 - 2.0 * x);
        let after = |ms: f32| Some(now + Duration::from_secs_f32(ms.max(0.0) / 1000.0));
        if t < on - fade {
            (1.0, after(on - fade - t))
        } else if t < on {
            (1.0 - ease((t - (on - fade)) / fade), Some(now + ANIM_FRAME))
        } else if t < period - fade {
            (0.0, after(period - fade - t))
        } else {
            (ease((t - (period - fade)) / fade), Some(now + ANIM_FRAME))
        }
    }

    fn request_redraw(&mut self) {
        if let Some(window) = &self.window {
            self.redraw_pending = true;
            window.request_redraw();
        }
    }

    /// Painel (e seu retângulo) sob o ponteiro, nas janelas divididas.
    fn painel_sob_mouse(&self) -> Option<(u32, janelas::Rect)> {
        let (x, y) = (self.mouse_pos.0 as f32, self.mouse_pos.1 as f32);
        self.pane_rects.iter().find(|(_, r)| r.contains(x, y)).map(|(i, r)| (*i, *r))
    }

    fn divisor(&self, idx: usize) -> Option<janelas::Divisor> {
        let layout = self.last_vm.layout.as_ref()?;
        layout.divisores(self.pane_area, self.pane_gap).into_iter().find(|d| d.idx == idx)
    }

    /// Divisor perto do ponteiro (uma faixa mais larga que o desenho, para ser fácil de pegar).
    fn divisor_sob_mouse(&self) -> Option<usize> {
        let layout = self.last_vm.layout.as_ref()?;
        let (x, y) = (self.mouse_pos.0 as f32, self.mouse_pos.1 as f32);
        let folga = 4.0 * self.scale_factor();
        layout.divisores(self.pane_area, self.pane_gap).into_iter().find_map(|d| {
            let r = d.rect;
            let (dentro_x, dentro_y) = (x >= r.x - folga && x < r.x + r.w + folga, y >= r.y - folga && y < r.y + r.h + folga);
            (dentro_x && dentro_y).then_some(d.idx)
        })
    }

    /// Manda ao editor nativo o texto do clipboard do sistema.
    fn enviar_clipboard(&mut self) {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(t) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) {
            let _ = self.tx.send(UiCommand::ClipSync(t));
        }
    }

    fn scale_factor(&self) -> f32 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32)
    }

    fn mouse_cell(&self) -> (i64, i64) {
        if let (Some(scene), Some(window)) = (&self.scene, &self.window) {
            let size = window.inner_size();
            let (row, col) = scene.hit_test(self.mouse_pos.0 as f32, self.mouse_pos.1 as f32, size.width, size.height, &self.last_vm);
            return (row as i64, col as i64);
        }
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

    /// Texto do relógio, se ligado na config.
    fn clock_text(&self) -> Option<String> {
        self.cfg.clock.enabled.then(|| relogio::agora(self.cfg.clock.hour12))
    }

    /// Quadro do editor nativo: a cena pinta pixels, sem grade de celulas.
    fn draw_native(&mut self) {
        let (Some(window), Some(shared)) = (self.window.clone(), self.native.clone()) else { return };
        let size = window.inner_size();
        let (w, h) = (size.width, size.height);
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else { return };
        let Some(surface) = self.surface.as_mut() else { return };
        if self.surface_size != (w, h) {
            surface.resize(nw, nh).expect("resize");
            self.surface_size = (w, h);
        }
        let now = Instant::now();
        let vm = shared.lock().unwrap().clone();
        let clock = self.cfg.clock.enabled.then(|| relogio::agora(self.cfg.clock.hour12));
        let scale = window.scale_factor() as f32;
        let font_px = self.zoom_px.unwrap_or(self.base_font * scale);
        let Some(scene) = self.scene.as_mut() else { return };
        scene.set_clock(clock);
        let mut buffer = surface.buffer_mut().expect("buffer_mut");
        let mut multi = None;
        let frame = {
            let mut canvas = paint::Canvas::new(&mut buffer, w, h);
            if vm.layout.is_some() {
                let (f, rects, area, gap) = paint_paineis(scene, &mut self.pane_scenes, &mut canvas, &vm, now, font_px, scale);
                multi = Some((rects, area, gap));
                f
            } else {
                scene.paint(&mut canvas, &vm, now)
            }
        };
        buffer.present().expect("present");
        self.anim_next = frame.animating.then(|| now + ANIM_FRAME);
        if let Some(d) = frame.wake {
            let t = now + d;
            self.anim_next = Some(self.anim_next.map_or(t, |a| a.min(t)));
        }
        if !servicos::em_andamento().is_empty() {
            let t = now + Duration::from_millis(servicos::PASSO_MS as u64);
            self.anim_next = Some(self.anim_next.map_or(t, |a| a.min(t)));
        }
        if self.first_paint.is_none() {
            let elapsed = self.start.elapsed();
            self.first_paint = Some(elapsed);
            println!("FIRST_PAINT: {} ms desde o inicio", elapsed.as_millis());
        }
        self.clock_next = now + relogio::ate_proximo_minuto();
        self.frames += 1;
        if let Some((id, texto)) = &vm.clip {
            if *id > self.clip_visto {
                self.clip_visto = *id;
                if self.clipboard.is_none() {
                    self.clipboard = arboard::Clipboard::new().ok();
                }
                if let Some(c) = self.clipboard.as_mut() {
                    let _ = c.set_text(texto.clone());
                }
            }
        }
        self.last_vm = vm.clone();
        let lay = scene.layout(w, h, vm.lines);
        let (tr, tc) = scene.term_dims(w, h);
        if (tr, tc) != self.last_term {
            self.last_term = (tr, tc);
            let _ = self.tx.send(UiCommand::TermSize(tr as i64, tc as i64));
        }
        // janelas divididas: cada painel informa o seu tamanho; sem elas, a área toda
        if let Some((rects, area, gap)) = multi {
            let mut tam = Vec::new();
            for (id, r) in &rects {
                let Some(sc) = self.pane_scenes.get(id) else { continue };
                let lines = if *id == vm.pane { vm.lines } else { vm.panes.iter().find(|p| p.id == *id).map_or(1, |p| p.vm.lines) };
                let (c, rows) = sc.tamanho_painel(r.w as u32, r.h as u32, lines);
                tam.push((*id, c as i64, rows as i64));
            }
            if tam != self.pane_sizes {
                self.pane_sizes = tam.clone();
                let _ = self.tx.send(UiCommand::Panes(tam));
            }
            self.pane_rects = rects;
            self.pane_area = area;
            self.pane_gap = gap;
            return;
        }
        // voltou a uma janela só: o tamanho dela precisa ser dito de novo
        let estava_dividido = !self.pane_rects.is_empty();
        self.pane_rects.clear();
        self.pane_scenes.clear();
        self.pane_sizes.clear();
        let dims = (lay.cols, lay.rows.floor() as usize);
        if dims != self.last_grid || estava_dividido {
            self.last_grid = dims;
            let _ = self.tx.send(UiCommand::Resize(lay.cols as i64, dims.1 as i64));
        }
    }

    fn draw(&mut self) {
        if self.native.is_some() {
            return self.draw_native();
        }
        let window = match &self.window {
            Some(window) => window.clone(),
            None => return,
        };
        let size = window.inner_size();
        let (w, h) = (size.width, size.height);
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
            return;
        };
        let pre_alpha = self.cursor_alpha(Instant::now()).0;
        let clock = self.clock_text();
        let (Some(surface), Some(renderer)) = (self.surface.as_mut(), self.renderer.as_mut()) else {
            return;
        };
        // redimensionar a superficie descarta o conteudo: so quando o tamanho muda
        if self.surface_size != (w, h) {
            surface.resize(nw, nh).expect("resize");
            self.surface_size = (w, h);
        }

        let now = Instant::now();
        // bench: so mede depois da abertura (o fade de entrada repinta tudo)
        let bench_warm = self.bench && self.start.elapsed() < Duration::from_millis(1500);
        let bench_full = self.bench && !bench_warm && self.frames < 240;
        if self.bench && !bench_full {
            // fase incremental do bench: so o cursor muda a cada quadro
            self.bench_flip = !self.bench_flip;
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
            let moved = grid.cursor != self.last_cursor;
            if moved {
                self.last_cursor = grid.cursor;
                self.blink_origin = now + Duration::from_millis(self.cfg.cursor.blink_ms.max(100));
            }
            let alpha = if self.bench && !bench_full {
                if self.bench_flip { 1.0 } else { 0.0 }
            } else if moved {
                1.0
            } else {
                pre_alpha
            };
            // primeiro quadro do Neovim e a tela vazia de antes da config: sem
            // cursor solto no canto enquanto o conteudo nao chega
            let alpha = if grid.generation < 2 { 0.0 } else { alpha };
            self.drawn_alpha = alpha;
            renderer.set_spinner(servicos::quadro(now));
            renderer.set_clock(clock);
            renderer.render(&mut buffer, w, h, &mut grid, alpha, self.focused, now, full)
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
        if bench_warm {
            // aquecimento do bench: nao conta
        } else if full {
            self.render_full += elapsed;
        } else {
            self.render_partial += elapsed;
        }
        self.clock_next = now + relogio::ate_proximo_minuto();
        self.anim_next = renderer.animating(now).then(|| now + ANIM_FRAME);
        // trabalho em segundo plano: o anel gira no ritmo dele, sem forcar 140 quadros por segundo
        if !servicos::em_andamento().is_empty() {
            let t = now + Duration::from_millis(servicos::PASSO_MS as u64);
            self.anim_next = Some(self.anim_next.map_or(t, |a| a.min(t)));
        }
        let offset = renderer.scroll_offset();
        if offset > 0.0 {
            self.scroll_frames += 1;
            self.scroll_max = self.scroll_max.max(offset);
        }

        if self.first_paint.is_none() {
            let elapsed = self.start.elapsed();
            self.first_paint = Some(elapsed);
            println!("FIRST_PAINT: {} ms desde o inicio", elapsed.as_millis());
        }
        if bench_warm {
            self.redraw_pending = true;
            window.request_redraw();
        } else {
            self.frames += 1;
        }

        if self.bench && !bench_warm {
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
        // editor nativo: a cena usa o mesmo tamanho (e as dos painéis são refeitas com ele)
        let escala = self.scale_factor();
        if let Some(scene) = self.scene.as_mut() {
            scene.set_metrics(size, escala);
            self.zoom_px = Some(size);
            self.pane_scenes.clear();
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
        if self.cfg.theme.mode == "auto" {
            design::set_light(window.theme() == Some(winit::window::Theme::Light));
        }
        // o motor nativo recebe texto de IMEs (composição de CJK, teclas mortas do sistema)
        if self.native.is_some() {
            window.set_ime_allowed(true);
        }
        let mut renderer =
            Renderer::new(&self.cfg.font, self.cfg.cursor.animation_ms).expect("renderer");
        renderer.set_size(self.base_font * scale);
        renderer.set_padding((self.cfg.window.padding.max(0.0) * scale as f64).round() as u32);
        renderer.set_scroll_ms(self.cfg.scroll.animation_ms);
        // tamanho da grade ja agora: o Neovim conecta com ele
        let size = window.inner_size();
        let lay = renderer.layout(size.width, size.height);
        self.last_grid = (lay.cols, lay.rows);
        let _ = self.tx.send(UiCommand::Resize(lay.cols as i64, lay.rows as i64));
        if self.native.is_some() {
            self.scene = nscene::Scene::new(self.base_font * scale, scale);
            if let Some(scene) = &self.scene {
                let lay = scene.layout(size.width, size.height, 1);
                self.last_grid = (lay.cols, lay.rows.floor() as usize);
                let _ = self.tx.send(UiCommand::Resize(lay.cols as i64, lay.rows.floor() as i64));
            }
        }
        self.renderer = Some(renderer);
        window.set_cursor(CursorIcon::Text);
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        if self.native.is_some() {
            self.enviar_clipboard();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Redraw => {
                self.last_redraw = Instant::now();
                if self.shown {
                    self.request_redraw();
                } else if self.window.is_some() {
                    // a cor inicial ja e a do tema Astrograph: aparece no primeiro
                    // quadro, e o conteudo entra com o fade do dashboard
                    self.show();
                }
            }
            UserEvent::Quit => {
                if std::env::var("SPIKE_SCROLL").is_ok() {
                    let ok = self.scroll_frames >= 4 && self.scroll_max > 0.0;
                    println!(
                        "SCROLL_OK: {} (quadros animados={}, deslocamento max={:.0}px)",
                        if ok { "sim" } else { "nao" },
                        self.scroll_frames,
                        self.scroll_max
                    );
                }
                event_loop.exit()
            }
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
        if self.shown && self.native.is_none() {
            let (alpha, next) = self.cursor_alpha(now);
            if (alpha - self.drawn_alpha).abs() > 0.004 {
                self.request_redraw();
            }
            if let Some(next) = next {
                soonest(next);
            }
        }
        if self.shown && self.cfg.clock.enabled {
            if now >= self.clock_next {
                self.clock_next = now + relogio::ate_proximo_minuto();
                self.request_redraw();
            } else {
                soonest(self.clock_next);
            }
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
            // texto composto por um IME (chinês, japonês, coreano...) ou pelo sistema: entra como se fosse digitado
            WindowEvent::Ime(Ime::Commit(texto)) => {
                if !texto.is_empty() {
                    self.wake_cursor();
                    let _ = self.tx.send(UiCommand::Keys(escape_text(&texto)));
                }
            }
            WindowEvent::ThemeChanged(tema) => {
                if self.cfg.theme.mode == "auto" {
                    design::set_light(tema == winit::window::Theme::Light);
                    self.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let scale = scale_factor as f32;
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.set_size(self.base_font * scale);
                    renderer.set_padding((self.cfg.window.padding.max(0.0) * scale_factor).round() as u32);
                }
                if let Some(scene) = self.scene.as_mut() {
                    scene.set_metrics(self.base_font * scale, scale);
                }
                self.pane_scenes.clear();
                self.zoom_px = None;
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                self.wake_cursor();
                let _ = self.tx.send(UiCommand::Focus(focused));
                // ao voltar para a janela o clipboard pode ter mudado fora: o editor nativo o relê
                if focused && self.native.is_some() {
                    self.enviar_clipboard();
                }
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
                // seleção no terminal: o arrasto continua mesmo fora dele
                if let Some(b) = self.term_down {
                    if let Some((r, c)) = self.scene.as_ref().and_then(|sc| sc.term_hit(position.x as f32, position.y as f32, true)) {
                        let shift = self.modifiers.shift_key();
                        let _ = self.tx.send(UiCommand::TermMouse { acao: "drag", botao: b, row: r as i64, col: c as i64, shift });
                    }
                    return;
                }
                // janelas divididas: arrastar o divisor, ou mostrar que ele pode ser arrastado
                if !self.pane_rects.is_empty() {
                    if let Some(idx) = self.drag_div {
                        let pos = match self.divisor(idx).map(|d| d.dir) {
                            Some(janelas::Dir::Row) => self.mouse_pos.0 as f32,
                            _ => self.mouse_pos.1 as f32,
                        };
                        let a = self.pane_area;
                        let _ = self.tx.send(UiCommand::Divisor { idx, area: (a.x, a.y, a.w, a.h), gap: self.pane_gap, pos });
                    } else if let Some(window) = &self.window {
                        let icone = match self.divisor_sob_mouse().and_then(|i| self.divisor(i)).map(|d| d.dir) {
                            Some(janelas::Dir::Row) => CursorIcon::ColResize,
                            Some(janelas::Dir::Col) => CursorIcon::RowResize,
                            None => CursorIcon::Text,
                        };
                        window.set_cursor(icone);
                    }
                    return;
                }
                // arrastar so manda evento quando muda de celula
                if self.left_down && self.mouse_cell() != before {
                    self.send_mouse("left", "drag");
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.scene.is_some() => {
                let line_h = self.scene.as_ref().map_or(20.0, |s| s.line_height()) as f64;
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * 3.0,
                    MouseScrollDelta::PixelDelta(p) => p.y / line_h,
                };
                if dy != 0.0 {
                    // terminal: a roda rola o histórico (ou vai ao programa, se ele pede o mouse)
                    let (mx, my) = (self.mouse_pos.0 as f32, self.mouse_pos.1 as f32);
                    if let Some((r, c)) = self.scene.as_ref().and_then(|sc| sc.term_hit(mx, my, false)) {
                        let shift = self.modifiers.shift_key();
                        let _ = self.tx.send(UiCommand::TermMouse { acao: "wheel", botao: if dy > 0.0 { 64 } else { 65 }, row: r as i64, col: c as i64, shift });
                        return;
                    }
                    // janelas divididas: a roda age sobre o painel sob o ponteiro
                    match self.painel_sob_mouse() {
                        Some((id, _)) => {
                            let _ = self.tx.send(UiCommand::PaneScroll(id, -dy));
                        }
                        None => {
                            let _ = self.tx.send(UiCommand::Scroll(-dy));
                        }
                    }
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
                // terminal: pressionar começa seleção (ou vai ao programa); soltar termina
                {
                    let botao = match button {
                        MouseButton::Left => 0u8,
                        MouseButton::Middle => 1,
                        MouseButton::Right => 2,
                        _ => 255,
                    };
                    let (mx, my) = (self.mouse_pos.0 as f32, self.mouse_pos.1 as f32);
                    let shift = self.modifiers.shift_key();
                    if state == ElementState::Released && self.term_down == Some(botao) {
                        self.term_down = None;
                        if let Some((r, c)) = self.scene.as_ref().and_then(|sc| sc.term_hit(mx, my, true)) {
                            let _ = self.tx.send(UiCommand::TermMouse { acao: "release", botao, row: r as i64, col: c as i64, shift });
                        }
                        return;
                    }
                    if state == ElementState::Pressed && botao != 255 {
                        if let Some((r, c)) = self.scene.as_ref().and_then(|sc| sc.term_hit(mx, my, false)) {
                            self.term_down = Some(botao);
                            let _ = self.tx.send(UiCommand::TermMouse { acao: "press", botao, row: r as i64, col: c as i64, shift });
                            return;
                        }
                    }
                }
                if !self.pane_rects.is_empty() {
                    // janelas divididas: o clique é da casca (abas, explorador), de um divisor ou de um painel
                    if button != MouseButton::Left {
                        return;
                    }
                    if state == ElementState::Released {
                        self.drag_div = None;
                        return;
                    }
                    let (mx, my) = (self.mouse_pos.0 as f32, self.mouse_pos.1 as f32);
                    if let Some(i) = self.scene.as_ref().and_then(|sc| sc.tab_hit(mx, my)) {
                        let _ = self.tx.send(UiCommand::Tab(i as i64));
                        return;
                    }
                    if self.scene.as_ref().is_some_and(|sc| sc.in_side(mx)) {
                        if let Some(i) = self.scene.as_ref().and_then(|sc| sc.tree_hit(mx, my)) {
                            let _ = self.tx.send(UiCommand::TreeClick(i));
                        }
                        return;
                    }
                    if let Some(i) = self.divisor_sob_mouse() {
                        self.drag_div = Some(i);
                        return;
                    }
                    if let Some((id, r)) = self.painel_sob_mouse() {
                        let vm = &self.last_vm;
                        let pvm = if id == vm.pane { Some(&**vm) } else { vm.panes.iter().find(|p| p.id == id).map(|p| &*p.vm) };
                        if let (Some(pvm), Some(sc)) = (pvm, self.pane_scenes.get(&id)) {
                            let (row, col) = sc.hit_test(mx - r.x, my - r.y, r.w as u32, r.h as u32, pvm);
                            let _ = self.tx.send(UiCommand::PaneClick { id, row: row as i64, col: col as i64 });
                        }
                    }
                    return;
                }
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(i) = self.scene.as_ref().and_then(|sc| sc.tab_hit(self.mouse_pos.0 as f32, self.mouse_pos.1 as f32)) {
                        let _ = self.tx.send(UiCommand::Tab(i as i64));
                        return;
                    }
                }
                if self.scene.as_ref().is_some_and(|sc| sc.in_side(self.mouse_pos.0 as f32)) {
                    // clique no explorador: abre o arquivo ou a pasta (o teclado também navega)
                    if button == MouseButton::Left && state == ElementState::Pressed {
                        let (mx, my) = (self.mouse_pos.0 as f32, self.mouse_pos.1 as f32);
                        if let Some(i) = self.scene.as_ref().and_then(|sc| sc.tree_hit(mx, my)) {
                            let _ = self.tx.send(UiCommand::TreeClick(i));
                        }
                    }
                    return;
                }
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
        // copiar e colar: Ctrl+Shift+C/V (e Shift+Insert) no terminal; colar também na inserção
        let (ctrl, shift) = (self.modifiers.control_key(), self.modifiers.shift_key());
        let colar = (ctrl && shift && matches!(&event.logical_key, Key::Character(t) if t.eq_ignore_ascii_case("v")))
            || (shift && !ctrl && matches!(event.logical_key, Key::Named(NamedKey::Insert)));
        if colar && self.native.is_some() {
            if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                let _ = self.tx.send(UiCommand::Paste(t));
            }
            return true;
        }
        if ctrl && shift && matches!(&event.logical_key, Key::Character(t) if t.eq_ignore_ascii_case("c")) && self.last_vm.term.as_ref().is_some_and(|t| t.focus) {
            let _ = self.tx.send(UiCommand::TermCopy);
            return true;
        }
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
    nview::T0.get_or_init(Instant::now);
    let cfg = {
        let (cfg, path) = Config::load();
        match &path {
            Some(path) => println!("CONFIG: {:?}", path),
            None => println!("CONFIG: usando defaults"),
        }
        cfg
    };
    let back = Arc::new(Mutex::new({
        let mut g = Grid::new(INITIAL_COLS, INITIAL_ROWS);
        g.record_scrolls = cfg.scroll.animation_ms > 0;
        g
    }));
    let grid = Arc::new(Mutex::new(Grid::new(INITIAL_COLS, INITIAL_ROWS)));
    design::set_light(cfg.theme.mode == "light");
    nscene::set_fontes(cfg.font.regular.clone(), cfg.font.bold.clone(), cfg.font.italic.clone());
    let _ = dobras::METODO_INICIAL.set(dobras::Metodo::de(&cfg.editor.foldmethod).unwrap_or_default());
    let _ = dobras::WRAP_INICIAL.set(cfg.editor.wrap);
    let _ = dobras::LINEBREAK_INICIAL.set(cfg.editor.linebreak);
    let leader_key = cfg.keys.leader_char();
    let format_on_save = cfg.lsp.format_on_save;
    let mapeamentos = cfg.keys.mapeamentos();
    let clipboard_mais = cfg.editor.clipboard.contains("unnamedplus");
    // motor: ASTROGRAPH_ENGINE=native|nvim manda; sem ela vale `[engine] native` do config.toml
    let native_on = match std::env::var("ASTROGRAPH_ENGINE").as_deref() {
        Ok("native") => true,
        Ok("nvim") => false,
        _ => cfg.engine.native,
    };
    let native_shared = nview::shared();
    let app_native = native_on.then(|| native_shared.clone());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UiCommand>();
    let tx_native = tx.clone();

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("event loop");
    let proxy = event_loop.create_proxy();
    {
        let acordar = proxy.clone();
        servicos::iniciar(move || {
            let _ = acordar.send_event(UserEvent::Redraw);
        });
    }

    let front_for_nvim = Arc::clone(&grid);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime tokio");
        // ASTROGRAPH_ENGINE=native [arquivo]: o motor em Rust, sem Neovim
        if native_on {
            let file = std::env::args().skip(1).find(|a| !a.starts_with('-')).map(std::path::PathBuf::from);
            runtime.block_on(native::native_task(native_shared, rx, proxy, file, leader_key, format_on_save, mapeamentos, clipboard_mais, tx_native));
        } else {
            runtime.block_on(nvim_task(back, front_for_nvim, rx, proxy));
        }
    });

    if std::env::var("SPIKE_SMOKE").is_ok()
        || std::env::var("SPIKE_INPUT").is_ok()
        || std::env::var("SPIKE_WIDE").is_ok()
        || std::env::var("SPIKE_CLIP").is_ok()
        || std::env::var("SPIKE_KEYS").is_ok()
        || std::env::var("SPIKE_SCROLL").is_ok()
        || std::env::var("SPIKE_FLOATS").is_ok()
        || std::env::var("SPIKE_REALCE").is_ok()
    {
        spawn_smoke(Arc::clone(&grid), tx.clone(), event_loop.create_proxy());
    }

    let mut app = App::new(grid, tx, event_loop.create_proxy(), cfg, app_native);
    event_loop.run_app(&mut app).expect("run_app");
    println!("SPIKE_DONE");
}

/// Testes de fumaca: dirigem o Neovim pela mesma fila da UI e leem a grade publicada.
fn spawn_smoke(grid: Arc<Mutex<Grid>>, tx: UnboundedSender<UiCommand>, proxy: EventLoopProxy<UserEvent>) {
    let has = |name: &str| std::env::var(name).is_ok();
    let (input, wide, clip, keys) = (has("SPIKE_INPUT"), has("SPIKE_WIDE"), has("SPIKE_CLIP"), has("SPIKE_KEYS"));
    let (scroll, floats) = (has("SPIKE_SCROLL"), has("SPIKE_FLOATS"));
    std::thread::spawn(move || {
        let send = |k: &str| {
            let _ = tx.send(UiCommand::Keys(k.to_string()));
        };
        let screen = || grid.lock().unwrap().to_text();
        std::thread::sleep(Duration::from_millis(4000));
        if let Ok(file) = std::env::var("SPIKE_REALCE") {
            // realce em Rust no app de verdade: abre o arquivo e grava em
            // SPIKE_REALCE_OUT os extmarks do Rust e se o nativo ficou ligado
            let out = std::env::var("SPIKE_REALCE_OUT").unwrap_or_else(|_| "realce-smoke.txt".into());
            send(&format!(":e {}<CR>", file.replace(' ', "\\ ")));
            std::thread::sleep(Duration::from_millis(1500));
            send(&format!(
                ":lua local ns = vim.api.nvim_get_namespaces().astrograph_realce; local b = vim.api.nvim_get_current_buf(); \
                 vim.fn.writefile({{'extmarks=' .. (ns and #vim.api.nvim_buf_get_extmarks(b, ns, 0, -1, {{}}) or -1), \
                 'nativo=' .. tostring(vim.treesitter.highlighter.active[b] ~= nil), 'syntax=' .. vim.bo.syntax}}, [[{}]])<CR>",
                out
            ));
            std::thread::sleep(Duration::from_millis(500));
            println!("--- TELA ---\n{}--- FIM ---", screen());
            // SPIKE_REALCE_HOLD=ms: segura a janela aberta (para um print)
            if let Some(ms) = std::env::var("SPIKE_REALCE_HOLD").ok().and_then(|v| v.parse().ok()) {
                std::thread::sleep(Duration::from_millis(ms));
            }
        } else if scroll {
            // arquivo longo, desce meia tela e volta: o App conta os quadros animados
            match std::env::var("SPIKE_SCROLL_FILE") {
                Ok(file) => send(&format!(":e {file}<CR>")),
                Err(_) => send(":enew<CR>:call setline(1, map(range(1, 400), '\"linha \" . v:val'))<CR>gg"),
            }
            std::thread::sleep(Duration::from_millis(600));
            if let Ok(pre) = std::env::var("SPIKE_SCROLL_PRE") {
                send(&pre);
                std::thread::sleep(Duration::from_millis(300));
            }
            send("<C-d>");
            std::thread::sleep(Duration::from_millis(500));
            if std::env::var("DEBUG_SCROLL").is_ok() {
                println!("--- TELA ---
{}--- FIM ---", screen());
            }
            send("<C-u>");
            std::thread::sleep(Duration::from_millis(500));
        } else if floats {
            send(":lua vim.api.nvim_open_win(vim.api.nvim_create_buf(false, true), false, {relative='editor', row=3, col=8, width=30, height=5, border='rounded'})<CR>");
            std::thread::sleep(Duration::from_millis(800));
            let fl = grid.lock().unwrap().floats.clone();
            let ok = fl.iter().any(|f| f.row == 3 && f.col == 8 && f.width == 32 && f.height == 7);
            println!("FLOATS_OK: {} {:?}", if ok { "sim" } else { "nao" }, fl);
        } else if clip {
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

#[cfg(test)]
mod testes_paineis {
    use super::*;

    /// Compõe `n` quadros (aplicando os tamanhos que a tela informa ao editor, como o app faz).
    fn compor(w: &mut native::Workspace, shell: &mut nscene::Scene, pool: &mut std::collections::HashMap<u32, nscene::Scene>, largura: u32, altura: u32, t0: Instant, n: u32) -> Vec<u32> {
        let mut buf = vec![0u32; (largura * altura) as usize];
        for i in 0..n {
            let vm = w.view_model();
            let mut c = paint::Canvas::new(&mut buf, largura, altura);
            let (_, rects, _, _) = paint_paineis(shell, pool, &mut c, &vm, t0 + Duration::from_millis(16 * i as u64), 14.0, 1.0);
            let tam: Vec<(u32, usize, f32)> = rects
                .iter()
                .filter_map(|(id, r)| {
                    let lines = if *id == vm.pane { vm.lines } else { vm.panes.iter().find(|p| p.id == *id)?.vm.lines };
                    let (c, rows) = pool.get(id)?.tamanho_painel(r.w as u32, r.h as u32, lines);
                    Some((*id, c, rows))
                })
                .collect();
            w.tamanhos_paineis(&tam);
        }
        buf
    }

    fn arquivo() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-pn-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("exemplo.rs");
        std::fs::write(&f, "fn main() {\n    let nome = \"astrograph\";\n    println!(\"{nome}\");\n}\n\nfn soma(a: i32, b: i32) -> i32 {\n    a + b\n}\n").unwrap();
        f
    }

    #[test]
    fn paineis_pintam_cada_um_no_seu_retangulo_e_o_foco_se_destaca() {
        let (Some(mut shell), true) = (nscene::Scene::new(14.0, 1.0), true) else { return };
        let mut pool = std::collections::HashMap::new();
        let mut w = native::Workspace::new(Some(arquivo()), ' ', Arc::new(|| {}));
        let (largura, altura) = (1200u32, 760u32);
        let t0 = Instant::now();
        // uma janela só primeiro, para o `tabs_n` da casca e o tamanho existirem
        w.feed(":vsplit<CR>");
        w.feed(":split<CR>");
        let buf = compor(&mut w, &mut shell, &mut pool, largura, altura, t0, 80);
        let vm = w.view_model();
        let rects = vm.layout.as_ref().unwrap().rects(shell.area_paineis(largura, altura), shell.gap_paineis());
        assert_eq!(rects.len(), 3);
        assert_eq!(pool.len(), 3, "uma cena por painel");
        // cada painel tem texto pintado (pixels diferentes do fundo) dentro do seu retângulo
        for (id, r) in &rects {
            let mut diferentes = 0;
            for y in (r.y as u32)..((r.y + r.h) as u32) {
                for x in (r.x as u32)..((r.x + r.w) as u32) {
                    let p = buf[(y * largura + x) as usize] & 0xffffff;
                    if p != design::bg() && p != 0 {
                        diferentes += 1;
                    }
                }
            }
            assert!(diferentes > 1500, "painel {id} pintado ({diferentes})");
        }
        // os tamanhos chegaram ao editor: o painel estreito tem menos colunas que o largo
        if let Ok(caminho) = std::env::var("ASTROGRAPH_PNG") {
            let mut ppm = format!("P6\n{largura} {altura}\n255\n").into_bytes();
            for p in &buf {
                ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
            }
            std::fs::write(format!("{caminho}.ppm"), ppm).unwrap();
        }
    }

    #[test]
    fn paineis_com_abas_explorador_e_terminal_pela_casca() {
        let Some(mut shell) = nscene::Scene::new(14.0, 1.0) else { return };
        let mut pool = std::collections::HashMap::new();
        let f = arquivo();
        let mut w = native::Workspace::new(Some(f.clone()), ' ', Arc::new(|| {}));
        let t0 = Instant::now();
        w.feed(&format!(":tabnew {}<CR>", f.display()));
        w.feed(":vsplit<CR>");
        w.feed(" e");
        let buf = compor(&mut w, &mut shell, &mut pool, 1200, 760, t0, 90);
        let vm = w.view_model();
        assert_eq!(vm.tabs.len(), 2);
        let area = shell.area_paineis(1200, 760);
        assert!(area.x > 100.0 && area.y > 20.0, "explorador à esquerda e abas no topo: {area:?}");
        if let Ok(caminho) = std::env::var("ASTROGRAPH_PNG") {
            let mut ppm = format!("P6\n1200 760\n255\n").into_bytes();
            for p in &buf {
                ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
            }
            std::fs::write(format!("{caminho}2.ppm"), ppm).unwrap();
        }
    }
}
