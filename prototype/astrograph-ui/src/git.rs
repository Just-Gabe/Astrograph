//! Git nativo do editor: branch e sinais de margem (linha adicionada, alterada,
//! removida) comparando o buffer ao que está em `HEAD`. Usa o `git` instalado só
//! para ler o branch e o conteúdo do `HEAD`; a comparação é em Rust (`similar`),
//! então os sinais acompanham o que você digita, sem salvar.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use similar::{ChangeTag, TextDiff};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Added,
    Changed,
    /// linhas apagadas logo antes desta linha (ou no fim do arquivo)
    Removed,
}

fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git").args(args).current_dir(dir).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Raiz do repositório que contém `dir`.
pub fn raiz(dir: &Path) -> Option<PathBuf> {
    let o = git(dir, &["rev-parse", "--show-toplevel"])?;
    Some(PathBuf::from(String::from_utf8_lossy(&o).trim()))
}

/// Nome do branch (ou os 7 primeiros caracteres do commit, com HEAD solto).
pub fn branch(dir: &Path) -> Option<String> {
    let o = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let nome = String::from_utf8_lossy(&o).trim().to_string();
    if nome == "HEAD" {
        let c = git(dir, &["rev-parse", "--short=7", "HEAD"])?;
        return Some(String::from_utf8_lossy(&c).trim().to_string());
    }
    (!nome.is_empty()).then_some(nome)
}

/// Linhas do arquivo em `HEAD`. `None`: fora de um repositório ou sem commit; `Some(vazio)` não
/// existe (arquivo novo é `Some(vec![])`, tudo vira "adicionado").
pub fn base(arquivo: &Path) -> Option<Vec<String>> {
    let dir = arquivo.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let raiz = raiz(dir)?;
    let abs = if arquivo.is_absolute() { arquivo.to_path_buf() } else { std::env::current_dir().ok()?.join(arquivo) };
    let abs = std::fs::canonicalize(&abs).unwrap_or(abs);
    let raiz_c = std::fs::canonicalize(&raiz).unwrap_or(raiz);
    let rel = abs.strip_prefix(&raiz_c).ok()?.to_string_lossy().replace('\\', "/");
    // `HEAD` ainda não existe num repositório recém-criado: tudo é novo
    match git(&raiz_c, &["show", &format!("HEAD:{rel}")]) {
        Some(bytes) => {
            let t = String::from_utf8_lossy(&bytes).into_owned();
            Some(t.strip_suffix('\n').unwrap_or(&t).split('\n').map(str::to_string).collect::<Vec<_>>()).map(|v| if v == [""] { Vec::new() } else { v })
        }
        None => Some(Vec::new()),
    }
}

/// Sinais por linha do buffer (índice base 0), comparando `base` com `atual`.
pub fn sinais(base: &[String], atual: &[String]) -> Vec<(usize, Sign)> {
    let a: Vec<&str> = base.iter().map(String::as_str).collect();
    let b: Vec<&str> = atual.iter().map(String::as_str).collect();
    let diff = TextDiff::from_slices(&a, &b);
    let mut out: Vec<(usize, Sign)> = Vec::new();
    for op in diff.ops() {
        use similar::DiffOp::*;
        match *op {
            Equal { .. } => {}
            Insert { new_index, new_len, .. } => out.extend((new_index..new_index + new_len).map(|r| (r, Sign::Added))),
            Delete { new_index, .. } => out.push((new_index.min(b.len().saturating_sub(1)), Sign::Removed)),
            Replace { old_len, new_index, new_len, .. } => {
                out.extend((new_index..new_index + new_len).map(|r| (r, Sign::Changed)));
                // substituiu mais linhas do que escreveu: sobra um trecho removido
                if old_len > new_len {
                    out.push(((new_index + new_len).min(b.len().saturating_sub(1)), Sign::Removed));
                }
            }
        }
    }
    let _ = ChangeTag::Equal;
    out.sort_by_key(|&(r, _)| r);
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn sinais_de_adicionada_alterada_e_removida() {
        let base = v(&["a", "b", "c", "d", "e"]);
        // b -> B (alterada), c apagada, f nova no fim
        let atual = v(&["a", "B", "d", "e", "f"]);
        let s = sinais(&base, &atual);
        assert!(s.contains(&(1, Sign::Changed)), "{s:?}");
        assert!(s.iter().any(|&(r, k)| k == Sign::Removed && r == 2), "{s:?}");
        assert!(s.contains(&(4, Sign::Added)), "{s:?}");
        assert!(sinais(&base, &base).is_empty());
        // arquivo novo: tudo adicionado
        assert_eq!(sinais(&[], &v(&["x", "y"])), vec![(0, Sign::Added), (1, Sign::Added)]);
    }

    fn repo(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-git-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let run = |args: &[&str]| {
            assert!(Command::new("git").args(args).current_dir(&d).output().unwrap().status.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "principal"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(d.join("f.txt"), "um\ndois\ntres\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "inicial"]);
        d
    }

    #[test]
    fn branch_e_base_do_head() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let d = repo("b");
        assert_eq!(branch(&d).as_deref(), Some("principal"));
        assert_eq!(base(&d.join("f.txt")), Some(v(&["um", "dois", "tres"])));
        // arquivo não rastreado: base vazia (tudo adicionado)
        std::fs::write(d.join("novo.txt"), "x\n").unwrap();
        assert_eq!(base(&d.join("novo.txt")), Some(vec![]));
        // fora de um repositório
        let fora = std::env::temp_dir().join(format!("astrograph-fora-{}", std::process::id()));
        std::fs::create_dir_all(&fora).unwrap();
        assert_eq!(branch(&fora), None);
    }
}
