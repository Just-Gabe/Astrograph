//! Buscas do editor nativo, sem programas externos: arquivos do projeto
//! (respeitando `.gitignore`), texto em todos eles e a lista de recentes.
//! Substitui o `fd`/`ripgrep` + picker do Neovim.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;

/// Teto de arquivos listados (projetos gigantes não travam a abertura do buscador).
const MAX_ARQUIVOS: usize = 60_000;
/// Arquivos maiores que isso ficam fora da busca de texto.
const MAX_BYTES: u64 = 1_500_000;

/// Caminhos relativos a `raiz`, ordenados. Pastas ocultas e o que o git ignora ficam de fora.
pub fn listar(raiz: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in WalkBuilder::new(raiz).hidden(true).git_ignore(true).git_global(false).follow_links(false).build().flatten() {
        if !e.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Ok(rel) = e.path().strip_prefix(raiz) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
        if out.len() >= MAX_ARQUIVOS {
            break;
        }
    }
    out.sort();
    out
}

/// Mensagem de uma busca em segundo plano.
#[derive(Debug)]
pub enum Parcial<T> {
    /// mais resultados (se for a lista de arquivos, ainda sem ordem)
    Itens(Vec<T>),
    /// a lista de arquivos terminou: a lista completa e ordenada, que substitui as parciais
    Completa(Vec<T>),
    /// a busca acabou (ou foi cancelada)
    Fim,
}

/// Busca rodando em outra thread. Soltar (ou `cancelar`) pára a thread no próximo arquivo.
pub struct Bg<T> {
    rx: Receiver<Parcial<T>>,
    cancel: Arc<AtomicBool>,
    pub feita: bool,
}

impl<T> Bg<T> {
    /// O que chegou desde a última chamada (não espera).
    pub fn novidades(&mut self) -> Vec<Parcial<T>> {
        let mut v = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            if matches!(m, Parcial::Fim) {
                self.feita = true;
            }
            v.push(m);
        }
        v
    }

    pub fn cancelar(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl<T> Drop for Bg<T> {
    fn drop(&mut self) {
        self.cancelar();
    }
}

/// Cache da lista de arquivos por raiz (a lista de ontem aparece já; a nova chega em seguida).
static CACHE_LISTA: Mutex<Option<HashMap<PathBuf, (Instant, Vec<String>)>>> = Mutex::new(None);

/// Lista guardada para `raiz` e há quanto tempo ela foi feita.
pub fn lista_em_cache(raiz: &Path) -> Option<(Duration, Vec<String>)> {
    CACHE_LISTA.lock().ok()?.as_ref()?.get(raiz).map(|(t, v)| (t.elapsed(), v.clone()))
}

fn guardar_lista(raiz: &Path, v: &[String]) {
    if let Ok(mut c) = CACHE_LISTA.lock() {
        c.get_or_insert_with(HashMap::new).insert(raiz.to_path_buf(), (Instant::now(), v.to_vec()));
    }
}

/// Esquece a lista de `raiz` (arquivo criado/apagado/renomeado).
pub fn esquecer_lista(raiz: &Path) {
    if let Ok(mut c) = CACHE_LISTA.lock() {
        if let Some(m) = c.as_mut() {
            m.remove(raiz);
        }
    }
}

/// `listar` em outra thread: manda lotes enquanto anda pelo disco e, no fim, a lista ordenada
/// (que também entra no cache). `notify` acorda a UI a cada novidade.
pub fn listar_bg(raiz: PathBuf, notify: Arc<dyn Fn() + Send + Sync>) -> Bg<String> {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut lote = Vec::new();
        for e in WalkBuilder::new(&raiz).hidden(true).git_ignore(true).git_global(false).follow_links(false).build().flatten() {
            if c.load(Ordering::Acquire) {
                let _ = tx.send(Parcial::Fim);
                return;
            }
            if !e.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if let Ok(rel) = e.path().strip_prefix(&raiz) {
                let r = rel.to_string_lossy().replace('\\', "/");
                lote.push(r.clone());
                out.push(r);
            }
            if lote.len() >= 2000 {
                let _ = tx.send(Parcial::Itens(std::mem::take(&mut lote)));
                notify();
            }
            if out.len() >= MAX_ARQUIVOS {
                break;
            }
        }
        out.sort();
        guardar_lista(&raiz, &out);
        let _ = tx.send(Parcial::Completa(out));
        let _ = tx.send(Parcial::Fim);
        notify();
    });
    Bg { rx, cancel, feita: false }
}

