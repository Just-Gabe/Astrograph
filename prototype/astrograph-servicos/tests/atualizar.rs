//! Atualizacao transacional com repositorios git de verdade (remoto bare local).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use astrograph_servicos::atualizar::{atualizar, Estado, Opcoes};

static N: AtomicUsize = AtomicUsize::new(0);

fn pasta_temp() -> PathBuf {
    let p = std::env::temp_dir().join(format!("astrograph-servicos-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// `file://` valido nos dois sistemas (no Windows: `file:///C:/...`).
fn url(p: &Path) -> String {
    let t = p.to_string_lossy().replace('\\', "/");
    if t.starts_with('/') {
        format!("file://{t}")
    } else {
        format!("file:///{t}")
    }
}

struct Mundo {
    base: PathBuf,
    bare: PathBuf,
    dev: PathBuf,
    n: usize,
}

impl Mundo {
    fn novo() -> Self {
        let base = pasta_temp();
        let bare = base.join("up.git");
        fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare", "-b", "main"]);
        git(&base, &["clone", "-q", &url(&bare), "dev"]);
        let dev = base.join("dev");
        fs::create_dir_all(dev.join("template")).unwrap();
        fs::write(dev.join("template/lazy-lock.json"), "{}").unwrap();
        fs::write(dev.join("a.txt"), "1").unwrap();
        fs::write(dev.join("b.txt"), "1").unwrap();
        git(&dev, &["checkout", "-q", "-b", "main"]);
        git(&dev, &["add", "-A"]);
        git(&dev, &["commit", "-q", "-m", "um"]);
        git(&dev, &["push", "-q", "origin", "main"]);
        Mundo { base, bare, dev, n: 1 }
    }
    fn publicar(&mut self, arq: &str, texto: &str) {
        fs::write(self.dev.join(arq), texto).unwrap();
        git(&self.dev, &["add", "-A"]);
        self.n += 1;
        git(&self.dev, &["commit", "-q", "-m", &format!("c{}", self.n)]);
        git(&self.dev, &["push", "-q", "-f", "origin", "main"]);
    }
    fn clonar(&self, nome: &str, raso: bool) -> PathBuf {
        let url = url(&self.bare);
        let mut args = vec!["clone", "-q"];
        if raso {
            args.push("--depth=1");
        } else {
            args.push("--no-local");
        }
        args.push(&url);
        args.push(nome);
        git(&self.base, &args);
        let d = self.base.join(nome);
        fs::create_dir_all(d.join("bin")).unwrap();
        d
    }
}

fn opcoes(raiz: &Path) -> Opcoes {
    let mut o = Opcoes::nova(raiz);
    o.ramo = Some("main".into());
    o.restaurar_plugins = false;
    o.recompilar = false;
    o.verificar = false;
    o
}

fn ler(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

#[test]
fn avanco_limpo_em_clone_raso_e_depois_ja_atualizado() {
    let mut m = Mundo::novo();
    let c = m.clonar("raso", true);
    m.publicar("a.txt", "2");
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::Atualizado, "{}", r.mensagem);
    assert_eq!(ler(&c.join("a.txt")), "2");
    assert_eq!(git(&c, &["rev-parse", "HEAD"]), r.depois.unwrap());
    assert_eq!(atualizar(&opcoes(&c)).status, Estado::JaAtualizado);
}

#[test]
fn alteracao_do_usuario_em_outro_arquivo_volta() {
    let mut m = Mundo::novo();
    let c = m.clonar("c", true);
    fs::write(c.join("b.txt"), "meu").unwrap();
    m.publicar("a.txt", "3");
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::Atualizado, "{}", r.mensagem);
    assert_eq!(ler(&c.join("a.txt")), "3");
    assert_eq!(ler(&c.join("b.txt")), "meu");
    assert_eq!(git(&c, &["stash", "list"]), "");
}

#[test]
fn conflito_deixa_a_versao_nova_limpa_e_a_alteracao_no_stash() {
    let mut m = Mundo::novo();
    let c = m.clonar("c", true);
    fs::write(c.join("a.txt"), "meu a").unwrap();
    m.publicar("a.txt", "4");
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::Atualizado, "{}", r.mensagem);
    assert_eq!(ler(&c.join("a.txt")), "4");
    assert!(r.stash.is_some());
    assert!(git(&c, &["stash", "list"]).contains("astrograph-update"));
    assert_eq!(git(&c, &["status", "--porcelain"]), "");
}

#[test]
fn arquivo_solto_no_caminho_de_um_arquivo_novo() {
    let mut m = Mundo::novo();
    let c = m.clonar("c", true);
    fs::write(c.join("novo.txt"), "solto").unwrap();
    m.publicar("novo.txt", "do upstream");
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::Atualizado, "{}", r.mensagem);
    assert_eq!(ler(&c.join("novo.txt")), "do upstream");
    assert_ne!(git(&c, &["stash", "list"]), "");
}

