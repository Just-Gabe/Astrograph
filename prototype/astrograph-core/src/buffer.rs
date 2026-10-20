//! Texto do buffer: uma `Rope` sem quebra de linha no fim. Um buffer vazio tem
//! uma linha vazia (como no Vim). Colunas são índices de caractere.

use ropey::Rope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(row: usize, col: usize) -> Self {
        Pos { row, col }
    }
}

#[derive(Debug, Clone)]
pub struct Buffer {
    pub rope: Rope,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer { rope: Rope::new() }
    }
}

impl Buffer {
    pub fn from_lines<S: AsRef<str>>(lines: &[S]) -> Self {
        let text = lines.iter().map(|l| l.as_ref()).collect::<Vec<_>>().join("\n");
        Buffer { rope: Rope::from_str(&text) }
    }

    pub fn from_text(text: &str) -> Self {
        Buffer { rope: Rope::from_str(text.strip_suffix('\n').unwrap_or(text)) }
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Comprimento da linha em caracteres, sem o `\n`.
    pub fn line_len(&self, row: usize) -> usize {
        let l = self.rope.line(row);
        let n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n - 1
        } else {
            n
        }
    }

    pub fn line(&self, row: usize) -> String {
        let l = self.rope.line(row).to_string();
        l.strip_suffix('\n').unwrap_or(&l).to_string()
    }

    pub fn lines(&self) -> Vec<String> {
        (0..self.line_count()).map(|r| self.line(r)).collect()
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Caractere em `pos`, ou `None` na coluna do fim da linha (o NUL do Vim).
    pub fn char_at(&self, pos: Pos) -> Option<char> {
        if pos.row >= self.line_count() || pos.col >= self.line_len(pos.row) {
            return None;
        }
        Some(self.rope.char(self.idx(pos)))
    }

    /// Índice de caractere da posição na rope.
    pub fn idx(&self, pos: Pos) -> usize {
        self.rope.line_to_char(pos.row) + pos.col
    }

    /// Posição de um índice de caractere.
    pub fn pos_of(&self, idx: usize) -> Pos {
        let idx = idx.min(self.rope.len_chars());
        let row = self.rope.char_to_line(idx);
        Pos { row, col: idx - self.rope.line_to_char(row) }
    }

    /// Remove `[a, b)` e devolve o texto removido.
    pub fn delete(&mut self, a: Pos, b: Pos) -> String {
        let (i, j) = (self.idx(a), self.idx(b));
        if i >= j {
            return String::new();
        }
        let out = self.rope.slice(i..j).to_string();
        self.rope.remove(i..j);
        out
    }

    pub fn insert(&mut self, at: Pos, text: &str) {
        let i = self.idx(at);
        self.rope.insert(i, text);
    }

    /// Insere uma linha inteira antes de `row` (`row == line_count()` acrescenta no fim).
    pub fn insert_line(&mut self, row: usize, text: &str) {
        if row >= self.line_count() {
            let end = self.rope.len_chars();
            self.rope.insert(end, &format!("\n{text}"));
        } else {
            let i = self.rope.line_to_char(row);
            self.rope.insert(i, &format!("{text}\n"));
        }
    }

    /// Remove as linhas `[a, b]` (inclusive). Se for o buffer todo, sobra uma linha vazia.
    pub fn delete_lines(&mut self, a: usize, b: usize) -> Vec<String> {
        let removed: Vec<String> = (a..=b).map(|r| self.line(r)).collect();
        let n = self.line_count();
        if a == 0 && b + 1 >= n {
            self.rope = Rope::new();
        } else if b + 1 >= n {
            // últimas linhas: leva junto o \n que as separa da anterior
            let start = self.rope.line_to_char(a) - 1;
            let end = self.rope.len_chars();
            self.rope.remove(start..end);
        } else {
            let start = self.rope.line_to_char(a);
            let end = self.rope.line_to_char(b + 1);
            self.rope.remove(start..end);
        }
        removed
    }

    /// Primeira coluna que não é espaço nem tab (o fim da linha se for toda em branco).
    pub fn first_nonblank(&self, row: usize) -> usize {
        let l = self.line(row);
        l.chars().position(|c| c != ' ' && c != '\t').unwrap_or_else(|| l.chars().count())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn linhas_e_vazio() {
        let b = Buffer::default();
        assert_eq!(b.line_count(), 1);
        assert_eq!(b.line(0), "");
        let b = Buffer::from_lines(&["ab", "", "cde"]);
        assert_eq!(b.line_count(), 3);
        assert_eq!((b.line_len(0), b.line_len(1), b.line_len(2)), (2, 0, 3));
        assert_eq!(b.char_at(Pos::new(2, 1)), Some('d'));
        assert_eq!(b.char_at(Pos::new(0, 2)), None);
    }

    #[test]
    fn apagar_linhas() {
        let mut b = Buffer::from_lines(&["a", "b", "c", "d"]);
        assert_eq!(b.delete_lines(1, 2), vec!["b", "c"]);
        assert_eq!(b.lines(), vec!["a", "d"]);
        b.delete_lines(1, 1);
        assert_eq!(b.lines(), vec!["a"]);
        b.delete_lines(0, 0);
        assert_eq!(b.lines(), vec![""]);
    }

    #[test]
    fn inserir() {
        let mut b = Buffer::from_lines(&["ac"]);
        b.insert(Pos::new(0, 1), "b");
        b.insert_line(1, "x");
        b.insert_line(0, "y");
        assert_eq!(b.lines(), vec!["y", "abc", "x"]);
        assert_eq!(b.delete(Pos::new(1, 0), Pos::new(1, 2)), "ab");
    }
}
