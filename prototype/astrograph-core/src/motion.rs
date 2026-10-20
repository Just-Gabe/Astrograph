//! Movimentos do Vim, portados do algoritmo dele (`fwd_word`, `bck_word`,
//! `end_word`, `findpar`...). As posições podem ficar em cima do NUL (coluna ==
//! comprimento da linha) durante o cálculo, como no Vim.

use crate::buffer::{Buffer, Pos};

/// Avança um caractere: 0 normal, 2 caiu no NUL do fim da linha, 1 mudou de linha,
/// -1 já estava no fim do arquivo.
pub fn inc(b: &Buffer, p: &mut Pos) -> i32 {
    let len = b.line_len(p.row);
    if p.col < len {
        p.col += 1;
        return if p.col < len { 0 } else { 2 };
    }
    if p.row + 1 < b.line_count() {
        p.row += 1;
        p.col = 0;
        return 1;
    }
    -1
}

/// Recua um caractere: 0 normal, 1 foi para o NUL da linha de cima, -1 início do arquivo.
pub fn dec(b: &Buffer, p: &mut Pos) -> i32 {
    if p.col > 0 {
        p.col -= 1;
        return 0;
    }
    if p.row > 0 {
        p.row -= 1;
        p.col = b.line_len(p.row);
        return 1;
    }
    -1
}

/// Classe do caractere: 0 branco (ou NUL), 1 pontuação, 2 palavra. Em `big` tudo
/// que não é branco é 1.
pub fn cls(b: &Buffer, p: Pos, big: bool) -> u8 {
    match b.char_at(p) {
        None | Some(' ') | Some('\t') => 0,
        Some(c) if big => {
            let _ = c;
            1
        }
        Some(c) if c.is_alphanumeric() || c == '_' => 2,
        Some(_) => 1,
    }
}

fn line_empty(b: &Buffer, row: usize) -> bool {
    b.line_len(row) == 0
}

fn skip_chars(b: &Buffer, p: &mut Pos, class: u8, big: bool, forward: bool) -> bool {
    while cls(b, *p, big) == class {
        let r = if forward { inc(b, p) } else { dec(b, p) };
        if r == -1 {
            return true;
        }
    }
    false
}

/// `w`/`W`. `eol`: com operador pendente (para na ponta da linha). `false` = falhou.
pub fn fwd_word(b: &Buffer, p: &mut Pos, count: usize, big: bool, eol: bool) -> bool {
    let mut count = count as i64;
    loop {
        count -= 1;
        if count < 0 {
            return true;
        }
        let sclass = cls(b, *p, big);
        let last_line = p.row + 1 == b.line_count();
        let i = inc(b, p);
        if i == -1 || (i >= 1 && last_line) {
            return false;
        }
        if i >= 1 && eol && count == 0 {
            return true;
        }
        if sclass != 0 {
            while cls(b, *p, big) == sclass {
                let i = inc(b, p);
                if i == -1 || (i >= 1 && eol && count == 0) {
                    return true;
                }
            }
        }
        while cls(b, *p, big) == 0 {
            if p.col == 0 && line_empty(b, p.row) {
                break;
            }
            let i = inc(b, p);
            if i == -1 || (i >= 1 && last_line) {
                return true;
            }
        }
    }
}

/// `b`/`B`.
pub fn bck_word(b: &Buffer, p: &mut Pos, count: usize, big: bool, stop: bool) -> bool {
    let mut count = count as i64;
    let mut stop = stop;
    loop {
        count -= 1;
        if count < 0 {
            return true;
        }
        let sclass = cls(b, *p, big);
        if dec(b, p) == -1 {
            return false;
        }
        let mut finished = false;
        if !stop || sclass == cls(b, *p, big) || sclass == 0 {
            while cls(b, *p, big) == 0 {
                if p.col == 0 && line_empty(b, p.row) {
                    finished = true;
                    break;
                }
                if dec(b, p) == -1 {
                    return true;
                }
            }
            if !finished {
                let c = cls(b, *p, big);
                if skip_chars(b, p, c, big, false) {
                    return true;
                }
            }
        }
        if !finished {
            inc(b, p);
        }
        stop = false;
    }
}

