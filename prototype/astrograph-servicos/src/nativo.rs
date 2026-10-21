//! Executaveis em Rust do Astrograph (`<clone>/bin`): compila o que falta ou mudou.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::proc::{git, rodar, LIMITE_GIT};

pub struct Alvo {
    pub crate_: &'static str,
    pub bin: &'static str,
}

pub const ALVOS: &[Alvo] = &[
    Alvo { crate_: "astrograph-agent", bin: "astrograph-agent" },
    Alvo { crate_: "astrograph-ui", bin: "astrograph" },
    Alvo { crate_: "astrograph-servicos", bin: "astrograph-servicos" },
];

pub fn exe(nome: &str) -> String {
    if cfg!(windows) {
        format!("{nome}.exe")
    } else {
        nome.to_string()
    }
}

pub fn destino(raiz: &Path, alvo: &Alvo) -> PathBuf {
    raiz.join("bin").join(exe(alvo.bin))
}

/// Troca o executavel sem apagar o que esta aberto: o antigo vira `.old` (no
/// Windows renomear um .exe em uso funciona; sobrescrever, nao).
pub fn trocar_binario(novo: &Path, alvo: &Path) -> bool {
    let velho = PathBuf::from(format!("{}.old", alvo.display()));
    let _ = fs::remove_file(&velho);
    let tinha = alvo.exists();
    if tinha && fs::rename(alvo, &velho).is_err() {
        return false;
    }
    if fs::rename(novo, alvo).is_err() {
        if tinha {
            let _ = fs::rename(&velho, alvo);
        }
        return false;
    }
    true
}

fn marca_falha(raiz: &Path, alvo: &Alvo) -> PathBuf {
    raiz.join("bin").join(format!(".falhou-{}", alvo.crate_))
}

fn commit_atual(raiz: &Path) -> String {
    git(raiz, &["rev-parse", "--short", "HEAD"], LIMITE_GIT).texto()
}

/// Pastas que entram no build de `crate_`: ela mesma e as crates locais de que depende
/// (`path = "../x"` no Cargo.toml, em qualquer nivel). Sem isso, mudar o motor
/// (`astrograph-core`) nao recompilaria o app.
pub fn pastas_do_build(raiz: &Path, crate_: &str) -> Vec<String> {
    let mut fila = vec![crate_.to_string()];
    let mut vistas: Vec<String> = Vec::new();
    while let Some(c) = fila.pop() {
        if vistas.contains(&c) {
            continue;
        }
        let toml = fs::read_to_string(raiz.join("prototype").join(&c).join("Cargo.toml")).unwrap_or_default();
        for linha in toml.lines() {
            let Some(i) = linha.find("path") else { continue };
            let resto = linha[i + 4..].trim_start();
            let Some(resto) = resto.strip_prefix('=') else { continue };
            let Some(resto) = resto.trim_start().strip_prefix('"') else { continue };
            let Some(f) = resto.find('"') else { continue };
            if let Some(nome) = resto[..f].strip_prefix("../") {
                fila.push(nome.trim_end_matches('/').to_string());
            }
        }
        vistas.push(c);
    }
    vistas
}

/// O que precisa de build: falta o executavel (e a ultima falha nao foi neste
/// commit) ou o codigo da crate mudou entre `de` e `para`.
pub fn precisam(raiz: &Path, mudanca: Option<(&str, &str)>) -> Vec<&'static Alvo> {
    let commit = commit_atual(raiz);
    ALVOS
        .iter()
        .filter(|a| raiz.join("prototype").join(a.crate_).join("Cargo.toml").exists())
        .filter(|a| {
            let existe = destino(raiz, a).exists();
            match mudanca {
                Some((de, para)) if existe => {
                    // 1 = mudou; erro (commit sumiu) = assume que mudou
                    let mut args: Vec<String> = ["diff", "--quiet", de, para, "--"].iter().map(|s| s.to_string()).collect();
                    args.extend(pastas_do_build(raiz, a.crate_).iter().map(|c| format!("prototype/{c}")));
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    git(raiz, &args, LIMITE_GIT).codigo != 0
                }
                _ => {
                    let falhou = fs::read_to_string(marca_falha(raiz, a)).map(|s| s.trim() == commit).unwrap_or(false);
                    !existe && !falhou
                }
            }
        })
        .collect()
}

