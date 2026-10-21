//! Tipo de arquivo pelo nome e pela primeira linha (shebang), sem rodar o
//! `filetype.lua` do Neovim (5,7 mil linhas de Lua avaliadas a cada arquivo).

use std::path::Path;

/// Nome da linguagem (ver `LANG_NAMES`) ou `None` quando nao ha gramatica.
pub fn detect(path: &str, first_line: &str) -> Option<&'static str> {
    let p = Path::new(path);
    let file = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let by_name = match file {
        "Cargo.lock" | "uv.lock" | "poetry.lock" | "Pipfile" => Some("toml"),
        ".bashrc" | ".bash_profile" | ".bash_aliases" | ".bash_logout" | ".profile" | "PKGBUILD" | "APKBUILD" => {
            Some("bash")
        }
        ".luarc.json" | "package.json" | "tsconfig.json" | ".eslintrc" | ".prettierrc" => Some("json"),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let by_ext = match ext.as_deref() {
        Some("lua") | Some("luau") | Some("rockspec") => Some("lua"),
        Some("rs") => Some("rust"),
        Some("py") | Some("pyi") | Some("pyw") => Some("python"),
        Some("json") | Some("jsonc") | Some("json5") | Some("geojson") => Some("json"),
        Some("sh") | Some("bash") | Some("ksh") | Some("ebuild") => Some("bash"),
        Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => Some("javascript"),
        Some("ts") | Some("mts") | Some("cts") => Some("typescript"),
        Some("tsx") => Some("tsx"),
        Some("toml") => Some("toml"),
        _ => None,
    };
    by_ext.or_else(|| shebang(first_line))
}

fn shebang(line: &str) -> Option<&'static str> {
    let rest = line.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let mut prog = words.next()?.rsplit('/').next()?;
    if prog == "env" {
        // `#!/usr/bin/env -S python3 -u`
        prog = words.find(|w| !w.starts_with('-'))?;
    }
    let prog = prog.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    match prog {
        "python" | "pypy" => Some("python"),
        "sh" | "bash" | "dash" | "ksh" => Some("bash"),
        "node" | "nodejs" | "deno" | "bun" => Some("javascript"),
        "lua" | "luajit" | "nvim" => Some("lua"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::detect;

    #[test]
    fn pelo_nome_extensao_e_shebang() {
        assert_eq!(detect("C:\\proj\\src\\main.rs", ""), Some("rust"));
        assert_eq!(detect("init.LUA", ""), Some("lua"));
        assert_eq!(detect("/x/Cargo.lock", ""), Some("toml"));
        assert_eq!(detect("/home/a/.bashrc", ""), Some("bash"));
        assert_eq!(detect("script", "#!/usr/bin/env -S python3 -u"), Some("python"));
        assert_eq!(detect("run", "#!/bin/bash -e"), Some("bash"));
        assert_eq!(detect("tool", "#!/usr/bin/env node"), Some("javascript"));
        assert_eq!(detect("app/api/route.ts", ""), Some("typescript"));
        assert_eq!(detect("a.mts", ""), Some("typescript"));
        assert_eq!(detect("App.tsx", ""), Some("tsx"));
        assert_eq!(detect("notas.txt", "texto"), None);
        assert_eq!(detect("sem-nada", ""), None);
    }
}
