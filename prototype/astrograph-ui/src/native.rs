//! Editor nativo: o motor modal em Rust (`astrograph-core`) sem Neovim.
//! `ASTROGRAPH_ENGINE=native [arquivo]`.
//!
//! `Session` não conhece janela nem threads: recebe teclas, redimensionamentos,
//! cliques e rolagem, e publica um `ViewModel` (texto, cores, cursor, estado). Quem
//! desenha e anima é a cena (`nscene`). Por isso é testável sem GUI.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use astrograph_core::motion::{col_for_vcol, vcol_cursor, vcol_start};
use astrograph_core::buffer::Pos;
use astrograph_core::{parse_keys, Editor, Key, Mode, VKind};
use astrograph_syntax::{detect, lang, Document, Lang};

use crate::design::{classify, ModeKind};
use crate::arvore::Arvore;
use crate::busca;
use crate::git;
use crate::janelas::{Dir, Lado, Layout, PaneId};
use crate::lsp::{self, Cliente};
use crate::nview::{CompItem, CompView, GitView, PickItem, PickerView, Shared, Span, TermView, ToastKind, TreeItem, TreeView, ViewModel};
use crate::term::{self, Term, TermScreen};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickKind {
    Arquivos,
    Texto,
    Recentes,
    /// lista pronta (ações de código, referências, símbolos, diagnósticos)
    Lista,
}

/// O que acontece ao escolher um item de uma lista.
#[derive(Clone, Debug)]
enum Destino {
    /// arquivo, linha, coluna UTF-16 (como o servidor manda)
    Local(PathBuf, usize, usize),
    /// ação de código
    Acao(lsp::Acao),
}

#[derive(Clone, Debug)]
struct ListaItem {
    texto: String,
    detalhe: String,
    destino: Destino,
}

/// Pedido ao servidor de linguagem que espera resposta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PedidoLsp {
    Renomear,
    Acoes,
    /// formatar o arquivo; `salvar`: gravar quando a resposta chegar (formatar ao salvar); `sair`: e fechar (`:wq`)
    Formatar { salvar: bool, sair: bool },
    Assinatura,
    Referencias,
    Implementacao,
    TipoDaDefinicao,
    Simbolos,
}

struct Pendente {
    id: i64,
    tipo: PedidoLsp,
    desde: std::time::Instant,
}

/// Quanto esperar o servidor antes de gravar mesmo sem a formatação.
const FORMATAR_ESPERA_MS: u128 = 2500;

struct Picker {
    kind: PickKind,
    titulo: String,
    lista: Vec<ListaItem>,
    /// índice em `lista` de cada item mostrado
    mapa: Vec<usize>,
    query: String,
    /// candidatos (arquivos/recentes); no texto, fica vazio
    base: Vec<String>,
    items: Vec<PickItem>,
    achados: Vec<busca::Achado>,
    total: usize,
    sel: usize,
}

const PICK_LINHAS: usize = 10;

struct Comp {
    start: Pos,
    items: Vec<lsp::Item>,
    /// os itens filtrados: (índice em `items`, posições que casaram)
    shown: Vec<(usize, Vec<usize>)>,
    sel: usize,
}

const COMP_LINHAS: usize = 8;

/// Um servidor de linguagem compartilhado por todas as abas.
pub type LspRef = std::sync::Arc<std::sync::Mutex<Cliente>>;
pub type LspPool = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, LspRef>>>;

/// Pedidos que a sessão faz ao espaço de trabalho (abas).
#[derive(Clone, Debug, PartialEq)]
pub enum PedidoAba {
    Nova(Option<PathBuf>),
    Fechar { forcar: bool },
    Proxima,
    Anterior,
    So,
}

/// Pedidos de janela (divisões) que a sessão faz ao espaço de trabalho.
#[derive(Clone, Debug, PartialEq)]
pub enum PedidoJanela {
    /// `:split`/`:vsplit`/`:new`/`:vnew`: o novo painel mostra o mesmo arquivo, `arquivo` ou um buffer vazio
    Dividir { dir: Dir, arquivo: Option<PathBuf>, vazio: bool },
    Foco(Lado),
    /// `<C-w>w` (+1), `<C-w>W` (-1)
    Ciclo(i32),
    /// `<C-w>p`: o painel anterior
    Anterior,
    Fechar { forcar: bool },
    So,
    Igualar,
    Maximizar(Dir),
}

/// Os outros painéis de uma aba. Mora na sessão focada: trocar o foco troca quem a carrega.
pub struct Splits {
    pub layout: Layout,
    pub outros: Vec<Session>,
    pub proximo_id: PaneId,
    pub ultimo: Option<PaneId>,
}

static PROXIMO_DOC: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct Session {
    pub ed: Editor,
    pub path: Option<PathBuf>,
    saved_rev: u64,
    /// linha de cima desejada (a cena suaviza)
    top: f32,
    left: usize,
    cols: usize,
    rows: f32,
    lang: Option<&'static Lang>,
    doc: Option<(Document, u64)>,
    toast: Option<(u64, String, ToastKind)>,
    toast_id: u64,
    seq: u64,
    dash: bool,
    picker: Option<Picker>,
    tree: Option<Arvore>,
    tree_focus: bool,
    term: Option<Term>,
    term_focus: bool,
    term_zen: bool,
    lsp: Option<LspRef>,
    lsp_pool: LspPool,
    comp: Option<Comp>,
    comp_req: Option<(i64, Pos)>,
    /// versão do texto em que o completar foi aceito/dispensado: não reabre nela
    comp_block: u64,
    lsp_rev: u64,
    lsp_diags: std::sync::Arc<Vec<lsp::Diag>>,
    hover_wait: Option<i64>,
    hover_text: Option<String>,
    def_wait: Option<i64>,
    git_branch: Option<String>,
    /// o arquivo em `HEAD` (None: fora de um repositório)
    git_base: Option<Vec<String>>,
    git_rev: u64,
    git_signs: std::sync::Arc<Vec<(usize, git::Sign)>>,
    /// apertou <C-\\>: o próximo <C-n> devolve o teclado ao editor
    term_esc: bool,
    term_dims: (u16, u16),
    term_screen: Option<std::sync::Arc<TermScreen>>,
    notify: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    leader: Vec<Key>,
    /// tecla líder (padrão Espaço)
    leader_key: char,
    root: PathBuf,
    bump: Option<(u64, f32)>,
    hl_pat: String,
    hl_id: u64,
    hl_off: bool,
    pub quit: bool,
    /// identifica o documento (muda a cada sessão nova)
    pub doc_id: u64,
    /// dentro do espaço de trabalho: abrir arquivo vira aba nova
    pub modo_abas: bool,
    pub abrir_pedido: Option<(PathBuf, Option<(usize, usize)>)>,
    pub aba_pedido: Option<PedidoAba>,
    pub jan_pedido: Option<PedidoJanela>,
    /// pedidos ao servidor de linguagem esperando resposta
    lsp_pend: Vec<Pendente>,
    /// trocas de texto do servidor (renomear, ações) que o espaço de trabalho aplica em todos os arquivos
    pub edicoes_pedido: Option<Vec<lsp::Edicao>>,
    /// assinatura da função em edição (aberta ao digitar `(` ou `,`)
    sig: Option<lsp::Assinatura>,
    /// pede ao servidor para formatar antes de gravar (`[lsp] format_on_save`)
    pub formatar_ao_salvar: bool,
    /// apertou <C-w> e espera a segunda tecla
    jan_espera: bool,
    /// identifica o painel dentro da aba
    pub pane_id: PaneId,
    pub splits: Option<Box<Splits>>,
    /// outro painel da aba mostra o mesmo arquivo (o buffer é compartilhado)
    pub irmao: bool,
}

const SCROLLOFF: f32 = 3.0;
/// linhas de realce calculadas além da tela, para a rolagem suave não ficar sem cor
const SPAN_MARGIN: usize = 80;

impl Drop for Session {
    fn drop(&mut self) {
        // o arquivo fecha no servidor (que segue vivo para as outras abas)
        if let (Some(c), Some(p)) = (&self.lsp, &self.path) {
            if let Ok(mut c) = c.lock() {
                c.fechar(p);
            }
        }
    }
}

impl Session {
    pub fn new(text: &str, path: Option<PathBuf>) -> Self {
        let lines: Vec<String> = text.strip_suffix('\n').unwrap_or(text).split('\n').map(str::to_string).collect();
        let ed = Editor::new(&lines);
        let lang = path.as_ref().and_then(|p| detect(&p.to_string_lossy(), lines.first().map(String::as_str).unwrap_or(""))).and_then(lang);
        let saved_rev = ed.buf.rev;
        Session {
            ed,
            path,
            saved_rev,
            top: 0.0,
            left: 0,
            cols: 80,
            rows: 24.0,
            lang,
            doc: None,
            dash: false,
            picker: None,
            tree: None,
            tree_focus: false,
            term: None,
            term_focus: false,
            term_zen: false,
            lsp: None,
            lsp_pool: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            comp: None,
            comp_req: None,
            comp_block: u64::MAX,
            lsp_rev: u64::MAX,
            lsp_diags: std::sync::Arc::new(Vec::new()),
            hover_wait: None,
            hover_text: None,
            def_wait: None,
            git_branch: None,
            git_base: None,
            git_rev: u64::MAX,
            git_signs: std::sync::Arc::new(Vec::new()),
            term_esc: false,
            term_dims: (12, 100),
            term_screen: None,
            notify: None,
            leader: Vec::new(),
            leader_key: ' ',
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            bump: None,
            hl_pat: String::new(),
            hl_id: 0,
            hl_off: false,
            toast: None,
            toast_id: 0,
            seq: 0,
            quit: false,
            doc_id: PROXIMO_DOC.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            modo_abas: false,
            abrir_pedido: None,
            aba_pedido: None,
            jan_pedido: None,
            lsp_pend: Vec::new(),
            edicoes_pedido: None,
            sig: None,
            formatar_ao_salvar: false,
            jan_espera: false,
            pane_id: 0,
            splits: None,
            irmao: false,
        }
    }

    /// Define a tecla líder (as menções a ela mostram o nome da tecla escolhida).
    pub fn set_leader(&mut self, c: char) {
        self.leader_key = c;
    }

    /// Sem arquivo: abre na tela inicial (some ao editar, ao `n` ou ao abrir um arquivo).
    pub fn welcome() -> Self {
        let mut s = Session::new("", None);
        s.git_load();
        s.dash = true;
        s
    }

    pub fn open(path: &Path) -> Self {
        let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let mut s = Session::new(&text, Some(path.to_path_buf()));
        s.git_load();
        if !path.exists() {
            s.say(format!("\"{}\" [novo]", path.display()), ToastKind::Info);
        }
        s
    }

    /// Tamanho da área de texto em colunas e linhas (a cena informa).
    pub fn resize(&mut self, cols: usize, rows: f32) {
        self.cols = cols.max(10);
        self.rows = rows.max(3.0);
    }

    pub fn modified(&self) -> bool {
        self.ed.buf.rev != self.saved_rev
    }

    /// Este painel ou algum outro da aba tem alterações não salvas.
    fn tem_alteracoes(&self) -> bool {
        self.modified() || self.splits.as_ref().is_some_and(|sp| sp.outros.iter().any(Session::modified))
    }

    fn say(&mut self, text: String, kind: ToastKind) {
        self.toast_id += 1;
        self.toast = Some((self.toast_id, text, kind));
    }

    // ------------------------------------------------------------------
    // entrada
    // ------------------------------------------------------------------

