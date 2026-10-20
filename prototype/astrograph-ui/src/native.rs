//! Editor nativo: o motor modal em Rust (`astrograph-core`) desenhando direto na
//! grade do renderer, sem Neovim. `ASTROGRAPH_ENGINE=native [arquivo]`.
//!
//! `Session` não conhece janela nem threads: recebe teclas, redimensionamentos e
//! cliques, e preenche uma `Grid` (o mesmo formato que o renderer já consome).
//! Por isso é testável sem GUI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use astrograph_core::motion::{char_width, vcol_cursor, vcol_start};
use astrograph_core::{parse_keys, Editor, Key, Mode, Pos, VKind};
use astrograph_syntax::{detect, lang, Document, Lang};

use crate::grid::{Cell, CellText, Grid, Hl};

/// Cores do tema Astrograph (escuro).
mod cor {
    pub const FG: u32 = 0x00dcdce0;
    pub const BG: u32 = 0x001f1f24;
    pub const DIM: u32 = 0x00686878;
    pub const NUM_CUR: u32 = 0x00c8c8d4;
    pub const STATUS_BG: u32 = 0x002a2a32;
    pub const SEL_BG: u32 = 0x003c3c56;
    pub const ACCENT: u32 = 0x007aa2f7;
    pub const GREEN: u32 = 0x009ece6a;
    pub const PURPLE: u32 = 0x00bb9af7;
    pub const ORANGE: u32 = 0x00ff9e64;
    pub const YELLOW: u32 = 0x00e0af68;
    pub const RED: u32 = 0x00f7768e;
    pub const CYAN: u32 = 0x007dcfff;
    pub const GRAY: u32 = 0x0056566a;
}

/// Classes de realce (o que o renderer pinta) a partir do nome da captura.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Cls {
    Comment,
    Str,
    Number,
    Keyword,
    Function,
    Type,
    Operator,
    Punct,
    Property,
    Variable,
    Tag,
}

fn classify(group: &str) -> Option<Cls> {
    let g = group.trim_start_matches('@');
    let head = g.split('.').next().unwrap_or(g);
    Some(match head {
        "comment" => Cls::Comment,
        "string" | "character" => Cls::Str,
        "number" | "boolean" | "constant" | "float" => Cls::Number,
        "keyword" | "conditional" | "repeat" | "include" | "exception" | "label" | "storageclass" => Cls::Keyword,
        "function" | "method" | "constructor" => Cls::Function,
        "type" | "namespace" | "module" | "attribute" => Cls::Type,
        "operator" => Cls::Operator,
        "punctuation" | "delimiter" => Cls::Punct,
        "property" | "field" | "parameter" => Cls::Property,
        "variable" => {
            if g.contains("builtin") {
                Cls::Keyword
            } else if g.contains("member") || g.contains("parameter") {
                Cls::Property
            } else {
                return None;
            }
        }
        "tag" => Cls::Tag,
        _ => return None,
    })
}

fn cls_fg(c: Cls) -> (u32, bool, bool) {
    match c {
        Cls::Comment => (cor::GRAY, false, true),
        Cls::Str => (cor::GREEN, false, false),
        Cls::Number => (cor::ORANGE, false, false),
        Cls::Keyword => (cor::PURPLE, false, false),
        Cls::Function => (cor::ACCENT, false, false),
        Cls::Type => (cor::YELLOW, false, false),
        Cls::Operator => (cor::CYAN, false, false),
        Cls::Punct => (cor::DIM, false, false),
        Cls::Property => (cor::RED, false, false),
        Cls::Variable => (cor::FG, false, false),
        Cls::Tag => (cor::RED, false, false),
    }
}

/// Estilos fixos da interface.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Ui {
    Num,
    NumCur,
    Status,
    ModeNormal,
    ModeInsert,
    ModeVisual,
    ModeCmd,
    Msg,
    Tilde,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key2 {
    Text(Option<Cls>, bool),
    Ui(Ui),
}

