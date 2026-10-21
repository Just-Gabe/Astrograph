//! `.editorconfig`: recuo (`indent_style`, `indent_size`, `tab_width`) e o que fazer ao gravar
//! (`trim_trailing_whitespace`, `insert_final_newline`) para o arquivo aberto.

use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Opcoes {
    pub expandtab: Option<bool>,
    pub shiftwidth: Option<usize>,
    pub tabstop: Option<usize>,
    pub trim: Option<bool>,
    pub final_newline: Option<bool>,
}

/// Traduz o glob do `.editorconfig` para regex: `*` (sem `/`), `**`, `?`, `{a,b}`, `[abc]`.
fn glob_para_regex(g: &str) -> Option<regex::Regex> {
    let c: Vec<char> = g.chars().collect();
    let mut r = String::from("^");
    let mut i = 0;
    let mut chaves = 0;
    while i < c.len() {
        match c[i] {
            '*' if c.get(i + 1) == Some(&'*') => {
                r.push_str(".*");
                i += 1;
            }
            '*' => r.push_str("[^/]*"),
            '?' => r.push_str("[^/]"),
            '{' => {
                chaves += 1;
                r.push_str("(?:");
            }
            '}' if chaves > 0 => {
                chaves -= 1;
                r.push(')');
            }
            ',' if chaves > 0 => r.push('|'),
            '[' => {
                // classe de caracteres: copia até o `]`
                let mut j = i + 1;
                let mut cls = String::from("[");
                if c.get(j) == Some(&'!') {
                    cls.push('^');
                    j += 1;
                }
                while j < c.len() && c[j] != ']' {
                    if c[j] == '\\' {
                        cls.push('\\');
                    }
                    cls.push(c[j]);
                    j += 1;
                }
                cls.push(']');
                r.push_str(&cls);
                i = j;
            }
            ch => r.push_str(&regex::escape(&ch.to_string())),
        }
        i += 1;
    }
    r.push('$');
    regex::Regex::new(&r).ok()
}

/// A seção `[glob]` vale para `rel` (caminho relativo à pasta do `.editorconfig`, com `/`)?
fn secao_vale(glob: &str, rel: &str) -> bool {
    let glob = glob.trim();
    // sem barra: casa o nome do arquivo em qualquer pasta
    let (padrao, alvo) = if glob.contains('/') { (glob.trim_start_matches('/').to_string(), rel.to_string()) } else { (glob.to_string(), rel.rsplit('/').next().unwrap_or(rel).to_string()) };
    glob_para_regex(&padrao).is_some_and(|re| re.is_match(&alvo))
}

/// Aplica as linhas de um `.editorconfig` (texto) ao arquivo `rel`; devolve se tem `root = true`.
fn aplicar(texto: &str, rel: &str, o: &mut Opcoes, tab_width: &mut Option<usize>, size_tab: &mut bool) -> bool {
    let mut root = false;
    let mut vale = false;
    let mut em_secao = false;
    for linha in texto.lines() {
        let l = linha.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some(g) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            em_secao = true;
            vale = secao_vale(g, rel);
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim().to_lowercase());
        if !em_secao {
            if k == "root" && v == "true" {
                root = true;
            }
            continue;
        }
        if !vale {
            continue;
        }
        match k.as_str() {
            "indent_style" => o.expandtab = Some(v == "space"),
            "indent_size" => {
                if v == "tab" {
                    *size_tab = true;
                    o.shiftwidth = None;
                } else if let Ok(n) = v.parse::<usize>() {
                    *size_tab = false;
                    o.shiftwidth = Some(n.max(1));
                }
            }
            "tab_width" => *tab_width = v.parse::<usize>().ok().map(|n| n.max(1)),
            "trim_trailing_whitespace" => o.trim = Some(v == "true"),
            "insert_final_newline" => o.final_newline = Some(v == "true"),
            _ => {}
        }
    }
    root
}