    /// Teclas em notação `dw<Esc>`. Setas, Home/End e PageUp/PageDown viram navegação
    /// (nunca texto); teclas sem função (F1, Insert...) são ignoradas.
    pub fn feed(&mut self, keys: &str) {
        if self.term_focus && self.term.is_some() {
            self.term_feed(keys);
            return;
        }
        let chars: Vec<char> = keys.chars().collect();
        let mut buf = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '<' {
                if let Some(j) = chars[i..].iter().take(14).position(|&c| c == '>') {
                    let name: String = chars[i + 1..i + j].iter().collect::<String>().to_lowercase();
                    if let Some(nav) = nav_name(&name) {
                        for k in parse_keys(&std::mem::take(&mut buf)) {
                            self.key(k);
                        }
                        if let Some(n) = nav {
                            self.nav(n);
                        }
                        i += j + 1;
                        continue;
                    }
                }
            }
            buf.push(chars[i]);
            i += 1;
        }
        for k in parse_keys(&buf) {
            self.key(k);
        }
    }

    /// Setas e afins. No buscador movem a seleção; no editor, o cursor.
    fn nav(&mut self, n: Nav) {
        if self.picker.is_some() {
            match n {
                Nav::Up => self.pick_move(-1),
                Nav::Down => self.pick_move(1),
                Nav::PageUp => self.pick_move(-(PICK_LINHAS as i64)),
                Nav::PageDown => self.pick_move(PICK_LINHAS as i64),
                _ => {}
            }
            return;
        }
        if self.tree_focus && self.tree.is_some() {
            match n {
                Nav::Up => self.tree_key(Key::Char('k')),
                Nav::Down => self.tree_key(Key::Char('j')),
                Nav::Left => self.tree_key(Key::Char('h')),
                Nav::Right => self.tree_key(Key::Char('l')),
                _ => {}
            }
            return;
        }
        if self.ed.cmdline().is_some() || self.ed.has_pending() {
            return;
        }
        let (row, col) = (self.ed.cursor.row, self.ed.cursor.col);
        let last = self.ed.buf.line_count() - 1;
        let page = (self.rows - 2.0).max(1.0) as usize;
        if self.ed.mode() == Mode::Insert && self.comp.as_ref().is_some_and(|c| !c.shown.is_empty()) && matches!(n, Nav::Up | Nav::Down) {
            return self.comp_move(if n == Nav::Down { 1 } else { -1 });
        }
        if self.ed.mode() == Mode::Insert {
            let len = self.ed.buf.line_len(row);
            let (r, c) = match n {
                Nav::Left => (row, col.saturating_sub(1)),
                Nav::Right => (row, (col + 1).min(len)),
                Nav::Up => (row.saturating_sub(1), col),
                Nav::Down => ((row + 1).min(last), col),
                Nav::Home => (row, 0),
                Nav::End => (row, len),
                Nav::PageUp => (row.saturating_sub(page), col),
                Nav::PageDown => ((row + page).min(last), col),
            };
            let c = c.min(self.ed.buf.line_len(r));
            self.ed.set_cursor(r, c);
        } else {
            match n {
                Nav::Left => self.ed.feed(&[Key::Char('h')]),
                Nav::Right => self.ed.feed(&[Key::Char('l')]),
                Nav::Up => self.ed.feed(&[Key::Char('k')]),
                Nav::Down => self.ed.feed(&[Key::Char('j')]),
                Nav::Home => self.ed.feed(&[Key::Char('0')]),
                Nav::End => self.ed.feed(&[Key::Char('$')]),
                Nav::PageUp => self.ed.set_cursor(row.saturating_sub(page), col),
                Nav::PageDown => self.ed.set_cursor((row + page).min(last), col),
            }
        }
    }

    fn key(&mut self, k: Key) {
        if self.picker.is_some() {
            self.pick_key(k);
            return;
        }
        let livre = self.ed.cmdline().is_none() && !self.ed.has_pending() && self.ed.mode() == Mode::Normal;
        // <Espaço> é o líder: `ff` arquivos, `fw` texto, `fo` recentes
        if livre && self.leader.is_empty() && k == Key::Char(self.leader_key) {
            self.leader.push(k);
            return;
        }
        if !self.leader.is_empty() {
            if k == Key::Esc {
                self.leader.clear();
                return;
            }
            self.leader.push(k);
            let seq: Vec<Key> = self.leader[1..].to_vec();
            match seq.as_slice() {
                [Key::Char('f')] => return,
                [Key::Char('e')] => self.toggle_tree(),
                [Key::Char('t')] => self.toggle_term(),
                [Key::Char('T')] => self.term_close(),
                [Key::Char('A')] => return,
                [Key::Char('r')] | [Key::Char('c')] | [Key::Char('l')] | [Key::Char('x')] => return,
                [Key::Char('r'), Key::Char('n')] => {
                    // abre a linha de comando já com o nome sob o cursor, para editar e dar Enter
                    let palavra = self.palavra_sob_cursor();
                    self.leader.clear();
                    self.ed.feed_str(&format!(":Rename {palavra}"));
                    return;
                }
                [Key::Char('c'), Key::Char('a')] => self.lsp_acoes(),
                [Key::Char('l'), Key::Char('f')] => {
                    self.lsp_formatar(false, false);
                }
                [Key::Char('l'), Key::Char('s')] => self.lsp_consulta(PedidoLsp::Simbolos),
                [Key::Char('l'), Key::Char('i')] => self.lsp_consulta(PedidoLsp::Implementacao),
                [Key::Char('l'), Key::Char('y')] => self.lsp_consulta(PedidoLsp::TipoDaDefinicao),
                [Key::Char('l'), Key::Char('r')] => self.lsp_consulta(PedidoLsp::Referencias),
                [Key::Char('x'), Key::Char('x')] => self.diag_lista(),
                [Key::Char('A'), Key::Char('l')] => self.open_agent(),
                [Key::Char('A'), Key::Char('z')] => self.toggle_zen(),
                [Key::Char('f'), Key::Char('f')] => self.pick_open(PickKind::Arquivos),
                [Key::Char('f'), Key::Char('w')] => self.pick_open(PickKind::Texto),
                [Key::Char('f'), Key::Char('o')] => self.pick_open(PickKind::Recentes),
                _ => {
                    // não é atalho: devolve as teclas ao editor
                    let all = std::mem::take(&mut self.leader);
                    for k in all {
                        self.dispatch(k);
                    }
                    return;
                }
            }
            self.leader.clear();
            return;
        }
        self.dispatch(k);
    }

    /// Tecla já sem líder: vai para o explorador (se ele tem o foco), a tela inicial ou o editor.
    fn dispatch(&mut self, k: Key) {
        if self.tree_focus && self.tree.is_some() {
            self.tree_key(k);
            return;
        }
        let livre = self.ed.cmdline().is_none() && !self.ed.has_pending() && self.ed.mode() == Mode::Normal;
        if self.jan_espera {
            self.jan_espera = false;
            self.janela_tecla(k);
            return;
        }
        if livre && k == Key::Ctrl('w') {
            self.jan_espera = true;
            return;
        }
        if self.dash && livre {
            match k {
                Key::Char('n') => {
                    self.dash = false;
                    self.ed.feed(&[Key::Char('i')]);
                    return;
                }
                Key::Char('f') => return self.pick_open(PickKind::Arquivos),
                Key::Char('o') => return self.pick_open(PickKind::Recentes),
                Key::Char('w') => return self.pick_open(PickKind::Texto),
                Key::Char('a') => return self.open_agent(),
                Key::Char('s') => {
                    self.say("Ainda não disponível no motor nativo.".into(), ToastKind::Info);
                    return;
                }
                _ => {}
            }
        }
        self.key_inner(k);
        if self.dash && (self.ed.mode() != Mode::Normal || self.modified()) {
            self.dash = false;
        }
    }

    // ------------------------------------------------------------------
    // LSP
    // ------------------------------------------------------------------

    /// Liga o servidor de linguagem do arquivo (um por programa, compartilhado entre as abas).
    pub fn lsp_attach(&mut self) {
        let off = std::env::var("ASTROGRAPH_LSP").is_ok_and(|v| v == "0");
        let Some(path) = self.path.clone() else {
            return;
        };
        self.lsp = None;
        self.lsp_rev = self.ed.buf.rev;
        self.lsp_diags = std::sync::Arc::new(Vec::new());
        let Some((prog, args, lang)) = (if off { None } else { lsp::servidor_para(&path) }) else {
            return;
        };
        let cli = {
            let mut pool = self.lsp_pool.lock().unwrap();
            match pool.get(&prog) {
                Some(c) => Some(c.clone()),
                None => {
                    let raiz = git::raiz(path.parent().unwrap_or(&self.root)).unwrap_or_else(|| self.root.clone());
                    let notify = self.notify.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
                    match Cliente::iniciar(&prog, &args, &raiz, notify) {
                        Ok(c) => {
                            let r: LspRef = std::sync::Arc::new(std::sync::Mutex::new(c));
                            pool.insert(prog.clone(), r.clone());
                            Some(r)
                        }
                        Err(_) => None,
                    }
                }
            }
        };
        if let Some(c) = &cli {
            c.lock().unwrap().abrir(&path, lang, &self.ed.buf.rope.to_string());
        }
        self.lsp = cli;
    }

    /// Manda o texto novo ao servidor e pega o que ele já respondeu.
    fn lsp_sync(&mut self) {
        let (Some(cl), Some(path)) = (self.lsp.clone(), self.path.clone()) else { return };
        let (diags, hover, def, comp) = {
            let mut c = cl.lock().unwrap();
            if self.lsp_rev != self.ed.buf.rev {
                self.lsp_rev = self.ed.buf.rev;
                c.mudou(&path, &self.ed.buf.rope.to_string());
            }
            let st = c.estado.lock().unwrap();
            let diags = st.diagnosticos.get(&lsp::uri(&path)).cloned().unwrap_or_default();
            (diags, st.hover.clone(), st.definicao.clone(), st.completar.clone())
        };
        // diagnósticos: colunas UTF-16 -> caracteres
        if diags.as_slice() != self.lsp_diags.as_slice() || self.lsp_diags.is_empty() != diags.is_empty() {
            let n = self.ed.buf.line_count();
            let conv = |row: usize, col: usize| -> usize {
                if row < n { lsp::col_chars(&self.ed.buf.line(row), col) } else { col }
            };
            self.lsp_diags = std::sync::Arc::new(
                diags.into_iter().map(|d| lsp::Diag { col: conv(d.row, d.col), end_col: conv(d.end_row, d.end_col), ..d }).collect(),
            );
        }
        // hover, definição e completar pedidos
        if let (Some((rid, start)), Some((cid, itens))) = (self.comp_req, comp) {
            if rid == cid {
                self.comp_req = None;
                if self.ed.mode() == Mode::Insert {
                    self.comp = Some(Comp { start, items: itens, shown: Vec::new(), sel: 0 });
                    self.comp_filter();
                }
            }
        }
        if let (Some(id), Some((hid, texto))) = (self.hover_wait, hover) {
            if id == hid {
                self.hover_wait = None;
                if texto.trim().is_empty() {
                    self.say("Sem informação aqui.".into(), ToastKind::Info);
                } else {
                    self.hover_text = Some(texto);
                }
            }
        }
        if let (Some(id), Some((did, locs))) = (self.def_wait, def) {
            if id == did {
                self.def_wait = None;
                match locs.first() {
                    None => self.say("Definição não encontrada.".into(), ToastKind::Info),
                    Some(l) => {
                        let (arq, row, col16) = (l.arquivo.clone(), l.row, l.col);
                        self.ir_para_local(&arq, row, col16);
                    }
                }
            }
        }
        self.lsp_respostas(&cl);
    }

    /// Vai para um local do servidor (arquivo, linha, coluna UTF-16): no arquivo aberto ou abrindo outro.
    fn ir_para_local(&mut self, arq: &Path, row: usize, col16: usize) {
        let mesmo = Some(arq) == self.path.as_deref() || arq.canonicalize().ok() == self.path.as_ref().and_then(|p| p.canonicalize().ok());
        if !mesmo {
            self.open_file(arq, Some((row, 0)));
        }
        if row < self.ed.buf.line_count() {
            let col = lsp::col_chars(&self.ed.buf.line(row), col16);
            self.ed.set_cursor(row, col);
        }
    }

    /// Depois de cada tecla no modo de inserção: abre, filtra ou fecha o completar.
    fn comp_update(&mut self) {
        if self.ed.mode() != Mode::Insert || self.lsp.is_none() || self.path.is_none() {
            self.comp = None;
            self.comp_req = None;
            return;
        }
        if self.comp_block == self.ed.buf.rev {
            return;
        }
        let (row, col) = (self.ed.cursor.row, self.ed.cursor.col);
        let linha: Vec<char> = self.ed.buf.line(row).chars().collect();
        let col = col.min(linha.len());
        let palavra = |c: char| c.is_alphanumeric() || c == '_';
        let mut ini = col;
        while ini > 0 && palavra(linha[ini - 1]) {
            ini -= 1;
        }
        let gatilho = match (col.checked_sub(1).map(|i| linha[i]), col.checked_sub(2).map(|i| linha[i])) {
            (Some('.'), _) => true,
            (Some(':'), Some(':')) => true,
            (Some('>'), Some('-')) => true,
            _ => false,
        };
        if col == ini && !gatilho {
            self.comp = None;
            self.comp_req = None;
            return;
        }
        let start = Pos::new(row, ini);
        let mesmo = self.comp.as_ref().is_some_and(|c| c.start == start) || self.comp_req.is_some_and(|(_, p)| p == start);
        if mesmo {
            self.comp_filter();
        } else if let (Some(c), Some(path)) = (self.lsp.as_ref(), self.path.as_ref()) {
            let id = c.lock().unwrap().completar(path, row, lsp::col_utf16(&self.ed.buf.line(row), col));
            self.comp_req = Some((id, start));
            self.comp = None;
        }
    }

    /// Assinatura da função: abre ao digitar `(` ou `,` e fecha ao digitar `)`, sair da inserção ou mudar de linha.
    fn sig_update(&mut self, k: Key) {
        if self.ed.mode() != Mode::Insert {
            self.sig = None;
            return;
        }
        match k {
            Key::Char('(') | Key::Char(',') if self.lsp.is_some() => self.lsp_assinatura(),
            Key::Char(')') | Key::Enter => self.sig = None,
            _ => {}
        }
    }

    /// Palavra (letras, dígitos, `_`) sob o cursor, para propor no renomear.
    fn palavra_sob_cursor(&self) -> String {
        let linha: Vec<char> = self.ed.buf.line(self.ed.cursor.row).chars().collect();
        let c = self.ed.cursor.col.min(linha.len().saturating_sub(1));
        let pal = |c: char| c.is_alphanumeric() || c == '_';
        if linha.is_empty() || !pal(linha[c]) {
            return String::new();
        }
        let (mut a, mut b) = (c, c);
        while a > 0 && pal(linha[a - 1]) {
            a -= 1;
        }
        while b + 1 < linha.len() && pal(linha[b + 1]) {
            b += 1;
        }
        linha[a..=b].iter().collect()
    }

    /// Filtra as sugestões pelo que já foi digitado desde o começo da palavra.
    fn comp_filter(&mut self) {
        let Some(c) = self.comp.as_mut() else { return };
        let (row, col) = (self.ed.cursor.row, self.ed.cursor.col);
        if row != c.start.row || col < c.start.col {
            self.comp = None;
            return;
        }
        let prefixo: Vec<char> = self.ed.buf.line(row).chars().skip(c.start.col).take(col - c.start.col).collect();
        let mut v: Vec<(i32, usize, Vec<usize>)> = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                if prefixo.is_empty() {
                    return Some((0, i, Vec::new()));
                }
                // quem começa com o que foi digitado vem antes do casamento espalhado
                let pre = it.filtro.to_lowercase().starts_with(&prefixo.iter().collect::<String>().to_lowercase());
                crate::busca::pontuar(&prefixo, &it.filtro).map(|(n, pos)| (n + if pre { 1000 } else { 0 }, i, pos))
            })
            .collect();
        if !prefixo.is_empty() {
            v.sort_by(|a, b| b.0.cmp(&a.0).then(c.items[a.1].label.len().cmp(&c.items[b.1].label.len())).then(a.1.cmp(&b.1)));
        }
        c.shown = v.into_iter().take(60).map(|(_, i, pos)| (i, pos)).collect();
        c.sel = c.sel.min(c.shown.len().saturating_sub(1));
    }

    fn comp_move(&mut self, d: i64) {
        if let Some(c) = self.comp.as_mut() {
            let n = c.shown.len() as i64;
            if n > 0 {
                c.sel = (c.sel as i64 + d).rem_euclid(n) as usize;
            }
        }
    }

    fn comp_accept(&mut self) {
        let Some(c) = self.comp.take() else { return };
        let Some((i, _)) = c.shown.get(c.sel) else { return };
        let item = &c.items[*i];
        let mut de = c.start;
        if let Some((r, c16)) = item.inicio {
            if r == self.ed.cursor.row {
                de = Pos::new(r, lsp::col_chars(&self.ed.buf.line(r), c16));
            }
        }
        self.ed.insert_replace(de, &item.texto);
        self.comp_req = None;
        self.comp_block = self.ed.buf.rev;
    }

    /// Servidor, arquivo e posição do cursor (coluna UTF-16); avisa quando não há servidor.
    fn lsp_ctx(&mut self) -> Option<(LspRef, PathBuf, usize, usize)> {
        let (Some(c), Some(path)) = (self.lsp.clone(), self.path.clone()) else {
            self.say("Sem servidor de linguagem para este arquivo.".into(), ToastKind::Info);
            return None;
        };
        let (r, col) = (self.ed.cursor.row, self.ed.cursor.col);
        let c16 = lsp::col_utf16(&self.ed.buf.line(r), col);
        Some((c, path, r, c16))
    }

    fn lsp_esperar(&mut self, id: i64, tipo: PedidoLsp) {
        self.lsp_pend.push(Pendente { id, tipo, desde: std::time::Instant::now() });
    }

    /// `<Líder>rn` / `:Rename nome`.
    fn lsp_renomear(&mut self, novo: &str) {
        if novo.trim().is_empty() {
            self.say("Uso: :Rename novo_nome".into(), ToastKind::Info);
            return;
        }
        let Some((c, path, r, c16)) = self.lsp_ctx() else { return };
        let id = c.lock().unwrap().renomear(&path, r, c16, novo.trim());
        self.lsp_esperar(id, PedidoLsp::Renomear);
    }

    /// `<Líder>ca`: ações de código para o cursor (com os diagnósticos da linha).
    fn lsp_acoes(&mut self) {
        let Some((c, path, r, c16)) = self.lsp_ctx() else { return };
        let linha = self.ed.buf.line(r);
        let diags: Vec<serde_json::Value> = self
            .lsp_diags
            .iter()
            .filter(|d| d.row <= r && r <= d.end_row)
            .map(|d| {
                serde_json::json!({
                    "range": {"start": {"line": d.row, "character": lsp::col_utf16(&self.ed.buf.line(d.row.min(self.ed.buf.line_count() - 1)), d.col)}, "end": {"line": d.end_row, "character": lsp::col_utf16(&self.ed.buf.line(d.end_row.min(self.ed.buf.line_count() - 1)), d.end_col)}},
                    "severity": d.severity,
                    "message": d.message,
                })
            })
            .collect();
        let _ = linha;
        let id = c.lock().unwrap().acoes(&path, (r, c16), (r, c16), diags);
        self.lsp_esperar(id, PedidoLsp::Acoes);
    }

    /// `<Líder>lf` / `:Format`; `salvar` grava depois (formatar ao salvar), `sair` fecha em seguida (`:wq`).
    fn lsp_formatar(&mut self, salvar: bool, sair: bool) -> bool {
        let (Some(c), Some(path)) = (self.lsp.clone(), self.path.clone()) else {
            if !salvar {
                self.say("Sem servidor de linguagem para este arquivo.".into(), ToastKind::Info);
            }
            return false;
        };
        // o servidor precisa estar pronto e ter a versão atual do texto
        if salvar && !c.lock().unwrap().estado.lock().unwrap().pronto {
            return false;
        }
        let tab = self.ed.shiftwidth.max(1);
        let id = {
            let mut cl = c.lock().unwrap();
            if self.lsp_rev != self.ed.buf.rev {
                self.lsp_rev = self.ed.buf.rev;
                cl.mudou(&path, &self.ed.buf.rope.to_string());
            }
            cl.formatar(&path, tab, true)
        };
        self.lsp_esperar(id, PedidoLsp::Formatar { salvar, sair });
        true
    }

    fn lsp_assinatura(&mut self) {
        let (Some(c), Some(path)) = (self.lsp.clone(), self.path.clone()) else { return };
        let (r, col) = (self.ed.cursor.row, self.ed.cursor.col);
        let c16 = lsp::col_utf16(&self.ed.buf.line(r), col);
        let id = {
            let mut cl = c.lock().unwrap();
            if self.lsp_rev != self.ed.buf.rev {
                self.lsp_rev = self.ed.buf.rev;
                cl.mudou(&path, &self.ed.buf.rope.to_string());
            }
            cl.assinatura(&path, r, c16)
        };
        self.lsp_esperar(id, PedidoLsp::Assinatura);
    }

    /// `gr` referências, `<Líder>li` implementação, `gy`/`<Líder>ly` tipo, `<Líder>ls` símbolos.
    fn lsp_consulta(&mut self, tipo: PedidoLsp) {
        let Some((c, path, r, c16)) = self.lsp_ctx() else { return };
        let id = {
            let cl = c.lock().unwrap();
            match tipo {
                PedidoLsp::Referencias => cl.referencias(&path, r, c16),
                PedidoLsp::Implementacao => cl.implementacao(&path, r, c16),
                PedidoLsp::TipoDaDefinicao => cl.tipo_da_definicao(&path, r, c16),
                _ => cl.simbolos(&path),
            }
        };
        self.lsp_esperar(id, tipo);
    }

    /// Abre o buscador com uma lista pronta.
    fn pick_lista(&mut self, titulo: &str, itens: Vec<ListaItem>) {
        let base: Vec<String> = itens.iter().map(|i| i.texto.clone()).collect();
        self.picker = Some(Picker { kind: PickKind::Lista, titulo: titulo.into(), lista: itens, mapa: Vec::new(), query: String::new(), total: base.len(), base, items: Vec::new(), achados: Vec::new(), sel: 0 });
        self.pick_refresh();
    }

    /// Texto da linha `row` de `arq` (do buffer, se for o aberto; senão do disco) para mostrar numa lista.
    fn linha_de(&self, arq: &Path, row: usize) -> String {
        let mesmo = Some(arq) == self.path.as_deref() || arq.canonicalize().ok() == self.path.as_ref().and_then(|p| p.canonicalize().ok());
        let t = if mesmo && row < self.ed.buf.line_count() {
            self.ed.buf.line(row)
        } else {
            std::fs::read_to_string(arq).ok().and_then(|t| t.lines().nth(row).map(str::to_string)).unwrap_or_default()
        };
        t.trim().chars().take(100).collect()
    }

    /// Respostas dos pedidos que esperavam o servidor, mensagens e edições que ele mandou.
    fn lsp_respostas(&mut self, cl: &LspRef) {
        // mensagens (`window/showMessage`) e `workspace/applyEdit`
        let (msgs, ext) = {
            let c = cl.lock().unwrap();
            let mut e = c.estado.lock().unwrap();
            (std::mem::take(&mut e.mensagens), std::mem::take(&mut e.edicoes_externas))
        };
        for (t, m) in msgs {
            let k = match t {
                1 => ToastKind::Error,
                2 | 3 => ToastKind::Info,
                _ => continue,
            };
            self.say(m, k);
        }
        let ed: Vec<lsp::Edicao> = ext.iter().flat_map(lsp::edicoes_de).collect();
        if !ed.is_empty() {
            self.edicoes_pedido = Some(ed);
        }
        if self.lsp_pend.is_empty() {
            return;
        }
        let pend = std::mem::take(&mut self.lsp_pend);
        for p in pend {
            let r = cl.lock().unwrap().resposta(p.id);
            let espera = p.desde.elapsed().as_millis();
            match r {
                Some(v) => self.lsp_resposta(p.tipo, v),
                // formatar ao salvar: o servidor não respondeu a tempo, grava assim mesmo
                None if matches!(p.tipo, PedidoLsp::Formatar { .. }) && espera > FORMATAR_ESPERA_MS => {
                    if let PedidoLsp::Formatar { salvar: true, sair } = p.tipo {
                        self.gravar("");
                        self.git_load();
                        if sair && !self.modified() {
                            self.quit = true;
                        }
                    }
                }
                None if espera > 15_000 => {}
                None => self.lsp_pend.push(p),
            }
        }
    }

    fn lsp_resposta(&mut self, tipo: PedidoLsp, v: serde_json::Value) {
        match tipo {
            PedidoLsp::Renomear => {
                let ed = lsp::edicoes_de(&v);
                if ed.is_empty() {
                    self.say("Não foi possível renomear aqui.".into(), ToastKind::Error);
                } else {
                    self.edicoes_pedido = Some(ed);
                }
            }
            PedidoLsp::Acoes => {
                let acoes = lsp::acoes_de(&v);
                if acoes.is_empty() {
                    self.say("Nenhuma ação de código aqui.".into(), ToastKind::Info);
                    return;
                }
                let itens = acoes.into_iter().map(|a| ListaItem { texto: a.titulo.clone(), detalhe: a.tipo.clone(), destino: Destino::Acao(a) }).collect();
                self.pick_lista("Ações de código", itens);
            }
            PedidoLsp::Formatar { salvar, sair } => {
                if let Some(path) = self.path.clone() {
                    let ed = lsp::edicoes_do_arquivo(&path, &v);
                    self.aplicar_edicoes_aqui(&ed);
                }
                if salvar {
                    self.gravar("");
                    self.git_load();
                    if sair && !self.modified() {
                        self.quit = true;
                    }
                }
            }
            PedidoLsp::Assinatura => self.sig = lsp::assinatura_de(&v),
            PedidoLsp::Referencias | PedidoLsp::Implementacao | PedidoLsp::TipoDaDefinicao => {
                let locais = lsp::locais_de(&v);
                let nome = match tipo {
                    PedidoLsp::Referencias => "Referências",
                    PedidoLsp::Implementacao => "Implementações",
                    _ => "Tipo",
                };
                match locais.len() {
                    0 => self.say(format!("{nome}: nada encontrado."), ToastKind::Info),
                    1 if tipo != PedidoLsp::Referencias => {
                        let l = &locais[0];
                        self.ir_para_local(&l.arquivo, l.row, l.col);
                    }
                    _ => {
                        let raiz = self.root.clone();
                        let itens = locais
                            .into_iter()
                            .map(|l| ListaItem {
                                texto: format!("{}:{}", relativo(&raiz, &l.arquivo.to_string_lossy()), l.row + 1),
                                detalhe: self.linha_de(&l.arquivo, l.row),
                                destino: Destino::Local(l.arquivo, l.row, l.col),
                            })
                            .collect();
                        self.pick_lista(nome, itens);
                    }
                }
            }
            PedidoLsp::Simbolos => {
                let simbolos = lsp::simbolos_de(&v);
                let Some(path) = self.path.clone() else { return };
                if simbolos.is_empty() {
                    self.say("Sem símbolos neste arquivo.".into(), ToastKind::Info);
                    return;
                }
                let itens = simbolos
                    .into_iter()
                    .map(|s| ListaItem {
                        texto: format!("{}{} {}", "  ".repeat(s.nivel), tipo_simbolo(s.tipo), s.nome),
                        detalhe: s.detalhe,
                        destino: Destino::Local(path.clone(), s.row, s.col),
                    })
                    .collect();
                self.pick_lista("Símbolos", itens);
            }
        }
    }

    /// Escolheu uma ação de código: aplica a edição e/ou roda o comando no servidor.
    fn aplicar_acao(&mut self, a: lsp::Acao) {
        if let Some(e) = &a.edicao {
            let ed = lsp::edicoes_de(e);
            if !ed.is_empty() {
                self.edicoes_pedido = Some(ed);
            }
        }
        if let (Some((cmd, args)), Some(c)) = (a.comando, self.lsp.clone()) {
            c.lock().unwrap().executar(&cmd, args);
        }
    }

    /// Aplica trocas de texto do servidor ao buffer aberto, como um único passo de desfazer.
    pub fn aplicar_edicoes_aqui(&mut self, ed: &[lsp::Edicao]) {
        let n = self.ed.buf.line_count();
        let conv = |lin: usize, c16: usize, buf: &astrograph_core::buffer::Buffer| -> Pos {
            let lin = lin.min(n - 1);
            Pos::new(lin, lsp::col_chars(&buf.line(lin), c16))
        };
        let v: Vec<(Pos, Pos, String)> = ed.iter().map(|e| (conv(e.ini.0, e.ini.1, &self.ed.buf), conv(e.fim.0, e.fim.1, &self.ed.buf), e.novo.clone())).collect();
        self.ed.aplicar_edicoes(&v);
    }

    /// `]d` / `[d`: próximo ou anterior diagnóstico; `lista`: abre a lista de todos (`<Líder>xx`).
    fn diag_pular(&mut self, adiante: bool) {
        let (r, c) = (self.ed.cursor.row, self.ed.cursor.col);
        let mut pos: Vec<(usize, usize)> = self.lsp_diags.iter().map(|d| (d.row, d.col)).collect();
        pos.sort();
        pos.dedup();
        let alvo = if adiante { pos.iter().find(|p| **p > (r, c)).or(pos.first()) } else { pos.iter().rev().find(|p| **p < (r, c)).or(pos.last()) };
        match alvo {
            Some(&(row, col)) => {
                let m = self.lsp_diags.iter().find(|d| (d.row, d.col) == (row, col)).map(|d| d.message.clone()).unwrap_or_default();
                self.ed.set_cursor(row, col);
                self.say(m.lines().next().unwrap_or("").to_string(), ToastKind::Info);
            }
            None => self.say("Sem diagnósticos.".into(), ToastKind::Info),
        }
    }

    fn diag_lista(&mut self) {
        let Some(path) = self.path.clone() else { return };
        if self.lsp_diags.is_empty() {
            self.say("Sem diagnósticos.".into(), ToastKind::Info);
            return;
        }
        let itens = self
            .lsp_diags
            .iter()
            .map(|d| {
                let sev = match d.severity {
                    1 => "erro",
                    2 => "aviso",
                    3 => "info",
                    _ => "dica",
                };
                let c16 = lsp::col_utf16(&self.ed.buf.line(d.row.min(self.ed.buf.line_count() - 1)), d.col);
                ListaItem { texto: format!("{}:{} {}", d.row + 1, d.col + 1, d.message.lines().next().unwrap_or("")), detalhe: sev.into(), destino: Destino::Local(path.clone(), d.row, c16) }
            })
            .collect();
        self.pick_lista("Diagnósticos", itens);
    }

    fn lsp_hover(&mut self) {
        let (Some(c), Some(path)) = (self.lsp.as_ref(), self.path.as_ref()) else {
            self.say("Sem servidor de linguagem para este arquivo.".into(), ToastKind::Info);
            return;
        };
        let (r, col) = (self.ed.cursor.row, self.ed.cursor.col);
        self.hover_wait = Some(c.lock().unwrap().hover(path, r, lsp::col_utf16(&self.ed.buf.line(r), col) as usize));
    }

    fn lsp_definition(&mut self) {
        let (Some(c), Some(path)) = (self.lsp.as_ref(), self.path.as_ref()) else {
            self.say("Sem servidor de linguagem para este arquivo.".into(), ToastKind::Info);
            return;
        };
        let (r, col) = (self.ed.cursor.row, self.ed.cursor.col);
        self.def_wait = Some(c.lock().unwrap().definicao(path, r, lsp::col_utf16(&self.ed.buf.line(r), col)));
    }

    // ------------------------------------------------------------------
    // git
    // ------------------------------------------------------------------

    /// Lê o branch e o arquivo em `HEAD` (de novo depois de salvar, de um commit no terminal...).
    pub fn git_load(&mut self) {
        let dir = self.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| self.root.clone());
        self.git_branch = git::branch(&dir);
        self.git_base = match (&self.git_branch, &self.path) {
            (Some(_), Some(p)) => git::base(p),
            _ => None,
        };
        self.git_rev = u64::MAX;
    }

    /// Sinais da margem para o texto atual (recalculados só quando o texto muda).
    fn git_signs(&mut self) -> std::sync::Arc<Vec<(usize, git::Sign)>> {
        if self.git_rev != self.ed.buf.rev {
            self.git_rev = self.ed.buf.rev;
            self.git_signs = std::sync::Arc::new(match &self.git_base {
                Some(base) => git::sinais(base, &self.ed.lines()),
                None => Vec::new(),
            });
        }
        self.git_signs.clone()
    }

    // ------------------------------------------------------------------
    // terminal
    // ------------------------------------------------------------------

    /// Quem acorda a UI quando o terminal tem saída nova.
    pub fn set_notify(&mut self, f: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.notify = Some(f);
    }

    fn term_spawn(&mut self, cmd: portable_pty::CommandBuilder) {
        let (rows, cols) = self.term_dims;
        let notify = self.notify.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
        match Term::spawn(cmd, rows, cols, Some(&self.root), notify) {
            Ok(t) => {
                self.term = Some(t);
                self.term_focus = true;
                self.term_screen = None;
            }
            Err(e) => self.say(format!("Não consegui abrir o terminal: {e}"), ToastKind::Error),
        }
    }

    /// `<Líder>t`: abre o terminal (shell) com o teclado nele; sem o foco, devolve o foco. `<Líder>T` fecha.
    fn toggle_term(&mut self) {
        match (&self.term, self.term_focus) {
            (None, _) => self.term_spawn(term::shell_padrao()),
            (Some(_), _) => self.term_focus = true,
        }
    }

    /// `<Líder>Al` / `a` na tela inicial: roda o primeiro agente instalado no terminal.
    fn open_agent(&mut self) {
        let Some(nome) = term::agente_instalado() else {
            self.say("Nenhum agente instalado (claude, opencode, codex ou gemini). Use <Líder>t para o terminal.".into(), ToastKind::Info);
            return;
        };
        self.term_close();
        self.term_spawn(portable_pty::CommandBuilder::new(nome));
    }

    /// `<Líder>Az`: modo zen. Abre o agente (se não houver terminal) em tela cheia, com o teclado nele.
    fn toggle_zen(&mut self) {
        if self.term.is_none() {
            self.open_agent();
            if self.term.is_none() {
                return;
            }
        }
        self.term_zen = !self.term_zen;
        self.term_focus = true;
    }

    fn term_close(&mut self) {
        if let Some(mut t) = self.term.take() {
            t.matar();
        }
        self.term_focus = false;
        self.term_zen = false;
        self.term_esc = false;
        self.term_screen = None;
    }

    /// Tamanho do painel (a cena calcula a partir da janela).
    pub fn term_resize(&mut self, rows: u16, cols: u16) {
        self.term_dims = (rows, cols);
        if let Some(t) = self.term.as_mut() {
            t.resize(rows, cols);
        }
    }

    /// Teclas com o terminal em foco: vão para o pty; `<C-\><C-n>` devolve o teclado ao editor.
    fn term_feed(&mut self, keys: &str) {
        let esc = |k: &str| matches!(k.to_lowercase().as_str(), "<c-bslash>" | "<c-\\>");
        if self.term_esc {
            self.term_esc = false;
            if keys.eq_ignore_ascii_case("<C-n>") {
                self.term_focus = false;
                self.term_zen = false;
                // o que foi feito no terminal pode ter mudado o git (commit, checkout...)
                self.git_load();
                return;
            }
            if let Some(t) = self.term.as_mut() {
                t.write(&[0x1c]);
            }
        } else if esc(keys) {
            self.term_esc = true;
            return;
        }
        if let Some(t) = self.term.as_mut() {
            t.write(&term::teclas_para_bytes(keys));
        }
    }

    /// Pega o instantâneo novo, se houver; um processo que terminou fecha o painel.
    fn term_refresh(&mut self) {
        let Some(t) = self.term.as_ref() else { return };
        if self.term_screen.is_some() && !t.pending.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let sc = t.snapshot();
        if sc.exited {
            self.term_close();
            self.say("Terminal encerrado.".into(), ToastKind::Info);
        } else {
            self.term_screen = Some(std::sync::Arc::new(sc));
        }
    }

    // ------------------------------------------------------------------
    // explorador de arquivos
    // ------------------------------------------------------------------

    fn toggle_tree(&mut self) {
        match (&self.tree, self.tree_focus) {
            (None, _) => {
                let mut t = Arvore::nova(&self.root);
                if let Some(p) = &self.path {
                    let abs = if p.is_absolute() { p.clone() } else { self.root.join(p) };
                    t.revelar(&abs);
                }
                self.tree = Some(t);
                self.tree_focus = true;
            }
            (Some(_), true) => {
                self.tree = None;
                self.tree_focus = false;
            }
            (Some(_), false) => self.tree_focus = true,
        }
    }

    fn tree_key(&mut self, k: Key) {
        let Some(t) = self.tree.as_mut() else { return };
        match k {
            Key::Char('j') | Key::Ctrl('n') => t.mover(1),
            Key::Char('k') | Key::Ctrl('p') => t.mover(-1),
            Key::Char('G') => t.mover(i64::MAX / 2),
            Key::Char('g') => t.mover(i64::MIN / 2),
            Key::Char('l') => t.avancar(),
            Key::Char('h') => t.recuar(),
            Key::Char('R') => t.recarregar(),
            Key::Esc | Key::Char('q') | Key::Tab => self.tree_focus = false,
            Key::Enter | Key::Char('o') => {
                let Some(n) = t.atual().cloned() else { return };
                if n.pasta {
                    t.alternar();
                } else {
                    self.open_file(&n.caminho, None);
                    // abrir um arquivo devolve o foco ao texto
                    self.tree_focus = false;
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // buscador (arquivos, texto, recentes)
    // ------------------------------------------------------------------

    fn pick_open(&mut self, kind: PickKind) {
        let base = match kind {
            PickKind::Arquivos => busca::listar(&self.root),
            PickKind::Recentes => busca::recentes(),
            PickKind::Texto | PickKind::Lista => Vec::new(),
        };
        self.picker = Some(Picker { kind, titulo: String::new(), lista: Vec::new(), mapa: Vec::new(), query: String::new(), total: base.len(), base, items: Vec::new(), achados: Vec::new(), sel: 0 });
        self.pick_refresh();
    }

    fn pick_refresh(&mut self) {
        let root = self.root.clone();
        let Some(p) = self.picker.as_mut() else { return };
        p.sel = 0;
        match p.kind {
            PickKind::Arquivos | PickKind::Recentes => {
                let r = busca::fuzzy(&p.query, &p.base, 200);
                p.total = if p.query.is_empty() { p.base.len() } else { r.len() };
                p.items = r
                    .into_iter()
                    .map(|c| {
                        let t = &p.base[c.item];
                        let shown = if p.kind == PickKind::Recentes { relativo(&root, t) } else { t.clone() };
                        // as posições valem para o texto original; em recentes mostramos o relativo
                        let hits = if shown == *t { c.pos } else { Vec::new() };
                        PickItem { text: shown, hits, detail: String::new() }
                    })
                    .collect();
            }
            PickKind::Lista => {
                let r = busca::fuzzy(&p.query, &p.base, 200);
                p.total = if p.query.is_empty() { p.base.len() } else { r.len() };
                p.mapa = r.iter().map(|c| c.item).collect();
                p.items = r.into_iter().map(|c| PickItem { text: p.base[c.item].clone(), hits: c.pos, detail: p.lista[c.item].detalhe.clone() }).collect();
            }
            PickKind::Texto => {
                p.achados = busca::buscar_texto(&root, &p.query, 200, std::time::Duration::from_millis(150));
                p.total = p.achados.len();
                p.items = p.achados.iter().map(|a| PickItem { text: format!("{}:{}", a.arquivo, a.linha + 1), hits: Vec::new(), detail: a.texto.clone() }).collect();
            }
        }
    }

    fn pick_move(&mut self, d: i64) {
        if let Some(p) = self.picker.as_mut() {
            let n = p.items.len();
            if n > 0 {
                p.sel = (p.sel as i64 + d).clamp(0, n as i64 - 1) as usize;
            }
        }
    }

    fn pick_key(&mut self, k: Key) {
        match k {
            Key::Esc | Key::Ctrl('c') => self.picker = None,
            Key::Enter => self.pick_accept(),
            Key::Ctrl('n') | Key::Ctrl('j') => self.pick_move(1),
            Key::Ctrl('p') | Key::Ctrl('k') => self.pick_move(-1),
            Key::Backspace => {
                if let Some(p) = self.picker.as_mut() {
                    p.query.pop();
                }
                self.pick_refresh();
            }
            Key::Ctrl('u') => {
                if let Some(p) = self.picker.as_mut() {
                    p.query.clear();
                }
                self.pick_refresh();
            }
            Key::Char(c) => {
                if let Some(p) = self.picker.as_mut() {
                    p.query.push(c);
                }
                self.pick_refresh();
            }
            _ => {}
        }
    }

    fn pick_accept(&mut self) {
        let Some(p) = self.picker.take() else { return };
        let Some(item) = p.items.get(p.sel) else { return };
        let (path, goto) = match p.kind {
            PickKind::Lista => {
                let Some(l) = p.mapa.get(p.sel).and_then(|i| p.lista.get(*i)).cloned() else { return };
                match l.destino {
                    Destino::Local(arq, row, col16) => self.ir_para_local(&arq, row, col16),
                    Destino::Acao(a) => self.aplicar_acao(a),
                }
                return;
            }
            PickKind::Arquivos => (self.root.join(&item.text), None),
            PickKind::Recentes => (p.base.iter().find(|b| relativo(&self.root, b) == item.text).map(PathBuf::from).unwrap_or_default(), None),
            PickKind::Texto => match p.achados.get(p.sel) {
                Some(a) => (self.root.join(&a.arquivo), Some((a.linha, a.col))),
                None => return,
            },
        };
        self.open_file(&path, goto);
    }

    /// Abre `path`: em aba nova (ou na que já o tem) quando há espaço de trabalho; senão no lugar.
    fn open_file(&mut self, path: &Path, goto: Option<(usize, usize)>) {
        if self.modo_abas {
            let mesmo = self.path.as_ref().is_some_and(|p| p == path || p.canonicalize().ok().zip(path.canonicalize().ok()).is_some_and(|(a, b)| a == b));
            if mesmo {
                if let Some((r, c)) = goto {
                    self.ed.set_cursor(r, c);
                }
            } else {
                self.abrir_pedido = Some((path.to_path_buf(), goto));
            }
            return;
        }
        self.open_file_aqui(path, goto);
    }

    /// Abre `path` no lugar do buffer atual (pede para salvar antes, como o `:e`).
    fn open_file_aqui(&mut self, path: &Path, goto: Option<(usize, usize)>) {
        if self.modified() {
            self.say("Alterações não salvas. Salve com :w antes de abrir outro arquivo.".into(), ToastKind::Error);
            return;
        }
        let (cols, rows, root) = (self.cols, self.rows, self.root.clone());
        let (tree, foco, lider) = (self.tree.take(), self.tree_focus, self.leader_key);
        let pool = self.lsp_pool.clone();
        let (pane_id, splits, jan) = (self.pane_id, self.splits.take(), self.jan_pedido.take());
        let (term, term_focus, term_zen, term_dims, term_screen, notify) = (self.term.take(), self.term_focus, self.term_zen, self.term_dims, self.term_screen.take(), self.notify.take());
        *self = Session::open(path);
        self.pane_id = pane_id;
        self.splits = splits;
        self.jan_pedido = jan;
        self.root = root;
        self.leader_key = lider;
        self.term = term;
        self.term_focus = term_focus;
        self.term_zen = term_zen;
        self.term_dims = term_dims;
        self.term_screen = term_screen;
        self.notify = notify;
        self.lsp_pool = pool;
        self.lsp_attach();
        self.tree = tree;
        self.tree_focus = foco;
        self.resize(cols, rows);
        busca::lembrar(path);
        if let Some(t) = self.tree.as_mut() {
            let abs = if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) };
            t.revelar(&abs);
        }
        if let Some((r, c)) = goto {
            self.ed.set_cursor(r, c);
        }
    }

    fn key_inner(&mut self, k: Key) {
        // qualquer tecla fecha o hover aberto
        self.hover_text = None;
        if self.ed.mode() == Mode::Insert && self.comp.as_ref().is_some_and(|c| !c.shown.is_empty()) {
            match k {
                Key::Tab | Key::Ctrl('y') => return self.comp_accept(),
                Key::Ctrl('n') => return self.comp_move(1),
                Key::Ctrl('p') => return self.comp_move(-1),
                Key::Esc | Key::Enter => {
                    self.comp = None;
                    self.comp_req = None;
                    self.comp_block = self.ed.buf.rev;
                }
                _ => {}
            }
        }
        let livre = self.ed.cmdline().is_none() && self.ed.mode() == Mode::Normal;
        if livre && !self.ed.has_pending() && k == Key::Char('K') {
            return self.lsp_hover();
        }
        if livre && self.ed.showcmd() == "g" && matches!(k, Key::Char('t' | 'T')) {
            self.ed.feed(&[Key::Esc]);
            self.aba_pedido = Some(if k == Key::Char('t') { PedidoAba::Proxima } else { PedidoAba::Anterior });
            return;
        }
        if livre && self.ed.showcmd() == "g" && k == Key::Char('d') {
            self.ed.feed(&[Key::Esc]);
            return self.lsp_definition();
        }
        if livre && self.ed.showcmd() == "g" && matches!(k, Key::Char('r' | 'y')) && self.lsp.is_some() {
            self.ed.feed(&[Key::Esc]);
            return self.lsp_consulta(if k == Key::Char('r') { PedidoLsp::Referencias } else { PedidoLsp::TipoDaDefinicao });
        }
        if livre && matches!(self.ed.showcmd().as_str(), "]" | "[") && k == Key::Char('d') {
            let adiante = self.ed.showcmd() == "]";
            self.ed.feed(&[Key::Esc]);
            return self.diag_pular(adiante);
        }
        self.ed.feed(&[k]);
        self.comp_update();
        self.sig_update(k);
        // `:s`, `:d`, `:g`... o núcleo resolve; `:w`, `:q`, `:e`... voltam para cá
        if let Some(linha) = self.ed.take_host_cmd() {
            self.exec_ex(&linha);
        }
        if self.ed.beeped() && self.ed.mode() == Mode::Normal {
            // o Vim apita; aqui só ignoramos (sem aviso na tela)
        }
    }

    /// Clique: `row` é a linha do buffer e `vcol` a coluna visual (a cena já fez a conta).
    pub fn click(&mut self, row: usize, vcol: usize) {
        if self.dash || self.picker.is_some() {
            return;
        }
        let r = row.min(self.ed.buf.line_count() - 1);
        let line = self.ed.buf.line(r);
        let len = line.chars().count();
        let c = if vcol >= vcol_start(&line, len) { len.saturating_sub(1) } else { col_for_vcol(&line, vcol) };
        self.ed.set_cursor(r, c);
    }

    /// Rolagem em linhas (pode ser fracionária): `lines > 0` desce.
    pub fn scroll(&mut self, lines: f32) {
        let max = self.ed.buf.line_count().saturating_sub(1) as f32;
        let wanted = self.top + lines;
        self.top = wanted.clamp(0.0, max);
        if wanted != self.top {
            let id = self.bump.map_or(1, |(i, _)| i + 1);
            self.bump = Some((id, wanted - self.top));
        }
        // o cursor acompanha para não sair da tela
        let so = SCROLLOFF.min((self.rows - 1.0) / 2.0);
        let row = self.ed.cursor.row as f32;
        let (lo, hi) = (self.top + so, self.top + self.rows - 1.0 - so);
        if row < lo {
            self.ed.set_cursor(lo.ceil() as usize, self.ed.cursor.col);
        } else if row > hi {
            self.ed.set_cursor(hi.floor().max(0.0) as usize, self.ed.cursor.col);
        }
    }

    // ------------------------------------------------------------------
    // comandos `:`
    // ------------------------------------------------------------------

    /// Segunda tecla do `<C-w>`.
    fn janela_tecla(&mut self, k: Key) {
        let c = match k {
            Key::Char(c) => c,
            Key::Ctrl(c) => c,
            _ => return,
        };
        self.jan_pedido = match c {
            's' | 'S' => Some(PedidoJanela::Dividir { dir: Dir::Col, arquivo: None, vazio: false }),
            'v' => Some(PedidoJanela::Dividir { dir: Dir::Row, arquivo: None, vazio: false }),
            'n' => Some(PedidoJanela::Dividir { dir: Dir::Col, arquivo: None, vazio: true }),
            'h' => Some(PedidoJanela::Foco(Lado::Esquerda)),
            'j' => Some(PedidoJanela::Foco(Lado::Baixo)),
            'k' => Some(PedidoJanela::Foco(Lado::Cima)),
            'l' => Some(PedidoJanela::Foco(Lado::Direita)),
            'w' => Some(PedidoJanela::Ciclo(1)),
            'W' => Some(PedidoJanela::Ciclo(-1)),
            'p' => Some(PedidoJanela::Anterior),
            'c' => Some(PedidoJanela::Fechar { forcar: false }),
            'q' => Some(PedidoJanela::Fechar { forcar: false }),
            'o' => Some(PedidoJanela::So),
            '=' => Some(PedidoJanela::Igualar),
            '_' => Some(PedidoJanela::Maximizar(Dir::Col)),
            '|' => Some(PedidoJanela::Maximizar(Dir::Row)),
            _ => None,
        };
    }

    fn exec_ex(&mut self, line: &str) {
        // o núcleo já aplicou a faixa (`1,3`, `%`, `'<,'>`): aqui só interessa o nome do comando
        let line = line.trim().trim_start_matches(|c: char| c.is_ascii_digit() || ",.$%'<>+-; :".contains(c)).trim();
        let (name, arg) = match line.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (line, ""),
        };
        let bang = name.ends_with('!');
        let name = name.trim_end_matches('!');
        if name.is_empty() {
            return;
        }
        if let Ok(n) = name.parse::<usize>() {
            let row = n.saturating_sub(1).min(self.ed.buf.line_count() - 1);
            self.ed.set_cursor(row, 0);
            self.ed.feed_str("^");
            return;
        }
        match name {
            "noh" | "nohl" | "nohlsearch" => self.hl_off = true,
            "w" | "write" => {
                self.write(arg);
                self.git_load();
            }
            "q" | "quit" => {
                if self.modified() && !bang && !self.irmao {
                    self.say("Alterações não salvas. Use :q! para sair sem salvar.".into(), ToastKind::Error);
                } else {
                    self.quit = true;
                }
            }
            "wq" | "x" | "xit" | "exit" => self.write_sair(arg, true),
            "e" | "edit" => {
                if arg.is_empty() {
                    return;
                }
                if self.modified() && !bang {
                    self.say("Alterações não salvas. Use :e! para descartar.".into(), ToastKind::Error);
                } else {
                    self.saved_rev = self.ed.buf.rev;
                    self.open_file_aqui(Path::new(arg), None);
                }
            }
            "sp" | "split" | "new" | "vs" | "vsp" | "vsplit" | "vne" | "vnew" => {
                let dir = if name.starts_with('v') { Dir::Row } else { Dir::Col };
                let vazio = name.ends_with("new") || name == "vne";
                self.jan_pedido = Some(PedidoJanela::Dividir { dir, arquivo: (!arg.is_empty()).then(|| PathBuf::from(arg)), vazio });
            }
            "Rename" | "LspRename" => self.lsp_renomear(arg),
            "Format" | "LspFormat" => {
                self.lsp_formatar(false, false);
            }
            "CodeAction" | "LspCodeAction" => self.lsp_acoes(),
            "Symbols" | "LspSymbols" => self.lsp_consulta(PedidoLsp::Simbolos),
            "References" | "LspReferences" => self.lsp_consulta(PedidoLsp::Referencias),
            "Diagnostics" | "LspDiagnostics" => self.diag_lista(),
            "clo" | "close" => self.jan_pedido = Some(PedidoJanela::Fechar { forcar: bang }),
            "on" | "only" => self.jan_pedido = Some(PedidoJanela::So),
            "tabnew" | "tabe" | "tabedit" => self.aba_pedido = Some(PedidoAba::Nova((!arg.is_empty()).then(|| PathBuf::from(arg)))),
            "tabc" | "tabclose" => self.aba_pedido = Some(PedidoAba::Fechar { forcar: bang }),
            "tabn" | "tabnext" => self.aba_pedido = Some(PedidoAba::Proxima),
            "tabp" | "tabprevious" | "tabN" | "tabNext" => self.aba_pedido = Some(PedidoAba::Anterior),
            "tabo" | "tabonly" => self.aba_pedido = Some(PedidoAba::So),
            "noh" | "nohlsearch" | "set" | "syntax" | "filetype" => {}
            _ => self.say(format!("Comando desconhecido: {name}"), ToastKind::Error),
        }
    }

    /// `:w`: com formatar ao salvar, pede ao servidor e grava quando ele responder (ou ao fim da espera).
    fn write(&mut self, arg: &str) {
        self.write_sair(arg, false);
    }

    fn write_sair(&mut self, arg: &str, sair: bool) {
        if arg.is_empty() && self.formatar_ao_salvar && self.lsp_formatar(true, sair) {
            return;
        }
        self.gravar(arg);
        if sair && !self.modified() {
            self.quit = true;
        }
    }

    fn gravar(&mut self, arg: &str) {
        let target = if arg.is_empty() { self.path.clone() } else { Some(PathBuf::from(arg)) };
        let Some(p) = target else {
            self.say("Sem nome de arquivo. Use :w nome".into(), ToastKind::Error);
            return;
        };
        let text = self.ed.to_file_text();
        match std::fs::write(&p, &text) {
            Ok(()) => {
                let nome = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.say(format!("{nome} salvo · {} linhas", self.ed.buf.line_count()), ToastKind::Done);
                if self.path.is_none() {
                    self.path = Some(p);
                }
                self.saved_rev = self.ed.buf.rev;
            }
            Err(e) => self.say(format!("Não consegui salvar: {e}"), ToastKind::Error),
        }
    }

    // ------------------------------------------------------------------
    // modelo da tela
    // ------------------------------------------------------------------

    fn refresh_syntax(&mut self) {
        let Some(l) = self.lang else { return };
        let rev = self.ed.buf.rev;
        if self.doc.as_ref().is_some_and(|(_, r)| *r == rev) {
            return;
        }
        let lines = self.ed.lines();
        self.doc = Some((Document::new(l, &lines), rev));
    }

    /// Mantém o cursor à vista: rola o mínimo (ou centraliza num salto longe).
    fn follow_cursor(&mut self) {
        let so = SCROLLOFF.min((self.rows - 1.0) / 2.0);
        let row = self.ed.cursor.row as f32;
        let (lo, hi) = (self.top + so, self.top + self.rows - 1.0 - so);
        if row < self.top - self.rows || row > self.top + 2.0 * self.rows {
            self.top = (row - self.rows / 3.0).max(0.0);
        } else if row < lo {
            self.top = (row - so).max(0.0);
        } else if row > hi {
            self.top = row - self.rows + 1.0 + so;
        }
        let last = self.ed.buf.line_count().saturating_sub(1) as f32;
        self.top = self.top.clamp(0.0, last);
        // horizontal (em colunas visuais)
        let line = self.ed.buf.line(self.ed.cursor.row);
        let v = vcol_cursor(&line, self.ed.cursor.col);
        if v < self.left {
            self.left = v;
        } else if v >= self.left + self.cols {
            self.left = v + 1 - self.cols;
        }
    }

    pub fn view_model(&mut self) -> ViewModel {
        self.term_refresh();
        self.lsp_sync();
        self.follow_cursor();
        self.refresh_syntax();
        self.seq += 1;
        let line_count = self.ed.buf.line_count();
        let lo = (self.top as usize).saturating_sub(SPAN_MARGIN);
        let hi = (self.top as usize + self.rows.ceil() as usize + SPAN_MARGIN).min(line_count);
        let mut spans: Vec<Vec<Span>> = vec![Vec::new(); hi.saturating_sub(lo)];
        if let Some((doc, _)) = &self.doc {
            for sp in doc.highlights(lo..hi) {
                let Some(cls) = classify(sp.group) else { continue };
                let line = self.ed.buf.line(sp.row);
                let to_char = |byte: usize| line.char_indices().take_while(|(b, _)| *b < byte).count() as u32;
                spans[sp.row - lo].push(Span { start: to_char(sp.start_col), end: to_char(sp.end_col), cls });
            }
        }
        let sel = self.ed.selection().map(|(k, a)| {
            let c = self.ed.cursor;
            // em bloco os dois cantos ficam como estão (o retângulo vem das colunas visuais de cada um)
            let (lo, hi) = if a <= c || k == VKind::Block { (a, c) } else { (c, a) };
            (k, (lo.row, lo.col), (hi.row, hi.col))
        });
        let search = self.ed.search_line();
        // realce da busca: ao digitar já mostra as ocorrências; `:noh` apaga até mudar o padrão
        let pat = match &search {
            Some(s) if s.chars().count() > 1 => Some(s.chars().skip(1).collect::<String>()),
            _ => self.ed.last_pattern().map(str::to_string),
        };
        let mut hits = Vec::new();
        let pat = pat.filter(|p| !p.is_empty());
        if pat.as_deref() != Some(self.hl_pat.as_str()) {
            self.hl_pat = pat.clone().unwrap_or_default();
            self.hl_id += 1;
            self.hl_off = false;
        }
        if let (Some(p), false) = (&pat, self.hl_off) {
            if let Some(re) = astrograph_core::search::compile(p) {
                for row in lo..hi {
                    let line = self.ed.buf.line(row);
                    for m in re.find_iter(&line).take(200) {
                        if m.start() == m.end() {
                            continue;
                        }
                        let a = line[..m.start()].chars().count();
                        hits.push((row, a, a + line[m.start()..m.end()].chars().count()));
                    }
                }
            }
        }
        let hits_id = if pat.is_some() && !self.hl_off { self.hl_id } else { 0 };
        let signs = self.git_signs();
        let mode = if self.term_focus && self.term.is_some() {
            ModeKind::Terminal
        } else if self.ed.cmdline().is_some() {
            ModeKind::Command
        } else if search.is_some() {
            ModeKind::Search
        } else {
            match self.ed.mode() {
                Mode::Normal => ModeKind::Normal,
                Mode::Insert => ModeKind::Insert,
                Mode::Visual => match sel.map(|s| s.0) {
                    Some(VKind::Line) => ModeKind::VisualLine,
                    Some(VKind::Block) => ModeKind::VisualBlock,
                    _ => ModeKind::Visual,
                },
            }
        };
        let cmd = self
            .ed
            .cmdline()
            .map(|c| (':', c.to_string()))
            .or_else(|| search.map(|s| (s.chars().next().unwrap_or('/'), s.chars().skip(1).collect())));
        ViewModel {
            tabs: Vec::new(),
            tab_active: 0,
            doc_id: self.doc_id,
            seq: self.seq,
            rev: self.ed.buf.rev,
            rope: self.ed.buf.rope.clone(),
            lines: line_count,
            span_lo: lo,
            spans,
            top: self.top,
            left: self.left,
            cursor: (self.ed.cursor.row, self.ed.cursor.col),
            mode,
            selection: sel,
            sel_eol: self.ed.bloco_ate_o_fim(),
            name: self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Sem nome".into()),
            modified: self.modified(),
            cmd,
            toast: self.toast.clone(),
            showcmd: if !self.leader.is_empty() {
                // <Espaço>f...: mostra a líder pelo nome da tecla e o que já foi digitado
                std::iter::once(tecla_nome(self.leader_key)).chain(self.leader[1..].iter().filter_map(|k| k.as_char().map(String::from))).collect()
            } else if self.ed.search_line().is_some() {
                String::new()
            } else {
                self.ed.showcmd()
            },
            bump: self.bump,
            dash: self.dash,
            diags: self.lsp_diags.clone(),
            hover: self.hover_text.clone(),
            completion: self.comp.as_ref().filter(|c| !c.shown.is_empty()).map(|c| {
                let first = c.sel.saturating_sub(COMP_LINHAS - 1);
                CompView {
                    items: c.shown.iter().skip(first).take(COMP_LINHAS).map(|(i, h)| CompItem { label: c.items[*i].label.clone(), detail: c.items[*i].detalhe.clone(), kind: c.items[*i].tipo, hits: h.clone() }).collect(),
                    sel: c.sel - first,
                    anchor: (c.start.row, c.start.col),
                }
            }),
            git: self.git_branch.clone().map(|branch| GitView { branch, signs: signs.clone() }),
            term: self.term_screen.as_ref().map(|s| TermView { screen: s.clone(), focus: self.term_focus, zen: self.term_zen }),
            tree: self.tree.as_ref().map(|t| {
                let first = t.sel.saturating_sub(100);
                TreeView {
                    root: t.raiz.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| t.raiz.display().to_string()),
                    items: t.nos.iter().skip(first).take(240).map(|n| TreeItem { name: n.nome.clone(), level: n.nivel, dir: n.pasta, open: n.aberta }).collect(),
                    first,
                    sel: t.sel,
                    total: t.nos.len(),
                    focus: self.tree_focus,
                }
            }),
            picker: self.picker.as_ref().map(|p| PickerView {
                title: match p.kind {
                    PickKind::Arquivos => "Buscar arquivo",
                    PickKind::Texto => "Buscar texto",
                    PickKind::Recentes => "Recentes",
                    PickKind::Lista => p.titulo.as_str(),
                }
                .into(),
                query: p.query.clone(),
                items: p.items.iter().skip(p.sel.saturating_sub(PICK_LINHAS - 1)).take(PICK_LINHAS).cloned().collect(),
                sel: p.sel.min(PICK_LINHAS - 1),
                total: p.total,
            }),
            hits,
            hits_id,
            flash: (self.ed.last_yank.0 > 0).then(|| {
                let (id, a, b, _) = self.ed.last_yank;
                ((a.row, a.col), (b.row, b.col), id)
            }),
            layout: None,
            pane: self.pane_id,
            panes: Vec::new(),
        }
    }
}

