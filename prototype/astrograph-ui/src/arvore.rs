//! Explorador de arquivos nativo: árvore de pastas que abre e fecha, ordenada
//! (pastas primeiro) e sem o que o `.gitignore` ou o nome oculto escondem.
//! Substitui o neo-tree.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct No {
    pub caminho: PathBuf,
    pub nome: String,
    pub nivel: usize,
    pub pasta: bool,
    pub aberta: bool,
}

pub struct Arvore {
    pub raiz: PathBuf,
    abertas: HashSet<PathBuf>,
    pub nos: Vec<No>,
    pub sel: usize,
}

/// Filhos diretos de `dir`: pastas primeiro, depois arquivos, cada grupo em ordem alfabética.
fn filhos(dir: &Path) -> Vec<(PathBuf, String, bool)> {
    let mut v: Vec<(PathBuf, String, bool)> = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .follow_links(false)
        .build()
        .flatten()
        .filter(|e| e.depth() == 1)
        .map(|e| {
            let pasta = e.file_type().is_some_and(|t| t.is_dir());
            (e.path().to_path_buf(), e.file_name().to_string_lossy().into_owned(), pasta)
        })
        .collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    v
}

impl Arvore {
    pub fn nova(raiz: &Path) -> Self {
        let mut a = Arvore { raiz: raiz.to_path_buf(), abertas: HashSet::new(), nos: Vec::new(), sel: 0 };
        a.recarregar();
        a
    }

    /// Relê o disco mantendo o que estava aberto e o item selecionado.
    pub fn recarregar(&mut self) {
        let atual = self.nos.get(self.sel).map(|n| n.caminho.clone());
        let mut nos = Vec::new();
        self.expandir(&self.raiz.clone(), 0, &mut nos);
        self.nos = nos;
        self.sel = atual.and_then(|c| self.nos.iter().position(|n| n.caminho == c)).unwrap_or(0).min(self.nos.len().saturating_sub(1));
    }

    fn expandir(&self, dir: &Path, nivel: usize, out: &mut Vec<No>) {
        for (caminho, nome, pasta) in filhos(dir) {
            let aberta = pasta && self.abertas.contains(&caminho);
            out.push(No { caminho: caminho.clone(), nome, nivel, pasta, aberta });
            if aberta {
                self.expandir(&caminho, nivel + 1, out);
            }
        }
    }

    pub fn mover(&mut self, d: i64) {
        if !self.nos.is_empty() {
            self.sel = (self.sel as i64 + d).clamp(0, self.nos.len() as i64 - 1) as usize;
        }
    }

    pub fn atual(&self) -> Option<&No> {
        self.nos.get(self.sel)
    }

    /// Abre/fecha a pasta selecionada (arquivo: nada). Devolve se mudou algo.
    pub fn alternar(&mut self) -> bool {
        let Some(n) = self.atual().cloned() else { return false };
        if !n.pasta {
            return false;
        }
        if !self.abertas.remove(&n.caminho) {
            self.abertas.insert(n.caminho);
        }
        self.recarregar();
        true
    }

    /// `h`: fecha a pasta aberta; senão vai para a pasta que contém o item.
    pub fn recuar(&mut self) {
        let Some(n) = self.atual().cloned() else { return };
        if n.pasta && n.aberta {
            self.abertas.remove(&n.caminho);
            self.recarregar();
            return;
        }
        if let Some(pai) = n.caminho.parent() {
            if let Some(i) = self.nos.iter().position(|x| x.caminho == pai) {
                self.sel = i;
            }
        }
    }

    /// `l`: abre a pasta (já aberta: desce para o primeiro filho).
    pub fn avancar(&mut self) {
        let Some(n) = self.atual().cloned() else { return };
        if !n.pasta {
            return;
        }
        if !n.aberta {
            self.alternar();
        } else if self.sel + 1 < self.nos.len() && self.nos[self.sel + 1].nivel > n.nivel {
            self.sel += 1;
        }
    }

