//! Renderizacao por CPU com softbuffer e atlas de glifos (ab_glyph).
//!
//! Cada quadro repinta so as linhas que mudaram (mais as linhas por onde o
//! cursor passou) e devolve essas faixas para `present_with_damage`: digitar
//! uma letra custa uma linha, nao a tela inteira. Cada linha e pintada em tres
//! passadas (fundos, glifos, decoracoes) para um caractere largo ou um glifo
//! que invade a celula vizinha nunca ser apagado pelo fundo do lado.
//!
//! Suporta uma cadeia de fontes de fallback (emoji, simbolos, CJK) por glifo.
//!
//! Ligaduras (`->`, `=>`, `!=`, `===`): so trechos de pontuacao ASCII de mesma
//! cor e estilo passam pelo shaping (rustybuzz), com o resultado em cache por
//! texto. As fontes de programacao trocam cada caractere por um glifo de uma
//! celula (as vezes invadindo a vizinha), entao a grade continua valendo e o
//! resto da linha segue o caminho rapido, celula a celula.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use ab_glyph::{point, Font, FontVec, Glyph, GlyphId, PxScale, ScaleFont};

use crate::config::FontCfg;
use crate::grid::{FloatRect, ScrollEvent, Cell, CellText, Grid, Resolved};

const ATLAS: usize = 2048;

/// Engrossa levemente a cobertura dos glifos (gama 1/1.15): o texto fica com o
/// peso "cheio" do macOS em vez do traco fino tipico do Windows.
const CONTRAST: f32 = 1.0 / 1.15;

const DEFAULTS: [(&str, &str, &str); 3] = [
    (
        "JetBrainsMonoNerdFontMono-Regular.ttf",
        "consola.ttf",
        "JetBrainsMonoNLNerdFont-Regular.ttf",
    ),
    (
        "JetBrainsMonoNerdFontMono-Bold.ttf",
        "consolab.ttf",
        "JetBrainsMonoNLNerdFont-Bold.ttf",
    ),
    (
        "JetBrainsMonoNerdFontMono-Italic.ttf",
        "consolai.ttf",
        "JetBrainsMonoNLNerdFont-Italic.ttf",
    ),
];

struct FontEntry {
    font: Rc<FontVec>,
    ratio: f32,
}

#[derive(Clone, Copy)]
struct GlyphInfo {
    ax: u32,
    ay: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

/// Glifo formado pelo shaping, relativo ao inicio do trecho.
#[derive(Clone, Copy)]
struct Shaped {
    id: u16,
    cell: u16,
    dx: i32,
    dy: i32,
}

/// Limite de trechos guardados por estilo (textos de pontuacao distintos).
const SHAPE_CACHE: usize = 4096;

/// O atlas encheu: o chamador limpa e tenta de novo.
struct AtlasFull;

#[derive(Clone, Copy, PartialEq)]
enum CursorShape {
    Block,
    Beam,
    Underline,
}

/// Onde a grade fica na janela (centralizada, com margem).
#[derive(Clone, Copy, PartialEq)]
pub struct Layout {
    pub cols: usize,
    pub rows: usize,
    pub ox: u32,
    pub oy: u32,
}

/// Retangulo do cursor desenhado (pixels), para apagar no proximo quadro.
#[derive(Clone, Copy, PartialEq)]
struct PxRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

/// Cursor deslizando entre celulas (ease-out), como no macOS.
struct CursorAnim {
    from: (f32, f32),
    to: (f32, f32),
    start: Instant,
}

/// Regiao deslizando depois de um `grid_scroll`: o conteudo novo comeca onde o
/// antigo estava e escorrega ate o lugar, com as linhas que sairam saindo junto.
struct ScrollAnim {
    top: usize,
    bot: usize,
    left: usize,
    right: usize,
    /// Deslocamento atual em pixels (> 0: conteudo desenhado abaixo do lugar final).
    offset: f32,
    /// Linhas que sairam por cima (a ultima e a vizinha da regiao).
    above: VecDeque<Vec<Cell>>,
    /// Linhas que sairam por baixo (a primeira e a vizinha da regiao).
    below: VecDeque<Vec<Cell>>,
    last: Instant,
}

impl ScrollAnim {
    fn bounds(&self) -> (usize, usize, usize, usize) {
        (self.top, self.bot, self.left, self.right)
    }

    fn overlaps(&self, top: usize, bot: usize, left: usize, right: usize) -> bool {
        self.top < bot && top < self.bot && self.left < right && left < self.right
    }
}

pub struct Renderer {
    styles: Vec<FontEntry>,
    fallbacks: Vec<FontEntry>,
    px: f32,
    line_height: f32,
    pub cell_w: u32,
    pub cell_h: u32,
    baseline: i32,
    line_px: u32,
    strike_y: i32,
    padding: u32,
    atlas: Vec<u8>,
    pen_x: u32,
    pen_y: u32,
    row_h: u32,
    cache: HashMap<(char, u8), Option<GlyphInfo>>,
    /// Glifos por id (saida do shaping), na fonte principal de cada estilo.
    by_id: HashMap<(u16, u8), Option<GlyphInfo>>,
    ligatures: bool,
    /// Trecho -> glifos; `None` quando o shaping nao muda nada (caminho rapido).
    shaped: [HashMap<Box<str>, Option<Rc<[Shaped]>>>; 3],
    run_text: String,
    ascii: [[Option<Option<GlyphInfo>>; 128]; 3],
    contrast: [u8; 256],
    anim_ms: u64,
    anim: Option<CursorAnim>,
    last_cursor: Option<PxRect>,
    /// Indicador de trabalho em segundo plano (quadro da animacao) e a linha em
    /// que foi pintado, para repintar a linha quando ele muda ou some.
    spinner: Option<usize>,
    spinner_row: Option<usize>,
    last_size: (u32, u32),
    last_layout: Option<Layout>,
    invalid: bool,
    paint: Vec<bool>,
    scroll_ms: u64,
    scrolls: Vec<ScrollAnim>,
    floats: Vec<FloatRect>,
    /// Forma do cursor em transicao: (largura, altura, deslocamento y) em pixels.
    cursor_dims: Option<(f32, f32, f32)>,
    cursor_dims_at: Option<Instant>,
}

impl Renderer {
    pub fn new(cfg: &FontCfg, anim_ms: u64) -> Result<Self, String> {
        let styles = resolve_styles(cfg)?;
        let fallbacks = load_fallbacks(&cfg.fallback);
        let mut contrast = [0u8; 256];
        for (i, slot) in contrast.iter_mut().enumerate() {
            *slot = ((i as f32 / 255.0).powf(CONTRAST) * 255.0).round() as u8;
        }
        let mut renderer = Renderer {
            styles,
            fallbacks,
            px: cfg.size.clamp(6.0, 96.0),
            line_height: cfg.line_height.clamp(0.8, 3.0),
            cell_w: 1,
            cell_h: 1,
            baseline: 0,
            line_px: 1,
            strike_y: 0,
            padding: 0,
            atlas: vec![0u8; ATLAS * ATLAS],
            pen_x: 0,
            pen_y: 0,
            row_h: 0,
            cache: HashMap::new(),
            by_id: HashMap::new(),
            ligatures: cfg.ligatures,
            shaped: Default::default(),
            run_text: String::new(),
            ascii: [[None; 128]; 3],
            contrast,
            anim_ms,
            anim: None,
            last_cursor: None,
            spinner: None,
            spinner_row: None,
            last_size: (0, 0),
            last_layout: None,
            invalid: true,
            paint: Vec::new(),
            scroll_ms: 0,
            scrolls: Vec::new(),
            floats: Vec::new(),
            cursor_dims: None,
            cursor_dims_at: None,
        };
        renderer.recompute_metrics();
        Ok(renderer)
    }

    fn scale_for(&self, entry: &FontEntry) -> PxScale {
        PxScale::from(self.px * entry.ratio)
    }

    fn recompute_metrics(&mut self) {
        let primary = &self.styles[0];
        let scaled = primary.font.as_scaled(self.scale_for(primary));
        let ascent = scaled.ascent();
        let descent = scaled.descent();
        if std::env::var("DEBUG_FONT").is_ok() {
            println!(
                "FONT_DBG: em={:.1}px ratio={:.3} ascent={:.2} descent={:.2} line_gap={:.2} advM={:.2} fallbacks={}",
                self.px,
                primary.ratio,
                ascent,
                descent,
                scaled.line_gap(),
                scaled.h_advance(primary.font.glyph_id('M')),
                self.fallbacks.len(),
            );
        }
        let natural = ascent - descent + scaled.line_gap();
        self.cell_h = (natural * self.line_height).ceil().max(1.0) as u32;
        self.cell_w = (scaled.h_advance(primary.font.glyph_id('M')).round() as u32).max(1);
        // com line_height > 1 o texto fica centralizado na linha
        let extra = (self.cell_h as f32 - natural).max(0.0) / 2.0;
        self.baseline = (ascent + extra).round() as i32;
        self.line_px = (self.px / 14.0).round().max(1.0) as u32;
        self.strike_y = self.baseline - (ascent * 0.3).round() as i32;
        // deslocamentos do shaping dependem do tamanho
        self.shaped.iter_mut().for_each(HashMap::clear);
        self.reset_atlas();
        self.invalid = true;
    }

