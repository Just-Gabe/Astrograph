//! Harness: opera o editor nativo por comandos de texto, pela mesma sessão e pela mesma cena que o
//! app desenha. Serve para teste (prints de cada passo e de cada animação) e para agentes que operam
//! o Astrograph (ao vivo, pelo servidor de `controle.rs`). Cada comando devolve JSON.
//!
//! Comandos (um por linha; `#` comenta):
//!
//! - `teclas <notação>`: `:w<CR>`, `<C-BS>`, `<Space>ff`...
//! - `texto <literal>`: digita o texto como está (`<` vira `<lt>`)
//! - `estado`: modo, texto em volta do cursor, linha de comando, buscador, explorador, abas, avisos
//! - `print [arquivo.png]`: a tela como PNG
//! - `abrir <arquivo> [linha]`, `pasta <pasta>`, `salvar [todos]`, `comando <ex>` (sem o `:`)
//! - `buffer [arquivo]`: o texto da aba, com o que ainda não foi salvo
//! - `atalhos`: o menu da tecla líder; `acao <teclas>`: roda um atalho dele (`acao ff`)
//! - `terminal <comando>`: roda num terminal de shell do app; `terminal_tela`: o que ele mostra
//! - `git_status`, `git_diff [arquivo]`, `git_commit [--tudo] <mensagem>`
//! - `clique <linha> <coluna>`, `rolar <linhas>`
//!
//! Só no roteiro (`astrograph --roteiro arquivo.txt [--saida pasta] [--tamanho 1280x800] [arquivo]`):
//! `espera <ms>`, `quadros <nome> <ms> [passo_ms]`, `congelou <ms> [sem_animar] [salvar=nome]`,
//! `tamanho <L>x<A>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::proc::SemJanela;
use crate::{native, nscene, paint};

/// Um quadro a cada 16 ms, como o app com animação em curso.
const QUADRO: Duration = Duration::from_millis(16);

/// Tamanho da janela em pixels (largura << 32 | altura) e o tamanho da fonte (bits do f32): a janela
/// atualiza a cada quadro; o print ao vivo desenha do mesmo tamanho.
pub static JANELA: AtomicU64 = AtomicU64::new((1280u64 << 32) | 800);
pub static FONTE_PX: AtomicU32 = AtomicU32::new(0);

pub fn anotar_janela(w: u32, h: u32, font_px: f32) {
    JANELA.store(((w as u64) << 32) | h as u64, Ordering::Relaxed);
    FONTE_PX.store(font_px.to_bits(), Ordering::Relaxed);
}

/// A cena fora da janela: pinta o `Workspace` num buffer de pixels.
pub struct Pintor {
    shell: nscene::Scene,
    pool: HashMap<u32, nscene::Scene>,
    largura: u32,
    altura: u32,
    font_px: f32,
    buf: Vec<u32>,
}

impl Pintor {
    pub fn new(font_px: f32, (largura, altura): (u32, u32)) -> Option<Pintor> {
        Some(Pintor { shell: nscene::Scene::new(font_px, 1.0)?, pool: HashMap::new(), largura, altura, font_px, buf: Vec::new() })
    }

    pub fn animacao(&mut self, cursor_ms: u64, rolagem_ms: u64) {
        self.shell.set_animacao(cursor_ms, rolagem_ms);
    }

    /// Pinta um quadro em `now` e devolve se ainda há animação. Com `ajustar`, a sessão passa a ter o
    /// tamanho que a cena mede (como o app faz depois de cada quadro); ao vivo, quem manda é a janela.
    fn pintar(&mut self, w: &mut native::Workspace, now: Instant, ajustar: bool) -> nscene::Frame {
        let vm = Arc::new(w.view_model());
        let (lw, lh) = (self.largura, self.altura);
        self.buf.resize((lw * lh) as usize, 0);
        let mut c = paint::Canvas::new(&mut self.buf, lw, lh);
        if vm.layout.is_some() {
            let (f, rects, _, _) = crate::paint_paineis(&mut self.shell, &mut self.pool, &mut c, &vm, now, self.font_px, 1.0);
            if ajustar {
                let tam: Vec<(u32, usize, f32)> = rects
                    .iter()
                    .filter_map(|(id, r)| {
                        let lines = if *id == vm.pane { vm.lines } else { vm.panes.iter().find(|p| p.id == *id)?.vm.lines };
                        let (c, rows) = self.pool.get(id)?.tamanho_painel(r.w as u32, r.h as u32, lines);
                        Some((*id, c, rows))
                    })
                    .collect();
                w.tamanhos_paineis(&tam);
            }
            return f;
        }
        let f = self.shell.paint(&mut c, &vm, now);
        if ajustar {
            let lay = self.shell.layout(lw, lh, vm.lines);
            w.resize(lay.cols.max(1), lay.rows.floor().max(1.0));
            let (tr, tc) = self.shell.term_dims(lw, lh);
            w.term_resize(tr.max(2) as u16, tc.max(10) as u16);
        }
        f
    }

