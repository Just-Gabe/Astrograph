//! Teclas na notação do Vim (`<Esc>`, `<CR>`, `<C-r>`...).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    Ctrl(char),
    Delete,
}

impl Key {
    /// Texto que a tecla escreve no modo de inserção.
    pub fn as_char(self) -> Option<char> {
        match self {
            Key::Char(c) => Some(c),
            Key::Tab => Some('\t'),
            _ => None,
        }
    }
}

/// `"dw<Esc>"` -> teclas. `<lt>` é `<`; `<` sozinho (sem `>` fechando um nome
/// conhecido) é a própria tecla.
pub fn parse_keys(s: &str) -> Vec<Key> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(j) = chars[i..].iter().position(|&c| c == '>') {
                let name: String = chars[i + 1..i + j].iter().collect();
                let low = name.to_lowercase();
                let key = match low.as_str() {
                    "esc" => Some(Key::Esc),
                    "cr" | "enter" | "return" => Some(Key::Enter),
                    "bs" | "backspace" => Some(Key::Backspace),
                    "tab" => Some(Key::Tab),
                    "del" | "delete" => Some(Key::Delete),
                    "lt" => Some(Key::Char('<')),
                    "space" => Some(Key::Char(' ')),
                    n if n.starts_with("c-") && n.chars().count() == 3 => Some(Key::Ctrl(n.chars().nth(2).unwrap())),
                    _ => None,
                };
                if let Some(k) = key {
                    out.push(k);
                    i += j + 1;
                    continue;
                }
            }
        }
        out.push(Key::Char(chars[i]));
        i += 1;
    }
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn notacao() {
        assert_eq!(parse_keys("dw<Esc>"), vec![Key::Char('d'), Key::Char('w'), Key::Esc]);
        assert_eq!(parse_keys("<C-r>u"), vec![Key::Ctrl('r'), Key::Char('u')]);
        assert_eq!(parse_keys("a<lt>b"), vec![Key::Char('a'), Key::Char('<'), Key::Char('b')]);
        assert_eq!(parse_keys("a<b"), vec![Key::Char('a'), Key::Char('<'), Key::Char('b')]);
        assert_eq!(parse_keys("<CR><BS>"), vec![Key::Enter, Key::Backspace]);
    }
}
