//! Textobjects por tree-sitter (`af`, `if`, `]f`...): porte do `shared.lua`
//! do nvim-treesitter-textobjects, com as mesmas consultas
//! (`queries/*/textobjects.scm`).
//!
//! Diferencas do original: usa so a arvore da linguagem do arquivo (o original
//! junta as arvores injetadas) e guarda as capturas de cada casamento num mapa
//! plano por nome (o original monta tabelas aninhadas pelo caminho
//! `function.outer`; so muda algo para uma consulta por prefixo, como
//! `@function` sozinho, que os atalhos do Astrograph nao usam).

use std::collections::HashMap;

use tree_sitter::{QueryCursor, QueryPredicateArg, StreamingIterator};

use crate::Document;

/// Trecho de texto: (linha, coluna, byte) do inicio e do fim (fim exclusivo),
/// como o `Range6` do nvim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range6 {
    pub srow: usize,
    pub scol: usize,
    pub sbyte: usize,
    pub erow: usize,
    pub ecol: usize,
    pub ebyte: usize,
}

impl Range6 {
    fn len(&self) -> isize {
        self.ebyte as isize - self.sbyte as isize
    }
    /// `vim.treesitter._range.contains(self, other)`
    fn contains(&self, o: &Range6) -> bool {
        (self.srow, self.scol) <= (o.srow, o.scol) && (self.erow, self.ecol) >= (o.erow, o.ecol)
    }
    fn contains_point(&self, row: usize, col: usize) -> bool {
        (self.srow, self.scol) <= (row, col) && (self.erow, self.ecol) >= (row, col + 1)
    }
}

/// Capturas de cada casamento da consulta (uma versao da arvore), com a
/// marca de vir da arvore principal (o `find_best_range` do original, usado
/// pelo movimento e pela troca, so olha a principal).
pub(crate) type Matches = Vec<(bool, HashMap<&'static str, Range6>)>;

/// Uma troca de texto: trecho (linha/coluna do inicio e do fim) e as linhas novas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub srow: usize,
    pub scol: usize,
    pub erow: usize,
    pub ecol: usize,
    pub lines: Vec<String>,
}

impl Document {
    /// Byte de (linha, coluna em bytes).
    fn byte_at(&self, row: usize, col: usize) -> usize {
        self.line_byte(row) + col
    }

    fn textobject_matches(&self) -> std::sync::Arc<Matches> {
        let mut slot = self.textobject_cache.lock().unwrap();
        slot.get_or_insert_with(|| std::sync::Arc::new(self.collect_textobjects())).clone()
    }

    fn collect_textobjects(&self) -> Matches {
        let mut out = Vec::new();
        self.collect_tree(self.lang, &self.tree, &self.text, 0, &mut out, 0);
        out
    }

    /// Casamentos de `tree` (arvore de `src`, que comeca no byte `off` do
    /// documento) e das arvores injetadas nela (ex.: o corpo de `tokio::select!`
    /// reparseado como Rust), como o `for_each_tree` do original.
    fn collect_tree(&self, lang: &'static crate::Lang, tree: &tree_sitter::Tree, src: &str, off: usize, out: &mut Matches, depth: usize) {
        if let Some(query) = lang.textobjects.as_ref() {
            self.collect_matches(query, tree, src, off, out, depth == 0);
        }
        if depth >= 2 {
            return;
        }
        for (target, s, e) in crate::injection_regions(lang, tree, src, 0..src.len()) {
            let snippet = &src[s..e];
            let mut parser = tree_sitter::Parser::new();
            if parser.set_language(&target.language).is_err() {
                continue;
            }
            if let Some(t) = parser.parse(snippet, None) {
                self.collect_tree(target, &t, snippet, off + s, out, depth + 1);
            }
        }
    }

    /// (linha, coluna) no documento de um ponto do trecho que comeca no byte `off`.
    fn doc_point(&self, off: usize, p: tree_sitter::Point) -> (usize, usize) {
        let base_row = self.starts.partition_point(|&b| b <= off).saturating_sub(1);
        let base_col = off - self.line_byte(base_row);
        if p.row == 0 { (base_row, base_col + p.column) } else { (base_row + p.row, p.column) }
    }