/// Aplica as trocas num arquivo que não está aberto, reescrevendo-o. `false` se não deu.
fn aplicar_no_disco(arq: &Path, ed: &[lsp::Edicao]) -> bool {
    let Ok(bytes) = std::fs::read(arq) else { return false };
    let texto = String::from_utf8_lossy(&bytes).into_owned();
    let termina_com_nl = texto.ends_with('\n');
    let linhas: Vec<String> = texto.strip_suffix('\n').unwrap_or(&texto).split('\n').map(str::to_string).collect();
    let mut e = Editor::new(&linhas);
    let n = e.buf.line_count();
    let conv = |lin: usize, c16: usize, buf: &astrograph_core::buffer::Buffer| -> Pos {
        let lin = lin.min(n - 1);
        Pos::new(lin, lsp::col_chars(&buf.line(lin), c16))
    };
    let v: Vec<(Pos, Pos, String)> = ed.iter().map(|x| (conv(x.ini.0, x.ini.1, &e.buf), conv(x.fim.0, x.fim.1, &e.buf), x.novo.clone())).collect();
    e.aplicar_edicoes(&v);
    let mut saida = e.lines().join("\n");
    if termina_com_nl {
        saida.push('\n');
    }
    std::fs::write(arq, saida).is_ok()
}

