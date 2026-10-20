//! A cena do editor nativo: pinta o `ViewModel` em pixels, com animações.
//!
//! Rolagem inercial, cursor que desliza (mola), destaque da linha atual que
//! acompanha, seleção que aparece e some, pílula de status, paleta de comandos que
//! entra com escala e fade, avisos que deslizam e barra de rolagem discreta. Tudo
//! em função do tempo (`dt`), para a mesma sensação em qualquer taxa de quadros.

use std::time::{Duration, Instant};

use astrograph_core::motion::{char_width, vcol_cursor, vcol_start};
use astrograph_core::VKind;

use crate::anim::{ease_out_cubic, ease_out_expo, lerp, mix_color, Chase, Spring};
use crate::design::{self as d, Cls, ModeKind};
use crate::nview::{PickerView, ToastKind, TreeView, ViewModel};
use crate::paint::{Canvas, Font, INTER_REGULAR, INTER_SEMIBOLD};

const MONO_NAMES: &[&str] = &[
    "JetBrainsMonoNerdFontMono-Regular.ttf",
    "JetBrainsMonoNLNerdFont-Regular.ttf",
    "JetBrainsMono-Regular.ttf",
    "SFMono-Regular.otf",
    "consola.ttf",
    "DejaVuSansMono.ttf",
    "LiberationMono-Regular.ttf",
];
const MONO_ITALIC: &[&str] = &["JetBrainsMonoNerdFontMono-Italic.ttf", "JetBrainsMonoNLNerdFont-Italic.ttf", "JetBrainsMono-Italic.ttf", "consolai.ttf", "DejaVuSansMono-Oblique.ttf"];
const MONO_BOLD: &[&str] = &["JetBrainsMonoNerdFontMono-Bold.ttf", "JetBrainsMonoNLNerdFont-Bold.ttf", "JetBrainsMono-Bold.ttf", "consolab.ttf", "DejaVuSansMono-Bold.ttf"];

/// Quanto tempo o aviso fica na tela.
const TOAST_MS: u128 = 3200;

pub struct Frame {
    /// ainda há animação em curso: peça outro quadro logo
    pub animating: bool,
    /// ou volte em tanto tempo (piscar do cursor)
    pub wake: Option<Duration>,
}

/// Medidas da janela (em pixels físicos).
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub text_x: f32,
    pub text_y: f32,
    pub line_h: f32,
    pub char_w: f32,
    pub rows: f32,
    pub cols: usize,
    pub gutter_x: f32,
    pub gutter_w: f32,
}

pub struct Scene {
    mono: Font,
    mono_italic: Option<Font>,
    mono_bold: Option<Font>,
    ui: Font,
    ui_semi: Font,
    scale: f32,
    font_px: f32,
    line_h: f32,
    char_w: f32,
    ascent: f32,
    first: bool,
    top: Spring,
    cur_x: Spring,
    cur_y: Spring,
    cur_w: Spring,
    hl_y: Spring,
    pill_w: Spring,
    sel: Chase,
    last_sel: Option<(VKind, (usize, usize), (usize, usize))>,
    bar: Chase,
    cmd: Chase,
    cmd_cache: (char, String),
    toast: Chase,
    toast_cache: Option<(String, ToastKind)>,
    toast_id: u64,
    toast_at: Instant,
    over: Spring,
    bump_id: u64,
    fade_rows: (usize, usize),
    fade: Chase,
    shift: Spring,
    shift_row: usize,
    last_lines: usize,
    side: Spring,
    tree_cache: Option<TreeView>,
    tree_top: Spring,
    tree_sel: Spring,
    pick: Chase,
    pick_sel: Spring,
    pick_cache: Option<PickerView>,
    clock: Option<String>,
    dash_at: Option<Instant>,
    ready_ms: u32,
    hit_id: u64,
    hit_fade: Chase,
    flash_id: u64,
    flash_at: Option<Instant>,
    flash_range: Option<((usize, usize), (usize, usize))>,
    mode: ModeKind,
    mode_from: u32,
    mode_to: u32,
    mode_at: Instant,
    mode_label: String,
    label_from: String,
    label_at: Instant,
    last_seq: u64,
    last_cursor: (usize, usize),
    moved_at: Instant,
    last_top: f32,
    scrolled_at: Instant,
    last_paint: Option<Instant>,
    /// pintura mais lenta do último segundo (para `ASTROGRAPH_PERF`)
    pub paint_ms: f32,
}

impl Scene {
    pub fn new(font_px: f32, scale: f32) -> Option<Scene> {
        let load = |names: &[&str]| crate::render::find_font_file(names).and_then(|p| Font::from_path(&p));
        let mono = load(MONO_NAMES).or_else(|| Font::from_bytes(INTER_REGULAR.to_vec()))?;
        let now = Instant::now();
        let mut s = Scene {
            mono,
            mono_italic: load(MONO_ITALIC),
            mono_bold: load(MONO_BOLD),
            ui: Font::from_bytes(INTER_REGULAR.to_vec())?,
            ui_semi: Font::from_bytes(INTER_SEMIBOLD.to_vec())?,
            scale,
            font_px,
            line_h: 0.0,
            char_w: 0.0,
            ascent: 0.0,
            first: true,
            top: Spring::new(0.0, 16.0, 1.0),
            cur_x: Spring::new(0.0, 34.0, 1.0),
            cur_y: Spring::new(0.0, 34.0, 1.0),
            cur_w: Spring::new(0.0, 40.0, 1.0),
            hl_y: Spring::new(0.0, 22.0, 1.0),
            pill_w: Spring::new(0.0, 26.0, 1.0),
            sel: Chase::new(0.0, 16.0),
            last_sel: None,
            bar: Chase::new(0.0, 9.0),
            cmd: Chase::new(0.0, 22.0),
            cmd_cache: (':', String::new()),
            toast: Chase::new(0.0, 14.0),
            toast_cache: None,
            toast_id: 0,
            toast_at: now,
            over: Spring::new(0.0, 20.0, 0.5),
            bump_id: 0,
            fade_rows: (0, 0),
            fade: Chase::new(1.0, 9.0),
            shift: Spring::new(0.0, 22.0, 1.0),
            shift_row: usize::MAX,
            last_lines: 0,
            side: Spring::new(0.0, 24.0, 1.0),
            tree_cache: None,
            tree_top: Spring::new(0.0, 26.0, 1.0),
            tree_sel: Spring::new(0.0, 34.0, 1.0),
            pick: Chase::new(0.0, 18.0),
            pick_sel: Spring::new(0.0, 36.0, 1.0),
            pick_cache: None,
            clock: None,
            dash_at: None,
            ready_ms: 0,
            hit_id: 0,
            hit_fade: Chase::new(0.0, 14.0),
            flash_id: 0,
            flash_at: None,
            flash_range: None,
            mode: ModeKind::Normal,
            mode_from: d::ACCENT,
            mode_to: d::ACCENT,
            mode_at: now,
            mode_label: "Normal".into(),
            label_from: "Normal".into(),
            label_at: now,
            last_seq: 0,
            last_cursor: (usize::MAX, 0),
            moved_at: now,
            last_top: 0.0,
            scrolled_at: now,
            last_paint: None,
            paint_ms: 0.0,
        };
        s.set_metrics(font_px, scale);
        Some(s)
    }

    pub fn set_metrics(&mut self, font_px: f32, scale: f32) {
        self.scale = scale;
        self.font_px = font_px;
        self.line_h = (font_px * d::M.line_height).round();
        self.char_w = self.mono.advance('0', font_px);
        let (asc, desc) = self.mono.metrics(font_px);
        // a linha de base fica no meio do espaço da linha
        self.ascent = ((self.line_h - (asc + desc)) / 2.0 + asc).round();
    }

    pub fn line_height(&self) -> f32 {
        self.line_h
    }

    pub fn layout(&self, w: u32, h: u32, lines: usize) -> Layout {
        let s = self.scale;
        let digits = lines.max(100).to_string().len().max(3) as f32;
        let gch = self.char_w * 0.84;
        // a posição acompanha a largura que desliza; o número de colunas já conta a final
        let (sx, sx_final) = (self.side.x.max(0.0), self.side.target.max(0.0));
        let gutter_x = d::M.pad_x * s + sx;
        let gutter_w = digits * gch;
        let text_x = (gutter_x + gutter_w + 18.0 * s).round();
        let text_x_final = (d::M.pad_x * s + sx_final + gutter_w + 18.0 * s).round();
        let text_y = (d::M.title_h * s).round();
        let area_h = h as f32 - text_y - d::M.bottom * s;
        let cols = (((w as f32 - text_x_final - d::M.pad_x * s) / self.char_w).floor() as i64).max(10) as usize;
        Layout { text_x, text_y, line_h: self.line_h, char_w: self.char_w, rows: (area_h / self.line_h).max(3.0), cols, gutter_x, gutter_w }
    }

    /// Hora para o canto inferior direito (`None` desliga o relógio).
    pub fn set_clock(&mut self, text: Option<String>) {
        self.clock = text;
    }

    /// O ponto (em pixels) está sobre o explorador?
    pub fn in_side(&self, x: f32) -> bool {
        self.side.x > 1.0 && x < self.side.x
    }

