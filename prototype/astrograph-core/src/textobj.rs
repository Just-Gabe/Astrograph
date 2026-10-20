//! Objetos de texto do Vim (`iw aw ip ap i( a( i" a"`...), portados de
//! `current_word`, `current_par`, `current_block` e `current_quote`.

use crate::buffer::{Buffer, Pos};
use crate::motion::{self, cls, dec, inc};

/// Região escolhida por um objeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Obj {
    pub start: Pos,
    pub end: Pos,
    pub inclusive: bool,
    pub linewise: bool,
}

/// `inc` que pula o NUL do fim da linha.
fn incl(b: &Buffer, p: &mut Pos) -> i32 {
    let mut r = inc(b, p);
    if r >= 1 && p.col > 0 {
        r = inc(b, p);
    }
    r
}

/// `dec` que pula o NUL do fim da linha.
fn decl(b: &Buffer, p: &mut Pos) -> i32 {
    let mut r = dec(b, p);
    if r == 1 && p.col > 0 {
        r = dec(b, p);
    }
    r
}

fn back_in_line(b: &Buffer, p: &mut Pos, big: bool) {
    let sclass = cls(b, *p, big);
    loop {
        if p.col == 0 {
            break;
        }
        dec(b, p);
        if cls(b, *p, big) != sclass {
            inc(b, p);
            break;
        }
    }
}

fn one_left(p: &mut Pos) -> bool {
    if p.col == 0 {
        return false;
    }
    p.col -= 1;
    true
}

/// `iw`/`aw`/`iW`/`aW`.
/// Em falha devolve onde o cursor ficou (o Vim deixa o cursor andado).
pub fn current_word(b: &Buffer, cursor: Pos, count: usize, include: bool, big: bool) -> Result<Obj, Pos> {
    let mut cur = cursor;
    let mut inclusive = true;
    let mut include_white = false;
    back_in_line(b, &mut cur, big);
    let start_pos = cur;
    if (cls(b, cur, big) == 0) == include {
        if !motion::end_word(b, &mut cur, 1, big, true, true) {
            return Err(cur);
        }
    } else {
        motion::fwd_word(b, &mut cur, 1, big, true);
        if cur.col == 0 {
            decl(b, &mut cur);
        } else {
            one_left(&mut cur);
        }
        if include {
            include_white = true;
        }
    }
    let mut start = start_pos;
    let mut count = count as i64 - 1;
    while count > 0 {
        inclusive = true;
        if incl(b, &mut cur) == -1 {
            return Err(cur);
        }
        if include != (cls(b, cur, big) == 0) {
            if !motion::fwd_word(b, &mut cur, 1, big, true) && count > 1 {
                return Err(cur);
            }
            if !one_left(&mut cur) {
                inclusive = false;
            }
        } else if !motion::end_word(b, &mut cur, 1, big, true, true) {
            return Err(cur);
        }
        count -= 1;
    }
    if include_white && (cls(b, cur, big) != 0 || (cur.col == 0 && !inclusive)) {
        // sem espaço depois: inclui o de antes (mas não o recuo da linha)
        let mut p = start_pos;
        if one_left(&mut p) {
            back_in_line(b, &mut p, big);
            if cls(b, p, big) == 0 && p.col > 0 {
                start = p;
            }
        }
    }
    Ok(Obj { start, end: cur, inclusive, linewise: false })
}

fn line_white(b: &Buffer, row: usize) -> bool {
    b.line(row).chars().all(|c| c == ' ' || c == '\t')
}

/// `ip`/`ap`.
pub fn current_par(b: &Buffer, cursor: Pos, count: usize, include: bool) -> Option<Obj> {
    let n = b.line_count();
    let mut start = cursor.row;
    let white_in_front = line_white(b, start);
    while start > 0 {
        if white_in_front {
            if !line_white(b, start - 1) {
                break;
            }
        } else if line_white(b, start - 1) {
            break;
        }
        start -= 1;
    }
    let mut end = start;
    while end < n && line_white(b, end) {
        end += 1;
    }
    // end é "um depois do último branco" (equivale ao --end_lnum do original)
    let mut end = end as i64 - 1;
    let mut i = count as i64;
    if !include && white_in_front {
        i -= 1;
    }
    let last = n as i64 - 1;
    let mut do_white = false;
    while i > 0 {
        i -= 1;
        if end == last {
            return None;
        }
        if !include {
            do_white = line_white(b, (end + 1) as usize);
        }
        if include || !do_white {
            end += 1;
            while end < last && !line_white(b, (end + 1) as usize) {
                end += 1;
            }
        }
        if i == 0 && white_in_front && include {
            break;
        }
        if include || do_white {
            while end < last && line_white(b, (end + 1) as usize) {
                end += 1;
            }
        }
    }
    // sem linhas em branco no fim: tenta pegar as do começo
    if !white_in_front && !line_white(b, end.max(0) as usize) && include {
        while start > 0 && line_white(b, start - 1) {
            start -= 1;
        }
    }
    Some(Obj { start: Pos::new(start, 0), end: Pos::new(end.max(0) as usize, 0), inclusive: false, linewise: true })
}