    /// Pinta até as animações assentarem (o tempo da cena avança sem esperar o relógio).
    fn assentar(&mut self, w: &mut native::Workspace) {
        let t0 = Instant::now();
        for i in 0..40 {
            let f = self.pintar(w, t0 + Duration::from_millis(40 * i), false);
            if i > 2 && !f.animating {
                break;
            }
        }
    }

    pub fn png_bytes(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.largura, self.altura);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        let mut rgb = Vec::with_capacity(self.buf.len() * 3);
        for p in &self.buf {
            rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        wr.write_image_data(&rgb).map_err(|e| e.to_string())?;
        drop(wr);
        Ok(out)
    }

    pub fn png(&self, caminho: &Path) -> Result<(), String> {
        if let Some(p) = caminho.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(caminho, self.png_bytes()?).map_err(|e| e.to_string())
    }
}

/// Onde os prints vão quando o pedido não diz o arquivo.
pub fn pasta_estado() -> PathBuf {
    std::env::var_os("ASTROGRAPH_STATE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("astrograph-state")))
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(|p| PathBuf::from(p).join("astrograph")))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state/astrograph")))
        .unwrap_or_else(std::env::temp_dir)
}

/// Caminho do pedido: relativo vale a partir de `base`.
fn resolver(base: &Path, p: &str) -> PathBuf {
    let p = PathBuf::from(p.trim().trim_matches('"'));
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

/// Estado da tela em JSON: o que um agente (ou um teste) precisa para saber onde está.
pub fn estado(w: &mut native::Workspace) -> Value {
    let vm = w.view_model();
    let (row, col) = vm.cursor_real;
    let ini = vm.cursor.0.saturating_sub(3);
    let linhas: Vec<String> = (ini..(vm.cursor.0 + 4).min(vm.lines)).map(|r| vm.line(r)).collect();
    json!({
        "pasta": w.raiz().to_string_lossy(),
        "arquivo": vm.name,
        "modificado": vm.modified,
        "modo": format!("{:?}", vm.mode),
        "cursor": { "linha": row + 1, "coluna": col + 1 },
        "linhas": vm.lines,
        "topo": vm.top as usize + 1,
        "trecho": { "primeira": ini + 1, "texto": linhas },
        "selecao": vm.selection.map(|(k, a, b)| json!({ "tipo": format!("{k:?}"), "de": [a.0 + 1, a.1 + 1], "ate": [b.0 + 1, b.1 + 1] })),
        "comando": vm.cmd.as_ref().map(|(c, t)| format!("{c}{t}")),
        "aviso": vm.toast.as_ref().map(|(_, t, k)| json!({ "texto": t, "tipo": format!("{k:?}") })),
        "tela_inicial": vm.dash,
        "buscador": vm.picker.as_ref().map(|p| json!({
            "titulo": p.title,
            "consulta": p.query,
            "selecionado": p.sel,
            "total": p.total,
            "buscando": p.buscando,
            "itens": p.items.iter().take(12).map(|i| if i.detail.is_empty() { i.text.clone() } else { format!("{}  {}", i.text, i.detail) }).collect::<Vec<_>>(),
        })),
        "explorador": vm.tree.as_ref().map(|t| json!({
            "raiz": t.root,
            "foco": t.focus,
            "selecionado": t.items.get(t.sel.saturating_sub(t.first)).map(|i| i.name.clone()),
        })),
        "terminal": vm.term.as_ref().map(|t| json!({ "foco": t.focus, "lateral": t.lateral, "zen": t.zen, "terminou": t.screen.exited })),
        "atalhos": vm.whichkey.iter().map(|(k, d)| format!("{k} {d}")).collect::<Vec<_>>(),
        "abas": vm.tabs.iter().map(|t| json!({ "nome": t.name, "modificada": t.modified })).collect::<Vec<_>>(),
        "aba_ativa": vm.tab_active + 1,
        "paineis": 1 + vm.panes.len(),
        "diagnosticos": vm.diags.iter().take(20).map(|d| json!({ "linha": d.row + 1, "coluna": d.col + 1, "gravidade": d.severity, "mensagem": d.message })).collect::<Vec<_>>(),
        "git": vm.git.as_ref().map(|g| g.branch.clone()),
        "progresso": vm.progress,
    })
}

/// Quanto um comando git pode demorar (hook, rede) antes de ser encerrado.
const GIT_LIMITE: Duration = Duration::from_secs(30);

/// Roda o git sem entrada (nunca para esperando senha ou credencial) e com tempo-limite.
fn git(raiz: &Path, args: &[String]) -> Result<String, String> {
    use std::io::Read;
    let mut filho = std::process::Command::new("git")
        .args(args)
        .current_dir(raiz)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .sem_janela()
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let ler = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut b);
            }
            b
        })
    };
    let (o, e) = (ler(filho.stdout.take().map(|x| Box::new(x) as _)), ler(filho.stderr.take().map(|x| Box::new(x) as _)));
    let fim = Instant::now() + GIT_LIMITE;
    let status = loop {
        match filho.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < fim => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                return Err(format!("git {}: passou de {} s e foi encerrado (hook ou pedido de senha?)", args.join(" "), GIT_LIMITE.as_secs()));
            }
        }
    };
    let texto = format!("{}{}", String::from_utf8_lossy(&o.join().unwrap_or_default()), String::from_utf8_lossy(&e.join().unwrap_or_default()));
    if status.success() {
        Ok(texto)
    } else {
        Err(format!("git {}: {}", args.join(" "), texto.trim()))
    }
}

