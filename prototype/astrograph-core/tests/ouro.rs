//! Regressão: casos em que o nvim real e o núcleo concordaram quando foram
//! gerados (`OURO=tests/ouro.tsv cargo run --release --example diferencial`).
//! Roda sem nvim.

use astrograph_core::Editor;

fn strs(v: &serde_json::Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn casos_do_nvim() {
    let dados = include_str!("ouro.tsv");
    let mut n = 0;
    for l in dados.lines().filter(|l| !l.is_empty()) {
        let c: Vec<&str> = l.split('\t').collect();
        let lines = strs(&serde_json::from_str(c[0]).unwrap());
        let keys = strs(&serde_json::from_str(c[1]).unwrap()).remove(0);
        let want = strs(&serde_json::from_str(c[2]).unwrap());
        let (row, col): (usize, usize) = (c[3].parse().unwrap(), c[4].parse().unwrap());
        let mut ed = Editor::new(&lines);
        ed.feed_str(&keys);
        ed.feed_str("<Esc>");
        assert_eq!(ed.lines(), want, "texto: {keys:?} em {lines:?}");
        assert_eq!((ed.cursor.row, ed.cursor.col), (row, col), "cursor: {keys:?} em {lines:?}");
        n += 1;
    }
    assert!(n > 100, "poucos casos: {n}");
}