    fn reset_atlas(&mut self) {
        self.atlas.fill(0);
        self.pen_x = 0;
        self.pen_y = 0;
        self.row_h = 0;
        self.cache.clear();
        self.by_id.clear();
        self.ascii = [[None; 128]; 3];
    }

    /// Ajusta o tamanho da fonte (zoom).
    pub fn set_size(&mut self, px: f32) {
        self.px = px.clamp(6.0, 96.0);
        self.recompute_metrics();
    }

    pub fn font_size(&self) -> f32 {
        self.px
    }

    /// Margem em pixels ao redor da grade.
    pub fn set_padding(&mut self, px: u32) {
        if px != self.padding {
            self.padding = px;
            self.invalid = true;
        }
    }

    /// Forca repintar a janela inteira no proximo quadro.
    pub fn invalidate(&mut self) {
        self.invalid = true;
    }

    pub fn layout(&self, win_w: u32, win_h: u32) -> Layout {
        let avail_w = win_w.saturating_sub(2 * self.padding);
        let avail_h = win_h.saturating_sub(2 * self.padding);
        let cols = (avail_w / self.cell_w).max(1) as usize;
        let rows = (avail_h / self.cell_h).max(1) as usize;
        // a sobra (menor que uma celula) fica dividida entre os lados
        let ox = self.padding + avail_w.saturating_sub(cols as u32 * self.cell_w) / 2;
        let oy = self.padding + avail_h.saturating_sub(rows as u32 * self.cell_h) / 2;
        Layout { cols, rows, ox, oy }
    }

    /// Maior deslocamento de rolagem ativo, em pixels (testes de fumaca).
    pub fn scroll_offset(&self) -> f32 {
        self.scrolls.iter().map(|a| a.offset.abs()).fold(0.0, f32::max)
    }

    /// Liga (`Some(quadro)`) ou desliga o indicador de carregamento do canto
    /// inferior direito.
    pub fn set_spinner(&mut self, frame: Option<usize>) {
        self.spinner = frame;
    }

    /// Duracao da rolagem suave (0 desliga).
    pub fn set_scroll_ms(&mut self, ms: u64) {
        self.scroll_ms = ms;
        if ms == 0 {
            self.scrolls.clear();
        }
    }

    /// Algo ainda esta em movimento (a UI deve pedir outro quadro).
    pub fn animating(&self, now: Instant) -> bool {
        !self.scrolls.is_empty()
            || self.cursor_dims_at.is_some()
            || self
                .anim
                .as_ref()
                .is_some_and(|a| now.duration_since(a.start) < Duration::from_millis(self.anim_ms))
    }

    /// Acrescenta uma rolagem do Neovim a animacao da regiao (rolagens seguidas
    /// se somam: segurar `j` desliza continuo, sem trancos).
    fn ingest_scroll(&mut self, ev: ScrollEvent, now: Instant) {
        let ch = self.cell_h as f32;
        let height = ev.bot - ev.top;
        let key = (ev.top, ev.bot, ev.left, ev.right);
        // outra regiao no mesmo lugar (janela redimensionada): recomeca
        self.scrolls.retain(|a| a.bounds() == key || !a.overlaps(ev.top, ev.bot, ev.left, ev.right));
        let idx = match self.scrolls.iter().position(|a| a.bounds() == key) {
            Some(i) => i,
            None => {
                self.scrolls.push(ScrollAnim {
                    top: ev.top,
                    bot: ev.bot,
                    left: ev.left,
                    right: ev.right,
                    offset: 0.0,
                    above: VecDeque::new(),
                    below: VecDeque::new(),
                    last: now,
                });
                self.scrolls.len() - 1
            }
        };
        let a = &mut self.scrolls[idx];
        // mudou de direcao no meio: parte do ponto atual sem o historico oposto
        if (ev.rows > 0 && a.offset < 0.0) || (ev.rows < 0 && a.offset > 0.0) {
            a.offset = 0.0;
            a.above.clear();
            a.below.clear();
        }
        let n = ev.rows.unsigned_abs() as f32;
        if ev.rows > 0 {
            a.above.extend(ev.leaving);
            while a.above.len() > height {
                a.above.pop_front();
            }
            a.offset = (a.offset + n * ch).min(a.above.len() as f32 * ch);
        } else {
            for row in ev.leaving.into_iter().rev() {
                a.below.push_front(row);
            }
            while a.below.len() > height {
                a.below.pop_back();
            }
            a.offset = (a.offset - n * ch).max(-(a.below.len() as f32) * ch);
        }
    }

    /// Desenha uma regiao rolando: conteudo atual deslocado e, na fresta, as
    /// linhas que sairam.
    fn paint_scroll(&mut self, buffer: &mut [u32], win_w: u32, win_h: u32, grid: &Grid, lay: &Layout, a: &ScrollAnim) {
        let (cw, ch) = (self.cell_w, self.cell_h);
        let right = a.right.min(grid.width).min(lay.cols);
        if a.bot > grid.height || a.left >= right {
            return;
        }
        let rx0 = lay.ox + a.left as u32 * cw;
        let ry0 = lay.oy + a.top as u32 * ch;
        let ry_end = lay.oy + a.bot as u32 * ch;
        let ry1 = ry_end.min(win_h);
        if ry0 >= ry1 {
            return;
        }
        let clip = (ry0, ry1);
        let width = right - a.left;
        fill_rect(buffer, win_w, ry1, rx0, ry0, width as u32 * cw, ry1 - ry0, grid.default_bg);
        let off = a.offset.round() as i32;
        let w = grid.width;
        for i in 0..(a.bot - a.top) {
            let y = ry0 as i32 + (i as u32 * ch) as i32 + off;
            if y + ch as i32 <= ry0 as i32 || y >= ry1 as i32 {
                continue;
            }
            let start = (a.top + i) * w + a.left;
            self.paint_cells(buffer, win_w, grid, &grid.cells[start..start + width], rx0, y, clip, None);
        }
        if off > 0 {
            for (k, row) in a.above.iter().rev().enumerate() {
                let y = ry0 as i32 - ((k as u32 + 1) * ch) as i32 + off;
                if y + ch as i32 <= ry0 as i32 {
                    break;
                }
                let n = row.len().min(width);
                self.paint_cells(buffer, win_w, grid, &row[..n], rx0, y, clip, None);
            }
        } else if off < 0 {
            for (k, row) in a.below.iter().enumerate() {
                let y = ry_end as i32 + (k as u32 * ch) as i32 + off;
                if y >= ry1 as i32 {
                    break;
                }
                let n = row.len().min(width);
                self.paint_cells(buffer, win_w, grid, &row[..n], rx0, y, clip, None);
            }
        }
    }

    /// Sombra suave sob as janelas flutuantes com borda (paineis "elevados", como
    /// os popovers do macOS). So toca as linhas repintadas neste quadro.
    fn draw_shadows(&self, buffer: &mut [u32], win_w: u32, win_h: u32, lay: &Layout, rows: usize, full: bool) {
        let (cw, ch) = (self.cell_w as f32, self.cell_h as f32);
        let px = |f: &FloatRect| {
            (
                lay.ox as f32 + f.col as f32 * cw,
                lay.oy as f32 + f.row as f32 * ch,
                f.width as f32 * cw,
                f.height as f32 * ch,
            )
        };
        let inside = |r: (f32, f32, f32, f32), x: f32, y: f32| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3;
        let contains = |outer: &FloatRect, inner: &FloatRect| {
            !std::ptr::eq(outer, inner)
                && inner.row >= outer.row
                && inner.col >= outer.col
                && inner.row + inner.height <= outer.row + outer.height
                && inner.col + inner.width <= outer.col + outer.width
        };
        let blur = ch * 1.1;
        let drop = ch * 0.3;
        let radius = cw * 0.5;
        for f in &self.floats {
            // paineis dentro de outro painel (as partes do picker) nao fazem sombra
            if self.floats.iter().any(|o| contains(o, f)) || f.blend >= 100 {
                continue;
            }
            let strength = 0.6 * (1.0 - f.blend as f32 / 100.0);
            if strength <= 0.01 {
                continue;
            }
            let r = px(f);
            let above: Vec<(f32, f32, f32, f32)> = self.floats.iter().filter(|o| o.zindex > f.zindex).map(px).collect();
            let (hw, hh) = (r.2 / 2.0, r.3 / 2.0);
            let (cx, cy) = (r.0 + hw, r.1 + drop + hh);
            let x_from = (r.0 - blur).max(0.0) as u32;
            let x_to = ((r.0 + r.2 + blur).ceil() as u32).min(win_w);
            let y_from = (r.1 + drop - blur).max(0.0) as u32;
            let y_to = ((r.1 + drop + r.3 + blur).ceil() as u32).min(win_h);
            for y in y_from..y_to {
                // linha de pixels pertence a uma linha repintada?
                if !full {
                    if y < lay.oy {
                        continue;
                    }
                    let row = ((y - lay.oy) as f32 / ch) as usize;
                    if row >= rows || !self.paint[row] {
                        continue;
                    }
                }
                let fy = y as f32 + 0.5;
                let base = y as usize * win_w as usize;
                for x in x_from..x_to {
                    let fx = x as f32 + 0.5;
                    if inside(r, fx, fy) || above.iter().any(|o| inside(*o, fx, fy)) {
                        continue;
                    }
                    let qx = ((fx - cx).abs() - hw + radius).max(0.0);
                    let qy = ((fy - cy).abs() - hh + radius).max(0.0);
                    let d = (qx * qx + qy * qy).sqrt() - radius;
                    let t = (d / blur).clamp(0.0, 1.0);
                    let a = strength * (1.0 - t * t * (3.0 - 2.0 * t));
                    if a > 0.004 {
                        let idx = base + x as usize;
                        buffer[idx] = mix(0, buffer[idx], (a * 255.0) as u32);
                    }
                }
            }
        }
    }

