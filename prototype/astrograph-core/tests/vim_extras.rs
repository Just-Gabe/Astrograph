//! Comportamentos do Vim que o núcleo ganhou. Cada linha da tabela foi gerada rodando as mesmas teclas
//! no nvim 0.11 (`texto` e `cursor` finais, registrador sem nome com o tipo depois do `|`).

use astrograph_core::Editor;

struct Caso {
    nome: &'static str,
    linhas: &'static [&'static str],
    teclas: &'static str,
    texto: &'static [&'static str],
    cursor: (usize, usize),
    reg: &'static str,
}

const CASOS: &[Caso] = &[
    Caso { nome: "C-a decimal", linhas: &["x = 10"], teclas: "<C-a>", texto: &["x = 11"], cursor: (0, 5), reg: "|" },
    Caso { nome: "C-x com contagem", linhas: &["x = 10"], teclas: "5<C-x>", texto: &["x = 5"], cursor: (0, 4), reg: "|" },
    Caso { nome: "C-a sem número", linhas: &["abc"], teclas: "<C-a>", texto: &["abc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "C-a número à frente", linhas: &["a 9 b 20"], teclas: "<C-a>", texto: &["a 10 b 20"], cursor: (0, 3), reg: "|" },
    Caso { nome: "C-a negativo cruza zero", linhas: &["y = -5"], teclas: "6<C-a>", texto: &["y = 1"], cursor: (0, 4), reg: "|" },
    Caso { nome: "C-x vira negativo", linhas: &["y = 1"], teclas: "3<C-x>", texto: &["y = -2"], cursor: (0, 5), reg: "|" },
    Caso { nome: "C-a zeros à esquerda", linhas: &["007"], teclas: "<C-a>", texto: &["008"], cursor: (0, 2), reg: "|" },
    Caso { nome: "C-a hex minúsculo", linhas: &["0x1f"], teclas: "<C-a>", texto: &["0x20"], cursor: (0, 3), reg: "|" },
    Caso { nome: "C-a hex maiúsculo", linhas: &["0xFF"], teclas: "<C-a>", texto: &["0x100"], cursor: (0, 4), reg: "|" },
    Caso { nome: "C-a binário", linhas: &["0b101"], teclas: "<C-a>", texto: &["0b110"], cursor: (0, 4), reg: "|" },
    Caso { nome: "C-a com ponto", linhas: &["n 1"], teclas: "<C-a>..", texto: &["n 4"], cursor: (0, 2), reg: "|" },
    Caso { nome: "visual C-a", linhas: &["1", "1", "1"], teclas: "VG<C-a>", texto: &["2", "2", "2"], cursor: (0, 0), reg: "|" },
    Caso { nome: "visual C-a contagem", linhas: &["1", "1", "1"], teclas: "VG2<C-a>", texto: &["3", "3", "3"], cursor: (0, 0), reg: "|" },
    Caso { nome: "visual g C-a", linhas: &["1", "1", "1"], teclas: "VGg<C-a>", texto: &["2", "3", "4"], cursor: (0, 0), reg: "|" },
    Caso { nome: "visual g C-x", linhas: &["1", "1", "1"], teclas: "VGg<C-x>", texto: &["0", "-1", "-2"], cursor: (0, 0), reg: "|" },
    Caso { nome: "visual g C-a pula sem número", linhas: &["0", "x", "0"], teclas: "VGg<C-a>", texto: &["1", "x", "2"], cursor: (0, 0), reg: "|" },
    Caso { nome: "gUiw", linhas: &["one two"], teclas: "gUiw", texto: &["ONE two"], cursor: (0, 0), reg: "|" },
    Caso { nome: "guiw", linhas: &["ONE two"], teclas: "guiw", texto: &["one two"], cursor: (0, 0), reg: "|" },
    Caso { nome: "g~~", linhas: &["One Two"], teclas: "g~~", texto: &["oNE tWO"], cursor: (0, 0), reg: "|" },
    Caso { nome: "gUj", linhas: &["abc", "def"], teclas: "gUj", texto: &["ABC", "DEF"], cursor: (0, 0), reg: "|" },
    Caso { nome: "g??", linhas: &["abc"], teclas: "g??", texto: &["nop"], cursor: (0, 0), reg: "|" },
    Caso { nome: "2gUU", linhas: &["abc", "def", "ghi"], teclas: "2gUU", texto: &["ABC", "DEF", "ghi"], cursor: (0, 0), reg: "|" },
    Caso { nome: "gUgU", linhas: &["abc"], teclas: "gUgU", texto: &["ABC"], cursor: (0, 0), reg: "|" },
    Caso { nome: "gUf. e ponto", linhas: &["a.b c.d"], teclas: "gUf.w.", texto: &["A.B C.d"], cursor: (0, 1), reg: "|" },
    Caso { nome: "gUh região vazia", linhas: &["one"], teclas: "gUh", texto: &["One"], cursor: (0, 0), reg: "|" },
    Caso { nome: "wgUU recuado", linhas: &["  abc def"], teclas: "wgUU", texto: &["  ABC DEF"], cursor: (0, 2), reg: "|" },
    Caso { nome: "viwd w ponto", linhas: &["aaa bbb ccc"], teclas: "viwdw.", texto: &["  ccc"], cursor: (0, 1), reg: "bbb|v" },
    Caso { nome: "Vjd ponto", linhas: &["1", "2", "3", "4", "5"], teclas: "Vjd.", texto: &["5"], cursor: (0, 0), reg: "3\n4\n|V" },
    Caso { nome: "viwc ponto", linhas: &["ab cd ef"], teclas: "viwcX<Esc>w.", texto: &["X X ef"], cursor: (0, 2), reg: "cd|v" },
    Caso { nome: "normal troca o ponto", linhas: &["abc abc"], teclas: "viwdwx.", texto: &[" c"], cursor: (0, 1), reg: "b|v" },
    Caso { nome: "undo visual volta ao inicio", linhas: &["hello world"], teclas: "wviwdu", texto: &["hello world"], cursor: (0, 6), reg: "world|v" },
    Caso { nome: "C-a falho e undo", linhas: &["abc def"], teclas: "w<C-a>wu", texto: &["abc def"], cursor: (0, 4), reg: "|" },
    Caso { nome: "bloco corta tab", linhas: &["\tx", "abc"], teclas: "<C-v>jx", texto: &["x", ""], cursor: (0, 0), reg: "\t\nabc|\u{16}8" },
    Caso { nome: "bloco corta largo", linhas: &["日本", "ab"], teclas: "<C-v>jx", texto: &["本", ""], cursor: (0, 0), reg: "日\nab|\u{16}2" },
    Caso { nome: "bloco r em largo", linhas: &["日本", "ab"], teclas: "<C-v>jlrz", texto: &["zz本", "zz"], cursor: (0, 0), reg: "|" },
    Caso { nome: "bloco y em tab", linhas: &["\tx", "abc"], teclas: "<C-v>jy", texto: &["\tx", "abc"], cursor: (0, 0), reg: "\t\nabc|\u{16}8" },
    Caso { nome: "anexa charwise", linhas: &["one two"], teclas: "\"ayw\"Ayw", texto: &["one two"], cursor: (0, 0), reg: "one one |v" },
    Caso { nome: "anexa linewise", linhas: &["one", "two"], teclas: "\"ayy\"Ayy", texto: &["one", "two"], cursor: (0, 0), reg: "one\none\n|V" },
    Caso { nome: "anexa char e linha", linhas: &["one", "two"], teclas: "\"ayw\"Ayy", texto: &["one", "two"], cursor: (0, 0), reg: "one\none\n|V" },
    Caso { nome: "buraco negro", linhas: &["abc def"], teclas: "yiw\"_dd", texto: &[""], cursor: (0, 0), reg: "abc|v" },
    Caso { nome: "registrador ponto", linhas: &["x"], teclas: "iab<Esc>\".p", texto: &["ababx"], cursor: (0, 3), reg: "|" },
    Caso { nome: "registrador busca", linhas: &["one two"], teclas: "/two<CR>0\"/P", texto: &["twoone two"], cursor: (0, 2), reg: "|" },
    Caso { nome: "registrador 0 no visual", linhas: &["a b"], teclas: "yiwwviw\"0p", texto: &["a a"], cursor: (0, 2), reg: "b|v" },
    Caso { nome: "C-r no insert", linhas: &["abc"], teclas: "yiwA <C-r>\"<Esc>", texto: &["abc abc"], cursor: (0, 6), reg: "abc|v" },
    Caso { nome: "C-r nomeado", linhas: &["abc"], teclas: "\"qyiwo<C-r>q<Esc>", texto: &["abc", "abc"], cursor: (1, 2), reg: "abc|v" },
    Caso { nome: "C-r linewise", linhas: &["abc", "d"], teclas: "yyjA<C-r>\"<Esc>", texto: &["abc", "dabc", ""], cursor: (2, 0), reg: "abc\n|V" },
    Caso { nome: "C-r e ponto", linhas: &["abc"], teclas: "yiwA-<C-r>\"<Esc>yl.", texto: &["abc-abc-abc"], cursor: (0, 10), reg: "c|v" },
    Caso { nome: "C-r vazio", linhas: &["abc"], teclas: "A<C-r>z<Esc>", texto: &["abc"], cursor: (0, 2), reg: "|" },
    Caso { nome: "p falho vira ponto", linhas: &["ab"], teclas: "pyl.", texto: &["aab"], cursor: (0, 1), reg: "a|v" },
    Caso { nome: "Bdw ponto anexa", linhas: &["say \"hi\" x"], teclas: "\"Bdw.", texto: &["hi\" x"], cursor: (0, 0), reg: "say \"|v" },
    Caso { nome: "1p sobe no ponto", linhas: &["a", "b", "c"], teclas: "dddd\"1p..", texto: &["c", "b", "a"], cursor: (2, 0), reg: "b\n|V" },
    Caso { nome: "map noremap normal", linhas: &["a", "b", "c"], teclas: ":nnoremap Q dd<CR>Q", texto: &["b", "c"], cursor: (0, 0), reg: "a\n|V" },
    Caso { nome: "map recursivo", linhas: &["abc"], teclas: ":nmap A B<CR>:nmap B x<CR>A", texto: &["bc"], cursor: (0, 0), reg: "a|v" },
    Caso { nome: "noremap nao recursa", linhas: &["abc def"], teclas: ":nmap Q W<CR>:nnoremap W dw<CR>Qx", texto: &["ef"], cursor: (0, 0), reg: "d|v" },
    Caso { nome: "imap jk", linhas: &["xyz"], teclas: ":inoremap jk <Esc><CR>Aabcjkx", texto: &["xyzabcjkx"], cursor: (0, 8), reg: "|" },
    Caso { nome: "prefixo com espera", linhas: &["abc"], teclas: ":nnoremap gz ihi<Esc><CR>gzx", texto: &["bc"], cursor: (0, 0), reg: "a|v" },
    Caso { nome: "prefixo que nao casa", linhas: &["abc"], teclas: ":nnoremap ab dd<CR>axy<Esc>", texto: &["axybc"], cursor: (0, 2), reg: "|" },
    Caso { nome: "nunmap", linhas: &["abc", "def"], teclas: ":nnoremap S dd<CR>:nunmap S<CR>Sxy<Esc>", texto: &["xy", "def"], cursor: (0, 1), reg: "abc\n|V" },
    Caso { nome: "vmap", linhas: &["abc def"], teclas: ":vnoremap Q d<CR>viwQ", texto: &[" def"], cursor: (0, 0), reg: "abc|v" },
    Caso { nome: "map normal e visual", linhas: &["abc"], teclas: ":noremap Q ~<CR>vQ", texto: &["Abc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "map com ponto", linhas: &["abcdef"], teclas: ":nnoremap Q x<CR>Q.", texto: &["cdef"], cursor: (0, 0), reg: "b|v" },
    Caso { nome: "map com contagem no rhs", linhas: &["a b c d e"], teclas: ":nnoremap Q 2dw<CR>Q", texto: &["c d e"], cursor: (0, 0), reg: "a b |v" },
    Caso { nome: "map com ex no rhs", linhas: &["abc", "def"], teclas: ":nnoremap Q :2d<CR><CR>Q", texto: &["abc", "def"], cursor: (1, 0), reg: "|" },
    Caso { nome: "map maior ganha", linhas: &["abc"], teclas: ":nnoremap gz x<CR>:nnoremap gzz dd<CR>gzz", texto: &[""], cursor: (0, 0), reg: "abc\n|V" },
    Caso { nome: "map espaco", linhas: &["abc def"], teclas: ":nnoremap <Space>w dw<CR> w", texto: &["adef"], cursor: (0, 1), reg: "bc |v" },
    Caso { nome: "map ctrl", linhas: &["abc def"], teclas: ":nnoremap <C-n> dw<CR><C-n>", texto: &["abc def"], cursor: (0, 0), reg: "|" },
    Caso { nome: "silent buffer", linhas: &["abc"], teclas: ":nnoremap <silent> <buffer> Q x<CR>Q", texto: &["bc"], cursor: (0, 0), reg: "a|v" },
    Caso { nome: "imap em visual nao vale", linhas: &["abc"], teclas: ":inoremap Q z<CR>vQ", texto: &["abc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "set ic", linhas: &["foo ONE bar one"], teclas: ":set ic<CR>/one<CR>x", texto: &["foo NE bar one"], cursor: (0, 4), reg: "O|v" },
    Caso { nome: "set ic scs maiúscula", linhas: &["foo one bar ONE"], teclas: ":set ic scs<CR>/ONE<CR>x", texto: &["foo one bar NE"], cursor: (0, 12), reg: "O|v" },
    Caso { nome: "set ic scs minúscula", linhas: &["foo ONE bar one"], teclas: ":set ic scs<CR>/one<CR>x", texto: &["foo NE bar one"], cursor: (0, 4), reg: "O|v" },
    Caso { nome: "set noic", linhas: &["foo ONE bar one"], teclas: ":set ic<CR>:set noic<CR>/one<CR>x", texto: &["foo ONE bar ne"], cursor: (0, 12), reg: "o|v" },
    Caso { nome: "set ic! inverte", linhas: &["foo ONE bar one"], teclas: ":set ic!<CR>/one<CR>x", texto: &["foo NE bar one"], cursor: (0, 4), reg: "O|v" },
    Caso { nome: "set et sw ts tab", linhas: &["abc"], teclas: ":set et sw=4 ts=4<CR>i<Tab>x<Esc>", texto: &["    xabc"], cursor: (0, 4), reg: "|" },
    Caso { nome: "set et tab no meio", linhas: &["abc"], teclas: ":set et ts=4<CR>lli<Tab>x<Esc>", texto: &["ab  xc"], cursor: (0, 4), reg: "|" },
    Caso { nome: "set sw=4 ts=4 desloca em tab", linhas: &["abc"], teclas: ":set sw=4 ts=4<CR>>>", texto: &["\tabc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "set sw=2 desloca", linhas: &["abc"], teclas: ":set sw=2<CR>>>", texto: &["  abc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "set et desloca", linhas: &["abc"], teclas: ":set et<CR>>>", texto: &["        abc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "set sw ts dois niveis", linhas: &["abc"], teclas: ":set sw=4 ts=4<CR>>>>>", texto: &["\t\tabc"], cursor: (0, 0), reg: "|" },
    Caso { nome: "set ts=4 largura de tab", linhas: &["\tabc"], teclas: ":set ts=4<CR>$x", texto: &["\tab"], cursor: (0, 2), reg: "c|v" },
    Caso { nome: "set varias opcoes", linhas: &["foo ONE"], teclas: ":set ic sw=3 et<CR>/one<CR>>>", texto: &["   foo ONE"], cursor: (0, 4), reg: "|" },
]; 

fn registro(ed: &Editor) -> String {
    match ed.register('"') {
        Some(r) => {
            let tipo = if r.block { format!("\u{16}{}", r.bwidth) } else if r.linewise { "V".to_string() } else { "v".to_string() };
            format!("{}|{}", if r.linewise { format!("{}\n", r.text) } else { r.text.clone() }, tipo)
        }
        None => "|".into(),
    }
}

#[test]
fn casos_curados_do_nvim() {
    let mut falhas = Vec::new();
    for c in CASOS {
        let mut ed = Editor::new(c.linhas);
        ed.feed_str(c.teclas);
        ed.feed_str("<Esc>");
        let (t, cur, reg) = (ed.lines(), (ed.cursor.row, ed.cursor.col), registro(&ed));
        if t != c.texto || cur != c.cursor || reg != c.reg {
            falhas.push(format!("{}: {:?} em {:?}\n   nvim: {:?} {:?} {:?}\n   rust: {:?} {:?} {:?}", c.nome, c.teclas, c.linhas, c.texto, c.cursor, c.reg, t, cur, reg));
        }
    }
    assert!(falhas.is_empty(), "{} caso(s) divergem do nvim:\n{}", falhas.len(), falhas.join("\n"));
}
