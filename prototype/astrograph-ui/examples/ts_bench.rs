//! Mede o tree-sitter em Rust no mesmo arquivo do `rpc_bench` (2000 linhas de
//! Lua): parse completo, parse incremental depois de digitar um caractere e a
//! consulta de realce da linha editada. Somado ao ciclo de RPC, diz se o realce
//! fora do processo do Neovim cabe no orcamento de uma tecla.
//!
//!   cargo run --release --example ts_bench

use std::time::{Duration, Instant};

use tree_sitter::{InputEdit, Parser, Point, Query, QueryCursor, StreamingIterator};

const ROUNDS: usize = 500;

fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    println!(
        "{label:<44} mediana {:>8.1} us   p99 {:>8.1} us   (n={})",
        us(at(0.5)),
        us(at(0.99)),
        samples.len()
    );
}

fn main() {
    let mut source: String = (0..2000)
        .map(|i| format!("local x{i} = function(a, b) return a + b end\n"))
        .collect();
    let lang = tree_sitter::Language::new(tree_sitter_lua::LANGUAGE);
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
    let query = Query::new(&lang, tree_sitter_lua::HIGHLIGHTS_QUERY).unwrap();

    let mut full = Vec::new();
    let mut tree = None;
    for _ in 0..20 {
        let t = Instant::now();
        tree = parser.parse(&source, None);
        full.push(t.elapsed());
    }
    report("parse completo (2000 linhas)", full);

    let mut tree = tree.unwrap();
    let mut incr = Vec::new();
    let mut hl = Vec::new();
    let row = 1999usize;
    let line_start = source[..source.len() - 1].rfind('\n').map_or(0, |p| p + 1);
    let mut col = source.len() - 1 - line_start;
    for _ in 0..ROUNDS {
        // digita "x" no fim da ultima linha
        let at = line_start + col;
        source.insert(at, 'x');
        let t = Instant::now();
        tree.edit(&InputEdit {
            start_byte: at,
            old_end_byte: at,
            new_end_byte: at + 1,
            start_position: Point::new(row, col),
            old_end_position: Point::new(row, col),
            new_end_position: Point::new(row, col + 1),
        });
        tree = parser.parse(&source, Some(&tree)).unwrap();
        incr.push(t.elapsed());
        col += 1;

        let t = Instant::now();
        let mut cursor = QueryCursor::new();
        cursor.set_point_range(Point::new(row, 0)..Point::new(row + 1, 0));
        let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
        let mut n = 0usize;
        while let Some((m, i)) = captures.next() {
            let _ = m.captures()[*i].node.byte_range();
            n += 1;
        }
        std::hint::black_box(n);
        hl.push(t.elapsed());
    }
    report("parse incremental (1 caractere)", incr);
    report("consulta de realce da linha editada", hl);
}
