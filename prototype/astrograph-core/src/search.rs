//! Busca: tradução da sintaxe de regex do Vim para a crate `regex` e a procura
//! (para frente e para trás, com `wrapscan`).

use regex::Regex;

use crate::buffer::{Buffer, Pos};

/// Traduz um padrão do Vim (magic por padrão; `\v` muito mágico) para `regex`.
/// Devolve o padrão e se é para ignorar maiúsculas (`\c`).
pub fn vim_to_regex(pat: &str) -> (String, bool) {
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut very_magic = false;
    let mut ignore_case = false;
    let mut i = 0;
    let lit = |c: char| -> String { regex::escape(&c.to_string()) };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            i += 2;
            match n {
                'v' => very_magic = true,
                'm' => very_magic = false,
                'c' => ignore_case = true,
                'C' => ignore_case = false,
                '<' | '>' => out.push_str("\\b"),
                '(' | ')' | '|' | '{' | '}' | '+' | '?' if !very_magic => out.push(n),
                '=' if !very_magic => out.push('?'),
                '(' | ')' | '|' | '{' | '}' | '+' | '?' | '=' | '@' => out.push_str(&lit(n)),
                'n' => out.push_str("\\n"),
                't' => out.push_str("\\t"),
                's' => out.push_str("[ \\t]"),
                'S' => out.push_str("[^ \\t]"),
                'd' => out.push_str("[0-9]"),
                'D' => out.push_str("[^0-9]"),
                'w' => out.push_str("[0-9A-Za-z_]"),
                'W' => out.push_str("[^0-9A-Za-z_]"),
                'a' => out.push_str("[A-Za-z]"),
                'l' => out.push_str("[a-z]"),
                'u' => out.push_str("[A-Z]"),
                'x' => out.push_str("[0-9A-Fa-f]"),
                '.' | '*' | '[' | ']' | '^' | '$' | '/' | '\\' | '~' => out.push_str(&lit(n)),
                other => out.push_str(&lit(other)),
            }
            continue;
        }
        i += 1;
        if very_magic {
            match c {
                '<' | '>' => out.push_str("\\b"),
                '=' => out.push('?'),
                '(' | ')' | '|' | '{' | '}' | '+' | '?' | '*' | '.' | '[' | ']' | '^' | '$' => out.push(c),
                _ => out.push_str(&lit(c)),
            }
        } else {
            match c {
                '*' | '.' | '[' | ']' | '^' | '$' => out.push(c),
                // sem barra, estes são literais no modo magic
                '(' | ')' | '|' | '{' | '}' | '+' | '?' => out.push_str(&lit(c)),
                _ => out.push_str(&lit(c)),
            }
        }
    }
    (out, ignore_case)
}

/// `compile` com as opções `ignorecase` e `smartcase` (maiúscula na busca liga a diferença de caixa).
pub fn compile_opts(pat: &str, ignorecase: bool, smartcase: bool) -> Option<Regex> {
    let (p, ic) = vim_to_regex(pat);
    let ic = ic || (ignorecase && !(smartcase && pat.chars().any(char::is_uppercase)));
    let p = if ic { format!("(?i){p}") } else { p };
    Regex::new(&p).ok()
}

pub fn compile(pat: &str) -> Option<Regex> {
    let (p, ic) = vim_to_regex(pat);
    let p = if ic { format!("(?i){p}") } else { p };
    Regex::new(&p).ok()
}

fn char_col(line: &str, byte: usize) -> usize {
    line[..byte].chars().count()
}

fn byte_of(line: &str, col: usize) -> usize {
    line.char_indices().nth(col).map(|(b, _)| b).unwrap_or(line.len())
}

/// Próxima ocorrência a partir de `from` (exclusive), com `wrapscan`.
/// `(posição, deu a volta)`.
pub fn find(b: &Buffer, re: &Regex, from: Pos, forward: bool) -> Option<(Pos, bool)> {
    let n = b.line_count();
    if forward {
        // resto da linha atual
        let line = b.line(from.row);
        let start = byte_of(&line, from.col + 1).min(line.len());
        if from.col < line.chars().count() {
            if let Some(m) = re.find_at(&line, start) {
                return Some((Pos::new(from.row, char_col(&line, m.start())), false));
            }
        }
        for k in 1..=n {
            let row = (from.row + k) % n;
            let wrapped = from.row + k >= n;
            let line = b.line(row);
            if row == from.row {
                // volta à linha de origem: vale tudo até a coluna do cursor
                if let Some(m) = re.find(&line) {
                    let c = char_col(&line, m.start());
                    if c <= from.col {
                        return Some((Pos::new(row, c), true));
                    }
                }
                return None;
            }
            if let Some(m) = re.find(&line) {
                return Some((Pos::new(row, char_col(&line, m.start())), wrapped));
            }
        }
        None
    } else {
        let last_before = |line: &str, limit_col: Option<usize>| -> Option<usize> {
            let mut found = None;
            let mut at = 0;
            while at <= line.len() {
                let Some(m) = re.find_at(line, at) else { break };
                let c = char_col(line, m.start());
                if let Some(l) = limit_col {
                    if c >= l {
                        break;
                    }
                }
                found = Some(c);
                // todas as posições de início, mesmo sobrepostas (como a busca para trás do Vim)
                at = m.start() + line[m.start()..].chars().next().map(char::len_utf8).unwrap_or(1);
            }
            found
        };
        let line = b.line(from.row);
        if let Some(c) = last_before(&line, Some(from.col)) {
            return Some((Pos::new(from.row, c), false));
        }
        for k in 1..=n {
            let row = (from.row + n - k % n) % n;
            let wrapped = k > from.row;
            let line = b.line(row);
            if row == from.row {
                return last_before(&line, None).filter(|c| *c >= from.col).map(|c| (Pos::new(row, c), true));
            }
            if let Some(c) = last_before(&line, None) {
                return Some((Pos::new(row, c), wrapped));
            }
        }
        None
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn traducao() {
        assert_eq!(vim_to_regex("a.b").0, "a.b");
        assert_eq!(vim_to_regex("\\<foo\\>").0, "\\bfoo\\b");
        assert_eq!(vim_to_regex("a\\|b").0, "a|b");
        assert_eq!(vim_to_regex("(a)").0, "\\(a\\)");
        assert_eq!(vim_to_regex("\\v(a|b)+").0, "(a|b)+");
        assert_eq!(vim_to_regex("a\\+").0, "a+");
        assert!(vim_to_regex("\\cfoo").1);
    }

    #[test]
    fn procura_para_os_dois_lados_com_volta() {
        let b = Buffer::from_lines(&["foo bar foo", "baz", "foo"]);
        let re = compile("foo").unwrap();
        assert_eq!(find(&b, &re, Pos::new(0, 0), true), Some((Pos::new(0, 8), false)));
        assert_eq!(find(&b, &re, Pos::new(0, 8), true), Some((Pos::new(2, 0), false)));
        assert_eq!(find(&b, &re, Pos::new(2, 0), true), Some((Pos::new(0, 0), true)));
        assert_eq!(find(&b, &re, Pos::new(0, 0), false), Some((Pos::new(2, 0), true)));
        assert_eq!(find(&b, &re, Pos::new(2, 0), false), Some((Pos::new(0, 8), false)));
        assert_eq!(find(&b, &re, Pos::new(0, 8), false), Some((Pos::new(0, 0), false)));
    }
}
