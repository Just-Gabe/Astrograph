//! Indentacao por tree-sitter: porte do `indent.lua` do nvim-treesitter, com as
//! mesmas consultas (`queries/*/indents.scm`), para a `indentexpr` sair do Lua.
//!
//! Diferenca do original: usa so a arvore da linguagem do arquivo (o original
//! procura a menor arvore injetada que contem a linha). As capturas sao feitas
//! uma vez por versao da arvore, como o `memoize` do original. A paridade e
//! medida linha a linha contra o nvim-treesitter no `indent_e2e`.

use std::collections::HashMap;

use tree_sitter::{Node, Point, Query, QueryCursor, QueryPredicateArg, StreamingIterator};

use crate::Document;

/// Padrao que capturou o no; os `#set!` dele (`indent.immediate`,
/// `indent.open_delimiter`...) sao lidos so quando o algoritmo pergunta, sem
/// copiar nada por captura (a varredura roda a cada versao da arvore).
#[derive(Clone, Copy)]
struct Meta {
    query: &'static Query,
    pattern: usize,
}

impl Meta {
    fn contains_key(&self, key: &str) -> bool {
        self.query.property_settings(self.pattern).iter().any(|p| &*p.key == key)
    }
    fn get(&self, key: &str) -> Option<Option<String>> {
        self.query
            .property_settings(self.pattern)
            .iter()
            .find(|p| &*p.key == key)
            .map(|p| p.value.as_deref().map(String::from))
    }
}

#[derive(Default)]
pub(crate) struct Captures {
    /// (no, tipo em `KINDS`, padrao), ordenado: o algoritmo so consulta a
    /// cadeia de ancestrais (~10 nos), e montar um mapa por captura custava
    /// mais que o proprio cursor do tree-sitter
    by: Vec<(usize, u8, Meta)>,
}

impl Captures {
    fn get(&self, cap: &str, node: &Node) -> Option<&Meta> {
        let k = KINDS.iter().position(|x| *x == cap)? as u8;
        let key = (node.id(), k);
        // a ultima captura do mesmo (no, tipo) vence, como no mapa do original
        let end = self.by.partition_point(|e| (e.0, e.1) <= key);
        (end > 0 && (self.by[end - 1].0, self.by[end - 1].1) == key).then(|| &self.by[end - 1].2)
    }
    fn has(&self, cap: &str, node: &Node) -> bool {
        self.get(cap, node).is_some()
    }
}

const KINDS: [&str; 8] = [
    "indent.auto",
    "indent.begin",
    "indent.end",
    "indent.dedent",
    "indent.branch",
    "indent.ignore",
    "indent.align",
    "indent.zero",
];

fn is_lua_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

impl Document {
    pub(crate) fn line(&self, row: usize) -> &str {
        let s = self.line_byte(row);
        let e = self.line_byte(row + 1);
        self.text[s..e].trim_end_matches('\n')
    }

    /// bytes de espaco no comeco da linha (o `find('^%s*')` do original)
    fn indentcols(&self, row: usize) -> usize {
        self.line(row).bytes().take_while(|&b| is_lua_space(b)).count()
    }

    /// largura da indentacao da linha em colunas (o `indent()` do Vim)
    fn vim_indent(&self, row: usize, ts: usize) -> i64 {
        let mut col = 0usize;
        for b in self.line(row).bytes() {
            match b {
                b' ' => col += 1,
                b'\t' => col = (col / ts.max(1) + 1) * ts.max(1),
                _ => break,
            }
        }
        col as i64
    }

    fn blank(&self, row: usize) -> bool {
        self.line(row).bytes().all(is_lua_space)
    }