pub struct Session {
    pub ed: Editor,
    pub path: Option<PathBuf>,
    saved_rev: u64,
    top: usize,
    left: usize,
    cols: usize,
    rows: usize,
    lang: Option<&'static Lang>,
    doc: Option<(Document, u64)>,
    cmdline: Option<String>,
    pub message: String,
    ids: HashMap<Key2, u64>,
    hl_dirty: bool,
    pub quit: bool,
}

const GUTTER_MIN: usize = 3;
const SCROLLOFF: usize = 3;

impl Session {
    pub fn new(text: &str, path: Option<PathBuf>) -> Self {
        let lines: Vec<String> = text.strip_suffix('\n').unwrap_or(text).split('\n').map(str::to_string).collect();
        let ed = Editor::new(&lines);
        let lang = path.as_ref().and_then(|p| detect(&p.to_string_lossy(), lines.first().map(String::as_str).unwrap_or(""))).and_then(lang);
        let saved_rev = ed.buf.rev;
        Session {
            ed,
            path,
            saved_rev,
            top: 0,
            left: 0,
            cols: 80,
            rows: 24,
            lang,
            doc: None,
            cmdline: None,
            message: String::new(),
            ids: HashMap::new(),
            hl_dirty: true,
            quit: false,
        }
    }

    pub fn open(path: &Path) -> Self {
        let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let mut s = Session::new(&text, Some(path.to_path_buf()));
        if !path.exists() {
            s.message = format!("\"{}\" [novo]", path.display());
        }
        s
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(10);
        self.rows = rows.max(3);
    }

    pub fn modified(&self) -> bool {
        self.ed.buf.rev != self.saved_rev
    }

    // ------------------------------------------------------------------
    // entrada
    // ------------------------------------------------------------------

    pub fn feed(&mut self, keys: &str) {
        for k in parse_keys(keys) {
            self.key(k);
        }
    }

    fn key(&mut self, k: Key) {
        if let Some(cmd) = self.cmdline.as_mut() {
            match k {
                Key::Esc | Key::Ctrl('c') => self.cmdline = None,
                Key::Enter => {
                    let c = self.cmdline.take().unwrap_or_default();
                    self.exec_ex(&c);
                }
                Key::Backspace => {
                    if cmd.pop().is_none() {
                        self.cmdline = None;
                    }
                }
                Key::Char(c) => cmd.push(c),
                Key::Tab => cmd.push('\t'),
                _ => {}
            }
            return;
        }
        if k == Key::Char(':') && !self.ed.has_pending() && self.ed.mode() != Mode::Insert {
            if self.ed.mode() == Mode::Visual {
                self.ed.feed(&[Key::Esc]);
            }
            self.cmdline = Some(String::new());
            self.message.clear();
            return;
        }
        self.message.clear();
        self.ed.feed(&[k]);
    }

    /// Clique: `row`/`col` em células da janela.
    pub fn click(&mut self, row: usize, col: usize) {
        let (gutter, text_rows) = (self.gutter(), self.rows.saturating_sub(2));
        if row >= text_rows || col < gutter {
            return;
        }
        let r = (self.top + row).min(self.ed.buf.line_count() - 1);
        let line = self.ed.buf.line(r);
        let want = self.left + (col - gutter);
        let c = astrograph_core::motion::col_for_vcol(&line, want);
        let len = line.chars().count();
        self.ed.set_cursor(r, if want >= vcol_start(&line, len) { len.saturating_sub(1) } else { c });
    }

    /// Roda da tela: `lines > 0` desce.
    pub fn scroll(&mut self, lines: i64) {
        let max = self.ed.buf.line_count().saturating_sub(1) as i64;
        self.top = (self.top as i64 + lines).clamp(0, max) as usize;
        // o cursor acompanha para não sair da tela
        let text_rows = self.rows.saturating_sub(2).max(1);
        let row = self.ed.cursor.row;
        if row < self.top {
            self.ed.set_cursor(self.top, self.ed.cursor.col);
        } else if row >= self.top + text_rows {
            self.ed.set_cursor(self.top + text_rows - 1, self.ed.cursor.col);
        }
    }