/// `e`/`E` (e o `ce` com `stop`).
pub fn end_word(b: &Buffer, p: &mut Pos, count: usize, big: bool, stop: bool, empty: bool) -> bool {
    let mut count = count as i64;
    let mut stop = stop;
    loop {
        count -= 1;
        if count < 0 {
            return true;
        }
        let sclass = cls(b, *p, big);
        if inc(b, p) == -1 {
            return false;
        }
        let mut finished = false;
        if cls(b, *p, big) == sclass && sclass != 0 {
            if skip_chars(b, p, sclass, big, true) {
                return false;
            }
        } else if !stop || sclass == 0 {
            while cls(b, *p, big) == 0 {
                if empty && p.col == 0 && line_empty(b, p.row) {
                    finished = true;
                    break;
                }
                if inc(b, p) == -1 {
                    return false;
                }
            }
            if !finished {
                let c = cls(b, *p, big);
                if skip_chars(b, p, c, big, true) {
                    return false;
                }
            }
        }
        if !finished {
            dec(b, p);
        }
        stop = false;
    }
}

/// `ge`/`gE`: fim da palavra anterior (`bckend_word` do Vim).
pub fn bckend_word(b: &Buffer, p: &mut Pos, count: usize, big: bool, eol: bool) -> bool {
    let mut count = count as i64;
    loop {
        count -= 1;
        if count < 0 {
            return true;
        }
        let sclass = cls(b, *p, big);
        let i = dec(b, p);
        if i == -1 {
            return false;
        }
        if eol && i == 1 {
            return true;
        }
        if sclass != 0 {
            while cls(b, *p, big) == sclass {
                let i = dec(b, p);
                if i == -1 || (eol && i == 1) {
                    return true;
                }
            }
        }
        while cls(b, *p, big) == 0 {
            if p.col == 0 && line_empty(b, p.row) {
                break;
            }
            let i = dec(b, p);
            if i == -1 || (eol && i == 1) {
                return true;
            }
        }
    }
}

/// Largura em colunas de tela do caractere na posição (tab até o próximo
/// múltiplo de 8; CJK e afins ocupam 2).
pub fn char_width(c: char, vcol: usize) -> usize {
    if c == '\t' {
        8 - vcol % 8
    } else {
        unicode_width::UnicodeWidthChar::width(c).unwrap_or(1).max(1)
    }
}

/// Coluna visual (0-based) onde o caractere `col` da linha começa.
pub fn vcol_start(line: &str, col: usize) -> usize {
    let mut v = 0;
    for c in line.chars().take(col) {
        v += char_width(c, v);
    }
    v
}

/// Coluna visual da última célula do caractere `col` (o tab ocupa várias).
pub fn vcol_end(line: &str, col: usize) -> usize {
    let start = vcol_start(line, col);
    match line.chars().nth(col) {
        Some(c) => start + char_width(c, start) - 1,
        None => start,
    }
}

/// Coluna visual do cursor no modo normal: o tab conta pela última célula, os
/// demais pela primeira (como o `w_virtcol` do Vim).
pub fn vcol_cursor(line: &str, col: usize) -> usize {
    match line.chars().nth(col) {
        Some('\t') => vcol_end(line, col),
        _ => vcol_start(line, col),
    }
}

/// Caractere cuja faixa visual contém `want` (o último se passar do fim).
pub fn col_for_vcol(line: &str, want: usize) -> usize {
    let mut v = 0;
    let mut last = 0;
    for (i, c) in line.chars().enumerate() {
        let w = char_width(c, v);
        if want < v + w {
            return i;
        }
        v += w;
        last = i;
    }
    last
}

/// `f`/`F`/`t`/`T`. `repeat`: é um `;`/`,` (um `t` colado no alvo pula para o próximo).
pub fn find_char(b: &Buffer, p: Pos, ch: char, forward: bool, till: bool, count: usize, repeat: bool) -> Option<usize> {
    let line: Vec<char> = b.line(p.row).chars().collect();
    let mut col = p.col as i64;
    let mut count = count.max(1);
    let step: i64 = if forward { 1 } else { -1 };
    let mut first = true;
    loop {
        col += step;
        if col < 0 || col as usize >= line.len() {
            return None;
        }
        if line[col as usize] == ch {
            // `;` depois de `t`: um alvo logo ao lado não conta
            if first && repeat && till && count == 1 && col == p.col as i64 + step {
                first = false;
                continue;
            }
            count -= 1;
            if count == 0 {
                let c = if till { col - step } else { col };
                return Some(c as usize);
            }
        }
        first = false;
    }
}