    /// Posição do clique no texto: (linha do buffer, coluna visual).
    pub fn hit_test(&self, x: f32, y: f32, w: u32, h: u32, vm: &ViewModel) -> (usize, usize) {
        let lay = self.layout(w, h, vm.lines);
        let row = (((y - lay.text_y + self.top.x) / lay.line_h).floor().max(0.0) as usize).min(vm.lines.saturating_sub(1));
        let vcol = vm.left + (((x - lay.text_x) / lay.char_w).floor().max(0.0) as usize);
        (row, vcol)
    }

    /// Pinta o quadro. `now` marca o instante do quadro.
    pub fn paint(&mut self, c: &mut Canvas, vm: &ViewModel, now: Instant) -> Frame {
        let t0 = Instant::now();
        let s = self.scale;
        let (w, h) = (c.w as f32, c.h as f32);
        let lay = self.layout(c.w as u32, c.h as u32, vm.lines);
        let dt = self.last_paint.map_or(0.0, |p| now.saturating_duration_since(p).as_secs_f32()).min(0.05);
        self.last_paint = Some(now);

        self.update_targets(vm, &lay, now);
        if self.first {
            self.snap_all(vm, &lay);
            self.first = false;
        }
        for sp in [&mut self.side, &mut self.tree_top, &mut self.tree_sel, &mut self.pick_sel, &mut self.over, &mut self.shift, &mut self.top, &mut self.cur_x, &mut self.cur_y, &mut self.cur_w, &mut self.hl_y, &mut self.pill_w] {
            sp.step(dt);
        }
        for ch in [&mut self.pick, &mut self.fade, &mut self.sel, &mut self.bar, &mut self.cmd, &mut self.toast, &mut self.hit_fade] {
            ch.step(dt);
        }
        // avisos: somem sozinhos
        if self.toast.target > 0.5 && now.saturating_duration_since(self.toast_at).as_millis() > TOAST_MS {
            self.toast.target = 0.0;
        }
        // barra de rolagem: aparece ao rolar, some depois
        if (self.top.x - self.last_top).abs() > 0.4 {
            self.scrolled_at = now;
            self.bar.target = 1.0;
        } else if now.saturating_duration_since(self.scrolled_at).as_millis() > 900 {
            self.bar.target = 0.0;
        }
        self.last_top = self.top.x;

        c.clear(d::BG);
        if vm.dash {
            if self.dash_at.is_none() {
                self.dash_at = Some(now);
                self.ready_ms = crate::nview::T0.get().map_or(0, |t| t.elapsed().as_millis() as u32);
            }
            self.paint_dash(c, now, (w, h, s));
        } else {
            self.dash_at = None;
            self.paint_content(c, vm, &lay, now);
        }
        self.paint_side(c, (w, h, s));
        self.paint_chrome(c, vm, &lay, now, (w, h, s));

        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.paint_ms = self.paint_ms.max(ms);
        self.frame(vm, now)
    }

    fn update_targets(&mut self, vm: &ViewModel, lay: &Layout, now: Instant) {
        let s = self.scale;
        self.top.target = vm.top * lay.line_h;
        // cursor em coordenadas do documento (a rolagem não o arrasta)
        let line = vm.line(vm.cursor.0);
        let insert = vm.mode == ModeKind::Insert;
        let v = if insert { vcol_start(&line, vm.cursor.1) } else { vcol_cursor(&line, vm.cursor.1) };
        let ch = line.chars().nth(vm.cursor.1);
        let cells = match ch {
            Some('\t') if !insert => 1.0,
            Some(c) if !insert => char_width(c, v) as f32,
            _ => 1.0,
        };
        let beam = insert || vm.cmd.is_some();
        let x = lay.text_x + (v as f32 - vm.left as f32) * lay.char_w;
        self.cur_x.target = x;
        self.cur_y.target = vm.cursor.0 as f32 * lay.line_h;
        self.cur_w.target = if beam { 2.0 * s } else { cells * lay.char_w };
        self.hl_y.target = self.cur_y.target;
        if vm.cursor != self.last_cursor || vm.seq != self.last_seq {
            self.moved_at = now;
        }
        // rolar além do fim: a tela cede um pouco e volta com a mola
        if let Some((id, lines)) = vm.bump {
            if id != self.bump_id {
                if !self.first {
                    let lim = 48.0 * s;
                    self.over.x = (self.over.x - lines.signum() * (lines.abs() * lay.line_h * 0.5).min(lim)).clamp(-lim, lim);
                    self.over.target = 0.0;
                }
                self.bump_id = id;
            }
        }
        // linhas entrando/saindo: o que vem depois desliza até o novo lugar
        if !self.first && self.last_lines != 0 && vm.lines != self.last_lines {
            let n = vm.lines.abs_diff(self.last_lines);
            let lo = self.last_cursor.0.min(vm.cursor.0);
            if n <= 40 {
                let dist = n as f32 * lay.line_h;
                if vm.lines < self.last_lines {
                    self.shift_row = lo;
                    self.shift.x += dist;
                } else {
                    self.shift_row = lo + n + (vm.cursor.0 > self.last_cursor.0) as usize;
                    self.shift.x -= dist;
                    self.fade_rows = (lo + (vm.cursor.0 > self.last_cursor.0) as usize, self.shift_row);
                    self.fade.v = 0.0;
                    self.fade.target = 1.0;
                }
                self.shift.target = 0.0;
            }
        }
        self.last_lines = vm.lines;
        self.last_cursor = vm.cursor;
        self.last_seq = vm.seq;
        // seleção
        if vm.selection.is_some() {
            self.last_sel = vm.selection;
            self.sel.target = 1.0;
        } else {
            self.sel.target = 0.0;
        }
        // modo: a cor e o rótulo trocam suavemente
        if vm.mode != self.mode {
            self.mode_from = self.mode_color(now);
            self.mode_to = vm.mode.color();
            self.mode_at = now;
            self.label_from = self.mode_label.clone();
            self.label_at = now;
            self.mode = vm.mode;
            self.mode_label = vm.mode.label().to_string();
        }
        // explorador: a largura desliza; a janela de itens alimenta a seleção
        match &vm.tree {
            Some(tv) => {
                if self.tree_cache.is_none() {
                    self.tree_sel.snap(tv.sel as f32);
                    self.tree_top.snap(0.0);
                }
                self.side.target = 264.0 * s;
                self.tree_sel.target = tv.sel as f32;
                self.tree_cache = Some(tv.clone());
            }
            None => self.side.target = 0.0,
        }
        // buscador
        match &vm.picker {
            Some(pv) => {
                if self.pick_cache.is_none() || self.pick.target < 0.5 {
                    self.pick_sel.snap(pv.sel as f32);
                }
                self.pick_sel.target = pv.sel as f32;
                self.pick_cache = Some(pv.clone());
                self.pick.target = 1.0;
            }
            None => self.pick.target = 0.0,
        }
        // paleta de comandos
        if let Some((k, t)) = &vm.cmd {
            self.cmd_cache = (*k, t.clone());
            self.cmd.target = 1.0;
        } else {
            self.cmd.target = 0.0;
        }
        // aviso novo
        if let Some((id, text, kind)) = &vm.toast {
            if *id != self.toast_id {
                self.toast_id = *id;
                self.toast_cache = Some((text.clone(), kind.clone()));
                self.toast.target = 1.0;
                self.toast_at = now;
            }
        }
        // clarão de cópia: só para cópias novas (o primeiro quadro adota o contador sem piscar)
        if let Some((a, b, id)) = vm.flash {
            if id != self.flash_id {
                if !self.first {
                    self.flash_range = Some((a, b));
                    self.flash_at = Some(now);
                }
                self.flash_id = id;
            }
        }
        // realce da busca: padrão novo reinicia o fade; sem padrão some
        if vm.hits_id != self.hit_id {
            self.hit_id = vm.hits_id;
            if vm.hits_id != 0 && !self.first {
                self.hit_fade.v = 0.0;
            }
        }
        self.hit_fade.target = if vm.hits_id != 0 { 1.0 } else { 0.0 };
        // pílula: largura-alvo
        let label_w = self.ui_semi.measure(&self.mode_label, 12.5 * s);
        let pos = format!("{}:{}", vm.cursor.0 + 1, vm.cursor.1 + 1);
        let pos_w = self.ui.measure(&pos, 12.0 * s);
        let show_w = if vm.showcmd.is_empty() { 0.0 } else { self.ui.measure(&vm.showcmd, 12.0 * s) + 14.0 * s };
        self.pill_w.target = 18.0 * s + 8.0 * s + 8.0 * s + label_w.max(46.0 * s) + 18.0 * s + 1.0 + 18.0 * s + pos_w.max(34.0 * s) + show_w + 18.0 * s;
    }

