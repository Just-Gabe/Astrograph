//! Atualizacao transacional do clone do Astrograph: ou termina com a versao nova
//! funcionando, ou volta exatamente para onde estava.
//!
//!   1. trava (um editor atualiza por vez) e busca (`fetch`); sem rede nao muda nada;
//!   2. guarda o que e do usuario (`stash --include-untracked`);
//!   3. avanca (fast-forward); historico reescrito em clone raso vira o ponto de
//!      restauracao `astrograph-backup-<data>`; clone de desenvolvimento com
//!      commits proprios: recusa;
//!   4. sincroniza os plugins (lazy-lock), recompila so o app que mudou;
//!   5. confere que a config sobe (nvim headless); se nao subir, desfaz tudo;
//!   6. devolve o que era do usuario; se conflitar, fica no stash.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::nativo;
use crate::proc::{git, rodar, Saida, LIMITE_GIT};

pub const TRAVA_VELHA: Duration = Duration::from_secs(30 * 60);
pub const PREFIXO_BACKUP: &str = "astrograph-backup-";

#[derive(Debug, Clone)]
pub struct Opcoes {
    pub raiz: PathBuf,
    pub remoto: String,
    pub ramo: Option<String>,
    pub forcar: bool,
    /// `Lazy! restore` num nvim headless (desligado nos testes)
    pub restaurar_plugins: bool,
    pub recompilar: bool,
    /// sobe a config num nvim headless e procura erro
    pub verificar: bool,
    pub nvim: String,
}