/// Um pedido de git pronto para rodar fora da task do editor.
pub struct TrabalhoGit {
    raiz: PathBuf,
    passos: Vec<Vec<String>>,
    chave: &'static str,
    /// avisos que vão junto da resposta (abas não salvas)
    extra: Value,
    commit: bool,
}

/// `git_status`, `git_diff [arquivo]`, `git_commit [--tudo] [--em pasta] -- mensagem`. As opções só valem antes
/// do `--`; sem ele, tudo é a mensagem (uma mensagem que começa com "--tudo" não vira opção).
pub fn preparar_git(w: &mut native::Workspace, linha: &str) -> Option<Result<TrabalhoGit, String>> {
    let linha = linha.trim();
    let (cmd, resto) = linha.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((linha, ""));
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Some(Ok(match cmd {
        "git_status" => TrabalhoGit { raiz: w.raiz(), passos: vec![s(&["status", "--short", "--branch"])], chave: "status", extra: Value::Null, commit: false },
        "git_diff" => {
            let mut a = s(&["diff", "--stat", "--patch", "--no-color"]);
            if !resto.is_empty() {
                a.extend(s(&["--", resto]));
            }
            TrabalhoGit { raiz: w.raiz(), passos: vec![a], chave: "diff", extra: Value::Null, commit: false }
        }
        "git_commit" => {
            // `-- msg`: sem opções; `opções -- msg`: o primeiro " -- " separa (a mensagem pode ter outros)
            let (opcoes, msg) = match resto.strip_prefix("-- ") {
                Some(m) => ("", m.trim()),
                None => match resto.split_once(" -- ") {
                    Some((o, m)) => (o.trim(), m.trim()),
                    None => ("", resto),
                },
            };
            let mut tudo = false;
            let mut raiz = w.raiz();
            let mut resto_op = opcoes;
            while !resto_op.is_empty() {
                if let Some(r) = resto_op.strip_prefix("--tudo") {
                    tudo = true;
                    resto_op = r.trim_start();
                } else if let Some(p) = resto_op.strip_prefix("--em ") {
                    // `--em` vai por último e leva o resto (a pasta pode ter espaços)
                    raiz = resolver(&w.raiz(), p.trim());
                    resto_op = "";
                } else {
                    return Some(Err(format!("git_commit: opção desconhecida {resto_op} (use [--tudo] [--em pasta] -- mensagem)")));
                }
            }
            if msg.is_empty() {
                return Some(Err("git_commit: falta a mensagem (git_commit [--tudo] [--em pasta] -- mensagem)".into()));
            }
            // não grava abas por conta própria: o commit leva o que está no disco, e a resposta avisa o que não está
            let nao_salvas = w.nao_salvas_em(&raiz);
            let mut passos = Vec::new();
            if tudo {
                passos.push(s(&["add", "-A"]));
            }
            passos.push(vec!["commit".into(), "-m".into(), msg.to_string()]);
            TrabalhoGit { raiz, passos, chave: "saida", extra: json!({ "abas_nao_salvas": nao_salvas }), commit: true }
        }
        _ => return None,
    }))
}