    fn snap_all(&mut self, vm: &ViewModel, lay: &Layout) {
        let _ = (vm, lay);
        self.side.snap(self.side.target);
        self.top.snap(self.top.target);
        self.cur_x.snap(self.cur_x.target);
        self.cur_y.snap(self.cur_y.target);
        self.cur_w.snap(self.cur_w.target);
        self.hl_y.snap(self.hl_y.target);
        self.pill_w.snap(self.pill_w.target);
        self.sel.v = self.sel.target;
        self.cmd.v = self.cmd.target;
        self.mode_from = vm.mode.color();
        self.mode_to = vm.mode.color();
        self.mode = vm.mode;
        self.mode_label = vm.mode.label().to_string();
        self.label_from = self.mode_label.clone();
        self.last_top = self.top.x;
    }

    fn mode_color(&self, now: Instant) -> u32 {
        let t = (now.saturating_duration_since(self.mode_at).as_secs_f32() / 0.22).clamp(0.0, 1.0);
        mix_color(self.mode_from, self.mode_to, ease_out_cubic(t))
    }

    /// Opacidade do cursor: aceso enquanto mexe, depois pisca suave.
    fn blink(&self, now: Instant) -> (f32, Option<Duration>) {
        const HOLD: f32 = 0.53;
        const FADE: f32 = 0.14;
        const OFF: f32 = 0.34;
        let t = now.saturating_duration_since(self.moved_at).as_secs_f32();
        if t < HOLD {
            return (1.0, Some(Duration::from_secs_f32(HOLD - t)));
        }
        let cycle = FADE + OFF + FADE + HOLD;
        let p = (t - HOLD) % cycle;
        if p < FADE {
            (1.0 - ease_out_cubic(p / FADE), Some(Duration::from_millis(16)))
        } else if p < FADE + OFF {
            (0.0, Some(Duration::from_secs_f32(FADE + OFF - p)))
        } else if p < FADE + OFF + FADE {
            (ease_out_cubic((p - FADE - OFF) / FADE), Some(Duration::from_millis(16)))
        } else {
            (1.0, Some(Duration::from_secs_f32(cycle - p)))
        }
    }

    fn frame(&self, vm: &ViewModel, now: Instant) -> Frame {
        let springs = [&self.side, &self.tree_top, &self.tree_sel, &self.pick_sel, &self.over, &self.shift, &self.top, &self.cur_x, &self.cur_y, &self.cur_w, &self.hl_y, &self.pill_w].iter().any(|s| !s.settled());
        let chases = [&self.pick, &self.fade, &self.sel, &self.bar, &self.cmd, &self.toast, &self.hit_fade].iter().any(|c| !c.settled());
        let mode_anim = now.saturating_duration_since(self.mode_at).as_millis() < 260;
        let toast_wait = self.toast.target > 0.5;
        let (_, blink_wake) = self.blink(now);
        let blinking = vm.cmd.is_none();
        let mut wake = if blinking { blink_wake } else { None };
        if toast_wait {
            let left = TOAST_MS.saturating_sub(now.saturating_duration_since(self.toast_at).as_millis());
            let d = Duration::from_millis(left as u64 + 5);
            wake = Some(wake.map_or(d, |w| w.min(d)));
        }
        if self.bar.target > 0.5 {
            let d = Duration::from_millis(920u64.saturating_sub(now.saturating_duration_since(self.scrolled_at).as_millis() as u64).max(10));
            wake = Some(wake.map_or(d, |w| w.min(d)));
        }
        let flash = self.flash_at.is_some();
        let intro = vm.dash && self.dash_at.is_some_and(|t| now.saturating_duration_since(t).as_millis() < 700);
        if self.clock.is_some() {
            let d = crate::relogio::ate_proximo_minuto();
            wake = Some(wake.map_or(d, |w| w.min(d)));
        }
        Frame { animating: springs || chases || mode_anim || flash || intro, wake }
    }

    // ------------------------------------------------------------------
    // conteúdo
    // ------------------------------------------------------------------

