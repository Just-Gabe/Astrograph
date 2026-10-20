//! Conta os casamentos da consulta de textobjects e a ultima linha capturada.
use tree_sitter::{QueryCursor, StreamingIterator};
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    let l = astrograph_syntax::detect(&path, "").and_then(astrograph_syntax::lang).unwrap();
    let mut p = tree_sitter::Parser::new();
    p.set_language(&l.language).unwrap();
    let tree = p.parse(&text, None).unwrap();
    let q = l.textobjects.as_ref().unwrap();
    for limit in [None, Some(u32::MAX)] {
        let mut c = QueryCursor::new();
        if let Some(n) = limit { c.set_match_limit(n); }
        let mut it = c.matches(q, tree.root_node(), text.as_bytes());
        let (mut n, mut last) = (0, 0);
        while let Some(m) = it.next() {
            n += 1;
            for cap in m.captures() { last = last.max(cap.node.end_position().row); }
        }
        println!("limite {limit:?}: {n} casamentos, ultima linha {last}, estourou {}", c.did_exceed_match_limit());
    }
    println!("linhas no arquivo: {}", text.lines().count());
    let mut c = QueryCursor::new();
    let mut it = c.matches(q, tree.root_node(), text.as_bytes());
    let names = q.capture_names();
    while let Some(m) = it.next() {
        for cap in m.captures() {
            if cap.node.kind() == "match_expression" {
                println!("match_expression linha {} capturado como {} (padrao {})", cap.node.start_position().row, names[cap.index as usize], m.pattern_index);
            }
        }
    }
    let mut n = tree.root_node().descendant_for_point_range(tree_sitter::Point::new(124, 16), tree_sitter::Point::new(124, 17));
    while let Some(x) = n {
        println!("  {} {:?}-{:?}", x.kind(), x.start_position(), x.end_position());
        n = x.parent();
    }
    for i in 0..q.pattern_count() {
        let start = q.start_byte_for_pattern(i);
        let src = q.pattern_count();
        let _ = src;
        if i > 0 && start == 0 { continue; }
    }
}