    /// Indentacao da linha `lnum` (1 = primeira) em colunas, ou -1 para o nvim
    /// manter a do `autoindent`. `sw` = `shiftwidth()`, `ts` = `&tabstop`.
    pub fn indent(&self, lnum: usize, sw: i64, ts: usize) -> i64 {
        let Some(query) = self.lang.indents.as_ref() else { return -1 };
        if lnum == 0 || lnum > self.line_count() {
            return -1;
        }
        let row = lnum - 1;
        let root = self.tree.root_node();
        // linha nao vazia anterior (o `prevnonblank` do original, 0-based)
        let prev = (0..row).rev().find(|&r| !self.blank(r));
        // capturas so dos nos de primeiro nivel que contem a linha anterior e a
        // pedida (o bloco inteiro: nenhum no do caminho e cortado ao meio, o que
        // mudaria as capturas em no de erro); ASTROGRAPH_INDENT_ARVORE=1 varre a
        // arvore toda, uma vez por versao, como o original
        let range = if std::env::var_os("ASTROGRAPH_INDENT_ARVORE").is_some() {
            0..self.text.len()
        } else {
            self.top_level_range(prev.unwrap_or(row), row)
        };
        // guardado por bloco ate a proxima edicao (varias linhas do mesmo bloco)
        let caps = {
            let mut slot = self.indent_caps.lock().unwrap();
            let memo = slot.get_or_insert_with(Default::default);
            memo.entry((range.start, range.end))
                .or_insert_with(|| std::sync::Arc::new(self.collect(query, Some(range.clone()))))
                .clone()
        };

        let first_node = |r: usize, col: Option<usize>| {
            let col = col.unwrap_or_else(|| self.indentcols(r));
            root.descendant_for_point_range(Point::new(r, col), Point::new(r, col + 1))
        };
        let last_node = |r: usize, col: usize| {
            root.descendant_for_point_range(Point::new(r, col), Point::new(r, col + 1))
        };

        let mut node = if self.blank(row) {
            match prev {
                None => None,
                Some(p) => {
                    let indentcols = self.indentcols(p);
                    let mut prevline = self.line(p).trim_matches(|c: char| c.is_ascii() && is_lua_space(c as u8));
                    let mut n = last_node(p, (indentcols + prevline.len()).saturating_sub(1));
                    if let Some(found) = n.filter(|x| x.kind().contains("comment")) {
                        // comentario no fim da linha anterior: ignora e acha o no antes dele
                        let first = first_node(p, Some(indentcols));
                        let scol = found.start_position().column;
                        if first.is_some_and(|f| f.id() != found.id()) {
                            let cut = scol.saturating_sub(indentcols).min(prevline.len());
                            prevline = prevline[..cut].trim_matches(|c: char| c.is_ascii() && is_lua_space(c as u8));
                            n = last_node(p, (indentcols + prevline.len()).saturating_sub(1));
                        }
                    }
                    if n.is_some_and(|x| caps.has("indent.end", &x)) {
                        n = first_node(row, None);
                    }
                    n
                }
            }
        } else {
            first_node(row, None)
        };

        let mut indent: i64 = 0;
        let mut processed_row: HashMap<usize, bool> = HashMap::new();
        if node.is_some_and(|n| caps.has("indent.zero", &n)) {
            return 0;
        }
        let mut align_promoted: HashMap<usize, Meta> = HashMap::new();
        let target = row;

        let debug = std::env::var("ASTROGRAPH_INDENT_DEBUG").is_ok();
        while let Some(n) = node {
            if debug {
                let kinds: Vec<&str> = KINDS.iter().copied().filter(|k| caps.has(k, &n)).collect();
                eprintln!("indent: {} {:?}-{:?} {kinds:?} (indent={indent})", n.kind(), n.start_position(), n.end_position());
            }
            let begin = caps.get("indent.begin", &n);
            let align_meta = caps.get("indent.align", &n).or_else(|| align_promoted.get(&n.id()));
            let srow = n.start_position().row;
            let erow = n.end_position().row;
            let spans_target = srow < target && target <= erow;

            if begin.is_none() && align_meta.is_none() && caps.has("indent.auto", &n) && spans_target {
                return -1;
            }
            if begin.is_none() && caps.has("indent.ignore", &n) && spans_target {
                return 0;
            }

            let mut is_processed = false;
            let row_done = processed_row.get(&srow).copied().unwrap_or(false);
            if !row_done
                && ((caps.has("indent.branch", &n) && srow == target) || (caps.has("indent.dedent", &n) && srow != target))
            {
                indent -= sw;
                is_processed = true;
            }

            let should_process = !row_done;
            let is_in_err = should_process && n.parent().is_some_and(|p| p.has_error());
            if should_process {
                if let Some(meta) = begin {
                    let immediate = meta.contains_key("indent.immediate");
                    let same_line = meta.contains_key("indent.start_at_same_line");
                    if (srow != erow || is_in_err || immediate) && (srow != target || same_line) {
                        indent += sw;
                        is_processed = true;
                    }
                }
            }

            // no em erro: o alinhamento do primeiro filho vale para o no
            if is_in_err && align_meta.is_none() {
                let mut cur = n.walk();
                for c in n.children(&mut cur) {
                    if let Some(m) = caps.get("indent.align", &c) {
                        align_promoted.insert(n.id(), *m);
                        break;
                    }
                }
            }
            let align_meta = caps.get("indent.align", &n).or_else(|| align_promoted.get(&n.id())).copied();

            if let Some(meta) = align_meta.filter(|_| should_process && (srow != erow || is_in_err) && srow != target) {
                let (o_node, o_last) = match meta.get("indent.open_delimiter").flatten() {
                    Some(d) => self.find_delimiter(&n, &d),
                    None => (Some(n), false),
                };
                let (c_node, c_last) = match meta.get("indent.close_delimiter").flatten() {
                    Some(d) => self.find_delimiter(&n, &d),
                    None => (Some(n), false),
                };
                if let Some(o) = o_node {
                    let o_srow = o.start_position().row;
                    let o_scol = o.start_position().column as i64;
                    let c_srow = c_node.map(|c| c.start_position().row);
                    let mut absolute = false;
                    if o_last {
                        // a linha anterior terminou no delimitador: indent normal
                        indent += sw;
                        if c_last && c_srow.is_some_and(|c| c < target) {
                            indent = (indent - sw).max(0);
                        }
                    } else if c_last && c_srow.is_some_and(|c| o_srow != c && c < target) {
                        indent = (indent - sw).max(0);
                    } else {
                        let incr = meta.get("indent.increment").flatten().and_then(|v| v.parse().ok()).unwrap_or(1);
                        indent = o_scol + incr;
                        absolute = true;
                    }
                    let avoid = c_srow.is_some_and(|c| c != o_srow && c == target)
                        && meta.contains_key("indent.avoid_last_matching_next");
                    if avoid && indent <= self.vim_indent(o_srow, ts) + sw {
                        indent += sw;
                    }
                    is_processed = true;
                    if absolute {
                        return indent;
                    }
                }
            }

            let entry = processed_row.entry(srow).or_insert(false);
            *entry = *entry || is_processed;
            node = n.parent();
        }
        indent
    }