    // ------------------------------------------------------------------
    // comandos `:`
    // ------------------------------------------------------------------

    fn exec_ex(&mut self, line: &str) {
        let line = line.trim();
        let (name, arg) = match line.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (line, ""),
        };
        let bang = name.ends_with('!');
        let name = name.trim_end_matches('!');
        if name.is_empty() {
            return;
        }
        if let Ok(n) = name.parse::<usize>() {
            let row = n.saturating_sub(1).min(self.ed.buf.line_count() - 1);
            self.ed.set_cursor(row, 0);
            self.ed.feed_str("^");
            return;
        }
        match name {
            "w" | "write" => self.write(arg),
            "q" | "quit" => {
                if self.modified() && !bang {
                    self.message = "E37: Alterações não salvas (acrescente ! para forçar)".into();
                } else {
                    self.quit = true;
                }
            }
            "wq" | "x" | "xit" | "exit" => {
                self.write(arg);
                if !self.modified() {
                    self.quit = true;
                }
            }
            "e" | "edit" => {
                if arg.is_empty() {
                    return;
                }
                if self.modified() && !bang {
                    self.message = "E37: Alterações não salvas (acrescente ! para forçar)".into();
                } else {
                    let (cols, rows) = (self.cols, self.rows);
                    *self = Session::open(Path::new(arg));
                    self.resize(cols, rows);
                }
            }
            "noh" | "nohlsearch" | "set" | "syntax" | "filetype" => {}
            _ => self.message = format!("E492: Comando desconhecido: {name}"),
        }
    }

    fn write(&mut self, arg: &str) {
        let target = if arg.is_empty() { self.path.clone() } else { Some(PathBuf::from(arg)) };
        let Some(p) = target else {
            self.message = "E32: Sem nome de arquivo".into();
            return;
        };
        let text = self.ed.to_file_text();
        match std::fs::write(&p, &text) {
            Ok(()) => {
                self.message = format!("\"{}\" {}L, {}B gravado", p.display(), self.ed.buf.line_count(), text.len());
                if self.path.is_none() {
                    self.path = Some(p);
                }
                self.saved_rev = self.ed.buf.rev;
            }
            Err(e) => self.message = format!("E212: Não consegui gravar: {e}"),
        }
    }

    // ------------------------------------------------------------------
    // desenho
    // ------------------------------------------------------------------

    fn gutter(&self) -> usize {
        let digits = self.ed.buf.line_count().to_string().len();
        digits.max(GUTTER_MIN) + 1
    }

    fn id(&mut self, k: Key2) -> u64 {
        if let Some(&i) = self.ids.get(&k) {
            return i;
        }
        let id = self.ids.len() as u64 + 1;
        self.ids.insert(k, id);
        self.hl_dirty = true;
        id
    }

    fn hl_of(k: Key2) -> Hl {
        let mut h = Hl::default();
        match k {
            Key2::Text(c, sel) => {
                if let Some(c) = c {
                    let (fg, bold, italic) = cls_fg(c);
                    h.fg = Some(fg);
                    h.bold = bold;
                    h.italic = italic;
                }
                if sel {
                    h.bg = Some(cor::SEL_BG);
                }
            }
            Key2::Ui(u) => match u {
                Ui::Num => h.fg = Some(cor::DIM),
                Ui::NumCur => {
                    h.fg = Some(cor::NUM_CUR);
                    h.bold = true;
                }
                Ui::Status => {
                    h.fg = Some(cor::NUM_CUR);
                    h.bg = Some(cor::STATUS_BG);
                }
                Ui::ModeNormal | Ui::ModeInsert | Ui::ModeVisual | Ui::ModeCmd => {
                    h.fg = Some(cor::BG);
                    h.bg = Some(match u {
                        Ui::ModeNormal => cor::ACCENT,
                        Ui::ModeInsert => cor::GREEN,
                        Ui::ModeVisual => cor::PURPLE,
                        _ => cor::ORANGE,
                    });
                    h.bold = true;
                }
                Ui::Msg => h.fg = Some(cor::YELLOW),
                Ui::Tilde => h.fg = Some(cor::DIM),
            },
        }
        h
    }

    fn refresh_syntax(&mut self) {
        let Some(l) = self.lang else { return };
        let rev = self.ed.buf.rev;
        if self.doc.as_ref().is_some_and(|(_, r)| *r == rev) {
            return;
        }
        let lines = self.ed.lines();
        self.doc = Some((Document::new(l, &lines), rev));
    }

    fn follow_cursor(&mut self) {
        let text_rows = self.rows.saturating_sub(2).max(1);
        let so = SCROLLOFF.min((text_rows - 1) / 2);
        let row = self.ed.cursor.row;
        if row < self.top + so {
            self.top = row.saturating_sub(so);
        }
        if row + so >= self.top + text_rows {
            self.top = row + so + 1 - text_rows;
        }
        let last = self.ed.buf.line_count().saturating_sub(1);
        if self.top > last {
            self.top = last;
        }
        // horizontal
        let text_cols = self.cols.saturating_sub(self.gutter()).max(1);
        let line = self.ed.buf.line(row);
        let v = vcol_cursor(&line, self.ed.cursor.col);
        if v < self.left {
            self.left = v;
        } else if v >= self.left + text_cols {
            self.left = v + 1 - text_cols;
        }
    }

    /// Preenche `g` (do tamanho da sessão) e marca só as linhas que mudaram.
    pub fn render(&mut self, g: &mut Grid) {
        if (g.width, g.height) != (self.cols, self.rows) {
            *g = Grid::new(self.cols, self.rows);
            self.hl_dirty = true;
        }
        self.follow_cursor();
        self.refresh_syntax();
        let (cols, rows) = (self.cols, self.rows);
        let gutter = self.gutter();
        let text_cols = cols.saturating_sub(gutter);
        let text_rows = rows.saturating_sub(2);
        let mut cells: Vec<Cell> = vec![Cell::default(); cols * rows];

        let sel = self.ed.selection().map(|(k, a)| {
            let c = self.ed.cursor;
            let (lo, hi) = if a <= c { (a, c) } else { (c, a) };
            (k, lo, hi)
        });
        let cur_row = self.ed.cursor.row;
        let line_count = self.ed.buf.line_count();
        let doc_spans = self.doc.as_ref().map(|(d, _)| d.highlights(self.top..(self.top + text_rows).min(line_count)));

        for sr in 0..text_rows {
            let row = self.top + sr;
            if row >= line_count {
                let id = self.id(Key2::Ui(Ui::Tilde));
                put(&mut cells, cols, sr, 0, '~', id);
                continue;
            }
            // número
            let num = format!("{:>w$} ", row + 1, w = gutter - 1);
            let nid = self.id(Key2::Ui(if row == cur_row { Ui::NumCur } else { Ui::Num }));
            for (i, ch) in num.chars().enumerate() {
                put(&mut cells, cols, sr, i, ch, nid);
            }
            // texto
            let line = self.ed.buf.line(row);
            let chars: Vec<char> = line.chars().collect();
            let mut groups: Vec<Option<Cls>> = vec![None; chars.len() + 1];
            if let Some(spans) = &doc_spans {
                // colunas dos spans são bytes: converte para índice de caractere
                let byte_to_char: Vec<usize> = {
                    let mut v = Vec::with_capacity(line.len() + 1);
                    for (ci, ch) in chars.iter().enumerate() {
                        for _ in 0..ch.len_utf8() {
                            v.push(ci);
                        }
                    }
                    v.push(chars.len());
                    v
                };
                for s in spans.iter().filter(|s| s.row == row) {
                    let (a, b) = (byte_to_char.get(s.start_col).copied().unwrap_or(chars.len()), byte_to_char.get(s.end_col).copied().unwrap_or(chars.len()));
                    if let Some(c) = classify(s.group) {
                        for g in groups.iter_mut().take(b).skip(a) {
                            *g = Some(c);
                        }
                    }
                }
            }
            let mut v = 0usize;
            for (ci, ch) in chars.iter().enumerate() {
                let w = char_width(*ch, v);
                let selected = sel.is_some_and(|(k, lo, hi)| in_selection(k, lo, hi, row, ci, chars.len()));
                let id = self.id(Key2::Text(groups[ci], selected));
                if *ch == '\t' {
                    for t in 0..w {
                        put_text(&mut cells, cols, sr, gutter, text_cols, self.left, v + t, ' ', id, false);
                    }
                } else {
                    put_text(&mut cells, cols, sr, gutter, text_cols, self.left, v, *ch, id, false);
                    if w == 2 {
                        put_text(&mut cells, cols, sr, gutter, text_cols, self.left, v + 1, ' ', id, true);
                    }
                }
                v += w;
            }
            // a quebra de linha selecionada (visual sobre o fim da linha)
            if sel.is_some_and(|(k, lo, hi)| in_selection(k, lo, hi, row, chars.len(), chars.len())) && sel.is_some_and(|(k, ..)| k == VKind::Char) {
                let id = self.id(Key2::Text(None, true));
                put_text(&mut cells, cols, sr, gutter, text_cols, self.left, v, ' ', id, false);
            } else if sel.is_some_and(|(k, lo, hi)| k == VKind::Line && row >= lo.row && row <= hi.row) {
                let id = self.id(Key2::Text(None, true));
                for x in v..text_cols + self.left {
                    put_text(&mut cells, cols, sr, gutter, text_cols, self.left, x, ' ', id, false);
                }
            }
        }

        // linha de status
        let (mode_txt, mode_ui) = if self.cmdline.is_some() {
            ("COMANDO", Ui::ModeCmd)
        } else {
            match self.ed.mode() {
                Mode::Normal => ("NORMAL", Ui::ModeNormal),
                Mode::Insert => ("INSERT", Ui::ModeInsert),
                Mode::Visual => match self.ed.selection().map(|s| s.0) {
                    Some(VKind::Line) => ("V-LINHA", Ui::ModeVisual),
                    _ => ("VISUAL", Ui::ModeVisual),
                },
            }
        };
        let st = self.id(Key2::Ui(Ui::Status));
        let mid = self.id(Key2::Ui(mode_ui));
        let srow = rows - 2;
        for c in 0..cols {
            put(&mut cells, cols, srow, c, ' ', st);
        }
        let badge = format!(" {mode_txt} ");
        for (i, ch) in badge.chars().enumerate() {
            put(&mut cells, cols, srow, i, ch, mid);
        }
        let name = self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "[sem nome]".into());
        let left = format!(" {name}{}", if self.modified() { " [+]" } else { "" });
        for (i, ch) in left.chars().enumerate() {
            if badge.chars().count() + i < cols {
                put(&mut cells, cols, srow, badge.chars().count() + i, ch, st);
            }
        }
        let pos = format!("{}:{}  {}% ", self.ed.cursor.row + 1, self.ed.cursor.col + 1, (self.ed.cursor.row + 1) * 100 / line_count.max(1));
        let start = cols.saturating_sub(pos.chars().count());
        for (i, ch) in pos.chars().enumerate() {
            if start + i < cols {
                put(&mut cells, cols, srow, start + i, ch, st);
            }
        }

        // linha de comando / mensagem
        let crow = rows - 1;
        let mut cursor = None;
        if let Some(c) = &self.cmdline {
            let text = format!(":{c}");
            for (i, ch) in text.chars().enumerate() {
                if i < cols {
                    put(&mut cells, cols, crow, i, ch, 0);
                }
            }
            cursor = Some((crow, text.chars().count().min(cols - 1)));
        } else if !self.message.is_empty() {
            let mid = self.id(Key2::Ui(Ui::Msg));
            for (i, ch) in self.message.chars().enumerate() {
                if i < cols {
                    put(&mut cells, cols, crow, i, ch, mid);
                }
            }
        }

        // cursor
        let cursor = cursor.unwrap_or_else(|| {
            let line = self.ed.buf.line(self.ed.cursor.row);
            let v = if self.ed.mode() == Mode::Insert { vcol_start(&line, self.ed.cursor.col) } else { vcol_cursor(&line, self.ed.cursor.col) };
            (self.ed.cursor.row.saturating_sub(self.top).min(text_rows.saturating_sub(1)), (gutter + v.saturating_sub(self.left)).min(cols - 1))
        });

        // publica: só as linhas que mudaram
        if self.hl_dirty {
            g.highlights.clear();
            for (&k, &id) in &self.ids {
                g.highlights.insert(id, Self::hl_of(k));
            }
            g.hl_changed = true;
            g.all_dirty = true;
            self.hl_dirty = false;
        }
        for r in 0..rows {
            let new = &cells[r * cols..(r + 1) * cols];
            let old = &g.cells[r * cols..(r + 1) * cols];
            if g.all_dirty || !row_eq(new, old) {
                g.cells[r * cols..(r + 1) * cols].clone_from_slice(new);
                g.dirty[r] = true;
            }
        }
        g.cursor = cursor;
        g.mode = if self.cmdline.is_some() {
            "cmdline_normal".into()
        } else {
            match self.ed.mode() {
                Mode::Normal => "normal",
                Mode::Insert => "insert",
                Mode::Visual => "visual",
            }
            .into()
        };
        g.title = Some(format!("{} — Astrograph", self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "sem nome".into())));
        g.flushed = true;
    }
}

