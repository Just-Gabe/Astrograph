//! Gramaticas compiladas no executavel e suas consultas de realce e injecao.
//!
//! As consultas sao as do nvim-treesitter (pasta `queries/`, ver NOTICE.md),
//! para o realce ter as mesmas cores do Astrograph de antes. Os predicados que
//! so o Neovim entende sao traduzidos na carga. Se a consulta do Neovim nao
//! compilar com a versao da gramatica, vale a consulta que vem com a gramatica.
//!
//! Cada linguagem e montada na primeira vez que e pedida (compilar a consulta
//! custa alguns ms; quem nunca abre um `.toml` nao paga por ele).

use std::sync::OnceLock;

use tree_sitter::{Language, Query, QueryPredicateArg};
use tree_sitter_language::LanguageFn;

pub struct Lang {
    pub name: &'static str,
    pub language: Language,
    pub highlights: Query,
    /// captura da consulta -> indice em `groups` (`None`: captura sem realce)
    pub group_of: Vec<Option<u16>>,
    /// grupos do nvim, no formato `@captura.linguagem` (como o realce nativo)
    pub groups: Vec<String>,
    /// convencao das consultas do Neovim: no mesmo trecho vence o padrao que
    /// vem DEPOIS (nas das gramaticas, o que vem antes)
    pub later_wins: bool,
    /// prioridade de cada padrao (`#set! priority N`; padrao 100)
    pub priority: Vec<u16>,
    /// trechos em outra linguagem (padrao Lua em string, TODO em comentario...)
    pub injections: Option<Injections>,
    /// consulta de indentacao (`@indent.begin`, `@indent.end`...)
    pub indents: Option<Query>,
    /// consulta de textobjects (`@function.outer`, `@parameter.inner`...)
    pub textobjects: Option<Query>,
}

pub struct Injections {
    pub query: Query,
    /// captura `@injection.content`
    pub content: u32,
    /// captura `@injection.language` (linguagem escrita no proprio texto)
    pub language: Option<u32>,
    /// por padrao: `#set! injection.language "x"`
    pub fixed: Vec<Option<String>>,
    /// por padrao: `#offset! @injection.content l0 c0 l1 c1`
    pub offset: Vec<Option<[isize; 4]>>,
}

/// Linguagens de arquivo (nome = o `filetype` do Neovim, exceto `bash`).
pub const LANG_NAMES: [&str; 9] = ["bash", "javascript", "json", "lua", "python", "rust", "toml", "typescript", "tsx"];

/// Linguagens que so aparecem dentro de outras (injecoes).
pub const INJECTED_NAMES: [&str; 6] = ["comment", "luadoc", "luap", "printf", "regex", "vim"];

const ALL: [&str; 15] = [
    "bash", "javascript", "json", "lua", "python", "rust", "toml", "typescript", "tsx", "comment", "luadoc", "luap", "printf", "regex", "vim",
];

// compiladas pelo build.rs a partir de vendor/ (ver vendor/NOTICE.md)
extern "C" {
    fn tree_sitter_luap() -> *const ();
    fn tree_sitter_luadoc() -> *const ();
}