impl Opcoes {
    pub fn nova(raiz: impl Into<PathBuf>) -> Self {
        Opcoes {
            raiz: raiz.into(),
            remoto: "origin".into(),
            ramo: None,
            forcar: false,
            restaurar_plugins: true,
            recompilar: true,
            verificar: true,
            nvim: std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Estado {
    Atualizado,
    JaAtualizado,
    Revertido,
    SemRede,
    Divergiu,
    Ocupado,
    SemGit,
    Erro,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resultado {
    pub status: Estado,
    pub mensagem: String,
    pub antes: Option<String>,
    pub depois: Option<String>,
    pub stash: Option<String>,
    pub backup: Option<String>,
    pub recompilados: Vec<String>,
}

impl Resultado {
    fn novo(status: Estado, mensagem: impl Into<String>) -> Self {
        Resultado { status, mensagem: mensagem.into(), antes: None, depois: None, stash: None, backup: None, recompilados: vec![] }
    }
    fn com_antes(mut self, antes: &str) -> Self {
        self.antes = Some(antes.to_string());
        self
    }
}

fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn carimbo() -> String {
    // AAAAMMDD-HHMMSS em UTC, sem dependencias
    let s = agora();
    let (dias, resto) = (s / 86400, s % 86400);
    let (mut a, mut d) = (1970u64, dias);
    loop {
        let bis = (a % 4 == 0 && a % 100 != 0) || a % 400 == 0;
        let n = if bis { 366 } else { 365 };
        if d < n {
            break;
        }
        d -= n;
        a += 1;
    }
    let bis = (a % 4 == 0 && a % 100 != 0) || a % 400 == 0;
    let meses = [31, if bis { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    while d >= meses[m] {
        d -= meses[m];
        m += 1;
    }
    format!("{a:04}{:02}{:02}-{:02}{:02}{:02}", m + 1, d + 1, resto / 3600, resto % 3600 / 60, resto % 60)
}

fn caminho_trava(raiz: &Path) -> PathBuf {
    raiz.join(".git").join("astrograph-update.lock")
}

fn pegar_trava(raiz: &Path) -> bool {
    let p = caminho_trava(raiz);
    if let Ok(meta) = fs::metadata(&p) {
        let idade = meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).unwrap_or_default();
        if idade < TRAVA_VELHA {
            return false;
        }
    }
    let _ = fs::write(&p, std::process::id().to_string());
    true
}

fn sync_plugins(o: &Opcoes) {
    if o.restaurar_plugins {
        let _ = rodar(&o.nvim, &["--headless", "+Lazy! restore", "+qa"], None, &[], Duration::from_secs(10 * 60));
    }
}

/// Sobe a config num nvim headless. Estourar o tempo nao conta como falha.
fn verificar(o: &Opcoes) -> Result<(), String> {
    if !o.verificar {
        return Ok(());
    }
    let r = rodar(&o.nvim, &["--headless", "+lua io.stdout:write('ASTROGRAPH_OK')", "+qa"], None, &[], Duration::from_secs(90));
    if r.estourou {
        return Ok(());
    }
    let texto = format!("{}\n{}", r.stdout, r.stderr);
    if !texto.contains("ASTROGRAPH_OK") {
        return Err(texto.trim().chars().take(600).collect());
    }
    let ruim = texto.lines().find(|l| {
        let tem_e5 = l.find("E5").is_some_and(|i| l[i + 2..].chars().take(3).all(|c| c.is_ascii_digit()) && l[i + 2..].len() >= 3);
        tem_e5 || (l.contains("module '") && l.contains("' not found")) || l.contains("Error detected while processing")
    });
    match ruim {
        Some(l) => Err(l.trim().to_string()),
        None => Ok(()),
    }
}

pub fn atualizar(o: &Opcoes) -> Resultado {
    let raiz = o.raiz.as_path();
    if !raiz.join(".git").exists() {
        return Resultado::novo(
            Estado::SemGit,
            "A config nao aponta para um clone git do Astrograph. Rode o instalador de novo (ele atualiza e religa tudo).",
        );
    }
    if !rodar("git", &["--version"], None, &[], Duration::from_secs(10)).ok() {
        return Resultado::novo(Estado::SemGit, "O git nao esta instalado (ou fora do PATH).");
    }
    if !pegar_trava(raiz) {
        return Resultado::novo(Estado::Ocupado, "Outro Neovim ja esta atualizando o Astrograph. Espere terminar.");
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| corpo(o)));
    let _ = fs::remove_file(caminho_trava(raiz));
    r.unwrap_or_else(|_| Resultado::novo(Estado::Erro, "Falha inesperada na atualizacao."))
}

fn corpo(o: &Opcoes) -> Resultado {
    let raiz = o.raiz.as_path();
    let g = |args: &[&str]| -> Saida { git(raiz, args, LIMITE_GIT) };
    let selo = carimbo();

    let head = g(&["rev-parse", "HEAD"]);
    if !head.ok() {
        return Resultado::novo(Estado::Erro, format!("Nao consegui ler o repositorio: {}", head.stderr.trim()));
    }
    let antes = head.texto();
    let raso = g(&["rev-parse", "--is-shallow-repository"]).texto() == "true";

    // ramo a seguir: o configurado, o upstream do atual, ou o main
    let ramo = o.ramo.clone().unwrap_or_else(|| {
        let up = g(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).texto();
        up.split_once('/').map(|(_, r)| r.to_string()).filter(|r| !r.is_empty()).unwrap_or_else(|| "main".into())
    });

    // 1. busca (nada mudou ainda se falhar)
    let mut fetch = vec!["fetch", "--no-tags", "--quiet"];
    if raso {
        fetch.push("--depth=1");
    }
    fetch.push(&o.remoto);
    fetch.push(&ramo);
    let f = g(&fetch);
    if !f.ok() {
        let por_que = f.stderr.trim().to_string();
        let sem_rede = f.estourou
            || ["Could not resolve", "unable to access", "Failed to connect", "timed out", "Connection"]
                .iter()
                .any(|p| por_que.contains(p));
        return if sem_rede {
            Resultado::novo(Estado::SemRede, format!("Sem conexao com {} (nada foi alterado).", o.remoto)).com_antes(&antes)
        } else {
            Resultado::novo(Estado::Erro, format!("O git nao conseguiu buscar a versao nova (nada foi alterado):\n{por_que}"))
                .com_antes(&antes)
        };
    }
    let depois = g(&["rev-parse", "FETCH_HEAD"]).texto();
    if depois == antes {
        let mut r = Resultado::novo(Estado::JaAtualizado, "O Astrograph ja esta na ultima versao.").com_antes(&antes);
        r.depois = Some(depois);
        return r;
    }

    // 2. guarda o que e do usuario (o Lazy reescreve o lazy-lock sozinho: descarta)
    let _ = g(&["checkout", "--", "template/lazy-lock.json"]);
    let sujo = g(&["status", "--porcelain=v1", "--untracked-files=normal"]).texto();
    let mut guardou = false;
    let nome_stash = format!("astrograph-update-{selo}");
    if !sujo.is_empty() {
        let s = g(&["stash", "push", "--include-untracked", "-m", &nome_stash]);
        if !s.ok() {
            return Resultado::novo(
                Estado::Erro,
                format!("Nao consegui guardar suas alteracoes antes de atualizar (nada foi alterado):\n{}", s.stderr.trim()),
            )
            .com_antes(&antes);
        }
        guardou = !s.stdout.contains("No local changes");
    }
    let voltar = |g: &dyn Fn(&[&str]) -> Saida| {
        let _ = g(&["reset", "--hard", &antes]);
        if guardou {
            let _ = g(&["stash", "pop", "--quiet"]);
        }
    };

    // 3. avanca
    let ancestral = g(&["merge-base", "--is-ancestor", &antes, &depois]).ok();
    let mut backup: Option<String> = None;
    if ancestral {
        let m = g(&["merge", "--ff-only", "--quiet", &depois]);
        if !m.ok() {
            voltar(&g);
            return Resultado::novo(
                Estado::Erro,
                format!("Nao consegui avancar para a versao nova (voltei ao que estava):\n{}", m.stderr.trim()),
            )
            .com_antes(&antes);
        }
    } else {
        if !(raso || o.forcar) {
            if guardou {
                let _ = g(&["stash", "pop", "--quiet"]);
            }
            return Resultado::novo(
                Estado::Divergiu,
                format!(
                    "Este clone tem historico proprio (commits que nao estao em {}/{}). Nao vou sobrescrever: atualize com git (merge ou rebase).",
                    o.remoto, ramo
                ),
            )
            .com_antes(&antes);
        }
        let nome = format!("{PREFIXO_BACKUP}{selo}");
        let _ = g(&["branch", "-f", &nome, &antes]);
        // fica um ponto de restauracao por atualizacao, so os 3 ultimos
        let lista = g(&["branch", "--list", &format!("{PREFIXO_BACKUP}*"), "--format=%(refname:short)"]).stdout;
        let mut todos: Vec<&str> = lista.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        todos.sort();
        let sobra = todos.len().saturating_sub(3);
        for velho in &todos[..sobra] {
            let _ = g(&["branch", "-D", velho]);
        }
        let r = g(&["reset", "--hard", &depois]);
        if !r.ok() {
            voltar(&g);
            return Resultado::novo(
                Estado::Erro,
                format!("Nao consegui trocar para a versao nova (voltei ao que estava):\n{}", r.stderr.trim()),
            )
            .com_antes(&antes);
        }
        backup = Some(nome);
    }

    // 4. plugins, app em Rust e conferencia (com a arvore limpa na versao nova)
    sync_plugins(o);
    let (mut compilados, mut falharam) = if o.recompilar { nativo::compilar(raiz, Some((&antes, &depois))) } else { (vec![], vec![]) };
    // sem Rust (ou com o build falhando) tenta os executáveis prontos que o CI publica para a main
    let mut aviso_binarios: Option<String> = None;
    if !falharam.is_empty() {
        let base = std::env::var("ASTROGRAPH_BINARIOS_URL").unwrap_or_else(|_| nativo::BINARIOS_PADRAO.to_string());
        let (ok, ruins, aviso) = nativo::baixar_prontos(raiz, &falharam, &depois, &base);
        compilados.extend(ok.into_iter().map(|c| format!("{c} (pronto)")));
        falharam = ruins;
        aviso_binarios = aviso;
    }
    if let Err(detalhe) = verificar(o) {
        voltar(&g);
        sync_plugins(o);
        let mut r = Resultado::novo(
            Estado::Revertido,
            format!("A versao nova nao subiu aqui, entao voltei para a anterior (nada foi perdido):\n{detalhe}"),
        )
        .com_antes(&antes);
        r.depois = Some(depois);
        return r;
    }

    // 6. devolve as alteracoes do usuario
    let mut guardado_no_stash = None;
    if guardou {
        let pop = g(&["stash", "pop", "--quiet"]);
        if !pop.ok() {
            let _ = g(&["reset", "--hard", "HEAD"]); // tira o meio-merge; o stash continua guardado
            guardado_no_stash = Some(nome_stash.clone());
        }
    }

    let curto = |s: &str| s.chars().take(7).collect::<String>();
    let mut msg = format!("Astrograph atualizado ({} -> {}).", curto(&antes), curto(&depois));
    let log = g(&["log", "--oneline", "-n", "8", &format!("{antes}..{depois}")]);
    if log.ok() && !log.texto().is_empty() {
        msg.push('\n');
        msg.push_str(&log.texto());
    }
    if !compilados.is_empty() {
        msg.push_str(&format!("\nApp recompilado: {}.", compilados.join(", ")));
    }
    if !falharam.is_empty() {
        msg.push_str(&format!(
            "\nNao consegui atualizar o app ({}): o antigo continua valendo. Instale o Rust (https://rustup.rs) para compilar aqui.",
            falharam.join(", ")
        ));
        if let Some(a) = &aviso_binarios {
            msg.push_str(&format!("\n{a}"));
        }
    }
    // num clone raso o git nao enxerga o pai da versao nova: o ramo e so ponto de
    // restauracao; so avisa quando o historico divergiu de verdade
    if let (Some(b), false) = (&backup, raso) {
        msg.push_str(&format!("\nA versao anterior ficou no ramo {b}."));
    }
    if let Some(s) = &guardado_no_stash {
        msg.push_str(&format!(
            "\nSuas alteracoes conflitaram com a versao nova e ficaram guardadas: git -C \"{}\" stash list ({s}).",
            raiz.display()
        ));
    }
    msg.push_str("\nReinicie o Astrograph (e o Neovim, se usa o espelho) para carregar a versao nova.");
    Resultado {
        status: Estado::Atualizado,
        mensagem: msg,
        antes: Some(antes),
        depois: Some(depois),
        stash: guardado_no_stash,
        backup,
        recompilados: compilados,
    }
}