/// Roda os passos do pedido (fora da task do editor ao vivo).
pub fn rodar_git(t: TrabalhoGit) -> Result<Value, String> {
    let mut saida = String::new();
    for passo in &t.passos {
        saida.push_str(&git(&t.raiz, passo)?);
    }
    let mut r = json!({ "pasta": t.raiz.to_string_lossy(), t.chave: saida });
    if t.commit {
        r["ok"] = json!(true);
        r["staged_com_tudo"] = json!(t.passos.len() > 1);
    }
    if let (Some(o), Some(e)) = (r.as_object_mut(), t.extra.as_object()) {
        o.extend(e.clone());
    }
    Ok(r)
}

/// Ao vivo ou no roteiro: onde gravar os prints e se a sessão segue o tamanho do pintor.
pub struct Contexto {
    pub saida: PathBuf,
    pub ao_vivo: bool,
}

/// Executa um comando comum ao roteiro e ao app aberto. `None` = não é um destes comandos.
pub fn executar(w: &mut native::Workspace, pintor: &mut Pintor, ctx: &Contexto, linha: &str) -> Option<Result<Value, String>> {
    let linha = linha.trim();
    let (cmd, resto) = linha.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((linha, ""));
    let ok = || Ok(json!({ "ok": true }));
    let raiz = w.raiz();
    Some(match cmd {
        "teclas" | "keys" => {
            w.feed(resto);
            ok()
        }
        "texto" | "text" => {
            w.feed(&resto.replace('<', "<lt>"));
            ok()
        }
        "estado" | "state" => Ok(estado(w)),
        "print" | "screenshot" => {
            if ctx.ao_vivo {
                pintor.assentar(w);
            } else {
                pintor.pintar(w, Instant::now(), true);
            }
            // só o nome do arquivo: o print fica sempre na pasta de prints (nada de caminho absoluto ou `..`)
            let nome = Path::new(resto).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty() && n != "..");
            let nome = nome.unwrap_or_else(|| format!("print-{}", chrono::Local::now().format("%Y%m%d-%H%M%S%.3f")));
            let nome = if nome.ends_with(".png") { nome } else { format!("{nome}.png") };
            let p = ctx.saida.join(&nome);
            pintor.png(&p).map(|_| json!({ "png": p.to_string_lossy(), "largura": pintor.largura, "altura": pintor.altura }))
        }
        "abrir" | "open" => {
            // `abrir caminho com espaço.rs 12`: o último número é a linha
            let (arq, lin) = match resto.rsplit_once(' ') {
                Some((a, n)) if n.parse::<usize>().is_ok() => (a, n.parse().ok()),
                _ => (resto, None),
            };
            if arq.is_empty() {
                return Some(Err("abrir: falta o arquivo".into()));
            }
            let p = resolver(&raiz, arq);
            if !p.is_file() {
                return Some(Err(format!("abrir: arquivo não existe: {}", p.display())));
            }
            w.abrir(&p, lin);
            Ok(json!({ "ok": true, "arquivo": p.to_string_lossy() }))
        }
        "pasta" | "folder" => {
            let p = resolver(&raiz, resto);
            if !p.is_dir() {
                return Some(Err(format!("pasta: não existe: {}", p.display())));
            }
            w.feed_editor(&format!(":cd {}<CR>", p.to_string_lossy().replace('<', "<lt>")), true);
            // o `:cd` recusa (aba sem salvar…) só com um aviso na tela: a resposta não pode dizer ok
            let alvo = p.canonicalize().map(|c| crate::busca::sem_prefixo_unc(&c)).unwrap_or(p);
            if w.raiz() != alvo {
                return Some(Err(format!("pasta: não trocou ({})", estado(w)["aviso"]["texto"].as_str().unwrap_or("sem aviso"))));
            }
            Ok(json!({ "ok": true, "pasta": w.raiz().to_string_lossy() }))
        }
        "salvar" | "save" => {
            if resto == "todos" || resto == "all" {
                let (gravadas, sem_nome) = w.salvar_todas();
                Ok(json!({ "gravadas": gravadas, "abas_sem_nome": sem_nome }))
            } else {
                w.feed_editor(":w<CR>", true);
                let e = estado(w);
                if e["modificado"] == json!(true) {
                    Err(format!("salvar: não gravou ({})", e["aviso"]["texto"].as_str().unwrap_or("sem nome? use `comando w arquivo`")))
                } else {
                    Ok(json!({ "ok": true, "arquivo": e["arquivo"] }))
                }
            }
        }
        "comando" | "ex" => {
            let c = resto.trim_start_matches(':');
            w.feed_editor(&format!(":{}<CR>", c.replace('<', "<lt>")), true);
            Ok(json!({ "ok": true, "aviso": estado(w)["aviso"] }))
        }
        "buffer" => {
            let alvo = (!resto.is_empty()).then(|| resolver(&raiz, resto));
            match w.texto(alvo.as_deref()) {
                Some((nome, texto, modificado)) => Ok(json!({ "arquivo": nome, "modificado": modificado, "texto": texto })),
                None => Err(format!("buffer: {resto} não está aberto")),
            }
        }
        "atalhos" | "menu" => {
            let lider = native::tecla_nome(w.lider());
            Ok(json!({ "lider": lider, "atalhos": crate::atalhos::LIDER.iter().map(|(t, d)| json!({ "teclas": t, "faz": d })).collect::<Vec<_>>() }))
        }
        "acao" | "action" => {
            if !crate::atalhos::LIDER.iter().any(|(t, _)| *t == resto) {
                return Some(Err(format!("acao: `{resto}` não está no menu (veja `atalhos`)")));
            }
            // não derruba o que o dono está fazendo (inserção, buscador, linha de comando): recusa e explica
            if !w.livre_para_atalho() {
                return Some(Err(format!("acao: o editor está ocupado ({}); mande `teclas <Esc>` antes se for o caso", estado(w)["modo"].as_str().unwrap_or("?"))));
            }
            let l = w.lider();
            let lider = if l == ' ' { "<Space>".to_string() } else { l.to_string() };
            // a ação decide para onde vai o foco (explorador, buscador…): o terminal não o recebe de volta
            w.feed_editor(&format!("{lider}{resto}"), false);
            ok()
        }
        "terminal" => {
            if resto.is_empty() {
                return Some(Err("terminal: falta o comando".into()));
            }
            w.terminal_rodar(resto);
            ok()
        }
        "terminal_tela" => match w.terminal_tela() {
            Some((linhas, terminou)) => Ok(json!({ "linhas": linhas, "terminou": terminou })),
            None => Err("terminal_tela: nenhum terminal aberto".into()),
        },
        "clique" | "click" => {
            let v: Vec<usize> = resto.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            let [l, c] = v[..] else { return Some(Err("clique: use `clique <linha> <coluna>`".into())) };
            w.click(l.saturating_sub(1), c.saturating_sub(1));
            ok()
        }
        "rolar" | "scroll" => match resto.parse::<f32>() {
            Ok(n) => {
                w.scroll(n);
                ok()
            }
            Err(_) => Err("rolar: número de linhas inválido".into()),
        },
        _ => return None,
    })
}

