//! Seleção visual em bloco (`<C-v>`): `d y c r ~ u U I A o O $` sobre um retângulo
//! de colunas visuais, como o Vim.

use super::*;

/// Inserção a replicar nas outras linhas depois do `<Esc>`.
#[derive(Debug, Clone)]
pub(super) struct BlocoIns {
    /// linhas que recebem a cópia (a primeira linha já recebeu a digitação)
    pub linhas: std::ops::RangeInclusive<usize>,
    /// coluna visual onde o texto entra
    pub vcol: usize,
    /// `A`: linhas curtas são completadas com espaços; senão (`I`, `c`) são puladas
    pub preencher: bool,
    /// `$A`: o texto entra no fim de cada linha
    pub fim_da_linha: bool,
    /// posição inicial do texto na primeira linha
    pub inicio: Pos,
    /// coluna visual da quina de cima à esquerda do bloco
    pub esquerda: usize,
}

struct Bloco {
    r0: usize,
    r1: usize,
    /// primeira e última coluna visual (inclusive)
    l: usize,
    r: usize,
    ate_o_fim: bool,
}

impl Editor {
    fn bloco(&self) -> Option<Bloco> {
        let vs = self.vis?;
        let (a, c) = (vs.anchor, self.cursor);
        let (la, lc) = (self.buf.line(a.row), self.buf.line(c.row));
        let (sa, ea) = (motion::vcol_start(&la, a.col), motion::vcol_end(&la, a.col));
        let (sc, ec) = (motion::vcol_start(&lc, c.col), motion::vcol_end(&lc, c.col));
        Some(Bloco { r0: a.row.min(c.row), r1: a.row.max(c.row), l: sa.min(sc), r: ea.max(ec), ate_o_fim: self.curswant == MAXWANT })
    }

    /// Caracteres da linha `row` que o bloco cobre: `[c0, c1)`.
    fn bloco_trecho(&self, row: usize, b: &Bloco) -> (usize, usize) {
        let linha = self.buf.line(row);
        let mut v = 0;
        let (mut c0, mut c1) = (None, 0);
        for (i, ch) in linha.chars().enumerate() {
            let w = motion::char_width(ch, v);
            let (vs, ve) = (v, v + w - 1);
            if vs <= b.r && ve >= b.l {
                if c0.is_none() {
                    c0 = Some(i);
                }
                c1 = i + 1;
            }
            v += w;
        }
        let n = linha.chars().count();
        match c0 {
            Some(c0) => (c0, if b.ate_o_fim { n } else { c1 }),
            None => {
                // a linha acaba antes do bloco (ou só tem espaço "de fora")
                if b.ate_o_fim && v > b.l { (n, n) } else { (n, n) }
            }
        }
    }

    /// Coluna (em caracteres) onde cai a coluna visual `vcol`; `None` se a linha é mais curta.
    fn col_na_vcol(&self, row: usize, vcol: usize) -> Option<usize> {
        let linha = self.buf.line(row);
        let mut v = 0;
        for (i, ch) in linha.chars().enumerate() {
            if v >= vcol {
                return Some(i);
            }
            v += motion::char_width(ch, v);
        }
        (v >= vcol).then(|| linha.chars().count())
    }

    fn bloco_sair(&mut self) {
        self.vis = None;
        self.mode = Mode::Normal;
    }

