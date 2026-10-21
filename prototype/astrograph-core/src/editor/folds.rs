//! Folds (dobras) manuais, como no Vim: `zf` cria, `zo zO zc zC za zA zv` abrem e fecham, `zR zM zr zm`
//! mexem no nível, `zd zD zE` apagam, `zj zk` andam entre elas. Uma dobra fechada some do texto: o cursor
//! pula por cima dela e os operadores por linhas levam o trecho inteiro.

use super::*;

/// Uma dobra: linhas `ini..=fim` (0-based). Dobras podem conter outras (aninhadas); não se cruzam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub ini: usize,
    pub fim: usize,
    pub fechada: bool,
}

impl Editor {
    /// As dobras que escondem linhas agora: a mais externa fechada de cada trecho, em ordem e sem se
    /// sobrepor. Cada uma esconde `ini+1..=fim` (a primeira linha fica à mostra).
    pub fn dobras_fechadas(&self) -> Vec<(usize, usize)> {
        if !self.foldenable || self.folds.is_empty() {
            return Vec::new();
        }
        let mut v: Vec<(usize, usize)> = Vec::new();
        // uma dobra de uma linha só não esconde nada (o Vim a ignora)
        let mut fs: Vec<&Fold> = self.folds.iter().filter(|f| f.fechada && f.fim > f.ini).collect();
        fs.sort_by_key(|f| (f.ini, std::cmp::Reverse(f.fim)));
        for f in fs {
            if v.last().is_none_or(|&(_, e)| f.ini > e) {
                v.push((f.ini, f.fim));
            }
        }
        v
    }

    /// A dobra fechada (a mais externa) que contém `row`.
    pub fn fold_fechada_em(&self, row: usize) -> Option<(usize, usize)> {
        if !self.foldenable {
            return None;
        }
        self.folds.iter().filter(|f| f.fechada && f.fim > f.ini && f.ini <= row && row <= f.fim).min_by_key(|f| (f.ini, std::cmp::Reverse(f.fim))).map(|f| (f.ini, f.fim))
    }