fn language_of(name: &str) -> Option<(Language, &'static str)> {
    // SAFETY: as funcoes so devolvem o ponteiro estatico da gramatica gerada
    let luap = unsafe { LanguageFn::from_raw(tree_sitter_luap) };
    let luadoc = unsafe { LanguageFn::from_raw(tree_sitter_luadoc) };
    Some(match name {
        "bash" => (Language::new(tree_sitter_bash::LANGUAGE), tree_sitter_bash::HIGHLIGHT_QUERY),
        "javascript" => (Language::new(tree_sitter_javascript::LANGUAGE), tree_sitter_javascript::HIGHLIGHT_QUERY),
        "json" => (Language::new(tree_sitter_json::LANGUAGE), tree_sitter_json::HIGHLIGHTS_QUERY),
        "lua" => (Language::new(tree_sitter_lua::LANGUAGE), tree_sitter_lua::HIGHLIGHTS_QUERY),
        "python" => (Language::new(tree_sitter_python::LANGUAGE), tree_sitter_python::HIGHLIGHTS_QUERY),
        "rust" => (Language::new(tree_sitter_rust::LANGUAGE), tree_sitter_rust::HIGHLIGHTS_QUERY),
        "toml" => (Language::new(tree_sitter_toml_ng::LANGUAGE), tree_sitter_toml_ng::HIGHLIGHTS_QUERY),
        "typescript" => (Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT), tree_sitter_typescript::HIGHLIGHTS_QUERY),
        "tsx" => (Language::new(tree_sitter_typescript::LANGUAGE_TSX), tree_sitter_typescript::HIGHLIGHTS_QUERY),
        "comment" => (Language::new(tree_sitter_comment::LANGUAGE), ""),
        "regex" => (Language::new(tree_sitter_regex::LANGUAGE), tree_sitter_regex::HIGHLIGHTS_QUERY),
        "printf" => (Language::new(tree_sitter_printf::LANGUAGE), tree_sitter_printf::HIGHLIGHTS_QUERY),
        "luap" => (Language::new(luap), ""),
        "luadoc" => (Language::new(luadoc), ""),
        "vim" => (tree_sitter_vim::language(), tree_sitter_vim::HIGHLIGHTS_QUERY),
        _ => return None,
    })
}

/// Consultas do Neovim embutidas no executavel.
fn nvim_query(kind: &str, name: &str) -> Option<&'static str> {
    macro_rules! q {
        ($($n:literal),*) => {
            match (kind, name) {
                $(("highlights", $n) => Some(include_str!(concat!("../queries/", $n, "/highlights.scm"))),)*
                _ => None,
            }
        };
    }
    macro_rules! inj {
        ($($n:literal),*) => {
            match name {
                $($n => Some(include_str!(concat!("../queries/", $n, "/injections.scm"))),)*
                _ => None,
            }
        };
    }
    macro_rules! ind {
        ($($n:literal),*) => {
            match name {
                $($n => Some(include_str!(concat!("../queries/", $n, "/indents.scm"))),)*
                _ => None,
            }
        };
    }
    if kind == "injections" {
        return inj!("bash", "ecma", "javascript", "json", "jsx", "lua", "python", "rust", "toml", "tsx", "typescript", "vim");
    }
    if kind == "indents" {
        return ind!("bash", "ecma", "javascript", "json", "jsx", "lua", "python", "rust", "toml", "tsx", "typescript");
    }
    macro_rules! tobj {
        ($($n:literal),*) => {
            match name {
                $($n => Some(include_str!(concat!("../queries/", $n, "/textobjects.scm"))),)*
                _ => None,
            }
        };
    }
    if kind == "textobjects" {
        return tobj!("bash", "ecma", "javascript", "json", "jsx", "lua", "python", "rust", "toml", "tsx", "typescript");
    }
    q!(
        "bash", "ecma", "javascript", "json", "jsx", "lua", "python", "rust", "toml", "tsx", "typescript", "comment", "luadoc", "luap",
        "printf", "regex", "vim"
    )
}

/// Consulta com as herdadas na frente (`; inherits: ecma,jsx`), como o Neovim
/// monta: as herdadas primeiro, a propria por ultimo (e vence).
fn with_inherits(kind: &str, name: &str) -> Option<String> {
    let own = nvim_query(kind, name)?;
    let mut out = String::new();
    if let Some(first) = own.lines().next() {
        if let Some(list) = first.trim().strip_prefix("; inherits:") {
            for parent in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                let parent = parent.trim_start_matches('(').trim_end_matches(')');
                if let Some(p) = with_inherits(kind, parent) {
                    out.push_str(&p);
                    out.push('\n');
                }
            }
        }
    }
    out.push_str(own);
    Some(translate_predicates(&out))
}