/// O roteiro: um `Workspace` só seu, um pintor e os comandos de tempo (esperar, quadros, congelou).
pub struct Harness {
    pub w: native::Workspace,
    pintor: Pintor,
    ctx: Contexto,
    /// uma thread da sessão (buscador, LSP, terminal) avisou que há novidade: o app repintaria
    acordar: Arc<AtomicBool>,
}

impl Harness {
    pub fn new(arquivo: Option<PathBuf>, leader: char, font_px: f32, tamanho: (u32, u32), saida: PathBuf) -> Option<Harness> {
        let pintor = Pintor::new(font_px, tamanho)?;
        let acordar = Arc::new(AtomicBool::new(false));
        let a = acordar.clone();
        let w = native::Workspace::new(arquivo, leader, Arc::new(move || a.store(true, Ordering::SeqCst)));
        let mut h = Harness { w, pintor, ctx: Contexto { saida, ao_vivo: false }, acordar };
        h.quadro();
        h.quadro();
        Some(h)
    }

    pub fn animacao(&mut self, cursor_ms: u64, rolagem_ms: u64) {
        self.pintor.animacao(cursor_ms, rolagem_ms);
    }

    fn pintar(&mut self, now: Instant) -> nscene::Frame {
        self.pintor.pintar(&mut self.w, now, true)
    }

