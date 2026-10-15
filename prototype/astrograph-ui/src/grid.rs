//! Estado da tela reconstruido a partir dos eventos `redraw` do Neovim.
//!
//! Alem do conteudo, guarda quais linhas mudaram desde o ultimo quadro (`dirty`)
//! para o renderer repintar so o necessario, e se o Neovim terminou um quadro
//! (`flush`): desenhar antes disso mostraria estados intermediarios.

use std::collections::HashMap;

use nvim_rs::Value;

#[derive(Clone, Default)]
pub struct Cell {
    /// Grafema da celula (um caractere, ou um caractere com acentos combinantes).
    pub text: CellText,
    pub hl: u64,
    /// Metade direita de um caractere largo (CJK): nao desenha glifo.
    pub cont: bool,
}

/// Texto da celula sem alocar no caso comum (um unico `char`).
#[derive(Clone)]
pub enum CellText {
    Char(char),
    Str(Box<str>),
}

impl Default for CellText {
    fn default() -> Self {
        CellText::Char(' ')
    }
}

impl CellText {
    fn from_str(text: &str) -> Self {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (None, _) => CellText::Char(' '),
            (Some(ch), None) => CellText::Char(ch),
            _ => CellText::Str(text.into()),
        }
    }

    /// Primeiro caractere (o que define o glifo base).
    pub fn first(&self) -> char {
        match self {
            CellText::Char(ch) => *ch,
            CellText::Str(s) => s.chars().next().unwrap_or(' '),
        }
    }

    /// Caracteres combinantes depois do primeiro (acentos, variacoes).
    pub fn rest(&self) -> impl Iterator<Item = char> + '_ {
        let s: &str = match self {
            CellText::Char(_) => "",
            CellText::Str(s) => s,
        };
        s.chars().skip(1)
    }

    pub fn is_blank(&self) -> bool {
        matches!(self, CellText::Char(' '))
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Hl {
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub sp: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub strikethrough: bool,
    pub reverse: bool,
}

#[derive(Clone, Copy)]
pub struct Resolved {
    pub fg: u32,
    pub bg: u32,
    pub sp: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub strikethrough: bool,
}

pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Cell>,
    pub highlights: HashMap<u64, Hl>,
    pub default_fg: u32,
    pub default_bg: u32,
    pub default_sp: u32,
    pub cursor: (usize, usize),
    pub mode: String,
    /// Linhas alteradas desde o ultimo quadro desenhado.
    pub dirty: Vec<bool>,
    /// Tudo mudou (cores, tamanho): o renderer repinta a tela inteira.
    pub all_dirty: bool,
    /// O Neovim mandou `flush`: ha um quadro completo para desenhar.
    pub flushed: bool,
    /// O fundo padrao mudou (a UI ajusta barra de titulo e margens).
    pub bg_changed: bool,
    /// A tabela de highlights ganhou ou mudou entradas desde a ultima publicacao.
    pub hl_changed: bool,
    /// Titulo pedido pelo Neovim (`set_title`), se mudou.
    pub title: Option<String>,
    /// Quadros publicados (0 = o Neovim ainda nao desenhou nada).
    pub generation: u64,
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
            default_sp: 0x00ff0000,
            cursor: (0, 0),
            mode: "normal".to_string(),
            dirty: vec![true; height],
            all_dirty: true,
            flushed: false,
            bg_changed: true,
            hl_changed: false,
            title: None,
            generation: 0,
        }
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells = vec![Cell::default(); width * height];
        self.dirty = vec![true; height];
        self.all_dirty = true;
    }

    fn clear(&mut self) {
        for cell in self.cells.iter_mut() {
            *cell = Cell::default();
        }
        self.all_dirty = true;
    }

    fn mark(&mut self, row: usize) {
        if let Some(d) = self.dirty.get_mut(row) {
            *d = true;
        }
    }

    /// Marca tudo como limpo depois de um quadro desenhado.
    pub fn clean(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = false);
        self.all_dirty = false;
        self.flushed = false;
    }

    /// Copia o quadro terminado (depois de `flush`) para a grade que a UI desenha,
    /// levando so as linhas alteradas. A UI nunca ve um quadro pela metade.
    pub fn publish(&mut self, front: &mut Grid) {
        if (front.width, front.height) != (self.width, self.height) {
            front.width = self.width;
            front.height = self.height;
            front.cells = self.cells.clone();
            front.dirty = vec![true; self.height];
            front.all_dirty = true;
        } else if self.all_dirty {
            front.cells.clone_from_slice(&self.cells);
            front.all_dirty = true;
        } else {
            let w = self.width;
            for (row, dirty) in self.dirty.iter().enumerate() {
                if *dirty {
                    let range = row * w..(row + 1) * w;
                    front.cells[range.clone()].clone_from_slice(&self.cells[range]);
                    front.dirty[row] = true;
                }
            }
        }
        if self.hl_changed {
            front.highlights.clone_from(&self.highlights);
        }
        front.default_fg = self.default_fg;
        front.default_bg = self.default_bg;
        front.default_sp = self.default_sp;
        front.cursor = self.cursor;
        if front.mode != self.mode {
            front.mode.clone_from(&self.mode);
        }
        front.bg_changed |= self.bg_changed;
        if let Some(title) = self.title.take() {
            front.title = Some(title);
        }
        front.generation += 1;
        self.clean();
        self.bg_changed = false;
        self.hl_changed = false;
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for row in 0..self.height {
            let mut line = String::new();
            for cell in &self.cells[row * self.width..(row + 1) * self.width] {
                if cell.cont {
                    continue;
                }
                match &cell.text {
                    CellText::Char(ch) => line.push(*ch),
                    CellText::Str(s) => line.push_str(s),
                }
            }
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
            sp: attr.sp.unwrap_or(if attr.undercurl { self.default_sp } else { fg }) & 0x00ff_ffff,
            bold: attr.bold,
            italic: attr.italic,
            underline: attr.underline,
            undercurl: attr.undercurl,
            strikethrough: attr.strikethrough,
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
                if width > 0 && height > 0 && (width, height) != (self.width, self.height) {
                    self.resize(width, height);
                }
            }
            "grid_clear" => self.clear(),
            "default_colors_set" => {
                let old_bg = self.default_bg;
                if let Some(fg) = args.first().and_then(|v| v.as_i64()).filter(|v| *v >= 0) {
                    self.default_fg = fg as u32 & 0x00ff_ffff;
                }
                if let Some(bg) = args.get(1).and_then(|v| v.as_i64()).filter(|v| *v >= 0) {
                    self.default_bg = bg as u32 & 0x00ff_ffff;
                }
                if let Some(sp) = args.get(2).and_then(|v| v.as_i64()).filter(|v| *v >= 0) {
                    self.default_sp = sp as u32 & 0x00ff_ffff;
                }
                if self.default_bg != old_bg {
                    self.bg_changed = true;
                }
                self.all_dirty = true;
            }
            "hl_attr_define" => {
                let id = args.first().and_then(|v| v.as_u64()).unwrap_or(0);
                let mut hl = Hl::default();
                if let Some(map) = args.get(1).and_then(|v| v.as_map()) {
                    for (key, value) in map {
                        let flag = || value.as_bool().unwrap_or(false);
                        match key.as_str() {
                            Some("foreground") => hl.fg = value.as_u64().map(|v| v as u32),
                            Some("background") => hl.bg = value.as_u64().map(|v| v as u32),
                            Some("special") => hl.sp = value.as_u64().map(|v| v as u32),
                            Some("bold") => hl.bold = flag(),
                            Some("italic") => hl.italic = flag(),
                            Some("underline") | Some("underdouble") | Some("underdotted")
                            | Some("underdashed") => hl.underline |= flag(),
                            Some("undercurl") => hl.undercurl = flag(),
                            Some("strikethrough") => hl.strikethrough = flag(),
                            Some("reverse") => hl.reverse = flag(),
                            _ => {}
                        }
                    }
                }
                // um id redefinido (ex.: troca de tema) pode estar em qualquer linha;
                // um id novo ainda nao foi usado por nenhuma celula
                if let Some(old) = self.highlights.insert(id, hl) {
                    if old != hl {
                        self.all_dirty = true;
                    }
                }
                self.hl_changed = true;
            }
            "grid_line" => {
                let row = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let mut col = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if row >= self.height {
                    return;
                }
                self.mark(row);
                let mut hl = 0;
                if let Some(Value::Array(cells)) = args.get(3) {
                    for raw in cells {
                        let cell = match raw.as_array() {
                            Some(cell) => cell,
                            None => continue,
                        };
                        let text = cell.first().and_then(|v| v.as_str()).unwrap_or(" ");
                        // o hl pode faltar: vale o ultimo informado na mesma linha
                        if let Some(id) = cell.get(1).and_then(|v| v.as_u64()) {
                            hl = id;
                        }
                        let repeat = cell.get(2).and_then(|v| v.as_u64()).unwrap_or(1);
                        // caractere largo: o Neovim manda o glifo e depois uma celula "" (a
                        // metade direita); cada uma ocupa exatamente uma coluna
                        let cont = text.is_empty();
                        let ctext = CellText::from_str(text);
                        for _ in 0..repeat {
                            if col >= self.width {
                                break;
                            }
                            self.cells[row * self.width + col] = Cell {
                                text: ctext.clone(),
                                hl,
                                cont,
                            };
                            col += 1;
                        }
                    }
                }
            }
            "grid_scroll" => {
                let top = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let bot = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let left = args.get(3).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let right = args.get(4).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let rows = args.get(5).and_then(|v| v.as_i64()).unwrap_or(0);
                self.scroll(top, bot, left, right, rows);
            }
            "grid_cursor_goto" => {
                let row = args.get(1).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let col = args.get(2).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let old = self.cursor.0;
                self.cursor = (row, col);
                self.mark(old);
                self.mark(row);
            }
            "mode_change" => {
                if let Some(mode) = args.first().and_then(|v| v.as_str()) {
                    self.mode = mode.to_string();
                }
                let row = self.cursor.0;
                self.mark(row);
            }
            "set_title" => {
                if let Some(title) = args.first().and_then(|v| v.as_str()) {
                    self.title = Some(title.to_string());
                }
            }
            "flush" => self.flushed = true,
            _ => {}
        }
    }

    /// `grid_scroll`: move a regiao [top, bot) x [left, right) por `rows` linhas
    /// (positivo = conteudo sobe). As linhas que ficam vazias serao reescritas
    /// pelo Neovim em seguida.
    fn scroll(&mut self, top: usize, bot: usize, left: usize, right: usize, rows: i64) {
        let bot = bot.min(self.height);
        let right = right.min(self.width);
        if rows == 0 || top >= bot || left >= right {
            return;
        }
        let w = self.width;
        if rows > 0 {
            let n = rows as usize;
            for row in top..bot.saturating_sub(n) {
                let (dst, src) = (row * w, (row + n) * w);
                for col in left..right {
                    self.cells[dst + col] = self.cells[src + col].clone();
                }
            }
        } else {
            let n = (-rows) as usize;
            for row in (top + n..bot).rev() {
                let (dst, src) = (row * w, (row - n) * w);
                for col in left..right {
                    self.cells[dst + col] = self.cells[src + col].clone();
                }
            }
        }
        for row in top..bot {
            self.mark(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(row: u64, col: u64, cells: Vec<Value>) -> Value {
        Value::Array(vec![
            Value::from("grid_line"),
            Value::Array(vec![Value::from(1), Value::from(row), Value::from(col), Value::Array(cells)]),
        ])
    }

    fn cell(text: &str, hl: Option<u64>, repeat: Option<u64>) -> Value {
        let mut v = vec![Value::from(text)];
        if let Some(h) = hl {
            v.push(Value::from(h));
        }
        if let Some(r) = repeat {
            v.push(Value::from(r));
        }
        Value::Array(v)
    }

    #[test]
    fn largo_ocupa_duas_colunas_sem_espaco_extra() {
        let mut g = Grid::new(10, 1);
        g.apply_redraw(&[line(0, 0, vec![
            cell("A", Some(0), None),
            cell("你", None, None),
            cell("", None, None),
            cell("好", None, None),
            cell("", None, None),
            cell("B", None, None),
        ])]);
        assert_eq!(g.to_text().trim_end(), "A你好B");
        assert!(g.cells[2].cont && g.cells[4].cont);
    }

    #[test]
    fn hl_omitido_herda_o_anterior() {
        let mut g = Grid::new(4, 1);
        g.apply_redraw(&[line(0, 0, vec![cell("a", Some(7), None), cell("b", None, None), cell("c", None, None)])]);
        assert_eq!(g.cells[1].hl, 7);
        assert_eq!(g.cells[2].hl, 7);
    }

    #[test]
    fn grafema_com_acento_combinante() {
        let mut g = Grid::new(3, 1);
        g.apply_redraw(&[line(0, 0, vec![cell("e\u{301}", Some(0), None)])]);
        assert_eq!(g.cells[0].text.first(), 'e');
        assert_eq!(g.cells[0].text.rest().collect::<String>(), "\u{301}");
    }

    #[test]
    fn scroll_sobe_e_marca_linhas() {
        let mut g = Grid::new(2, 3);
        for (r, t) in ["a", "b", "c"].iter().enumerate() {
            g.apply_redraw(&[line(r as u64, 0, vec![cell(t, Some(0), None)])]);
        }
        g.clean();
        g.scroll(0, 3, 0, 2, 1);
        assert_eq!(g.cells[0].text.first(), 'b');
        assert_eq!(g.cells[2].text.first(), 'c');
        assert!(g.dirty.iter().all(|d| *d));
    }

    #[test]
    fn flush_sinaliza_quadro() {
        let mut g = Grid::new(2, 2);
        assert!(!g.flushed);
        g.apply_redraw(&[Value::Array(vec![Value::from("flush"), Value::Array(vec![])])]);
        assert!(g.flushed);
    }

    #[test]
    fn publica_so_linhas_sujas() {
        let mut back = Grid::new(3, 3);
        let mut front = Grid::new(3, 3);
        back.publish(&mut front);
        front.clean();
        back.apply_redraw(&[line(1, 0, vec![cell("x", Some(0), None)])]);
        back.publish(&mut front);
        assert_eq!(front.dirty, vec![false, true, false]);
        assert_eq!(front.cells[3].text.first(), 'x');
        assert!(!back.dirty.iter().any(|d| *d));
    }

    #[test]
    fn hl_novo_nao_repinta_tudo() {
        let mut g = Grid::new(2, 2);
        g.clean();
        let def = |fg: u64| Value::Array(vec![
            Value::from("hl_attr_define"),
            Value::Array(vec![Value::from(5), Value::Map(vec![(Value::from("foreground"), Value::from(fg))])]),
        ]);
        g.apply_redraw(&[def(1)]);
        assert!(!g.all_dirty && g.hl_changed);
        g.apply_redraw(&[def(2)]);
        assert!(g.all_dirty);
    }
}
