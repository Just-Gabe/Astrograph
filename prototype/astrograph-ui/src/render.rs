//! Renderizacao por CPU com softbuffer e atlas de glifos (ab_glyph).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ab_glyph::{point, Font, FontVec, Glyph, PxScale, ScaleFont};
use unicode_width::UnicodeWidthChar;

use crate::config::FontCfg;
use crate::grid::Grid;

const ATLAS: usize = 1600;

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

#[derive(Clone, Copy)]
struct GlyphInfo {
    ax: u32,
    ay: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum CursorShape {
    Block,
    Beam,
    Underline,
}

pub struct Renderer {
    fonts: Vec<Rc<FontVec>>,
    px: f32,
    /// Fator para converter tamanho pedido (em px de EM) na escala do ab_glyph.
    ratio: f32,
    line_height: f32,
    pub cell_w: u32,
    pub cell_h: u32,
    baseline: i32,
    atlas: Vec<u8>,
    pen_x: u32,
    pen_y: u32,
    row_h: u32,
    cache: HashMap<(char, u8), Option<GlyphInfo>>,
}

impl Renderer {
    pub fn new(cfg: &FontCfg) -> Result<Self, String> {
        let (fonts, ratio) = resolve_fonts(cfg)?;
        let mut renderer = Renderer {
            fonts,
            px: cfg.size.clamp(6.0, 96.0),
            ratio,
            line_height: cfg.line_height.clamp(0.8, 3.0),
            cell_w: 1,
            cell_h: 1,
            baseline: 0,
            atlas: vec![0u8; ATLAS * ATLAS],
            pen_x: 0,
            pen_y: 0,
            row_h: 0,
            cache: HashMap::new(),
        };
        renderer.recompute_metrics();
        Ok(renderer)
    }

    fn scale(&self) -> PxScale {
        PxScale::from(self.px * self.ratio)
    }

    fn recompute_metrics(&mut self) {
        let scaled = self.fonts[0].as_scaled(self.scale());
        let ascent = scaled.ascent();
        let descent = scaled.descent();
        if std::env::var("DEBUG_FONT").is_ok() {
            println!(
                "FONT_DBG: em={:.1}px ratio={:.3} ascent={:.2} descent={:.2} line_gap={:.2} advM={:.2}",
                self.px,
                self.ratio,
                ascent,
                descent,
                scaled.line_gap(),
                scaled.h_advance(self.fonts[0].glyph_id('M')),
            );
        }
        self.cell_h = ((ascent - descent + scaled.line_gap()) * self.line_height).ceil() as u32;
        self.cell_w = scaled.h_advance(self.fonts[0].glyph_id('M')).round() as u32;
        self.cell_w = self.cell_w.max(1);
        self.cell_h = self.cell_h.max(1);
        self.baseline = ascent.round() as i32;
        self.atlas.iter_mut().for_each(|b| *b = 0);
        self.pen_x = 0;
        self.pen_y = 0;
        self.row_h = 0;
        self.cache.clear();
    }

    /// Ajusta o tamanho da fonte (zoom). Retorna o tamanho efetivo.
    pub fn set_size(&mut self, px: f32) {
        self.px = px.clamp(6.0, 96.0);
        self.recompute_metrics();
    }

    pub fn font_size(&self) -> f32 {
        self.px
    }

    fn style(bold: bool, italic: bool) -> u8 {
        match (bold, italic) {
            (true, _) => 1,
            (false, true) => 2,
            _ => 0,
        }
    }

    fn cursor_shape(mode: &str) -> CursorShape {
        if mode.contains("insert") {
            CursorShape::Beam
        } else if mode.contains("replace") {
            CursorShape::Underline
        } else {
            CursorShape::Block
        }
    }