    pub(super) fn bloco_op(&mut self, op: char, count: Option<usize>) {
        let Some(b) = self.bloco() else { return };
        let _ = count;
        // `$` no bloco: os trechos vão até o fim de cada linha
        let trechos: Vec<(usize, usize)> = (b.r0..=b.r1).map(|r| self.bloco_trecho(r, &b)).collect();
        let texto: Vec<String> = (b.r0..=b.r1).zip(&trechos).map(|(r, &(c0, c1))| self.buf.line(r).chars().skip(c0).take(c1 - c0).collect()).collect();
        let topo = Pos::new(b.r0, trechos[0].0);
        // largura do registro: o retângulo todo, ou (com `$`) a linha mais comprida do trecho
        let larg = if b.ate_o_fim { texto.iter().map(|t| motion::vcol_start(t, t.chars().count())).max().unwrap_or(0).max(1) } else { b.r - b.l + 1 };
        let topo_col = |s: &Self| s.col_na_vcol(b.r0, b.l).unwrap_or_else(|| s.buf.line_len(b.r0));
        self.visual_op = true;
        match op {
            'y' => {
                self.bloco_sair();
                self.store_bwidth = larg;
                self.store(None, texto.join("\n"), false, false);
                self.cursor = Pos::new(b.r0, trechos[0].0.min(topo_col(self)));
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            'd' | 'x' | 'X' | 'D' => {
                // X e D apagam as linhas inteiras do bloco
                if op == 'X' || op == 'D' {
                    self.bloco_sair();
                    self.ex_apagar_linhas_visual(b.r0, b.r1);
                    return;
                }
                self.bloco_sair();
                self.checkpoint();
                self.store_bwidth = larg;
                self.store(None, texto.join("\n"), false, true);
                for (k, r) in (b.r0..=b.r1).enumerate() {
                    let (c0, c1) = trechos[k];
                    if c1 > c0 {
                        self.buf.delete(Pos::new(r, c0), Pos::new(r, c1));
                    }
                }
                self.cursor = Pos::new(b.r0, trechos[0].0.min(self.buf.line_len(b.r0)));
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            'c' | 's' => {
                self.bloco_sair();
                self.checkpoint();
                self.store_bwidth = larg;
                self.store(None, texto.join("\n"), false, true);
                for (k, r) in (b.r0..=b.r1).enumerate() {
                    let (c0, c1) = trechos[k];
                    if c1 > c0 {
                        self.buf.delete(Pos::new(r, c0), Pos::new(r, c1));
                    }
                }
                self.cursor = Pos::new(b.r0, trechos[0].0.min(self.buf.line_len(b.r0)));
                self.begin_insert_state('c', 1, None);
                if b.r1 > b.r0 {
                    self.block_ins = Some(BlocoIns { linhas: b.r0 + 1..=b.r1, vcol: b.l, preencher: false, fim_da_linha: false, inicio: self.cursor, esquerda: b.l });
                }
            }
            'C' | 'S' | 'R' => {
                // linhas inteiras, como no visual por linhas
                if let Some(vs) = self.vis.as_mut() {
                    vs.kind = VKind::Line;
                }
                self.visual_operate(op, count);
            }
            '~' | 'u' | 'U' => {
                self.bloco_sair();
                self.checkpoint();
                for (k, r) in (b.r0..=b.r1).enumerate() {
                    let (c0, c1) = trechos[k];
                    if c1 <= c0 {
                        continue;
                    }
                    let antigo: String = self.buf.line(r).chars().skip(c0).take(c1 - c0).collect();
                    let novo: String = antigo
                        .chars()
                        .map(|c| match op {
                            'u' => c.to_lowercase().next().unwrap_or(c),
                            'U' => c.to_uppercase().next().unwrap_or(c),
                            _ => {
                                if c.is_lowercase() {
                                    c.to_uppercase().next().unwrap_or(c)
                                } else if c.is_uppercase() {
                                    c.to_lowercase().next().unwrap_or(c)
                                } else {
                                    c
                                }
                            }
                        })
                        .collect();
                    self.buf.delete(Pos::new(r, c0), Pos::new(r, c1));
                    self.buf.insert(Pos::new(r, c0), &novo);
                }
                self.cursor = Pos::new(b.r0, trechos[0].0.min(topo_col(self)));
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            _ => {
                self.failed = true;
            }
        }
        let _ = topo;
    }

    fn ex_apagar_linhas_visual(&mut self, a: usize, b: usize) {
        self.cursor = Pos::new(a, 0);
        self.apply_operator('d', Pos::new(a, 0), Pos::new(b, 0), Kind::Line, None);
    }

    pub(super) fn bloco_replace(&mut self, ch: char) {
        let Some(b) = self.bloco() else { return };
        self.bloco_sair();
        self.checkpoint();
        let mut primeiro = None;
        for r in b.r0..=b.r1 {
            let (c0, c1) = self.bloco_trecho(r, &b);
            if c1 > c0 {
                let novo: String = std::iter::repeat(ch).take(c1 - c0).collect();
                self.buf.delete(Pos::new(r, c0), Pos::new(r, c1));
                self.buf.insert(Pos::new(r, c0), &novo);
                primeiro.get_or_insert(c0);
            }
        }
        let c = primeiro.unwrap_or(0);
        self.cursor = Pos::new(b.r0, c.min(self.buf.line_len(b.r0)));
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    /// `I` / `A` no bloco: entra no modo de inserção na primeira linha; o texto vai às outras no `<Esc>`.
    pub(super) fn bloco_inserir(&mut self, anexar: bool) {
        let Some(b) = self.bloco() else { return };
        self.bloco_sair();
        self.checkpoint();
        let fim = anexar && b.ate_o_fim;
        let vcol = if anexar { b.r + 1 } else { b.l };
        let linha = self.buf.line(b.r0);
        let col = if fim {
            linha.chars().count()
        } else {
            match self.col_na_vcol(b.r0, vcol) {
                Some(c) => c,
                None => {
                    // `A` numa linha curta: completa com espaços até a coluna
                    let atual = motion::vcol_start(&linha, linha.chars().count());
                    let falta = vcol.saturating_sub(atual);
                    let n = linha.chars().count();
                    if anexar && falta > 0 {
                        self.buf.insert(Pos::new(b.r0, n), &" ".repeat(falta));
                    }
                    self.buf.line_len(b.r0)
                }
            }
        };
        self.cursor = Pos::new(b.r0, col);
        self.begin_insert_state(if anexar { 'A' } else { 'I' }, 1, None);
        if b.r1 > b.r0 || anexar {
            self.block_ins = Some(BlocoIns { linhas: b.r0 + 1..=b.r1, vcol, preencher: anexar, fim_da_linha: fim, inicio: self.cursor, esquerda: b.l });
        }
    }

    /// Depois do `<Esc>`: copia o que foi digitado para as outras linhas do bloco.
    pub(super) fn bloco_replicar(&mut self, bi: BlocoIns) {
        let mut texto = String::new();
        let mut valido = true;
        for k in &self.ins.typed {
            match k {
                Key::Char(c) => texto.push(*c),
                Key::Tab => texto.push('\t'),
                Key::Backspace => {
                    texto.pop();
                }
                // quebra de linha ou outra coisa: o Vim não replica
                _ => valido = false,
            }
        }
        let topo = self.cursor;
        if valido && !texto.is_empty() {
            for r in bi.linhas.clone() {
                if r > self.last_row() {
                    break;
                }
                let linha = self.buf.line(r);
                let n = linha.chars().count();
                let col = if bi.fim_da_linha {
                    Some(n)
                } else {
                    match self.col_na_vcol(r, bi.vcol) {
                        Some(c) => Some(c),
                        None if bi.preencher => {
                            let atual = motion::vcol_start(&linha, n);
                            self.buf.insert(Pos::new(r, n), &" ".repeat(bi.vcol.saturating_sub(atual)));
                            Some(self.buf.line_len(r))
                        }
                        None => None,
                    }
                };
                if let Some(c) = col {
                    self.buf.insert(Pos::new(r, c), &texto);
                }
            }
        }
        // `A`: o cursor volta à quina de cima à esquerda do bloco; `I` e `c` ficam no fim do texto
        if bi.preencher || bi.fim_da_linha {
            let alvo = self.col_na_vcol(topo.row, bi.esquerda).unwrap_or_else(|| self.buf.line_len(topo.row));
            self.cursor = Pos::new(topo.row, alvo.min(self.buf.line_len(topo.row)));
            self.esc_sem_recuo = true;
        }
    }

    /// `p` / `P` de um registro em bloco: cada linha do registro entra numa linha do buffer, na mesma coluna.
    pub(super) fn put_bloco(&mut self, r: &Reg, after: bool, count: usize) {
        let linha0 = self.buf.line(self.cursor.row);
        let c = self.cursor.col;
        let vcol = if after && !linha0.is_empty() { motion::vcol_end(&linha0, c) + 1 } else { motion::vcol_start(&linha0, c) };
        let pedacos: Vec<&str> = r.text.split('\n').collect();
        let largura = r.bwidth.max(1);
        let r0 = self.cursor.row;
        for (k, pedaco) in pedacos.iter().enumerate() {
            let row = r0 + k;
            if row > self.last_row() {
                let n = self.buf.line_count();
                self.buf.insert_line(n, "");
            }
            let linha = self.buf.line(row);
            let n = linha.chars().count();
            let atual = motion::vcol_start(&linha, n);
            if atual < vcol {
                self.buf.insert(Pos::new(row, n), &" ".repeat(vcol - atual));
            }
            let col = self.col_na_vcol(row, vcol).unwrap_or_else(|| self.buf.line_len(row));
            let tem_depois = col < self.buf.line_len(row);
            let largura_pedaco = motion::vcol_start(pedaco, pedaco.chars().count());
            let mut texto = String::new();
            for i in 0..count {
                texto.push_str(pedaco);
                // preenche até a largura do bloco, exceto no fim da linha
                if tem_depois || i + 1 < count {
                    texto.push_str(&" ".repeat(largura.saturating_sub(largura_pedaco)));
                }
            }
            self.buf.insert(Pos::new(row, col), &texto);
        }
        let col = self.col_na_vcol(r0, vcol).unwrap_or(0);
        self.cursor = Pos::new(r0, col);
        self.fix_cursor();
        self.curswant = self.want_here();
    }
}
