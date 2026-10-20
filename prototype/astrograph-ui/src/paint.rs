//! Pintor de pixels: retângulos arredondados com borda suave, sombras, degradês
//! e texto (fonte proporcional ou monoespaçada) em qualquer posição em pixels.
//! Formato do buffer: `0x00RRGGBB`, como o softbuffer.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use ab_glyph::{point, Font as _, FontVec, PxScale, ScaleFont};

/// Inter, a fonte da interface (licença OFL em `assets/fonts/OFL.txt`).
pub static INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.otf");
pub static INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.otf");

pub struct Glyph {
    pub w: u32,
    pub h: u32,
    pub left: i32,
    pub top: i32,
    pub cov: Vec<u8>,
}

pub struct Font {
    font: FontVec,
    cache: HashMap<(char, u32), Option<Rc<Glyph>>>,
    adv: HashMap<(char, u32), f32>,
}

fn key(px: f32) -> u32 {
    (px * 16.0).round() as u32
}

impl Font {
    pub fn from_bytes(data: Vec<u8>) -> Option<Font> {
        Some(Font { font: FontVec::try_from_vec(data).ok()?, cache: HashMap::new(), adv: HashMap::new() })
    }

    pub fn from_path(path: &Path) -> Option<Font> {
        Font::from_bytes(std::fs::read(path).ok()?)
    }

    pub fn has(&self, c: char) -> bool {
        self.font.glyph_id(c).0 != 0
    }

    /// (ascendente, descendente) em pixels para o tamanho `px`.
    pub fn metrics(&self, px: f32) -> (f32, f32) {
        let sf = self.font.as_scaled(PxScale::from(px));
        (sf.ascent(), -sf.descent())
    }

    pub fn advance(&mut self, c: char, px: f32) -> f32 {
        let k = (c, key(px));
        if let Some(a) = self.adv.get(&k) {
            return *a;
        }
        let sf = self.font.as_scaled(PxScale::from(px));
        let a = sf.h_advance(self.font.glyph_id(c));
        self.adv.insert(k, a);
        a
    }

    pub fn measure(&mut self, s: &str, px: f32) -> f32 {
        let mut prev = None;
        let mut w = 0.0;
        for c in s.chars() {
            let id = self.font.glyph_id(c);
            if let Some(p) = prev {
                w += self.font.as_scaled(PxScale::from(px)).kern(p, id);
            }
            w += self.advance(c, px);
            prev = Some(id);
        }
        w
    }

    pub fn glyph(&mut self, c: char, px: f32) -> Option<Rc<Glyph>> {
        let k = (c, key(px));
        if let Some(g) = self.cache.get(&k) {
            return g.clone();
        }
        let scale = PxScale::from(px);
        let id = self.font.glyph_id(c);
        let g = id.with_scale_and_position(scale, point(0.0, 0.0));
        let out = self.font.outline_glyph(g).map(|og| {
            let b = og.px_bounds();
            let (w, h) = (b.width().ceil().max(1.0) as u32, b.height().ceil().max(1.0) as u32);
            let mut cov = vec![0u8; (w * h) as usize];
            og.draw(|x, y, c| {
                if x < w && y < h {
                    cov[(y * w + x) as usize] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            });
            Rc::new(Glyph { w, h, left: b.min.x.floor() as i32, top: b.min.y.floor() as i32, cov })
        });
        self.cache.insert(k, out.clone());
        out
    }
}

#[inline]
fn blend(dst: u32, src: u32, a: u32) -> u32 {
    if a == 0 {
        return dst;
    }
    if a >= 255 {
        return src & 0x00ff_ffff;
    }
    let inv = 255 - a;
    let ch = |s: u32| {
        let v = ((src >> s) & 0xff) * a + ((dst >> s) & 0xff) * inv;
        (v * 257 + 32768) >> 16
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Contraste do texto claro sobre fundo escuro: engrossa um pouco a cobertura.
fn lut() -> &'static [u8; 256] {
    use std::sync::OnceLock;
    static T: OnceLock<[u8; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = ((i as f32 / 255.0).powf(0.8) * 255.0).round() as u8;
        }
        t
    })
}

pub struct Canvas<'a> {
    pub buf: &'a mut [u32],
    pub w: i32,
    pub h: i32,
    /// área onde se pode pintar (x0, y0, x1, y1), exclusivo no fim
    pub clip: (i32, i32, i32, i32),
}

/// Distância assinada de (cx, cy) a um retângulo arredondado.
#[inline]
fn sdf_rrect(cx: f32, cy: f32, mx: f32, my: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (cx - mx).abs() - (hw - r);
    let qy = (cy - my).abs() - (hh - r);
    if qx > 0.0 && qy > 0.0 {
        (qx * qx + qy * qy).sqrt() - r
    } else {
        qx.max(qy) - r
    }
}

impl<'a> Canvas<'a> {
    pub fn new(buf: &'a mut [u32], w: u32, h: u32) -> Self {
        Canvas { buf, w: w as i32, h: h as i32, clip: (0, 0, w as i32, h as i32) }
    }

