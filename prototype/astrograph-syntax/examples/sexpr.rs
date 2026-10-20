//! Imprime a arvore (s-expression) de um arquivo, para comparar com a do nvim.
//!
//!   cargo run --release --example sexpr -- <arquivo>
fn main() {
    let path = std::env::args().nth(1).expect("arquivo");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let l = astrograph_syntax::detect(&path, "").and_then(astrograph_syntax::lang).expect("gramatica");
    println!("rust: {}", astrograph_syntax::Document::new(l, &lines).sexpr());
}
