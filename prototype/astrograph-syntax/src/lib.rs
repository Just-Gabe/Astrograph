//! Tipo de arquivo e realce por tree-sitter, feitos no processo do Astrograph.
//!
//! Substitui o que o Neovim faz em Lua/Vimscript no runtime (`filetype.lua`,
//! `syntax/*.vim`, `vim.treesitter`): as gramaticas vem compiladas no
//! executavel e o parse roda fora do loop do nvim. O editor so recebe o
//! resultado, em lote, como extmarks (ver `ROADMAP.md`, S1).
//!
//! O documento espelha o buffer pelos eventos de linha do `nvim_buf_attach`
//! (`nvim_buf_lines_event`: troca as linhas `[first, last)` por `new`).

mod detect;
mod indent;
mod langs;
mod textobjects;

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::{InputEdit, Parser, Point, QueryCursor, StreamingIterator, Tree};

pub use detect::detect;
pub use langs::{lang, Lang, INJECTED_NAMES, LANG_NAMES};
pub use textobjects::{Edit, Range6};

/// Trecho de uma linha com um grupo de realce. Colunas em bytes, como as do
/// `nvim_buf_set_extmark`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub group: &'static str,
}

/// Copia do buffer com a arvore de sintaxe mantida em dia por edicoes.
pub struct Document {
    lang: &'static Lang,
    parser: Parser,
    tree: Tree,
    text: String,
    /// byte de inicio de cada linha em `text` (cada linha termina em '\n')
    starts: Vec<usize>,
    /// capturas de indent da arvore atual (refeitas a cada edicao, como o
    /// `memoize` por versao da arvore do nvim-treesitter)
    indent_caps: std::sync::Mutex<Option<HashMap<(usize, usize), std::sync::Arc<indent::Captures>>>>,
    /// casamentos de textobjects da arvore atual (refeitos a cada edicao)
    textobject_cache: std::sync::Mutex<Option<std::sync::Arc<textobjects::Matches>>>,
}

impl Document {
    pub fn new(lang: &'static Lang, lines: &[String]) -> Self {
        let mut parser = Parser::new();
        parser.set_language(&lang.language).expect("versao da gramatica compativel");
        let mut text = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
        for l in lines {
            text.push_str(l);
            text.push('\n');
        }
        let tree = parser.parse(&text, None).expect("parse sem tempo-limite");
        let starts = line_starts(&text);
        Self { lang, parser, tree, text, starts, indent_caps: Default::default(), textobject_cache: Default::default() }
    }

    pub fn lang(&self) -> &'static Lang {
        self.lang
    }

    /// A arvore em s-expression (para comparar com a do nvim ao depurar).
    pub fn sexpr(&self) -> String {
        self.tree.root_node().to_sexp()
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    fn line_byte(&self, row: usize) -> usize {
        self.starts.get(row).copied().unwrap_or(self.text.len())
    }

    /// Troca as linhas `[first, last)` por `new` (o evento de linha do nvim;
    /// `last` pode ser -1 = ate o fim). Devolve as linhas cujo realce pode ter
    /// mudado: as editadas e as que o tree-sitter diz que mudaram de sintaxe
    /// (ex.: abrir um comentario de bloco muda o resto do arquivo).
    pub fn apply_lines(&mut self, first: usize, last: Option<usize>, new: &[String]) -> Vec<Range<usize>> {
        let n = self.line_count();
        let first = first.min(n);
        let last = last.unwrap_or(n).clamp(first, n);
        let start = self.line_byte(first);
        let old_end = self.line_byte(last);
        let mut insert = String::with_capacity(new.iter().map(|l| l.len() + 1).sum());
        for l in new {
            insert.push_str(l);
            insert.push('\n');
        }
        self.text.replace_range(start..old_end, &insert);
        let edit = InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: start + insert.len(),
            start_position: Point::new(first, 0),
            old_end_position: Point::new(last, 0),
            new_end_position: Point::new(first + new.len(), 0),
        };
        let mut old = self.tree.clone();
        old.edit(&edit);
        let tree = self.parser.parse(&self.text, Some(&old)).expect("parse sem tempo-limite");
        // indice de linhas: as novas no lugar das trocadas; as de depois so
        // deslocam pelo tamanho da edicao (sem reler o texto)
        let delta = insert.len() as isize - (old_end - start) as isize;
        let tail: Vec<usize> = self.starts[last..].iter().map(|&s| (s as isize + delta) as usize).collect();
        self.starts.truncate(first);
        let mut b = start;
        for l in new {
            self.starts.push(b);
            b += l.len() + 1;
        }
        self.starts.extend(tail);

        let mut ranges = vec![first..first + new.len().max(1)];
        for r in old.changed_ranges(&tree) {
            ranges.push(r.start_point.row..r.end_point.row + 1);
        }
        self.tree = tree;
        *self.indent_caps.lock().unwrap() = None;
        *self.textobject_cache.lock().unwrap() = None;
        merge(ranges, self.line_count())
    }

    /// Realce das linhas `rows`, sem sobreposicao: em cada byte vale a maior
    /// prioridade (`#set! priority`), depois a captura mais interna e, no mesmo
    /// trecho, o padrao que a convencao da consulta manda (`Lang::later_wins`).
    pub fn highlights(&self, rows: Range<usize>) -> Vec<Span> {
        let rows = rows.start.min(self.line_count())..rows.end.min(self.line_count());
        if rows.is_empty() {
            return Vec::new();
        }
        let base = self.line_byte(rows.start);
        let end = self.line_byte(rows.end);
        let mut paint: Vec<Option<&'static str>> = vec![None; end - base];
        paint_lang(self.lang, &self.tree, &self.text, 0, base..end, &mut paint, 0);
        let mut spans = Vec::new();
        for row in rows {
            let ls = self.line_byte(row) - base;
            let le = self.line_byte(row + 1).saturating_sub(1).max(self.line_byte(row)) - base;
            let mut col = ls;
            while col < le {
                let g = paint[col];
                let mut stop = col + 1;
                while stop < le && paint[stop] == g {
                    stop += 1;
                }
                if let Some(group) = g {
                    spans.push(Span { row, start_col: col - ls, end_col: stop - ls, group });
                }
                col = stop;
            }
        }
        spans
    }

    #[cfg(test)]
    fn text(&self) -> &str {
        &self.text
    }
}

