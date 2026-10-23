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

/// `<C-w>` numa linha de texto (linha de comando, busca, buscador, prompts): apaga os espaços antes do
/// fim e a palavra (letras, dígitos e `_`) ou o bloco de pontuação antes deles, como o Vim na linha de
/// comando.
pub fn apagar_palavra(s: &mut String) {
    let classe = |c: char| if c.is_whitespace() { 0 } else if c.is_alphanumeric() || c == '_' { 2 } else { 1 };
    while s.ends_with(char::is_whitespace) {
        s.pop();
    }
    if let Some(k) = s.chars().last().map(classe) {
        while s.chars().last().is_some_and(|c| classe(c) == k) {
            s.pop();
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

    #[test]
    fn apagar_palavra_como_o_ctrl_w_da_linha_de_comando() {
        let casos = [("echo uma palavra", "echo uma "), ("abc  ", ""), ("foo.bar", "foo."), ("a.b..", "a.b"), ("x = ", "x "), ("", ""), ("ação", ""), ("main tela", "main ")];
        for (antes, depois) in casos {
            let mut s = antes.to_string();
            apagar_palavra(&mut s);
            assert_eq!(s, depois, "{antes:?}");
        }
    }
}