/// `buscar_texto` em outra thread: manda os achados de cada arquivo assim que o lê. Uma nova tecla
/// solta esta busca (e ela pára) e começa outra.
pub fn buscar_texto_bg(raiz: PathBuf, padrao: String, limite: usize, notify: Arc<dyn Fn() + Send + Sync>) -> Bg<Achado> {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        let Some(re) = regex_de(&padrao) else {
            let _ = tx.send(Parcial::Fim);
            notify();
            return;
        };
        let mut total = 0;
        for e in WalkBuilder::new(&raiz).hidden(true).git_ignore(true).git_global(false).build().flatten() {
            if c.load(Ordering::Acquire) || total >= limite {
                break;
            }
            if !e.file_type().is_some_and(|t| t.is_file()) || e.metadata().is_ok_and(|m| m.len() > MAX_BYTES) {
                continue;
            }
            let achados = achados_no_arquivo(&raiz, e.path(), &re, limite - total);
            if !achados.is_empty() {
                total += achados.len();
                if tx.send(Parcial::Itens(achados)).is_err() {
                    return;
                }
                notify();
            }
        }
        let _ = tx.send(Parcial::Fim);
        notify();
    });
    Bg { rx, cancel, feita: false }
}

fn regex_de(padrao: &str) -> Option<regex::Regex> {
    if padrao.is_empty() {
        return None;
    }
    let ic = !padrao.chars().any(|c| c.is_uppercase());
    let ia = if ic { format!("(?i){padrao}") } else { padrao.to_string() };
    regex::Regex::new(&ia).or_else(|_| regex::Regex::new(&format!("{}{}", if ic { "(?i)" } else { "" }, regex::escape(padrao)))).ok()
}

fn achados_no_arquivo(raiz: &Path, caminho: &Path, re: &regex::Regex, restam: usize) -> Vec<Achado> {
    let Ok(bytes) = std::fs::read(caminho) else { return Vec::new() };
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return Vec::new();
    }
    let texto = String::from_utf8_lossy(&bytes);
    let rel = caminho.strip_prefix(raiz).unwrap_or(caminho).to_string_lossy().replace('\\', "/");
    let mut out = Vec::new();
    for (i, linha) in texto.lines().enumerate() {
        if let Some(m) = re.find(linha) {
            out.push(Achado { arquivo: rel.clone(), linha: i, texto: linha.trim().chars().take(160).collect(), col: linha[..m.start()].chars().count() });
            if out.len() >= restam {
                break;
            }
        }
    }
    out
}

/// Como `fuzzy`, mas só olha os itens de `entre` (se dado) e devolve também todos os que casaram:
/// quem digita mais uma letra reaproveita o conjunto da tecla anterior em vez de varrer tudo.
pub fn fuzzy_sub(busca: &str, itens: &[String], entre: Option<&[usize]>, limite: usize) -> (Vec<Casou>, Vec<usize>) {
    let q: Vec<char> = busca.chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        let r: Vec<Casou> = itens.iter().enumerate().take(limite).map(|(item, _)| Casou { item, nota: 0, pos: Vec::new() }).collect();
        return (r, (0..itens.len()).collect());
    }
    let candidatos: Box<dyn Iterator<Item = usize>> = match entre {
        Some(e) => Box::new(e.iter().copied().filter(|i| *i < itens.len())),
        None => Box::new(0..itens.len()),
    };
    let mut v: Vec<Casou> = candidatos.filter_map(|item| pontuar(&q, &itens[item]).map(|(nota, pos)| Casou { item, nota, pos })).collect();
    let todos: Vec<usize> = v.iter().map(|c| c.item).collect();
    v.sort_by(|a, b| b.nota.cmp(&a.nota).then(itens[a.item].len().cmp(&itens[b.item].len())).then(a.item.cmp(&b.item)));
    v.truncate(limite);
    (v, todos)
}

/// Resultado de `fuzzy`: índice do item, pontuação e posições (em caracteres) que casaram.
#[derive(Debug, Clone, PartialEq)]
pub struct Casou {
    pub item: usize,
    pub nota: i32,
    pub pos: Vec<usize>,
}