    fn quadro(&mut self) -> bool {
        self.pintar(Instant::now()).animating
    }

    /// Pinta como o app pinta: só enquanto a cena diz que anima (a cada 7 ms) ou quando ela pede para
    /// acordar. Depois de `ms`, compara a última imagem pintada com um quadro novo: se diferem, a tela do app
    /// teria ficado parada no meio de uma animação até a próxima tecla.
    pub fn congelou(&mut self, ms: u64, sem_animar: bool, salvar: Option<&str>) -> Value {
        let fim = Instant::now() + Duration::from_millis(ms);
        let mut quadros = 0u32;
        let mut proximo = Some(Instant::now());
        let mut ultimo = Vec::new();
        let mut ultimo_em = Instant::now();
        while Instant::now() < fim {
            // espera o próximo quadro pedido, mas acorda antes se uma thread avisar (como o app)
            let alvo = proximo.unwrap_or(fim).min(fim);
            while Instant::now() < alvo && !self.acordar.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            if Instant::now() >= fim {
                break;
            }
            self.acordar.store(false, Ordering::SeqCst);
            let agora = Instant::now();
            let f = self.pintor.pintar(&mut self.w, agora, false);
            quadros += 1;
            ultimo.clone_from(&self.pintor.buf);
            ultimo_em = agora;
            proximo = (f.animating && !sem_animar).then(|| agora + Duration::from_millis(7));
            if let Some(d) = f.wake {
                let t = agora + d;
                proximo = Some(proximo.map_or(t, |a: Instant| a.min(t)));
            }
        }
        // ainda pedindo quadros no fim da janela: a animação não assenta (o app repintaria para sempre)
        let nunca_parou = proximo.is_some_and(|t| t.saturating_duration_since(ultimo_em) < Duration::from_millis(20)) && ultimo_em.elapsed() < Duration::from_millis(30);
        if let Some(d) = fim.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
        self.quadro();
        if let Some(nome) = salvar {
            let real = std::mem::replace(&mut self.pintor.buf, ultimo.clone());
            let _ = self.pintor.png(&self.ctx.saida.join(format!("{nome}-parado.png")));
            self.pintor.buf = real;
            let _ = self.pintor.png(&self.ctx.saida.join(format!("{nome}-real.png")));
        }
        let w = self.pintor.largura as usize;
        let (mut diferentes, mut maior) = (0usize, 0u32);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for (i, (a, b)) in ultimo.iter().zip(&self.pintor.buf).enumerate() {
            if a == b {
                continue;
            }
            diferentes += 1;
            let d = (0..3).map(|k| ((a >> (8 * k)) & 0xff).abs_diff((b >> (8 * k)) & 0xff)).max().unwrap_or(0);
            maior = maior.max(d);
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        let caixa = (diferentes > 0).then(|| json!([x0, y0, x1, y1]));
        json!({ "quadros_pintados": quadros, "pixels_diferentes": diferentes, "maior_diferenca": maior, "onde": caixa, "parou_ha_ms": ultimo_em.elapsed().as_millis() as u64, "congelou": diferentes > 0, "nunca_parou": nunca_parou })
    }

    /// O tempo corre `ms`, com um quadro a cada 16 ms (respostas de threads e animações chegam).
    pub fn esperar(&mut self, ms: u64) {
        let fim = Instant::now() + Duration::from_millis(ms);
        loop {
            self.quadro();
            let agora = Instant::now();
            if agora >= fim {
                break;
            }
            std::thread::sleep(QUADRO.min(fim - agora));
        }
    }

    /// Executa um comando do roteiro e devolve o resultado (ou o erro) em JSON.
    pub fn comando(&mut self, linha: &str) -> Result<Value, String> {
        let linha = linha.trim();
        let (cmd, resto) = linha.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((linha, ""));
        let r = match cmd {
            "espera" | "wait" => {
                let ms: u64 = resto.parse().map_err(|_| format!("espera: número de ms inválido: {resto:?}"))?;
                self.esperar(ms);
                Ok(json!({ "ok": true }))
            }
            "quadros" | "frames" => {
                let mut it = resto.split_whitespace();
                let nome = it.next().ok_or("quadros: falta o nome")?.to_string();
                let ms: u64 = it.next().and_then(|v| v.parse().ok()).ok_or("quadros: falta a duração em ms")?;
                let passo: u64 = it.next().and_then(|v| v.parse().ok()).unwrap_or(16).max(1);
                let t0 = Instant::now();
                let mut arquivos = Vec::new();
                let mut i = 0u64;
                while i * passo <= ms {
                    let alvo = t0 + Duration::from_millis(i * passo);
                    if let Some(d) = alvo.checked_duration_since(Instant::now()) {
                        std::thread::sleep(d);
                    }
                    self.pintar(alvo);
                    let p = self.ctx.saida.join(format!("{nome}-{:04}.png", i * passo));
                    self.pintor.png(&p).map_err(|e| format!("quadros: {e}"))?;
                    arquivos.push(p.to_string_lossy().into_owned());
                    i += 1;
                }
                Ok(json!({ "pngs": arquivos }))
            }
            "congelou" | "stall" => {
                let mut it = resto.split_whitespace();
                let ms: u64 = it.next().and_then(|v| v.parse().ok()).ok_or_else(|| format!("congelou: número de ms inválido: {resto:?}"))?;
                let resto: Vec<&str> = it.collect();
                let salvar = resto.iter().find_map(|a| a.strip_prefix("salvar="));
                Ok(self.congelou(ms, resto.contains(&"sem_animar"), salvar))
            }
            "tamanho" | "size" => {
                let (a, b) = resto.split_once('x').ok_or("tamanho: use 1280x800")?;
                self.pintor.largura = a.trim().parse().map_err(|_| "tamanho: largura inválida")?;
                self.pintor.altura = b.trim().parse().map_err(|_| "tamanho: altura inválida")?;
                Ok(json!({ "ok": true }))
            }
            "" => Ok(Value::Null),
            _ if preparar_git(&mut self.w, linha).is_some() => {
                let r = preparar_git(&mut self.w, linha).unwrap().and_then(rodar_git);
                self.w.git_recarregar();
                r
            }
            _ => match executar(&mut self.w, &mut self.pintor, &self.ctx, linha) {
                Some(r) => r,
                None => Err(format!("comando desconhecido: {cmd}")),
            },
        };
        self.quadro();
        r
    }
}

/// Comandos que mudam algo: aparecem como aviso na tela do dono e vão para o registro.
fn muda_algo(cmd: &str) -> bool {
    matches!(cmd, "teclas" | "keys" | "texto" | "text" | "comando" | "ex" | "abrir" | "open" | "pasta" | "folder" | "salvar" | "save" | "acao" | "action" | "terminal" | "git_commit" | "clique" | "click" | "rolar" | "scroll")
}

/// Registro das ações dos agentes (`agentes.log` na pasta de estado; passa de 1 MB, vira `.1`).
fn registrar(linha: &str, ok: bool) {
    let arq = pasta_estado().join("agentes.log");
    if std::fs::metadata(&arq).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::rename(&arq, arq.with_extension("log.1"));
    }
    let resumo: String = linha.chars().take(300).collect();
    let l = format!("{}\t{}\t{}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), if ok { "ok" } else { "erro" }, resumo.replace('\n', " "));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&arq) {
        let _ = std::io::Write::write_all(&mut f, l.as_bytes());
    }
}

