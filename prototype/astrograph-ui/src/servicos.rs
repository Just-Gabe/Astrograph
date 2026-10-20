//! Servicos do Astrograph rodando no proprio app: preparo do projeto, git,
//! compilacao dos executaveis e o indicador de "trabalhando" do canto da tela.
//!
//! O app e a fonte da verdade. O Lua (`astrograph.prepare`) so cuida do que
//! precisa de plugins do Neovim (parsers, Mason) e avisa aqui quando esta
//! ocupado (`astrograph_busy`), para existir um unico indicador, desenhado pelo
//! renderer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use astrograph_servicos::{nativo, projeto};
use nvim_rs::compat::tokio::Compat;
use nvim_rs::{Neovim, Value};
use serde_json::json;
use tokio::process::ChildStdin;

type Nvim = Neovim<Compat<ChildStdin>>;

/// So aparece o indicador depois disto ocupado (evita piscar).
pub const MOSTRAR_APOS: Duration = Duration::from_millis(400);
/// Passo da animacao do anel.
pub const PASSO_MS: u128 = 90;
/// Uma tarefa que nunca termina deixa de segurar o indicador.
const LIMITE_TAREFA: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
struct Ocupacao {
    nomes: Vec<(u64, String, Instant)>,
    desde: Option<Instant>,
    proximo: u64,
}

static OCUPACAO: Mutex<Ocupacao> = Mutex::new(Ocupacao { nomes: Vec::new(), desde: None, proximo: 0 });
static ACORDAR: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static LUA: OnceLock<Mutex<HashMap<String, Tarefa>>> = OnceLock::new();
static DESLIGADO: AtomicBool = AtomicBool::new(false);

/// `acordar` pede um quadro ao loop de eventos (ligar/desligar o indicador).
pub fn iniciar(acordar: impl Fn() + Send + Sync + 'static) {
    let _ = ACORDAR.set(Box::new(acordar));
    DESLIGADO.store(std::env::var("ASTROGRAPH_SERVICOS").is_ok_and(|v| v == "nvim"), Ordering::Relaxed);
}

/// `ASTROGRAPH_SERVICOS=nvim` devolve o preparo ao Lua (reserva).
pub fn ligado() -> bool {
    !DESLIGADO.load(Ordering::Relaxed)
}

fn acordar() {
    if let Some(f) = ACORDAR.get() {
        f();
    }
}

/// Segura o indicador enquanto existir.
pub struct Tarefa {
    id: u64,
}

impl Tarefa {
    pub fn nova(nome: &str) -> Tarefa {
        let mut o = OCUPACAO.lock().unwrap();
        o.proximo += 1;
        let id = o.proximo;
        let agora = Instant::now();
        o.nomes.push((id, nome.to_string(), agora));
        o.desde.get_or_insert(agora);
        drop(o);
        acordar();
        Tarefa { id }
    }
}

impl Drop for Tarefa {
    fn drop(&mut self) {
        let mut o = OCUPACAO.lock().unwrap();
        o.nomes.retain(|(id, _, _)| *id != self.id);
        if o.nomes.is_empty() {
            o.desde = None;
        }
        drop(o);
        acordar();
    }
}

/// Quadro do anel (0..8), ou `None` se nada esta ocupado ha tempo suficiente.
pub fn quadro(agora: Instant) -> Option<usize> {
    let mut o = OCUPACAO.lock().unwrap();
    // tarefas esquecidas (Lua que caiu no meio) nao seguram o indicador para sempre
    let antes = o.nomes.len();
    o.nomes.retain(|(_, _, ini)| agora.saturating_duration_since(*ini) < LIMITE_TAREFA);
    if o.nomes.is_empty() {
        if antes > 0 {
            o.desde = None;
        }
        return None;
    }
    let desde = o.desde?;
    let ocupado = agora.saturating_duration_since(desde);
    (ocupado >= MOSTRAR_APOS).then(|| (ocupado.as_millis() / PASSO_MS % 8) as usize)
}

/// Nomes das tarefas em andamento (para `:Astrograph preparo`).
pub fn em_andamento() -> Vec<String> {
    let mut v: Vec<String> = OCUPACAO.lock().unwrap().nomes.iter().map(|(_, n, _)| n.clone()).collect();
    v.sort();
    v.dedup();
    v
}

/// O Lua avisa que comecou/terminou algo (`astrograph_busy`, nome, ligado).
pub fn ocupado_pelo_lua(args: &[Value]) {
    let (Some(nome), Some(liga)) = (args.first().and_then(Value::as_str), args.get(1).and_then(Value::as_bool)) else {
        return;
    };
    let mapa = LUA.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = mapa.lock().unwrap();
    if liga {
        m.insert(nome.to_string(), Tarefa::nova(nome));
    } else {
        m.remove(nome); // o Drop libera
    }
}