fn in_selection(kind: VKind, lo: Pos, hi: Pos, row: usize, col: usize, line_len: usize) -> bool {
    match kind {
        VKind::Line => row >= lo.row && row <= hi.row,
        VKind::Char => {
            if row < lo.row || row > hi.row {
                return false;
            }
            let starts = row > lo.row || col >= lo.col;
            let ends = row < hi.row || col <= hi.col;
            let _ = line_len;
            starts && ends
        }
    }
}

fn put(cells: &mut [Cell], cols: usize, row: usize, col: usize, ch: char, hl: u64) {
    if col < cols {
        cells[row * cols + col] = Cell { text: CellText::Char(ch), hl, cont: false };
    }
}

/// Coloca na coluna visual `v` (com rolagem horizontal `left`) dentro da área de texto.
#[allow(clippy::too_many_arguments)]
fn put_text(cells: &mut [Cell], cols: usize, row: usize, gutter: usize, text_cols: usize, left: usize, v: usize, ch: char, hl: u64, cont: bool) {
    if v < left || v - left >= text_cols {
        return;
    }
    let col = gutter + v - left;
    if col < cols {
        cells[row * cols + col] = Cell { text: CellText::Char(ch), hl, cont };
    }
}

fn row_eq(a: &[Cell], b: &[Cell]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.hl == y.hl && x.cont == y.cont && x.text.first() == y.text.first())
}