    fn style(bold: bool, italic: bool) -> u8 {
        match (bold, italic) {
            (true, _) => 1,
            (false, true) => 2,
            _ => 0,
        }
    }

    fn cursor_shape(mode: &str) -> CursorShape {
        if mode.contains("insert") || mode.starts_with("cmdline") {
            CursorShape::Beam
        } else if mode.contains("replace") {
            CursorShape::Underline
        } else {
            CursorShape::Block
        }
    }

    /// Posicao do cursor neste instante (desliza ate o alvo em `anim_ms`).
    fn cursor_pos(&mut self, target: (f32, f32), now: Instant) -> (f32, f32) {
        if self.anim_ms == 0 {
            return target;
        }
        let total = self.anim_ms as f32;
        let current = match &self.anim {
            Some(a) => {
                let t = (now.duration_since(a.start).as_secs_f32() * 1000.0 / total).min(1.0);
                let e = 1.0 - (1.0 - t).powi(3);
                (a.from.0 + (a.to.0 - a.from.0) * e, a.from.1 + (a.to.1 - a.from.1) * e)
            }
            None => target,
        };
        let moved = self.anim.as_ref().map_or(true, |a| a.to != target);
        if moved {
            // primeira posicao conhecida: aparece direto, sem deslizar do canto
            let from = if self.anim.is_some() { current } else { target };
            self.anim = Some(CursorAnim { from, to: target, start: now });
            return from;
        }
        current
    }