/// Raiz do clone do Astrograph: `ASTROGRAPH_ROOT`, ou a pasta acima de `bin/`
/// onde o executavel foi instalado.
pub fn raiz_do_clone() -> Option<PathBuf> {
    if let Some(r) = std::env::var_os("ASTROGRAPH_ROOT") {
        return Some(PathBuf::from(r));
    }
    let exe = std::env::current_exe().ok()?;
    let raiz = exe.parent()?.parent()?.to_path_buf();
    raiz.join(".git").exists().then_some(raiz)
}

/// Linguagens e git da pasta, publicados em `g:astrograph_preparo` (JSON) para o
/// Lua e para `:Astrograph preparo`.
pub async fn preparar(nvim: Nvim, pasta: PathBuf) {
    let _t = Tarefa::nova("projeto");
    let resumo = tokio::task::spawn_blocking(move || resumo(&pasta)).await;
    if let Ok(v) = resumo {
        let _ = nvim.set_var("astrograph_preparo", Value::from(v.to_string())).await;
    }
}

fn resumo(pasta: &Path) -> serde_json::Value {
    let langs = projeto::detectar(pasta);
    let larga = projeto::muito_larga(pasta);
    let git = if larga { Ok(None) } else { projeto::resumo_git(pasta) };
    json!({
        "pasta": pasta.to_string_lossy(),
        "linguagens": langs,
        "pacotes_mason": projeto::pacotes_mason(&langs),
        "git": git.as_ref().ok().cloned().flatten(),
        "git_instalado": git.is_ok(),
        "muito_larga": larga,
    })
}

/// Compila em segundo plano os executaveis em Rust que faltam em `<clone>/bin`.
pub async fn compilar_nativos() {
    let Some(raiz) = raiz_do_clone() else { return };
    let _t = Tarefa::nova("rust");
    let _ = tokio::task::spawn_blocking(move || nativo::compilar(&raiz, None)).await;
}

/// Pasta atual do Neovim.
pub async fn pasta_atual(nvim: &Nvim) -> Option<PathBuf> {
    let v = nvim.call_function("getcwd", vec![]).await.ok()?;
    v.as_str().map(PathBuf::from)
}

/// Registra o canal do app para o Lua e assina `DirChanged`.
pub async fn ligar(nvim: &Nvim) {
    let chan = nvim.get_api_info().await.ok().and_then(|i| i.first().and_then(Value::as_i64)).unwrap_or(0);
    let _ = nvim.set_var("astrograph_app_chan", Value::from(chan)).await;
    let _ = nvim
        .exec_lua(
            "local chan = ...; vim.api.nvim_create_autocmd('DirChanged', { group = vim.api.nvim_create_augroup('astrograph_app_servicos', { clear = true }), \
             callback = function() vim.rpcnotify(chan, 'astrograph_dir', vim.fn.getcwd()) end })",
            vec![Value::from(chan)],
        )
        .await;
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn indicador_so_depois_de_um_tempo_e_some_ao_liberar() {
        let t = Tarefa::nova("teste");
        let agora = Instant::now();
        assert_eq!(quadro(agora), None, "recem-iniciada: ainda nao mostra");
        assert!(em_andamento().contains(&"teste".to_string()));
        let depois = agora + MOSTRAR_APOS + Duration::from_millis(10);
        assert!(quadro(depois).is_some(), "ocupado ha tempo: mostra");
        let outro = quadro(depois + Duration::from_millis(PASSO_MS as u64));
        assert_ne!(quadro(depois), outro, "o anel gira");
        drop(t);
        assert_eq!(quadro(depois), None, "liberou: some");
        assert!(!em_andamento().contains(&"teste".to_string()));
    }

    #[test]
    fn aviso_do_lua_liga_e_desliga() {
        ocupado_pelo_lua(&[Value::from("parsers-lua"), Value::from(true)]);
        assert!(em_andamento().contains(&"parsers-lua".to_string()));
        ocupado_pelo_lua(&[Value::from("parsers-lua"), Value::from(false)]);
        assert!(!em_andamento().contains(&"parsers-lua".to_string()));
        ocupado_pelo_lua(&[Value::from("x")]); // argumentos incompletos: ignora
    }

    #[test]
    fn resumo_de_uma_pasta_com_projeto() {
        let p = std::env::temp_dir().join(format!("astrograph-ui-resumo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("Cargo.toml"), "").unwrap();
        let r = resumo(&p);
        assert_eq!(r["linguagens"][0], "rust");
        assert_eq!(r["muito_larga"], false);
    }
}