/// Opções do `.editorconfig` que valem para `arquivo` (sobe as pastas até um `root = true`).
pub fn para(arquivo: &Path) -> Opcoes {
    let arquivo = std::fs::canonicalize(arquivo).unwrap_or_else(|_| arquivo.to_path_buf());
    let mut pastas = Vec::new();
    let mut p = arquivo.parent();
    while let Some(d) = p {
        pastas.push(d.to_path_buf());
        p = d.parent();
    }
    // do mais perto para o mais longe, parando no `root = true`; o mais perto vence
    let mut arquivos = Vec::new();
    for d in &pastas {
        let f = d.join(".editorconfig");
        if let Ok(t) = std::fs::read_to_string(&f) {
            let rel = arquivo.strip_prefix(d).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let mut tmp = Opcoes::default();
            let (mut tw, mut st) = (None, false);
            let root = aplicar(&t, &rel, &mut tmp, &mut tw, &mut st);
            arquivos.push((t, rel));
            if root {
                break;
            }
        }
    }
    let mut o = Opcoes::default();
    let (mut tab_width, mut size_tab) = (None, false);
    // aplica do mais longe ao mais perto: o último vence
    for (t, rel) in arquivos.iter().rev() {
        aplicar(t, rel, &mut o, &mut tab_width, &mut size_tab);
    }
    o.tabstop = tab_width;
    if size_tab {
        // indent_size = tab: o recuo é um tab_width (ou o tabstop padrão)
        o.shiftwidth = Some(tab_width.unwrap_or(8));
    } else if o.tabstop.is_none() && o.shiftwidth.is_some() && o.expandtab == Some(false) {
        // recuo com tabs sem tab_width: o tab vale o indent_size
        o.tabstop = o.shiftwidth;
    }
    o
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-ec-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub/deep")).unwrap();
        d
    }

    #[test]
    fn secoes_globs_e_heranca() {
        let d = pasta("a");
        std::fs::write(d.join(".editorconfig"), "root = true\n\n[*]\nindent_style = space\nindent_size = 2\ntrim_trailing_whitespace = true\n\n[*.{py,rs}]\nindent_size = 4\n\n[Makefile]\nindent_style = tab\n\n[sub/**.md]\ninsert_final_newline = false\n").unwrap();
        std::fs::write(d.join("sub/.editorconfig"), "[*.rs]\nindent_size = 3\n").unwrap();
        for f in ["a.lua", "a.py", "Makefile", "sub/x.rs", "sub/deep/n.md"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let lua = para(&d.join("a.lua"));
        assert_eq!((lua.expandtab, lua.shiftwidth, lua.trim), (Some(true), Some(2), Some(true)));
        assert_eq!(para(&d.join("a.py")).shiftwidth, Some(4));
        let mk = para(&d.join("Makefile"));
        assert_eq!(mk.expandtab, Some(false));
        assert_eq!(mk.tabstop, mk.shiftwidth, "recuo com tab, sem tab_width");
        assert_eq!(para(&d.join("sub/x.rs")).shiftwidth, Some(3), "a pasta mais perto vence");
        assert_eq!(para(&d.join("sub/deep/n.md")).final_newline, Some(false));
    }

    #[test]
    fn root_para_a_busca_e_indent_size_tab() {
        let d = pasta("b");
        std::fs::write(d.join(".editorconfig"), "[*]\nindent_size = 8\n").unwrap();
        std::fs::write(d.join("sub/.editorconfig"), "root = true\n[*]\nindent_style = tab\nindent_size = tab\ntab_width = 4\n").unwrap();
        std::fs::write(d.join("sub/f.go"), "").unwrap();
        let o = para(&d.join("sub/f.go"));
        assert_eq!((o.expandtab, o.shiftwidth, o.tabstop), (Some(false), Some(4), Some(4)));
    }

    #[test]
    fn sem_arquivo_nada_muda() {
        let d = pasta("c");
        std::fs::write(d.join("x.txt"), "").unwrap();
        // pode haver um .editorconfig acima de /tmp na máquina de teste; só garante que não quebra
        let _ = para(&d.join("x.txt"));
    }
}