    /// Desenha o que mudou. Devolve as faixas verticais alteradas `(y, altura)`.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        buffer: &mut [u32],
        win_w: u32,
        win_h: u32,
        grid: &mut Grid,
        cursor_alpha: f32,
        focused: bool,
        now: Instant,
        full: bool,
    ) -> Vec<(u32, u32)> {
        let cursor_visible = cursor_alpha > 0.01;
        let lay = self.layout(win_w, win_h);
        let floats_changed = std::mem::take(&mut grid.floats_changed);
        if floats_changed {
            self.floats.clone_from(&grid.floats);
        }
        let geometry_changed = self.last_size != (win_w, win_h) || self.last_layout != Some(lay);
        let full = full || self.invalid || grid.all_dirty || geometry_changed || floats_changed;
        let rows = lay.rows.min(grid.height);
        let (cw, ch) = (self.cell_w, self.cell_h);

        // rolagem suave: novas rolagens entram, as atuais desaceleram
        let events = std::mem::take(&mut grid.scrolls);
        if geometry_changed || self.scroll_ms == 0 {
            self.scrolls.clear();
        } else {
            for ev in events {
                // regiao sob uma janela flutuante: o Neovim redesenha, nao anima
                let covered = self.floats.iter().any(|f| {
                    f.row < ev.bot && ev.top < f.row + f.height && f.col < ev.right && ev.left < f.col + f.width
                });
                if !covered {
                    self.ingest_scroll(ev, now);
                }
            }
        }
        let tau = self.scroll_ms as f32 / 4000.0; // ~95% do caminho em 3/4 da duracao
        let mut scroll_rows: Vec<(usize, usize)> = Vec::new();
        for a in &mut self.scrolls {
            let dt = now.duration_since(a.last).as_secs_f32();
            a.last = now;
            a.offset *= (-dt / tau.max(0.001)).exp();
            scroll_rows.push((a.top, a.bot));
        }
        self.scrolls.retain(|a| a.offset.abs() >= 0.5);

        // cursor: alvo em pixels e retangulo deste quadro
        let (crow, ccol) = grid.cursor;
        let cursor_in = crow < rows && ccol < lay.cols.min(grid.width);
        let target = (
            (lay.ox + ccol as u32 * cw) as f32,
            (lay.oy + crow as u32 * ch) as f32,
        );
        let pos = self.cursor_pos(target, now);
        let settled = pos == target;
        let shape = Self::cursor_shape(&grid.mode);
        let wide = cursor_in
            && grid
                .cells
                .get(crow * grid.width + ccol + 1)
                .is_some_and(|c| c.cont);
        // o cursor acompanha o texto que esta deslizando
        let cursor_dy = self
            .scrolls
            .iter()
            .find(|a| (a.top..a.bot).contains(&crow) && (a.left..a.right).contains(&ccol))
            .map_or(0.0, |a| a.offset);
        // forma do cursor: bloco, barra ou sublinhado; a troca e animada (o bloco
        // afina ate virar barra ao entrar no modo de insercao)
        let block_w = if wide { cw * 2 } else { cw } as f32;
        let target_dims = match (shape, focused) {
            (CursorShape::Beam, true) => ((cw / 8).max(2) as f32, ch as f32, 0.0),
            (CursorShape::Underline, true) => {
                let h = (ch / 8).max(2) as f32;
                (block_w, h, ch as f32 - h)
            }
            _ => (block_w, ch as f32, 0.0),
        };
        let dims = match (self.cursor_dims, self.cursor_dims_at) {
            (Some(d), Some(at)) if self.anim_ms > 0 => {
                let k = 1.0 - (-now.duration_since(at).as_secs_f32() / 0.028).exp();
                let step = |a: f32, b: f32| a + (b - a) * k;
                let next = (step(d.0, target_dims.0), step(d.1, target_dims.1), step(d.2, target_dims.2));
                let close = (next.0 - target_dims.0).abs() < 0.5
                    && (next.1 - target_dims.1).abs() < 0.5
                    && (next.2 - target_dims.2).abs() < 0.5;
                if close { target_dims } else { next }
            }
            (Some(d), None) if d != target_dims && self.anim_ms > 0 => d,
            _ => target_dims,
        };
        self.cursor_dims_at = (dims != target_dims).then_some(now);
        self.cursor_dims = Some(dims);
        let morphing = dims != target_dims;
        let cursor_rect = (cursor_visible && cursor_in).then(|| {
            let (x, y) = (pos.0.round() as i32, (pos.1 + cursor_dy).round() as i32);
            PxRect {
                x,
                y: y + dims.2.round() as i32,
                w: (dims.0.round() as u32).max(1),
                h: (dims.1.round() as u32).max(1),
            }
        });

        // linhas a repintar
        self.paint.clear();
        self.paint.resize(rows, full);
        if !full {
            for (r, slot) in self.paint.iter_mut().enumerate() {
                *slot = grid.dirty.get(r).copied().unwrap_or(false);
            }
            for &(top, bot) in &scroll_rows {
                for slot in self.paint.iter_mut().take(bot).skip(top) {
                    *slot = true;
                }
            }
            let spin_row = rows.saturating_sub(3);
            for r in [self.spinner_row, self.spinner.map(|_| spin_row)].into_iter().flatten() {
                if let Some(slot) = self.paint.get_mut(r) {
                    *slot = true;
                }
            }
            for rect in [self.last_cursor, cursor_rect].into_iter().flatten() {
                let top = (rect.y - lay.oy as i32).max(0) as u32 / ch;
                let bot = ((rect.y + rect.h as i32 - 1 - lay.oy as i32).max(0) as u32) / ch;
                for r in top..=bot {
                    if let Some(slot) = self.paint.get_mut(r as usize) {
                        *slot = true;
                    }
                }
            }
        } else {
            buffer.fill(grid.default_bg);
        }

        for r in 0..rows {
            if self.paint[r] {
                self.paint_row(buffer, win_w, win_h, grid, &lay, r);
            }
        }
        let anims = std::mem::take(&mut self.scrolls);
        for a in &anims {
            self.paint_scroll(buffer, win_w, win_h, grid, &lay, a);
        }
        self.scrolls = anims;
        if !self.floats.is_empty() {
            self.draw_shadows(buffer, win_w, win_h, &lay, rows, full);
        }

        self.spinner_row = None;
        if let Some(frame) = self.spinner {
            let row = rows.saturating_sub(3);
            self.draw_spinner(buffer, win_w, win_h, grid, &lay, row, frame);
            self.spinner_row = Some(row);
        }

        if let Some(rect) = cursor_rect {
            let a8 = (cursor_alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
            self.draw_cursor(buffer, win_w, win_h, grid, rect, shape, focused, settled && cursor_in && !morphing, a8);
        }
        self.last_cursor = cursor_rect;
        self.last_size = (win_w, win_h);
        self.last_layout = Some(lay);
        self.invalid = false;
        grid.clean();

        if full {
            return vec![(0, win_h)];
        }
        // junta linhas vizinhas numa faixa so
        let mut spans: Vec<(u32, u32)> = Vec::new();
        for r in 0..rows {
            if !self.paint[r] {
                continue;
            }
            let y = lay.oy + r as u32 * ch;
            match spans.last_mut() {
                Some((sy, sh)) if *sy + *sh == y => *sh += ch,
                _ => spans.push((y, ch)),
            }
        }
        spans
            .into_iter()
            .filter_map(|(y, h)| (y < win_h).then(|| (y, h.min(win_h - y))))
            .collect()
    }

    /// Anel de 8 pontos girando, na penultima coluna e acima da linha de status.
    /// Pontos quadrados, sem depender de fonte: o mais novo e o mais forte e os
    /// outros esmaecem para a cor de fundo.
    #[allow(clippy::too_many_arguments)]
    fn draw_spinner(&self, buffer: &mut [u32], win_w: u32, win_h: u32, grid: &Grid, lay: &Layout, row: usize, frame: usize) {
        let (cw, ch) = (self.cell_w as i32, self.cell_h as i32);
        let col = lay.cols.saturating_sub(2) as i32;
        let (cx, cy) = (
            lay.ox as i32 + col * cw + cw / 2,
            lay.oy as i32 + row as i32 * ch + ch / 2,
        );
        let radius = (ch as f32 * 0.26).max(3.0);
        let dot = ((ch / 9).max(2)) as i32;
        let (fg, bg) = (grid.default_fg, grid.default_bg);
        for i in 0..8usize {
            let age = (frame + 8 - i) % 8; // 0 = ponto da frente
            let strength = 255 - (age as u32 * 28).min(220);
            let color = mix(fg, bg, strength * 7 / 10 + 40);
            let ang = i as f32 * std::f32::consts::TAU / 8.0;
            let (dx, dy) = ((ang.cos() * radius).round() as i32, (ang.sin() * radius).round() as i32);
            let (x0, y0) = (cx + dx - dot / 2, cy + dy - dot / 2);
            for y in y0.max(0)..(y0 + dot).min(win_h as i32) {
                for x in x0.max(0)..(x0 + dot).min(win_w as i32) {
                    if let Some(px) = buffer.get_mut((y as u32 * win_w + x as u32) as usize) {
                        *px = color;
                    }
                }
            }
        }
    }

    fn paint_row(&mut self, buffer: &mut [u32], win_w: u32, win_h: u32, grid: &Grid, lay: &Layout, r: usize) {
        let ch = self.cell_h;
        let y0 = lay.oy + r as u32 * ch;
        let y1 = (y0 + ch).min(win_h);
        // a faixa inteira (inclusive margens laterais) volta ao fundo padrao
        fill_rect(buffer, win_w, win_h, 0, y0, win_w, ch, grid.default_bg);
        let cols = lay.cols.min(grid.width);
        let start = r * grid.width;
        let cursor_col = (grid.cursor.0 == r).then_some(grid.cursor.1);
        self.paint_cells(buffer, win_w, grid, &grid.cells[start..start + cols], lay.ox, y0 as i32, (y0, y1), cursor_col);
    }

    /// Pinta uma sequencia de celulas com a origem em `(x0, y)`, recortada nas
    /// linhas de pixel `clip` (a rolagem suave desenha linhas fora da grade).
    #[allow(clippy::too_many_arguments)]
    fn paint_cells(
        &mut self,
        buffer: &mut [u32],
        win_w: u32,
        grid: &Grid,
        cells: &[Cell],
        x0: u32,
        y: i32,
        clip: (u32, u32),
        cursor_col: Option<usize>,
    ) {
        let (cw, ch) = (self.cell_w, self.cell_h);
        // trecho visivel desta linha
        let vis = (y.max(clip.0 as i32) as u32, ((y + ch as i32).max(0) as u32).min(clip.1));
        if vis.0 >= vis.1 {
            return;
        }
        let cols = cells.len();

        // resolve os atributos uma vez por celula (celulas vizinhas repetem o hl)
        let mut res: Vec<Resolved> = Vec::with_capacity(cols);
        let mut last: Option<(u64, Resolved)> = None;
        for cell in cells {
            let resolved = match last {
                Some((id, value)) if id == cell.hl => value,
                _ => {
                    let value = grid.resolve(cell.hl);
                    last = Some((cell.hl, value));
                    value
                }
            };
            res.push(resolved);
        }

        // 1) fundos, em trechos de mesma cor
        let mut c = 0;
        while c < cols {
            let bg = res[c].bg;
            let mut end = c + 1;
            while end < cols && res[end].bg == bg {
                end += 1;
            }
            if bg != grid.default_bg {
                let x = x0 + c as u32 * cw;
                fill_rect(buffer, win_w, vis.1, x, vis.0, (end - c) as u32 * cw, vis.1 - vis.0, bg);
            }
            c = end;
        }

        // 2) glifos (recortados no trecho visivel); o trecho sob o cursor fica
        // sem ligadura para o caractere da celula aparecer
        let mut c = 0;
        while c < cols {
            let cell = &cells[c];
            if cell.cont || cell.text.is_blank() {
                c += 1;
                continue;
            }
            let x = x0 + c as u32 * cw;
            let style = Self::style(res[c].bold, res[c].italic);
            let fg = res[c].fg;
            if self.ligatures && ligature_char(cell) {
                let mut end = c + 1;
                while end < cols
                    && ligature_char(&cells[end])
                    && res[end].fg == fg
                    && Self::style(res[end].bold, res[end].italic) == style
                {
                    end += 1;
                }
                let under_cursor = cursor_col.is_some_and(|k| (c..end).contains(&k));
                let shaped = if end - c >= 2 && !under_cursor { self.shape_run(&cells[c..end], style) } else { None };
                match shaped {
                    Some(glyphs) => {
                        for g in glyphs.iter() {
                            let gx = x as i32 + g.cell as i32 * cw as i32 + g.dx;
                            if let Some(info) = self.glyph_by_id(g.id, style) {
                                self.draw_info(buffer, win_w, vis, gx, y + g.dy, info, fg);
                            }
                        }
                    }
                    None => {
                        for (k, cell) in cells.iter().enumerate().take(end).skip(c) {
                            let kx = x0 + k as u32 * cw;
                            self.blit(buffer, win_w, vis, kx, y, cell.text.first(), fg, style);
                        }
                    }
                }
                c = end;
                continue;
            }
            self.draw_char(buffer, win_w, vis, x, y, cell.text.first(), fg, style);
            for extra in cell.text.rest() {
                self.blit(buffer, win_w, vis, x, y, extra, fg, style);
            }
            c += 1;
        }

        // 3) sublinhado, ondulado e riscado
        for (c, a) in res.iter().enumerate() {
            if !(a.underline || a.undercurl || a.strikethrough) {
                continue;
            }
            let x = x0 + c as u32 * cw;
            let w = if cells[c..].get(1).is_some_and(|n| n.cont) { cw * 2 } else { cw };
            let t = self.line_px;
            let line_at = |yy: i32| -> Option<u32> {
                (yy >= vis.0 as i32 && yy < vis.1 as i32).then_some(yy as u32)
            };
            if a.underline {
                let uy = (y + self.baseline + t as i32 + 1).min(y + ch as i32 - t as i32);
                if let Some(uy) = line_at(uy) {
                    fill_rect(buffer, win_w, vis.1, x, uy, w, t, a.sp);
                }
            }
            if a.undercurl {
                self.undercurl(buffer, win_w, vis, x, y, w, a.sp);
            }
            if a.strikethrough {
                if let Some(sy) = line_at(y + self.strike_y) {
                    fill_rect(buffer, win_w, vis.1, x, sy, w, t, a.fg);
                }
            }
        }
    }

    /// Linha ondulada suave (antisserrilhada), continua entre celulas.
    #[allow(clippy::too_many_arguments)]
    fn undercurl(&self, buffer: &mut [u32], win_w: u32, clip: (u32, u32), x0: u32, y: i32, w: u32, color: u32) {
        let amp = (self.cell_h as f32 / 14.0).max(1.0);
        let base = y as f32 + self.baseline as f32 + amp + 1.5;
        let period = self.cell_w as f32;
        for x in x0..(x0 + w).min(win_w) {
            let yf = base + amp * (x as f32 / period * std::f32::consts::TAU).sin();
            let yi = yf.floor() as i32;
            let frac = yf - yi as f32;
            for (y, a) in [(yi, 1.0 - frac), (yi + 1, frac)] {
                if y < clip.0 as i32 || y >= clip.1 as i32 {
                    continue;
                }
                let idx = y as usize * win_w as usize + x as usize;
                buffer[idx] = mix(color, buffer[idx], (a * 255.0) as u32);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cursor(
        &mut self,
        buffer: &mut [u32],
        win_w: u32,
        win_h: u32,
        grid: &Grid,
        rect: PxRect,
        shape: CursorShape,
        focused: bool,
        settled: bool,
        alpha: u32,
    ) {
        let (row, col) = grid.cursor;
        let cell = &grid.cells[row * grid.width + col];
        let res = grid.resolve(cell.hl);
        let (x, y) = (rect.x.max(0) as u32, rect.y.max(0) as u32);
        if !focused {
            // janela sem foco: so o contorno, como no Terminal do macOS
            fill_rect(buffer, win_w, win_h, x, y, rect.w, 1, res.fg);
            fill_rect(buffer, win_w, win_h, x, y + rect.h - 1, rect.w, 1, res.fg);
            fill_rect(buffer, win_w, win_h, x, y, 1, rect.h, res.fg);
            fill_rect(buffer, win_w, win_h, x + rect.w - 1, y, 1, rect.h, res.fg);
            return;
        }
        // piscar com fade: o cursor se mistura ao que esta embaixo
        blend_rect(buffer, win_w, win_h, x, y, rect.w, rect.h, res.fg, alpha);
        // bloco parado: o caractere aparece "vazado" na cor do fundo
        if shape == CursorShape::Block && settled && !cell.cont && !cell.text.is_blank() {
            let style = Self::style(res.bold, res.italic);
            let clip = (y, (y + rect.h).min(win_h));
            let color = mix(res.bg, res.fg, alpha);
            self.draw_char(buffer, win_w, clip, x, y as i32, cell.text.first(), color, style);
            for extra in cell.text.rest() {
                self.blit(buffer, win_w, clip, x, y as i32, extra, color, style);
            }
        }
    }

    /// Desenha um caractere: blocos e linhas de moldura sao geometria propria
    /// (preenchem a celula sem frestas, em qualquer fonte e zoom); o resto vem
    /// do atlas.
    #[allow(clippy::too_many_arguments)]
    fn draw_char(&mut self, buffer: &mut [u32], win_w: u32, clip: (u32, u32), x: u32, y: i32, ch: char, color: u32, style: u8) {
        // geometria so com a celula inteira abaixo do topo da janela (y >= 0)
        if y < 0 || !self.draw_geometric(buffer, win_w, clip, x, y as u32, ch, color) {
            self.blit(buffer, win_w, clip, x, y, ch, color, style);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_geometric(&self, buffer: &mut [u32], win_w: u32, clip: (u32, u32), x0: u32, y0: u32, ch: char, color: u32) -> bool {
        let (cw, chh) = (self.cell_w, self.cell_h);
        // retangulo em fracoes da celula, recortado na linha
        let mut rect = |fx0: f32, fy0: f32, fx1: f32, fy1: f32, alpha: u32| {
            let xa = x0 + (fx0 * cw as f32).round() as u32;
            let xb = x0 + (fx1 * cw as f32).round() as u32;
            let ya = (y0 + (fy0 * chh as f32).round() as u32).max(clip.0);
            let yb = (y0 + (fy1 * chh as f32).round() as u32).min(clip.1);
            let xb = xb.min(win_w);
            for y in ya..yb {
                let row = y as usize * win_w as usize;
                for px in &mut buffer[row + xa as usize..row + xb as usize] {
                    *px = if alpha >= 255 { color } else { mix(color, *px, alpha) };
                }
            }
        };
        let code = ch as u32;
        match code {
            // blocos: ▀ ▁..▇ █ ▉..▏ ▐ ░ ▒ ▓ ▔ ▕ e quadrantes
            0x2580 => rect(0.0, 0.0, 1.0, 0.5, 255),
            0x2581..=0x2588 => rect(0.0, 1.0 - (code - 0x2580) as f32 / 8.0, 1.0, 1.0, 255),
            0x2589..=0x258F => rect(0.0, 0.0, (0x2590 - code) as f32 / 8.0, 1.0, 255),
            0x2590 => rect(0.5, 0.0, 1.0, 1.0, 255),
            0x2591..=0x2593 => rect(0.0, 0.0, 1.0, 1.0, 64 * (code - 0x2590)),
            0x2594 => rect(0.0, 0.0, 1.0, 0.125, 255),
            0x2595 => rect(0.875, 0.0, 1.0, 1.0, 255),
            0x2596..=0x259F => {
                const Q: [u8; 10] = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14];
                let m = Q[(code - 0x2596) as usize];
                for (bit, (fx, fy)) in [(1, (0.0, 0.0)), (2, (0.5, 0.0)), (4, (0.0, 0.5)), (8, (0.5, 0.5))] {
                    if m & bit != 0 {
                        rect(fx, fy, fx + 0.5, fy + 0.5, 255);
                    }
                }
            }
            // molduras finas e grossas (U+2500..254B, meias linhas U+2574..257F):
            // cada braco com o peso proprio; as juncoes ficam solidas
            0x2500..=0x2503 | 0x250C..=0x254B | 0x2574..=0x257F => {
                let Some([l, r, u, d]) = box_arms(code) else {
                    return false;
                };
                let weight = |w: u8| match w {
                    0 => 0,
                    1 => self.line_px,
                    _ => (self.line_px * 2).max(2),
                };
                let (tl, tr, tu, td) = (weight(l), weight(r), weight(u), weight(d));
                let th = tl.max(tr); // espessura da faixa horizontal no centro
                let tv = tu.max(td);
                let mid_x = x0 + cw / 2;
                let mid_y = y0 + chh / 2;
                // os bracos horizontais cobrem a largura da faixa vertical (e vice-versa)
                let cover_x0 = mid_x.saturating_sub(tv.max(th) / 2 + tv.max(th) % 2);
                let cover_x1 = mid_x + tv.max(th) / 2;
                let cover_y0 = mid_y.saturating_sub(th.max(tv) / 2 + th.max(tv) % 2);
                let cover_y1 = mid_y + th.max(tv) / 2;
                let hband = |t: u32| y0 + (chh - t.min(chh)) / 2;
                let vband = |t: u32| x0 + (cw - t.min(cw)) / 2;
                let mut hline = |xa: u32, xb: u32, t: u32| {
                    let y = hband(t);
                    if y + t > clip.0 && y < clip.1 {
                        let top = y.max(clip.0);
                        fill_rect(buffer, win_w, clip.1, xa, top, xb.saturating_sub(xa), y + t - top, color);
                    }
                };
                if l > 0 {
                    hline(x0, if u + d > 0 { cover_x1 } else { mid_x + tl / 2 }, tl);
                }
                if r > 0 {
                    hline(if u + d > 0 { cover_x0 } else { mid_x.saturating_sub(tr / 2 + tr % 2) }, x0 + cw, tr);
                }
                let mut vline = |ya: u32, yb: u32, t: u32| {
                    let x = vband(t);
                    let top = ya.max(clip.0);
                    if yb > top {
                        fill_rect(buffer, win_w, clip.1, x, top, t, yb - top, color);
                    }
                };
                if u > 0 {
                    vline(y0, if l + r > 0 { cover_y1 } else { mid_y + tu / 2 }, tu);
                }
                if d > 0 {
                    vline(if l + r > 0 { cover_y0 } else { mid_y.saturating_sub(td / 2 + td % 2) }, y0 + chh, td);
                }
            }
            // cantos arredondados: ╭ ╮ ╯ ╰ (arco antisserrilhado)
            0x256D..=0x2570 => {
                let (dx, dy): (f32, f32) = match code {
                    0x256D => (1.0, 1.0),
                    0x256E => (-1.0, 1.0),
                    0x256F => (-1.0, -1.0),
                    _ => (1.0, -1.0),
                };
                let t = self.line_px as f32;
                let cx = x0 as f32 + cw as f32 / 2.0;
                let cy = y0 as f32 + chh as f32 / 2.0;
                let rad = (cw.min(chh) as f32 / 2.0).max(1.0);
                let (acx, acy) = (cx + dx * rad, cy + dy * rad);
                let ti = self.line_px;
                let lx = x0 + (cw - ti.min(cw)) / 2;
                let ly = y0 + (chh - ti.min(chh)) / 2;
                // reta horizontal do fim do arco ate a borda
                let (h0, h1) = if dx > 0.0 { (acx.round() as u32, x0 + cw) } else { (x0, acx.round() as u32) };
                if ly >= clip.0 {
                    fill_rect(buffer, win_w, clip.1, h0, ly, h1.saturating_sub(h0), ti, color);
                }
                // reta vertical do fim do arco ate a borda
                let (v0, v1) = if dy > 0.0 { (acy.round() as u32, y0 + chh) } else { (y0, acy.round() as u32) };
                let v0 = v0.max(clip.0);
                fill_rect(buffer, win_w, clip.1, lx, v0, ti, v1.saturating_sub(v0), color);
                // arco: pixels no quadrante voltado para o centro da celula
                let xs = if dx > 0.0 { (cx - t).floor() as i32..acx.ceil() as i32 } else { acx.floor() as i32..(cx + t).ceil() as i32 };
                let ys = if dy > 0.0 { (cy - t).floor() as i32..acy.ceil() as i32 } else { acy.floor() as i32..(cy + t).ceil() as i32 };
                for py in ys {
                    if py < clip.0 as i32 || py >= clip.1 as i32 {
                        continue;
                    }
                    for px in xs.clone() {
                        if px < 0 || px >= win_w as i32 {
                            continue;
                        }
                        let (fx, fy) = (px as f32 + 0.5 - acx, py as f32 + 0.5 - acy);
                        if fx * dx > 0.0 || fy * dy > 0.0 {
                            continue;
                        }
                        let dist = ((fx * fx + fy * fy).sqrt() - rad).abs();
                        let cov = (t / 2.0 + 0.5 - dist).clamp(0.0, 1.0);
                        if cov > 0.0 {
                            let idx = py as usize * win_w as usize + px as usize;
                            buffer[idx] = mix(color, buffer[idx], (cov * 255.0) as u32);
                        }
                    }
                }
            }
            _ => return false,
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        buffer: &mut [u32],
        win_w: u32,
        clip: (u32, u32),
        x0: u32,
        y0: i32,
        ch: char,
        fg: u32,
        style: u8,
    ) {
        if let Some(info) = self.glyph(ch, style) {
            self.draw_info(buffer, win_w, clip, x0 as i32, y0, info, fg);
        }
    }

    /// Copia um glifo do atlas com a origem da celula em `(x0, y0)`.
    #[allow(clippy::too_many_arguments)]
    fn draw_info(&self, buffer: &mut [u32], win_w: u32, clip: (u32, u32), x0: i32, y0: i32, info: GlyphInfo, fg: u32) {
        let origin_x = x0 + info.left;
        let origin_y = y0 + self.baseline + info.top;
        let gy0 = (clip.0 as i32 - origin_y).max(0) as u32;
        let gy1 = ((clip.1 as i32 - origin_y).max(0) as u32).min(info.h);
        let gx0 = (-origin_x).max(0) as u32;
        let gx1 = ((win_w as i32 - origin_x).max(0) as u32).min(info.w);
        for gy in gy0..gy1 {
            let src = (info.ay + gy) as usize * ATLAS + info.ax as usize;
            let dst = (origin_y + gy as i32) as usize * win_w as usize;
            for gx in gx0..gx1 {
                let a = self.atlas[src + gx as usize] as u32;
                if a == 0 {
                    continue;
                }
                let idx = dst + (origin_x + gx as i32) as usize;
                buffer[idx] = if a == 255 { fg } else { mix(fg, buffer[idx], a) };
            }
        }
    }

    /// Glifos do trecho de pontuacao, ou `None` se o shaping nao muda o desenho.
    fn shape_run(&mut self, cells: &[Cell], style: u8) -> Option<Rc<[Shaped]>> {
        self.run_text.clear();
        self.run_text.extend(cells.iter().map(|cell| cell.text.first()));
        if let Some(hit) = self.shaped[style as usize].get(self.run_text.as_str()) {
            return hit.clone();
        }
        let shaped = shape_text(&self.styles[style as usize], self.px, &self.run_text);
        let cache = &mut self.shaped[style as usize];
        if cache.len() >= SHAPE_CACHE {
            cache.clear();
        }
        cache.insert(self.run_text.as_str().into(), shaped.clone());
        shaped
    }

    fn glyph_by_id(&mut self, id: u16, style: u8) -> Option<GlyphInfo> {
        if let Some(info) = self.by_id.get(&(id, style)) {
            return *info;
        }
        let entry = &self.styles[style as usize];
        let (font, ratio) = (entry.font.clone(), entry.ratio);
        let info = match self.rasterize_id(&font, ratio, GlyphId(id)) {
            Ok(info) => info,
            Err(AtlasFull) => {
                self.reset_atlas();
                self.rasterize_id(&font, ratio, GlyphId(id)).unwrap_or(None)
            }
        };
        self.by_id.insert((id, style), info);
        info
    }

    fn glyph(&mut self, ch: char, style: u8) -> Option<GlyphInfo> {
        let code = ch as usize;
        if code < 128 {
            if let Some(info) = self.ascii[style as usize][code] {
                return info;
            }
        } else if let Some(info) = self.cache.get(&(ch, style)) {
            return *info;
        }
        let info = match self.rasterize(ch, style) {
            Ok(info) => info,
            // atlas cheio (muitos glifos ou zoom grande): recomeca e tenta de novo
            Err(AtlasFull) => {
                self.reset_atlas();
                self.rasterize(ch, style).unwrap_or(None)
            }
        };
        if code < 128 {
            self.ascii[style as usize][code] = Some(info);
        } else {
            self.cache.insert((ch, style), info);
        }
        info
    }

    fn rasterize(&mut self, ch: char, style: u8) -> Result<Option<GlyphInfo>, AtlasFull> {
        let primary = self.styles[style as usize].font.clone();
        let ratio = self.styles[style as usize].ratio;
        if let Some(info) = self.rasterize_with(&primary, ratio, ch)? {
            return Ok(Some(info));
        }
        for i in 0..self.fallbacks.len() {
            let (font, ratio) = (self.fallbacks[i].font.clone(), self.fallbacks[i].ratio);
            if let Some(info) = self.rasterize_with(&font, ratio, ch)? {
                if std::env::var("DEBUG_FONT").is_ok() {
                    eprintln!("FALLBACK: U+{:04X} via fonte #{}", ch as u32, i);
                }
                return Ok(Some(info));
            }
        }
        if std::env::var("DEBUG_FONT").is_ok() && ch != ' ' {
            eprintln!("MISSING: U+{:04X} sem glifo (tofu)", ch as u32);
        }
        Ok(None)
    }

    fn rasterize_with(&mut self, font: &FontVec, ratio: f32, ch: char) -> Result<Option<GlyphInfo>, AtlasFull> {
        let id = font.glyph_id(ch);
        if id.0 == 0 {
            return Ok(None);
        }
        self.rasterize_id(font, ratio, id)
    }

    fn rasterize_id(&mut self, font: &FontVec, ratio: f32, id: GlyphId) -> Result<Option<GlyphInfo>, AtlasFull> {
        let glyph = Glyph {
            id,
            scale: PxScale::from(self.px * ratio),
            position: point(0.0, 0.0),
        };
        let outlined = match font.outline_glyph(glyph) {
            Some(outlined) => outlined,
            None => return Ok(None),
        };
        let bounds = outlined.px_bounds();
        let width_px = (bounds.max.x - bounds.min.x).ceil() as i32;
        let height_px = (bounds.max.y - bounds.min.y).ceil() as i32;
        if width_px <= 0 || height_px <= 0 {
            return Ok(None);
        }
        let (ax, ay) = self.alloc(width_px as u32, height_px as u32).ok_or(AtlasFull)?;
        let atlas = &mut self.atlas;
        let contrast = &self.contrast;
        outlined.draw(|x, y, coverage| {
            let dest = (ay + y) as usize * ATLAS + (ax + x) as usize;
            let value = contrast[(coverage.clamp(0.0, 1.0) * 255.0).round() as usize];
            if value > atlas[dest] {
                atlas[dest] = value;
            }
        });
        Ok(Some(GlyphInfo {
            ax,
            ay,
            w: width_px as u32,
            h: height_px as u32,
            left: bounds.min.x.floor() as i32,
            top: bounds.min.y.floor() as i32,
        }))
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > ATLAS as u32 || h > ATLAS as u32 {
            return None;
        }
        if self.pen_x + w > ATLAS as u32 {
            self.pen_x = 0;
            self.pen_y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.pen_y + h > ATLAS as u32 {
            return None;
        }
        let pos = (self.pen_x, self.pen_y);
        self.pen_x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(pos)
    }
}

/// Pontuacao ASCII: onde as fontes de programacao poem ligaduras.
fn ligature_char(cell: &Cell) -> bool {
    !cell.cont && matches!(cell.text, CellText::Char(ch) if ch.is_ascii_punctuation())
}

/// Forma `text` (ASCII) com a fonte do estilo. `None` quando cada caractere
/// continua com o proprio glifo no proprio lugar: o desenho por celula basta.
fn shape_text(entry: &FontEntry, px: f32, text: &str) -> Option<Rc<[Shaped]>> {
    let face = rustybuzz::Face::from_slice(entry.font.as_slice(), 0)?;
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(rustybuzz::Direction::LeftToRight);
    let out = rustybuzz::shape(&face, &[], buffer);
    let scale = px / face.units_per_em() as f32;
    let bytes = text.as_bytes();
    let mut changed = out.len() != bytes.len();
    let mut glyphs = Vec::with_capacity(out.len());
    let (mut pen, mut cluster) = (0i32, u32::MAX);
    for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
        if info.glyph_id == 0 {
            return None;
        }
        // varios glifos no mesmo caractere avancam a partir da celula dele
        if info.cluster != cluster {
            cluster = info.cluster;
            pen = 0;
        }
        let own = bytes
            .get(cluster as usize)
            .and_then(|&b| face.glyph_index(b as char))
            .map(|g| g.0 as u32);
        changed |= own != Some(info.glyph_id) || pen != 0 || pos.x_offset != 0 || pos.y_offset != 0;
        glyphs.push(Shaped {
            id: info.glyph_id as u16,
            cell: cluster as u16,
            dx: ((pen + pos.x_offset) as f32 * scale).round() as i32,
            dy: -(pos.y_offset as f32 * scale).round() as i32,
        });
        pen += pos.x_advance;
    }
    changed.then(|| glyphs.into())
}

/// Bracos de um caractere de moldura: [esquerda, direita, cima, baixo],
/// com peso 0 (nenhum), 1 (fino) ou 2 (grosso). Linhas tracejadas ficam com a fonte.
fn box_arms(code: u32) -> Option<[u8; 4]> {
    const TABLE: [[u8; 4]; 0x4C] = [
        [1, 1, 0, 0], [2, 2, 0, 0], [0, 0, 1, 1], [0, 0, 2, 2], // ─ ━ │ ┃
        [9, 9, 9, 9], [9, 9, 9, 9], [9, 9, 9, 9], [9, 9, 9, 9], // tracejados
        [9, 9, 9, 9], [9, 9, 9, 9], [9, 9, 9, 9], [9, 9, 9, 9],
        [0, 1, 0, 1], [0, 2, 0, 1], [0, 1, 0, 2], [0, 2, 0, 2], // ┌ ┍ ┎ ┏
        [1, 0, 0, 1], [2, 0, 0, 1], [1, 0, 0, 2], [2, 0, 0, 2], // ┐ ┑ ┒ ┓
        [0, 1, 1, 0], [0, 2, 1, 0], [0, 1, 2, 0], [0, 2, 2, 0], // └ ┕ ┖ ┗
        [1, 0, 1, 0], [2, 0, 1, 0], [1, 0, 2, 0], [2, 0, 2, 0], // ┘ ┙ ┚ ┛
        [0, 1, 1, 1], [0, 2, 1, 1], [0, 1, 2, 1], [0, 1, 1, 2], // ├ ┝ ┞ ┟
        [0, 1, 2, 2], [0, 2, 2, 1], [0, 2, 1, 2], [0, 2, 2, 2], // ┠ ┡ ┢ ┣
        [1, 0, 1, 1], [2, 0, 1, 1], [1, 0, 2, 1], [1, 0, 1, 2], // ┤ ┥ ┦ ┧
        [1, 0, 2, 2], [2, 0, 2, 1], [2, 0, 1, 2], [2, 0, 2, 2], // ┨ ┩ ┪ ┫
        [1, 1, 0, 1], [2, 1, 0, 1], [1, 2, 0, 1], [2, 2, 0, 1], // ┬ ┭ ┮ ┯
        [1, 1, 0, 2], [2, 1, 0, 2], [1, 2, 0, 2], [2, 2, 0, 2], // ┰ ┱ ┲ ┳
        [1, 1, 1, 0], [2, 1, 1, 0], [1, 2, 1, 0], [2, 2, 1, 0], // ┴ ┵ ┶ ┷
        [1, 1, 2, 0], [2, 1, 2, 0], [1, 2, 2, 0], [2, 2, 2, 0], // ┸ ┹ ┺ ┻
        [1, 1, 1, 1], [2, 1, 1, 1], [1, 2, 1, 1], [2, 2, 1, 1], // ┼ ┽ ┾ ┿
        [1, 1, 2, 1], [1, 1, 1, 2], [1, 1, 2, 2], [2, 1, 2, 1], // ╀ ╁ ╂ ╃
        [1, 2, 2, 1], [2, 1, 1, 2], [1, 2, 1, 2], [2, 2, 2, 1], // ╄ ╅ ╆ ╇
        [2, 2, 1, 2], [2, 1, 2, 2], [1, 2, 2, 2], [2, 2, 2, 2], // ╈ ╉ ╊ ╋
    ];
    const HALF: [[u8; 4]; 12] = [
        [1, 0, 0, 0], [0, 0, 1, 0], [0, 1, 0, 0], [0, 0, 0, 1], // ╴ ╵ ╶ ╷
        [2, 0, 0, 0], [0, 0, 2, 0], [0, 2, 0, 0], [0, 0, 0, 2], // ╸ ╹ ╺ ╻
        [1, 2, 0, 0], [0, 0, 1, 2], [2, 1, 0, 0], [0, 0, 2, 1], // ╼ ╽ ╾ ╿
    ];
    let arms = match code {
        0x2500..=0x254B => TABLE[(code - 0x2500) as usize],
        0x2574..=0x257F => HALF[(code - 0x2574) as usize],
        _ => return None,
    };
    (arms[0] != 9).then_some(arms)
}

/// Mistura `fg` sobre `bg` com alfa 0..=255, em inteiros (sem f32 por pixel).
#[inline(always)]
fn mix(fg: u32, bg: u32, a: u32) -> u32 {
    let inv = 255 - a;
    // vermelho e azul juntos numa so multiplicacao; verde a parte
    let rb = (fg & 0x00ff_00ff) * a + (bg & 0x00ff_00ff) * inv + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00ff_00ff)) >> 8) & 0x00ff_00ff;
    let g = (fg & 0x0000_ff00) * a + (bg & 0x0000_ff00) * inv + 0x0000_8000;
    let g = ((g + ((g >> 8) & 0x0000_ff00)) >> 8) & 0x0000_ff00;
    rb | g
}

/// Como `fill_rect`, misturando a cor com o que ja esta pintado (alfa 0..=255).
#[allow(clippy::too_many_arguments)]
fn blend_rect(buffer: &mut [u32], win_w: u32, win_h: u32, x0: u32, y0: u32, w: u32, h: u32, color: u32, alpha: u32) {
    if alpha >= 255 {
        return fill_rect(buffer, win_w, win_h, x0, y0, w, h, color);
    }
    if alpha == 0 {
        return;
    }
    let x_end = (x0 + w).min(win_w) as usize;
    for y in y0..(y0 + h).min(win_h) {
        let row = y as usize * win_w as usize;
        for px in &mut buffer[row + x0 as usize..row + x_end.max(x0 as usize)] {
            *px = mix(color, *px, alpha);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(buffer: &mut [u32], win_w: u32, win_h: u32, x0: u32, y0: u32, w: u32, h: u32, color: u32) {
    let x_end = (x0 + w).min(win_w) as usize;
    let y_end = (y0 + h).min(win_h);
    if x0 as usize >= x_end {
        return;
    }
    for y in y0..y_end {
        let row = y as usize * win_w as usize;
        buffer[row + x0 as usize..row + x_end].fill(color);
    }
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        dirs.push(PathBuf::from("C:\\Windows\\Fonts"));
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
        return dirs;
    }
    // Linux e macOS: onde o instalador poe a Nerd Font (por usuario) e as do sistema
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(data) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share"))) {
        dirs.push(data.join("fonts"));
    }
    if let Some(h) = &home {
        dirs.push(h.join("Library/Fonts"));
        dirs.push(h.join(".fonts"));
    }
    for d in ["/Library/Fonts", "/System/Library/Fonts", "/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/truetype"] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

fn resolve_styles(cfg: &FontCfg) -> Result<Vec<FontEntry>, String> {
    let overrides = [cfg.regular.as_deref(), cfg.bold.as_deref(), cfg.italic.as_deref()];
    let mut styles = Vec::new();
    for (i, (primary, fallback, alt)) in DEFAULTS.iter().enumerate() {
        let mut names: Vec<String> = Vec::new();
        if let Some(name) = overrides[i] {
            names.push(name.to_string());
        }
        names.push(primary.to_string());
        names.push(alt.to_string());
        names.push(fallback.to_string());
        let entry = load_font(&names)
            .ok_or_else(|| format!("nao encontrei nenhuma fonte para o estilo {} ({primary})", i))?;
        styles.push(entry);
    }
    Ok(styles)
}

fn load_fallbacks(names: &[String]) -> Vec<FontEntry> {
    names.iter().filter_map(|name| load_font(std::slice::from_ref(name))).collect()
}

fn load_font(names: &[String]) -> Option<FontEntry> {
    for name in names {
        let direct = Path::new(name);
        if direct.is_absolute() || name.contains('\\') || name.contains('/') {
            if let Some(entry) = read_font(direct) {
                return Some(entry);
            }
        }
        for dir in font_dirs() {
            if let Some(entry) = read_font(&dir.join(name)) {
                return Some(entry);
            }
        }
    }
    None
}

fn read_font(path: &Path) -> Option<FontEntry> {
    let data = std::fs::read(path).ok()?;
    let ratio = {
        let face = ttf_parser::Face::parse(&data, 0).ok()?;
        let units = face.units_per_em() as f32;
        let extent = (face.ascender() as f32 - face.descender() as f32).abs();
        if units > 0.0 { extent / units } else { 1.32 }
    };
    // sem copiar o arquivo: fontes CJK passam de 10 MB
    let font = FontVec::try_from_vec(data).ok()?;
    Some(FontEntry { font: Rc::new(font), ratio })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Cell;

    /// Renderer com a fonte padrao, se ela tiver ligaduras nesta maquina.
    fn renderer(ligatures: bool) -> Option<Renderer> {
        let cfg = FontCfg { ligatures, fallback: Vec::new(), ..FontCfg::default() };
        let r = Renderer::new(&cfg, 0).ok()?;
        if shape_text(&r.styles[0], r.px, "->").is_none() {
            eprintln!("fonte sem ligaduras instalada: teste ignorado");
            return None;
        }
        Some(r)
    }

    /// Pinta "x -> y != z" e devolve os pixels da linha 0.
    fn paint(r: &mut Renderer, cursor_col: usize) -> Vec<u32> {
        let mut grid = Grid::new(16, 2);
        for (c, ch) in "x -> y != z".chars().enumerate() {
            grid.cells[c] = Cell { text: CellText::Char(ch), hl: 0, cont: false };
        }
        grid.cursor = (0, cursor_col);
        let (w, h) = (r.cell_w * 16, r.cell_h * 2);
        let mut buffer = vec![0u32; (w * h) as usize];
        r.render(&mut buffer, w, h, &mut grid, 0.0, true, Instant::now(), true);
        buffer[..(w * r.cell_h) as usize].to_vec()
    }

    /// Pior caso: tela inteira de codigo cheio de ligaduras, cache quente.
    /// `cargo test --release -- --ignored --nocapture bench_ligaduras`
    #[test]
    #[ignore]
    fn bench_ligaduras() {
        const LINE: &str = "    if a != b && x => y { f(x) -> g(y) === z; } // <= >= :: |> <!-- --> ";
        for ligatures in [false, true] {
            let Some(mut r) = renderer(ligatures) else { return };
            let (cols, rows) = (120usize, 40usize);
            let mut grid = Grid::new(cols, rows);
            for row in 0..rows {
                for (c, ch) in LINE.chars().cycle().skip(row).take(cols).enumerate() {
                    grid.cells[row * cols + c] = Cell { text: CellText::Char(ch), hl: 0, cont: false };
                }
            }
            let (w, h) = (r.cell_w * cols as u32, r.cell_h * rows as u32);
            let mut buffer = vec![0u32; (w * h) as usize];
            r.render(&mut buffer, w, h, &mut grid, 1.0, true, Instant::now(), true);
            let n = 200;
            let start = Instant::now();
            for _ in 0..n {
                r.render(&mut buffer, w, h, &mut grid, 1.0, true, Instant::now(), true);
            }
            let full = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
            // quadro tipico: o cursor anda uma celula e repinta a linha dele
            let start = Instant::now();
            for i in 0..n * 10 {
                grid.cursor = (5, i % cols);
                r.render(&mut buffer, w, h, &mut grid, 1.0, true, Instant::now(), false);
            }
            let partial = start.elapsed().as_secs_f64() * 1000.0 / (n * 10) as f64;
            println!("BENCH_LIG: ligaduras={ligatures} tela inteira {full:.3} ms, uma linha {partial:.4} ms ({w}x{h})");
        }
    }

    #[test]
    fn shaping_so_muda_o_que_tem_ligadura() {
        let Some(r) = renderer(true) else { return };
        let arrow = shape_text(&r.styles[0], r.px, "->").expect("-> vira ligadura");
        // fontes de programacao: um glifo por celula, a grade continua valendo
        assert_eq!(arrow.len(), 2);
        assert_eq!((arrow[0].cell, arrow[1].cell), (0, 1));
        assert!(shape_text(&r.styles[0], r.px, "!=").is_some());
        assert!(shape_text(&r.styles[0], r.px, "===").is_some());
        assert!(shape_text(&r.styles[0], r.px, "();").is_none());
    }

    #[test]
    fn ligadura_desenha_diferente_e_some_sob_o_cursor() {
        let (Some(mut on), Some(mut off)) = (renderer(true), renderer(false)) else { return };
        let plain = paint(&mut off, 15);
        assert_ne!(paint(&mut on, 15), plain, "-> e != deveriam virar ligadura");
        // cursor em cada trecho: aquele trecho volta aos caracteres da celula
        let cw = on.cell_w as usize;
        let cells = |px: &[u32], from: usize, to: usize| -> Vec<u32> {
            px.chunks(cw * 16)
                .flat_map(|row| row[from * cw..to * cw].to_vec())
                .collect()
        };
        let under = paint(&mut on, 3);
        assert_eq!(cells(&under, 2, 4), cells(&plain, 2, 4), "-> sob o cursor");
        assert_ne!(cells(&under, 7, 9), cells(&plain, 7, 9), "!= longe do cursor continua ligadura");
    }

    #[test]
    fn indicador_aparece_gira_e_some_sem_sujar_o_resto() {
        let Some(mut r) = renderer(false) else { return };
        let (cols, rows) = (40usize, 10usize);
        let mut grid = Grid::new(cols, rows);
        let (w, h) = (r.cell_w * cols as u32, r.cell_h * rows as u32);
        let mut buf = vec![0u32; (w * h) as usize];
        let mut pintar = |r: &mut Renderer, buf: &mut Vec<u32>, grid: &mut Grid, full: bool| {
            r.render(buf, w, h, grid, 0.0, true, Instant::now(), full);
        };
        pintar(&mut r, &mut buf, &mut grid, true);
        let limpo = buf.clone();
        // ligado: so a regiao do canto inferior direito (acima da linha de status) muda
        r.set_spinner(Some(0));
        pintar(&mut r, &mut buf, &mut grid, false);
        let diff: Vec<usize> = (0..buf.len()).filter(|&i| buf[i] != limpo[i]).collect();
        assert!(!diff.is_empty(), "o indicador desenha pixels");
        let (x, y): (Vec<u32>, Vec<u32>) = diff.iter().map(|&i| (i as u32 % w, i as u32 / w)).unzip();
        let linha = (rows - 3) as u32;
        assert!(y.iter().all(|&y| y / r.cell_h == linha), "so na linha rows-3");
        assert!(x.iter().all(|&x| x / r.cell_w == (cols - 2) as u32), "so na penultima coluna");
        // girando: outro quadro, outros pixels
        let quadro0 = buf.clone();
        r.set_spinner(Some(3));
        pintar(&mut r, &mut buf, &mut grid, false);
        assert_ne!(buf, quadro0, "o ponto da frente anda");
        // desligado: a linha volta a ser a de antes
        r.set_spinner(None);
        pintar(&mut r, &mut buf, &mut grid, false);
        assert_eq!(buf, limpo, "some sem deixar rastro");
    }

    #[test]
    fn mix_extremos_e_meio() {
        assert_eq!(mix(0x00ff_ffff, 0x0000_0000, 255), 0x00ff_ffff);
        assert_eq!(mix(0x00ff_ffff, 0x0012_3456, 0), 0x0012_3456);
        let m = mix(0x00ff_0000, 0x0000_00ff, 128);
        assert!(((m >> 16) & 0xff).abs_diff(128) <= 1 && (m & 0xff).abs_diff(127) <= 1);
        assert_eq!(m & 0x0000_ff00, 0);
    }
}