/// `%`: o par do primeiro colchete a partir do cursor, na linha.
pub fn match_pair(b: &Buffer, p: Pos) -> Option<Pos> {
    let line: Vec<char> = b.line(p.row).chars().collect();
    let start = (p.col..line.len()).find(|&i| "()[]{}".contains(line[i]))?;
    let open = line[start];
    let (close, forward) = match open {
        '(' => (')', true),
        '[' => (']', true),
        '{' => ('}', true),
        ')' => ('(', false),
        ']' => ('[', false),
        _ => ('{', false),
    };
    let mut depth = 0i32;
    let mut pos = Pos::new(p.row, start);
    loop {
        match b.char_at(pos) {
            Some(c) if c == open => depth += 1,
            Some(c) if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            _ => {}
        }
        let r = if forward { inc(b, &mut pos) } else { dec(b, &mut pos) };
        if r == -1 {
            return None;
        }
    }
}

/// `{`/`}`: (posição, inclusive).
pub fn paragraph(b: &Buffer, from: Pos, forward: bool, count: usize) -> Option<(Pos, bool)> {
    let n = b.line_count() as i64;
    let dir: i64 = if forward { 1 } else { -1 };
    let mut curr = from.row as i64;
    let mut count = count as i64;
    while count > 0 {
        count -= 1;
        let mut did_skip = false;
        let mut first = true;
        loop {
            if b.line_len(curr as usize) != 0 {
                did_skip = true;
            }
            if !first && did_skip && b.line_len(curr as usize) == 0 {
                break;
            }
            curr += dir;
            if curr < 0 || curr >= n {
                if count > 0 {
                    return None;
                }
                curr -= dir;
                break;
            }
            first = false;
        }
    }
    let row = curr as usize;
    if row + 1 == b.line_count() && forward {
        let len = b.line_len(row);
        if len != 0 {
            return Some((Pos::new(row, len - 1), true));
        }
        return Some((Pos::new(row, 0), false));
    }
    Some((Pos::new(row, 0), false))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn b(lines: &[&str]) -> Buffer {
        Buffer::from_lines(lines)
    }

    #[test]
    fn w_e_b_basico() {
        let buf = b(&["foo bar.baz  qux"]);
        let mut p = Pos::new(0, 0);
        assert!(fwd_word(&buf, &mut p, 1, false, false));
        assert_eq!(p, Pos::new(0, 4));
        assert!(fwd_word(&buf, &mut p, 1, false, false));
        assert_eq!(p, Pos::new(0, 7)); // o "." é uma palavra
        let mut p = Pos::new(0, 0);
        assert!(end_word(&buf, &mut p, 1, false, false, false));
        assert_eq!(p, Pos::new(0, 2));
        let mut p = Pos::new(0, 9);
        assert!(bck_word(&buf, &mut p, 1, false, false));
        assert_eq!(p, Pos::new(0, 8));
    }

    #[test]
    fn linha_vazia_conta_como_palavra() {
        let buf = b(&["a", "", "b"]);
        let mut p = Pos::new(0, 0);
        fwd_word(&buf, &mut p, 1, false, false);
        assert_eq!(p, Pos::new(1, 0));
        fwd_word(&buf, &mut p, 1, false, false);
        assert_eq!(p, Pos::new(2, 0));
    }

    #[test]
    fn f_t_e_pares() {
        let buf = b(&["a(b[c]d)e"]);
        assert_eq!(find_char(&buf, Pos::new(0, 0), 'd', true, false, 1, false), Some(6));
        assert_eq!(find_char(&buf, Pos::new(0, 0), 'd', true, true, 1, false), Some(5));
        assert_eq!(match_pair(&buf, Pos::new(0, 0)), Some(Pos::new(0, 7)));
        assert_eq!(match_pair(&buf, Pos::new(0, 7)), Some(Pos::new(0, 1)));
    }

    #[test]
    fn paragrafos() {
        let buf = b(&["a", "b", "", "c"]);
        assert_eq!(paragraph(&buf, Pos::new(0, 0), true, 1), Some((Pos::new(2, 0), false)));
        assert_eq!(paragraph(&buf, Pos::new(2, 0), true, 1), Some((Pos::new(3, 0), true)));
        assert_eq!(paragraph(&buf, Pos::new(3, 0), false, 1), Some((Pos::new(2, 0), false)));
    }
}