pub fn compilar_um(raiz: &Path, alvo: &Alvo) -> bool {
    let manifest = raiz.join("prototype").join(alvo.crate_).join("Cargo.toml");
    let m = manifest.to_string_lossy().into_owned();
    let (prog, args): (&str, Vec<&str>) = if !cfg!(windows) && rodar("nice", &["--version"], None, &[], Duration::from_secs(5)).ok() {
        ("nice", vec!["-n", "19", "cargo", "build", "--release", "--manifest-path", &m])
    } else {
        ("cargo", vec!["build", "--release", "--manifest-path", &m])
    };
    let b = rodar(prog, &args, None, &[], Duration::from_secs(30 * 60));
    let construido = raiz.join("prototype").join(alvo.crate_).join("target").join("release").join(exe(alvo.crate_));
    let alvo_bin = destino(raiz, alvo);
    let novo = PathBuf::from(format!("{}.new", alvo_bin.display()));
    let _ = fs::create_dir_all(alvo_bin.parent().unwrap());
    let ok = b.ok() && fs::copy(&construido, &novo).is_ok() && trocar_binario(&novo, &alvo_bin);
    if !ok {
        let _ = fs::remove_file(&novo);
        let _ = fs::write(marca_falha(raiz, alvo), commit_atual(raiz));
    } else {
        let _ = fs::remove_file(marca_falha(raiz, alvo));
    }
    ok
}

/// Compila o que precisa (um por vez, prioridade baixa). Sem `cargo`, nada.
/// Devolve (compilados, falharam).
pub fn compilar(raiz: &Path, mudanca: Option<(&str, &str)>) -> (Vec<String>, Vec<String>) {
    let (mut ok, mut falhou) = (Vec::new(), Vec::new());
    if !rodar("cargo", &["--version"], None, &[], Duration::from_secs(10)).ok() {
        // sem cargo o app não é recompilado: avisa em vez de ficar calado com o app velho
        falhou.extend(precisam(raiz, mudanca).iter().map(|a| a.crate_.to_string()));
        return (ok, falhou);
    }
    for a in precisam(raiz, mudanca) {
        if compilar_um(raiz, a) {
            ok.push(a.crate_.to_string());
        } else {
            falhou.push(a.crate_.to_string());
        }
    }
    (ok, falhou)
}

/// Onde o CI publica os executáveis da `main` (release móvel `main-latest`, refeito a cada push).
pub const BINARIOS_PADRAO: &str = "https://github.com/Just-Gabe/Astrograph/releases/download/main-latest";

/// Sufixo dos arquivos publicados: `windows-x64`, `linux-x64` ou `macos-arm64`; e a extensão.
pub fn plataforma() -> Option<(&'static str, &'static str)> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some(("windows-x64", ".exe"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(("linux-x64", ""))
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(("macos-arm64", ""))
    } else {
        None
    }
}

fn baixar_texto(url: &str) -> Result<String, String> {
    let s = rodar("curl", &["-fsSL", "--max-time", "60", "-H", "User-Agent: astrograph", url], None, &[], Duration::from_secs(90));
    if s.ok() {
        Ok(s.stdout)
    } else {
        Err(format!("{url}: {}", s.stderr.trim()))
    }
}