/// Pinta em `paint` (bytes `range` do documento) as capturas de `lang` sobre
/// `tree`, que e a arvore de `src`; `src` comeca no byte `off` do documento.
/// Depois pinta por cima as injecoes (ate 2 niveis: lua dentro de lua...).
fn paint_lang(
    lang: &'static Lang,
    tree: &Tree,
    src: &str,
    off: usize,
    range: Range<usize>,
    paint: &mut [Option<&'static str>],
    depth: usize,
) {
    let local = range.start.saturating_sub(off)..range.end.saturating_sub(off).min(src.len());
    if local.is_empty() {
        return;
    }
    // capturas da faixa: (inicio, fim, padrao, grupo), em bytes do documento
    let mut caps: Vec<(usize, usize, usize, u16)> = Vec::new();
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(local.clone());
    let mut it = cursor.captures(&lang.highlights, tree.root_node(), src.as_bytes());
    while let Some((m, i)) = it.next() {
        let c = m.captures()[*i];
        let Some(g) = lang.group_of[c.index as usize] else { continue };
        let r = c.node.byte_range();
        let (s, e) = ((r.start + off).max(range.start), (r.end + off).min(range.end));
        if s < e {
            caps.push((s, e, m.pattern_index, g));
        }
    }
    // pinta da menor prioridade para a maior e do mais externo para o mais
    // interno; no mesmo trecho, quem pinta por ultimo vence
    caps.sort_by(|a, b| {
        lang.priority[a.2]
            .cmp(&lang.priority[b.2])
            .then((b.1 - b.0).cmp(&(a.1 - a.0)))
            .then(if lang.later_wins { a.2.cmp(&b.2) } else { b.2.cmp(&a.2) })
    });
    for (s, e, _, g) in caps {
        paint[s - range.start..e - range.start].fill(Some(lang.groups[g as usize].as_str()));
    }

    if depth >= 2 {
        return;
    }
    let regions = injection_regions(lang, tree, src, local);
    for (target, s, e) in regions {
        let snippet = &src[s..e];
        let mut parser = Parser::new();
        if parser.set_language(&target.language).is_err() {
            continue;
        }
        if let Some(t) = parser.parse(snippet, None) {
            paint_lang(target, &t, snippet, off + s, range.clone(), paint, depth + 1);
        }
    }
}

/// Trechos de `src` em outra linguagem (as injecoes da consulta de `lang`),
/// dentro dos bytes `range`: (linguagem, inicio, fim).
pub(crate) fn injection_regions(lang: &'static Lang, tree: &Tree, src: &str, range: Range<usize>) -> Vec<(&'static Lang, usize, usize)> {
    let Some(inj) = lang.injections.as_ref() else { return Vec::new() };
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range);
    let mut matches = cursor.matches(&inj.query, tree.root_node(), src.as_bytes());
    let mut regions: Vec<(&'static Lang, usize, usize)> = Vec::new();
    while let Some(m) = matches.next() {
        let p = m.pattern_index;
        let name = inj.fixed[p].clone().or_else(|| {
            let cap = inj.language?;
            let node = m.captures().iter().find(|c| c.index == cap)?.node;
            node.utf8_text(src.as_bytes()).ok().map(|t| t.trim().to_ascii_lowercase())
        });
        let Some(target) = name.as_deref().and_then(lang_by_name) else { continue };
        for c in m.captures().iter().filter(|c| c.index == inj.content) {
            let (mut s, mut e) = (c.node.start_byte(), c.node.end_byte());
            // `#offset!` na mesma linha (tirar aspas, `--` do comentario...)
            if let Some([r0, c0, r1, c1]) = inj.offset[p] {
                if r0 == 0 && r1 == 0 {
                    s = (s as isize + c0).max(0) as usize;
                    e = (e as isize + c1).max(0) as usize;
                }
            }
            if s < e && e <= src.len() && src.is_char_boundary(s) && src.is_char_boundary(e) {
                regions.push((target, s, e));
            }
        }
    }
    regions
}

fn lang_by_name(name: &str) -> Option<&'static Lang> {
    lang(name)
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut v = Vec::new();
    let mut at = 0;
    for l in text.split_inclusive('\n') {
        v.push(at);
        at += l.len();
    }
    v
}

