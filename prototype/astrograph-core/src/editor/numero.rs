//! `<C-a>` e `<C-x>` no modo normal: soma ou subtrai do número no cursor (ou o primeiro depois
//! dele na linha). Formatos como no `nrformats=bin,hex` do Neovim: decimal (com sinal e zeros à
//! esquerda preservados), `0x` hexadecimal e `0b` binário.

use super::{Editor, Mode, VKind};
use crate::buffer::Pos;

fn digito(c: char) -> bool {
    c.is_ascii_digit()
}

fn hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

fn bin(c: char) -> bool {
    c == '0' || c == '1'
}

/// Onde começa e termina (exclusivo) o número da linha que o cursor alcança, e seu tipo.
pub(super) struct Achado {
    pub ini: usize,
    pub fim: usize,
    pub base: u32,
    pub negativo: bool,
}

/// Procura o número como o `do_addsub` do Vim.
pub(super) fn achar(linha: &[char], cursor: usize) -> Option<Achado> {
    let n = linha.len();
    if n == 0 {
        return None;
    }
    let at = |i: usize| linha.get(i).copied().unwrap_or('\0');
    let mut col = cursor.min(n - 1);
    // dentro de um hexadecimal ou binário (depois do `0x`/`0b`)?
    let mut c = col;
    while c > 0 && bin(at(c)) {
        c -= 1;
    }
    let mut ch = col;
    while ch > 0 && hex(at(ch)) {
        ch -= 1;
    }
    let eh_hex = |c: usize| c > 0 && matches!(at(c), 'x' | 'X') && at(c - 1) == '0' && hex(at(c + 1));
    let eh_bin = |c: usize| c > 0 && matches!(at(c), 'b' | 'B') && at(c - 1) == '0' && bin(at(c + 1));
    let (base, ini) = if eh_hex(ch) {
        (16, ch - 1)
    } else {
        // `0b101` com o cursor num dígito binário; senão recomeça só com dígitos decimais
        let mut d = col;
        while d > 0 && digito(at(d)) {
            d -= 1;
        }
        if eh_bin(c) {
            (2, c - 1)
        } else if eh_hex(d) {
            (16, d - 1)
        } else {
            // procura para a frente o primeiro dígito, depois recua até o começo do número
            while col < n && !digito(at(col)) {
                col += 1;
            }
            if col >= n {
                return None;
            }
            while col > 0 && digito(at(col - 1)) {
                col -= 1;
            }
            // número decimal; um `0x`/`0b` que começa aqui vira hex/bin
            if at(col) == '0' && matches!(at(col + 1), 'x' | 'X') && hex(at(col + 2)) {
                (16, col)
            } else if at(col) == '0' && matches!(at(col + 1), 'b' | 'B') && bin(at(col + 2)) {
                (2, col)
            } else {
                (10, col)
            }
        }
    };
    let (corpo, mut fim) = match base {
        16 | 2 => (ini + 2, ini + 2),
        _ => (ini, ini),
    };
    let ok = |c: char| match base {
        16 => hex(c),
        2 => bin(c),
        _ => digito(c),
    };
    while fim < n && ok(at(fim)) {
        fim += 1;
    }
    if fim == corpo {
        return None;
    }
    // sinal: só em decimal, e só se o `-` vem logo antes do primeiro dígito
    let negativo = base == 10 && ini > 0 && at(ini - 1) == '-';
    Some(Achado { ini: if negativo { ini - 1 } else { ini }, fim, base, negativo })
}

/// O número `a` (texto `texto`) somado de `delta`, no mesmo formato.
fn somar(texto: &str, a: &Achado, delta: i64) -> String {
    let digitos: String = match a.base {
        10 => texto.trim_start_matches('-').to_string(),
        _ => texto[2..].to_string(),
    };
    let valor = u128::from_str_radix(&digitos, a.base).unwrap_or(u128::MAX);
    match a.base {
        10 => {
            let atual = if a.negativo { -(valor as i128) } else { valor as i128 };
            let r = (atual + delta as i128).clamp(-(u64::MAX as i128), u64::MAX as i128);
            let neg = r < 0;
            let mut s = r.unsigned_abs().to_string();
            // zeros à esquerda ficam (o número não muda de largura)
            if digitos.starts_with('0') && digitos.len() > 1 {
                while s.len() < digitos.len() {
                    s.insert(0, '0');
                }
            }
            if neg { format!("-{s}") } else { s }
        }
        16 => {
            let r = (valor as i128 + delta as i128).rem_euclid(1i128 << 64) as u64;
            let maiusculo = digitos.chars().any(|c| c.is_ascii_uppercase());
            let mut s = if maiusculo { format!("{r:X}") } else { format!("{r:x}") };
            while s.len() < digitos.len() {
                s.insert(0, '0');
            }
            format!("{}{}", &texto[..2], s)
        }
        _ => {
            let r = (valor as i128 + delta as i128).rem_euclid(1i128 << 64) as u64;
            let mut s = format!("{r:b}");
            while s.len() < digitos.len() {
                s.insert(0, '0');
            }
            format!("{}{}", &texto[..2], s)
        }
    }
}