    /// Índices das dobras que contêm `row`, da mais externa para a mais interna.
    fn dobras_em(&self, row: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.folds.len()).filter(|&i| self.folds[i].ini <= row && row <= self.folds[i].fim).collect();
        v.sort_by_key(|&i| (self.folds[i].ini, std::cmp::Reverse(self.folds[i].fim)));
        v
    }

    /// Fundo de `n` passos para baixo, contando cada dobra fechada como uma linha só.
    pub(super) fn descer_visiveis(&self, row: usize, n: usize) -> Option<usize> {
        let mut r = row;
        let mut deu = false;
        for _ in 0..n {
            let fim = self.fold_fechada_em(r).map_or(r, |(_, e)| e);
            if fim >= self.last_row() {
                break;
            }
            r = fim + 1;
            deu = true;
        }
        deu.then_some(r)
    }

    /// `n` passos para cima (cada dobra fechada conta uma linha; cai no começo dela).
    pub(super) fn subir_visiveis(&self, row: usize, n: usize) -> Option<usize> {
        let mut r = row;
        let mut deu = false;
        for _ in 0..n {
            let ini = self.fold_fechada_em(r).map_or(r, |(s, _)| s);
            if ini == 0 {
                break;
            }
            r = self.fold_fechada_em(ini - 1).map_or(ini - 1, |(s, _)| s);
            deu = true;
        }
        deu.then_some(r)
    }

    /// Leva as pontas de um trecho por linhas para os limites das dobras fechadas que as contêm.
    pub(super) fn expandir_dobras(&self, a: usize, b: usize) -> (usize, usize) {
        let a2 = self.fold_fechada_em(a).map_or(a, |(s, _)| s);
        let b2 = self.fold_fechada_em(b).map_or(b, |(_, e)| e);
        (a2, b2)
    }

    /// O cursor não pode ficar numa linha escondida: vai para a primeira linha da dobra.
    pub(super) fn cursor_fora_das_dobras(&mut self) {
        if let Some((s, _)) = self.fold_fechada_em(self.cursor.row) {
            if s != self.cursor.row {
                let col = self.col_for_want(s);
                self.cursor = Pos::new(s, col);
                self.fix_cursor();
            }
        }
    }

    /// Abre as dobras que escondem a linha do cursor (`zv`).
    pub(super) fn abrir_dobras_no_cursor(&mut self) {
        let r = self.cursor.row;
        for i in self.dobras_em(r) {
            self.folds[i].fechada = false;
        }
    }

    /// `zf`: cria uma dobra fechada em `a..=b`. Dobras que ficam inteiras dentro dela passam a ser aninhadas; uma
    /// que cruza a borda é incluída (a nova cresce até cobrir).
    pub(super) fn fold_criar(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (a.min(b), a.max(b).min(self.last_row()));
        // dobras que cruzam as bordas: a nova cresce até cobrir (só quando o pai também a contém)
        loop {
            let mut mudou = false;
            for f in &self.folds {
                let cruza_ini = f.ini < a && f.fim >= a && f.fim < b;
                let cruza_fim = f.ini > a && f.ini <= b && f.fim > b;
                if cruza_ini {
                    a = f.ini;
                    mudou = true;
                }
                if cruza_fim {
                    b = f.fim;
                    mudou = true;
                }
            }
            if !mudou {
                break;
            }
        }
        if self.folds.iter().any(|f| f.ini == a && f.fim == b) {
            // já existe: só fecha
            for f in self.folds.iter_mut().filter(|f| f.ini == a && f.fim == b) {
                f.fechada = true;
            }
            return;
        }
        self.folds.push(Fold { ini: a, fim: b, fechada: true });
        self.foldenable = true;
    }

    /// Profundidade (0 = mais externa) de cada dobra.
    fn profundidades(&self) -> Vec<usize> {
        (0..self.folds.len())
            .map(|i| {
                let f = self.folds[i];
                self.folds.iter().enumerate().filter(|&(j, g)| j != i && g.ini <= f.ini && g.fim >= f.fim && (g.ini < f.ini || g.fim > f.fim)).count()
            })
            .collect()
    }

    /// Aplica o `foldlevel`: fecha as dobras de profundidade >= nível, abre as outras.
    fn aplicar_nivel(&mut self) {
        let p = self.profundidades();
        for (i, f) in self.folds.iter_mut().enumerate() {
            f.fechada = p[i] >= self.foldlevel;
        }
    }

    /// `zo`/`zO`/`zc`/`zC` na linha `r`. `abrir`: abre; `recursivo`: nas aninhadas também.
    fn fold_mudar(&mut self, r: usize, abrir: bool, recursivo: bool) -> bool {
        let em = self.dobras_em(r);
        let mut achada: Option<usize> = None;
        let mut mudou = false;
        for &i in &em {
            if !abrir && recursivo {
                if !self.folds[i].fechada {
                    self.folds[i].fechada = true;
                    mudou = true;
                }
                continue;
            }
            if self.folds[i].fechada {
                if abrir {
                    self.folds[i].fechada = false;
                    mudou = true;
                    if recursivo {
                        let (s, e) = (self.folds[i].ini, self.folds[i].fim);
                        for f in self.folds.iter_mut().filter(|f| f.ini >= s && f.fim <= e) {
                            f.fechada = false;
                        }
                    }
                }
                break;
            }
            achada = Some(i);
        }
        if !abrir && !recursivo {
            if let Some(i) = achada {
                self.folds[i].fechada = true;
                mudou = true;
            }
        }
        mudou
    }

    /// Os comandos `z{c}` do modo normal.
    pub(super) fn zcmd(&mut self, c: char, count: usize) {
        let r = self.cursor.row;
        let fechada = self.fold_fechada_em(r).is_some();
        let tem_dobra = !self.dobras_em(r).is_empty();
        match c {
            'o' | 'O' | 'c' | 'C' | 'a' | 'A' | 'v' | 'd' | 'D' if !tem_dobra && !matches!(c, 'v') => {
                // E490: nenhuma dobra encontrada
                self.failed = true;
            }
            'o' => {
                for _ in 0..count {
                    self.fold_mudar(r, true, false);
                }
            }
            'O' => {
                self.fold_mudar(r, true, true);
            }
            'c' => {
                for _ in 0..count {
                    self.fold_mudar(r, false, false);
                }
            }
            'C' => {
                self.fold_mudar(r, false, true);
            }
            'a' => {
                if fechada {
                    self.fold_mudar(r, true, false);
                } else {
                    self.fold_mudar(r, false, false);
                }
            }
            'A' => {
                if fechada {
                    self.fold_mudar(r, true, true);
                } else {
                    self.fold_mudar(r, false, true);
                }
            }
            'v' => {}
            'R' => {
                self.foldlevel = self.profundidades().into_iter().max().map_or(0, |m| m + 1);
                self.aplicar_nivel();
            }
            'M' => {
                self.foldlevel = 0;
                self.aplicar_nivel();
            }
            'r' => {
                let max = self.profundidades().into_iter().max().map_or(0, |m| m + 1);
                self.foldlevel = (self.foldlevel + count).min(max);
                self.aplicar_nivel();
            }
            'm' => {
                self.foldlevel = self.foldlevel.saturating_sub(count);
                self.aplicar_nivel();
            }
            'x' | 'X' => {
                self.aplicar_nivel();
            }
            'd' => {
                // a mais interna (a fechada, se o cursor está numa): as aninhadas sobem um nível
                let em = self.dobras_em(r);
                let alvo = em.iter().copied().find(|&i| self.folds[i].fechada).or_else(|| em.last().copied());
                if let Some(i) = alvo {
                    self.folds.remove(i);
                }
            }
            'D' => {
                let em = self.dobras_em(r);
                let alvo = em.iter().copied().find(|&i| self.folds[i].fechada).or_else(|| em.first().copied());
                if let Some(i) = alvo {
                    let (s, e) = (self.folds[i].ini, self.folds[i].fim);
                    self.folds.retain(|f| !(f.ini >= s && f.fim <= e));
                }
            }
            'E' => self.folds.clear(),
            'n' => self.foldenable = false,
            'N' => self.foldenable = true,
            'i' => self.foldenable = !self.foldenable,
            'j' => {
                // começo da próxima dobra
                let proximo = self.folds.iter().filter(|f| f.ini > r).map(|f| f.ini).min();
                match proximo {
                    Some(p) => {
                        self.cursor = Pos::new(p, self.col_for_want(p));
                        self.fix_cursor();
                    }
                    None => self.failed = true,
                }
            }
            'k' => {
                // fim da dobra anterior
                let anterior = self.folds.iter().filter(|f| f.fim < r).map(|f| f.fim).max();
                match anterior {
                    Some(p) => {
                        self.cursor = Pos::new(p, self.col_for_want(p));
                        self.fix_cursor();
                    }
                    None => self.failed = true,
                }
            }
            _ => {}
        }
        if matches!(c, 'v' | 'x') {
            self.abrir_dobras_no_cursor();
        }
        self.curswant = self.want_here();
    }

    /// Dobras automáticas (`foldmethod=indent` ou `syntax`): troca todas pelas `faixas` `(ini, fim)`. As que já
    /// existiam com a mesma faixa mantêm o estado; as novas fecham se estiverem fundo demais para o `foldlevel`.
    pub fn definir_dobras_auto(&mut self, faixas: &[(usize, usize)]) {
        let antes: std::collections::HashMap<(usize, usize), bool> = self.folds.iter().map(|f| ((f.ini, f.fim), f.fechada)).collect();
        let mut novas: Vec<Fold> = faixas.iter().filter(|(a, b)| b > a).map(|&(ini, fim)| Fold { ini, fim, fechada: false }).collect();
        novas.sort_by_key(|f| (f.ini, std::cmp::Reverse(f.fim)));
        novas.dedup_by_key(|f| (f.ini, f.fim));
        self.folds = novas;
        let prof = self.profundidades();
        let nivel = self.foldlevel;
        for (i, f) in self.folds.iter_mut().enumerate() {
            f.fechada = antes.get(&(f.ini, f.fim)).copied().unwrap_or(prof[i] >= nivel);
        }
    }

    /// `:set foldlevel=N`.
    pub fn set_foldlevel(&mut self, n: usize) {
        self.foldlevel = n;
        self.aplicar_nivel();
    }

    /// As dobras (para o app salvar ou comparar).
    pub fn dobras(&self) -> &[Fold] {
        &self.folds
    }

    /// As linhas mudaram: as dobras acompanham (como as marcas).
    pub(super) fn ajustar_folds(&mut self, old: &Rope) {
        if self.folds.is_empty() {
            return;
        }
        let (n0, n1) = (old.len_lines(), self.buf.rope.len_lines());
        if n0 == n1 {
            return;
        }
        let old_line = |r: usize| -> String {
            let l = old.line(r).to_string();
            l.strip_suffix('\n').unwrap_or(&l).to_string()
        };
        let common = n0.min(n1);
        let f = (0..common).find(|&r| old_line(r) != self.buf.line(r)).unwrap_or(common);
        if n1 < n0 {
            let k = n0 - n1;
            let pure = f >= n1 || old_line(f + k) == self.buf.line(f);
            let mut novas = Vec::with_capacity(self.folds.len());
            for fo in &self.folds {
                let mapa = |row: usize, fim: bool| -> usize {
                    let apagada = if pure { row >= f && row < f + k } else { row > f && row <= f + k };
                    if apagada {
                        if fim { f.saturating_sub(if pure { 1 } else { 0 }) } else if pure { f } else { f }
                    } else if row >= f + k {
                        row - k
                    } else {
                        row
                    }
                };
                let (ini, fim) = (mapa(fo.ini, false), mapa(fo.fim, true));
                if ini <= fim && fim < n1 {
                    novas.push(Fold { ini, fim, fechada: fo.fechada });
                }
            }
            self.folds = novas;
        } else {
            let n = n1 - n0;
            let pure = f < n0 && old_line(f) == self.buf.line(f + n);
            for fo in self.folds.iter_mut() {
                let desloca = |row: usize| if (pure && row >= f) || (!pure && row > f) { row + n } else { row };
                fo.ini = desloca(fo.ini);
                fo.fim = desloca(fo.fim);
            }
        }
        // uma dobra de uma linha só não esconde nada, mas continua existindo (o Vim as mantém)
    }
}