/// Letra curta para o tipo do símbolo (`SymbolKind` do LSP).
fn tipo_simbolo(k: u8) -> &'static str {
    match k {
        2 | 3 | 4 => "mod",
        5 => "class",
        6 | 9 | 12 => "fn",
        7 | 8 => "campo",
        10 => "enum",
        11 => "trait",
        13 | 14 | 15 | 16 => "var",
        22 => "variante",
        23 => "struct",
        _ => "·",
    }
}

/// `<C-w>o` só descarta painéis se nenhuma alteração se perder (os do mesmo arquivo do focado
/// compartilham o texto, então valem como salvos).
fn forcar_so(sp: &Splits) -> bool {
    sp.outros.iter().all(|o| !o.modified() || o.irmao)
}

/// Passa o que é "do espaço de trabalho" (explorador, terminal, líder) de uma aba para outra.
fn mover_paineis(de: &mut Session, para: &mut Session) {
    para.tree = de.tree.take();
    para.tree_focus = std::mem::take(&mut de.tree_focus);
    para.term = de.term.take();
    para.term_focus = std::mem::take(&mut de.term_focus);
    para.term_zen = std::mem::take(&mut de.term_zen);
    para.term_esc = std::mem::take(&mut de.term_esc);
    para.term_dims = de.term_dims;
    para.term_screen = de.term_screen.take();
    para.leader_key = de.leader_key;
    para.root = de.root.clone();
}