    /// Capturas de indent da arvore inteira (como o original: um no de erro
    /// pode receber captura de um padrao que comeca longe da linha), com os
    /// predicados que so o Neovim tem (`#not-has-parent?`, `#not-kind-eq?`).
    /// Bytes dos filhos da raiz que tocam as linhas `[from, to]`.
    fn top_level_range(&self, from: usize, to: usize) -> std::ops::Range<usize> {
        let root = self.tree.root_node();
        let (mut s, mut e) = (usize::MAX, 0usize);
        let mut cur = root.walk();
        for c in root.children(&mut cur) {
            if c.end_position().row >= from && c.start_position().row <= to {
                s = s.min(c.start_byte());
                e = e.max(c.end_byte());
            }
        }
        if s == usize::MAX {
            // so linhas vazias no fim: o bloco anterior e o que conta
            return self.line_byte(from)..self.text.len();
        }
        s..e.max(self.line_byte(to + 1).min(self.text.len()))
    }

    fn collect(&self, query: &'static Query, range: Option<std::ops::Range<usize>>) -> Captures {
        let mut caps = Captures::default();
        // captura -> tipo (indice em KINDS); padrao -> tem predicado geral
        let kind_of: Vec<Option<usize>> = query
            .capture_names()
            .iter()
            .map(|n| KINDS.iter().position(|k| k == n))
            .collect();
        let general: Vec<bool> = (0..query.pattern_count()).map(|p| !query.general_predicates(p).is_empty()).collect();
        let mut cursor = QueryCursor::new();
        if let Some(r) = range {
            cursor.set_byte_range(r);
        }
        // por capturas, como o nvim-treesitter (`iter_captures`), e nao por
        // casamentos: em no de erro o cursor de capturas devolve capturas que o
        // de casamentos nao devolve (ex.: `(ERROR "try" . ":") @indent.begin`
        // numa docstring aberta), e o indent tem de dar o mesmo numero
        let mut it = cursor.captures(query, self.tree.root_node(), self.text.as_bytes());
        while let Some((m, i)) = it.next() {
            let c = m.captures()[*i];
            let Some(k) = kind_of[c.index as usize] else { continue };
            if general[m.pattern_index] && !general_predicates_ok(query, m) {
                continue;
            }
            caps.by.push((c.node.id(), k as u8, Meta { query, pattern: m.pattern_index }));
        }
        // estavel: entre capturas do mesmo (no, tipo), a ordem de chegada fica
        caps.by.sort_by_key(|e| (e.0, e.1));
        caps
    }