/// Tarefa do editor nativo: lê a fila de comandos da UI e publica quadros.
pub async fn native_task(
    front: std::sync::Arc<std::sync::Mutex<Grid>>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::UiCommand>,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
    file: Option<PathBuf>,
) {
    use crate::UiCommand;
    let mut s = match &file {
        Some(p) => Session::open(p),
        None => Session::new("", None),
    };
    let mut back = Grid::new(80, 24);
    while let Some(cmd) = rx.recv().await {
        match cmd {
            UiCommand::Keys(k) => s.feed(&k),
            UiCommand::Resize(c, r) => s.resize(c.max(1) as usize, r.max(1) as usize),
            UiCommand::Focus(_) => continue,
            UiCommand::Mouse { button, action, row, col, .. } => match (button, action) {
                ("left", "press") => s.click(row.max(0) as usize, col.max(0) as usize),
                ("wheel", "down") => s.scroll(3),
                ("wheel", "up") => s.scroll(-3),
                _ => continue,
            },
        }
        if s.quit {
            let _ = proxy.send_event(crate::UserEvent::Quit);
            return;
        }
        s.render(&mut back);
        back.publish(&mut front.lock().unwrap());
        let _ = proxy.send_event(crate::UserEvent::Redraw);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn tela(s: &mut Session) -> String {
        let mut g = Grid::new(s.cols, s.rows);
        s.render(&mut g);
        g.to_text()
    }

    #[test]
    fn mostra_texto_numeros_e_status() {
        let mut s = Session::new("hello\nworld\n", Some(PathBuf::from("a.txt")));
        s.resize(40, 8);
        let t = tela(&mut s);
        assert!(t.contains("  1 hello"), "{t}");
        assert!(t.contains("  2 world"), "{t}");
        assert!(t.contains("NORMAL"), "{t}");
        assert!(t.contains("1:1"), "{t}");
        assert!(t.lines().nth(2).unwrap().starts_with("~"), "linhas vazias com til: {t}");
    }

    #[test]
    fn edita_com_o_motor_e_marca_modificado() {
        let mut s = Session::new("one two\nthree\n", Some(PathBuf::from("a.txt")));
        s.resize(40, 8);
        s.feed("dwjA!<Esc>");
        let t = tela(&mut s);
        assert!(t.contains("  1 two"), "{t}");
        assert!(t.contains("  2 three!"), "{t}");
        assert!(t.contains("[+]"), "{t}");
        s.feed("u");
        assert!(tela(&mut s).contains("  2 three"));
    }

    #[test]
    fn comando_dois_pontos_grava_e_sai() {
        let dir = std::env::temp_dir().join(format!("astrograph-native-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.txt");
        let mut s = Session::new("abc\n", Some(f.clone()));
        s.resize(40, 6);
        s.feed("xx:q<CR>");
        assert!(!s.quit, "não sai com alterações");
        assert!(tela(&mut s).contains("E37"));
        s.feed(":w<CR>");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "c\n");
        assert!(!s.modified());
        s.feed(":q<CR>");
        assert!(s.quit);
        let mut t = Session::new("abc\n", Some(f));
        t.feed("x:q!<CR>");
        assert!(t.quit);
    }

    #[test]
    fn modo_visual_na_status_e_cursor() {
        let mut s = Session::new("abc def\n", None);
        s.resize(40, 6);
        s.feed("vee");
        let mut g = Grid::new(40, 6);
        s.render(&mut g);
        assert!(g.to_text().contains("VISUAL"));
        assert_eq!(g.mode, "visual");
        // cursor na coluna do 'f' (4 do gutter + 6)
        assert_eq!(g.cursor, (0, 4 + 6));
        // célula selecionada tem fundo diferente
        let sel = g.resolve(g.cells[4].hl);
        let normal = g.resolve(g.cells[4 + 8].hl);
        assert_ne!(sel.bg, normal.bg);
    }

    #[test]
    fn realce_de_sintaxe_do_tree_sitter() {
        let mut s = Session::new("local x = 1 -- ok\n", Some(PathBuf::from("a.lua")));
        s.resize(40, 6);
        let mut g = Grid::new(40, 6);
        s.render(&mut g);
        let cor_kw = g.resolve(g.cells[4].hl).fg; // 'local'
        let cor_x = g.resolve(g.cells[4 + 6].hl).fg; // 'x'
        assert_ne!(cor_kw, cor_x, "palavra-chave e variável com cores diferentes");
    }

    #[test]
    fn tab_e_rolagem() {
        let linhas: Vec<String> = (1..=50).map(|i| format!("\tlinha {i}")).collect();
        let mut s = Session::new(&linhas.join("\n"), None);
        s.resize(40, 10);
        assert!(tela(&mut s).contains("  1         linha 1"));
        s.feed("30G");
        let t = tela(&mut s);
        assert!(t.contains(" 30 "), "a linha 30 apareceu: {t}");
        assert!(!t.contains("  1 "), "rolou: {t}");
    }
}