    pub fn draw(
        &mut self,
        buffer: &mut [u32],
        win_w: u32,
        win_h: u32,
        grid: &Grid,
        cursor_visible: bool,
    ) {
        let fill = grid.default_bg & 0x00ff_ffff;
        for pixel in buffer.iter_mut() {
            *pixel = fill;
        }
        let shape = Self::cursor_shape(&grid.mode);
        let cols = ((win_w / self.cell_w) as usize).min(grid.width);
        let rows = ((win_h / self.cell_h) as usize).min(grid.height);
        for row in 0..rows {
            for col in 0..cols {
                let cell = grid.cells[row * grid.width + col];
                let mut res = grid.resolve(cell.hl);
                let is_cursor = cursor_visible
                    && shape == CursorShape::Block
                    && (row, col) == grid.cursor;
                if is_cursor {
                    std::mem::swap(&mut res.fg, &mut res.bg);
                }
                let x0 = col as u32 * self.cell_w;
                let y0 = row as u32 * self.cell_h;
                fill_rect(buffer, win_w, win_h, x0, y0, self.cell_w, self.cell_h, res.bg);
                if cell.ch != ' ' && !cell.cont {
                    self.blit(
                        buffer, win_w, win_h, x0, y0, cell.ch, res.fg, res.bold, res.italic,
                    );
                }
                if res.underline {
                    let uy = y0 + self.cell_h.saturating_sub(2);
                    fill_rect(buffer, win_w, win_h, x0, uy, self.cell_w, 1, res.fg);
                }
            }
        }
        if cursor_visible && shape != CursorShape::Block {
            self.draw_thin_cursor(buffer, win_w, win_h, grid, shape);
        }
    }

