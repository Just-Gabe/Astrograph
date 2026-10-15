//! Estado da tela reconstruido a partir dos eventos `redraw` do Neovim.

use std::collections::HashMap;

use nvim_rs::Value;

#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub hl: u64,
    /// Celula de continuacao de um caractere largo (nao desenha glifo).
    pub cont: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            hl: 0,
            cont: false,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Hl {
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Clone, Copy)]
pub struct Resolved {
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Cell>,
    pub highlights: HashMap<u64, Hl>,
    pub default_fg: u32,
    pub default_bg: u32,
    pub cursor: (usize, usize),
    pub mode: String,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        Grid {
            width,
            height,
            cells: vec![Cell::default(); width * height],
            highlights: HashMap::new(),
            default_fg: 0x00d0d0d0,
            default_bg: 0x00101014,
            cursor: (0, 0),
            mode: "normal".to_string(),
        }
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells = vec![Cell::default(); width * height];
    }

    fn clear(&mut self) {
        for cell in self.cells.iter_mut() {
            *cell = Cell::default();
        }
    }

    fn put(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.height && col < self.width {
            self.cells[row * self.width + col] = cell;
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for row in 0..self.height {
            let line: String = self.cells[row * self.width..(row + 1) * self.width]
                .iter()
                .map(|c| c.ch)
                .collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    pub fn resolve(&self, hl: u64) -> Resolved {
        let attr = self.highlights.get(&hl).copied().unwrap_or_default();
        let mut fg = attr.fg.unwrap_or(self.default_fg);
        let mut bg = attr.bg.unwrap_or(self.default_bg);
        if attr.reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        Resolved {
            fg: fg & 0x00ff_ffff,
            bg: bg & 0x00ff_ffff,
            bold: attr.bold,
            italic: attr.italic,
            underline: attr.underline,
        }
    }

    /// Aplica um lote `redraw`. Cada entrada e `[nome, payload1, payload2, ...]`
    /// (eventos consecutivos de mesmo nome vem agrupados).
    pub fn apply_redraw(&mut self, events: &[Value]) {
        for event in events {
            let parts = match event.as_array() {
                Some(parts) => parts,
                None => continue,
            };
            let name = parts.first().and_then(|v| v.as_str()).unwrap_or("");
            for payload in parts.iter().skip(1) {
                if let Some(args) = payload.as_array() {
                    self.apply_event(name, args);
                }
            }
        }
    }

    fn apply_event(&mut self, name: &str, args: &[Value]) {
        match name {
            "grid_resize" => {
                let width = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let height = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if width > 0 && height > 0 {
                    self.resize(width, height);
                }
            }
            "grid_clear" => self.clear(),
            "default_colors_set" => {
                if let Some(fg) = args.first().and_then(|v| v.as_u64()) {
                    self.default_fg = fg as u32 & 0x00ff_ffff;
                }
                if let Some(bg) = args.get(1).and_then(|v| v.as_u64()) {
                    self.default_bg = bg as u32 & 0x00ff_ffff;
                }
            }
            "hl_attr_define" => {
                let id = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
                let mut hl = Hl::default();
                if let Some(map) = args.get(1).and_then(|v| v.as_map()) {
                    for (key, value) in map {
                        match key.as_str() {
                            Some("foreground") => hl.fg = value.as_u64().map(|v| v as u32),
                            Some("background") => hl.bg = value.as_u64().map(|v| v as u32),
                            Some("bold") => hl.bold = value.as_bool().unwrap_or(false),
                            Some("italic") => hl.italic = value.as_bool().unwrap_or(false),
                            Some("underline") => hl.underline = value.as_bool().unwrap_or(false),
                            Some("reverse") => hl.reverse = value.as_bool().unwrap_or(false),
                            _ => {}
                        }
                    }
                }
                self.highlights.insert(id, hl);
            }
            "grid_line" => {
                let row = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let mut col = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if let Some(Value::Array(cells)) = args.get(3) {
                    for raw in cells {
                        let cell = match raw.as_array() {
                            Some(cell) => cell,
                            None => continue,
                        };
                        let text = cell.first().and_then(|v| v.as_str()).unwrap_or(" ");
                        let hl = cell.get(1).and_then(|v| v.as_u64()).unwrap_or(0);
                        let repeat = cell.get(2).and_then(|v| v.as_u64()).unwrap_or(1);
                        let mut chars = text.chars();
                        let ch = chars.next().unwrap_or(' ');
                        let wide = crate::render::char_width(ch) == 2;
                        for _ in 0..repeat {
                            if wide {
                                self.put(row, col, Cell { ch, hl, cont: false });
                                self.put(
                                    row,
                                    col + 1,
                                    Cell {
                                        ch: ' ',
                                        hl,
                                        cont: true,
                                    },
                                );
                                col += 2;
                            } else {
                                self.put(
                                    row,
                                    col,
                                    Cell {
                                        ch,
                                        hl,
                                        cont: false,
                                    },
                                );
                                col += 1;
                            }
                        }
                    }
                }
            }
            "grid_scroll" => {
                let top = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as i64;
                let bot = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as i64;
                let left = args.get(3).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let right = args.get(4).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let rows = args.get(5).and_then(|v| v.as_i64()).unwrap_or(0);
                self.scroll(top, bot, left, right, rows);
            }
            "grid_cursor_goto" => {
                let row = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let col = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                self.cursor = (row, col);
            }
            "mode_change" => {
                if let Some(mode) = args.first().and_then(|v| v.as_str()) {
                    self.mode = mode.to_string();
                }
            }
            _ => {}
        }
    }

    fn scroll(&mut self, top: i64, bot: i64, left: usize, right: usize, rows: i64) {        if rows == 0 || top < 0 || bot > self.height as i64 || top >= bot {
            return;
        }
        let top = top as usize;
        let bot = bot as usize;
        if rows > 0 {
            for row in top..bot {
                let src = row as i64 + rows;
                for col in left..right.min(self.width) {
                    let cell = if src < bot as i64 {
                        self.cells[src as usize * self.width + col]
                    } else {
                        Cell::default()
                    };
                    self.cells[row * self.width + col] = cell;
                }
            }
        } else {
            let rows = (-rows) as usize;
            for row in (top..bot).rev() {
                let src = row as i64 - rows as i64;
                for col in left..right.min(self.width) {
                    let cell = if src >= top as i64 {
                        self.cells[src as usize * self.width + col]
                    } else {
                        Cell::default()
                    };
                    self.cells[row * self.width + col] = cell;
                }
            }
        }
    }
}