/// As abas abertas e a ativa. Cada aba é uma sessão; os servidores de linguagem são compartilhados.
pub struct Workspace {
    abas: Vec<Session>,
    ativa: usize,
    pool: LspPool,
    notify: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    leader: char,
    /// tamanho da área de texto (colunas, linhas)
    dims: (usize, f32),
    pub sair: bool,
}

impl Workspace {
    pub fn new(arquivo: Option<PathBuf>, leader: char, notify: std::sync::Arc<dyn Fn() + Send + Sync>) -> Self {
        let pool: LspPool = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let mut s = match &arquivo {
            Some(p) => Session::open(p),
            None => Session::welcome(),
        };
        s.lsp_pool = pool.clone();
        s.set_leader(leader);
        s.set_notify(notify.clone());
        s.modo_abas = true;
        s.lsp_attach();
        Workspace { abas: vec![s], ativa: 0, pool, notify: Some(notify), leader, dims: (80, 24.0), sair: false }
    }

    fn atual(&mut self) -> &mut Session {
        &mut self.abas[self.ativa]
    }

    fn nova_sessao(&self, arquivo: Option<&Path>) -> Session {
        let mut s = match arquivo {
            Some(p) => Session::open(p),
            None => Session::new("", None),
        };
        s.lsp_pool = self.pool.clone();
        s.set_leader(self.leader);
        if let Some(n) = &self.notify {
            s.set_notify(n.clone());
        }
        s.modo_abas = true;
        s.resize(self.dims.0, self.dims.1);
        s.lsp_attach();
        s
    }

