//! Deteccao do projeto pela raiz (barata: so olha a pasta, nao varre).

use std::path::Path;

use serde::Serialize;

use crate::proc::{git, LIMITE_GIT};

/// arquivo/pasta que revela a linguagem -> parsers do Treesitter
const MARCADORES: &[(&[&str], &[&str])] = &[
    (&["Cargo.toml"], &["rust", "toml"]),
    (&["package.json"], &["javascript", "json"]),
    (&["tsconfig.json"], &["typescript", "json"]),
    (&["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"], &["python", "toml"]),
    (&["go.mod"], &["go", "gomod"]),
    (&[".luarc.json", "stylua.toml", ".stylua.toml", "init.lua", "lua"], &["lua"]),
    (&[".github", "docker-compose.yml", "docker-compose.yaml"], &["yaml"]),
    (&["Makefile", "justfile"], &["bash", "make"]),
];

/// linguagem -> pacotes do Mason (servidor/formatador) que o projeto costuma pedir
const MASON: &[(&str, &[&str])] = &[
    ("rust", &["rust-analyzer"]),
    ("python", &["basedpyright"]),
    ("javascript", &["typescript-language-server"]),
    ("typescript", &["typescript-language-server"]),
    ("go", &["gopls"]),
    ("json", &["json-lsp"]),
    ("yaml", &["yaml-language-server"]),
    ("toml", &["taplo"]),
    ("bash", &["bash-language-server"]),
];

/// parsers pequenos que valem em qualquer repositorio git
pub const LINGUAGENS_GIT: &[&str] = &["diff", "git_config", "git_rebase", "gitattributes", "gitcommit", "gitignore"];

pub fn detectar(raiz: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (arquivos, langs) in MARCADORES {
        if arquivos.iter().any(|a| raiz.join(a).exists()) {
            for l in *langs {
                if !out.iter().any(|x| x == l) {
                    out.push((*l).to_string());
                }
            }
        }
    }
    out
}

pub fn pacotes_mason(langs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in langs {
        for (nome, pacotes) in MASON {
            if nome == l {
                for p in *pacotes {
                    if !out.iter().any(|x| x == p) {
                        out.push((*p).to_string());
                    }
                }
            }
        }
    }
    out
}

/// Home ou raiz do disco: nao vale varrer nem rodar git.
pub fn muito_larga(raiz: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    let r = norm(raiz);
    if r.is_empty() || (r.len() == 2 && r.ends_with(':')) {
        return true;
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| norm(Path::new(&h)));
    home.is_some_and(|h| h == r)
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct InfoGit {
    pub raiz_git: String,
    pub ramo: Option<String>,
    pub head: Option<String>,
    pub alteracoes: usize,
    pub refs: usize,
    pub ultimo_commit: Option<i64>,
}

/// Resultado do resumo do git: `None` fora de repositorio; `Err` se o git falta.
pub fn resumo_git(raiz: &Path) -> Result<Option<InfoGit>, String> {
    let topo = git(raiz, &["rev-parse", "--show-toplevel"], LIMITE_GIT);
    if topo.codigo == -1 && topo.stderr.contains("nao consegui iniciar") {
        return Err("git nao instalado".into());
    }
    if !topo.ok() {
        return Ok(None);
    }
    let mut info = InfoGit { raiz_git: topo.texto(), ..Default::default() };
    // so leitura: indice e refs ficam quentes, nada e escrito nem vai a rede
    let status = git(raiz, &["status", "--porcelain=v1", "-b"], LIMITE_GIT);
    let mut linhas = status.stdout.lines();
    if let Some(cab) = linhas.next() {
        info.ramo = cab
            .strip_prefix("## No commits yet on ")
            .or_else(|| cab.strip_prefix("## Initial commit on "))
            .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
            .or_else(|| {
                cab.strip_prefix("## ").map(|s| s.split(|c: char| c == '.' || c.is_whitespace()).next().unwrap_or("").to_string())
            })
            .filter(|s| !s.is_empty());
    }
    info.alteracoes = linhas.filter(|l| !l.trim().is_empty()).count();
    let head = git(raiz, &["rev-parse", "--short", "HEAD"], LIMITE_GIT);
    info.head = head.ok().then(|| head.texto());
    let refs = git(raiz, &["for-each-ref", "--count=500", "--format=%(refname)", "refs/heads", "refs/remotes"], LIMITE_GIT);
    info.refs = refs.stdout.lines().filter(|l| !l.trim().is_empty()).count();
    let log = git(raiz, &["log", "-1", "--format=%ct"], LIMITE_GIT);
    info.ultimo_commit = log.texto().parse().ok();
    Ok(Some(info))
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn temp(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("astrograph-projeto-{}-{nome}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn detecta_pela_raiz_sem_repetir() {
        let p = temp("detecta");
        fs::write(p.join("Cargo.toml"), "").unwrap();
        fs::write(p.join("package.json"), "{}").unwrap();
        fs::write(p.join("init.lua"), "").unwrap();
        let l = detectar(&p);
        for esperado in ["rust", "toml", "javascript", "json", "lua"] {
            assert!(l.iter().any(|x| x == esperado), "{esperado} em {l:?}");
        }
        assert!(!l.iter().any(|x| x == "python"));
        let mut unicos = l.clone();
        unicos.dedup();
        assert_eq!(unicos.len(), l.len());
        let pk = pacotes_mason(&["rust".into(), "typescript".into(), "javascript".into(), "lua".into()]);
        assert_eq!(pk, vec!["rust-analyzer", "typescript-language-server"]);
    }

    #[test]
    fn home_e_raiz_do_disco_sao_largas() {
        assert!(muito_larga(Path::new("C:\\")));
        assert!(muito_larga(Path::new("/")));
        assert!(!muito_larga(&temp("larga")));
    }

    fn git_cmd(p: &Path, args: &[&str]) {
        let st = Command::new("git").arg("-C").arg(p).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).status().unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn resumo_do_git_inclusive_sem_commits_e_fora_de_repo() {
        let p = temp("git");
        assert_eq!(resumo_git(&p).unwrap(), None, "fora de repositorio");
        git_cmd(&p, &["init", "-q", "-b", "principal"]);
        fs::write(p.join("a.txt"), "x").unwrap();
        let novo = resumo_git(&p).unwrap().unwrap();
        assert_eq!(novo.ramo.as_deref(), Some("principal"), "ramo sem commits");
        assert_eq!(novo.alteracoes, 1);
        git_cmd(&p, &["add", "-A"]);
        git_cmd(&p, &["commit", "-q", "-m", "i"]);
        let r = resumo_git(&p).unwrap().unwrap();
        assert_eq!(r.ramo.as_deref(), Some("principal"));
        assert_eq!(r.alteracoes, 0);
        assert!(r.head.is_some() && r.refs >= 1 && r.ultimo_commit.is_some());
    }
}