/// `#lua-match?`/`#not-lua-match?` -> `#match?`/`#not-match?` com o padrao Lua
/// convertido em regex.
fn translate_predicates(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(at) = rest.find("lua-match?") {
        // `#lua-match?` ou `#not-lua-match?`
        out.push_str(&rest[..at]);
        out.push_str("match?");
        rest = &rest[at + "lua-match?".len()..];
        let Some(q) = rest.find('"') else { break };
        out.push_str(&rest[..=q]);
        rest = &rest[q + 1..];
        let mut end = 0;
        let bytes = rest.as_bytes();
        while end < bytes.len() && bytes[end] != b'"' {
            if bytes[end] == b'\\' {
                end += 1;
            }
            end += 1;
        }
        out.push_str(&lua_pattern_to_regex(&rest[..end.min(rest.len())]));
        rest = &rest[end.min(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// Padrao Lua -> regex, sem barras invertidas (vai dentro de string da
/// consulta): `%u` -> `[A-Z]`, `%.` -> `[.]`, `-` (repeticao curta) -> `*?`.
fn lua_pattern_to_regex(p: &str) -> String {
    fn class(c: char) -> Option<&'static str> {
        Some(match c {
            'a' => "A-Za-z",
            'd' => "0-9",
            'l' => "a-z",
            'u' => "A-Z",
            'w' => "A-Za-z0-9",
            'x' => "0-9A-Fa-f",
            's' => "[:space:]",
            'p' => "[:punct:]",
            _ => return None,
        })
    }
    let mut out = String::new();
    let mut chars = p.chars().peekable();
    let mut in_set = false;
    while let Some(c) = chars.next() {
        match c {
            '%' => match chars.next() {
                Some(k) if class(k).is_some() => {
                    let cls = class(k).unwrap();
                    if in_set {
                        out.push_str(cls);
                    } else {
                        out.push('[');
                        out.push_str(cls);
                        out.push(']');
                    }
                }
                Some(lit) => {
                    if in_set {
                        if lit == ']' || lit == '\\' || lit == '^' || lit == '-' {
                            out.push_str(&format!("\\\\{lit}"));
                        } else {
                            out.push(lit);
                        }
                    } else {
                        out.push('[');
                        out.push(lit);
                        out.push(']');
                    }
                }
                None => {}
            },
            '[' if !in_set => {
                in_set = true;
                out.push('[');
                if chars.peek() == Some(&'^') {
                    out.push(chars.next().unwrap());
                }
            }
            ']' if in_set => {
                in_set = false;
                out.push(']');
            }
            '-' if !in_set => out.push_str("*?"),
            other => out.push(other),
        }
    }
    out
}

fn build_injections(language: &Language, name: &str) -> Option<Injections> {
    let src = with_inherits("injections", name)?;
    let query = match Query::new(language, &src) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("ASTROGRAPH_REALCE: injecoes de {name} nao compilam ({e}); sem injecoes");
            return None;
        }
    };
    let content = query.capture_index_for_name("injection.content")?;
    let language_cap = query.capture_index_for_name("injection.language");
    let n = query.pattern_count();
    let fixed = (0..n)
        .map(|i| {
            query
                .property_settings(i)
                .iter()
                .find(|p| &*p.key == "injection.language")
                .and_then(|p| p.value.as_deref().map(String::from))
        })
        .collect();
    let offset = (0..n)
        .map(|i| {
            query.general_predicates(i).iter().find(|p| &*p.operator == "offset!").and_then(|p| {
                let nums: Vec<isize> = p
                    .args
                    .iter()
                    .filter_map(|a| match a {
                        QueryPredicateArg::String(s) => s.parse().ok(),
                        QueryPredicateArg::Capture(_) => None,
                    })
                    .collect();
                (nums.len() == 4).then(|| [nums[0], nums[1], nums[2], nums[3]])
            })
        })
        .collect();
    Some(Injections { query, content, language: language_cap, fixed, offset })
}

fn build(name: &'static str) -> Option<Lang> {
    let (language, own_query) = language_of(name)?;
    let nvim = with_inherits("highlights", name).and_then(|src| match Query::new(&language, &src) {
        Ok(q) => Some(q),
        Err(e) => {
            eprintln!("ASTROGRAPH_REALCE: consulta do Neovim para {name} nao compila ({e}); usando a da gramatica");
            None
        }
    });
    let later_wins = nvim.is_some();
    let highlights = match nvim {
        Some(q) => q,
        None => Query::new(&language, own_query).expect("consulta de realce da gramatica"),
    };
    let priority = (0..highlights.pattern_count())
        .map(|i| {
            highlights
                .property_settings(i)
                .iter()
                .find(|p| &*p.key == "priority")
                .and_then(|p| p.value.as_deref()?.parse().ok())
                .unwrap_or(100)
        })
        .collect();
    let mut groups: Vec<String> = Vec::new();
    let group_of = highlights
        .capture_names()
        .iter()
        .map(|cap| {
            let modern = modern_capture(cap)?;
            let full = format!("@{modern}.{name}");
            let idx = groups.iter().position(|g| *g == full).unwrap_or_else(|| {
                groups.push(full);
                groups.len() - 1
            });
            Some(idx as u16)
        })
        .collect();
    let injections = build_injections(&language, name);
    let indents = with_inherits("indents", name).and_then(|src| match Query::new(&language, &src) {
        Ok(q) => Some(q),
        Err(e) => {
            eprintln!("ASTROGRAPH_REALCE: indent de {name} nao compila ({e}); sem indent em Rust");
            None
        }
    });
    let textobjects = with_inherits("textobjects", name).and_then(|src| match Query::new(&language, &src) {
        Ok(q) => Some(q),
        Err(e) => {
            eprintln!("ASTROGRAPH_REALCE: textobjects de {name} nao compilam ({e})");
            None
        }
    });
    Some(Lang { name, language, highlights, group_of, groups, later_wins, priority, injections, indents, textobjects })
}

/// Nome atual da captura (as consultas das gramaticas ainda usam alguns nomes
/// antigos, que o Neovim nao liga mais a cor nenhuma). `None`: nao realca.
fn modern_capture(cap: &str) -> Option<&str> {
    if cap.starts_with('_') {
        return None;
    }
    Some(match cap {
        "none" | "spell" | "nospell" | "conceal" | "embedded" | "local" => return None,
        "repeat" => "keyword.repeat",
        "conditional" => "keyword.conditional",
        "include" => "keyword.import",
        "exception" => "keyword.exception",
        "preproc" => "keyword.directive",
        "field" => "variable.member",
        "method" => "function.method",
        "method.call" => "function.method.call",
        "parameter" => "variable.parameter",
        "escape" => "string.escape",
        "symbol" => "string.special.symbol",
        other => other,
    })
}

static CACHE: [OnceLock<Option<Lang>>; 15] = [const { OnceLock::new() }; 15];

/// Linguagem pelo nome: o `filetype` do Neovim (`sh` -> bash) ou o nome que
/// aparece numa injecao (bloco de codigo: `js`, `py`...).
pub fn lang(name: &str) -> Option<&'static Lang> {
    let name = match name {
        "sh" | "shell" | "zsh" => "bash",
        "javascriptreact" | "js" | "jsx" | "node" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "typescriptreact" => "tsx",
        "jsonc" | "json5" => "json",
        "py" | "python3" => "python",
        "rs" => "rust",
        other => other,
    };
    let i = ALL.iter().position(|n| *n == name)?;
    CACHE[i].get_or_init(|| build(ALL[i])).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padroes_lua_viram_regex() {
        assert_eq!(lua_pattern_to_regex("^[A-Z][A-Z%d_]*$"), "^[A-Z][A-Z0-9_]*$");
        assert_eq!(lua_pattern_to_regex("^%u"), "^[A-Z]");
        assert_eq!(lua_pattern_to_regex("^[-][-](%s?)@"), "^[-][-]([[:space:]]?)@");
        assert_eq!(lua_pattern_to_regex("^[%l_].*$"), "^[a-z_].*$");
        assert_eq!(lua_pattern_to_regex("a.-b"), "a.*?b");
        assert_eq!(lua_pattern_to_regex("%."), "[.]");
    }

    #[test]
    fn consultas_do_neovim_compilam_em_todas() {
        for name in ALL {
            let l = lang(name).unwrap_or_else(|| panic!("{name}"));
            // vim: o crate (0.4.0) e mais antigo que a consulta do Neovim (falta
            // o no `tab`); fica com a consulta da propria gramatica
            if name != "vim" {
                assert!(l.later_wins, "{name}: caiu na consulta da gramatica");
            }
        }
        for name in LANG_NAMES {
            assert!(lang(name).unwrap().injections.is_some(), "{name}: sem injecoes");
            assert!(lang(name).unwrap().indents.is_some(), "{name}: sem indent");
            assert!(lang(name).unwrap().textobjects.is_some(), "{name}: sem textobjects");
        }
    }
}