/// Ao vivo: o pintor nasce no primeiro pedido que precisa dele, do tamanho da janela. A resposta vai pelo
/// canal; o git roda fora da task do editor (um hook lento não congela a tela).
pub fn executar_ao_vivo(w: &mut native::Workspace, pintor: &mut Option<Pintor>, linha: &str, resp: tokio::sync::oneshot::Sender<Result<Value, String>>) {
    let cmd = linha.split_whitespace().next().unwrap_or("").to_string();
    let resumo: String = linha.chars().take(80).collect();
    if muda_algo(&cmd) {
        w.aviso_agente(format!("Agente: {resumo}"));
    }
    if cmd == "git_recarregar" {
        w.git_recarregar();
        let _ = resp.send(Ok(json!({ "ok": true })));
        return;
    }
    if let Some(trabalho) = preparar_git(w, linha) {
        let linha = linha.to_string();
        match trabalho {
            Err(e) => {
                registrar(&linha, false);
                let _ = resp.send(Err(e));
            }
            Ok(t) => {
                let commit = t.commit;
                tokio::task::spawn_blocking(move || {
                    let r = rodar_git(t);
                    let ok = r.is_ok();
                    let _ = resp.send(r);
                    if commit {
                        registrar(&linha, ok);
                        // a margem e a branch do editor acompanham o commit (depois da resposta)
                        crate::controle::pedido_interno("git_recarregar");
                    }
                });
            }
        }
        return;
    }
    let r = (|| {
        let janela = JANELA.load(Ordering::Relaxed);
        let tamanho = ((janela >> 32) as u32, janela as u32);
        let fonte = f32::from_bits(FONTE_PX.load(Ordering::Relaxed));
        let fonte = if fonte > 4.0 { fonte } else { crate::config::atual().font.size };
        if pintor.as_ref().is_none_or(|p| (p.largura, p.altura) != tamanho || p.font_px != fonte) {
            *pintor = Pintor::new(fonte, tamanho);
        }
        let Some(p) = pintor.as_mut() else { return Err("sem fonte para desenhar".into()) };
        let ctx = Contexto { saida: pasta_estado().join("prints"), ao_vivo: true };
        if matches!(cmd.as_str(), "espera" | "wait" | "quadros" | "frames" | "congelou" | "stall" | "tamanho" | "size") {
            return Err(format!("{cmd}: só no roteiro (ao vivo, espere no cliente: `astrograph --ctl espera 500`)"));
        }
        let r = executar(w, p, &ctx, linha).unwrap_or_else(|| Err(format!("comando desconhecido: {cmd}")));
        if cmd == "print" || cmd == "screenshot" {
            limpar_prints(&ctx.saida);
        }
        r
    })();
    if muda_algo(&cmd) {
        registrar(linha, r.is_ok());
    }
    let _ = resp.send(r);
}

