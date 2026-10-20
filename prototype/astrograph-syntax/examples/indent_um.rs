//! Indent de uma linha de um arquivo (depuracao com ASTROGRAPH_INDENT_DEBUG=1).
//!
//!   cargo run --release --example indent_um -- <arquivo> <linha> [shiftwidth]
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("arquivo");
    let lnum: usize = a.next().expect("linha").parse().unwrap();
    let sw: i64 = a.next().map_or(4, |v| v.parse().unwrap());
    let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    if lines.last().is_some_and(|l| l.is_empty()) && lines.len() > 1 {
        lines.pop();
    }
    let l = astrograph_syntax::detect(&path, "").and_then(astrograph_syntax::lang).expect("gramatica");
    let doc = astrograph_syntax::Document::new(l, &lines);
    println!("indent({lnum}) = {} (linhas: {})", doc.indent(lnum, sw, 8), lines.len());
}