    fn trocar(&mut self, i: usize) {
        if i == self.ativa || i >= self.abas.len() {
            return;
        }
        let (a, b) = if self.ativa < i {
            let (x, y) = self.abas.split_at_mut(i);
            (&mut x[self.ativa], &mut y[0])
        } else {
            let (x, y) = self.abas.split_at_mut(self.ativa);
            (&mut y[0], &mut x[i])
        };
        mover_paineis(a, b);
        self.ativa = i;
    }

    fn abrir_em_aba(&mut self, arquivo: &Path, goto: Option<(usize, usize)>) {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let alvo = canon(arquivo);
        if let Some(i) = self.abas.iter().position(|s| s.path.as_deref().map(canon).as_ref() == Some(&alvo)) {
            self.trocar(i);
        } else {
            let mut nova = self.nova_sessao(Some(arquivo));
            let ativa = self.ativa;
            mover_paineis(&mut self.abas[ativa], &mut nova);
            self.abas.insert(self.ativa + 1, nova);
            self.ativa += 1;
        }
        if let Some((r, c)) = goto {
            let s = self.atual();
            let c = c.min(s.ed.buf.line_len(r.min(s.ed.buf.line_count() - 1)));
            s.ed.set_cursor(r, c);
        }
    }

    fn fechar_aba(&mut self, forcar: bool) {
        if self.abas.len() == 1 {
            // última aba: `:q` normal
            if self.atual().tem_alteracoes() && !forcar {
                self.atual().say("Alterações não salvas. Use :q! para sair sem salvar.".into(), ToastKind::Error);
            } else {
                self.sair = true;
            }
            return;
        }
        if self.atual().tem_alteracoes() && !forcar {
            self.atual().say("Alterações não salvas. Use :tabc! para fechar sem salvar.".into(), ToastKind::Error);
            return;
        }
        let ativa = self.ativa;
        let mut velha = self.abas.remove(ativa);
        self.ativa = ativa.min(self.abas.len() - 1);
        let nova = self.ativa;
        mover_paineis(&mut velha, &mut self.abas[nova]);
    }

    // ------------------------------------------------------------------
    // janelas divididas
    // ------------------------------------------------------------------

    fn janela(&mut self, p: PedidoJanela) {
        use crate::janelas::{vizinho, Rect};
        let ativa = self.ativa;
        match p {
            PedidoJanela::Dividir { dir, arquivo, vazio } => {
                let foco = &self.abas[ativa];
                let (cols, rows) = (foco.cols, foco.rows);
                let mut nova = match (&arquivo, vazio) {
                    (Some(a), _) => self.nova_sessao(Some(a)),
                    (None, true) => self.nova_sessao(None),
                    (None, false) => {
                        let origem = &self.abas[ativa];
                        let mut n = self.nova_sessao(origem.path.as_deref());
                        n.ed.sincronizar(&origem.ed.buf.rope);
                        if !origem.modified() {
                            n.saved_rev = n.ed.buf.rev;
                        }
                        n.ed.set_cursor(origem.ed.cursor.row, origem.ed.cursor.col);
                        n.top = origem.top;
                        n.left = origem.left;
                        n
                    }
                };
                nova.resize(cols, rows);
                let foco = &mut self.abas[ativa];
                let mut sp = foco.splits.take().unwrap_or_else(|| Box::new(Splits { layout: Layout::leaf(foco.pane_id), outros: Vec::new(), proximo_id: foco.pane_id + 1, ultimo: None }));
                let id = sp.proximo_id;
                sp.proximo_id += 1;
                sp.layout.dividir(foco.pane_id, dir, id, false);
                nova.pane_id = id;
                let mut velho = std::mem::replace(&mut self.abas[ativa], nova);
                mover_paineis(&mut velho, &mut self.abas[ativa]);
                sp.ultimo = Some(velho.pane_id);
                sp.outros.push(velho);
                self.abas[ativa].splits = Some(sp);
            }
            PedidoJanela::Foco(lado) => {
                let Some(sp) = self.abas[ativa].splits.as_ref() else { return };
                let rects = sp.layout.rects(Rect::new(0.0, 0.0, 1000.0, 1000.0), 0.0);
                if let Some(id) = vizinho(&rects, self.abas[ativa].pane_id, lado) {
                    self.focar(id);
                }
            }
            PedidoJanela::Ciclo(d) => {
                let Some(sp) = self.abas[ativa].splits.as_ref() else { return };
                let ids = sp.layout.ids();
                let i = ids.iter().position(|x| *x == self.abas[ativa].pane_id).unwrap_or(0) as i32;
                let n = ids.len() as i32;
                self.focar(ids[(i + d).rem_euclid(n) as usize]);
            }
            PedidoJanela::Anterior => {
                if let Some(id) = self.abas[ativa].splits.as_ref().and_then(|sp| sp.ultimo) {
                    self.focar(id);
                }
            }
            PedidoJanela::Fechar { forcar } => self.fechar_painel(forcar),
            PedidoJanela::So => {
                let foco = &mut self.abas[ativa];
                let Some(sp) = foco.splits.as_ref() else { return };
                if !forcar_so(sp) {
                    foco.say("Há painéis com alterações não salvas.".into(), ToastKind::Error);
                    return;
                }
                foco.splits = None;
            }
            PedidoJanela::Igualar => {
                if let Some(sp) = self.abas[ativa].splits.as_mut() {
                    sp.layout.igualar();
                }
            }
            PedidoJanela::Maximizar(dir) => {
                let id = self.abas[ativa].pane_id;
                if let Some(sp) = self.abas[ativa].splits.as_mut() {
                    sp.layout.maximizar(id, dir);
                }
            }
        }
    }

    /// Aplica as trocas de texto do servidor (renomear, ações de código) em todos os arquivos:
    /// os abertos (em qualquer aba ou painel) mudam no buffer, com um passo de desfazer; os outros
    /// são reescritos no disco.
    fn aplicar_edicoes_ws(&mut self, ed: Vec<lsp::Edicao>) {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let mut grupos: Vec<(PathBuf, Vec<lsp::Edicao>)> = Vec::new();
        for e in ed {
            match grupos.iter_mut().find(|(a, _)| *a == e.arquivo) {
                Some((_, v)) => v.push(e),
                None => grupos.push((e.arquivo.clone(), vec![e])),
            }
        }
        let (mut total, mut abertos, mut disco) = (0usize, 0usize, 0usize);
        for (arq, lista) in &grupos {
            let alvo = canon(arq);
            let mut achou = false;
            for aba in self.abas.iter_mut() {
                let mut sessoes: Vec<&mut Session> = vec![aba];
                // (os painéis ficam dentro da sessão focada)
                let outros: Vec<*mut Session> = sessoes[0].splits.as_mut().map(|sp| sp.outros.iter_mut().map(|o| o as *mut Session).collect()).unwrap_or_default();
                for o in outros {
                    // SAFETY: `outros` são sessões distintas da própria `aba`, vivas durante o laço
                    sessoes.push(unsafe { &mut *o });
                }
                for s in sessoes {
                    if s.path.as_deref().map(canon).as_ref() == Some(&alvo) {
                        s.aplicar_edicoes_aqui(lista);
                        achou = true;
                    }
                }
            }
            if achou {
                abertos += 1;
            } else if aplicar_no_disco(arq, lista) {
                disco += 1;
            }
            total += lista.len();
        }
        let msg = match (abertos, disco) {
            (_, 0) => format!("{total} alterações em {abertos} arquivo(s)"),
            (0, _) => format!("{total} alterações gravadas em {disco} arquivo(s) no disco"),
            _ => format!("{total} alterações: {abertos} aberto(s), {disco} gravado(s) no disco"),
        };
        self.atual().say(msg, ToastKind::Done);
    }

    /// Dá o foco ao painel `id` da aba ativa (o editor vai para a lista de painéis do outro).
    pub fn focar(&mut self, id: PaneId) {
        let ativa = self.ativa;
        if self.abas[ativa].pane_id == id {
            return;
        }
        let Some(mut sp) = self.abas[ativa].splits.take() else { return };
        let Some(pos) = sp.outros.iter().position(|s| s.pane_id == id) else {
            self.abas[ativa].splits = Some(sp);
            return;
        };
        let novo = sp.outros.remove(pos);
        let mut velho = std::mem::replace(&mut self.abas[ativa], novo);
        mover_paineis(&mut velho, &mut self.abas[ativa]);
        sp.ultimo = Some(velho.pane_id);
        sp.outros.push(velho);
        self.abas[ativa].splits = Some(sp);
    }

    /// `:close` / `:q` com painéis: fecha o focado e o espaço vai para o vizinho. Alterações só
    /// se perdem se forem do único painel com aquele buffer.
    fn fechar_painel(&mut self, forcar: bool) {
        let ativa = self.ativa;
        let foco = &mut self.abas[ativa];
        let Some(sp) = foco.splits.as_ref() else {
            foco.say("Último painel da aba (use :tabc ou :q).".into(), ToastKind::Error);
            return;
        };
        if foco.modified() && !forcar && !foco.irmao {
            foco.say("Alterações não salvas. Use :close! para fechar sem salvar.".into(), ToastKind::Error);
            return;
        }
        let id = foco.pane_id;
        let ids = sp.layout.ids();
        let i = ids.iter().position(|x| *x == id).unwrap_or(0);
        let destino = sp.ultimo.filter(|u| *u != id && ids.contains(u)).unwrap_or(if i > 0 { ids[i - 1] } else { ids[1] });
        self.focar(destino);
        // o painel que perdeu o foco é o fechado: sai da lista e do layout
        let foco = &mut self.abas[ativa];
        if let Some(sp) = foco.splits.as_mut() {
            sp.outros.retain(|s| s.pane_id != id);
            sp.layout.fechar(id);
            sp.ultimo = None;
            if sp.outros.is_empty() {
                foco.splits = None;
            }
        }
    }

    /// Painéis do mesmo arquivo compartilham o texto: o que o focado mudou vai para os outros (que
    /// guardam o texto antigo no desfazer) e o estado de "salvo" acompanha.
    fn sincronizar_irmaos(&mut self) {
        let foco = &mut self.abas[self.ativa];
        if foco.splits.is_none() {
            foco.irmao = false;
            return;
        }
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let meu = foco.path.as_deref().map(canon);
        let rope = foco.ed.buf.rope.clone();
        let modificado = foco.modified();
        let sp = foco.splits.as_mut().unwrap();
        let mut algum = false;
        for o in sp.outros.iter_mut() {
            let mesmo = meu.is_some() && o.path.as_deref().map(canon) == meu;
            o.irmao = mesmo;
            if !mesmo {
                continue;
            }
            algum = true;
            if o.ed.buf.rope != rope {
                o.ed.sincronizar(&rope);
            }
            if modificado {
                if o.saved_rev == o.ed.buf.rev {
                    o.saved_rev = o.ed.buf.rev.wrapping_sub(1);
                }
            } else {
                o.saved_rev = o.ed.buf.rev;
            }
        }
        foco.irmao = algum;
    }

    /// Rola o painel `id` (a roda do mouse age sobre o painel sob o ponteiro, sem tirar o foco).
    pub fn rolar_painel(&mut self, id: PaneId, linhas: f32) {
        let foco = &mut self.abas[self.ativa];
        if foco.pane_id == id {
            foco.scroll(linhas);
        } else if let Some(o) = foco.splits.as_mut().and_then(|sp| sp.outros.iter_mut().find(|o| o.pane_id == id)) {
            o.scroll(linhas);
        }
    }

    /// Tamanho de cada painel (colunas, linhas), calculado pela tela a partir do layout.
    pub fn tamanhos_paineis(&mut self, tam: &[(PaneId, usize, f32)]) {
        let foco = &mut self.abas[self.ativa];
        for &(id, c, r) in tam {
            if foco.pane_id == id {
                foco.resize(c, r);
            } else if let Some(o) = foco.splits.as_mut().and_then(|sp| sp.outros.iter_mut().find(|o| o.pane_id == id)) {
                o.resize(c, r);
            }
        }
    }

    /// Arrasta um divisor do layout (a tela manda o índice e a posição em pixels).
    pub fn arrastar_divisor(&mut self, idx: usize, area: crate::janelas::Rect, gap: f32, pos: f32) {
        if let Some(sp) = self.abas[self.ativa].splits.as_mut() {
            sp.layout.arrastar(idx, area, gap, pos);
        }
    }

    /// Resolve o que a aba ativa pediu (abrir arquivo, comandos de aba, sair).
    fn depois(&mut self) {
        for _ in 0..8 {
            // as edições do painel focado chegam aos irmãos antes de qualquer pedido (fechar, dividir)
            self.sincronizar_irmaos();
            let s = self.atual();
            if let Some((p, g)) = s.abrir_pedido.take() {
                self.abrir_em_aba(&p, g);
                continue;
            }
            if let Some(pedido) = s.aba_pedido.take() {
                match pedido {
                    PedidoAba::Nova(p) => {
                        let mut nova = self.nova_sessao(p.as_deref());
                        let ativa = self.ativa;
                        mover_paineis(&mut self.abas[ativa], &mut nova);
                        self.abas.insert(self.ativa + 1, nova);
                        self.ativa += 1;
                    }
                    PedidoAba::Fechar { forcar } => self.fechar_aba(forcar),
                    PedidoAba::Proxima => {
                        let n = self.abas.len();
                        self.trocar((self.ativa + 1) % n);
                    }
                    PedidoAba::Anterior => {
                        let n = self.abas.len();
                        self.trocar((self.ativa + n - 1) % n);
                    }
                    PedidoAba::So => {
                        let manter = self.abas.remove(self.ativa);
                        self.abas = vec![manter];
                        self.ativa = 0;
                    }
                }
                continue;
            }
            if let Some(p) = s.jan_pedido.take() {
                self.janela(p);
                continue;
            }
            if let Some(ed) = s.edicoes_pedido.take() {
                self.aplicar_edicoes_ws(ed);
                continue;
            }
            if s.quit {
                s.quit = false;
                // `:q` com painéis divididos fecha o painel; com várias abas, a aba; na última, sai
                if s.splits.is_some() {
                    self.janela(PedidoJanela::Fechar { forcar: true });
                } else if self.abas.len() > 1 {
                    self.fechar_aba(true);
                } else {
                    self.sair = true;
                }
                continue;
            }
            break;
        }
    }