/// Mantém só os 50 prints mais novos.
fn limpar_prints(pasta: &Path) {
    let Ok(rd) = std::fs::read_dir(pasta) else { return };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "png")).filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    if v.len() <= 50 {
        return;
    }
    v.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in v.into_iter().skip(50) {
        let _ = std::fs::remove_file(p);
    }
}

/// `--roteiro`: lê os comandos do arquivo (ou `-` para a entrada padrão) e imprime uma linha
/// JSON por comando. Sai com 1 se algum comando falhar.
pub fn rodar_roteiro(roteiro: &str, saida: PathBuf, tamanho: (u32, u32), arquivo: Option<PathBuf>, leader: char, font_px: f32, anim: (u64, u64)) -> i32 {
    let texto = if roteiro == "-" {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s);
        s
    } else {
        match std::fs::read_to_string(roteiro) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("roteiro {roteiro}: {e}");
                return 2;
            }
        }
    };
    let Some(mut h) = Harness::new(arquivo, leader, font_px, tamanho, saida) else {
        eprintln!("sem fonte para desenhar");
        return 2;
    };
    h.animacao(anim.0, anim.1);
    let mut falhou = false;
    for (n, linha) in texto.lines().enumerate() {
        let l = linha.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        match h.comando(l) {
            Ok(Value::Null) => {}
            Ok(v) => println!("{}", json!({ "linha": n + 1, "comando": l, "resultado": v })),
            Err(e) => {
                falhou = true;
                println!("{}", json!({ "linha": n + 1, "comando": l, "erro": e }));
            }
        }
        if h.w.sair {
            break;
        }
    }
    i32::from(falhou)
}