    fn collect_matches(&self, query: &'static tree_sitter::Query, tree: &tree_sitter::Tree, src: &str, off: usize, out: &mut Matches, host: bool) {
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut it = cursor.matches(query, tree.root_node(), src.as_bytes());
        while let Some(m) = it.next() {
            // `#offset! @cap l0 c0 l1 c1`: o trecho da captura deslocado
            let mut offsets: HashMap<u32, [isize; 4]> = HashMap::new();
            for p in query.general_predicates(m.pattern_index) {
                if &*p.operator != "offset!" {
                    continue;
                }
                let mut cap = None;
                let mut nums = Vec::new();
                for a in p.args.iter() {
                    match a {
                        QueryPredicateArg::Capture(i) => cap = Some(*i),
                        QueryPredicateArg::String(s) => nums.extend(s.parse::<isize>().ok()),
                    }
                }
                if let (Some(c), [a, b, c2, d]) = (cap, nums.as_slice()) {
                    offsets.insert(c, [*a, *b, *c2, *d]);
                }
            }
            let mut prepared: HashMap<&'static str, Range6> = HashMap::new();
            for c in m.captures() {
                let name: &'static str = names[c.index as usize];
                let (srow, scol) = self.doc_point(off, c.node.start_position());
                let (erow, ecol) = self.doc_point(off, c.node.end_position());
                let mut r = Range6 { srow, scol, sbyte: off + c.node.start_byte(), erow, ecol, ebyte: off + c.node.end_byte() };
                // varios nos na mesma captura (quantificador): do 1o ao ultimo
                if let Some(prev) = prepared.get(name) {
                    r.srow = prev.srow;
                    r.scol = prev.scol;
                    r.sbyte = prev.sbyte;
                }
                if let Some([dr0, dc0, dr1, dc1]) = offsets.get(&c.index) {
                    r.srow = (r.srow as isize + dr0).max(0) as usize;
                    r.scol = (r.scol as isize + dc0).max(0) as usize;
                    r.erow = (r.erow as isize + dr1).max(0) as usize;
                    r.ecol = (r.ecol as isize + dc1).max(0) as usize;
                    r.sbyte = self.byte_at(r.srow, r.scol);
                    r.ebyte = self.byte_at(r.erow, r.ecol);
                }
                prepared.insert(name, r);
            }
            out.push((host, prepared));
        }
    }

    fn capture_ranges(&self, matches: &Matches, name: &str) -> Vec<Range6> {
        matches.iter().filter_map(|m| m.1.get(name).copied()).collect()
    }

    /// `find_best_range` do original: so a arvore principal; entre os que
    /// passam no filtro, o de maior nota (no empate, o primeiro).
    fn find_best(&self, name: &str, filter: impl Fn(&Range6) -> bool, score: impl Fn(&Range6) -> isize) -> Option<Range6> {
        let matches = self.textobject_matches();
        let mut best: Option<(Range6, isize)> = None;
        for (host, m) in matches.iter() {
            let Some(r) = m.get(name).filter(|_| *host) else { continue };
            if filter(r) {
                let s = score(r);
                if best.is_none_or(|(_, b)| s > b) {
                    best = Some((*r, s));
                }
            }
        }
        best.map(|b| b.0)
    }

    /// `]f`, `[k`...: para onde o cursor vai depois de `count` saltos. `start`:
    /// `Some(true)` = inicio do objeto, `Some(false)` = fim, `None` = o mais
    /// perto dos dois. Devolve (linha, coluna em bytes) ou `None` se nao ha.
    pub fn textobject_goto(
        &self,
        capture: &str,
        forward: bool,
        start: Option<bool>,
        row: usize,
        col: usize,
        count: usize,
    ) -> Option<(usize, usize)> {
        let name = capture.strip_prefix('@')?;
        let starts: &[bool] = match start {
            None => &[true, false],
            Some(true) => &[true],
            Some(false) => &[false],
        };
        let (mut row, mut col) = (row, col);
        let mut moved = None;
        for _ in 0..count.max(1) {
            let score = |st: bool, r: &Range6| {
                let s = if st { r.sbyte } else { r.ebyte } as isize;
                if forward {
                    -s
                } else {
                    s
                }
            };
            let filter = |st: bool, r: &Range6| {
                let (mut sr, mut sc) = (r.srow as isize, r.scol as isize);
                if !st {
                    if r.ecol == 0 {
                        sr = r.erow as isize - 1;
                        sc = r.ecol as isize;
                    } else {
                        sr = r.erow as isize;
                        sc = r.ecol as isize - 1;
                    }
                }
                let (row, col) = (row as isize, col as isize);
                if forward {
                    sr > row || (sr == row && sc > col)
                } else {
                    sr < row || (sr == row && sc < col)
                }
            };
            let mut best: Option<(Range6, isize, bool)> = None;
            for &st in starts {
                if let Some(r) = self.find_best(name, |r| filter(st, r), |r| score(st, r)) {
                    let s = score(st, &r);
                    if best.is_none_or(|(_, b, _)| s > b) {
                        best = Some((r, s, st));
                    }
                }
            }
            let Some((r, _, st)) = best else { break };
            // o `goto_node` do original
            (row, col) = if st {
                (r.srow, r.scol)
            } else if r.ecol == 0 {
                let er = r.erow.saturating_sub(1);
                (er, self.line(er).len())
            } else {
                (r.erow, r.ecol - 1)
            };
            moved = Some((row, col));
        }
        moved
    }

