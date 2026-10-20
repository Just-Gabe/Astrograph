//! Registradores `+` e `*` e `clipboard=unnamedplus`: o app leva e traz o texto do clipboard do sistema.

use astrograph_core::Editor;

fn ed(linhas: &[&str]) -> Editor {
    Editor::new(linhas)
}

#[test]
fn mais_y_manda_para_o_clipboard_e_mais_p_le_do_clipboard() {
    let mut e = ed(&["one two"]);
    e.feed_str("\"+yw");
    assert_eq!(e.clipboard_alterado().as_deref(), Some("one "));
    assert_eq!(e.clipboard_alterado(), None, "só avisa uma vez");
    // o app diz o que há no clipboard; `"+p` cola de lá
    e.definir_clipboard(Some("XYZ".into()));
    e.feed_str("$\"+p");
    assert_eq!(e.lines(), vec!["one twoXYZ"]);
    // `"*` é o mesmo clipboard
    e.definir_clipboard(Some("Q".into()));
    e.feed_str("\"*P");
    assert!(e.lines()[0].contains('Q'));
    // clipboard vazio: nada a colar
    e.definir_clipboard(None);
    let antes = e.lines();
    e.feed_str("\"+p");
    assert_eq!(e.lines(), antes);
}

#[test]
fn texto_com_quebra_no_fim_vem_do_clipboard_como_linhas() {
    let mut e = ed(&["a", "b"]);
    e.definir_clipboard(Some("nova linha\n".into()));
    e.feed_str("\"+p");
    assert_eq!(e.lines(), vec!["a", "nova linha", "b"]);
    // copiar linhas manda o texto com a quebra de linha no fim
    let mut e = ed(&["um", "dois"]);
    e.feed_str("\"+yy");
    assert_eq!(e.clipboard_alterado().as_deref(), Some("um\n"));
}

#[test]
fn unnamedplus_usa_o_clipboard_nas_copias_e_colagens_comuns() {
    let mut e = ed(&["abc def"]);
    e.clipboard_unnamedplus = true;
    e.feed_str("yw");
    assert_eq!(e.clipboard_alterado().as_deref(), Some("abc "));
    e.feed_str("dw");
    assert_eq!(e.clipboard_alterado().as_deref(), Some("abc "));
    // outro programa copiou algo: o `p` comum cola isso
    e.definir_clipboard(Some("zz".into()));
    e.feed_str("p");
    assert_eq!(e.lines(), vec!["dzzef"]);
    // sem a opção, o clipboard não entra nas cópias comuns
    let mut e = ed(&["abc"]);
    e.feed_str("yw");
    assert_eq!(e.clipboard_alterado(), None);
}