impl Editor {
    /// `<C-a>` (`delta` > 0) e `<C-x>` (`delta` < 0), com a contagem já multiplicada.
    pub(super) fn incrementar(&mut self, delta: i64) {
        let row = self.cursor.row;
        let linha: Vec<char> = self.buf.line(row).chars().collect();
        // o Vim salva o desfazer antes de procurar o número: sem número, o `u` ainda desfaz esse passo vazio
        self.checkpoint();
        let Some(a) = achar(&linha, self.cursor.col) else {
            self.failed = true;
            return;
        };
        let texto: String = linha[a.ini..a.fim].iter().collect();
        let novo = somar(&texto, &a, delta);
        self.buf.delete(Pos::new(row, a.ini), Pos::new(row, a.fim));
        self.buf.insert(Pos::new(row, a.ini), &novo);
        // o cursor fica no último caractere do número
        self.cursor = Pos::new(row, a.ini + novo.chars().count() - 1);
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    /// `<C-a>`/`<C-x>` na seleção visual: o primeiro número de cada linha (dentro da seleção); com `g`
    /// a soma cresce a cada linha que tem número (1×, 2×, 3×...).
    pub(super) fn visual_incrementar(&mut self, delta: i64, progressivo: bool) {
        let Some(vs) = self.vis else { return };
        let (a, end, _) = self.visual_range();
        // linha, [c0, c1) de cada linha da seleção
        let mut trechos: Vec<(usize, usize, usize)> = Vec::new();
        match vs.kind {
            VKind::Line => {
                for r in a.row..=end.row {
                    trechos.push((r, 0, self.buf.line_len(r)));
                }
            }
            VKind::Char => {
                let ultima = if end.col == 0 && end.row > a.row { end.row - 1 } else { end.row };
                for r in a.row..=ultima {
                    let len = self.buf.line_len(r);
                    let c0 = if r == a.row { a.col } else { 0 };
                    let c1 = if r == ultima && !(end.col == 0 && end.row > ultima) { end.col.min(len) } else { len };
                    trechos.push((r, c0.min(len), c1.max(c0.min(len))));
                }
            }
            VKind::Block => {
                let Some(b) = self.bloco() else { return };
                for r in b.r0..=b.r1 {
                    let (c0, c1) = self.bloco_trecho(r, &b);
                    trechos.push((r, c0, c1));
                }
            }
        }
        let inicio = if vs.anchor <= self.cursor { vs.anchor } else { self.cursor };
        let inicio = if vs.kind == VKind::Line { Pos::new(inicio.row, 0) } else { inicio };
        self.checkpoint();
        let mut k = 0i64;
        for (row, c0, c1) in trechos {
            let linha: Vec<char> = self.buf.line(row).chars().collect();
            let c1 = c1.min(linha.len());
            if c0 >= c1 {
                continue;
            }
            let Some(a) = achar(&linha[c0..c1], 0) else { continue };
            k += 1;
            let texto: String = linha[c0 + a.ini..c0 + a.fim].iter().collect();
            let d = if progressivo { delta * k } else { delta };
            let novo = somar(&texto, &a, d);
            self.buf.delete(Pos::new(row, c0 + a.ini), Pos::new(row, c0 + a.fim));
            self.buf.insert(Pos::new(row, c0 + a.ini), &novo);
        }
        self.vis = None;
        self.mode = Mode::Normal;
        self.cursor = inicio;
        self.fix_cursor();
        self.curswant = self.want_here();
        if k == 0 {
            self.failed = true;
        }
    }
}
