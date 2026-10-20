//! Custo da varredura de capturas de indent (uma por versao da arvore):
//! so o cursor de capturas do tree-sitter x a varredura completa do indent.
//!
//!   cargo run --release --example indent_bench -- <arquivo>
use std::time::Instant;
use tree_sitter::{QueryCursor, StreamingIterator};

fn main() {
    let path = std::env::args().nth(1).expect("arquivo");
    let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let l = astrograph_syntax::detect(&path, "").and_then(astrograph_syntax::lang).unwrap();
    let doc = astrograph_syntax::Document::new(l, &lines);
    let q = l.indents.as_ref().unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&l.language).unwrap();
    let src = lines.iter().map(|x| format!("{x}\n")).collect::<String>();
    let tree = parser.parse(&src, None).unwrap();
    let n = 50;
    let t = Instant::now();
    let mut count = 0;
    for _ in 0..n {
        let mut c = QueryCursor::new();
        let mut it = c.captures(q, tree.root_node(), src.as_bytes());
        while let Some(_) = it.next() {
            count += 1;
        }
    }
    println!("cursor de capturas: {:.0} us por varredura ({} capturas)", t.elapsed().as_secs_f64() * 1e6 / n as f64, count / n);
    let t = Instant::now();
    for _ in 0..n {
        let mut d = astrograph_syntax::Document::new(l, &lines[..1]);
        d.apply_lines(0, None, &lines);
        let t1 = Instant::now();
        let _ = d.indent(lines.len() / 2, 2, 8);
        std::hint::black_box(t1);
    }
    let _ = doc;
    println!("(referencia) parse completo + 1 indent: {:.0} us", t.elapsed().as_secs_f64() * 1e6 / n as f64);
    let mut d = astrograph_syntax::Document::new(l, &lines);
    let mut first = 0.0;
    for i in 0..n {
        d.apply_lines(1, Some(1), &[String::new()]);
        let t1 = Instant::now();
        let _ = d.indent(2, 2, 8);
        if i > 0 { first += t1.elapsed().as_secs_f64() * 1e6; }
    }
    println!("indent logo depois de uma edicao (varredura + algoritmo): {:.0} us", first / (n - 1) as f64);
    let mid = lines.len() / 2;
    let mut tot = 0.0;
    for _ in 0..n {
        let keep = d.line_count();
        let _ = keep;
        d.apply_lines(mid, Some(mid), &[String::new()]);
        let t1 = Instant::now();
        let _ = d.indent(mid + 1, 4, 8);
        tot += t1.elapsed().as_secs_f64() * 1e6;
        d.apply_lines(mid, Some(mid + 1), &[]);
    }
    println!("indent no meio do arquivo depois de uma edicao: {:.0} us", tot / n as f64);
    let t1 = Instant::now();
    for _ in 0..n {
        let _ = d.indent(2, 2, 8);
    }
    println!("indent com o cache pronto (so o algoritmo): {:.0} us", t1.elapsed().as_secs_f64() * 1e6 / n as f64);
    let t1 = Instant::now();
    for _ in 0..n {
        d.apply_lines(1, Some(1), &[String::new()]);
    }
    println!("apply_lines (parse incremental) sozinho: {:.0} us", t1.elapsed().as_secs_f64() * 1e6 / n as f64);
}
