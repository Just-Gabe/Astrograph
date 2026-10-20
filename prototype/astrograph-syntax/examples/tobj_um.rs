//! Textobject num ponto de um arquivo (depuracao).
//!
//!   cargo run --release --example tobj_um -- <arquivo> <@captura> <linha0> <coluna0>
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let text = std::fs::read_to_string(&a[0]).unwrap().replace("\r\n", "\n");
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let l = astrograph_syntax::detect(&a[0], "").and_then(astrograph_syntax::lang).unwrap();
    let d = astrograph_syntax::Document::new(l, &lines);
    let (row, col): (usize, usize) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    println!("{:?}", d.textobject_at(&a[1], row, col, true, false));
    for r in d.textobject_ranges(&a[1]) {
        if std::env::var("TODOS").is_ok() || (r.srow + 15 >= row && r.srow <= row + 15) {
            println!("  candidato {r:?}");
        }
    }
}