    pub fn feed(&mut self, teclas: &str) {
        self.atual().feed(teclas);
        self.depois();
        self.sincronizar_irmaos();
    }

    pub fn resize(&mut self, cols: usize, rows: f32) {
        self.dims = (cols, rows);
        // com painéis, o tamanho de cada um vem de `tamanhos_paineis`
        if self.atual().splits.is_none() {
            self.atual().resize(cols, rows);
        }
    }

    pub fn scroll(&mut self, linhas: f32) {
        self.atual().scroll(linhas);
    }

    pub fn click(&mut self, row: usize, col: usize) {
        self.atual().click(row, col);
    }

    pub fn term_resize(&mut self, r: u16, c: u16) {
        self.atual().term_resize(r, c);
    }

    /// Vai para a aba `i` (clique na barra).
    pub fn ir_para(&mut self, i: usize) {
        self.trocar(i);
    }

    pub fn view_model(&mut self) -> ViewModel {
        let mut vm = self.abas[self.ativa].view_model();
        vm.tabs = self
            .abas
            .iter()
            .map(|s| crate::nview::TabInfo { name: s.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Sem nome".into()), modified: s.modified() })
            .collect();
        vm.tab_active = self.ativa;
        if let Some(sp) = self.abas[self.ativa].splits.as_mut() {
            vm.layout = Some(sp.layout.clone());
            for o in sp.outros.iter_mut() {
                let id = o.pane_id;
                vm.panes.push(crate::nview::PaneView { id, vm: Box::new(o.view_model()) });
            }
        }
        vm
    }

    pub fn n_abas(&self) -> usize {
        self.abas.len()
    }
}

/// Tarefa do editor nativo: lê a fila de comandos da UI e publica o modelo.
pub async fn native_task(
    shared: Shared,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::UiCommand>,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
    file: Option<PathBuf>,
    leader: char,
    tx: tokio::sync::mpsc::UnboundedSender<crate::UiCommand>,
) {
    use crate::UiCommand;
    let notify: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || {
        let _ = tx.send(crate::UiCommand::TermOut);
    });
    let mut w = Workspace::new(file, leader, notify);
    while let Some(cmd) = rx.recv().await {
        match cmd {
            UiCommand::Keys(k) => w.feed(&k),
            UiCommand::Resize(c, r) => w.resize(c.max(1) as usize, r.max(1) as f32),
            UiCommand::Focus(_) => continue,
            UiCommand::Scroll(lines) => w.scroll(lines as f32),
            UiCommand::TermOut => {}
            UiCommand::TermSize(r, c) => w.term_resize(r.max(2) as u16, c.max(10) as u16),
            UiCommand::Tab(i) => w.ir_para(i.max(0) as usize),
            UiCommand::Panes(t) => {
                let t: Vec<(PaneId, usize, f32)> = t.into_iter().map(|(id, c, r)| (id, c.max(1) as usize, r.max(1) as f32)).collect();
                w.tamanhos_paineis(&t);
            }
            UiCommand::PaneClick { id, row, col } => {
                w.focar(id);
                w.click(row.max(0) as usize, col.max(0) as usize);
            }
            UiCommand::PaneScroll(id, linhas) => w.rolar_painel(id, linhas as f32),
            UiCommand::Divisor { idx, area, gap, pos } => w.arrastar_divisor(idx, crate::janelas::Rect::new(area.0, area.1, area.2, area.3), gap, pos),
            UiCommand::Mouse { button, action, row, col, .. } => match (button, action) {
                ("left", "press") => w.click(row.max(0) as usize, col.max(0) as usize),
                _ => continue,
            },
        }
        if w.sair {
            let _ = proxy.send_event(crate::UserEvent::Quit);
            return;
        }
        *shared.lock().unwrap() = Arc::new(w.view_model());
        let _ = proxy.send_event(crate::UserEvent::Redraw);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Nav {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// `Some(Some(nav))` navega, `Some(None)` é tecla ignorada, `None` não é nome de tecla de navegação.
fn nav_name(name: &str) -> Option<Option<Nav>> {
    let base = name.rsplit('-').next().unwrap_or(name);
    let modificada = name.contains('-') && name.len() > base.len();
    let nav = match base {
        "up" => Nav::Up,
        "down" => Nav::Down,
        "left" => Nav::Left,
        "right" => Nav::Right,
        "home" => Nav::Home,
        "end" => Nav::End,
        "pageup" | "pgup" => Nav::PageUp,
        "pagedown" | "pgdn" => Nav::PageDown,
        "insert" | "ins" => return Some(None),
        f if f.len() >= 2 && f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => return Some(None),
        _ => return None,
    };
    // com Ctrl/Alt/Shift o comportamento seria outro: por ora, ignora
    Some(if modificada { None } else { Some(nav) })
}

/// Nome da tecla para mostrar ao usuário: `<Espaço>`, `<Tab>` ou `<,>`.
pub fn tecla_nome(c: char) -> String {
    match c {
        ' ' => "<Espaço>".into(),
        '\t' => "<Tab>".into(),
        c => format!("<{c}>"),
    }
}

/// Caminho absoluto em relação à raiz (se estiver dentro dela).
fn relativo(root: &Path, abs: &str) -> String {
    Path::new(abs).strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| abs.to_string())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn modelo(s: &mut Session) -> ViewModel {
        s.view_model()
    }

    fn projeto(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("astrograph-nat-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/alfa.rs"), "fn alfa() {}\nfn agulha() {}\n").unwrap();
        std::fs::write(d.join("src/beta.rs"), "fn beta() {}\n").unwrap();
        std::fs::write(d.join("LEIA.md"), "oi\n").unwrap();
        d
    }

    #[test]
    fn espaco_ff_abre_o_buscador_e_enter_abre_o_arquivo() {
        let d = projeto("ff");
        let mut s = Session::new("x\n", None);
        s.root = d.clone();
        s.resize(80, 20.0);
        s.feed(" ff");
        let m = modelo(&mut s);
        let pv = m.picker.expect("buscador aberto");
        assert_eq!(pv.title, "Buscar arquivo");
        assert_eq!(pv.items.len(), 3);
        s.feed("bet");
        let pv = modelo(&mut s).picker.unwrap();
        assert_eq!(pv.items[0].text, "src/beta.rs");
        assert_eq!(pv.items[0].hits, vec![4, 5, 6]);
        s.feed("<CR>");
        let m = modelo(&mut s);
        assert!(m.picker.is_none());
        assert_eq!(m.name, "beta.rs");
        assert_eq!(m.line(0), "fn beta() {}");
    }

    #[test]
    fn espaco_fw_busca_texto_e_pula_para_a_linha() {
        let d = projeto("fw");
        let mut s = Session::new("x\n", None);
        s.root = d;
        s.resize(80, 20.0);
        s.feed(" fwagulha");
        let pv = modelo(&mut s).picker.unwrap();
        assert_eq!(pv.items.len(), 1);
        assert_eq!(pv.items[0].text, "src/alfa.rs:2");
        s.feed("<CR>");
        let m = modelo(&mut s);
        assert_eq!(m.name, "alfa.rs");
        assert_eq!(m.cursor, (1, 3));
    }

    #[test]
    fn espaco_e_abre_o_explorador_navega_e_abre_arquivo() {
        let d = projeto("tree");
        let mut s = Session::new("x\n", None);
        s.root = d;
        s.resize(80, 20.0);
        s.feed(" e");
        let t = modelo(&mut s).tree.expect("explorador aberto");
        assert_eq!(t.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["src", "LEIA.md"]);
        assert!(t.focus);
        // l abre a pasta, j desce até beta.rs
        s.feed("ljj");
        let t = modelo(&mut s).tree.unwrap();
        assert_eq!(t.items[t.sel].name, "beta.rs");
        s.feed("<CR>");
        let m = modelo(&mut s);
        assert_eq!(m.name, "beta.rs");
        let t = m.tree.expect("o explorador continua aberto");
        assert!(!t.focus, "o foco volta ao texto");
        assert_eq!(t.items[t.sel].name, "beta.rs", "e acompanha o arquivo aberto");
        // com o foco no texto, j é movimento do editor; <Espaço>e devolve o foco, de novo fecha
        s.feed(" e");
        assert!(modelo(&mut s).tree.unwrap().focus);
        s.feed(" e");
        assert!(modelo(&mut s).tree.is_none());
    }

    #[test]
    fn lider_configuravel_e_mostrada_pelo_nome_da_tecla() {
        let d = projeto("lider");
        let mut s = Session::new("abc\n", None);
        s.root = d;
        s.resize(80, 20.0);
        // padrão: <Espaço>
        s.feed(" f");
        assert_eq!(modelo(&mut s).showcmd, "<Espaço>f");
        s.feed("<Esc>");
        assert_eq!(modelo(&mut s).showcmd, "");
        // outra líder: a vírgula abre o buscador e o Espaço volta a ser movimento
        s.set_leader(',');
        s.feed(",f");
        assert_eq!(modelo(&mut s).showcmd, "<,>f");
        s.feed("f");
        assert!(modelo(&mut s).picker.is_some());
        s.feed("<Esc>");
        s.feed(" ");
        assert!(modelo(&mut s).picker.is_none());
        assert_eq!(modelo(&mut s).cursor, (0, 1), "Espaço anda como `l`");
        assert_eq!(tecla_nome('\t'), "<Tab>");
    }

    #[cfg(unix)]
    #[test]
    fn zen_alterna_e_ctrl_barra_ctrl_n_sai() {
        std::env::set_var("SHELL", "/bin/sh");
        let mut s = Session::new("x\n", None);
        s.root = std::env::temp_dir();
        s.resize(80, 20.0);
        s.feed(" t");
        assert!(!modelo(&mut s).term.as_ref().unwrap().zen);
        s.toggle_zen();
        assert!(modelo(&mut s).term.as_ref().is_some_and(|t| t.zen && t.focus), "zen com o teclado no terminal");
        s.toggle_zen();
        assert!(!modelo(&mut s).term.as_ref().unwrap().zen);
        s.toggle_zen();
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        let m = modelo(&mut s);
        assert!(m.term.as_ref().is_some_and(|t| !t.zen && !t.focus), "sair devolve o editor");
        s.term_close();
    }

    #[test]
    fn espaco_t_abre_o_terminal_recebe_teclas_e_devolve_o_foco() {
        std::env::set_var("SHELL", "/bin/sh");
        let mut s = Session::new("x\n", None);
        s.root = std::env::temp_dir();
        s.resize(80, 20.0);
        s.feed(" t");
        let m = modelo(&mut s);
        assert!(m.term.as_ref().is_some_and(|t| t.focus), "terminal aberto e com o teclado");
        assert_eq!(m.mode, ModeKind::Terminal);
        // o shell recebe as teclas (e o editor não)
        s.feed("echo $((6*7))<CR>");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let achou = loop {
            let m = modelo(&mut s);
            let texto: String = m.term.as_ref().map(|t| t.screen.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>() + "\n").collect()).unwrap_or_default();
            if texto.lines().any(|l| l.trim().ends_with("42") && !l.contains("echo")) || std::time::Instant::now() > fim {
                break texto.lines().any(|l| l.trim().ends_with("42") && !l.contains("echo"));
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        let dump: String = modelo(&mut s).term.as_ref().map(|t| t.screen.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>().trim_end().to_string() + "|\n").collect()).unwrap_or_default();
        assert!(achou, "a saída do shell chegou ao instantâneo: {dump}");
        assert_eq!(modelo(&mut s).line(0), "x", "o texto do editor não mudou");
        // <C-\\><C-n> devolve o teclado; <Líder>t volta; de novo fecha
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        let m = modelo(&mut s);
        assert!(m.term.is_some() && m.mode != ModeKind::Terminal);
        s.feed(" t");
        assert!(modelo(&mut s).mode == ModeKind::Terminal);
        // com o teclado no terminal, o líder vai para o shell; <C-\\><C-n> e <Líder>T fecham
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        s.feed(" T");
        assert!(modelo(&mut s).term.is_none());
    }

    #[test]
    fn git_mostra_branch_e_sinais_ao_vivo() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let d = projeto("git");
        let run = |args: &[&str]| assert!(std::process::Command::new("git").args(args).current_dir(&d).output().unwrap().status.success());
        run(&["init", "-q", "-b", "principal"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "x"]);
        let mut s = Session::open(&d.join("src/alfa.rs"));
        s.resize(80, 20.0);
        let g = modelo(&mut s).git.expect("dentro de um repositório");
        assert_eq!(g.branch, "principal");
        assert!(g.signs.is_empty(), "arquivo igual ao HEAD: sem sinais");
        // editar sem salvar já aparece: linha nova no fim e a primeira alterada
        s.feed("Gonovo<Esc>ggA!<Esc>");
        let g = modelo(&mut s).git.unwrap();
        assert!(g.signs.contains(&(0, git::Sign::Changed)), "{:?}", g.signs);
        assert!(g.signs.iter().any(|&(r, k)| k == git::Sign::Added && r >= 2), "{:?}", g.signs);
        // desfazer tudo: some
        s.feed("uu");
        assert!(modelo(&mut s).git.unwrap().signs.is_empty());
    }

    #[test]
    fn lsp_mostra_diagnosticos_hover_e_vai_para_a_definicao() {
        let Some(py) = ["python3", "python"].into_iter().find(|p| std::process::Command::new(p).arg("--version").output().is_ok()) else { return };
        let _trava = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let d = projeto("lsp");
        let script = d.join("falso.py");
        std::fs::write(&script, crate::lsp::testes::FALSO).unwrap();
        std::env::set_var("ASTROGRAPH_LSP_CMD", format!("{py} {}", script.display()));
        let alvo = d.join("src/alfa.rs");
        let mut s = Session::open(&alvo);
        s.root = d.clone();
        s.resize(80, 20.0);
        s.lsp_attach();
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let m = loop {
            let m = modelo(&mut s);
            if !m.diags.is_empty() || std::time::Instant::now() > fim {
                break m;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        assert_eq!(m.diags.len(), 1, "diagnóstico do servidor chegou");
        assert_eq!((m.diags[0].row, m.diags[0].col, m.diags[0].severity), (0, 1, 2));
        // K abre o hover; qualquer tecla fecha
        s.feed("K");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let hover = loop {
            let m = modelo(&mut s);
            if m.hover.is_some() || std::time::Instant::now() > fim {
                break m.hover;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        assert_eq!(hover.as_deref(), Some("fn x()\ndoc da x"));
        s.feed("l");
        assert!(modelo(&mut s).hover.is_none());
        // gd: o falso devolve outro arquivo (que não existe): abre como novo, na linha 4
        s.feed("gd");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < fim && modelo(&mut s).name == "alfa.rs" {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        assert_eq!(modelo(&mut s).name, "alvo x.rs");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn substituir_apagar_e_global_pela_linha_de_comando() {
        let mut s = Session::new("foo bar\nbaz foo\nqux\n", None);
        s.resize(40, 6.0);
        s.feed(":%s/foo/X/g<CR>");
        assert_eq!(s.ed.lines(), vec!["X bar", "baz X", "qux"]);
        // a linha de comando aparece enquanto digita e some no Enter
        s.feed(":2d");
        assert_eq!(modelo(&mut s).cmd, Some((':', "2d".to_string())));
        s.feed("<CR>");
        assert!(modelo(&mut s).cmd.is_none());
        assert_eq!(s.ed.lines(), vec!["X bar", "qux"]);
        s.feed(":g/q/s/q/Q/<CR>");
        assert_eq!(s.ed.lines(), vec!["X bar", "Qux"]);
        // seleção visual vira a faixa '<,'>
        s.feed("ggVj:s/$/!/<CR>");
        assert_eq!(s.ed.lines(), vec!["X bar!", "Qux!"]);
        // desfazer volta tudo de um comando só
        s.feed("u");
        assert_eq!(s.ed.lines(), vec!["X bar", "Qux"]);
    }

    #[test]
    fn completar_do_servidor_filtra_navega_e_aceita() {
        let Some(py) = ["python3", "python"].into_iter().find(|p| std::process::Command::new(p).arg("--version").output().is_ok()) else { return };
        let _trava = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let d = projeto("comp");
        let script = d.join("falso.py");
        std::fs::write(&script, crate::lsp::testes::FALSO).unwrap();
        std::env::set_var("ASTROGRAPH_LSP_CMD", format!("{py} {}", script.display()));
        let mut s = Session::open(&d.join("src/alfa.rs"));
        s.root = d.clone();
        s.resize(80, 20.0);
        s.lsp_attach();
        // abre uma linha nova e digita o começo de uma palavra
        s.feed("Gopri");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let m = loop {
            let m = modelo(&mut s);
            if m.completion.is_some() || std::time::Instant::now() > fim {
                break m;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        let c = m.completion.expect("popup");
        let rotulos: Vec<&str> = c.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(rotulos, vec!["print", "private", "println!"], "filtrado por 'pri' e ordenado: {rotulos:?}");
        assert_eq!(c.sel, 0);
        // digitar mais afina a lista sem novo pedido; Ctrl-n anda; Tab aceita
        s.feed("n");
        let c = modelo(&mut s).completion.unwrap();
        assert_eq!(c.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["print", "println!"]);
        s.feed("<C-n>");
        assert_eq!(modelo(&mut s).completion.unwrap().sel, 1);
        s.feed("<Tab>");
        let m = modelo(&mut s);
        assert!(m.completion.is_none());
        let ultimo = m.lines - 1;
        assert_eq!(m.line(ultimo), "println!(\"\")", "texto do snippet sem os marcadores");
        // Esc fecha o popup sem sair do que foi digitado
        s.feed("<Esc>");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn visual_em_bloco_modelo_e_edicao() {
        let mut s = Session::new("abcd\nefgh\nijkl\n", None);
        s.resize(40, 8.0);
        s.feed("l<C-v>jl");
        let m = modelo(&mut s);
        assert_eq!(m.mode, ModeKind::VisualBlock);
        assert_eq!(m.selection, Some((VKind::Block, (0, 1), (1, 2))), "os dois cantos, sem ordenar");
        s.feed("d");
        assert_eq!(s.ed.lines(), vec!["ad", "eh", "ijkl"]);
        // I em todas as linhas, A no fim com $
        s.feed("gg0<C-v>jjIX<Esc>");
        assert_eq!(s.ed.lines(), vec!["Xad", "Xeh", "Xijkl"]);
        s.feed("gg0<C-v>jj$A!<Esc>");
        assert_eq!(s.ed.lines(), vec!["Xad!", "Xeh!", "Xijkl!"]);
        // y e p em bloco: cola na mesma coluna das linhas de baixo
        s.feed("gg0l<C-v>jly");
        s.feed("G$p");
        assert_eq!(s.ed.lines()[2], "Xijkl!ad");
        assert_eq!(s.ed.lines()[3], "      eh");
    }

    fn texto(vm: &ViewModel) -> String {
        vm.rope.to_string()
    }

    #[test]
    fn janelas_dividir_compartilha_o_buffer_e_foca_pelo_teclado() {
        let d = projeto("jan");
        let mut w = Workspace::new(Some(d.join("src/alfa.rs")), ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.atual().root = d.clone();
        assert!(w.view_model().layout.is_none(), "uma janela só: sem layout");
        // :vsplit — o novo fica à esquerda e com o foco; os dois mostram o mesmo arquivo
        w.feed(":vsplit<CR>");
        let vm = w.view_model();
        let ids = vm.layout.as_ref().unwrap().ids();
        assert_eq!(ids, vec![1, 0], "novo à esquerda");
        assert_eq!((vm.pane, vm.panes.len(), vm.panes[0].id), (1, 1, 0));
        assert_eq!(vm.name, "alfa.rs");
        // editar num painel aparece no outro (mesmo buffer)
        w.feed("ggdd");
        let vm = w.view_model();
        assert_eq!(texto(&vm), "fn agulha() {}");
        assert_eq!(texto(&vm.panes[0].vm), "fn agulha() {}", "o outro painel recebeu a edição");
        assert!(vm.modified && vm.panes[0].vm.modified, "os dois ficam modificados");
        // <C-w>l vai para o painel da direita (o original); <C-w>h volta
        w.feed("<C-w>l");
        assert_eq!(w.view_model().pane, 0);
        w.feed("<C-w>h");
        assert_eq!(w.view_model().pane, 1);
        // salvar num painel limpa o aviso nos dois
        w.feed(":w<CR>");
        let vm = w.view_model();
        assert!(!vm.modified && !vm.panes[0].vm.modified, "salvo nos dois");
        // o desfazer do painel focado traz a linha de volta nos dois
        w.feed("u");
        let vm = w.view_model();
        assert_eq!(texto(&vm).lines().count(), 2);
        assert_eq!(texto(&vm.panes[0].vm).lines().count(), 2);
        // <C-w>s divide o focado em baixo da esquerda: três painéis; <C-w>w percorre em ordem
        w.feed("<C-w>s");
        let vm = w.view_model();
        assert_eq!(vm.layout.as_ref().unwrap().len(), 3);
        let atual = vm.pane;
        w.feed("<C-w>w");
        assert_ne!(w.view_model().pane, atual);
        // :close fecha o focado; o último painel não fecha
        w.feed(":close<CR>");
        assert_eq!(w.view_model().layout.as_ref().unwrap().len(), 2);
        w.feed("<C-w>o");
        let vm = w.view_model();
        assert!(vm.layout.is_none() && vm.panes.is_empty(), "só restou um");
        w.feed(":close<CR>");
        assert!(w.view_model().toast.is_some_and(|t| t.2 == ToastKind::Error), "último painel");
    }

    #[test]
    fn janelas_q_fecha_so_o_painel_e_protege_alteracoes_unicas() {
        let d = projeto("jq");
        let mut w = Workspace::new(Some(d.join("src/alfa.rs")), ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.atual().root = d.clone();
        w.feed(":split<CR>");
        assert_eq!(w.n_abas(), 1);
        // buffer compartilhado e alterado: :q fecha o painel sem perder nada (o outro tem o texto)
        w.feed("x:q<CR>");
        let vm = w.view_model();
        assert!(vm.layout.is_none() && !w.sair, "ficou um painel e o app continua");
        assert!(vm.modified, "a alteração não se perdeu");
        // :new abre um buffer vazio e separado; alterá-lo protege o :q
        w.feed(":new<CR>");
        let vm = w.view_model();
        assert_eq!(vm.panes.len(), 1);
        w.feed("ihello<Esc>:q<CR>");
        assert_eq!(w.view_model().layout.as_ref().unwrap().len(), 2, "alteração única: o :q recusa");
        w.feed(":q!<CR>");
        assert!(w.view_model().layout.is_none());
        assert_eq!(texto(&w.view_model()), "n alfa() {}\nfn agulha() {}", "o painel original ficou como estava");
    }

    #[test]
    fn abas_abrir_trocar_fechar_e_proteger_alteracoes() {
        let d = projeto("abas");
        let mut w = Workspace::new(Some(d.join("src/alfa.rs")), ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.atual().root = d.clone();
        assert_eq!(w.n_abas(), 1);
        // :tabnew com arquivo abre outra aba
        w.feed(&format!(":tabnew {}<CR>", d.join("src/beta.rs").display()));
        assert_eq!(w.n_abas(), 2);
        let vm = w.view_model();
        assert_eq!(vm.tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["alfa.rs", "beta.rs"]);
        assert_eq!(vm.tab_active, 1);
        let doc_b = vm.doc_id;
        // gt / gT
        w.feed("gt");
        let vm = w.view_model();
        assert_eq!((vm.tab_active, vm.name.as_str()), (0, "alfa.rs"));
        assert_ne!(vm.doc_id, doc_b, "outro documento: a cena recomeça");
        w.feed("gT");
        assert_eq!(w.view_model().name, "beta.rs");
        // o buscador abre numa aba nova; o mesmo arquivo de novo só troca para ela
        w.feed(" ff");
        w.feed("LEIA<CR>");
        assert_eq!(w.n_abas(), 3);
        assert_eq!(w.view_model().name, "LEIA.md");
        w.feed(" ffalfa<CR>");
        assert_eq!(w.n_abas(), 3, "alfa.rs já estava aberto");
        assert_eq!(w.view_model().name, "alfa.rs");
        // alteração não salva protege o :q; :q! fecha a aba (as outras continuam)
        w.feed("x:q<CR>");
        assert_eq!(w.n_abas(), 3);
        assert!(w.view_model().toast.is_some_and(|t| t.2 == ToastKind::Error));
        w.feed(":tabc!<CR>");
        assert_eq!(w.n_abas(), 2);
        assert!(!w.sair);
        // :q na aba limpa fecha só ela; na última, sai
        w.feed(":q<CR>");
        assert_eq!(w.n_abas(), 1);
        w.feed(":q<CR>");
        assert!(w.sair);
    }

    #[test]
    fn esc_fecha_e_espaco_sozinho_continua_valendo_como_movimento() {
        let mut s = Session::new("abc def\n", None);
        s.resize(80, 20.0);
        s.feed(" ff<Esc>");
        assert!(modelo(&mut s).picker.is_none());
        s.feed(" x");
        // <Espaço> move uma coluna e `x` apaga o `b`: não é atalho, volta ao editor
        assert_eq!(s.ed.lines()[0], "ac def");
    }

    #[test]
    fn setas_navegam_e_nunca_viram_texto() {
        let mut s = Session::new("abc\ndef\n", None);
        s.resize(80, 20.0);
        s.feed("<Down><Right>");
        assert_eq!(modelo(&mut s).cursor, (1, 1));
        s.feed("i<Left><Up>X<F5><Esc>");
        let linhas = s.ed.lines();
        assert_eq!(linhas[0], "Xabc", "{linhas:?}");
        assert_eq!(linhas[1], "def");
    }

    #[test]
    fn publica_texto_cursor_e_modo() {
        let mut s = Session::new("hello\nworld\n", Some(PathBuf::from("a.txt")));
        s.resize(40, 8.0);
        let m = modelo(&mut s);
        assert_eq!(m.line(0), "hello");
        assert_eq!(m.line(1), "world");
        assert_eq!(m.cursor, (0, 0));
        assert_eq!(m.mode, ModeKind::Normal);
        assert_eq!(m.name, "a.txt");
        assert!(!m.modified);
    }

    #[test]
    fn edita_com_o_motor_e_marca_modificado() {
        let mut s = Session::new("one two\nthree\n", Some(PathBuf::from("a.txt")));
        s.resize(40, 8.0);
        s.feed("dwjA!<Esc>");
        let m = modelo(&mut s);
        assert_eq!(m.line(0), "two");
        assert_eq!(m.line(1), "three!");
        assert!(m.modified);
        s.feed("u");
        assert_eq!(modelo(&mut s).line(1), "three");
    }

    #[test]
    fn comando_dois_pontos_grava_e_sai() {
        let dir = std::env::temp_dir().join(format!("astrograph-native-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.txt");
        let mut s = Session::new("abc\n", Some(f.clone()));
        s.resize(40, 6.0);
        s.feed("xx:q<CR>");
        assert!(!s.quit, "não sai com alterações");
        let m = modelo(&mut s);
        assert!(m.toast.as_ref().is_some_and(|t| t.2 == ToastKind::Error), "avisa por toast");
        s.feed(":w<CR>");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "c\n");
        assert!(!s.modified());
        assert!(modelo(&mut s).toast.as_ref().is_some_and(|t| t.2 == ToastKind::Done));
        s.feed(":q<CR>");
        assert!(s.quit);
    }

    #[test]
    fn linha_de_comando_e_busca_no_modelo() {
        let mut s = Session::new("abc def\n", None);
        s.resize(40, 6.0);
        s.feed(":w");
        let m = modelo(&mut s);
        assert_eq!(m.mode, ModeKind::Command);
        assert_eq!(m.cmd, Some((':', "w".to_string())));
        s.feed("<Esc>/de");
        let m = modelo(&mut s);
        assert_eq!(m.mode, ModeKind::Search);
        assert_eq!(m.cmd, Some(('/', "de".to_string())));
        s.feed("<CR>");
        let m = modelo(&mut s);
        assert_eq!(m.cursor, (0, 4));
        assert!(m.cmd.is_none());
    }

    #[test]
    fn visual_tem_selecao_e_modo() {
        let mut s = Session::new("abc def\n", None);
        s.resize(40, 6.0);
        s.feed("vee");
        let m = modelo(&mut s);
        assert_eq!(m.mode, ModeKind::Visual);
        assert_eq!(m.selection, Some((VKind::Char, (0, 0), (0, 6))));
        s.feed("<Esc>V");
        assert_eq!(modelo(&mut s).mode, ModeKind::VisualLine);
    }

    #[test]
    fn realce_de_sintaxe_vira_spans() {
        let mut s = Session::new("local x = 1 -- ok\n", Some(PathBuf::from("a.lua")));
        s.resize(40, 6.0);
        let m = modelo(&mut s);
        let sp = m.spans_of(0);
        assert!(sp.iter().any(|x| x.cls == crate::design::Cls::Keyword && x.start == 0 && x.end == 5), "local é palavra-chave: {sp:?}");
        assert!(sp.iter().any(|x| x.cls == crate::design::Cls::Comment));
    }

    #[test]
    fn rolagem_segue_o_cursor_e_a_roda_move_a_vista() {
        let linhas: Vec<String> = (1..=100).map(|i| format!("linha {i}")).collect();
        let mut s = Session::new(&linhas.join("\n"), None);
        s.resize(40, 10.0);
        s.feed("30G");
        let m = modelo(&mut s);
        assert!(m.top > 15.0 && m.top < 30.0, "cursor à vista: top={}", m.top);
        let antes = m.top;
        s.scroll(-5.0);
        assert_eq!(modelo(&mut s).top, antes - 5.0 + 0.0 * 1.0 - 0.0, "a roda sobe 5 linhas");
        s.feed("gg");
        assert_eq!(modelo(&mut s).top, 0.0);
    }

    #[test]
    fn clique_leva_o_cursor_a_linha_e_coluna_visual() {
        let mut s = Session::new("\tabc\nxyz\n", None);
        s.resize(40, 6.0);
        s.click(0, 9);
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (0, 2));
        s.click(1, 50);
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (1, 2));
    }
}