/// Casamento "difuso": os caracteres da busca aparecem em ordem no caminho.
/// Maiúscula na busca liga a diferença de caixa. Bônus: início de palavra
/// (depois de `/ _ - .`), caracteres seguidos e acertos no nome do arquivo.
pub fn pontuar(busca: &[char], texto: &str) -> Option<(i32, Vec<usize>)> {
    if busca.is_empty() {
        return Some((0, Vec::new()));
    }
    let sensivel = busca.iter().any(|c| c.is_uppercase());
    let t: Vec<char> = texto.chars().collect();
    let eq = |a: char, b: char| if sensivel { a == b } else { a.to_lowercase().eq(b.to_lowercase()) };
    let nome_ini = t.iter().rposition(|&c| c == '/').map_or(0, |i| i + 1);
    // pega a ocorrência que começa mais perto do nome do arquivo: tenta alinhar do fim
    let mut melhor: Option<(i32, Vec<usize>)> = None;
    let mut inicio = 0;
    while inicio < t.len() {
        // primeiro caractere da busca a partir de `inicio`
        let Some(p0) = (inicio..t.len()).find(|&i| eq(t[i], busca[0])) else { break };
        let mut pos = vec![p0];
        let mut j = p0 + 1;
        let mut ok = true;
        for &b in &busca[1..] {
            match (j..t.len()).find(|&i| eq(t[i], b)) {
                Some(i) => {
                    pos.push(i);
                    j = i + 1;
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            break;
        }
        let mut nota = 0i32;
        for (k, &i) in pos.iter().enumerate() {
            nota += 10;
            if i == 0 || matches!(t[i - 1], '/' | '_' | '-' | '.' | ' ') {
                nota += 18;
            }
            if i >= nome_ini {
                nota += 6;
            }
            if k > 0 && pos[k - 1] + 1 == i {
                nota += 14;
            }
            if sensivel && t[i] == busca[k] {
                nota += 2;
            }
        }
        nota -= (pos[0] as i32) / 4;
        nota -= (t.len() as i32 - busca.len() as i32) / 8;
        if pos[0] >= nome_ini {
            nota += 12;
        }
        if melhor.as_ref().is_none_or(|(n, _)| nota > *n) {
            melhor = Some((nota, pos));
        }
        inicio = p0 + 1;
        if inicio > nome_ini + 1 && melhor.is_some() {
            break;
        }
    }
    melhor
}

/// Os `limite` melhores itens para `busca` (vazia: os primeiros, na ordem dada).
pub fn fuzzy(busca: &str, itens: &[String], limite: usize) -> Vec<Casou> {
    let q: Vec<char> = busca.chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return itens.iter().enumerate().take(limite).map(|(item, _)| Casou { item, nota: 0, pos: Vec::new() }).collect();
    }
    let mut v: Vec<Casou> = itens.iter().enumerate().filter_map(|(item, t)| pontuar(&q, t).map(|(nota, pos)| Casou { item, nota, pos })).collect();
    v.sort_by(|a, b| b.nota.cmp(&a.nota).then(itens[a.item].len().cmp(&itens[b.item].len())).then(a.item.cmp(&b.item)));
    v.truncate(limite);
    v
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Achado {
    pub arquivo: String,
    /// linha base 0
    pub linha: usize,
    pub texto: String,
    /// coluna (em caracteres) do início do trecho que casou
    pub col: usize,
}

/// Procura `padrao` (regex; se não compilar, texto literal) nos arquivos de `raiz`.
/// Para quando juntar `limite` achados ou passar de `tempo`.
pub fn buscar_texto(raiz: &Path, padrao: &str, limite: usize, tempo: Duration) -> Vec<Achado> {
    if padrao.is_empty() {
        return Vec::new();
    }
    let ic = !padrao.chars().any(|c| c.is_uppercase());
    let ia = if ic { format!("(?i){padrao}") } else { padrao.to_string() };
    let re = regex::Regex::new(&ia).or_else(|_| regex::Regex::new(&format!("{}{}", if ic { "(?i)" } else { "" }, regex::escape(padrao)))).ok();
    let Some(re) = re else { return Vec::new() };
    let t0 = Instant::now();
    let mut out = Vec::new();
    for e in WalkBuilder::new(raiz).hidden(true).git_ignore(true).git_global(false).build().flatten() {
        if out.len() >= limite || t0.elapsed() > tempo {
            break;
        }
        if !e.file_type().is_some_and(|t| t.is_file()) || e.metadata().is_ok_and(|m| m.len() > MAX_BYTES) {
            continue;
        }
        let Ok(bytes) = std::fs::read(e.path()) else { continue };
        if bytes.iter().take(8192).any(|&b| b == 0) {
            continue;
        }
        let texto = String::from_utf8_lossy(&bytes);
        let rel = e.path().strip_prefix(raiz).unwrap_or(e.path()).to_string_lossy().replace('\\', "/");
        for (i, linha) in texto.lines().enumerate() {
            if let Some(m) = re.find(linha) {
                out.push(Achado { arquivo: rel.clone(), linha: i, texto: linha.trim().chars().take(160).collect(), col: linha[..m.start()].chars().count() });
                if out.len() >= limite {
                    break;
                }
            }
        }
    }
    out
}

/// Onde a lista de recentes fica: `$ASTROGRAPH_STATE`, `%LOCALAPPDATA%`, `$XDG_STATE_HOME` ou `~/.local/state`.
fn arquivo_recentes() -> Option<PathBuf> {
    let base = std::env::var_os("ASTROGRAPH_STATE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("astrograph-state")))
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(|p| PathBuf::from(p).join("astrograph")))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state/astrograph")))?;
    Some(base.join("recentes.txt"))
}

/// Recentes, do mais novo para o mais antigo, só os que ainda existem.
pub fn recentes() -> Vec<String> {
    let Some(f) = arquivo_recentes() else { return Vec::new() };
    std::fs::read_to_string(f).map(|t| t.lines().filter(|l| !l.is_empty() && Path::new(l).exists()).map(str::to_string).collect()).unwrap_or_default()
}

/// Põe `caminho` (absoluto) no topo dos recentes (sem repetir, no máximo 60).
pub fn lembrar(caminho: &Path) {
    let Some(f) = arquivo_recentes() else { return };
    let abs = std::fs::canonicalize(caminho).unwrap_or_else(|_| caminho.to_path_buf());
    let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let mut lista = vec![abs.clone()];
    if let Ok(t) = std::fs::read_to_string(&f) {
        lista.extend(t.lines().filter(|l| !l.is_empty() && *l != abs).map(str::to_string));
    }
    lista.truncate(60);
    if let Some(p) = f.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(f, lista.join("\n"));
}

#[cfg(test)]
mod testes {
    use super::*;

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-busca-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn espera<T>(bg: &mut Bg<T>) -> Vec<Parcial<T>> {
        let mut tudo = Vec::new();
        let fim = Instant::now() + Duration::from_secs(10);
        while !bg.feita && Instant::now() < fim {
            tudo.extend(bg.novidades());
            std::thread::sleep(Duration::from_millis(10));
        }
        tudo.extend(bg.novidades());
        tudo
    }

    #[test]
    fn listar_em_segundo_plano_traz_a_lista_ordenada_e_enche_o_cache() {
        let d = tmp("bg-listar");
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("z.txt"), "1").unwrap();
        std::fs::write(d.join("a/b/c.rs"), "1").unwrap();
        std::fs::write(d.join("a/m.rs"), "1").unwrap();
        assert!(lista_em_cache(&d).is_none());
        let mut bg = listar_bg(d.clone(), Arc::new(|| {}));
        let msgs = espera(&mut bg);
        let completa = msgs.iter().find_map(|m| if let Parcial::Completa(v) = m { Some(v.clone()) } else { None }).expect("lista completa");
        assert_eq!(completa, listar(&d));
        assert_eq!(completa, vec!["a/b/c.rs", "a/m.rs", "z.txt"]);
        let (idade, em_cache) = lista_em_cache(&d).expect("guardada no cache");
        assert_eq!(em_cache, completa);
        assert!(idade < Duration::from_secs(5));
        esquecer_lista(&d);
        assert!(lista_em_cache(&d).is_none());
    }

    #[test]
    fn buscar_texto_em_segundo_plano_manda_lotes_e_pode_ser_cancelada() {
        let d = tmp("bg-texto");
        for i in 0..40 {
            std::fs::write(d.join(format!("f{i}.txt")), format!("nada\nachei agulha {i}\n")).unwrap();
        }
        let mut bg = buscar_texto_bg(d.clone(), "agulha".into(), 1000, Arc::new(|| {}));
        let msgs = espera(&mut bg);
        let achados: Vec<Achado> = msgs.into_iter().filter_map(|m| if let Parcial::Itens(v) = m { Some(v) } else { None }).flatten().collect();
        assert_eq!(achados.len(), 40);
        assert!(achados.iter().all(|a| a.linha == 1 && a.col == 6));
        // o limite vale
        let mut bg = buscar_texto_bg(d.clone(), "agulha".into(), 5, Arc::new(|| {}));
        let n: usize = espera(&mut bg).into_iter().filter_map(|m| if let Parcial::Itens(v) = m { Some(v.len()) } else { None }).sum();
        assert_eq!(n, 5);
        // cancelada: a thread pára e a seguinte funciona normalmente
        let bg = buscar_texto_bg(d.clone(), "agulha".into(), 1000, Arc::new(|| {}));
        drop(bg);
        let mut bg = buscar_texto_bg(d, "nada".into(), 1000, Arc::new(|| {}));
        let n: usize = espera(&mut bg).into_iter().filter_map(|m| if let Parcial::Itens(v) = m { Some(v.len()) } else { None }).sum();
        assert_eq!(n, 40);
    }

    #[test]
    fn fuzzy_incremental_so_olha_os_que_ja_casaram_e_acha_o_mesmo() {
        let itens: Vec<String> = ["src/main.rs", "src/native.rs", "src/nscene.rs", "docs/notes.md", "README.md", "src/term.rs"].map(String::from).to_vec();
        let (top1, todos1) = fuzzy_sub("n", &itens, None, 10);
        assert!(top1.len() >= 4 && todos1.len() == top1.len());
        // mais uma letra: só entre os de antes, com o mesmo resultado da busca completa
        let (top2, todos2) = fuzzy_sub("ns", &itens, Some(&todos1), 10);
        let completa = fuzzy("ns", &itens, 10);
        assert_eq!(top2, completa);
        assert!(todos2.iter().all(|i| todos1.contains(i)), "subconjunto");
        // sem busca: tudo, na ordem dada
        let (vazia, todos) = fuzzy_sub("", &itens, None, 3);
        assert_eq!(vazia.iter().map(|c| c.item).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(todos.len(), itens.len());
        // índices velhos fora do alcance não derrubam
        let (r, _) = fuzzy_sub("main", &itens, Some(&[0, 99]), 10);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn fuzzy_prefere_nome_de_arquivo_e_inicio_de_palavra() {
        let itens: Vec<String> = ["src/main.rs", "docs/maintenance/notes.md", "src/native.rs", "README.md", "src/nscene.rs"].map(String::from).to_vec();
        let r = fuzzy("main", &itens, 10);
        assert_eq!(itens[r[0].item], "src/main.rs", "{r:?}");
        let r = fuzzy("ns", &itens, 10);
        assert_eq!(itens[r[0].item], "src/nscene.rs");
        assert!(fuzzy("zzz", &itens, 10).is_empty());
        assert_eq!(fuzzy("", &itens, 3).len(), 3);
    }

    #[test]
    fn maiuscula_liga_a_diferenca_de_caixa() {
        let itens: Vec<String> = ["readme.md", "README.md"].map(String::from).to_vec();
        assert_eq!(fuzzy("README", &itens, 5).len(), 1);
        assert_eq!(fuzzy("readme", &itens, 5).len(), 2);
    }

    #[test]
    fn posicoes_apontam_os_caracteres_que_casaram() {
        let q: Vec<char> = "mr".chars().collect();
        let (_, pos) = pontuar(&q, "src/main.rs").unwrap();
        assert_eq!(pos, vec![4, 9]);
    }

    #[test]
    fn lista_respeita_gitignore_e_oculta() {
        let d = tmp("lista");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(d.join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(d.join("target/x.o"), "x").unwrap();
        std::fs::write(d.join(".segredo"), "x").unwrap();
        let l = listar(&d);
        assert_eq!(l, vec!["src/a.rs".to_string()], "{l:?}");
    }

    #[test]
    fn busca_de_texto_acha_linha_e_coluna() {
        let d = tmp("grep");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("a.txt"), "um\ndois agulha tres\nquatro\n").unwrap();
        std::fs::write(d.join("b.bin"), b"agulha\0\0").unwrap();
        let r = buscar_texto(&d, "agulha", 10, Duration::from_secs(2));
        assert_eq!(r, vec![Achado { arquivo: "a.txt".into(), linha: 1, texto: "dois agulha tres".into(), col: 5 }]);
        // regex inválida cai para texto literal
        std::fs::write(d.join("c.txt"), "abre ( aqui\n").unwrap();
        assert_eq!(buscar_texto(&d, "( aqui", 10, Duration::from_secs(2)).len(), 1);
    }

    #[test]
    fn recentes_guardam_sem_repetir() {
        let d = tmp("rec");
        std::env::set_var("ASTROGRAPH_STATE", d.join("estado"));
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        lembrar(&a);
        lembrar(&b);
        lembrar(&a);
        let r = recentes();
        assert_eq!(r.len(), 2);
        assert!(r[0].ends_with("a.txt") && r[1].ends_with("b.txt"), "{r:?}");
        std::env::remove_var("ASTROGRAPH_STATE");
    }
}