fn in_indent(b: &Buffer, p: Pos, extra: usize) -> bool {
    let lead = b.line(p.row).chars().take_while(|c| *c == ' ' || *c == '\t').count();
    lead >= p.col + extra
}

/// O `(` (ou `{`...) sem par antes de `from` (exclusive), com aninhamento.
fn find_open(b: &Buffer, from: Pos, open: char, close: char) -> Option<Pos> {
    let mut p = from;
    let mut depth = 0i32;
    loop {
        if dec(b, &mut p) == -1 {
            return None;
        }
        match b.char_at(p) {
            Some(c) if c == close => depth += 1,
            Some(c) if c == open => {
                if depth == 0 {
                    return Some(p);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

/// O próximo `(` depois de `from` (busca para frente, até em outras linhas): o
/// Vim faz isso quando o cursor não está dentro de um bloco.
fn find_next_open(b: &Buffer, from: Pos, open: char, close: char) -> Option<Pos> {
    let mut p = from;
    let mut depth = 0i32;
    loop {
        if inc(b, &mut p) == -1 {
            return None;
        }
        match b.char_at(p) {
            Some(c) if c == close => depth += 1,
            Some(c) if c == open => {
                if depth == 0 {
                    return Some(p);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

/// O `)` que fecha o `(` em `from`, com aninhamento.
fn find_close(b: &Buffer, from: Pos, open: char, close: char) -> Option<Pos> {
    let mut p = from;
    let mut depth = 0i32;
    loop {
        if inc(b, &mut p) == -1 {
            return None;
        }
        match b.char_at(p) {
            Some(c) if c == open => depth += 1,
            Some(c) if c == close => {
                if depth == 0 {
                    return Some(p);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

/// `i( a( i[ a[ i{ a{ i< a<` (`what` é o caractere que abre).
pub fn current_block(b: &Buffer, cursor: Pos, count: usize, include: bool, open: char) -> Option<Obj> {
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => '>',
    };
    let mut cur = cursor;
    if open == '{' {
        // ignora o recuo
        while in_indent(b, cur, 1) {
            if inc(b, &mut cur) != 0 {
                break;
            }
        }
    }
    if b.char_at(cur) == Some(open) {
        cur.col += 1;
    }
    let mut start_pos = cur;
    let mut count = count;
    let mut from = cur;
    if find_open(b, from, open, close).is_some() {
        // dentro de um bloco: sobe `count` níveis
        while count > 0 {
            count -= 1;
            match find_open(b, from, open, close) {
                Some(p) => {
                    from = p;
                    start_pos = p;
                }
                None => return None,
            }
        }
    } else {
        // fora de qualquer bloco: o próximo, para frente
        while count > 0 {
            count -= 1;
            match find_next_open(b, from, open, close) {
                Some(p) => {
                    from = p;
                    start_pos = p;
                }
                None => return None,
            }
        }
    }
    let end_pos = find_close(b, start_pos, open, close)?;
    let mut end = end_pos;
    let mut sol = false;
    if !include {
        incl(b, &mut start_pos);
        sol = end.col == 0;
        decl(b, &mut end);
        while in_indent(b, end, 1) {
            sol = true;
            if decl(b, &mut end) != 0 {
                break;
            }
        }
    }
    let mut inclusive = false;
    if include {
        // inclui o fecha-parêntese
        inclusive = true;
    } else if sol {
        incl(b, &mut end);
    } else if start_pos <= end {
        inclusive = true;
    } else {
        // nada entre os delimitadores
        end = start_pos;
    }
    Some(Obj { start: start_pos, end, inclusive, linewise: false })
}

fn find_next_quote(line: &[char], mut col: usize, quote: char, esc: bool) -> Option<usize> {
    while col < line.len() {
        let c = line[col];
        if esc && c == '\\' {
            col += 1;
            if col >= line.len() {
                return None;
            }
        } else if c == quote {
            return Some(col);
        }
        col += 1;
    }
    None
}

fn find_prev_quote(line: &[char], mut col: usize, quote: char, esc: bool) -> usize {
    while col > 0 {
        col -= 1;
        let mut n = 0;
        if esc {
            while col >= n + 1 && line[col - n - 1] == '\\' {
                n += 1;
            }
        }
        if n & 1 == 1 {
            col -= n;
        } else if line[col] == quote {
            break;
        }
    }
    col
}

/// `i" a" i' a' i` a``.
pub fn current_quote(b: &Buffer, cursor: Pos, count: usize, include: bool, quote: char) -> Option<Obj> {
    let line: Vec<char> = b.line(cursor.row).chars().collect();
    if line.is_empty() {
        return None;
    }
    let first_col = cursor.col.min(line.len() - 1);
    let (mut col_start, col_end);
    if line[first_col] == quote {
        // no meio da linha sem saber se abre ou fecha: conta desde o começo
        let mut cs = 0;
        loop {
            let s = find_next_quote(&line, cs, quote, false)?;
            if s > first_col {
                return None;
            }
            let e = find_next_quote(&line, s + 1, quote, true)?;
            if s <= first_col && first_col <= e {
                col_start = s;
                col_end = e;
                break;
            }
            cs = e + 1;
        }
    } else {
        col_start = find_prev_quote(&line, first_col, quote, true);
        if line[col_start] != quote {
            col_start = find_next_quote(&line, col_start, quote, true)?;
        }
        col_end = find_next_quote(&line, col_start + 1, quote, true)?;
    }
    let mut col_end = col_end;
    if include {
        let white = |i: usize| i < line.len() && (line[i] == ' ' || line[i] == '\t');
        if white(col_end + 1) {
            while white(col_end + 1) {
                col_end += 1;
            }
        } else {
            while col_start > 0 && (line[col_start - 1] == ' ' || line[col_start - 1] == '\t') {
                col_start -= 1;
            }
        }
    }
    if !include && count < 2 {
        col_start += 1;
    }
    let start = Pos::new(cursor.row, col_start);
    let mut end = Pos::new(cursor.row, col_end);
    let mut inclusive = false;
    if include || count > 1 {
        if inc(b, &mut end) == 2 {
            inclusive = true;
        }
    }
    Some(Obj { start, end, inclusive, linewise: false })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn b(lines: &[&str]) -> Buffer {
        Buffer::from_lines(lines)
    }

    #[test]
    fn palavra_e_blocos() {
        let buf = b(&["foo bar baz"]);
        let o = current_word(&buf, Pos::new(0, 5), 1, false, false).unwrap();
        assert_eq!((o.start, o.end, o.inclusive), (Pos::new(0, 4), Pos::new(0, 6), true));
        let o = current_word(&buf, Pos::new(0, 5), 1, true, false).unwrap();
        assert_eq!((o.start, o.end), (Pos::new(0, 4), Pos::new(0, 7)));
        let buf = b(&["f(a, (b), c)"]);
        let o = current_block(&buf, Pos::new(0, 6), 1, false, '(').unwrap();
        assert_eq!((o.start, o.end), (Pos::new(0, 6), Pos::new(0, 6)));
        let o = current_block(&buf, Pos::new(0, 6), 2, false, '(').unwrap();
        assert_eq!((o.start, o.end), (Pos::new(0, 2), Pos::new(0, 10)));
    }

    #[test]
    fn aspas_e_paragrafo() {
        let buf = b(&["say \"hi there\" ok"]);
        let o = current_quote(&buf, Pos::new(0, 8), 1, false, '"').unwrap();
        assert_eq!((o.start, o.end, o.inclusive), (Pos::new(0, 5), Pos::new(0, 13), false));
        let o = current_quote(&buf, Pos::new(0, 8), 1, true, '"').unwrap();
        assert_eq!((o.start.col, o.end.col), (4, 15));
        let buf = b(&["a", "b", "", "c"]);
        let o = current_par(&buf, Pos::new(0, 0), 1, false).unwrap();
        assert_eq!((o.start.row, o.end.row, o.linewise), (0, 1, true));
    }
}