    pub fn set_clip(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.clip = (x0.max(0), y0.max(0), x1.min(self.w), y1.min(self.h));
    }

    pub fn reset_clip(&mut self) {
        self.clip = (0, 0, self.w, self.h);
    }

    pub fn clear(&mut self, color: u32) {
        self.buf.fill(color & 0x00ff_ffff);
    }

    #[inline]
    fn put(&mut self, x: i32, y: i32, color: u32, a: u32) {
        if x < self.clip.0 || y < self.clip.1 || x >= self.clip.2 || y >= self.clip.3 {
            return;
        }
        let i = (y * self.w + x) as usize;
        self.buf[i] = blend(self.buf[i], color, a);
    }

    /// Retângulo reto; `alpha` 0..=1.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32, alpha: f32) {
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (x0, y0, x1, y1) = (x.max(self.clip.0), y.max(self.clip.1), (x + w).min(self.clip.2), (y + h).min(self.clip.3));
        for py in y0..y1 {
            let row = (py * self.w) as usize;
            for px in x0..x1 {
                let i = row + px as usize;
                self.buf[i] = blend(self.buf[i], color, a);
            }
        }
    }

    /// Retângulo com cantos arredondados e borda suave.
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32, alpha: f32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let (mx, my, hw, hh) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let (x0, y0, x1, y1) = (
            (x.floor() as i32).max(self.clip.0),
            (y.floor() as i32).max(self.clip.1),
            ((x + w).ceil() as i32).min(self.clip.2),
            ((y + h).ceil() as i32).min(self.clip.3),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                let d = sdf_rrect(px as f32 + 0.5, py as f32 + 0.5, mx, my, hw, hh, r);
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.put(px, py, color, (cov * alpha * 255.0).round() as u32);
                }
            }
        }
    }

    /// Contorno de `t` pixels (por dentro do retângulo).
    pub fn rrect_border(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, color: u32, alpha: f32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let (mx, my, hw, hh) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let (x0, y0, x1, y1) = (
            (x.floor() as i32).max(self.clip.0),
            (y.floor() as i32).max(self.clip.1),
            ((x + w).ceil() as i32).min(self.clip.2),
            ((y + h).ceil() as i32).min(self.clip.3),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                let d = sdf_rrect(px as f32 + 0.5, py as f32 + 0.5, mx, my, hw, hh, r);
                let outer = (0.5 - d).clamp(0.0, 1.0);
                let inner = (0.5 + d + t).clamp(0.0, 1.0);
                let cov = outer.min(inner);
                if cov > 0.0 {
                    self.put(px, py, color, (cov * alpha * 255.0).round() as u32);
                }
            }
        }
    }

    /// Sombra suave atrás de um retângulo arredondado.
    #[allow(clippy::too_many_arguments)]
    pub fn shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, blur: f32, dy: f32, color: u32, alpha: f32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let y = y + dy;
        let (mx, my, hw, hh) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let pad = blur * 1.6;
        let (x0, y0, x1, y1) = (
            ((x - pad).floor() as i32).max(self.clip.0),
            ((y - pad).floor() as i32).max(self.clip.1),
            ((x + w + pad).ceil() as i32).min(self.clip.2),
            ((y + h + pad).ceil() as i32).min(self.clip.3),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                let d = sdf_rrect(px as f32 + 0.5, py as f32 + 0.5, mx, my, hw, hh, r);
                // 1 dentro, 0 a `pad` de distância, com cauda suave
                let t = ((d + blur * 0.3) / (blur * 1.3)).clamp(0.0, 1.0);
                let k = 1.0 - t * t * (3.0 - 2.0 * t);
                let k = k * k;
                if k > 0.002 {
                    self.put(px, py, color, (k * alpha * 255.0).round() as u32);
                }
            }
        }
    }

    /// Degradê vertical de `a_top` a `a_bottom` de opacidade da mesma cor.
    pub fn vgrad(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32, a_top: f32, a_bottom: f32) {
        let (x0, y0, x1, y1) = (x.max(self.clip.0), y.max(self.clip.1), (x + w).min(self.clip.2), (y + h).min(self.clip.3));
        for py in y0..y1 {
            let t = (py - y) as f32 / h.max(1) as f32;
            let a = ((a_top + (a_bottom - a_top) * t).clamp(0.0, 1.0) * 255.0).round() as u32;
            let row = (py * self.w) as usize;
            for px in x0..x1 {
                let i = row + px as usize;
                self.buf[i] = blend(self.buf[i], color, a);
            }
        }
    }

    /// Texto com a linha de base em `baseline`. Devolve a largura pintada.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, font: &mut Font, px: f32, x: f32, baseline: f32, s: &str, color: u32, alpha: f32) -> f32 {
        let lut = lut();
        let mut pen = x;
        let mut prev = None;
        for c in s.chars() {
            let id = font.font.glyph_id(c);
            if let Some(p) = prev {
                pen += font.font.as_scaled(PxScale::from(px)).kern(p, id);
            }
            prev = Some(id);
            if c != ' ' && c != '\t' {
                if let Some(g) = font.glyph(c, px) {
                    let gx = pen.round() as i32 + g.left;
                    let gy = baseline.round() as i32 + g.top;
                    self.blit(&g, gx, gy, color, alpha, lut);
                }
            }
            pen += font.advance(c, px);
        }
        pen - x
    }

    /// Um caractere com origem fixa (modo monoespaçado: a grade manda no avanço).
    #[allow(clippy::too_many_arguments)]
    pub fn glyph_at(&mut self, font: &mut Font, px: f32, x: f32, baseline: f32, c: char, color: u32, alpha: f32) {
        if c == ' ' || c == '\t' {
            return;
        }
        if let Some(g) = font.glyph(c, px) {
            self.blit(&g, x.round() as i32 + g.left, baseline.round() as i32 + g.top, color, alpha, lut());
        }
    }

    fn blit(&mut self, g: &Glyph, gx: i32, gy: i32, color: u32, alpha: f32, lut: &[u8; 256]) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        let ya = (gy).max(cy0);
        let yb = (gy + g.h as i32).min(cy1);
        let xa = gx.max(cx0);
        let xb = (gx + g.w as i32).min(cx1);
        let a8 = (alpha.clamp(0.0, 1.0) * 255.0) as u32;
        for py in ya..yb {
            let row = ((py - gy) as u32 * g.w) as usize;
            let dst = (py * self.w) as usize;
            for px in xa..xb {
                let c = g.cov[row + (px - gx) as usize];
                if c == 0 {
                    continue;
                }
                let a = (lut[c as usize] as u32 * a8 + 127) / 255;
                let i = dst + px as usize;
                self.buf[i] = blend(self.buf[i], color, a);
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn canvas(w: u32, h: u32) -> Vec<u32> {
        vec![0x0020_2020; (w * h) as usize]
    }

    #[test]
    fn retangulo_arredondado_cobre_o_centro_e_suaviza_o_canto() {
        let mut b = canvas(40, 30);
        let mut c = Canvas::new(&mut b, 40, 30);
        c.rrect(5.0, 5.0, 30.0, 20.0, 8.0, 0x00ff_ffff, 1.0);
        assert_eq!(b[15 * 40 + 20] & 0xff, 0xff, "centro sólido");
        assert_eq!(b[0], 0x0020_2020, "fora intocado");
        let canto = b[5 * 40 + 5] & 0xff;
        assert!(canto < 0x30, "o canto fica de fora: {canto:#x}");
        let borda = b[5 * 40 + 20] & 0xff;
        assert!(borda > 0xd0, "a borda reta é cheia: {borda:#x}");
        // meia transparência
        let mut b = canvas(40, 30);
        Canvas::new(&mut b, 40, 30).rrect(5.0, 5.0, 30.0, 20.0, 4.0, 0x00ff_ffff, 0.5);
        let v = b[15 * 40 + 20] & 0xff;
        assert!((0x80..=0x98).contains(&v), "{v:#x}");
    }

    #[test]
    fn sombra_decai_com_a_distancia() {
        let mut b = canvas(80, 60);
        Canvas::new(&mut b, 80, 60).shadow(20.0, 15.0, 40.0, 20.0, 8.0, 10.0, 0.0, 0, 0.8);
        let perto = b[25 * 80 + 40] & 0xff; // dentro do retângulo
        let meio = b[25 * 80 + 66] & 0xff; // 6px fora
        let longe = b[25 * 80 + 79] & 0xff; // 19px fora
        assert!(perto < meio && meio < longe, "{perto} < {meio} < {longe}");
        assert_eq!(longe, 0x20, "longe não é tocado");
    }

    #[test]
    fn texto_inter_mede_e_pinta() {
        let mut f = Font::from_bytes(INTER_REGULAR.to_vec()).expect("Inter embutida");
        assert!(f.measure("Olá mundo", 14.0) > f.measure("Olá", 14.0));
        assert!(f.advance('i', 14.0) < f.advance('m', 14.0), "fonte proporcional");
        let mut b = canvas(120, 30);
        let mut c = Canvas::new(&mut b, 120, 30);
        let w = c.text(&mut f, 16.0, 4.0, 20.0, "Astrograph", 0x00ff_ffff, 1.0);
        assert!(w > 60.0);
        assert!(b.iter().any(|p| *p != 0x0020_2020), "pintou");
        assert!(b.iter().filter(|p| (**p & 0xff) > 0x80).count() > 20, "pixels claros");
    }

    #[test]
    fn clip_limita_o_que_pinta() {
        let mut b = canvas(30, 30);
        let mut c = Canvas::new(&mut b, 30, 30);
        c.set_clip(10, 10, 20, 20);
        c.rect(0, 0, 30, 30, 0x00ff_0000, 1.0);
        assert_eq!(b[5 * 30 + 5], 0x0020_2020);
        assert_eq!(b[15 * 30 + 15], 0x00ff_0000);
    }
}
