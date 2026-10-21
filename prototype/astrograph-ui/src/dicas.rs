//! Dicas visuais que ensinam os atalhos: cada uma aparece poucas vezes (o contador fica no estado do
//! usuário), no máximo uma por poucos segundos, e tudo se desliga em Configurações (`[ui] tips`).

use std::collections::{HashMap, HashSet};
#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(not(test))]
static LIGADAS: AtomicBool = AtomicBool::new(true);
// nos testes o estado é de cada thread (cada teste roda na sua): um teste que abre arquivo não consome as dicas de outro
#[cfg(test)]
thread_local! {
    static LIGADAS_T: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ESTADO_T: std::cell::RefCell<Option<Estado>> = const { std::cell::RefCell::new(None) };
}

struct Estado {
    contagem: HashMap<String, u32>,
    carregado: bool,
    /// já mostradas nesta execução (cada dica aparece no máximo uma vez por execução)
    vistas: HashSet<String>,
    ultima: Option<Instant>,
}

#[cfg(not(test))]
static ESTADO: Mutex<Option<Estado>> = Mutex::new(None);

fn com_estado<R>(f: impl FnOnce(&mut Option<Estado>) -> R) -> R {
    #[cfg(not(test))]
    return f(&mut ESTADO.lock().unwrap());
    #[cfg(test)]
    ESTADO_T.with(|e| f(&mut e.borrow_mut()))
}

/// Intervalo mínimo entre duas dicas: não vira chuva de avisos.
const ENTRE_DICAS: Duration = Duration::from_secs(8);

pub fn ligar(b: bool) {
    #[cfg(not(test))]
    LIGADAS.store(b, Ordering::Relaxed);
    #[cfg(test)]
    LIGADAS_T.with(|l| l.set(b));
}

pub fn ligadas() -> bool {
    #[cfg(not(test))]
    return LIGADAS.load(Ordering::Relaxed);
    #[cfg(test)]
    LIGADAS_T.with(|l| l.get())
}

fn arquivo() -> Option<std::path::PathBuf> {
    crate::busca::pasta_estado().map(|d| d.join("dicas.txt"))
}

fn carregar() -> HashMap<String, u32> {
    if cfg!(test) {
        return HashMap::new();
    }
    let Some(f) = arquivo() else { return HashMap::new() };
    std::fs::read_to_string(f)
        .map(|t| t.lines().filter_map(|l| l.split_once('=').and_then(|(k, v)| Some((k.to_string(), v.trim().parse().ok()?)))).collect())
        .unwrap_or_default()
}

fn gravar(c: &HashMap<String, u32>) {
    if cfg!(test) {
        return;
    }
    let Some(f) = arquivo() else { return };
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut linhas: Vec<String> = c.iter().map(|(k, v)| format!("{k}={v}")).collect();
    linhas.sort();
    let _ = std::fs::write(f, linhas.join("\n") + "\n");
}

/// Esta dica pode aparecer agora? Conta uma exibição quando sim. `max`: quantas vezes, no total, ela já pode ter aparecido.
pub fn mostrar(id: &str, max: u32) -> bool {
    if !ligadas() {
        return false;
    }
    com_estado(|g| {
        let e = g.get_or_insert_with(|| Estado { contagem: HashMap::new(), carregado: false, vistas: HashSet::new(), ultima: None });
        if !e.carregado {
            e.contagem = carregar();
            e.carregado = true;
        }
        if e.vistas.contains(id) || e.contagem.get(id).copied().unwrap_or(0) >= max {
            return false;
        }
        if e.ultima.is_some_and(|t| t.elapsed() < ENTRE_DICAS) {
            return false;
        }
        e.vistas.insert(id.to_string());
        *e.contagem.entry(id.to_string()).or_insert(0) += 1;
        e.ultima = Some(Instant::now());
        gravar(&e.contagem);
        true
    })
}

/// Nunca mais mostra esta dica (o contador vai ao teto e fica gravado).
pub fn silenciar(id: &str) {
    com_estado(|g| {
        let e = g.get_or_insert_with(|| Estado { contagem: HashMap::new(), carregado: false, vistas: HashSet::new(), ultima: None });
        if !e.carregado {
            e.contagem = carregar();
            e.carregado = true;
        }
        e.contagem.insert(id.to_string(), 99);
        gravar(&e.contagem);
    })
}

/// Só para os testes: esquece o que já foi mostrado e o intervalo entre dicas.
#[cfg(test)]
pub fn zerar() {
    com_estado(|g| *g = None);
    ligar(true);
}

#[cfg(test)]
pub fn encerrar() {
    com_estado(|g| *g = None);
    ligar(false);
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn cada_dica_aparece_uma_vez_por_execucao_e_com_intervalo() {
        let _g = crate::dicas::TESTE.lock().unwrap_or_else(|e| e.into_inner());
        zerar();
        assert!(mostrar("a", 2));
        assert!(!mostrar("a", 2), "a mesma não repete na execução");
        assert!(!mostrar("b", 2), "outra logo em seguida espera o intervalo");
        ligar(false);
        assert!(!mostrar("c", 2), "desligadas nunca aparecem");
        encerrar();
    }
}

/// Os testes mexem no estado global: um de cada vez.
#[cfg(test)]
pub static TESTE: Mutex<()> = Mutex::new(());