    /// Abre as pastas até `arquivo` e o seleciona (para "revelar" o arquivo atual).
    pub fn revelar(&mut self, arquivo: &Path) {
        let Ok(rel) = arquivo.strip_prefix(&self.raiz) else { return };
        let mut p = self.raiz.clone();
        let comps: Vec<_> = rel.components().collect();
        for c in &comps[..comps.len().saturating_sub(1)] {
            p.push(c);
            self.abertas.insert(p.clone());
        }
        self.recarregar();
        if let Some(i) = self.nos.iter().position(|n| n.caminho == arquivo) {
            self.sel = i;
        }
    }
}

/// Operações de arquivo do explorador (`a`, `r`, `d`, `y`/`x`, `p`).
pub mod ops {
    use std::io;
    use std::path::{Path, PathBuf};

    /// Cria um arquivo (ou uma pasta, se `nome` termina em `/`) dentro de `dir`; cria as pastas do caminho.
    pub fn criar(dir: &Path, nome: &str) -> io::Result<PathBuf> {
        let nome = nome.trim();
        if nome.is_empty() || nome.starts_with('/') || nome.split('/').any(|c| c == "..") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "nome inválido"));
        }
        let destino = dir.join(nome.trim_end_matches('/'));
        if destino.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "já existe"));
        }
        if nome.ends_with('/') {
            std::fs::create_dir_all(&destino)?;
        } else {
            if let Some(pai) = destino.parent() {
                std::fs::create_dir_all(pai)?;
            }
            std::fs::File::create(&destino)?;
        }
        Ok(destino)
    }

    pub fn renomear(de: &Path, novo_nome: &str) -> io::Result<PathBuf> {
        let novo_nome = novo_nome.trim();
        if novo_nome.is_empty() || novo_nome.contains('/') || novo_nome.contains('\\') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "nome inválido"));
        }
        let para = de.with_file_name(novo_nome);
        if para.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "já existe"));
        }
        std::fs::rename(de, &para)?;
        Ok(para)
    }

    pub fn apagar(alvo: &Path) -> io::Result<()> {
        if alvo.is_dir() {
            std::fs::remove_dir_all(alvo)
        } else {
            std::fs::remove_file(alvo)
        }
    }

    fn copiar_rec(de: &Path, para: &Path) -> io::Result<()> {
        if de.is_dir() {
            std::fs::create_dir_all(para)?;
            for e in std::fs::read_dir(de)? {
                let e = e?;
                copiar_rec(&e.path(), &para.join(e.file_name()))?;
            }
            Ok(())
        } else {
            std::fs::copy(de, para).map(|_| ())
        }
    }

    /// Nome livre em `dir` para colar `origem` (acrescenta `_copia`, `_copia2`... se já existir).
    fn destino_livre(dir: &Path, origem: &Path) -> PathBuf {
        let nome = origem.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let direto = dir.join(&nome);
        if !direto.exists() {
            return direto;
        }
        let (base, ext) = match nome.rsplit_once('.') {
            Some((b, e)) if !b.is_empty() => (b.to_string(), format!(".{e}")),
            _ => (nome.clone(), String::new()),
        };
        for n in 1.. {
            let sufixo = if n == 1 { "_copia".to_string() } else { format!("_copia{n}") };
            let c = dir.join(format!("{base}{sufixo}{ext}"));
            if !c.exists() {
                return c;
            }
        }
        unreachable!()
    }

    /// Cola `origem` dentro de `dir` (copiando, ou movendo se `mover`).
    pub fn colar(origem: &Path, dir: &Path, mover: bool) -> io::Result<PathBuf> {
        if dir.starts_with(origem) && origem.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "não dá para colar uma pasta dentro dela mesma"));
        }
        let para = if mover { dir.join(origem.file_name().unwrap_or_default()) } else { destino_livre(dir, origem) };
        if mover {
            if para == origem {
                return Ok(para);
            }
            if para.exists() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "já existe"));
            }
            if std::fs::rename(origem, &para).is_err() {
                copiar_rec(origem, &para)?;
                apagar(origem)?;
            }
        } else {
            copiar_rec(origem, &para)?;
        }
        Ok(para)
    }
}

impl Arvore {
    /// Pasta onde um item novo entra: a pasta selecionada (se aberta ou não) ou a que contém o arquivo.
    pub fn pasta_alvo(&self) -> PathBuf {
        match self.atual() {
            Some(n) if n.pasta => n.caminho.clone(),
            Some(n) => n.caminho.parent().map(Path::to_path_buf).unwrap_or_else(|| self.raiz.clone()),
            None => self.raiz.clone(),
        }
    }