    fn draw_thin_cursor(
        &self,
        buffer: &mut [u32],
        win_w: u32,
        win_h: u32,
        grid: &Grid,
        shape: CursorShape,
    ) {
        let (row, col) = grid.cursor;
        let cols = (win_w / self.cell_w) as usize;
        let rows = (win_h / self.cell_h) as usize;
        if col >= cols || row >= rows {
            return;
        }
        let fg = grid.resolve(grid.cells[row * grid.width + col].hl).fg;
        let x0 = col as u32 * self.cell_w;
        let y0 = row as u32 * self.cell_h;
        match shape {
            CursorShape::Beam => {
                let w = (self.cell_w / 8).max(2);
                fill_rect(buffer, win_w, win_h, x0, y0, w, self.cell_h, fg);
            }
            CursorShape::Underline => {
                let h = (self.cell_h / 8).max(2);
                fill_rect(
                    buffer,
                    win_w,
                    win_h,
                    x0,
                    y0 + self.cell_h.saturating_sub(h),
                    self.cell_w,
                    h,
                    fg,
                );
            }
            CursorShape::Block => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        buffer: &mut [u32],
        win_w: u32,
        win_h: u32,
        x0: u32,
        y0: u32,
        ch: char,
        fg: u32,
        bold: bool,
        italic: bool,
    ) {
        let info = match self.glyph(ch, Self::style(bold, italic)) {
            Some(info) => info,
            None => return,
        };
        let origin_x = x0 as i32 + info.left;
        let origin_y = y0 as i32 + self.baseline + info.top;
        let (fr, fg_g, fb) = channels(fg);
        for gy in 0..info.h {
            let sy = origin_y + gy as i32;
            if sy < 0 || sy >= win_h as i32 {
                continue;
            }
            for gx in 0..info.w {
                let sx = origin_x + gx as i32;
                if sx < 0 || sx >= win_w as i32 {
                    continue;
                }
                let coverage = self.atlas[(info.ay + gy) as usize * ATLAS + (info.ax + gx) as usize];
                if coverage == 0 {
                    continue;
                }
                let c = coverage as f32 / 255.0;
                let inv = 1.0 - c;
                let idx = sy as usize * win_w as usize + sx as usize;
                let (dr, dg, db) = channels(buffer[idx]);
                let r = (fr * c + dr * inv).round() as u32;
                let g = (fg_g * c + dg * inv).round() as u32;
                let b = (fb * c + db * inv).round() as u32;
                buffer[idx] = (r << 16) | (g << 8) | b;
            }
        }
    }

    fn glyph(&mut self, ch: char, style: u8) -> Option<GlyphInfo> {
        if let Some(info) = self.cache.get(&(ch, style)) {
            return *info;
        }
        let info = self.rasterize(ch, style);
        self.cache.insert((ch, style), info);
        info
    }

    fn rasterize(&mut self, ch: char, style: u8) -> Option<GlyphInfo> {
        let font = self.fonts[style as usize].clone();
        let glyph = Glyph {
            id: font.glyph_id(ch),
            scale: self.scale(),
            position: point(0.0, 0.0),
        };
        let outlined = font.outline_glyph(glyph)?;
        let bounds = outlined.px_bounds();
        let width_px = (bounds.max.x - bounds.min.x).ceil() as i32;
        let height_px = (bounds.max.y - bounds.min.y).ceil() as i32;
        if width_px <= 0 || height_px <= 0 {
            return None;
        }
        let (ax, ay) = self.alloc(width_px as u32, height_px as u32)?;
        let atlas = &mut self.atlas;
        outlined.draw(|x, y, coverage| {
            let dest = (ay + y) as usize * ATLAS + (ax + x) as usize;
            let value = (coverage * 255.0).round() as u8;
            if value > atlas[dest] {
                atlas[dest] = value;
            }
        });
        Some(GlyphInfo {
            ax,
            ay,
            w: width_px as u32,
            h: height_px as u32,
            left: bounds.min.x.floor() as i32,
            top: bounds.min.y.floor() as i32,
        })
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

pub fn char_width(ch: char) -> u64 {
    UnicodeWidthChar::width(ch).unwrap_or(1).max(1) as u64
}

fn channels(color: u32) -> (f32, f32, f32) {
    (
        ((color >> 16) & 0xff) as f32,
        ((color >> 8) & 0xff) as f32,
        (color & 0xff) as f32,
    )
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(
    buffer: &mut [u32],
    win_w: u32,
    win_h: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    color: u32,
) {
    let color = color & 0x00ff_ffff;
    let x_end = (x0 + w).min(win_w);
    let y_end = (y0 + h).min(win_h);
    for y in y0..y_end {
        let row = y as usize * win_w as usize;
        for x in x0..x_end {
            buffer[row + x as usize] = color;
        }
    }
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("C:\\Windows\\Fonts")];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
    }
    dirs
}

fn resolve_fonts(cfg: &FontCfg) -> Result<(Vec<Rc<FontVec>>, f32), String> {
    let overrides = [
        cfg.regular.as_deref(),
        cfg.bold.as_deref(),
        cfg.italic.as_deref(),
    ];
    let mut fonts = Vec::new();
    let mut ratio = 1.32_f32;
    for (i, (primary, fallback, alt)) in DEFAULTS.iter().enumerate() {
        let mut names: Vec<String> = Vec::new();
        if let Some(name) = overrides[i] {
            names.push(name.to_string());
        }
        names.push(primary.to_string());
        names.push(alt.to_string());
        names.push(fallback.to_string());
        let (font, font_ratio) = load_font(&names)
            .ok_or_else(|| format!("nao encontrei nenhuma fonte para o estilo {} ({primary})", i))?;
        if i == 0 {
            ratio = font_ratio;
        }
        fonts.push(Rc::new(font));
    }
    Ok((fonts, ratio))
}

fn load_font(names: &[String]) -> Option<(FontVec, f32)> {
    for name in names {
        let direct = Path::new(name);
        if direct.is_absolute() || name.contains('\\') || name.contains('/') {
            if let Some(font) = read_font(direct) {
                return Some(font);
            }
        }
        for dir in font_dirs() {
            if let Some(font) = read_font(&dir.join(name)) {
                return Some(font);
            }
        }
    }
    None
}

fn read_font(path: &Path) -> Option<(FontVec, f32)> {
    let data = std::fs::read(path).ok()?;
    let font = FontVec::try_from_vec(data.clone()).ok()?;
    let upem = ttf_parser::Face::parse(&data, 0).ok()?;
    let units = upem.units_per_em() as f32;
    let extent = (upem.ascender() as f32 - upem.descender() as f32).abs();
    let ratio = if units > 0.0 { extent / units } else { 1.32 };
    Some((font, ratio))
}