    /// Filho de `node` do tipo `delim` e se ele e o ultimo da linha (so espaco
    /// e o proprio delimitador depois dele).
    fn find_delimiter<'t>(&self, node: &Node<'t>, delim: &str) -> (Option<Node<'t>>, bool) {
        let mut cur = node.walk();
        for child in node.children(&mut cur) {
            if child.kind() == delim {
                let line = self.line(child.start_position().row);
                let after = line.get(child.end_position().column..).unwrap_or("");
                let rest = after.chars().filter(|c| !c.is_whitespace() && !delim.contains(*c)).count();
                return (Some(child), rest == 0);
            }
        }
        (None, false)
    }
}

fn general_predicates_ok(query: &tree_sitter::Query, m: &tree_sitter::QueryMatch) -> bool {
    for p in query.general_predicates(m.pattern_index) {
        let node_of = |a: &QueryPredicateArg| match a {
            QueryPredicateArg::Capture(i) => m.captures().iter().find(|c| c.index == *i).map(|c| c.node),
            QueryPredicateArg::String(_) => None,
        };
        let words: Vec<&str> = p
            .args
            .iter()
            .filter_map(|a| match a {
                QueryPredicateArg::String(s) => Some(&**s),
                QueryPredicateArg::Capture(_) => None,
            })
            .collect();
        let Some(node) = p.args.first().and_then(node_of) else { continue };
        let ok = match &*p.operator {
            "not-has-parent?" => !node.parent().is_some_and(|par| words.contains(&par.kind())),
            "has-parent?" => node.parent().is_some_and(|par| words.contains(&par.kind())),
            "not-kind-eq?" => !words.contains(&node.kind()),
            "kind-eq?" => words.contains(&node.kind()),
            _ => true,
        };
        if !ok {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use crate::{lang, Document};

    fn doc(name: &str, src: &str) -> Document {
        let lines: Vec<String> = src.split('\n').map(String::from).collect();
        Document::new(lang(name).unwrap(), &lines)
    }

    #[test]
    fn lua_dentro_de_funcao_e_end() {
        let d = doc("lua", "local function f()\n  local x = 1\n\nend");
        assert_eq!(d.indent(2, 2, 8), 2);
        // linha vazia dentro do corpo
        assert_eq!(d.indent(3, 2, 8), 2);
        assert_eq!(d.indent(4, 2, 8), 0);
    }

    #[test]
    fn python_bloco_e_tabela() {
        let d = doc("python", "def f(a,\n      b):\n    return [\n        1,\n    ]");
        assert_eq!(d.indent(3, 4, 8), 4);
        assert_eq!(d.indent(4, 4, 8), 8);
    }
}