#[test]
fn historico_reescrito_em_clone_raso_segue_com_ponto_de_restauracao() {
    let mut m = Mundo::novo();
    m.publicar("a.txt", "2");
    m.publicar("a.txt", "3");
    let c = m.clonar("c", true);
    git(&m.dev, &["reset", "-q", "--hard", "HEAD~2"]);
    m.publicar("a.txt", "reescrito");
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::Atualizado, "{}", r.mensagem);
    assert_eq!(ler(&c.join("a.txt")), "reescrito");
    assert!(git(&c, &["branch", "--list", "astrograph-backup-*"]).contains("astrograph-backup-"));
}

#[test]
fn clone_de_desenvolvimento_com_commit_proprio_recusa() {
    let mut m = Mundo::novo();
    let c = m.clonar("completo", false);
    fs::write(c.join("mine.txt"), "meu commit").unwrap();
    git(&c, &["add", "-A"]);
    git(&c, &["commit", "-q", "-m", "meu"]);
    let meu = git(&c, &["rev-parse", "HEAD"]);
    m.publicar("a.txt", "outro");
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::Divergiu, "{}", r.mensagem);
    assert_eq!(git(&c, &["rev-parse", "HEAD"]), meu);
}

#[test]
fn sem_rede_nao_muda_nada() {
    let m = Mundo::novo();
    let c = m.clonar("off", true);
    git(&c, &["remote", "set-url", "origin", "http://127.0.0.1:9/x.git"]);
    let antes = git(&c, &["rev-parse", "HEAD"]);
    let r = atualizar(&opcoes(&c));
    assert_eq!(r.status, Estado::SemRede, "{}", r.mensagem);
    assert_eq!(git(&c, &["rev-parse", "HEAD"]), antes);
}

#[test]
fn trava_ativa_segura_e_trava_velha_nao() {
    let mut m = Mundo::novo();
    let c = m.clonar("trava", true);
    m.publicar("a.txt", "para a trava");
    let trava = c.join(".git/astrograph-update.lock");
    fs::write(&trava, "123").unwrap();
    assert_eq!(atualizar(&opcoes(&c)).status, Estado::Ocupado);
    let velho = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 3600);
    fs::File::options().write(true).open(&trava).unwrap().set_modified(velho).unwrap();
    assert_eq!(atualizar(&opcoes(&c)).status, Estado::Atualizado);
    assert!(!trava.exists(), "trava solta no fim");
}

#[test]
fn pasta_sem_git() {
    let p = pasta_temp();
    assert_eq!(atualizar(&opcoes(&p)).status, Estado::SemGit);
}

#[cfg(unix)]
fn nvim_falso(dir: &Path, bom: bool) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(if bom { "nvim-bom" } else { "nvim-ruim" });
    let corpo = if bom {
        "#!/bin/sh\nprintf ASTROGRAPH_OK\n"
    } else {
        "#!/bin/sh\necho \"E5113: Error while calling lua chunk: module 'x' not found\" >&2\n"
    };
    fs::write(&p, corpo).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p.to_string_lossy().into_owned()
}

#[cfg(unix)]
#[test]
fn versao_que_nao_sobe_volta_ao_que_estava_com_a_alteracao_do_usuario() {
    let mut m = Mundo::novo();
    let c = m.clonar("verifica", true);
    let antes = git(&c, &["rev-parse", "HEAD"]);
    fs::write(c.join("b.txt"), "editado").unwrap();
    m.publicar("a.txt", "quebra");
    let mut o = opcoes(&c);
    o.verificar = true;
    o.nvim = nvim_falso(&m.base, false);
    let r = atualizar(&o);
    assert_eq!(r.status, Estado::Revertido, "{}", r.mensagem);
    assert_eq!(git(&c, &["rev-parse", "HEAD"]), antes);
    assert_eq!(ler(&c.join("b.txt")), "editado");
    assert!(r.mensagem.contains("module 'x' not found"));
    // corrigido: atualiza
    o.nvim = nvim_falso(&m.base, true);
    assert_eq!(atualizar(&o).status, Estado::Atualizado);
    assert_eq!(ler(&c.join("a.txt")), "quebra");
}