    fn paint_content(&mut self, c: &mut Canvas, vm: &ViewModel, lay: &Layout, now: Instant) {
        let s = self.scale;
        let (w, h) = (c.w as f32, c.h as f32);
        let top_px = self.top.x - self.over.x;
        let clip_bottom = (h - d::M.bottom * s * 0.55) as i32;
        // o que é do editor (destaques, seleção, cursor) fica preso à área do texto,
        // sem invadir o título nem a base, mesmo quando a mola passa do alvo
        let edit = (self.side.x.max(0.0) as i32, lay.text_y as i32, c.w, (h - d::M.bottom * s) as i32);
        c.set_clip(edit.0, edit.1, edit.2, edit.3);

        // destaque da linha atual: barra macia que desliza
        let hl_y = lay.text_y + self.hl_y.x - top_px;
        if vm.mode != ModeKind::Visual && vm.mode != ModeKind::VisualLine {
            c.rrect(10.0 * s, hl_y, w - 20.0 * s, lay.line_h, 8.0 * s, d::WHITE, 0.045);
        }

        // ocorrências da busca (a atual mais forte)
        if self.hit_fade.v > 0.01 {
            let k = self.hit_fade.v;
            for &(row, a, b) in &vm.hits {
                let y = lay.text_y + row as f32 * lay.line_h - top_px;
                if y + lay.line_h < 0.0 || y > h {
                    continue;
                }
                let line = vm.line(row);
                let (va, vb) = (vcol_start(&line, a).saturating_sub(vm.left), vcol_start(&line, b).saturating_sub(vm.left));
                let x = lay.text_x + va as f32 * lay.char_w;
                let wpx = vb.saturating_sub(va) as f32 * lay.char_w;
                let current = vm.cursor == (row, a);
                let (col, al) = if current { (d::ORANGE, 0.55) } else { (d::ORANGE, 0.24) };
                c.rrect(x - 1.0 * s, y + 2.0, wpx + 2.0 * s, lay.line_h - 4.0, 4.0 * s, col, al * k);
            }
        }

        // seleção
        if self.sel.v > 0.01 {
            if let Some((kind, lo, hi)) = self.last_sel {
                self.paint_selection(c, vm, lay, kind, lo, hi, top_px, self.sel.v);
            }
        }

        // clarão de cópia: o trecho acende e some em ~350 ms
        if let (Some(t0), Some((a, b))) = (self.flash_at, self.flash_range) {
            let k = now.saturating_duration_since(t0).as_secs_f32() / 0.35;
            if k < 1.0 {
                let alpha = 0.55 * (1.0 - crate::anim::ease_out_cubic(k));
                let kind = if a.1 == 0 && b.0 > a.0 && b.1 >= vm.line(b.0).chars().count() { VKind::Line } else { VKind::Char };
                let hi = if kind == VKind::Char && b.1 > 0 { (b.0, b.1 - 1) } else { b };
                self.paint_flash(c, vm, lay, kind, a, hi, top_px, alpha);
            } else {
                self.flash_at = None;
            }
        }

        // o texto rola por baixo do véu do título
        c.set_clip(self.side.x.max(0.0) as i32, 0, c.w, clip_bottom);
        // linhas visíveis
        let pad = (self.shift.x.abs() / lay.line_h).ceil() as i64;
        let first = ((top_px / lay.line_h).floor() as i64 - 1 - pad).max(0) as usize;
        let last = (((top_px + h - lay.text_y) / lay.line_h).ceil() as usize + 1 + pad as usize).min(vm.lines);
        let gpx = self.font_px * 0.84;
        let cur_row = vm.cursor.0;
        let mut classes: Vec<Option<Cls>> = Vec::new();
        // par de colchetes sob o cursor
        let pair: Vec<(usize, usize)> = match_bracket(vm).map(|m| vec![m, vm.cursor]).unwrap_or_default();
        for row in first..last {
            let slide = if row >= self.shift_row { self.shift.x } else { 0.0 };
            let y = lay.text_y + row as f32 * lay.line_h - top_px + slide;
            if y + lay.line_h < 0.0 || y > h {
                continue;
            }
            let base = y + self.ascent;
            // guias de recuo: linhas finas a cada 4 colunas dentro do recuo
            let ind = indent_cols(vm, row);
            let mut g = 0;
            while g < ind {
                let gx = lay.text_x + (g as f32 - vm.left as f32) * lay.char_w;
                if gx >= lay.text_x - 1.0 {
                    c.rect(gx.round() as i32, y as i32, 1.max(s as i32), lay.line_h as i32, d::WHITE, 0.07);
                }
                g += 4;
            }
            for &(mr, mc) in pair.iter().filter(|b| b.0 == row) {
                let l = vm.line(mr);
                let vx = lay.text_x + (vcol_start(&l, mc) as f32 - vm.left as f32) * lay.char_w;
                c.rrect(vx - 1.0 * s, y + 2.0, lay.char_w + 2.0 * s, lay.line_h - 4.0, 4.0 * s, d::ACCENT, 0.30);
            }
            let ra = if row >= self.fade_rows.0 && row < self.fade_rows.1 { self.fade.v.clamp(0.0, 1.0) } else { 1.0 };
            // número da linha
            let num = (row + 1).to_string();
            let nw = self.mono.measure(&num, gpx);
            let ncol = if row == cur_row { d::DIM } else { d::FAINT };
            c.text(&mut self.mono, gpx, lay.gutter_x + lay.gutter_w - nw, base, &num, ncol, ra);
            // texto
            let line = vm.line(row);
            let n = line.chars().count();
            classes.clear();
            classes.resize(n + 1, None);
            for sp in vm.spans_of(row) {
                for i in (sp.start as usize)..(sp.end as usize).min(n) {
                    classes[i] = Some(sp.cls);
                }
            }
            let mut v = 0usize;
            for (i, ch) in line.chars().enumerate() {
                let wd = char_width(ch, v);
                if v + wd > vm.left && v < vm.left + lay.cols + 2 {
                    let (color, bold, italic) = match classes[i] {
                        Some(k) => d::syntax(k),
                        None => (d::TEXT, false, false),
                    };
                    let x = lay.text_x + (v as f32 - vm.left as f32) * lay.char_w;
                    let font = if italic && self.mono_italic.is_some() {
                        self.mono_italic.as_mut().unwrap()
                    } else if bold && self.mono_bold.is_some() {
                        self.mono_bold.as_mut().unwrap()
                    } else {
                        &mut self.mono
                    };
                    c.glyph_at(font, self.font_px, x, base, ch, color, ra);
                }
                v += wd;
            }
        }

        // cursor: de volta à área do editor
        c.set_clip(edit.0, edit.1, edit.2, edit.3);
        self.paint_cursor(c, vm, lay, top_px, now);
        c.set_clip(self.side.x.max(0.0) as i32, 0, c.w, clip_bottom);

        // barra de rolagem
        if self.bar.v > 0.01 && vm.lines as f32 * lay.line_h > h {
            let area = h - lay.text_y - d::M.bottom * s;
            let total = vm.lines as f32 * lay.line_h;
            let thumb = (area * (area / total)).clamp(30.0 * s, area);
            let y = lay.text_y + (area - thumb) * (top_px / (total - lay.line_h).max(1.0)).clamp(0.0, 1.0);
            c.rrect(w - 14.0 * s, y, 5.0 * s, thumb, 2.5 * s, d::WHITE, 0.32 * self.bar.v);
        }
        c.reset_clip();
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_flash(&mut self, c: &mut Canvas, vm: &ViewModel, lay: &Layout, kind: VKind, lo: (usize, usize), hi: (usize, usize), top_px: f32, alpha: f32) {
        let s = self.scale;
        for row in lo.0..=hi.0.min(vm.lines.saturating_sub(1)) {
            let y = lay.text_y + row as f32 * lay.line_h - top_px;
            let line = vm.line(row);
            let len = line.chars().count();
            let (a, b) = match kind {
                VKind::Line => (0, vcol_start(&line, len) + 1),
                VKind::Char => {
                    let a = if row == lo.0 { vcol_start(&line, lo.1.min(len)) } else { 0 };
                    let b = if row == hi.0 { vcol_cursor(&line, hi.1.min(len.saturating_sub(1))) + 1 } else { vcol_start(&line, len) + 1 };
                    (a, b)
                }
            };
            let (a, b) = (a.saturating_sub(vm.left), b.saturating_sub(vm.left));
            let x = lay.text_x + a as f32 * lay.char_w;
            let wpx = (b.saturating_sub(a) as f32 * lay.char_w).max(lay.char_w * 0.6);
            c.rrect(x - 1.0 * s, y + 1.0, wpx + 2.0 * s, lay.line_h - 2.0, 4.0 * s, d::ORANGE, alpha);
        }
    }

    fn paint_selection(&mut self, c: &mut Canvas, vm: &ViewModel, lay: &Layout, kind: VKind, lo: (usize, usize), hi: (usize, usize), top_px: f32, alpha: f32) {
        let s = self.scale;
        for row in lo.0..=hi.0.min(vm.lines.saturating_sub(1)) {
            let y = lay.text_y + row as f32 * lay.line_h - top_px;
            if y + lay.line_h < 0.0 || y > c.h as f32 {
                continue;
            }
            let line = vm.line(row);
            let len = line.chars().count();
            let (a, b) = match kind {
                VKind::Line => (0, vcol_start(&line, len) + 1),
                VKind::Char => {
                    let a = if row == lo.0 { vcol_start(&line, lo.1.min(len)) } else { 0 };
                    let b = if row == hi.0 && hi.1 < len {
                        vcol_cursor(&line, hi.1) + 1
                    } else {
                        vcol_start(&line, len) + 1
                    };
                    (a, b)
                }
            };
            let (a, b) = (a.saturating_sub(vm.left), b.saturating_sub(vm.left));
            let x = lay.text_x + a as f32 * lay.char_w;
            let wpx = ((b.saturating_sub(a)) as f32 * lay.char_w).max(lay.char_w * 0.6);
            c.rrect(x - 1.0 * s, y + 1.0, wpx + 2.0 * s, lay.line_h - 2.0, 4.0 * s, d::ACCENT, 0.30 * alpha);
        }
    }

    fn paint_cursor(&mut self, c: &mut Canvas, vm: &ViewModel, lay: &Layout, top_px: f32, now: Instant) {
        let s = self.scale;
        let (alpha, _) = self.blink(now);
        if vm.cmd.is_some() {
            return;
        }
        let x = self.cur_x.x;
        let y = lay.text_y + self.cur_y.x - top_px;
        let wpx = self.cur_w.x.max(2.0 * s);
        let color = self.mode_color(now);
        let insert = vm.mode == ModeKind::Insert;
        if insert {
            c.rrect(x - 1.0 * s, y + 3.0 * s, wpx, lay.line_h - 6.0 * s, 1.0 * s, color, alpha);
        } else {
            c.rrect(x, y + 1.0, wpx, lay.line_h - 2.0, 4.0 * s, color, 0.92 * alpha);
            // o caractere sob o bloco, no tom do fundo
            let line = vm.line(vm.cursor.0);
            if let Some(ch) = line.chars().nth(vm.cursor.1) {
                if ch != '\t' && alpha > 0.4 {
                    let v = vcol_start(&line, vm.cursor.1);
                    let gx = lay.text_x + (v as f32 - vm.left as f32) * lay.char_w;
                    c.set_clip(x as i32, (y + 1.0) as i32, (x + wpx) as i32 + 1, (y + lay.line_h) as i32);
                    c.glyph_at(&mut self.mono, self.font_px, gx, y + self.ascent, ch, d::BG, alpha);
                    c.reset_clip();
                    // continua valendo o corte do editor
                    c.set_clip(self.side.x.max(0.0) as i32, lay.text_y as i32, c.w, (c.h as f32 - d::M.bottom * s) as i32);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // moldura: título, pílula, paleta, avisos
    // ------------------------------------------------------------------

    fn paint_chrome(&mut self, c: &mut Canvas, vm: &ViewModel, lay: &Layout, now: Instant, (w, h, s): (f32, f32, f32)) {
        // título: o texto rola por baixo de um véu que se dissolve (à direita do explorador)
        let prev_clip = c.clip;
        c.set_clip(self.side.x.max(0.0) as i32, 0, c.w, c.h);
        if vm.dash {
            c.clip = prev_clip;
            self.paint_overlays(c, vm, lay, (w, h, s));
            return;
        }
        c.vgrad(0, 0, w as i32, (lay.text_y + 14.0 * s) as i32, d::BG, 1.0, 0.0);
        c.rect(0, 0, w as i32, (lay.text_y * 0.62) as i32, d::BG, 0.9);
        let tpx = 13.0 * s;
        let tw = self.ui_semi.measure(&vm.name, tpx);
        let dot = if vm.modified { 12.0 * s } else { 0.0 };
        let tx = (w + self.side.x.max(0.0) - tw - dot) / 2.0;
        let tb = lay.text_y * 0.5 + tpx * 0.36;
        c.text(&mut self.ui_semi, tpx, tx, tb, &vm.name, d::TEXT, 0.92);
        if vm.modified {
            c.rrect(tx + tw + 8.0 * s, tb - 6.5 * s, 6.0 * s, 6.0 * s, 3.0 * s, d::ORANGE, 0.95);
        }

        c.clip = prev_clip;
        self.paint_overlays(c, vm, lay, (w, h, s));
        self.paint_pill(c, vm, now, (w, h, s));
    }

    fn paint_overlays(&mut self, c: &mut Canvas, _vm: &ViewModel, lay: &Layout, (w, h, s): (f32, f32, f32)) {
        self.paint_clock(c, (w, h, s));
        // escurece o fundo atrás da paleta
        let veil = self.cmd.v.max(self.pick.v);
        if veil > 0.01 {
            c.rect(0, 0, w as i32, h as i32, d::BLACK, 0.20 * veil);
        }
        self.paint_palette(c, (w, h, s));
        self.paint_picker(c, (w, h, s));
        self.paint_toast(c, lay, (w, h, s));
    }

    /// Tela inicial: estrela, nome em traço fino, linha discreta, menu e rodapé.
    /// Mesma composição do dashboard do Neovim; entra com fade escalonado (~0,6 s).
    fn paint_dash(&mut self, c: &mut Canvas, now: Instant, (w, h, s): (f32, f32, f32)) {
        let p = self.dash_at.map_or(1.0, |t| (now.saturating_duration_since(t).as_secs_f32() / 0.6).min(1.0));
        let at = |start: f32, len: f32| ease_out_cubic(((p - start) / len).clamp(0.0, 1.0));
        let fg = d::TEXT;
        let secondary = mix_color(fg, d::BG, 0.45);
        let tertiary = mix_color(fg, d::BG, 0.62);
        let strong = mix_color(fg, d::WHITE, 0.5);
        let px = self.font_px;
        let (asc, desc) = self.mono.metrics(px);
        let pitch = (asc + desc).round();
        let cw = self.char_w;
        let cx = (w / 2.0).round();
        let mut y = (h * 0.13).round();

        // estrela
        let star = "✦";
        let sw = self.mono.measure(star, px);
        y += pitch;
        let star_col = mix_color(d::BG, d::ACCENT, at(0.0, 0.45));
        if self.mono.has('✦') {
            c.text(&mut self.mono, px, cx - sw / 2.0, y, star, star_col, 1.0);
        } else if self.ui.has('✦') {
            let w2 = self.ui.measure(star, px);
            c.text(&mut self.ui, px, cx - w2 / 2.0, y, star, star_col, 1.0);
        } else {
            // sem o glifo na fonte: estrela de quatro pontas desenhada
            let r = px * 0.5;
            let my = y - px * 0.35;
            c.rrect(cx - r, my - 0.9 * s, 2.0 * r, 1.8 * s, 0.9 * s, d::ACCENT, 0.9);
            c.rrect(cx - 0.9 * s, my - r, 1.8 * s, 2.0 * r, 0.9 * s, d::ACCENT, 0.9);
            c.rrect(cx - r * 0.4, my - r * 0.4, r * 0.8, r * 0.8, r * 0.4, d::ACCENT, 0.9);
        }
        y += pitch * 1.4;

        // nome em traço fino, com leve degradê vertical
        const BANNER: [&str; 3] = [
            "┏━┓┏━┓╺┳╸┏━┓┏━┓┏━╸┏━┓┏━┓┏━┓╻ ╻",
            "┣━┫┗━┓ ┃ ┣┳┛┃ ┃┃╺┓┣┳┛┣━┫┣━┛┣━┫",
            "╹ ╹┗━┛ ╹ ╹┗╸┗━┛┗━┛╹┗╸╹ ╹╹  ╹ ╹",
        ];
        for (i, line) in BANNER.iter().enumerate() {
            let col = mix_color(strong, secondary, 0.18 * i as f32);
            let lw = line.chars().count() as f32 * cw;
            let a = at(0.15 + i as f32 * 0.08, 0.5);
            let mut x = cx - lw / 2.0;
            for ch in line.chars() {
                if ch != ' ' {
                    c.glyph_at(&mut self.mono, px, x.round(), y, ch, mix_color(d::BG, col, a), 1.0);
                }
                x += cw;
            }
            y += pitch;
        }
        y += pitch * 1.4;

        // linha discreta
        let tag = "Editor com agentes de IA";
        let tw = self.mono.measure(tag, px);
        c.text(&mut self.mono, px, cx - tw / 2.0, y, tag, mix_color(d::BG, secondary, at(0.4, 0.45)), 1.0);
        y += pitch * 3.0;

        // menu: ícone, descrição e atalho à direita (coluna de 50 células)
        let menu: [(&str, &str, &str); 6] = [
            ("\u{f0415}", "Novo arquivo", "n"),
            ("\u{f002}", "Buscar arquivo", "f"),
            ("\u{f0214}", "Recentes", "o"),
            ("\u{f0b9a}", "Buscar palavra", "w"),
            ("\u{f06a9}", "Agentes de IA", "a"),
            ("\u{f01b2}", "Última sessão", "s"),
        ];
        let left = cx - 25.0 * cw;
        let right = cx + 25.0 * cw;
        // em janelas baixas o menu se aperta para não encostar no rodapé
        let step = (px * 3.2).min((h * 0.88 - pitch * 2.0 - y) / 5.0).round().max(pitch);
        let m = at(0.45, 0.55);
        for (i, (icon, desc, key)) in menu.iter().enumerate() {
            let base = y + i as f32 * step;
            if i == 0 {
                // cursor sobre a primeira ação, como no dashboard
                c.rrect(left + 3.0 * cw, base - asc, cw, asc + desc_h(&self.mono, px), 2.0 * s, mix_color(d::BG, d::TEXT, m), 0.85);
            }
            let ic = icon.chars().next().unwrap_or(' ');
            if self.mono.has(ic) {
                c.glyph_at(&mut self.mono, px * 0.8, left, base, ic, mix_color(d::BG, secondary, m), 1.0);
            }
            for (j, ch) in desc.chars().enumerate() {
                let col = if i == 0 && j == 0 { d::BG } else { mix_color(d::BG, fg, m) };
                c.glyph_at(&mut self.mono, px, left + (3.0 + j as f32) * cw, base, ch, col, 1.0);
            }
            c.glyph_at(&mut self.mono, px, right - cw, base, key.chars().next().unwrap_or(' '), mix_color(d::BG, tertiary, m), 1.0);
        }

        // rodapé
        let foot = format!("pronto em {} ms", self.ready_ms);
        let fw = self.mono.measure(&foot, px);
        c.text(&mut self.mono, px, cx - fw / 2.0, (h * 0.88).round(), &foot, mix_color(d::BG, tertiary, m), 1.0);
    }

    /// Relógio pequeno e discreto no canto inferior direito, alinhado à pílula.
    fn paint_clock(&mut self, c: &mut Canvas, (w, h, s): (f32, f32, f32)) {
        let Some(text) = self.clock.clone() else { return };
        let px = 12.5 * s;
        let tw = self.ui.measure(&text, px);
        let x = (w - d::M.pad_x * s - tw).round();
        // mesma linha da pílula: centro a 37 px (em escala 1) da borda de baixo
        let mid = h - 20.0 * s - 17.0 * s;
        c.text(&mut self.ui, px, x, (mid + px * 0.36).round(), &text, d::DIM, 0.85);
    }

    fn paint_pill(&mut self, c: &mut Canvas, vm: &ViewModel, now: Instant, (w, h, s): (f32, f32, f32)) {
        let ph = 34.0 * s;
        let pw = self.pill_w.x;
        let x = ((w - pw) / 2.0).round();
        let y = (h - 20.0 * s - ph).round();
        let fade = 1.0 - 0.55 * self.cmd.v;
        c.shadow(x, y, pw, ph, ph / 2.0, 16.0 * s, 6.0 * s, d::BLACK, 0.42 * fade);
        c.rrect(x, y, pw, ph, ph / 2.0, d::SURFACE, 0.95 * fade);
        c.rrect_border(x, y, pw, ph, ph / 2.0, 1.0, d::WHITE, 0.09 * fade);
        let mut cx = x + 18.0 * s;
        let mid = y + ph / 2.0;
        // ponto colorido do modo (cor em transição)
        let col = self.mode_color(now);
        c.rrect(cx, mid - 4.0 * s, 8.0 * s, 8.0 * s, 4.0 * s, col, fade);
        cx += 16.0 * s;
        // rótulo: o anterior sai e o novo entra
        let lt = (now.saturating_duration_since(self.label_at).as_secs_f32() / 0.2).clamp(0.0, 1.0);
        let lpx = 12.5 * s;
        let e = ease_out_cubic(lt);
        if lt < 1.0 {
            c.text(&mut self.ui_semi, lpx, cx, mid + lpx * 0.36 - 5.0 * s * e, &self.label_from.clone(), d::TEXT_STRONG, (1.0 - e) * fade);
            c.text(&mut self.ui_semi, lpx, cx, mid + lpx * 0.36 + 5.0 * s * (1.0 - e), &self.mode_label.clone(), d::TEXT_STRONG, e * fade);
        } else {
            c.text(&mut self.ui_semi, lpx, cx, mid + lpx * 0.36, &self.mode_label.clone(), d::TEXT_STRONG, fade);
        }
        let label_w = self.ui_semi.measure(&self.mode_label, lpx).max(46.0 * s);
        cx += label_w + 18.0 * s;
        c.rect(cx as i32, (mid - 8.0 * s) as i32, 1, (16.0 * s) as i32, d::WHITE, 0.12 * fade);
        cx += 18.0 * s;
        let pos = format!("{}:{}", vm.cursor.0 + 1, vm.cursor.1 + 1);
        let ppx = 12.0 * s;
        c.text(&mut self.ui, ppx, cx, mid + ppx * 0.36, &pos, d::DIM, fade);
        if !vm.showcmd.is_empty() {
            let pw2 = self.ui.measure(&pos, ppx).max(34.0 * s);
            c.text(&mut self.ui_semi, ppx, cx + pw2 + 14.0 * s, mid + ppx * 0.36, &vm.showcmd, d::ACCENT, fade);
        }
    }

    fn paint_palette(&mut self, c: &mut Canvas, (w, h, s): (f32, f32, f32)) {
        let p = self.cmd.v;
        if p < 0.01 {
            return;
        }
        let e = ease_out_expo(p);
        let (pw0, ph0) = ((560.0 * s).min(w - 48.0 * s), 52.0 * s);
        let k = lerp(0.965, 1.0, e);
        let (pw, ph) = (pw0 * k, ph0 * k);
        let x = ((w - pw) / 2.0).round();
        let y = (h * 0.16 + (1.0 - e) * -8.0 * s).round();
        c.shadow(x, y, pw, ph, 14.0 * s, 30.0 * s, 12.0 * s, d::BLACK, 0.6 * p);
        c.rrect(x, y, pw, ph, 14.0 * s, d::SURFACE_HI, 0.985 * p);
        c.rrect_border(x, y, pw, ph, 14.0 * s, 1.0, d::WHITE, 0.11 * p);
        let (kind, text) = self.cmd_cache.clone();
        let mid = y + ph / 2.0;
        let ipx = 18.0 * s;
        let icon = kind.to_string();
        let ix = x + 20.0 * s;
        c.text(&mut self.ui_semi, ipx, ix, mid + ipx * 0.36, &icon, d::ORANGE, p);
        let tx = ix + 22.0 * s;
        let px = self.font_px * 1.05;
        let tw = self.mono.measure(&text, px);
        for (i, ch) in text.chars().enumerate() {
            let cw = self.mono.advance(ch, px);
            c.glyph_at(&mut self.mono, px, tx + i as f32 * cw, mid + px * 0.34, ch, d::TEXT_STRONG, p);
        }
        // cursor de texto
        c.rrect(tx + tw + 1.0 * s, mid - px * 0.62, 2.0 * s, px * 1.24, 1.0 * s, d::ACCENT, p);
    }

    /// Explorador: painel que desliza da esquerda, itens com recuo, seleção que desliza.
    fn paint_side(&mut self, c: &mut Canvas, (w, h, s): (f32, f32, f32)) {
        let sw = self.side.x;
        if sw < 2.0 {
            if self.side.target < 1.0 {
                self.tree_cache = None;
            }
            return;
        }
        let Some(tv) = self.tree_cache.clone() else { return };
        let _ = w;
        let top = (d::M.title_h * s).round();
        let row_h = 27.0 * s;
        let area = h - top - d::M.bottom * s;
        let vis = (area / row_h).floor().max(1.0);
        // mantém a seleção dentro da janela visível
        let want = if (tv.sel as f32) < self.tree_top.target {
            tv.sel as f32
        } else if tv.sel as f32 > self.tree_top.target + vis - 1.0 {
            tv.sel as f32 - vis + 1.0
        } else {
            self.tree_top.target
        };
        self.tree_top.target = want.max(0.0);
        let prev_clip = c.clip;
        c.set_clip(0, 0, sw as i32, c.h);
        // fundo e borda
        c.rect(0, 0, sw as i32, c.h, d::SURFACE, 0.55);
        c.rect((sw - 1.0) as i32, 0, 1.max(s as i32), c.h, d::WHITE, 0.07);
        // título
        let hpx = 12.0 * s;
        c.text(&mut self.ui_semi, hpx, 20.0 * s, top * 0.5 + hpx * 0.36, &tv.root.to_uppercase(), d::DIM, 0.95);
        c.set_clip(0, top as i32, sw as i32, (c.h as f32 - d::M.bottom * s) as i32);
        let t0 = self.tree_top.x;
        // seleção
        let sy = top + (self.tree_sel.x - t0) * row_h;
        let (col, al) = if tv.focus { (d::ACCENT, 0.24) } else { (d::WHITE, 0.07) };
        c.rrect(8.0 * s, sy + 1.0, sw - 16.0 * s - 1.0, row_h - 2.0, 8.0 * s, col, al);
        let npx = 13.0 * s;
        for (i, it) in tv.items.iter().enumerate() {
            let abs = tv.first + i;
            let y = top + (abs as f32 - t0) * row_h;
            if y + row_h < top || y > top + area {
                continue;
            }
            let base = y + row_h / 2.0 + npx * 0.36;
            let x = 18.0 * s + it.level as f32 * 14.0 * s;
            if it.dir {
                // seta: aponta à direita (fechada) ou para baixo (aberta)
                let g = if it.open { "⌄" } else { "›" };
                if self.ui.has(it.open.then_some('⌄').unwrap_or('›')) {
                    c.text(&mut self.ui_semi, npx, x, base - if it.open { 3.0 * s } else { 0.0 }, g, d::DIM, 1.0);
                }
            }
            let nx = x + 15.0 * s;
            let (font_semi, color) = if it.dir { (true, d::TEXT_STRONG) } else { (false, if abs == tv.sel { d::TEXT_STRONG } else { d::TEXT }) };
            let maxw = sw - nx - 12.0 * s;
            let mut name = it.name.clone();
            while self.ui.measure(&name, npx) > maxw && name.chars().count() > 3 {
                name.pop();
            }
            if name != it.name {
                name.push('…');
            }
            if font_semi {
                c.text(&mut self.ui_semi, npx, nx, base, &name, color, 1.0);
            } else {
                c.text(&mut self.ui, npx, nx, base, &name, color, 0.92);
            }
        }
        c.clip = prev_clip;
    }

    /// Buscador: campo de texto, lista com o trecho que casou em destaque e seleção que desliza.
    fn paint_picker(&mut self, c: &mut Canvas, (w, h, s): (f32, f32, f32)) {
        let p = self.pick.v;
        if p < 0.01 {
            return;
        }
        let Some(pv) = self.pick_cache.clone() else { return };
        let e = ease_out_expo(p);
        let rows = pv.items.len().max(1) as f32;
        let (pw0, row_h, head) = ((680.0 * s).min(w - 48.0 * s), 34.0 * s, 56.0 * s);
        let ph0 = head + rows * row_h + 18.0 * s;
        let k = lerp(0.965, 1.0, e);
        let (pw, ph) = (pw0 * k, ph0 * k);
        let x = ((w - pw) / 2.0).round();
        let y = (h * 0.14 + (1.0 - e) * -8.0 * s).round();
        c.shadow(x, y, pw, ph, 16.0 * s, 34.0 * s, 14.0 * s, d::BLACK, 0.6 * p);
        c.rrect(x, y, pw, ph, 16.0 * s, d::SURFACE_HI, 0.985 * p);
        c.rrect_border(x, y, pw, ph, 16.0 * s, 1.0, d::WHITE, 0.11 * p);
        // campo
        let mid = y + head / 2.0 * k;
        let ipx = 13.0 * s;
        c.text(&mut self.ui_semi, ipx, x + 22.0 * s, mid + ipx * 0.36, &pv.title, d::DIM, p);
        let tw0 = self.ui_semi.measure(&pv.title, ipx);
        let px = self.font_px * 1.05;
        let tx = x + 22.0 * s + tw0 + 16.0 * s;
        let mut pen = tx;
        for ch in pv.query.chars() {
            let cw = self.mono.advance(ch, px);
            c.glyph_at(&mut self.mono, px, pen, mid + px * 0.34, ch, d::TEXT_STRONG, p);
            pen += cw;
        }
        c.rrect(pen + 1.0 * s, mid - px * 0.62, 2.0 * s, px * 1.24, 1.0 * s, d::ACCENT, p);
        let cnt = format!("{}", pv.total);
        let cw = self.ui.measure(&cnt, 12.0 * s);
        c.text(&mut self.ui, 12.0 * s, x + pw - 22.0 * s - cw, mid + 12.0 * s * 0.36, &cnt, d::FAINT, p);
        c.rect((x + 14.0 * s) as i32, (y + head - 2.0 * s) as i32, (pw - 28.0 * s) as i32, 1.max(s as i32), d::WHITE, 0.07 * p);
        // lista
        let top = y + head + 6.0 * s;
        if pv.items.is_empty() {
            c.text(&mut self.ui, 13.0 * s, x + 22.0 * s, top + row_h * 0.62, if pv.query.is_empty() { "Nada por aqui ainda" } else { "Nenhum resultado" }, d::DIM, p);
            return;
        }
        // seleção desliza entre as linhas
        let sy = top + self.pick_sel.x * row_h * k;
        c.rrect(x + 8.0 * s, sy, pw - 16.0 * s, row_h * k, 9.0 * s, d::ACCENT, 0.22 * p);
        let fpx = self.font_px;
        for (i, it) in pv.items.iter().enumerate() {
            let base = top + i as f32 * row_h * k + row_h * k * 0.5 + fpx * 0.34;
            let a = p * if i == pv.sel { 1.0 } else { 0.86 };
            let mut pen = x + 22.0 * s;
            let maxx = x + pw - 22.0 * s;
            for (j, ch) in it.text.chars().enumerate() {
                let cw = self.mono.advance(ch, fpx);
                if pen + cw > maxx {
                    break;
                }
                let hit = it.hits.contains(&j);
                let col = if hit { d::ACCENT_SOFT } else if i == pv.sel { d::TEXT_STRONG } else { d::TEXT };
                c.glyph_at(&mut self.mono, fpx, pen, base, ch, col, a);
                pen += cw;
            }
            if !it.detail.is_empty() {
                pen += 18.0 * s;
                for ch in it.detail.chars() {
                    let cw = self.mono.advance(ch, fpx * 0.92);
                    if pen + cw > maxx {
                        break;
                    }
                    c.glyph_at(&mut self.mono, fpx * 0.92, pen, base, ch, d::DIM, a);
                    pen += cw;
                }
            }
        }
    }

    fn paint_toast(&mut self, c: &mut Canvas, lay: &Layout, (w, _h, s): (f32, f32, f32)) {
        let p = self.toast.v;
        if p < 0.01 {
            return;
        }
        let Some((text, kind)) = self.toast_cache.clone() else { return };
        let e = ease_out_expo(p);
        let tpx = 13.0 * s;
        let tw = self.ui.measure(&text, tpx);
        let (bw, bh) = (tw + 56.0 * s, 40.0 * s);
        let x = (w - bw - 24.0 * s + (1.0 - e) * 34.0 * s).round();
        let y = (lay.text_y + 6.0 * s).round();
        c.shadow(x, y, bw, bh, 12.0 * s, 22.0 * s, 8.0 * s, d::BLACK, 0.5 * p);
        c.rrect(x, y, bw, bh, 12.0 * s, d::SURFACE_HI, 0.98 * p);
        c.rrect_border(x, y, bw, bh, 12.0 * s, 1.0, d::WHITE, 0.10 * p);
        let col = match kind {
            ToastKind::Info => d::ACCENT,
            ToastKind::Error => d::RED,
            ToastKind::Done => d::GREEN,
        };
        c.rrect(x + 18.0 * s, y + bh / 2.0 - 4.0 * s, 8.0 * s, 8.0 * s, 4.0 * s, col, p);
        c.text(&mut self.ui, tpx, x + 36.0 * s, y + bh / 2.0 + tpx * 0.36, &text, d::TEXT_STRONG, p);
    }
}

/// Colunas de recuo da linha (tab = 4); linha vazia herda a da próxima com texto.
fn indent_cols(vm: &ViewModel, row: usize) -> usize {
    for r in row..(row + 20).min(vm.lines) {
        let l = vm.line(r);
        if l.trim().is_empty() {
            continue;
        }
        let mut n = 0;
        for ch in l.chars() {
            match ch {
                ' ' => n += 1,
                '\t' => n += 4 - n % 4,
                _ => break,
            }
        }
        return n;
    }
    0
}

/// Colchete que casa com o que está sob o cursor (busca limitada a 600 linhas).
fn match_bracket(vm: &ViewModel) -> Option<(usize, usize)> {
    let (row, col) = vm.cursor;
    let line = vm.line(row);
    let ch = line.chars().nth(col)?;
    let (open, close, fwd) = match ch {
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '{' => ('{', '}', true),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth = 0i32;
    if fwd {
        for r in row..(row + 600).min(vm.lines) {
            let l = vm.line(r);
            for (i, c) in l.chars().enumerate() {
                if r == row && i < col {
                    continue;
                }
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((r, i));
                    }
                }
            }
        }
    } else {
        for r in (row.saturating_sub(600)..=row).rev() {
            let l = vm.line(r);
            let chars: Vec<char> = l.chars().collect();
            for i in (0..chars.len()).rev() {
                if r == row && i > col {
                    continue;
                }
                if chars[i] == close {
                    depth += 1;
                } else if chars[i] == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some((r, i));
                    }
                }
            }
        }
    }
    None
}

fn desc_h(font: &Font, px: f32) -> f32 {
    font.metrics(px).1
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::native::Session;

    fn cena() -> Option<Scene> {
        Scene::new(14.0, 1.0)
    }

    fn quadro(sc: &mut Scene, vm: &ViewModel, w: u32, h: u32, now: Instant) -> Vec<u32> {
        let mut buf = vec![0u32; (w * h) as usize];
        let mut c = Canvas::new(&mut buf, w, h);
        sc.paint(&mut c, vm, now);
        buf
    }

    #[test]
    fn pinta_texto_cursor_e_tem_cores() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("fn main() {\n    let x = 1;\n}\n", Some("a.rs".into()));
        let lay = sc.layout(640, 400, 3);
        s.resize(lay.cols, lay.rows.floor());
        let vm = s.view_model();
        let buf = quadro(&mut sc, &vm, 640, 400, Instant::now());
        assert!(buf.iter().any(|p| *p == d::BG), "fundo");
        let cores: std::collections::HashSet<u32> = buf.iter().copied().collect();
        assert!(cores.len() > 40, "texto com antialias e cores: {}", cores.len());
        // o cursor (azul) está pintado na primeira linha de texto
        let y = (lay.text_y + lay.line_h / 2.0) as usize;
        let x = (lay.text_x + lay.char_w / 2.0) as usize;
        let p = buf[y * 640 + x];
        assert_ne!(p, d::BG);
        assert!((p & 0xff) > (p >> 16 & 0xff), "azulado: {p:#x}");
    }

    #[test]
    fn a_rolagem_anima_em_vez_de_pular() {
        let Some(mut sc) = cena() else { return };
        let linhas: Vec<String> = (1..=200).map(|i| format!("linha {i}")).collect();
        let mut s = Session::new(&linhas.join("\n"), None);
        let lay = sc.layout(640, 400, 200);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        quadro(&mut sc, &vm, 640, 400, t0);
        s.feed("100G");
        let vm = s.view_model();
        assert!(vm.top > 80.0);
        let mut tops = Vec::new();
        let mut animando = 0;
        for i in 1..=40 {
            let mut buf = vec![0u32; 640 * 400];
            let mut c = Canvas::new(&mut buf, 640, 400);
            let f = sc.paint(&mut c, &vm, t0 + Duration::from_millis(16 * i));
            animando += f.animating as u32;
            tops.push(sc.top.x);
        }
        assert!(tops[0] < tops[10] && tops[10] < tops[30], "sobe aos poucos: {:?}", &tops[..12]);
        assert!(tops[0] > 0.0 && tops[0] < vm.top * lay.line_h * 0.5, "o primeiro quadro não salta");
        assert!(animando > 10);
        assert!((tops[39] - vm.top * lay.line_h).abs() < vm.top * lay.line_h * 0.08, "chega perto do alvo");
    }

    #[test]
    fn paleta_de_comandos_entra_com_fade() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("abc\n", None);
        let lay = sc.layout(640, 400, 1);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        let sem = quadro(&mut sc, &vm, 640, 400, t0);
        s.feed(":w");
        let vm = s.view_model();
        let a = quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16));
        let mut b = a.clone();
        for i in 2..24 {
            b = quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16 * i));
        }
        let dif = |x: &[u32], y: &[u32]| x.iter().zip(y).filter(|(p, q)| p != q).count();
        assert!(dif(&sem, &a) > 0, "a paleta começa a aparecer");
        assert!(dif(&a, &b) > 100, "e continua mudando até assentar");
        assert!(sc.cmd.v > 0.9);
    }

    #[test]
    fn clique_vira_linha_e_coluna_visual() {
        let Some(sc) = cena() else { return };
        let mut s = Session::new("abc\ndef\nghi\n", None);
        let lay = sc.layout(640, 400, 3);
        s.resize(lay.cols, lay.rows.floor());
        let vm = s.view_model();
        let (row, vcol) = sc.hit_test(lay.text_x + lay.char_w * 1.5, lay.text_y + lay.line_h * 1.5, 640, 400, &vm);
        assert_eq!((row, vcol), (1, 1));
    }

    #[test]
    fn copiar_acende_o_trecho_e_apaga() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("alpha beta\ngamma\n", None);
        let lay = sc.layout(640, 400, 2);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        let antes = quadro(&mut sc, &vm, 640, 400, t0);
        s.feed("yw");
        let vm = s.view_model();
        assert!(vm.flash.is_some());
        let a = quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16));
        let dif = |x: &[u32], y: &[u32]| x.iter().zip(y).filter(|(p, q)| p != q).count();
        assert!(dif(&antes, &a) > 50, "o trecho copiado acende");
        let mut fim = a.clone();
        for i in 2..40 {
            fim = quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16 * i));
        }
        assert!(dif(&a, &fim) > 50 && sc.flash_at.is_none(), "e apaga sozinho");
    }

    #[test]
    fn busca_realca_ocorrencias_com_fade() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("foo bar foo\nbaz foo\n", None);
        let lay = sc.layout(640, 400, 2);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        let antes = quadro(&mut sc, &vm, 640, 400, t0);
        s.feed("/foo<CR>");
        let vm = s.view_model();
        assert_eq!(vm.hits.len(), 3);
        assert!(vm.hits_id != 0);
        let mut fim = antes.clone();
        for i in 1..30 {
            fim = quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16 * i));
        }
        let dif = antes.iter().zip(&fim).filter(|(p, q)| p != q).count();
        assert!(dif > 200, "ocorrências acesas");
        s.feed(":noh<CR>");
        let vm = s.view_model();
        assert!(vm.hits.is_empty() && vm.hits_id == 0);
    }

    #[test]
    fn apagar_linha_faz_o_resto_subir_deslizando() {
        let Some(mut sc) = cena() else { return };
        let texto: String = (0..8).map(|i| format!("linha {i}\n")).collect();
        let mut s = Session::new(&texto, None);
        let lay = sc.layout(640, 400, 8);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        quadro(&mut sc, &vm, 640, 400, t0);
        s.feed("jdd");
        let vm = s.view_model();
        assert_eq!(vm.lines, 7);
        quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16));
        assert!(sc.shift.x > lay.line_h * 0.5, "o resto começa onde estava");
        let mut fim = vec![];
        for i in 2..60 {
            fim = quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16 * i));
        }
        assert!(!fim.is_empty() && sc.shift.x.abs() < 0.5, "e assenta no lugar");
    }

    #[test]
    fn rolar_alem_do_inicio_cede_e_volta() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("a\nb\nc\n", None);
        let lay = sc.layout(640, 400, 3);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        quadro(&mut sc, &vm, 640, 400, t0);
        s.scroll(-3.0);
        let vm = s.view_model();
        assert!(vm.bump.is_some());
        quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16));
        assert!(sc.over.x > 1.0, "a tela cede para baixo");
        for i in 2..80 {
            quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16 * i));
        }
        assert!(sc.over.x.abs() < 0.5, "e volta ao lugar");
    }

    #[test]
    fn linhas_coladas_entram_com_fade() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("um\ndois\ntres\n", None);
        let lay = sc.layout(640, 400, 3);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        quadro(&mut sc, &vm, 640, 400, t0);
        s.feed("yyjp");
        let vm = s.view_model();
        quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16));
        assert!(sc.fade.v < 0.6 && sc.fade_rows.1 > sc.fade_rows.0, "começa transparente: {:?} {}", sc.fade_rows, sc.fade.v);
        for i in 2..80 {
            quadro(&mut sc, &vm, 640, 400, t0 + Duration::from_millis(16 * i));
        }
        assert!(sc.fade.v > 0.98);
    }

    #[test]
    fn colchetes_casam_e_recuo_conta_tabs() {
        let mut s = Session::new("fn a() {\n\tif (x) {\n\n\t\tb\n\t}\n}\n", None);
        s.resize(80, 20.0);
        let vm = s.view_model();
        assert_eq!(match_bracket(&vm), None);
        s.feed("$");
        let vm = s.view_model();
        assert_eq!(match_bracket(&vm), Some((5, 0)));
        assert_eq!(indent_cols(&vm, 3), 8, "linha vazia herda a da próxima");
        assert_eq!(indent_cols(&vm, 1), 4);
    }

    #[test]
    fn cursor_e_destaque_ficam_presos_a_area_do_texto() {
        let Some(mut sc) = cena() else { return };
        let texto: String = (0..300).map(|i| format!("linha numero {i}\n")).collect();
        let mut s = Session::new(&texto, None);
        let (w, h) = (640u32, 400u32);
        let lay = sc.layout(w, h, 300);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        quadro(&mut sc, &vm, w, h, t0);
        let mut n = 0u64;
        for passo in ["G", "50k", "100k", "300j", "gg", "G"] {
            s.feed(passo);
            let vm = s.view_model();
            for _ in 0..30 {
                n += 1;
                let buf = quadro(&mut sc, &vm, w, h, t0 + Duration::from_millis(8 * n));
                // faixa do título e faixa entre o texto e a pílula: sem a cor do cursor
                let faixas = [(0u32, (lay.text_y as u32).saturating_sub(2)), ((h as f32 - d::M.bottom) as u32 + 1, h - 56)];
                for (y0, y1) in faixas {
                    for y in y0..y1 {
                        for x in 0..w {
                            let px = buf[(y * w + x) as usize];
                            let azul = (px & 0xff) > 0xc0 && ((px >> 8) & 0xff) > 0x60 && ((px >> 16) & 0xff) < 0x40;
                            assert!(!azul, "cursor invadiu a faixa y={y} no passo {passo}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tela_inicial_aparece_some_ao_editar_e_avisa_o_que_falta() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::welcome();
        let lay = sc.layout(800, 600, 1);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        assert!(vm.dash);
        let a = quadro(&mut sc, &vm, 800, 600, t0);
        let mut b = a.clone();
        for i in 1..60 {
            b = quadro(&mut sc, &vm, 800, 600, t0 + Duration::from_millis(16 * i));
        }
        assert!(a.iter().zip(&b).filter(|(x, y)| x != y).count() > 500, "entra com fade");
        s.feed("a");
        let vm = s.view_model();
        assert!(vm.dash && vm.toast.is_some(), "a ainda não existe: avisa e segue na tela inicial");
        s.feed("f");
        assert!(s.view_model().picker.is_some(), "f abre o buscador de arquivos");
        s.feed("<Esc>");
        s.feed("n");
        let vm = s.view_model();
        assert!(!vm.dash && vm.mode == ModeKind::Insert, "n começa a escrever");
    }

    #[test]
    fn buscador_entra_com_fade_e_mostra_resultados() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("x\n", None);
        let d = std::env::temp_dir().join(format!("astrograph-cena-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("um.txt"), "1").unwrap();
        std::fs::write(d.join("dois.txt"), "2").unwrap();
        s.feed(" ff");
        let _ = &d;
        let lay = sc.layout(800, 600, 1);
        s.resize(lay.cols, lay.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        let antes = quadro(&mut sc, &vm, 800, 600, t0);
        s.feed("ff");
        let vm = s.view_model();
        assert!(vm.picker.is_some());
        let mut depois = antes.clone();
        for i in 1..40 {
            depois = quadro(&mut sc, &vm, 800, 600, t0 + Duration::from_millis(16 * i));
        }
        assert!(antes.iter().zip(&depois).filter(|(a, b)| a != b).count() > 2000);
        s.feed("<Esc>");
        let vm = s.view_model();
        for i in 40..100 {
            quadro(&mut sc, &vm, 800, 600, t0 + Duration::from_millis(16 * i));
        }
        assert!(sc.pick.v < 0.01, "fecha com fade");
    }

    #[test]
    fn explorador_desliza_e_empurra_o_texto() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("fn main() {}\n", None);
        let lay0 = sc.layout(800, 600, 1);
        s.resize(lay0.cols, lay0.rows.floor());
        let t0 = Instant::now();
        let vm = s.view_model();
        quadro(&mut sc, &vm, 800, 600, t0);
        let x0 = sc.layout(800, 600, 1).text_x;
        s.feed(" e");
        let vm = s.view_model();
        assert!(vm.tree.is_some());
        quadro(&mut sc, &vm, 800, 600, t0 + Duration::from_millis(16));
        let meio = sc.layout(800, 600, 1).text_x;
        for i in 2..80 {
            quadro(&mut sc, &vm, 800, 600, t0 + Duration::from_millis(16 * i));
        }
        let fim = sc.layout(800, 600, 1).text_x;
        assert!(x0 < meio && meio < fim, "o texto é empurrado aos poucos: {x0} {meio} {fim}");
        assert!(sc.in_side(10.0) && !sc.in_side(fim));
        s.feed(" e");
        let vm = s.view_model();
        for i in 80..160 {
            quadro(&mut sc, &vm, 800, 600, t0 + Duration::from_millis(16 * i));
        }
        assert!((sc.layout(800, 600, 1).text_x - x0).abs() < 1.0, "e volta ao fechar");
    }

    #[test]
    fn relogio_aparece_no_canto_inferior_direito() {
        let Some(mut sc) = cena() else { return };
        let mut s = Session::new("abc\n", None);
        let (w, h) = (800u32, 600u32);
        let lay = sc.layout(w, h, 1);
        s.resize(lay.cols, lay.rows.floor());
        let vm = s.view_model();
        let t0 = Instant::now();
        sc.set_clock(None);
        let sem = quadro(&mut sc, &vm, w, h, t0);
        sc.set_clock(Some("14:32".into()));
        let com = quadro(&mut sc, &vm, w, h, t0);
        let mut dentro = 0;
        let mut fora = 0;
        for y in 0..h {
            for x in 0..w {
                if sem[(y * w + x) as usize] != com[(y * w + x) as usize] {
                    if x > w - 160 && y > h - 70 {
                        dentro += 1;
                    } else {
                        fora += 1;
                    }
                }
            }
        }
        assert!(dentro > 40, "o relógio foi desenhado no canto: {dentro}");
        assert_eq!(fora, 0, "e só ali");
    }

    /// Medição: `cargo test --release custo_do_quadro -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn custo_do_quadro() {
        let Some(mut sc) = cena() else { return };
        let texto: String = (0..60).map(|i| format!("    let valor_{i} = calcular(\"texto\", {i}) + outra_funcao(x, y); // nota\n")).collect();
        let mut s = Session::new(&texto, Some("a.rs".into()));
        for (w, h) in [(1280u32, 800u32), (2560, 1600)] {
            let lay = sc.layout(w, h, 60);
            s.resize(lay.cols, lay.rows.floor());
            let vm = s.view_model();
            let mut buf = vec![0u32; (w * h) as usize];
            let t0 = Instant::now();
            let n = 120;
            for i in 0..n {
                let mut c = Canvas::new(&mut buf, w, h);
                sc.paint(&mut c, &vm, t0 + Duration::from_millis(8 * i));
            }
            println!("{w}x{h}: {:.2} ms/quadro", t0.elapsed().as_secs_f64() * 1000.0 / n as f64);
        }
    }
}