fn sha256_de(arquivo: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(arquivo).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Baixa os executáveis prontos da `main` (sem precisar de Rust). Só vale se o CI já publicou o commit
/// `commit` e o hash de cada arquivo confere com o `SHA256SUMS.txt`. Devolve (trocados, falharam, aviso).
pub fn baixar_prontos(raiz: &Path, crates: &[String], commit: &str, base: &str) -> (Vec<String>, Vec<String>, Option<String>) {
    let base = base.trim_end_matches('/');
    let falhou = |aviso: String| (vec![], crates.to_vec(), Some(aviso));
    let Some((plat, ext)) = plataforma() else {
        return falhou("Não há executável pronto para este sistema.".into());
    };
    let publicado = match baixar_texto(&format!("{base}/COMMIT.txt")) {
        Ok(t) => t.trim().to_string(),
        Err(e) => return falhou(format!("Não achei os executáveis prontos da main ({e}).")),
    };
    if publicado.is_empty() || !commit.starts_with(&publicado) && !publicado.starts_with(commit) {
        return falhou(format!(
            "O app novo ainda está sendo compilado pelo CI (publicado: {}, esperado: {}). Rode :Astrograph atualizar de novo em alguns minutos.",
            publicado.chars().take(7).collect::<String>(),
            commit.chars().take(7).collect::<String>()
        ));
    }
    let somas = match baixar_texto(&format!("{base}/SHA256SUMS.txt")) {
        Ok(t) => t,
        Err(e) => return falhou(format!("Sem SHA256SUMS.txt, não baixo nada sem conferir ({e}).")),
    };
    let (mut ok, mut ruins) = (Vec::new(), Vec::new());
    for c in crates {
        let Some(alvo) = ALVOS.iter().find(|a| a.crate_ == c) else { continue };
        let asset = format!("{}-{plat}{ext}", alvo.bin);
        let esperado = somas.lines().find_map(|l| {
            let (h, n) = l.split_once(char::is_whitespace)?;
            (n.trim().trim_start_matches('*') == asset).then(|| h.to_lowercase())
        });
        let dest = destino(raiz, alvo);
        let novo = PathBuf::from(format!("{}.new", dest.display()));
        let _ = fs::create_dir_all(dest.parent().unwrap());
        let baixou = rodar(
            "curl",
            &["-fsSL", "--max-time", "600", "-o", &novo.to_string_lossy(), &format!("{base}/{asset}")],
            None,
            &[],
            Duration::from_secs(660),
        )
        .ok();
        let confere = baixou && esperado.is_some() && sha256_de(&novo) == esperado;
        if confere && trocar_binario(&novo, &dest) {
            ok.push(c.clone());
        } else {
            let _ = fs::remove_file(&novo);
            ruins.push(c.clone());
        }
    }
    let aviso = (!ruins.is_empty()).then(|| "O download não conferiu com o SHA256 publicado (descartado).".to_string());
    (ok, ruins, aviso)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_app_depende_do_motor_e_dos_servicos() {
        let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut ui = pastas_do_build(&raiz, "astrograph-ui");
        ui.sort();
        for esperado in ["astrograph-core", "astrograph-servicos", "astrograph-syntax", "astrograph-ui"] {
            assert!(ui.iter().any(|c| c == esperado), "{esperado} em {ui:?}");
        }
        assert_eq!(pastas_do_build(&raiz, "astrograph-agent"), vec!["astrograph-agent".to_string()]);
    }

    #[test]
    fn baixa_so_com_commit_e_hash_certos() {
        use sha2::{Digest, Sha256};
        let Some((plat, ext)) = plataforma() else { return };
        let d = std::env::temp_dir().join(format!("astrograph-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let (pub_, raiz) = (d.join("pub"), d.join("raiz"));
        fs::create_dir_all(&pub_).unwrap();
        fs::create_dir_all(raiz.join("bin")).unwrap();
        let conteudo = b"binario-novo";
        let asset = format!("astrograph-{plat}{ext}");
        fs::write(pub_.join(&asset), conteudo).unwrap();
        let hash: String = Sha256::digest(conteudo).iter().map(|b| format!("{b:02x}")).collect();
        fs::write(pub_.join("SHA256SUMS.txt"), format!("{hash}  {asset}\n")).unwrap();
        fs::write(pub_.join("COMMIT.txt"), "abc1234def\n").unwrap();
        fs::write(raiz.join("bin").join(exe("astrograph")), b"velho").unwrap();
        let base = format!("file://{}", pub_.display());
        let crates = vec!["astrograph-ui".to_string()];
        // commit ainda não publicado: não troca
        let (ok, ruins, aviso) = baixar_prontos(&raiz, &crates, "ffffffffff", &base);
        assert!(ok.is_empty() && ruins == crates && aviso.unwrap().contains("compilado"));
        assert_eq!(fs::read(raiz.join("bin").join(exe("astrograph"))).unwrap(), b"velho");
        // hash errado: descarta
        fs::write(pub_.join("SHA256SUMS.txt"), format!("{}  {asset}\n", "0".repeat(64))).unwrap();
        let (ok, ruins, _) = baixar_prontos(&raiz, &crates, "abc1234def", &base);
        assert!(ok.is_empty() && ruins == crates);
        assert_eq!(fs::read(raiz.join("bin").join(exe("astrograph"))).unwrap(), b"velho");
        // tudo certo
        fs::write(pub_.join("SHA256SUMS.txt"), format!("{hash}  {asset}\n")).unwrap();
        let (ok, ruins, aviso) = baixar_prontos(&raiz, &crates, "abc1234def", &base);
        assert_eq!((ok, ruins, aviso), (crates, vec![], None));
        assert_eq!(fs::read(raiz.join("bin").join(exe("astrograph"))).unwrap(), conteudo);
        let _ = fs::remove_dir_all(&d);
    }
}
