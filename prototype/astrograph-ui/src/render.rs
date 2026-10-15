//! Renderizacao por CPU com softbuffer e atlas de glifos (ab_glyph).

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use ab_glyph::{point, Font, FontVec, Glyph, PxScale, ScaleFont};

use crate::grid::Grid;

const ATLAS: usize = 1600;

const FONTS: [(&str, &str, &str); 3] = [
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

pub struct Renderer {
    fonts: Vec<Rc<FontVec>>,
    px: f32,
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
    pub fn new(font_px: f32) -> Result<Self, String> {
        let mut fonts = Vec::new();
        for (primary, fallback, alt) in FONTS {
            let font = load_font(&[primary, alt, fallback])
                .ok_or_else(|| format!("nao encontrei a fonte {primary}"))?;
            fonts.push(Rc::new(font));
        }
        let scaled = fonts[0].as_scaled(PxScale::from(font_px));
        let ascent = scaled.ascent();
        let descent = scaled.descent();
        let cell_h = (ascent - descent + scaled.line_gap()).ceil() as u32;
        let cell_w = scaled.h_advance(fonts[0].glyph_id('M')).round() as u32;
        let baseline = ascent.round() as i32;
        Ok(Renderer {
            fonts,
            px: font_px,
            cell_w: cell_w.max(1),
            cell_h: cell_h.max(1),
            baseline,
            atlas: vec![0u8; ATLAS * ATLAS],
            pen_x: 0,
            pen_y: 0,
            row_h: 0,
            cache: HashMap::new(),
        })
    }

    fn style(bold: bool, italic: bool) -> u8 {
        match (bold, italic) {
            (true, _) => 1,
            (false, true) => 2,
            _ => 0,
        }
    }

    pub fn draw(&mut self, buffer: &mut [u32], win_w: u32, win_h: u32, grid: &Grid, cursor: bool) {
        let fill = grid.default_bg & 0x00ff_ffff;
        for pixel in buffer.iter_mut() {
            *pixel = fill;
        }
        let cols = ((win_w / self.cell_w) as usize).min(grid.width);
        let rows = ((win_h / self.cell_h) as usize).min(grid.height);
        for row in 0..rows {
            for col in 0..cols {
                let cell = grid.cells[row * grid.width + col];
                let mut res = grid.resolve(cell.hl);
                let is_cursor = cursor && (row, col) == grid.cursor;
                if is_cursor {
                    std::mem::swap(&mut res.fg, &mut res.bg);
                }
                let x0 = col as u32 * self.cell_w;
                let y0 = row as u32 * self.cell_h;
                fill_rect(buffer, win_w, win_h, x0, y0, self.cell_w, self.cell_h, res.bg);
                if cell.ch != ' ' {
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
                let dst = buffer[idx];
                let (dr, dg, db) = channels(dst);
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
            scale: PxScale::from(self.px),
            position: point(0.0, 0.0),
        };
        let outlined = font.outline_glyph(glyph)?;
        let bounds = outlined.px_bounds();
        let w = (bounds.max.x - bounds.min.x).ceil() as i32;
        let h = (bounds.max.y - bounds.min.y).ceil() as i32;
        if w <= 0 || h <= 0 {
            return None;
        }
        let (ax, ay) = self.alloc(w as u32, h as u32)?;
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
            w: w as u32,
            h: h as u32,
            left: bounds.min.x.floor() as i32,
            top: bounds.min.y.floor() as i32,
        })
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
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

fn load_font(names: &[&str]) -> Option<FontVec> {
    let dirs = font_dirs();
    for name in names {
        for dir in &dirs {
            let path = dir.join(name);
            if let Ok(data) = std::fs::read(&path) {
                if let Ok(font) = FontVec::try_from_vec(data) {
                    return Some(font);
                }
            }
        }
    }
    None
}