    fn text_of(&self, r: &Range6) -> &str {
        &self.text[r.sbyte.min(self.text.len())..r.ebyte.min(self.text.len())]
    }

    /// `>A`, `<F`...: troca o objeto no cursor com o vizinho (`direction` > 0 =
    /// proximo). Devolve as edicoes (da ultima para a primeira no texto, para
    /// aplicar em ordem) e o cursor final, como o `swap_nodes` do original.
    pub fn textobject_swap(&self, capture: &str, direction: isize, row: usize, col: usize) -> Option<(Vec<Edit>, (usize, usize))> {
        let name = capture.strip_prefix('@')?;
        let range = self.textobject_at(capture, row, col, false, false)?;
        let adjacent = if direction > 0 {
            let (node_end, search_start) = (range.ebyte, range.ebyte);
            self.find_best(
                name,
                |r| {
                    !(r.srow == range.srow && r.scol == range.scol && r.erow == range.erow && r.ecol == range.ecol)
                        && r.sbyte >= search_start
                        && r.ebyte >= node_end
                },
                |r| -(r.sbyte as isize),
            )
        } else {
            let (node_start, search_end) = (range.sbyte, range.sbyte);
            self.find_best(name, |r| r.ebyte <= search_end && r.sbyte < node_start, |r| r.ebyte as isize)
        }?;
        let (r1, r2) = (range, adjacent);
        let text1: Vec<String> = self.text_of(&r1).split('\n').map(String::from).collect();
        let text2: Vec<String> = self.text_of(&r2).split('\n').map(String::from).collect();
        let e1 = Edit { srow: r1.srow, scol: r1.scol, erow: r1.erow, ecol: r1.ecol, lines: text2.clone() };
        let e2 = Edit { srow: r2.srow, scol: r2.scol, erow: r2.erow, ecol: r2.ecol, lines: text1.clone() };
        // cursor no segundo trecho depois da troca (as contas do original)
        let before = r1.erow < r2.srow || (r1.erow == r2.srow && r1.ecol <= r2.scol);
        let line_delta: isize = if before { text2.len() as isize - text1.len() as isize } else { 0 };
        let mut char_delta: isize = 0;
        if r1.erow == r2.srow && r1.ecol <= r2.scol {
            let last2 = text2.last().map_or(0, |l| l.len()) as isize;
            if line_delta != 0 {
                char_delta = last2 - r1.ecol as isize;
                if r1.srow as isize == r2.srow as isize + line_delta {
                    char_delta += r1.scol as isize;
                }
            } else {
                char_delta = last2 - text1.last().map_or(0, |l| l.len()) as isize;
            }
        }
        let cursor = ((r2.srow as isize + line_delta).max(0) as usize, (r2.scol as isize + char_delta).max(0) as usize);
        let mut edits = vec![e1, e2];
        edits.sort_by(|a, b| (b.srow, b.scol).cmp(&(a.srow, a.scol)));
        Some((edits, cursor))
    }