fn merge(mut ranges: Vec<Range<usize>>, n: usize) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < n.max(1));
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        let r = r.start..r.end.min(n);
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    fn group_at<'a>(spans: &'a [Span], row: usize, col: usize) -> Option<&'a str> {
        spans.iter().find(|s| s.row == row && s.start_col <= col && col < s.end_col).map(|s| s.group)
    }

    #[test]
    fn realce_basico_lua() {
        let doc = Document::new(lang("lua").unwrap(), &lines("local x = \"oi\" -- nota\nreturn x"));
        let spans = doc.highlights(0..2);
        assert_eq!(group_at(&spans, 0, 0), Some("@keyword.lua"));
        assert_eq!(group_at(&spans, 0, 11), Some("@string.lua"));
        assert_eq!(group_at(&spans, 0, 16), Some("@comment.lua"));
        assert_eq!(group_at(&spans, 1, 0), Some("@keyword.return.lua"));
        // sem sobreposicao e dentro da linha
        for w in spans.windows(2) {
            assert!(w[0].row < w[1].row || w[0].end_col <= w[1].start_col, "{w:?}");
        }
    }

    #[test]
    fn edicao_incremental_igual_ao_parse_do_zero() {
        let mut doc = Document::new(lang("rust").unwrap(), &lines("fn a() {}\nfn b() {}\nfn c() {}"));
        // troca a linha do meio e insere duas
        let changed = doc.apply_lines(1, Some(2), &lines("/* aberto\nainda\nfn b() {}"));
        assert_eq!(doc.text(), "fn a() {}\n/* aberto\nainda\nfn b() {}\nfn c() {}\n");
        assert_eq!(doc.line_count(), 5);
        // o comentario aberto muda a sintaxe ate o fim: as linhas seguintes entram
        assert!(changed.iter().any(|r| r.contains(&4)), "{changed:?}");
        let fresh = Document::new(lang("rust").unwrap(), &lines(doc.text()));
        assert_eq!(doc.highlights(0..5), fresh.highlights(0..5));
        assert_eq!(group_at(&doc.highlights(4..5), 4, 0), Some("@comment.rust"));
    }

    #[test]
    fn apagar_ate_o_fim_e_inserir_no_fim() {
        let mut doc = Document::new(lang("python").unwrap(), &lines("a = 1\nb = 2\nc = 3"));
        doc.apply_lines(1, None, &[]);
        assert_eq!(doc.text(), "a = 1\n");
        doc.apply_lines(1, Some(1), &lines("def f():\n    return 'x'"));
        assert_eq!(doc.text(), "a = 1\ndef f():\n    return 'x'\n");
        let fresh = Document::new(lang("python").unwrap(), &lines(doc.text()));
        assert_eq!(doc.highlights(0..3), fresh.highlights(0..3));
    }

    #[test]
    fn injecoes_comentario_e_padrao_lua() {
        let src = "-- TODO: trocar
local n = s:match(\"%d+\")";
        let doc = Document::new(lang("lua").unwrap(), &lines(src));
        let spans = doc.highlights(0..2);
        // `TODO` vem da gramatica de comentario, por cima do @comment do Lua
        let todo = group_at(&spans, 0, 3).unwrap();
        assert!(todo.ends_with(".comment"), "{todo}");
        assert_eq!(group_at(&spans, 0, 10), Some("@comment.lua"));
        // `%d` dentro da string vem dos padroes Lua (luap)
        let pat = group_at(&spans, 1, 19).unwrap();
        assert!(pat.ends_with(".luap"), "{pat}");
    }

    #[test]
    fn todas_as_gramaticas_carregam() {
        for name in LANG_NAMES {
            let l = lang(name).unwrap_or_else(|| panic!("{name}"));
            let doc = Document::new(l, &lines("x"));
            let _ = doc.highlights(0..1);
        }
    }
}