    /// Abre a pasta (para mostrar o que acabou de entrar nela) e seleciona `caminho`.
    pub fn mostrar(&mut self, caminho: &Path) {
        let mut pai = caminho.parent();
        while let Some(p) = pai {
            if p == self.raiz || !p.starts_with(&self.raiz) {
                break;
            }
            self.abertas.insert(p.to_path_buf());
            pai = p.parent();
        }
        self.recarregar();
        if let Some(i) = self.nos.iter().position(|n| n.caminho == caminho) {
            self.sel = i;
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn projeto(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-arv-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("src/mod")).unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(d.join("src/b.rs"), "").unwrap();
        std::fs::write(d.join("src/A.rs"), "").unwrap();
        std::fs::write(d.join("src/mod/x.rs"), "").unwrap();
        std::fs::write(d.join("LEIA.md"), "").unwrap();
        d
    }

    fn nomes(a: &Arvore) -> Vec<String> {
        a.nos.iter().map(|n| format!("{}{}{}", "  ".repeat(n.nivel), n.nome, if n.pasta { "/" } else { "" })).collect()
    }

    #[test]
    fn pastas_primeiro_sem_ignorados_e_abre_fecha() {
        let d = projeto("a");
        let mut a = Arvore::nova(&d);
        assert_eq!(nomes(&a), vec!["src/", "LEIA.md"]);
        a.alternar();
        assert_eq!(nomes(&a), vec!["src/", "  mod/", "  A.rs", "  b.rs", "LEIA.md"]);
        a.mover(1);
        a.avancar();
        assert_eq!(nomes(&a)[2], "    x.rs");
        a.recuar();
        assert_eq!(a.atual().unwrap().nome, "mod", "h fecha a pasta");
        a.recuar();
        assert_eq!(a.atual().unwrap().nome, "src", "h de novo vai para a pasta pai");
    }

    #[test]
    fn recarregar_mantem_a_selecao_e_revelar_abre_o_caminho() {
        let d = projeto("b");
        let mut a = Arvore::nova(&d);
        a.revelar(&d.join("src/mod/x.rs"));
        assert_eq!(a.atual().unwrap().nome, "x.rs");
        std::fs::write(d.join("src/mod/y.rs"), "").unwrap();
        a.recarregar();
        assert_eq!(a.atual().unwrap().nome, "x.rs");
        assert_eq!(a.nos.iter().filter(|n| n.nome.ends_with(".rs")).count(), 4);
    }

    #[test]
    fn operacoes_de_arquivo() {
        let d = projeto("ops");
        let novo = ops::criar(&d, "a/b/c.txt").unwrap();
        assert!(novo.is_file());
        assert!(ops::criar(&d, "pasta/").unwrap().is_dir());
        assert!(ops::criar(&d, "a/b/c.txt").is_err(), "já existe");
        assert!(ops::criar(&d, "../fora").is_err());
        let r = ops::renomear(&novo, "d.txt").unwrap();
        assert!(r.is_file() && !novo.exists());
        assert!(ops::renomear(&r, "x/y").is_err());
        let c1 = ops::colar(&d.join("LEIA.md"), &d, false).unwrap();
        assert_eq!(c1.file_name().unwrap(), "LEIA_copia.md");
        let c2 = ops::colar(&d.join("LEIA.md"), &d, false).unwrap();
        assert_eq!(c2.file_name().unwrap(), "LEIA_copia2.md");
        let m = ops::colar(&d.join("LEIA.md"), &d.join("pasta"), true).unwrap();
        assert!(m.is_file() && !d.join("LEIA.md").exists());
        let pasta = ops::colar(&d.join("src"), &d.join("pasta"), false).unwrap();
        assert!(pasta.join("mod/x.rs").is_file());
        assert!(ops::colar(&d.join("src"), &d.join("src/mod"), false).is_err());
        ops::apagar(&d.join("pasta")).unwrap();
        assert!(!d.join("pasta").exists());
        let mut a = Arvore::nova(&d);
        a.mostrar(&d.join("a/b/d.txt"));
        assert_eq!(a.atual().unwrap().nome, "d.txt");
    }
}