    /// O textobject `capture` (ex.: `@function.inner`) no ponto (linha, coluna
    /// em bytes, 0-based): o menor que contem o ponto; senao o proximo
    /// (`lookahead`) ou o anterior (`lookbehind`). O `textobject_at_point` do
    /// nvim-treesitter-textobjects.
    pub fn textobject_at(&self, capture: &str, row: usize, col: usize, lookahead: bool, lookbehind: bool) -> Option<Range6> {
        let name = capture.strip_prefix('@')?;
        let matches = self.textobject_matches();
        let ranges = self.capture_ranges(&matches, name);
        if name.ends_with("outer") {
            return best_range_at_point(&ranges, row, col, lookahead, lookbehind);
        }
        // @x.inner: procura dentro do @x.outer que contem o ponto
        let outer_name = match name.find('.') {
            Some(i) => format!("{}.outer", &name[..i]),
            None => format!("{name}.outer"),
        };
        let ranges_outer = self.capture_ranges(&matches, &outer_name);
        if ranges_outer.is_empty() {
            return best_range_at_point(&ranges, row, col, lookahead, lookbehind);
        }
        let Some(outer) = best_range_at_point(&ranges_outer, row, col, false, false) else {
            return best_range_at_point(&ranges, row, col, lookahead, lookbehind);
        };
        let within: Vec<Range6> = ranges.iter().copied().filter(|r| outer.contains(r)).collect();
        if within.is_empty() {
            return best_range_at_point(&ranges, row, col, lookahead, lookbehind);
        }
        best_range_at_point(&within, row, col, lookahead, lookbehind)
            .or_else(|| best_range_at_point(&within, outer.srow, outer.scol, true, false))
    }

    /// Todos os trechos de uma captura (depuracao).
    pub fn textobject_ranges(&self, capture: &str) -> Vec<Range6> {
        let m = self.textobject_matches();
        self.capture_ranges(&m, capture.trim_start_matches('@'))
    }

    /// Nomes de captura da consulta de textobjects (para registrar so os
    /// atalhos que a linguagem tem).
    pub fn has_textobject(&self, capture: &str) -> bool {
        let name = capture.trim_start_matches('@');
        self.lang.textobjects.as_ref().is_some_and(|q| q.capture_names().iter().any(|n| *n == name))
    }
}

/// `best_range_at_point` do original, inclusive no desempate.
fn best_range_at_point(ranges: &[Range6], row: usize, col: usize, lookahead: bool, lookbehind: bool) -> Option<Range6> {
    let mut range_length: Option<isize> = None;
    let mut smallest: Option<Range6> = None;
    let mut earliest_start: Option<usize> = None;
    let mut ahead: Option<(Range6, isize, usize)> = None; // (range, tamanho, inicio)
    let mut behind: Option<(Range6, isize, usize)> = None;

    for r in ranges {
        if r.contains_point(row, col) {
            let length = r.len();
            if range_length.is_none_or(|l| length < l) {
                smallest = Some(*r);
                range_length = Some(length);
            }
            // mesmo tamanho: o que comeca antes
            if range_length.is_some() && smallest.is_some_and(|s| length == s.len()) {
                if earliest_start.is_none_or(|e| r.sbyte < e) {
                    smallest = Some(*r);
                    range_length = Some(length);
                    earliest_start = Some(r.sbyte);
                }
            }
        } else if lookahead {
            if r.srow > row || (r.srow == row && r.scol > col) {
                let length = r.len();
                let better = match ahead {
                    None => true,
                    Some((_, l, s)) => s > r.sbyte || (s == r.sbyte && l < length),
                };
                if better {
                    ahead = Some((*r, length, r.sbyte));
                }
            }
        } else if lookbehind && (r.srow < row || (r.srow == row && r.scol < col)) {
            let length = r.len();
            let better = match behind {
                None => true,
                Some((_, l, s)) => s < r.sbyte || (s == r.sbyte && l > length),
            };
            if better {
                behind = Some((*r, length, r.sbyte));
            }
        }
    }
    smallest.or(ahead.map(|a| a.0)).or(behind.map(|b| b.0))
}

#[cfg(test)]
mod tests {
    use crate::{lang, Document};

    fn doc(name: &str, src: &str) -> Document {
        let lines: Vec<String> = src.split('\n').map(String::from).collect();
        Document::new(lang(name).unwrap(), &lines)
    }

    #[test]
    fn funcao_lua_dentro_e_ao_redor() {
        let d = doc("lua", "local x = 1\nlocal function f(a)\n  return a\nend\n");
        let outer = d.textobject_at("@function.outer", 2, 4, true, false).unwrap();
        assert_eq!((outer.srow, outer.erow), (1, 3));
        let inner = d.textobject_at("@function.inner", 2, 4, true, false).unwrap();
        assert_eq!((inner.srow, inner.scol, inner.erow), (2, 2, 2));
        // fora de funcao, com lookahead: a proxima
        let ahead = d.textobject_at("@function.outer", 0, 0, true, false).unwrap();
        assert_eq!(ahead.srow, 1);
        assert!(d.textobject_at("@function.outer", 0, 0, false, false).is_none());
    }
}
