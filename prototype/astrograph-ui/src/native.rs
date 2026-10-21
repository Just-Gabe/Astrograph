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
use crate::nview::{SigView, CompItem, CompView, GitView, PickItem, PickerView, Shared, Span, TermView, ToastKind, TreeItem, TreeView, ViewModel};
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
    /// trocar o agente / o modo do agente / retomar uma conversa do Claude Code
    Agente(&'static str),
    Modo(&'static str),
    Sessao { id: String, pasta: PathBuf },
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
    SimbolosDoProjeto,
}

struct Pendente {
    id: i64,
    tipo: PedidoLsp,
    desde: std::time::Instant,
    /// a resposta não interessa mais (a assinatura fechou antes de ela chegar)
    descartar: bool,
}

/// Quanto esperar o servidor antes de gravar mesmo sem a formatação.
const FORMATAR_ESPERA_MS: u128 = 2500;

/// Quanto esperar o agente abrir antes de entregar um pedido pronto.
const AGENTE_ESPERA_MS: u128 = 1200;

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
    /// lista de arquivos sendo montada em segundo plano
    listando: Option<busca::Bg<String>>,
    /// busca de texto em andamento (uma tecla nova solta esta e começa outra)
    buscando: Option<busca::Bg<busca::Achado>>,
    /// a base veio do cache: os lotes parciais não entram, só a lista nova inteira
    base_do_cache: bool,
    /// consulta anterior e os itens que casaram (a próxima letra só olha esses)
    prev: Option<(String, Vec<usize>)>,
    /// apertou Enter antes de chegar resultado: aceita o primeiro quando chegar
    aceitar_depois: bool,
}

const PICK_LINHAS: usize = 10;
/// linhas do trecho mostrado ao lado do buscador
const PICK_PREVIEW: usize = 12;

struct Comp {
    start: Pos,
    items: Vec<lsp::Item>,
    /// os itens filtrados: (índice em `items`, posições que casaram)
    shown: Vec<(usize, Vec<usize>)>,
    sel: usize,
}

const COMP_LINHAS: usize = 8;
/// quanto esperar depois da líder para mostrar o menu de atalhos
const LIDER_MENU_MS: u64 = 450;

/// Paradas do snippet que acabou de entrar: (início, fim) de cada uma, a atual e se o valor padrão
/// ainda está selecionado.
struct Snip {
    stops: Vec<(Pos, Pos)>,
    idx: usize,
    sel: bool,
}

/// Um servidor de linguagem compartilhado por todas as abas.
pub type LspRef = std::sync::Arc<std::sync::Mutex<Cliente>>;
pub type LspPool = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, LspRef>>>;

/// Pedidos que a sessão faz ao espaço de trabalho (abas).
#[derive(Clone, Debug, PartialEq)]
pub enum PedidoAba {
    Nova(Option<PathBuf>),
    /// aba sem arquivo com um texto pronto (a resposta da IA)
    NovaComTexto(String, String),
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

/// O que a IA vai fazer com o pedido digitado.
#[derive(Clone, Debug)]
enum IaTipo {
    /// trocar o trecho `[ini, fim)` (o buffer tinha a revisão `rev`)
    Editar { ini: Pos, fim: Pos, codigo: String, rev: u64 },
    /// responder numa aba nova (com o trecho selecionado como contexto, se houver)
    Perguntar { trecho: Option<String> },
}

struct IaJob {
    rx: std::sync::mpsc::Receiver<Result<String, String>>,
    tipo: IaTipo,
}

/// Pergunta aberta no explorador.
#[derive(Clone, Debug)]
enum TreeOp {
    Novo(std::path::PathBuf),
    Renomear(std::path::PathBuf),
    Apagar(std::path::PathBuf),
}

impl TreeOp {
    fn rotulo(&self) -> String {
        match self {
            TreeOp::Novo(d) => format!("Novo em {}/ (termine com / para pasta): ", d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            TreeOp::Renomear(_) => "Renomear para: ".into(),
            TreeOp::Apagar(p) => format!("Apagar {}? (s/n): ", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        }
    }
}

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
    /// dobras automáticas: o método e a revisão do texto para a qual foram calculadas
    fold_metodo: crate::dobras::Metodo,
    fold_rev: u64,
    /// quebra de linha (`:set wrap`) e o cache de "há alguma linha mais larga que a tela" (revisão, largura, resposta)
    wrap: bool,
    linebreak: bool,
    larga_cache: std::cell::Cell<(u64, usize, bool)>,
    /// projeção das dobras fechadas (cache por revisão e dobras)
    proj: std::cell::RefCell<Option<std::sync::Arc<crate::dobras::Proj>>>,
    /// quando a líder foi apertada (o menu de atalhos aparece depois de um tempo)
    leader_desde: Option<std::time::Instant>,
    /// pedido à IA sendo digitado e o que está rodando (`<Líder>AE`)
    ia_prompt: Option<(IaTipo, String)>,
    ia_job: Option<IaJob>,
    /// teste: programa e argumentos no lugar do agente (o pedido entra como último argumento)
    ia_cmd: Option<(String, Vec<String>)>,
    /// texto para uma aba nova (resposta da IA) e o nome fixo desta sessão
    aba_texto: Option<(String, String)>,
    /// `:Astrograph atualizar` em andamento
    atualizando: Option<std::sync::mpsc::Receiver<Result<crate::comandos::Resultado, String>>>,
    nome_fixo: Option<String>,
    /// estado de git de cada arquivo (decoração do explorador) e a leitura em andamento
    tree_git: std::collections::HashMap<PathBuf, char>,
    tree_git_rx: Option<std::sync::mpsc::Receiver<std::collections::HashMap<PathBuf, char>>>,
    /// snippet em andamento (Tab pula de parada em parada)
    snip: Option<Snip>,
    /// `.editorconfig`: limpar o espaço do fim das linhas e o que fazer com a última quebra ao gravar
    ec_trim: bool,
    ec_final_nl: Option<bool>,
    /// pergunta aberta no explorador (`a`, `r`, `d`) e o que foi digitado
    tree_prompt: Option<(TreeOp, String)>,
    /// `y`/`x` no explorador: o que `p` cola e se é para mover
    tree_clip: Option<(std::path::PathBuf, bool)>,
    term: Option<Term>,
    term_focus: bool,
    term_zen: bool,
    /// seleção do terminal com o mouse (pontas em linha, coluna da tela) e se o botão está apertado
    term_sel: Option<((usize, usize), (usize, usize))>,
    term_arrasto: bool,
    /// último texto copiado para o clipboard do sistema (id, texto)
    clip: Option<(u64, String)>,
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
    git_base: Option<std::sync::Arc<Vec<String>>>,
    /// branch e base do `HEAD` sendo lidos em segundo plano (o `git` é um processo: não trava a tela)
    git_rx: Option<std::sync::mpsc::Receiver<(Option<String>, Option<Vec<String>>)>>,
    /// sinais de um arquivo grande sendo calculados em segundo plano: (rev do texto, resultado)
    sinais_job: Option<(u64, std::sync::mpsc::Receiver<Vec<(usize, git::Sign)>>)>,
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
    /// o terminal aberto é o de um agente (os pedidos prontos vão para ele)
    term_agente: bool,
    /// outros terminais abertos (o atual fica em `term`): (terminal, é agente)
    term_fundo: Vec<(Term, bool)>,
    /// pedido pronto esperando o agente terminar de abrir: (desde quando, texto)
    agente_fila: Option<(std::time::Instant, String)>,
    agente_fila_enter: bool,
    /// só nos testes: pasta do `config.json` e agentes tidos como instalados
    agentes_pasta: Option<PathBuf>,
    agentes_tem: Option<Vec<&'static str>>,
    /// arquivo no disco: última modificação vista e quando se olhou (para recarregar o que o agente editou)
    mtime: Option<std::time::SystemTime>,
    mtime_olhado: std::time::Instant,
    /// mouse: último clique (instante, linha, coluna visual, quantos seguidos) e se o botão esquerdo está apertado
    clique: Option<(std::time::Instant, usize, usize, u8)>,
    mouse_baixo: bool,
    /// `:set number relativenumber scrolloff`
    numero: bool,
    relativo: bool,
    scrolloff: f32,
    /// pedidos ao servidor de linguagem esperando resposta
    lsp_pend: Vec<Pendente>,
    /// trocas de texto do servidor (renomear, ações) que o espaço de trabalho aplica em todos os arquivos
    pub edicoes_pedido: Option<Vec<lsp::Edicao>>,
    /// assinatura da função em edição (aberta ao digitar `(` ou `,`)
    sig: Option<(lsp::Assinatura, usize)>,
    /// pede ao servidor para formatar antes de gravar (`[lsp] format_on_save`)
    pub formatar_ao_salvar: bool,
    /// apertou <C-w> e espera a segunda tecla
    jan_espera: bool,
    /// apertou `]` ou `[` e espera o `d` (diagnóstico seguinte/anterior)
    colchete_espera: Option<char>,
    /// identifica o painel dentro da aba
    pub pane_id: PaneId,
    pub splits: Option<Box<Splits>>,
    /// outro painel da aba mostra o mesmo arquivo (o buffer é compartilhado)
    pub irmao: bool,
}

/// arquivos até esta quantidade de linhas têm os sinais do git calculados na hora; maiores, em segundo plano
const SINAIS_SINCRONOS: usize = 5000;

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
            fold_metodo: crate::dobras::METODO_INICIAL.get().copied().unwrap_or_default(),
            fold_rev: u64::MAX,
            wrap: crate::dobras::WRAP_INICIAL.get().copied().unwrap_or(false),
            linebreak: crate::dobras::LINEBREAK_INICIAL.get().copied().unwrap_or(false),
            larga_cache: std::cell::Cell::new((u64::MAX, 0, false)),
            proj: std::cell::RefCell::new(None),
            leader_desde: None,
            ia_prompt: None,
            ia_job: None,
            ia_cmd: None,
            aba_texto: None,
            atualizando: None,
            nome_fixo: None,
            tree_git: Default::default(),
            tree_git_rx: None,
            snip: None,
            ec_trim: false,
            ec_final_nl: None,
            tree_prompt: None,
            tree_clip: None,
            term: None,
            term_focus: false,
            term_zen: false,
            term_sel: None,
            term_arrasto: false,
            clip: None,
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
            git_rx: None,
            sinais_job: None,
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
            term_agente: false,
            term_fundo: Vec::new(),
            agente_fila: None,
            agente_fila_enter: true,
            agentes_pasta: None,
            agentes_tem: None,
            mtime: None,
            mtime_olhado: std::time::Instant::now(),
            clique: None,
            mouse_baixo: false,
            numero: true,
            relativo: false,
            scrolloff: 3.0,
            lsp_pend: Vec::new(),
            edicoes_pedido: None,
            sig: None,
            formatar_ao_salvar: false,
            jan_espera: false,
            colchete_espera: None,
            pane_id: 0,
            splits: None,
            irmao: false,
        }
    }

    /// Define a tecla líder (as menções a ela mostram o nome da tecla escolhida).
    pub fn set_leader(&mut self, c: char) {
        self.leader_key = c;
        self.ed.set_leader(c);
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
        s.aplicar_editorconfig(path);
        if let Some((r, c)) = busca::cursor_guardado(path).filter(|_| path.is_file()) {
            s.ed.set_cursor(r.min(s.ed.buf.line_count().saturating_sub(1)), c);
        }
        s.git_load();
        s.mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if !path.exists() {
            s.say(format!("\"{}\" [novo]", path.display()), ToastKind::Info);
        }
        s
    }

    /// Fecha o app guardando onde o cursor estava no arquivo.
    fn sair(&mut self) {
        self.guardar_cursor();
        self.quit = true;
    }

    fn guardar_cursor(&self) {
        if let Some(p) = self.path.as_ref().filter(|p| p.is_file()) {
            busca::guardar_cursor(p, self.ed.cursor.row, self.ed.cursor.col);
        }
    }

    /// Recuo e limpeza ao gravar do `.editorconfig` que vale para o arquivo.
    fn aplicar_editorconfig(&mut self, path: &Path) {
        let o = crate::editorconfig::para(path);
        let mut set = String::new();
        if let Some(e) = o.expandtab {
            set.push_str(if e { " expandtab" } else { " noexpandtab" });
        }
        if let Some(n) = o.shiftwidth {
            set.push_str(&format!(" shiftwidth={n}"));
        }
        if let Some(n) = o.tabstop {
            set.push_str(&format!(" tabstop={n}"));
        }
        if !set.is_empty() {
            self.ed.set_option(&set);
        }
        self.ec_trim = o.trim == Some(true);
        self.ec_final_nl = o.final_newline;
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
        if self.ed.cmdline().is_some() {
            // histórico da linha de comando
            match n {
                Nav::Up => self.ed.feed(&[Key::Ctrl('p')]),
                Nav::Down => self.ed.feed(&[Key::Ctrl('n')]),
                _ => {}
            }
            return;
        }
        if self.ed.has_pending() {
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
        // <Espaço> é o líder: `ff` arquivos, `fw` texto, `fo` recentes (no visual também, para `<Líder>Ai` na seleção)
        let livre_visual = self.ed.cmdline().is_none() && !self.ed.has_pending() && self.ed.mode() == Mode::Visual;
        if (livre || livre_visual) && self.leader.is_empty() && k == Key::Char(self.leader_key) {
            self.leader.push(k);
            self.leader_desde = Some(std::time::Instant::now());
            // acorda a tela quando chegar a hora de mostrar o menu de atalhos
            if let Some(n) = self.notify.clone() {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(LIDER_MENU_MS + 30));
                    n();
                });
            }
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
                [Key::Char('N')] => self.term_novo(),
                [Key::Char(']')] => self.term_ciclar(true),
                [Key::Char('[')] => self.term_ciclar(false),
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
                [Key::Char('l'), Key::Char('S')] => {
                    // abre a linha de comando já com o comando, para digitar o nome do símbolo
                    self.leader.clear();
                    self.ed.feed_str(":WorkspaceSymbol ");
                    return;
                }
                [Key::Char('l'), Key::Char('i')] => self.lsp_consulta(PedidoLsp::Implementacao),
                [Key::Char('l'), Key::Char('y')] => self.lsp_consulta(PedidoLsp::TipoDaDefinicao),
                [Key::Char('l'), Key::Char('r')] => self.lsp_consulta(PedidoLsp::Referencias),
                [Key::Char('x'), Key::Char('x')] => self.diag_lista(),
                [Key::Char('A'), Key::Char('l')] => self.open_agent(),
                [Key::Char('A'), Key::Char('g')] => self.agente_escolher(),
                [Key::Char('A'), Key::Char('w')] => self.agente_modo(),
                [Key::Char('A'), Key::Char('S')] => self.agente_sessoes(),
                [Key::Char('A'), Key::Char('x')] => self.agente_corrigir(),
                [Key::Char('A'), Key::Char('E')] => self.ia_abrir(),
                [Key::Char('A'), Key::Char('c')] => self.agente_citar(),
                [Key::Char('A'), Key::Char('i')] => self.agente_explicar(),
                [Key::Char('A'), Key::Char('r')] => self.agente_enviar(crate::agentes::pedido_revisar()),
                [Key::Char('A'), Key::Char('m')] => self.agente_enviar(crate::agentes::pedido_commit()),
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
        if self.ia_prompt.is_some() {
            return self.ia_prompt_key(k);
        }
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
        // `]d` / `[d`: o núcleo não tem esses prefixos; a sessão guarda o colchete e repassa se não vier `d`
        if let Some(c) = self.colchete_espera.take() {
            if k == Key::Char('d') {
                return self.diag_pular(c == ']');
            }
            self.ed.feed(&[Key::Char(c)]);
        } else if livre && matches!(k, Key::Char(']' | '[')) && self.picker.is_none() {
            if let Key::Char(c) = k {
                self.colchete_espera = Some(c);
            }
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
                    // última conversa do Claude Code (no projeto atual, se houver; senão a mais recente)
                    let lista = crate::agentes::pasta_do_claude().map(|b| crate::agentes::sessoes_claude(&b)).unwrap_or_default();
                    let alvo = lista.iter().find(|s| s.pasta == self.root).or_else(|| lista.first()).cloned();
                    match alvo {
                        Some(s) => self.retomar_sessao(&s.id, &s.pasta),
                        None => self.say("Nenhuma conversa anterior encontrada.".into(), ToastKind::Info),
                    }
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
        let raiz = lsp::raiz_do_projeto(&path, &self.root);
        // um servidor por programa e por projeto: arquivos de repositórios diferentes não se misturam
        let chave = format!("{prog}\0{}", raiz.display());
        let cli = {
            let mut pool = self.lsp_pool.lock().unwrap();
            match pool.get(&chave) {
                Some(c) => Some(c.clone()),
                None => {
                    let notify = self.notify.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
                    match Cliente::iniciar(&prog, &args, &raiz, notify) {
                        Ok(c) => {
                            let r: LspRef = std::sync::Arc::new(std::sync::Mutex::new(c));
                            pool.insert(chave.clone(), r.clone());
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
            self.sig_fechar();
            return;
        }
        match k {
            Key::Char('(') | Key::Char(',') if self.lsp.is_some() => self.lsp_assinatura(),
            Key::Char(')') | Key::Enter => self.sig_fechar(),
            _ => {}
        }
    }

    /// Fecha a assinatura e descarta as respostas que ainda vêm a caminho.
    fn sig_fechar(&mut self) {
        self.sig = None;
        for p in self.lsp_pend.iter_mut().filter(|p| p.tipo == PedidoLsp::Assinatura) {
            p.descartar = true;
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

    /// O "pedaço sem espaço" sob o cursor (caminho ou URL), sem pontuação nas pontas.
    fn token_sob_cursor(&self) -> String {
        let linha: Vec<char> = self.ed.buf.line(self.ed.cursor.row).chars().collect();
        if linha.is_empty() {
            return String::new();
        }
        let c = self.ed.cursor.col.min(linha.len() - 1);
        let sep = |c: char| c.is_whitespace() || "\"'`()[]{}<>,;|".contains(c);
        if sep(linha[c]) {
            return String::new();
        }
        let (mut a, mut b) = (c, c);
        while a > 0 && !sep(linha[a - 1]) {
            a -= 1;
        }
        while b + 1 < linha.len() && !sep(linha[b + 1]) {
            b += 1;
        }
        let t: String = linha[a..=b].iter().collect();
        t.trim_end_matches(['.', ':', '!', '?']).to_string()
    }

    /// `gf`: abre o arquivo cujo nome está sob o cursor (`caminho`, `caminho:linha` ou `caminho:linha:col`).
    fn ir_para_arquivo(&mut self) {
        let t = self.token_sob_cursor();
        if t.is_empty() {
            self.say("E446: Nenhum nome de arquivo sob o cursor".into(), ToastKind::Error);
            return;
        }
        // caminho:linha[:col]
        let mut partes: Vec<&str> = t.split(':').collect();
        let mut pos: Vec<usize> = Vec::new();
        while partes.len() > 1 && partes.last().is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) && pos.len() < 2 {
            pos.insert(0, partes.pop().unwrap().parse().unwrap_or(1));
        }
        let nome = partes.join(":");
        let nome = nome.strip_prefix("file://").unwrap_or(&nome).to_string();
        let base_arq = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
        let cands: Vec<PathBuf> = [base_arq, Some(self.root.clone())]
            .into_iter()
            .flatten()
            .map(|d| if let Some(rest) = nome.strip_prefix("~/") { std::env::var_os("HOME").map(PathBuf::from).unwrap_or(d).join(rest) } else { d.join(&nome) })
            .collect();
        let Some(alvo) = cands.into_iter().find(|p| p.is_file()) else {
            self.say(format!("E447: Arquivo \"{nome}\" não encontrado"), ToastKind::Error);
            return;
        };
        let goto = pos.first().map(|l| (l.saturating_sub(1), pos.get(1).map_or(0, |c| c.saturating_sub(1))));
        self.open_file(&alvo, goto);
    }

    /// `gx`: abre a URL (ou arquivo) sob o cursor com o programa padrão do sistema.
    fn abrir_com_o_sistema(&mut self) {
        let t = self.token_sob_cursor();
        if t.is_empty() {
            self.say("Nada sob o cursor para abrir.".into(), ToastKind::Info);
            return;
        }
        let (prog, args): (&str, Vec<String>) = if cfg!(windows) {
            ("cmd", vec!["/C".into(), "start".into(), String::new(), t.clone()])
        } else if cfg!(target_os = "macos") {
            ("open", vec![t.clone()])
        } else {
            ("xdg-open", vec![t.clone()])
        };
        match std::process::Command::new(prog).args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            Ok(_) => self.say(format!("Abrindo {t}"), ToastKind::Info),
            Err(e) => self.say(format!("Não consegui abrir: {e}"), ToastKind::Error),
        }
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
        let (texto, paradas, extras) = (item.texto.clone(), item.paradas.clone(), item.extras.clone());
        self.ed.insert_replace(de, &texto);
        self.comp_req = None;
        // posição de cada parada do snippet, a partir do começo do texto inserido
        let pos_de = |off: usize| -> Pos {
            let antes: Vec<char> = texto.chars().take(off).collect();
            match antes.iter().rposition(|&c| c == '\n') {
                None => Pos::new(de.row, de.col + off),
                Some(i) => Pos::new(de.row + antes.iter().filter(|&&c| c == '\n').count(), off - i - 1),
            }
        };
        let mut stops: Vec<(Pos, Pos)> = paradas.iter().map(|&(a, b)| (pos_de(a), pos_de(b))).collect();
        // imports automáticos: entram acima e empurram o que vem depois
        if !extras.is_empty() {
            let n = self.ed.buf.line_count();
            let v: Vec<(Pos, Pos, String)> = extras
                .iter()
                .map(|((l0, c0), (l1, c1), t)| {
                    let (l0, l1) = ((*l0).min(n - 1), (*l1).min(n - 1));
                    (Pos::new(l0, lsp::col_chars(&self.ed.buf.line(l0), *c0)), Pos::new(l1, lsp::col_chars(&self.ed.buf.line(l1), *c1)), t.clone())
                })
                .collect();
            let d = self.ed.editar_na_insercao(&v);
            for (a, b) in stops.iter_mut() {
                a.row = (a.row as i64 + d).max(0) as usize;
                b.row = (b.row as i64 + d).max(0) as usize;
            }
        }
        self.comp_block = self.ed.buf.rev;
        if !stops.is_empty() && stops.iter().any(|(a, b)| a != b || *a != self.ed.cursor) {
            self.snip = Some(Snip { stops, idx: 0, sel: false });
            self.snip_ir(0);
        } else {
            self.snip = None;
        }
    }

    /// Vai para a parada `idx` do snippet (o valor padrão fica selecionado: a primeira letra o troca).
    fn snip_ir(&mut self, idx: usize) {
        let Some(sn) = self.snip.as_mut() else { return };
        let Some(&(a, b)) = sn.stops.get(idx) else {
            self.snip = None;
            return;
        };
        sn.idx = idx;
        sn.sel = a != b && a.row == b.row;
        let n = self.ed.buf.line_count();
        if a.row >= n {
            self.snip = None;
            return;
        }
        self.ed.cursor = Pos::new(a.row, a.col.min(self.ed.buf.line_len(a.row)));
        // a última parada (`$0`) encerra o modo
        if idx + 1 == sn.stops.len() && !sn.sel {
            self.snip = None;
        }
    }

    /// Teclas no modo snippet (inserção com paradas). `true` se a tecla foi consumida.
    fn snip_tecla(&mut self, k: Key) -> bool {
        let Some(sn) = self.snip.as_ref() else { return false };
        let (idx, sel) = (sn.idx, sn.sel);
        match k {
            Key::Tab => {
                self.snip_ir(idx + 1);
                return true;
            }
            Key::Esc | Key::Enter | Key::Ctrl('c') => {
                self.snip = None;
                return false;
            }
            Key::Char(_) | Key::Backspace | Key::Delete if sel => {
                // o valor padrão é trocado pelo que vier
                let (a, b) = sn.stops[idx];
                self.ed.buf.delete(a, b);
                self.ed.cursor = a;
                let d = b.col as i64 - a.col as i64;
                let sn = self.snip.as_mut().unwrap();
                sn.sel = false;
                for (i, (x, y)) in sn.stops.iter_mut().enumerate() {
                    if i == idx {
                        y.col = a.col;
                    } else if x.row == a.row && x.col >= b.col {
                        x.col -= d as usize;
                        if y.row == a.row {
                            y.col -= d as usize;
                        }
                    }
                }
                return matches!(k, Key::Backspace | Key::Delete);
            }
            _ => {}
        }
        false
    }

    /// Depois de uma tecla de texto no modo snippet: as paradas da mesma linha andam junto.
    fn snip_ajustar(&mut self, antes: (usize, usize, usize)) {
        let Some(sn) = self.snip.as_mut() else { return };
        let (row, col, len) = antes;
        if self.ed.cursor.row != row || self.ed.buf.line_count() == 0 {
            self.snip = None;
            return;
        }
        let d = self.ed.buf.line_len(row) as i64 - len as i64;
        if d == 0 {
            return;
        }
        for (x, y) in sn.stops.iter_mut() {
            if x.row == row && x.col > col {
                x.col = (x.col as i64 + d).max(0) as usize;
            }
            if y.row == row && y.col >= col {
                y.col = (y.col as i64 + d).max(0) as usize;
            }
        }
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
        self.lsp_pend.push(Pendente { id, tipo, desde: std::time::Instant::now(), descartar: false });
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
        self.picker = Some(Picker { kind: PickKind::Lista, titulo: titulo.into(), lista: itens, mapa: Vec::new(), query: String::new(), total: base.len(), base, items: Vec::new(), achados: Vec::new(), sel: 0, listando: None, buscando: None, base_do_cache: false, prev: None, aceitar_depois: false });
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
                Some(_) if p.descartar => {}
                Some(v) => self.lsp_resposta(p.tipo, v),
                // formatar ao salvar: o servidor não respondeu a tempo, grava assim mesmo
                None if matches!(p.tipo, PedidoLsp::Formatar { .. }) && espera > FORMATAR_ESPERA_MS => {
                    if let PedidoLsp::Formatar { salvar: true, sair } = p.tipo {
                        self.gravar("");
                        self.git_load();
                        if sair && !self.modified() {
                            self.sair();
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
                        self.sair();
                    }
                }
            }
            PedidoLsp::Assinatura => self.sig = lsp::assinatura_de(&v).map(|a| (a, self.ed.cursor.row)),
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
            PedidoLsp::SimbolosDoProjeto => {
                let itens: Vec<ListaItem> = lsp::simbolos_de(&v)
                    .into_iter()
                    .filter_map(|s| {
                        let arq = s.arquivo?;
                        let rel = arq.strip_prefix(&self.root).map(|r| r.display().to_string()).unwrap_or_else(|_| arq.display().to_string());
                        Some(ListaItem { texto: format!("{} {}", tipo_simbolo(s.tipo), s.nome), detalhe: format!("{rel}:{}", s.row + 1), destino: Destino::Local(arq, s.row, s.col) })
                    })
                    .collect();
                if itens.is_empty() {
                    self.say("Nenhum símbolo encontrado.".into(), ToastKind::Info);
                } else {
                    self.pick_lista("Símbolos do projeto", itens);
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
        let caminho = self.path.clone();
        let notify = self.notify.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        // o `git` é um processo (dezenas de ms, mais no Windows): roda fora da thread do editor
        std::thread::spawn(move || {
            let branch = git::branch(&dir);
            let base = match (&branch, &caminho) {
                (Some(_), Some(p)) => git::base(p),
                _ => None,
            };
            let _ = tx.send((branch, base));
            if let Some(n) = notify {
                n();
            }
        });
        self.git_rx = Some(rx);
    }

    /// Objetos de texto de tree-sitter (`if`/`af` função, `ic`/`ac` classe, `ia`/`aa` parâmetro) com um
    /// operador (`d`, `c`, `y`, `<`, `>`) ou no visual. `true` se a tecla foi tratada aqui.
    fn objeto_sintaxe(&mut self, k: Key) -> bool {
        let Key::Char(obj @ ('f' | 'c' | 'a')) = k else { return false };
        let pend = self.ed.pendentes();
        let visual = self.ed.mode() == Mode::Visual;
        let (op, incl) = match (pend.as_slice(), visual) {
            ([Key::Char(i @ ('i' | 'a'))], true) => (None, *i == 'a'),
            ([.., Key::Char(op @ ('d' | 'c' | 'y' | '<' | '>')), Key::Char(i @ ('i' | 'a'))], false) => (Some(*op), *i == 'a'),
            _ => return false,
        };
        if self.ed.mode() == Mode::Insert || self.lang.is_none() {
            return false;
        }
        self.refresh_syntax();
        let Some((doc, _)) = self.doc.as_ref() else { return false };
        let nome = match obj {
            'f' => "function",
            'c' => "class",
            _ => "parameter",
        };
        let captura = format!("@{nome}.{}", if incl { "outer" } else { "inner" });
        if !doc.has_textobject(&captura) && !doc.has_textobject(&format!("@{nome}.outer")) {
            return false;
        }
        let (row, col) = (self.ed.cursor.row, self.ed.cursor.col);
        let linha = self.ed.buf.line(row);
        let bcol = linha.char_indices().nth(col).map_or(linha.len(), |(b, _)| b);
        let Some(r) = doc.textobject_at(&captura, row, bcol, true, false) else {
            self.ed.feed(&[Key::Esc]);
            return true;
        };
        // bytes -> caracteres
        let (n0, n1) = (self.ed.buf.line(r.srow), self.ed.buf.line(r.erow.min(self.ed.buf.line_count() - 1)));
        let ch = |l: &str, b: usize| l.char_indices().take_while(|(i, _)| *i < b).count();
        let ini = Pos::new(r.srow, ch(&n0, r.scol));
        let fim = Pos::new(r.erow.min(self.ed.buf.line_count() - 1), ch(&n1, r.ecol));
        match op {
            None => {
                // visual: do começo ao último caractere do trecho
                let ultimo = if fim.col > 0 { Pos::new(fim.row, fim.col - 1) } else if fim.row > ini.row { Pos::new(fim.row - 1, self.ed.buf.line_len(fim.row - 1).saturating_sub(1)) } else { fim };
                self.ed.selecionar(ini, ultimo);
            }
            Some(op) => {
                self.ed.operar_intervalo(op, ini, fim, false);
            }
        }
        true
    }

    /// Menu de atalhos: depois da líder, esperando um tempo, mostra o que cada tecla faz.
    fn menu_lider(&self) -> Vec<(String, String)> {
        if self.leader.is_empty() || self.leader_desde.is_none_or(|t| t.elapsed().as_millis() < LIDER_MENU_MS as u128) {
            return Vec::new();
        }
        let prefixo: String = self.leader[1..].iter().filter_map(|k| k.as_char()).collect();
        crate::atalhos::proximos(&prefixo)
    }

    /// Texto do trabalho que o servidor de linguagem está fazendo (indexando...), se houver.
    fn lsp_progresso(&self) -> Option<String> {
        let cl = self.lsp.as_ref()?;
        let c = cl.lock().ok()?;
        let st = c.estado.lock().ok()?;
        let mut v: Vec<String> = st.progresso.values().map(lsp::Progresso::texto).filter(|t| !t.is_empty()).collect();
        v.sort();
        v.first().map(|t| format!("{}: {t}", c.nome))
    }

    /// Lê o estado do git dos arquivos para o explorador (em segundo plano).
    fn tree_git_load(&mut self) {
        let Some(t) = self.tree.as_ref() else { return };
        let dir = t.raiz.clone();
        let notify = self.notify.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(git::estados(&dir));
            if let Some(n) = notify {
                n();
            }
        });
        self.tree_git_rx = Some(rx);
    }

    /// Pega o branch e a base que a thread do git acabou de ler.
    fn git_poll(&mut self) {
        if let Some(rx) = self.tree_git_rx.as_ref() {
            if let Ok(m) = rx.try_recv() {
                self.tree_git = m;
                self.tree_git_rx = None;
                self.seq += 1;
            }
        }
        let Some(rx) = self.git_rx.as_ref() else { return };
        if let Ok((branch, base)) = rx.try_recv() {
            self.git_branch = branch;
            self.git_base = base.map(std::sync::Arc::new);
            self.git_rev = u64::MAX;
            self.git_rx = None;
        }
    }

    /// Sinais da margem para o texto atual (recalculados só quando o texto muda).
    fn git_signs(&mut self) -> std::sync::Arc<Vec<(usize, git::Sign)>> {
        // resultado do cálculo em segundo plano, se chegou
        if let Some((_, rx)) = &self.sinais_job {
            if let Ok(v) = rx.try_recv() {
                self.git_signs = std::sync::Arc::new(v);
                self.sinais_job = None;
            }
        }
        if self.git_rev != self.ed.buf.rev && self.sinais_job.is_none() {
            self.git_rev = self.ed.buf.rev;
            match self.git_base.clone() {
                None => self.git_signs = std::sync::Arc::new(Vec::new()),
                // arquivo pequeno: o diff é instantâneo e os sinais acompanham cada tecla
                Some(base) if self.ed.buf.line_count() <= SINAIS_SINCRONOS => self.git_signs = std::sync::Arc::new(git::sinais(&base, &self.ed.lines())),
                // arquivo grande: o diff roda em outra thread; enquanto ele anda, os sinais antigos ficam
                Some(base) => {
                    let rope = self.ed.buf.rope.clone();
                    let notify = self.notify.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let linhas: Vec<String> = rope.lines().map(|l| l.to_string().trim_end_matches(['\n', '\r']).to_string()).collect();
                        let _ = tx.send(git::sinais(&base, &linhas));
                        if let Some(n) = notify {
                            n();
                        }
                    });
                    self.sinais_job = Some((self.ed.buf.rev, rx));
                }
            }
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
        let pasta = self.root.clone();
        self.term_spawn_em(cmd, &pasta);
    }

    fn term_spawn_em(&mut self, cmd: portable_pty::CommandBuilder, pasta: &Path) {
        let (rows, cols) = self.term_dims;
        let notify = self.notify.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
        self.term_agente = false;
        match Term::spawn(cmd, rows, cols, Some(pasta), notify) {
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

    /// `<Líder>N`: mais um terminal (o atual vai para o fundo e continua rodando).
    fn term_novo(&mut self) {
        if let Some(t) = self.term.take() {
            self.term_fundo.push((t, self.term_agente));
        }
        self.term_screen = None;
        self.term_sel = None;
        self.term_spawn(term::shell_padrao());
        let n = self.term_fundo.len() + 1;
        self.say(format!("Terminal {n} aberto"), ToastKind::Info);
    }

    /// `<Líder>]` / `<Líder>[`: próximo e anterior terminal.
    fn term_ciclar(&mut self, adiante: bool) {
        if self.term_fundo.is_empty() || self.term.is_none() {
            return;
        }
        let atual = (self.term.take().unwrap(), self.term_agente);
        let (t, ag) = if adiante { self.term_fundo.remove(0) } else { self.term_fundo.pop().unwrap() };
        if adiante {
            self.term_fundo.push(atual);
        } else {
            self.term_fundo.insert(0, atual);
        }
        self.term = Some(t);
        self.term_agente = ag;
        self.term_screen = None;
        self.term_sel = None;
        self.term_focus = true;
        self.term_esc = false;
        self.say(format!("{} terminais abertos", self.term_fundo.len() + 1), ToastKind::Info);
    }

    /// `<Líder>Al` / `a` na tela inicial: roda o primeiro agente instalado no terminal.
    fn open_agent(&mut self) {
        let root = self.root.clone();
        self.abrir_agente(&root, &[]);
    }

    fn pasta_agentes(&self) -> PathBuf {
        self.agentes_pasta.clone().unwrap_or_else(crate::agentes::pasta_config)
    }

    fn agente_instalado(&self, bin: &str) -> bool {
        match &self.agentes_tem {
            Some(v) => v.contains(&bin),
            None => crate::agentes::instalado(bin),
        }
    }

    fn agente_atual(&self) -> Option<&'static crate::agentes::Agente> {
        crate::agentes::atual_em(&self.pasta_agentes(), |b| self.agente_instalado(b))
    }

    /// Abre o agente escolhido, no modo escolhido, no terminal (em `pasta`, com `extra` argumentos).
    fn abrir_agente(&mut self, pasta: &Path, extra: &[&str]) -> bool {
        let Some(a) = self.agente_atual() else {
            self.say("Nenhum agente instalado (claude, opencode, codex ou gemini). Use <Líder>t para o terminal.".into(), ToastKind::Info);
            return false;
        };
        let modo = crate::agentes::modo_atual_em(&self.pasta_agentes());
        let (mut linha, exato) = crate::agentes::linha_de_comando(a, modo.id);
        if a.bin == "ollama" {
            match crate::agentes::modelo_ollama(&self.pasta_agentes()) {
                Some(m) => linha = format!("ollama run {m}"),
                None => {
                    self.say("Nenhum modelo do Ollama: rode `ollama pull llama3.2` ou defina ollama_model no config.json.".into(), ToastKind::Info);
                    return false;
                }
            }
        }
        let mut partes = linha.split_whitespace();
        let mut cmd = portable_pty::CommandBuilder::new(partes.next().unwrap_or(a.bin));
        for p in partes {
            cmd.arg(p);
        }
        for e in extra {
            cmd.arg(e);
        }
        self.term_close();
        self.term_spawn_em(cmd, pasta);
        if self.term.is_some() {
            self.term_agente = true;
            if !exato {
                self.say(format!("{} não tem o modo \"{}\": abriu no padrão dele.", a.label, modo.label), ToastKind::Info);
            }
        }
        self.term.is_some()
    }

    /// `<Líder>Ag`: lista os agentes instalados (o atual marcado); escolher vale para o terminal e fica salvo.
    fn agente_escolher(&mut self) {
        let instalados: Vec<&'static crate::agentes::Agente> = crate::agentes::AGENTES.iter().filter(|a| self.agente_instalado(a.bin)).collect();
        if instalados.is_empty() {
            self.say("Nenhum agente instalado. Instale com: npm i -g @anthropic-ai/claude-code (ou opencode-ai, @openai/codex, @google/gemini-cli).".into(), ToastKind::Info);
            return;
        }
        let atual = self.agente_atual().map(|a| a.bin);
        let itens = instalados
            .into_iter()
            .map(|a| ListaItem { texto: format!("{} {}", if atual == Some(a.bin) { "●" } else { " " }, a.label), detalhe: a.bin.to_string(), destino: Destino::Agente(a.bin) })
            .collect();
        self.pick_lista("Agente de IA", itens);
    }

    fn escolher_agente(&mut self, bin: &'static str) {
        let pasta = self.pasta_agentes();
        if crate::agentes::definir_atual(&pasta, bin) {
            let label = crate::agentes::AGENTES.iter().find(|a| a.bin == bin).map_or(bin, |a| a.label);
            self.say(format!("Agente: {label}"), ToastKind::Done);
        } else {
            self.say("Não consegui salvar a escolha (vale só até fechar).".into(), ToastKind::Error);
        }
    }

    /// `<Líder>Aw`: o modo de permissão do agente.
    fn agente_modo(&mut self) {
        let atual = crate::agentes::modo_atual_em(&self.pasta_agentes()).id;
        let itens = crate::agentes::MODOS
            .iter()
            .map(|m| ListaItem { texto: format!("{} {}", if atual == m.id { "●" } else { " " }, m.label), detalhe: m.desc.to_string(), destino: Destino::Modo(m.id) })
            .collect();
        self.pick_lista("Modo do agente", itens);
    }

    fn escolher_modo(&mut self, id: &'static str) {
        let pasta = self.pasta_agentes();
        if crate::agentes::definir_modo(&pasta, id) {
            let label = crate::agentes::MODOS.iter().find(|m| m.id == id).map_or(id, |m| m.label);
            self.say(format!("Modo do agente: {label} (vale da próxima vez que ele abrir)"), ToastKind::Done);
        }
    }

    /// `<Líder>AS`: conversas do Claude Code de todos os projetos, da mais recente.
    fn agente_sessoes(&mut self) {
        let lista = crate::agentes::pasta_do_claude().map(|b| crate::agentes::sessoes_claude(&b)).unwrap_or_default();
        if lista.is_empty() {
            self.say("Nenhuma conversa do Claude Code encontrada (~/.claude/projects).".into(), ToastKind::Info);
            return;
        }
        let itens = lista
            .into_iter()
            .take(300)
            .map(|s| {
                let pasta = s.pasta.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| s.pasta.display().to_string());
                ListaItem { texto: s.titulo, detalhe: format!("{pasta} · {}", crate::agentes::ha_quanto_tempo(s.quando)), destino: Destino::Sessao { id: s.id, pasta: s.pasta } }
            })
            .collect();
        self.pick_lista("Conversas do agente", itens);
    }

    /// Retoma uma conversa do Claude Code na pasta dela.
    fn retomar_sessao(&mut self, id: &str, pasta: &Path) {
        if !self.agente_instalado("claude") {
            self.say("O Claude Code não está instalado (npm i -g @anthropic-ai/claude-code).".into(), ToastKind::Info);
            return;
        }
        let mut cmd = portable_pty::CommandBuilder::new("claude");
        let modo = crate::agentes::modo_atual_em(&self.pasta_agentes());
        let (linha, _) = crate::agentes::linha_de_comando(&crate::agentes::AGENTES[0], modo.id);
        for p in linha.split_whitespace().skip(1) {
            cmd.arg(p);
        }
        cmd.arg("--resume");
        cmd.arg(id);
        self.term_close();
        self.term_spawn_em(cmd, pasta);
        self.term_agente = self.term.is_some();
    }

    /// Manda um pedido pronto ao agente: abre o agente se preciso (o texto espera ele terminar de subir).
    fn agente_enviar(&mut self, pedido: String) {
        self.agente_enviar_com(pedido, true);
    }

    /// `enter`: manda o texto e aperta Enter; senão só deixa o texto digitado no agente (citação).
    fn agente_enviar_com(&mut self, pedido: String, enter: bool) {
        if self.term.is_some() && self.term_agente {
            if let Some(t) = self.term.as_mut() {
                t.colar(&pedido);
                if enter {
                    t.write(b"\r");
                }
            }
            self.term_focus = true;
            return;
        }
        if self.abrir_agente(&self.root.clone(), &[]) {
            self.agente_fila = Some((std::time::Instant::now(), pedido));
            self.agente_fila_enter = enter;
        }
    }

    /// `<Líder>Ac`: cita no agente o arquivo (ou as linhas selecionadas) como `@arquivo#L3-9 `, sem enviar.
    fn agente_citar(&mut self) {
        let Some(arq) = self.path.clone() else {
            self.say("Salve o arquivo antes (o agente precisa do nome).".into(), ToastKind::Info);
            return;
        };
        let rel = relativo(&self.root, &arq.to_string_lossy());
        let ref_ = match self.ed.selection() {
            Some((_, anc)) => {
                let (a, b) = (anc.row.min(self.ed.cursor.row) + 1, anc.row.max(self.ed.cursor.row) + 1);
                self.ed.feed(&[Key::Esc]);
                if a == b { format!("@{rel}#L{a} ") } else { format!("@{rel}#L{a}-{b} ") }
            }
            None => format!("@{rel} "),
        };
        self.agente_enviar_com(ref_, false);
    }

    /// Entrega o pedido que esperava o agente abrir: quando ele já desenhou algo e passou um tempo.
    fn agente_fila_poll(&mut self) {
        let Some((desde, _)) = &self.agente_fila else { return };
        let pronto = desde.elapsed().as_millis() >= AGENTE_ESPERA_MS && self.term_screen.as_ref().is_some_and(|s| s.rows.iter().any(|r| r.iter().any(|c| c.ch != ' ')));
        if !pronto {
            return;
        }
        if let Some((_, pedido)) = self.agente_fila.take() {
            let enter = std::mem::replace(&mut self.agente_fila_enter, true);
            if let Some(t) = self.term.as_mut() {
                t.colar(&pedido);
                if enter {
                    t.write(b"\r");
                }
            }
        }
    }

    /// `<Líder>Ax`: pede ao agente para corrigir o diagnóstico da linha do cursor.
    fn agente_corrigir(&mut self) {
        let Some(arq) = self.path.clone() else {
            self.say("Salve o arquivo antes (o agente precisa do nome).".into(), ToastKind::Info);
            return;
        };
        let linha = self.ed.cursor.row;
        let Some(d) = self.lsp_diags.iter().find(|d| d.row <= linha && linha <= d.end_row).cloned() else {
            self.say("Sem diagnóstico nesta linha.".into(), ToastKind::Info);
            return;
        };
        let rel = relativo(&self.root, &arq.to_string_lossy());
        let grav = match d.severity {
            1 => "erro",
            2 => "aviso",
            _ => "problema",
        };
        self.agente_enviar(crate::agentes::pedido_corrigir(&rel, d.row + 1, grav, d.message.lines().next().unwrap_or("")));
    }

    /// `<Líder>Ai`: pede ao agente para explicar a seleção (ou a linha do cursor).
    fn agente_explicar(&mut self) {
        let Some(arq) = self.path.clone() else {
            self.say("Salve o arquivo antes (o agente precisa do nome).".into(), ToastKind::Info);
            return;
        };
        let (de, ate) = match self.ed.selection() {
            Some((_, anc)) => (anc.row.min(self.ed.cursor.row), anc.row.max(self.ed.cursor.row)),
            None => (self.ed.cursor.row, self.ed.cursor.row),
        };
        if self.ed.mode() == Mode::Visual {
            self.ed.feed(&[Key::Esc]);
        }
        let rel = relativo(&self.root, &arq.to_string_lossy());
        self.agente_enviar(crate::agentes::pedido_explicar(&rel, de + 1, ate + 1));
    }

    /// `:Astrograph <subcomando>`: o mesmo conjunto do Neovim (atualizar, saude, agentes, configurar, login,
    /// sessoes, atalhos, docs).
    fn comando_astrograph(&mut self, arg: &str) {
        let (sub, resto) = match arg.trim().split_once(char::is_whitespace) {
            Some((s, r)) => (s, r.trim()),
            None => (arg.trim(), ""),
        };
        match sub {
            "atualizar" | "update" => self.atualizar_astrograph(),
            "saude" | "health" => {
                let t = crate::comandos::saude("motor nativo", &lsp::servidores_instalados());
                self.aba_texto = Some(("Saúde do Astrograph".into(), t));
            }
            "atalhos" => {
                let t = crate::comandos::atalhos(&tecla_nome(self.leader_key));
                self.aba_texto = Some(("Atalhos".into(), t));
            }
            "agentes" | "agente" => self.agente_escolher(),
            "modo" => self.agente_modo(),
            "configurar" => self.agente_instalar(resto),
            "login" => self.agente_login(),
            "sessoes" => self.agente_sessoes(),
            "preparo" => {}
            "docs" => {
                let _ = std::process::Command::new(if cfg!(windows) { "cmd" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" })
                    .args(if cfg!(windows) { vec!["/C", "start", "", "https://just-gabe.github.io/Astrograph/"] } else { vec!["https://just-gabe.github.io/Astrograph/"] })
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                self.say("Abrindo a documentação no navegador".into(), ToastKind::Info);
            }
            "" => self.say("Uso: :Astrograph atualizar | saude | agentes | modo | configurar | login | sessoes | atalhos | docs".into(), ToastKind::Info),
            outro => self.say(format!("Subcomando desconhecido: {outro}. Use :Astrograph para ver a lista."), ToastKind::Error),
        }
    }

    /// `:Astrograph atualizar`: atualização transacional pelo `astrograph-servicos`, em segundo plano.
    fn atualizar_astrograph(&mut self) {
        if self.atualizando.is_some() {
            self.say("A atualização já está rodando.".into(), ToastKind::Info);
            return;
        }
        let Some(raiz) = crate::comandos::raiz_do_clone() else {
            self.say("Não achei a pasta do Astrograph (instalação fora do padrão): defina ASTROGRAPH_ROOT.".into(), ToastKind::Error);
            return;
        };
        let notify = self.notify.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::comandos::atualizar(&raiz));
            if let Some(n) = notify {
                n();
            }
        });
        self.atualizando = Some(rx);
        self.say("Atualizando o Astrograph em segundo plano…".into(), ToastKind::Info);
    }

    fn atualizar_poll(&mut self) {
        let Some(rx) = self.atualizando.as_ref() else { return };
        let Ok(r) = rx.try_recv() else { return };
        self.atualizando = None;
        match r {
            Ok(res) => {
                let msg = if res.status == "atualizado" { format!("{}\n\nReinicie o app para usar a versão nova.", res.mensagem) } else { res.mensagem.clone() };
                let tipo = match res.status.as_str() {
                    "atualizado" => ToastKind::Done,
                    "ja-atualizado" => ToastKind::Info,
                    _ => ToastKind::Error,
                };
                if msg.contains('\n') || msg.chars().count() > 110 {
                    self.aba_texto = Some(("Atualização do Astrograph".into(), msg));
                } else {
                    self.say(msg, tipo);
                }
            }
            Err(e) => self.say(format!("A atualização falhou: {e}"), ToastKind::Error),
        }
    }

    /// Abre um terminal e roda `comando` nele (instalar ou fazer login de um agente).
    fn terminal_rodando(&mut self, comando: &str) {
        self.term_spawn(term::shell_padrao());
        if let Some(t) = self.term.as_mut() {
            t.colar(comando);
            t.write(b"\r");
        }
    }

    /// `:AgentInstall [agente]`: instala o CLI do agente (o escolhido, se não disser qual) com o npm.
    fn agente_instalar(&mut self, arg: &str) {
        let alvo = match arg.trim() {
            "" => crate::agentes::ler_config(&self.pasta_agentes())["agent"].as_str().and_then(|b| crate::agentes::AGENTES.iter().find(|a| a.bin == b)).or_else(|| crate::agentes::AGENTES.iter().find(|a| !self.agente_instalado(a.bin))),
            b => crate::agentes::AGENTES.iter().find(|a| a.bin == b),
        };
        let Some(a) = alvo else {
            self.say("Uso: :AgentInstall claude|opencode|codex|gemini".into(), ToastKind::Info);
            return;
        };
        if self.agente_instalado(a.bin) {
            self.say(format!("{} já está instalado. Use :AgentLogin para entrar.", a.label), ToastKind::Info);
            return;
        }
        if !self.agente_instalado("npm") && !crate::agentes::instalado("npm") {
            self.say("Falta o npm (instale o Node.js) para instalar os agentes.".into(), ToastKind::Error);
            return;
        }
        self.terminal_rodando(&crate::agentes::comando_instalar(a));
        self.say(format!("Instalando {}… quando acabar, rode :AgentLogin", a.label), ToastKind::Info);
    }

    /// `:AgentLogin`: faz o login do agente escolhido no terminal.
    fn agente_login(&mut self) {
        let Some(a) = self.agente_atual() else {
            self.say("Nenhum agente instalado: use :AgentInstall.".into(), ToastKind::Info);
            return;
        };
        self.terminal_rodando(&crate::agentes::comando_login(a));
    }

    /// `<Líder>AE`: com uma seleção pede uma edição à IA (a proposta troca o trecho, `u` desfaz); sem ela,
    /// faz uma pergunta sobre o arquivo e a resposta abre numa aba.
    fn ia_abrir(&mut self) {
        if self.ia_job.is_some() {
            self.say("A IA ainda está trabalhando no pedido anterior.".into(), ToastKind::Info);
            return;
        }
        if self.agente_atual().is_none() && self.ia_cmd.is_none() {
            self.say("Nenhum agente instalado (claude, opencode, codex ou gemini).".into(), ToastKind::Info);
            return;
        }
        let sel = self.ed.selection().map(|(k, anc)| (k, anc, self.ed.cursor));
        let tipo = match sel {
            Some((k, anc, cur)) => {
                let (lo, hi) = if anc <= cur { (anc, cur) } else { (cur, anc) };
                let (ini, fim) = if k == VKind::Char {
                    (lo, Pos::new(hi.row, (hi.col + 1).min(self.ed.buf.line_len(hi.row))))
                } else {
                    (Pos::new(lo.row, 0), Pos::new(hi.row, self.ed.buf.line_len(hi.row)))
                };
                let codigo = (ini.row..=fim.row)
                    .map(|r| {
                        let l = self.ed.buf.line(r);
                        let a = if r == ini.row { ini.col } else { 0 };
                        let b = if r == fim.row { fim.col } else { l.chars().count() };
                        l.chars().skip(a).take(b.saturating_sub(a)).collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.ed.feed(&[Key::Esc]);
                IaTipo::Editar { ini, fim, codigo, rev: self.ed.buf.rev }
            }
            None => IaTipo::Perguntar { trecho: None },
        };
        self.ia_prompt = Some((tipo, String::new()));
    }

    fn ia_prompt_key(&mut self, k: Key) {
        let Some((tipo, mut texto)) = self.ia_prompt.take() else { return };
        match k {
            Key::Esc | Key::Ctrl('c') => return,
            Key::Backspace => {
                texto.pop();
            }
            Key::Char(c) => texto.push(c),
            Key::Enter => return self.ia_enviar(tipo, texto),
            _ => {}
        }
        self.ia_prompt = Some((tipo, texto));
    }

    fn ia_enviar(&mut self, tipo: IaTipo, instrucao: String) {
        let instrucao = instrucao.trim().to_string();
        if instrucao.is_empty() {
            return;
        }
        let arq = self.path.as_ref().map(|p| relativo(&self.root, &p.to_string_lossy())).unwrap_or_else(|| "(sem nome)".into());
        let pedido = match &tipo {
            IaTipo::Editar { codigo, .. } => crate::agentes::pedido_editar(&arq, &instrucao, codigo),
            IaTipo::Perguntar { trecho } => crate::agentes::pedido_perguntar(&arq, &instrucao, trecho.as_deref()),
        };
        let (prog, mut args) = match (&self.ia_cmd, self.agente_atual()) {
            (Some((p, a)), _) => (p.clone(), a.clone()),
            (None, Some(a)) => {
                let (p, mut v) = crate::agentes::comando_direto(a, &pedido);
                v.pop();
                (p, v)
            }
            (None, None) => return,
        };
        args.push(pedido);
        let pasta = self.root.clone();
        let notify = self.notify.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = crate::agentes::rodar_direto(&prog, &args, &pasta, std::time::Duration::from_secs(300));
            let _ = tx.send(r);
            if let Some(n) = notify {
                n();
            }
        });
        self.ia_job = Some(IaJob { rx, tipo });
        self.say("IA trabalhando no pedido…".into(), ToastKind::Info);
    }

    /// Resposta da IA: troca o trecho (um passo só no desfazer) ou abre a resposta numa aba.
    fn ia_poll(&mut self) {
        let Some(job) = self.ia_job.as_ref() else { return };
        let Ok(r) = job.rx.try_recv() else { return };
        let job = self.ia_job.take().unwrap();
        let out = match r {
            Ok(o) => o,
            Err(e) => return self.say(format!("A IA falhou: {e}"), ToastKind::Error),
        };
        match job.tipo {
            IaTipo::Editar { ini, fim, rev, .. } => {
                if self.ed.buf.rev != rev {
                    return self.say("O texto mudou enquanto a IA pensava: proposta descartada.".into(), ToastKind::Error);
                }
                let novo = crate::agentes::limpar_resposta(&out);
                if novo.is_empty() {
                    return self.say("A IA não devolveu código.".into(), ToastKind::Error);
                }
                self.ed.aplicar_edicoes(&[(ini, fim, novo)]);
                self.ed.set_cursor(ini.row, ini.col);
                self.say("Edição da IA aplicada · u desfaz".into(), ToastKind::Done);
            }
            IaTipo::Perguntar { .. } => {
                let t = out.trim().to_string();
                if t.is_empty() {
                    return self.say("A IA não respondeu.".into(), ToastKind::Error);
                }
                self.aba_texto = Some(("Resposta da IA".into(), t));
            }
        }
    }

    /// Recarrega o arquivo se ele mudou no disco (o agente editou). Com alterações não salvas só avisa.
    fn recarregar_se_mudou(&mut self) {
        let Some(p) = self.path.clone() else { return };
        if self.mtime_olhado.elapsed().as_millis() < 700 {
            return;
        }
        self.mtime_olhado = std::time::Instant::now();
        let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) else { return };
        match self.mtime {
            None => self.mtime = Some(m),
            Some(antes) if antes == m => {}
            Some(_) => {
                self.mtime = Some(m);
                if self.modified() {
                    self.say("O arquivo mudou no disco, mas você tem alterações não salvas: :e! recarrega.".into(), ToastKind::Error);
                    return;
                }
                let Ok(bytes) = std::fs::read(&p) else { return };
                let texto = String::from_utf8_lossy(&bytes);
                let rope = ropey::Rope::from_str(texto.strip_suffix('\n').unwrap_or(&texto));
                if rope != self.ed.buf.rope {
                    self.ed.sincronizar(&rope);
                    self.saved_rev = self.ed.buf.rev;
                    self.git_load();
                    self.say("Arquivo recarregado (mudou no disco)".into(), ToastKind::Info);
                }
            }
        }
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
        self.term_agente = false;
        // havia outro aberto: ele volta ao primeiro plano
        if let Some((t, ag)) = self.term_fundo.pop() {
            self.term = Some(t);
            self.term_agente = ag;
            self.term_focus = true;
            self.term_screen = None;
            self.term_sel = None;
            self.term_esc = false;
            return;
        }
        self.agente_fila = None;
        self.term_focus = false;
        self.term_zen = false;
        self.term_esc = false;
        self.term_sel = None;
        self.term_arrasto = false;
        self.term_screen = None;
    }

    /// Tamanho do painel (a cena calcula a partir da janela).
    pub fn term_resize(&mut self, rows: u16, cols: u16) {
        self.term_dims = (rows, cols);
        if let Some(t) = self.term.as_mut() {
            t.resize(rows, cols);
        }
        for (t, _) in self.term_fundo.iter_mut() {
            t.resize(rows, cols);
        }
    }

    /// Teclas com o terminal em foco: vão para o pty; `<C-\><C-n>` devolve o teclado ao editor.
    fn term_feed(&mut self, keys: &str) {
        // processo terminado: qualquer tecla fecha o painel
        if self.term_screen.as_ref().is_some_and(|s| s.exited) {
            self.term_close();
            return;
        }
        // Shift+PageUp/PageDown percorrem o histórico
        let k = keys.to_lowercase();
        if k == "<s-pageup>" || k == "<s-pagedown>" {
            let d = if k == "<s-pageup>" { self.term_dims.0 as isize - 1 } else { -(self.term_dims.0 as isize - 1) };
            if let Some(t) = self.term.as_mut() {
                t.rolar(d);
            }
            return;
        }
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
        // o processo terminou: o painel fica com o que sobrou na tela e avisa; uma tecla fecha
        self.term_screen = Some(std::sync::Arc::new(sc));
    }

    /// Texto selecionado no terminal, se houver.
    fn term_texto_selecionado(&self) -> Option<String> {
        let (a, b) = self.term_sel?;
        let sc = self.term_screen.as_ref()?;
        let t = sc.texto(a, b);
        (!t.trim().is_empty()).then_some(t)
    }

    fn copiar_clip(&mut self, texto: String) {
        let id = self.clip.as_ref().map_or(1, |(i, _)| i + 1);
        self.clip = Some((id, texto));
    }

    /// `Ctrl+Shift+C`: copia a seleção do terminal para o clipboard do sistema.
    pub fn term_copiar(&mut self) {
        match self.term_texto_selecionado() {
            Some(t) => {
                let n = t.lines().count();
                self.copiar_clip(t);
                self.say(format!("{n} linha(s) copiada(s)"), ToastKind::Done);
            }
            None => self.say("Nada selecionado no terminal.".into(), ToastKind::Info),
        }
    }

    /// `Ctrl+Shift+V` / `Shift+Insert`: cola no terminal (com `bracketed paste` se o programa pede) ou no
    /// modo de inserção do editor.
    pub fn colar(&mut self, texto: &str) {
        if self.term_focus {
            if let Some(t) = self.term.as_mut() {
                t.colar(texto);
            }
            self.term_sel = None;
        } else if self.ed.mode() == Mode::Insert {
            let teclas: Vec<Key> = texto.replace("\r\n", "\n").chars().map(|c| if c == '\n' { Key::Enter } else { Key::Char(c) }).collect();
            self.ed.feed(&teclas);
        }
    }

    /// Mouse sobre o terminal: o programa recebe os eventos se pediu (e `shift` não está apertado);
    /// senão seleciona (copia ao soltar) e a roda rola o histórico.
    /// `acao`: press, drag, release, wheel (`botao` 64 sobe, 65 desce).
    pub fn term_mouse(&mut self, acao: &str, botao: u8, row: usize, col: usize, shift: bool) {
        let Some(sc) = self.term_screen.clone() else { return };
        let (row, col) = (row.min(sc.rows.len().saturating_sub(1)), col.min(sc.rows.first().map_or(0, |r| r.len()).saturating_sub(1)));
        let repassa = sc.mouse.modo != term::ModoMouse::Nenhum && !shift && sc.scroll == 0;
        if repassa {
            let (solta, mov) = (acao == "release", acao == "drag");
            let ok = match sc.mouse.modo {
                term::ModoMouse::Nenhum => false,
                term::ModoMouse::Cliques => !mov,
                _ => true,
            };
            if ok {
                if let Some(t) = self.term.as_mut() {
                    t.escrever_cru(&term::mouse_bytes(sc.mouse, botao, col, row, solta, mov));
                }
            }
            if acao == "press" {
                self.term_focus = true;
            }
            return;
        }
        match acao {
            "wheel" => {
                let d = if botao == 64 { 3 } else { -3 };
                if let Some(t) = self.term.as_mut() {
                    t.rolar(d);
                }
            }
            "press" if botao == 0 => {
                self.term_focus = true;
                self.term_sel = Some(((row, col), (row, col)));
                self.term_arrasto = true;
            }
            "drag" if self.term_arrasto => {
                if let Some((a, _)) = self.term_sel {
                    self.term_sel = Some((a, (row, col)));
                }
            }
            "release" if self.term_arrasto => {
                self.term_arrasto = false;
                // um clique sem arrastar não seleciona nada
                if self.term_sel.is_some_and(|(a, b)| a == b) {
                    self.term_sel = None;
                } else if let Some(t) = self.term_texto_selecionado() {
                    self.copiar_clip(t);
                }
            }
            _ => {}
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
                self.tree_git_load();
            }
            (Some(_), true) => {
                self.tree = None;
                self.tree_focus = false;
            }
            (Some(_), false) => self.tree_focus = true,
        }
    }

    fn tree_prompt_key(&mut self, k: Key) {
        let Some((op, mut texto)) = self.tree_prompt.take() else { return };
        match k {
            Key::Esc | Key::Ctrl('c') => return,
            Key::Backspace => {
                texto.pop();
            }
            Key::Char(c) => texto.push(c),
            Key::Enter => return self.tree_prompt_ok(op, texto),
            _ => {}
        }
        self.tree_prompt = Some((op, texto));
    }

    fn tree_prompt_ok(&mut self, op: TreeOp, texto: String) {
        use crate::arvore::ops;
        let Some(t) = self.tree.as_mut() else { return };
        let r = match &op {
            TreeOp::Novo(dir) => ops::criar(dir, &texto).map(|p| (p, "Criado")),
            TreeOp::Renomear(de) => ops::renomear(de, &texto).map(|p| (p, "Renomeado")),
            TreeOp::Apagar(alvo) => {
                if matches!(texto.trim(), "s" | "S" | "y" | "Y") {
                    ops::apagar(alvo).map(|_| (alvo.clone(), "Apagado"))
                } else {
                    return;
                }
            }
        };
        match r {
            Ok((p, verbo)) => {
                if matches!(op, TreeOp::Apagar(_)) {
                    t.recarregar();
                } else {
                    t.mostrar(&p);
                }
                let nome = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.say(format!("{verbo}: {nome}"), ToastKind::Done);
                self.tree_git_load();
            }
            Err(e) => self.say(format!("Não deu: {e}"), ToastKind::Error),
        }
    }

    fn tree_key(&mut self, k: Key) {
        if self.tree_prompt.is_some() {
            return self.tree_prompt_key(k);
        }
        let Some(t) = self.tree.as_mut() else { return };
        match k {
            Key::Char('a') => {
                let dir = t.pasta_alvo();
                self.tree_prompt = Some((TreeOp::Novo(dir), String::new()));
            }
            Key::Char('r') => {
                if let Some(n) = t.atual() {
                    let (c, nome) = (n.caminho.clone(), n.nome.clone());
                    self.tree_prompt = Some((TreeOp::Renomear(c), nome));
                }
            }
            Key::Char('d') => {
                if let Some(n) = t.atual() {
                    self.tree_prompt = Some((TreeOp::Apagar(n.caminho.clone()), String::new()));
                }
            }
            Key::Char(c @ ('y' | 'x')) => {
                if let Some(n) = t.atual() {
                    let nome = n.nome.clone();
                    self.tree_clip = Some((n.caminho.clone(), c == 'x'));
                    self.say(format!("{}: {nome}", if c == 'x' { "Recortado" } else { "Copiado" }), ToastKind::Info);
                }
            }
            Key::Char('p') => {
                let Some((origem, mover)) = self.tree_clip.clone() else {
                    self.say("Nada copiado (use y ou x)".into(), ToastKind::Info);
                    return;
                };
                let dir = t.pasta_alvo();
                match crate::arvore::ops::colar(&origem, &dir, mover) {
                    Ok(p) => {
                        t.mostrar(&p);
                        if mover {
                            self.tree_clip = None;
                        }
                        self.say("Colado".into(), ToastKind::Done);
                    }
                    Err(e) => self.say(format!("Não deu: {e}"), ToastKind::Error),
                }
            }
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
        let notify = self.notify.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
        let (base, listando, base_do_cache) = match kind {
            PickKind::Arquivos => {
                // a lista guardada aparece já; a nova (do disco) chega em segundo plano
                let cache = busca::lista_em_cache(&self.root);
                let fresca = cache.as_ref().is_some_and(|(idade, _)| *idade < std::time::Duration::from_secs(5));
                let do_cache = cache.is_some();
                let base = cache.map(|(_, v)| v).unwrap_or_default();
                let bg = (!fresca).then(|| busca::listar_bg(self.root.clone(), notify));
                (base, bg, do_cache)
            }
            PickKind::Recentes => (busca::recentes(), None, false),
            PickKind::Texto | PickKind::Lista => (Vec::new(), None, false),
        };
        self.picker = Some(Picker { kind, titulo: String::new(), lista: Vec::new(), mapa: Vec::new(), query: String::new(), total: base.len(), base, items: Vec::new(), achados: Vec::new(), sel: 0, listando, buscando: None, base_do_cache, prev: None, aceitar_depois: false });
        self.pick_refresh();
    }

    /// Trecho do arquivo do item selecionado (buscador de arquivos, recentes e texto).
    fn pick_preview(&self, p: &Picker) -> Vec<(usize, String, bool)> {
        use std::io::Read;
        let Some(it) = p.items.get(p.sel) else { return Vec::new() };
        let (rel, alvo) = match p.kind {
            PickKind::Arquivos | PickKind::Recentes => (it.text.clone(), None),
            PickKind::Texto => match it.text.rsplit_once(':') {
                Some((f, l)) => (f.to_string(), l.parse::<usize>().ok().map(|n| n.saturating_sub(1))),
                None => return Vec::new(),
            },
            PickKind::Lista => return Vec::new(),
        };
        let caminho = if std::path::Path::new(&rel).is_absolute() { std::path::PathBuf::from(&rel) } else { self.root.join(&rel) };
        let Ok(f) = std::fs::File::open(&caminho) else { return Vec::new() };
        let mut buf = Vec::new();
        if f.take(96 * 1024).read_to_end(&mut buf).is_err() || buf.contains(&0) {
            return Vec::new();
        }
        let texto = String::from_utf8_lossy(&buf);
        let linhas: Vec<&str> = texto.lines().collect();
        let n = PICK_PREVIEW;
        let ini = match alvo {
            Some(a) => a.saturating_sub(n / 2).min(linhas.len().saturating_sub(n)),
            None => 0,
        };
        linhas.iter().enumerate().skip(ini).take(n).map(|(i, l)| (i + 1, l.chars().take(200).collect(), alvo == Some(i))).collect()
    }

    fn pick_refresh(&mut self) {
        self.pick_atualizar(false);
    }

    /// Refaz os itens mostrados. `manter_sel`: chegou novidade em segundo plano, não volta ao topo.
    fn pick_atualizar(&mut self, manter_sel: bool) {
        let root = self.root.clone();
        let notify = self.notify.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
        let Some(p) = self.picker.as_mut() else { return };
        if !manter_sel {
            p.sel = 0;
        }
        match p.kind {
            PickKind::Arquivos | PickKind::Recentes => {
                // a letra a mais só procura entre os que já casaram
                let entre = p.prev.as_ref().filter(|(q, _)| !q.is_empty() && p.query.starts_with(q.as_str()) && p.query != *q).map(|(_, v)| v.clone());
                let entre = if manter_sel { None } else { entre };
                let (r, todos) = busca::fuzzy_sub(&p.query, &p.base, entre.as_deref(), 200);
                p.prev = Some((p.query.clone(), todos));
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
                if !manter_sel {
                    // consulta nova: pára a busca anterior e começa outra, sem esperar o disco
                    p.achados.clear();
                    p.buscando = (!p.query.is_empty()).then(|| busca::buscar_texto_bg(root.clone(), p.query.clone(), 200, notify));
                }
                p.total = p.achados.len();
                p.items = p.achados.iter().map(|a| PickItem { text: format!("{}:{}", a.arquivo, a.linha + 1), hits: Vec::new(), detail: a.texto.clone() }).collect();
            }
        }
        p.sel = p.sel.min(p.items.len().saturating_sub(1));
    }

    /// Pega o que as buscas em segundo plano já acharam.
    fn pick_poll(&mut self) {
        use busca::Parcial;
        let Some(p) = self.picker.as_mut() else { return };
        let mut mudou = false;
        if let Some(bg) = p.listando.as_mut() {
            for m in bg.novidades() {
                match m {
                    Parcial::Itens(v) if !p.base_do_cache => {
                        p.base.extend(v);
                        mudou = true;
                    }
                    Parcial::Completa(v) => {
                        p.base = v;
                        p.base_do_cache = false;
                        mudou = true;
                    }
                    _ => {}
                }
            }
            if bg.feita {
                p.listando = None;
            }
        }
        if let Some(bg) = p.buscando.as_mut() {
            for m in bg.novidades() {
                if let Parcial::Itens(v) = m {
                    p.achados.extend(v);
                    mudou = true;
                }
            }
            if bg.feita {
                p.buscando = None;
            }
        }
        if mudou {
            // só depois de novos itens na base o conjunto anterior deixa de valer
            if let Some(p) = self.picker.as_mut() {
                p.prev = None;
            }
            self.pick_atualizar(true);
        }
        // Enter já apertado: aceita assim que houver o que aceitar (ou desiste se a busca acabou vazia)
        if let Some(p) = self.picker.as_mut() {
            if p.aceitar_depois {
                if !p.items.is_empty() {
                    p.aceitar_depois = false;
                    self.pick_accept();
                } else if p.listando.is_none() && p.buscando.is_none() {
                    p.aceitar_depois = false;
                }
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
        // Enter antes de a busca em segundo plano trazer algo: espera chegar
        if let Some(p) = self.picker.as_mut() {
            if p.items.is_empty() && (p.listando.is_some() || p.buscando.is_some()) {
                p.aceitar_depois = true;
                return;
            }
        }
        let Some(p) = self.picker.take() else { return };
        let Some(item) = p.items.get(p.sel) else { return };
        let (path, goto) = match p.kind {
            PickKind::Lista => {
                let Some(l) = p.mapa.get(p.sel).and_then(|i| p.lista.get(*i)).cloned() else { return };
                match l.destino {
                    Destino::Local(arq, row, col16) => self.ir_para_local(&arq, row, col16),
                    Destino::Acao(a) => self.aplicar_acao(a),
                    Destino::Agente(bin) => self.escolher_agente(bin),
                    Destino::Modo(id) => self.escolher_modo(id),
                    Destino::Sessao { id, pasta } => self.retomar_sessao(&id, &pasta),
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
        self.guardar_cursor();
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
        if self.ed.mode() != Mode::Insert {
            self.snip = None;
        }
        if self.snip.is_some() && self.snip_tecla(k) {
            return;
        }
        let antes_snip = (self.ed.cursor.row, self.ed.cursor.col, self.ed.buf.line_len(self.ed.cursor.row));
        let tinha_snip = self.snip.is_some();
        if self.objeto_sintaxe(k) {
            return;
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
        if livre && self.ed.showcmd() == "g" && k == Key::Char('f') {
            self.ed.feed(&[Key::Esc]);
            return self.ir_para_arquivo();
        }
        if livre && self.ed.showcmd() == "g" && k == Key::Char('x') {
            self.ed.feed(&[Key::Esc]);
            return self.abrir_com_o_sistema();
        }
        if livre && self.ed.showcmd() == "g" && matches!(k, Key::Char('r' | 'y')) && self.lsp.is_some() {
            self.ed.feed(&[Key::Esc]);
            return self.lsp_consulta(if k == Key::Char('r') { PedidoLsp::Referencias } else { PedidoLsp::TipoDaDefinicao });
        }
        self.ed.set_nome_arquivo(self.path.as_ref().map(|p| p.to_string_lossy().into_owned()));
        self.ed.feed(&[k]);
        if tinha_snip {
            self.snip_ajustar(antes_snip);
        }
        self.comp_update();
        self.sig_update(k);
        // `:s`, `:d`, `:g`... o núcleo resolve; `:w`, `:q`, `:e`... voltam para cá
        if let Some(linha) = self.ed.take_host_cmd() {
            self.exec_ex(&linha);
        }
        if let Some(a) = self.ed.take_host_aba() {
            self.aba_texto = Some(a);
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
        // clicar no texto tira o teclado do terminal
        self.term_focus = false;
        self.term_zen = false;
        let (r, c) = self.clique_em(row, vcol, self.ed.mode() == Mode::Insert);
        self.ed.set_cursor(r, c);
    }

    /// Botão esquerdo no texto: `press` posiciona (duplo clique pega a palavra, triplo a linha; com `shift`
    /// estende a seleção), `drag` seleciona arrastando, `release` solta.
    pub fn mouse_esq(&mut self, acao: &str, row: usize, vcol: usize, shift: bool) {
        if self.dash || self.picker.is_some() {
            return;
        }
        match acao {
            "press" => {
                let agora = std::time::Instant::now();
                let n = match self.clique {
                    Some((t, r, c, n)) if agora.duration_since(t).as_millis() < 450 && r == row && c == vcol => (n % 3) + 1,
                    _ => 1,
                };
                self.clique = Some((agora, row, vcol, n));
                self.mouse_baixo = true;
                if shift && self.ed.mode() != Mode::Insert {
                    // Shift+clique: a seleção vai do cursor até o clique
                    if self.ed.mode() != Mode::Visual {
                        self.ed.feed(&[Key::Char('v')]);
                    }
                    self.mouse_para(row, vcol);
                    return;
                }
                if self.ed.mode() == Mode::Visual {
                    self.ed.feed(&[Key::Esc]);
                }
                self.click(row, vcol);
                match n {
                    2 if self.ed.mode() == Mode::Normal => self.ed.feed_str("viw"),
                    3 if self.ed.mode() == Mode::Normal => self.ed.feed_str("V"),
                    _ => {}
                }
            }
            "drag" if self.mouse_baixo => {
                if self.ed.mode() == Mode::Normal {
                    // começa a seleção no ponto do clique (onde o cursor está)
                    self.ed.feed(&[Key::Char('v')]);
                }
                if self.ed.mode() == Mode::Visual {
                    self.mouse_para(row, vcol);
                }
            }
            "release" => self.mouse_baixo = false,
            _ => {}
        }
    }

    /// Leva o cursor (e a ponta móvel da seleção) à célula `(row, vcol)`.
    fn mouse_para(&mut self, row: usize, vcol: usize) {
        let (r, c) = self.clique_em(row, vcol, false);
        self.ed.set_cursor(r, c);
    }

    /// (linha mostrada, coluna visual) da tela -> (linha, coluna em caracteres) do arquivo, com dobras e
    /// linhas quebradas. `depois_do_fim`: na inserção o cursor pode ficar depois do último caractere.
    fn clique_em(&self, row: usize, vcol: usize, depois_do_fim: bool) -> (usize, usize) {
        let proj = self.proj_atual();
        let (r, ini, fim) = match &proj {
            Some(p) => {
                let pr = row.min(p.linhas() - 1);
                (p.mapa[pr], p.ini[pr], Some((p.fim[pr], p.mapa.get(pr + 1) == Some(&p.mapa[pr]))))
            }
            None => (row.min(self.ed.buf.line_count() - 1), 0, None),
        };
        let r = r.min(self.ed.buf.line_count() - 1);
        let linha: Vec<char> = self.ed.buf.line(r).chars().collect();
        let fim_seg = fim.map_or(linha.len(), |(f, _)| f.min(linha.len()));
        let seg: String = linha[ini.min(fim_seg)..fim_seg].iter().collect();
        let len = seg.chars().count();
        let mais_trechos = fim.is_some_and(|(_, m)| m);
        // num trecho que não é o último o cursor para no último caractere dele
        let max = if depois_do_fim && !mais_trechos { len } else { len.saturating_sub(1) };
        let c = if vcol >= vcol_start(&seg, len) { max } else { col_for_vcol(&seg, vcol) };
        (r, ini + c)
    }

    /// Rolagem em linhas (pode ser fracionária): `lines > 0` desce.
    pub fn scroll(&mut self, lines: f32) {
        let proj = self.proj_atual();
        let n = proj.as_ref().map_or(self.ed.buf.line_count(), |p| p.linhas());
        let a_proj = |r: usize| proj.as_ref().map_or(r, |p| p.a_proj(r));
        let a_real = |p: usize| proj.as_ref().map_or(p, |q| q.a_real(p));
        let max = n.saturating_sub(1) as f32;
        let top_p = a_proj(self.top as usize) as f32 + self.top.fract();
        let wanted = top_p + lines;
        let novo = wanted.clamp(0.0, max);
        self.top = a_real(novo.floor() as usize) as f32 + novo.fract();
        if wanted != novo {
            let id = self.bump.map_or(1, |(i, _)| i + 1);
            self.bump = Some((id, wanted - novo));
        }
        // o cursor acompanha para não sair da tela
        let so = self.scrolloff.min((self.rows - 1.0) / 2.0);
        let row = proj.as_ref().map_or(self.ed.cursor.row, |p| p.pos_a_proj(self.ed.cursor.row, self.ed.cursor.col).0) as f32;
        let (lo, hi) = (novo + so, novo + self.rows - 1.0 - so);
        if row < lo {
            self.ed.set_cursor(a_real(lo.ceil() as usize), self.ed.cursor.col);
        } else if row > hi {
            self.ed.set_cursor(a_real(hi.floor().max(0.0) as usize), self.ed.cursor.col);
        }
    }

    // ------------------------------------------------------------------
    // comandos `:`
    // ------------------------------------------------------------------

    /// `:set` das opções que são da interface (as do núcleo já foram tratadas): numeração, `scrolloff`,
    /// `clipboard`. As que o app não tem avisam.
    fn set_ui(&mut self, args: &str) {
        for t in args.split_whitespace() {
            let (nome, valor) = match t.split_once(['=', ':']) {
                Some((n, v)) => (n, Some(v)),
                None => (t, None),
            };
            let (nome, inverte) = match nome.strip_suffix('!') {
                Some(n) => (n, true),
                None => (nome, false),
            };
            let (base, liga) = match nome.strip_prefix("no").filter(|r| matches!(*r, "number" | "nu" | "relativenumber" | "rnu" | "wrap" | "linebreak" | "lbr" | "list" | "hlsearch" | "hls" | "incsearch" | "is" | "cursorline" | "cul")) {
                Some(r) => (r, false),
                None => (nome, true),
            };
            let flip = |v: &mut bool| *v = if inverte { !*v } else { liga };
            match base {
                "number" | "nu" => flip(&mut self.numero),
                "relativenumber" | "rnu" => flip(&mut self.relativo),
                "wrap" => flip(&mut self.wrap),
                "linebreak" | "lbr" => flip(&mut self.linebreak),
                "scrolloff" | "so" => match valor.and_then(|v| v.parse::<f32>().ok()) {
                    Some(n) if n >= 0.0 => self.scrolloff = n,
                    _ => self.say(format!("E521: número esperado: {t}"), ToastKind::Error),
                },
                "clipboard" | "cb" => {
                    let v = valor.unwrap_or("");
                    self.ed.clipboard_unnamedplus = v.contains("unnamedplus");
                }
                // opções que o app não implementa mas que não devem incomodar
                "foldmethod" | "fdm" => match valor.and_then(crate::dobras::Metodo::de) {
                    Some(m) => {
                        self.fold_metodo = m;
                        self.fold_rev = u64::MAX;
                        // as dobras automáticas começam abertas (como `foldlevel=99`); `zM` fecha tudo
                        self.ed.set_foldlevel(99);
                    }
                    None => self.say(format!("E474: Argumento inválido: {t}"), ToastKind::Error),
                },
                "foldlevel" | "fdl" => match valor.and_then(|v| v.parse::<usize>().ok()) {
                    Some(n) => self.ed.set_foldlevel(n),
                    None => self.say(format!("E521: número esperado: {t}"), ToastKind::Error),
                },
                "background" | "bg" => match valor {
                    Some("light") => crate::design::set_light(true),
                    Some("dark") => crate::design::set_light(false),
                    _ => self.say(format!("E474: Argumento inválido: {t}"), ToastKind::Error),
                },
                "hlsearch" | "hls" | "incsearch" | "is" | "list" | "cursorline" | "cul" | "mouse" | "termguicolors" | "tgc" | "hidden" | "hid" | "autoindent" | "ai" | "smartindent" | "si" | "ruler" | "showcmd" | "laststatus" | "ls" => {}
                _ => self.say(format!("E518: Opção desconhecida: {t}"), ToastKind::Error),
            }
        }
    }

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

    /// Roda `cmd` no shell e devolve a saída (stdout + stderr), com `entrada` no stdin.
    fn shell_saida(cmd: &str, entrada: &str) -> String {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut c = if cfg!(windows) { Command::new("cmd") } else { Command::new("sh") };
        c.args(if cfg!(windows) { ["/C", cmd] } else { ["-c", cmd] });
        let Ok(mut filho) = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
            return format!("não rodou: {cmd}");
        };
        if let Some(mut si) = filho.stdin.take() {
            let e = entrada.to_string();
            std::thread::spawn(move || {
                let _ = si.write_all(e.as_bytes());
            });
        }
        match filho.wait_with_output() {
            Ok(o) => {
                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                s.push_str(&String::from_utf8_lossy(&o.stderr));
                s
            }
            Err(e) => e.to_string(),
        }
    }

    /// `:r arq`, `:r !cmd`, `:!cmd` e `:{faixa}!cmd` (o núcleo manda no formato `Nr arg`, `!cmd`, `A,B!cmd`).
    /// Devolve `true` se tratou.
    fn ex_shell(&mut self, line: &str) -> bool {
        let t = line.trim();
        let ultima = self.ed.buf.line_count() - 1;
        if let Some(cmd) = t.strip_prefix('!') {
            let saida = Self::shell_saida(cmd.trim(), "");
            self.aba_texto = Some((format!("!{}", cmd.trim()), saida));
            return true;
        }
        let dig = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if dig > 0 && t[dig..].starts_with("r ") {
            let depois: usize = t[..dig].parse().unwrap_or(0);
            let arg = t[dig + 2..].trim();
            let texto = match arg.strip_prefix('!') {
                Some(c) => Self::shell_saida(c.trim(), ""),
                None => std::fs::read_to_string(arg).unwrap_or_default(),
            };
            if texto.is_empty() {
                return true;
            }
            let row = depois.min(ultima + 1);
            // entra como linhas novas depois de `row` (0 = antes da primeira)
            let (pos, txt) = if row == 0 {
                (Pos::new(0, 0), texto.clone())
            } else {
                let r = row - 1;
                (Pos::new(r, self.ed.buf.line_len(r)), format!("\n{}", texto.trim_end_matches('\n')))
            };
            let txt = if row == 0 && !txt.ends_with('\n') { format!("{txt}\n") } else { txt };
            self.ed.aplicar_edicoes(&[(pos, pos, txt)]);
            return true;
        }
        if let Some((faixa, cmd)) = t.split_once('!') {
            if let Some((a, b)) = faixa.split_once(',') {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    let (a, b) = (a.saturating_sub(1).min(ultima), b.saturating_sub(1).min(ultima));
                    let ini = Pos::new(a, 0);
                    let fim = Pos::new(b, self.ed.buf.line_len(b));
                    let entrada: String = (a..=b).map(|r| format!("{}\n", self.ed.buf.line(r))).collect();
                    let saida = Self::shell_saida(cmd.trim(), &entrada);
                    self.ed.aplicar_edicoes(&[(ini, fim, saida.trim_end_matches('\n').to_string())]);
                    return true;
                }
            }
        }
        false
    }

    fn exec_ex(&mut self, line: &str) {
        if self.ex_shell(line) {
            return;
        }
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
            "set" => self.set_ui(arg),
            "w" | "write" => {
                self.write(arg);
                self.git_load();
            }
            "q" | "quit" => {
                if self.modified() && !bang && !self.irmao {
                    self.say("Alterações não salvas. Use :q! para sair sem salvar.".into(), ToastKind::Error);
                } else {
                    self.sair();
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
            "Astrograph" => self.comando_astrograph(arg),
            "AgentInstall" => self.agente_instalar(arg),
            "AgentLogin" => self.agente_login(),
            "Symbols" | "LspSymbols" => self.lsp_consulta(PedidoLsp::Simbolos),
            "WorkspaceSymbol" | "LspWorkspaceSymbols" => {
                let Some((c, _, _, _)) = self.lsp_ctx() else { return };
                let id = c.lock().unwrap().simbolos_do_projeto(arg.trim());
                self.lsp_esperar(id, PedidoLsp::SimbolosDoProjeto);
            }
            "References" | "LspReferences" => self.lsp_consulta(PedidoLsp::Referencias),
            "Diagnostics" | "LspDiagnostics" => self.diag_lista(),
            "clo" | "close" => self.jan_pedido = Some(PedidoJanela::Fechar { forcar: bang }),
            "on" | "only" => self.jan_pedido = Some(PedidoJanela::So),
            "tabnew" | "tabe" | "tabedit" => self.aba_pedido = Some(PedidoAba::Nova((!arg.is_empty()).then(|| PathBuf::from(arg)))),
            "tabc" | "tabclose" => self.aba_pedido = Some(PedidoAba::Fechar { forcar: bang }),
            "tabn" | "tabnext" => self.aba_pedido = Some(PedidoAba::Proxima),
            "tabp" | "tabprevious" | "tabN" | "tabNext" => self.aba_pedido = Some(PedidoAba::Anterior),
            "tabo" | "tabonly" => self.aba_pedido = Some(PedidoAba::So),
            "noh" | "nohlsearch" | "syntax" | "filetype" => {}
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
            self.sair();
        }
    }

    fn gravar(&mut self, arg: &str) {
        let target = if arg.is_empty() { self.path.clone() } else { Some(PathBuf::from(arg)) };
        let Some(p) = target else {
            self.say("Sem nome de arquivo. Use :w nome".into(), ToastKind::Error);
            return;
        };
        if self.ec_trim {
            // `trim_trailing_whitespace`: tira o espaço do fim das linhas (um passo só no desfazer)
            let mut ed_edicoes = Vec::new();
            for r in 0..self.ed.buf.line_count() {
                let l = self.ed.buf.line(r);
                let n = l.chars().count();
                let t = l.trim_end_matches([' ', '\t']).chars().count();
                if t < n {
                    ed_edicoes.push((Pos::new(r, t), Pos::new(r, n), String::new()));
                }
            }
            if !ed_edicoes.is_empty() {
                self.ed.aplicar_edicoes(&ed_edicoes);
            }
        }
        let mut text = self.ed.to_file_text();
        match self.ec_final_nl {
            Some(true) if !text.is_empty() && !text.ends_with('\n') => text.push('\n'),
            Some(false) => {
                while text.ends_with('\n') {
                    text.pop();
                }
            }
            _ => {}
        }
        match std::fs::write(&p, &text) {
            Ok(()) => {
                let nome = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.say(format!("{nome} salvo · {} linhas", self.ed.buf.line_count()), ToastKind::Done);
                // o que acabamos de gravar não é "mudou no disco"
                self.mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                if self.path.is_none() {
                    self.path = Some(p);
                }
                self.saved_rev = self.ed.buf.rev;
                self.tree_git_load();
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
    /// Refaz as dobras automáticas (`foldmethod=indent|syntax`) quando o texto mudou.
    fn fold_auto(&mut self) {
        let rev = self.ed.buf.rev;
        if self.fold_metodo == crate::dobras::Metodo::Manual || self.fold_rev == rev {
            return;
        }
        self.fold_rev = rev;
        let faixas = match self.fold_metodo {
            crate::dobras::Metodo::Indent => crate::dobras::por_recuo(&self.ed.lines(), self.ed.shiftwidth(), astrograph_core::motion::tabstop()),
            crate::dobras::Metodo::Syntax => {
                self.refresh_syntax();
                let Some((doc, _)) = self.doc.as_ref() else { return };
                let mut v = Vec::new();
                for cap in ["@function.outer", "@class.outer", "@loop.outer", "@conditional.outer", "@block.outer"] {
                    for r in doc.textobject_ranges(cap) {
                        // fim exclusivo na coluna 0: a dobra acaba na linha de cima
                        let fim = if r.ecol == 0 && r.erow > r.srow { r.erow - 1 } else { r.erow };
                        v.push((r.srow, fim));
                    }
                }
                v
            }
            crate::dobras::Metodo::Manual => return,
        };
        self.ed.definir_dobras_auto(&faixas);
    }

    /// Alguma linha é mais larga que `w` colunas? (só importa com `wrap`; guarda a resposta por revisão)
    fn tem_linha_larga(&self, w: usize) -> bool {
        let rev = self.ed.buf.rev;
        let (r0, w0, v0) = self.larga_cache.get();
        if r0 == rev && w0 == w {
            return v0;
        }
        let buf = &self.ed.buf;
        let mut larga = false;
        for i in 0..buf.line_count() {
            let n = buf.line_len(i);
            // tab e caracteres largos ocupam mais de uma coluna: só aí vale medir de verdade
            if n > w || (n * 2 > w && vcol_start(&buf.line(i), n) > w) {
                larga = true;
                break;
            }
        }
        self.larga_cache.set((rev, w, larga));
        larga
    }

    /// A projeção do texto (dobras fechadas escondidas e linhas quebradas); None quando a tela mostra o texto
    /// como está.
    fn proj_atual(&self) -> Option<std::sync::Arc<crate::dobras::Proj>> {
        let dobras = self.ed.dobras_fechadas();
        let larg = if self.wrap { Some(self.cols.max(10)) } else { None };
        if dobras.is_empty() && !larg.is_some_and(|w| self.tem_linha_larga(w)) {
            *self.proj.borrow_mut() = None;
            return None;
        }
        let rev = self.ed.buf.rev;
        if let Some(p) = self.proj.borrow().as_ref() {
            if p.rev == rev && p.dobras == dobras && p.larg == larg && p.em_palavra == self.linebreak {
                return Some(p.clone());
            }
        }
        let buf = &self.ed.buf;
        let p = std::sync::Arc::new(crate::dobras::Proj::construir(&|r| buf.line(r), buf.line_count(), &dobras, rev, larg, self.linebreak));
        *self.proj.borrow_mut() = Some(p.clone());
        Some(p)
    }

    /// Linha do arquivo de uma linha mostrada (clique, arrastar).
    fn proj_a_real(&self, row: usize) -> usize {
        self.proj_atual().map_or(row, |p| p.a_real(row))
    }

    fn follow_cursor(&mut self) {
        let proj = self.proj_atual();
        let n = proj.as_ref().map_or(self.ed.buf.line_count(), |p| p.linhas());
        let a_proj = |r: usize| proj.as_ref().map_or(r, |p| p.a_proj(r));
        let top_p = a_proj(self.top as usize) as f32 + self.top.fract();
        let so = self.scrolloff.min((self.rows - 1.0) / 2.0);
        // a linha mostrada do cursor (numa linha quebrada, o trecho onde ele está)
        let row = proj.as_ref().map_or(self.ed.cursor.row, |p| p.pos_a_proj(self.ed.cursor.row, self.ed.cursor.col).0) as f32;
        let mut top = top_p;
        let (lo, hi) = (top + so, top + self.rows - 1.0 - so);
        if row < top - self.rows || row > top + 2.0 * self.rows {
            top = (row - self.rows / 3.0).max(0.0);
        } else if row < lo {
            top = (row - so).max(0.0);
        } else if row > hi {
            top = row - self.rows + 1.0 + so;
        }
        let last = n.saturating_sub(1) as f32;
        top = top.clamp(0.0, last);
        // só mexe se mudou (a conversão de volta arredonda para a linha do arquivo)
        if top != top_p {
            self.top = match &proj {
                Some(p) => p.a_real(top.floor() as usize) as f32 + top.fract(),
                None => top,
            };
        }
        // horizontal (em colunas visuais); com `wrap` não há rolagem lateral
        if self.wrap {
            self.left = 0;
            return;
        }
        let line = self.ed.buf.line(self.ed.cursor.row);
        let v = vcol_cursor(&line, self.ed.cursor.col);
        if v < self.left {
            self.left = v;
        } else if v >= self.left + self.cols {
            self.left = v + 1 - self.cols;
        }
    }

    pub fn view_model(&mut self) -> ViewModel {
        if let Some(t) = self.ed.clipboard_alterado() {
            self.copiar_clip(t);
        }
        self.pick_poll();
        self.ia_poll();
        self.atualizar_poll();
        self.agente_fila_poll();
        self.recarregar_se_mudou();
        self.git_poll();
        self.term_refresh();
        self.lsp_sync();
        self.fold_auto();
        self.follow_cursor();
        self.refresh_syntax();
        self.seq += 1;
        let proj = self.proj_atual();
        let real_n = self.ed.buf.line_count();
        let line_count = proj.as_ref().map_or(real_n, |p| p.linhas());
        // topo na linha mostrada (com dobras o topo do arquivo e o da tela diferem)
        let top_v = match &proj {
            Some(p) => p.a_proj(self.top as usize) as f32 + self.top.fract(),
            None => self.top,
        };
        let lo = (top_v as usize).saturating_sub(SPAN_MARGIN);
        let hi = (top_v as usize + self.rows.ceil() as usize + SPAN_MARGIN).min(line_count);
        let mut spans: Vec<Vec<Span>> = vec![Vec::new(); hi.saturating_sub(lo)];
        if let Some((doc, _)) = &self.doc {
            // `ini..fim`: o trecho da linha que aparece nesta linha mostrada (linhas quebradas)
            let mut poe = |row_real: usize, prow: usize, ini: usize, fim: usize, sp: &astrograph_syntax::Span| {
                let Some(cls) = classify(sp.group) else { return };
                let line = self.ed.buf.line(row_real);
                let to_char = |byte: usize| line.char_indices().take_while(|(b, _)| *b < byte).count();
                let (a, b) = (to_char(sp.start_col).max(ini), to_char(sp.end_col).min(fim));
                if a < b {
                    spans[prow - lo].push(Span { start: (a - ini) as u32, end: (b - ini) as u32, cls });
                }
            };
            match &proj {
                None => {
                    for sp in doc.highlights(lo..hi) {
                        poe(sp.row, sp.row, 0, usize::MAX, &sp);
                    }
                }
                Some(p) => {
                    let mut ultimo: Option<(usize, Vec<astrograph_syntax::Span>)> = None;
                    for prow in lo..hi {
                        let r = p.mapa[prow];
                        // as linhas quebradas repetem a mesma linha do arquivo: o realce dela vem uma vez só
                        if ultimo.as_ref().is_none_or(|(u, _)| *u != r) {
                            ultimo = Some((r, doc.highlights(r..r + 1)));
                        }
                        for sp in &ultimo.as_ref().unwrap().1 {
                            poe(r, prow, p.ini[prow], p.fim[prow], sp);
                        }
                    }
                }
            }
        }
        // o aviso "⋯ N linhas" da dobra fica em cinza
        if let Some(p) = &proj {
            for prow in lo..hi {
                if let Some((fim_txt, n)) = p.marcas[prow] {
                    spans[prow - lo].push(Span { start: fim_txt as u32, end: (fim_txt + n) as u32, cls: crate::design::Cls::Comment });
                }
            }
        }
        // posições do arquivo -> da tela (uma linha escondida cai no começo da dobra)
        let mapa_pos = |r: usize, c: usize| -> (usize, usize) {
            match &proj {
                Some(p) => p.pos_a_proj(r, c),
                None => (r, c),
            }
        };
        let sel = self.ed.selection().map(|(k, a)| {
            let c = self.ed.cursor;
            // em bloco os dois cantos ficam como estão (o retângulo vem das colunas visuais de cada um)
            let (lo, hi) = if a <= c || k == VKind::Block { (a, c) } else { (c, a) };
            (k, mapa_pos(lo.row, lo.col), mapa_pos(hi.row, hi.col))
        });
        // valor padrão do snippet: aparece selecionado até a primeira tecla
        let sel = sel.or_else(|| {
            let sn = self.snip.as_ref().filter(|s| s.sel)?;
            let (a, b) = sn.stops[sn.idx];
            Some((VKind::Char, mapa_pos(a.row, a.col), mapa_pos(b.row, b.col.saturating_sub(1))))
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
                for prow in lo..hi {
                    let row = proj.as_ref().map_or(prow, |p| p.mapa[prow]);
                    let (ini, fim) = proj.as_ref().map_or((0, usize::MAX), |p| (p.ini[prow], p.fim[prow]));
                    let line = self.ed.buf.line(row);
                    for m in re.find_iter(&line).take(200) {
                        if m.start() == m.end() {
                            continue;
                        }
                        let a = line[..m.start()].chars().count();
                        let b = a + line[m.start()..m.end()].chars().count();
                        let (a, b) = (a.max(ini), b.min(fim));
                        if a < b {
                            hits.push((prow, a - ini, b - ini));
                        }
                    }
                }
            }
        }
        let hits_id = if pat.is_some() && !self.hl_off { self.hl_id } else { 0 };
        let signs = self.git_signs();
        // com dobras: os sinais, diagnósticos e o realce de cópia vão para as linhas mostradas
        let (signs, diags) = match &proj {
            Some(p) => {
                let mut v: Vec<(usize, git::Sign)> = Vec::new();
                for (r, sg) in signs.iter() {
                    let pr = p.a_proj(*r);
                    if v.last().is_none_or(|(l, _)| *l != pr) {
                        v.push((pr, *sg));
                    }
                }
                let d: Vec<lsp::Diag> = self
                    .lsp_diags
                    .iter()
                    .map(|g| {
                        let (r0, c0) = mapa_pos(g.row, g.col);
                        let (r1, c1) = mapa_pos(g.end_row, g.end_col);
                        lsp::Diag { row: r0, col: c0, end_row: r1, end_col: c1, ..g.clone() }
                    })
                    .collect();
                (std::sync::Arc::new(v), std::sync::Arc::new(d))
            }
            None => (signs, self.lsp_diags.clone()),
        };
        let mode = if self.term_focus && self.term.is_some() {
            ModeKind::Terminal
        } else if self.ed.cmdline().is_some() || self.tree_prompt.is_some() || self.ia_prompt.is_some() {
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
            .ia_prompt
            .as_ref()
            .map(|(tipo, t)| ('✦', format!("{}{t}", if matches!(tipo, IaTipo::Editar { .. }) { "Editar a seleção: " } else { "Perguntar à IA: " })))
            .or_else(|| self.tree_prompt.as_ref().map(|(op, t)| ('›', format!("{}{t}", op.rotulo()))))
            .or_else(|| self.ed.cmdline().map(|c| (':', c.to_string())))
            .or_else(|| search.map(|s| (s.chars().next().unwrap_or('/'), s.chars().skip(1).collect())));
        ViewModel {
            tabs: Vec::new(),
            tab_active: 0,
            doc_id: self.doc_id,
            seq: self.seq,
            rev: self.ed.buf.rev,
            rope: proj.as_ref().map_or_else(|| self.ed.buf.rope.clone(), |p| p.rope.clone()),
            lines: line_count,
            span_lo: lo,
            spans,
            top: top_v,
            left: self.left,
            cursor: mapa_pos(self.ed.cursor.row, self.ed.cursor.col),
            row_map: proj.as_ref().map(|p| p.mapa.clone()),
            row_cont: proj.as_ref().map(|p| p.cont.clone()),
            cursor_real: (self.ed.cursor.row, self.ed.cursor.col),
            mode,
            selection: sel,
            sel_eol: self.ed.bloco_ate_o_fim(),
            name: self.nome_fixo.clone().or_else(|| self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "Sem nome".into()),
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
            diags,
            hover: self.hover_text.clone(),
            whichkey: self.menu_lider(),
            progress: if self.atualizando.is_some() { Some("Atualizando o Astrograph…".to_string()) } else if self.ia_job.is_some() { Some("IA trabalhando…".to_string()) } else { self.lsp_progresso() },
            numero: self.numero,
            relativo: self.relativo,
            sig: self.sig.as_ref().filter(|(_, r)| *r == self.ed.cursor.row && self.ed.mode() == Mode::Insert).map(|(a, _)| SigView { texto: a.texto.clone(), ativo: a.ativo, doc: a.doc.clone() }),
            completion: self.comp.as_ref().filter(|c| !c.shown.is_empty()).map(|c| {
                let first = c.sel.saturating_sub(COMP_LINHAS - 1);
                CompView {
                    items: c.shown.iter().skip(first).take(COMP_LINHAS).map(|(i, h)| CompItem { label: c.items[*i].label.clone(), detail: c.items[*i].detalhe.clone(), kind: c.items[*i].tipo, hits: h.clone() }).collect(),
                    sel: c.sel - first,
                    anchor: mapa_pos(c.start.row, c.start.col),
                }
            }),
            git: self.git_branch.clone().map(|branch| GitView { branch, signs: signs.clone() }),
            term: self.term_screen.as_ref().map(|s| TermView { screen: s.clone(), focus: self.term_focus, zen: self.term_zen, sel: self.term_sel }),
            tree: self.tree.as_ref().map(|t| {
                let first = t.sel.saturating_sub(100);
                TreeView {
                    root: t.raiz.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| t.raiz.display().to_string()),
                    items: t.nos.iter().skip(first).take(240).map(|n| TreeItem { name: n.nome.clone(), level: n.nivel, dir: n.pasta, open: n.aberta, git: self.tree_git.get(&n.caminho).copied().unwrap_or(' ') }).collect(),
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
                buscando: p.listando.is_some() || p.buscando.is_some(),
                preview: self.pick_preview(p),
            }),
            hits,
            hits_id,
            flash: (self.ed.last_yank.0 > 0).then(|| {
                let (id, a, b, _) = self.ed.last_yank;
                (mapa_pos(a.row, a.col), mapa_pos(b.row, b.col), id)
            }),
            clip: self.clip.clone(),
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
    /// formatar com o servidor ao gravar (`[lsp] format_on_save`)
    formatar: bool,
    /// `clipboard=unnamedplus` nas sessões novas
    clipboard_mais: bool,
    /// mapeamentos do usuário (do `config.toml` e dos `:map`) e a versão que cada editor já tem
    mapas: Vec<astrograph_core::mapas::Mapa>,
    mapas_v: u64,
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
        Workspace { abas: vec![s], ativa: 0, pool, notify: Some(notify), leader, formatar: false, clipboard_mais: false, mapas: Vec::new(), mapas_v: 0, dims: (80, 24.0), sair: false }
    }

    fn atual(&mut self) -> &mut Session {
        &mut self.abas[self.ativa]
    }

    /// Aplica a cada editor (e aos das próximas sessões) os mapeamentos do `config.toml`.
    pub fn aplicar_mapas(&mut self, mapas: &[(u8, String, String)]) {
        let leader = self.leader;
        {
            let ed = &mut self.abas[self.ativa].ed;
            ed.set_leader(leader);
            for (modos, lhs, rhs) in mapas {
                ed.mapear(*modos, lhs, rhs, false);
            }
            self.mapas = ed.mapas();
            self.mapas_v = ed.mapas_versao();
        }
        self.sincronizar_mapas(true);
    }

    /// `clipboard=unnamedplus`: copiar e colar sem registrador usa o clipboard do sistema.
    pub fn clipboard_unnamedplus(&mut self, ligado: bool) {
        self.clipboard_mais = ligado;
        for a in self.abas.iter_mut() {
            a.ed.clipboard_unnamedplus = ligado;
            if let Some(sp) = a.splits.as_mut() {
                for o in sp.outros.iter_mut() {
                    o.ed.clipboard_unnamedplus = ligado;
                }
            }
        }
    }

    /// O app diz o que está no clipboard do sistema.
    pub fn clipboard_do_sistema(&mut self, texto: &str) {
        for a in self.abas.iter_mut() {
            a.ed.definir_clipboard(Some(texto.to_string()));
            if let Some(sp) = a.splits.as_mut() {
                for o in sp.outros.iter_mut() {
                    o.ed.definir_clipboard(Some(texto.to_string()));
                }
            }
        }
    }

    /// Copia a tabela de mapeamentos do editor focado para os outros quando ela mudou (`:nnoremap`).
    fn sincronizar_mapas(&mut self, forcar: bool) {
        let v = self.abas[self.ativa].ed.mapas_versao();
        if !forcar && v == self.mapas_v {
            return;
        }
        let m = self.abas[self.ativa].ed.mapas();
        self.mapas = m.clone();
        self.mapas_v = v;
        for a in self.abas.iter_mut() {
            a.ed.definir_mapas(m.clone(), v);
            if let Some(sp) = a.splits.as_mut() {
                for o in sp.outros.iter_mut() {
                    o.ed.definir_mapas(m.clone(), v);
                }
            }
        }
    }

    /// Liga ou desliga o formatar-ao-salvar em todas as sessões (e nas que vierem).
    pub fn formatar_ao_salvar(&mut self, ligado: bool) {
        self.formatar = ligado;
        for a in self.abas.iter_mut() {
            a.formatar_ao_salvar = ligado;
            if let Some(sp) = a.splits.as_mut() {
                for o in sp.outros.iter_mut() {
                    o.formatar_ao_salvar = ligado;
                }
            }
        }
    }

    fn nova_sessao(&self, arquivo: Option<&Path>) -> Session {
        let mut s = match arquivo {
            Some(p) => Session::open(p),
            None => Session::new("", None),
        };
        s.lsp_pool = self.pool.clone();
        s.set_leader(self.leader);
        s.formatar_ao_salvar = self.formatar;
        s.ed.definir_mapas(self.mapas.clone(), self.mapas_v);
        s.ed.clipboard_unnamedplus = self.clipboard_mais;
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
                    PedidoAba::NovaComTexto(titulo, t) => {
                        let mut nova = self.nova_sessao(None);
                        nova.ed.sincronizar(&ropey::Rope::from_str(&t));
                        nova.saved_rev = nova.ed.buf.rev;
                        nova.nome_fixo = Some(titulo);
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
            if let Some((titulo, t)) = s.aba_texto.take() {
                s.aba_pedido = Some(PedidoAba::NovaComTexto(titulo, t));
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
        self.sincronizar_mapas(false);
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

    pub fn mouse_esq(&mut self, acao: &str, row: usize, col: usize, shift: bool) {
        self.atual().mouse_esq(acao, row, col, shift);
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
        // respostas do servidor chegam sozinhas (renomear, formatar e gravar, `:wq`...): resolve já
        let s = &self.abas[self.ativa];
        if s.edicoes_pedido.is_some() || s.quit || s.abrir_pedido.is_some() || s.aba_pedido.is_some() || s.jan_pedido.is_some() || s.aba_texto.is_some() {
            self.depois();
            if !self.sair {
                vm = self.abas[self.ativa].view_model();
            }
        }
        vm.tabs = self
            .abas
            .iter()
            .map(|s| crate::nview::TabInfo { name: s.nome_fixo.clone().or_else(|| s.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "Sem nome".into()), modified: s.modified() })
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
    formatar_ao_salvar: bool,
    mapeamentos: Vec<(u8, String, String)>,
    clipboard_mais: bool,
    tx: tokio::sync::mpsc::UnboundedSender<crate::UiCommand>,
) {
    use crate::UiCommand;
    let notify: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || {
        let _ = tx.send(crate::UiCommand::TermOut);
    });
    let mut w = Workspace::new(file, leader, notify);
    w.formatar_ao_salvar(formatar_ao_salvar);
    w.aplicar_mapas(&mapeamentos);
    w.clipboard_unnamedplus(clipboard_mais);
    while let Some(cmd) = rx.recv().await {
        match cmd {
            UiCommand::Keys(k) => w.feed(&k),
            UiCommand::Resize(c, r) => w.resize(c.max(1) as usize, r.max(1) as f32),
            UiCommand::Focus(_) => continue,
            UiCommand::Scroll(lines) => w.scroll(lines as f32),
            UiCommand::TermOut => {}
            UiCommand::TermSize(r, c) => w.term_resize(r.max(2) as u16, c.max(10) as u16),
            UiCommand::Tab(i) => w.ir_para(i.max(0) as usize),
            UiCommand::TermMouse { acao, botao, row, col, shift } => w.atual().term_mouse(acao, botao, row.max(0) as usize, col.max(0) as usize, shift),
            UiCommand::Paste(t) => w.atual().colar(&t),
            UiCommand::ClipSync(t) => w.clipboard_do_sistema(&t),
            UiCommand::TermCopy => w.atual().term_copiar(),
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
            UiCommand::Mouse { button, action, row, col, modifier } => match (button, action) {
                ("left", "press" | "drag" | "release") => w.mouse_esq(action, row.max(0) as usize, col.max(0) as usize, modifier.contains("S-")),
                _ => continue,
            },
        }
        if w.sair {
            let _ = proxy.send_event(crate::UserEvent::Quit);
            return;
        }
        *shared.lock().unwrap() = Arc::new(w.view_model());
        // `:wq` com formatação assíncrona termina aqui, quando o servidor responde
        if w.sair {
            let _ = proxy.send_event(crate::UserEvent::Quit);
            return;
        }
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
        // a lista de arquivos vem de uma thread: espera chegar
        let m = espera_term(&mut s, |m| m.picker.as_ref().is_some_and(|p| p.items.len() == 3 && !p.buscando));
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
    fn enter_antes_da_lista_chegar_abre_o_arquivo_quando_ela_chega() {
        let d = projeto("ff-enter");
        let mut s = Session::new("x\n", None);
        s.root = d;
        s.resize(80, 20.0);
        // tudo numa tecla só: o Enter vem antes de a thread trazer a lista
        s.feed(" ffbeta<CR>");
        let m = espera_term(&mut s, |m| m.name == "beta.rs");
        assert_eq!(m.name, "beta.rs");
        assert!(m.picker.is_none());
    }

    #[test]
    fn sinais_do_git_de_arquivo_grande_vem_de_outra_thread_e_nao_travam() {
        let linhas: Vec<String> = (0..8000).map(|i| format!("linha {i}")).collect();
        let mut s = Session::new(&linhas.join("\n"), None);
        s.resize(80, 20.0);
        s.git_branch = Some("main".into());
        s.git_base = Some(std::sync::Arc::new(linhas.clone()));
        s.git_rev = u64::MAX;
        assert!(modelo(&mut s).git.unwrap().signs.is_empty(), "igual ao HEAD: sem sinais");
        s.feed("ggAX<Esc>");
        // o cálculo é em segundo plano: os sinais aparecem pouco depois
        let m = espera_term(&mut s, |m| m.git.as_ref().is_some_and(|g| !g.signs.is_empty()));
        assert!(m.git.unwrap().signs.contains(&(0, git::Sign::Changed)));
        // desfazer: voltam a ficar vazios
        s.feed("u");
        let m = espera_term(&mut s, |m| m.git.as_ref().is_some_and(|g| g.signs.is_empty()));
        assert!(m.git.unwrap().signs.is_empty());
    }

    #[test]
    fn espaco_fw_busca_texto_e_pula_para_a_linha() {
        let d = projeto("fw");
        let mut s = Session::new("x\n", None);
        s.root = d;
        s.resize(80, 20.0);
        s.feed(" fwagulha");
        let pv = espera_term(&mut s, |m| m.picker.as_ref().is_some_and(|p| p.items.len() == 1 && !p.buscando)).picker.unwrap();
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
    fn editorconfig_define_o_recuo_e_limpa_ao_gravar() {
        let d = projeto("ec");
        std::fs::write(d.join(".editorconfig"), "root = true\n[*.rs]\nindent_style = space\nindent_size = 3\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n").unwrap();
        let arq = d.join("src/ec.rs");
        std::fs::write(&arq, "fn a() {   \nx\n}").unwrap();
        let mut s = Session::open(&arq);
        s.resize(80, 20.0);
        // `>>` recua 3 espaços (shiftwidth e expandtab do .editorconfig)
        s.feed("j>>");
        assert_eq!(s.ed.buf.line(1), "   x");
        s.feed(":w<CR>");
        let gravado = std::fs::read_to_string(&arq).unwrap();
        assert_eq!(gravado, "fn a() {\n   x\n}\n", "sem espaço no fim das linhas e com a quebra final");
    }

    #[test]
    fn reabrir_um_arquivo_volta_para_onde_o_cursor_estava() {
        let d = projeto("cursor");
        let arq = d.join("src/volta.rs");
        std::fs::write(&arq, "um\ndois\ntres\nquatro\n").unwrap();
        let mut s = Session::open(&arq);
        s.resize(80, 20.0);
        s.feed("3Gl");
        s.feed(":q<CR>");
        assert!(s.quit);
        let s2 = Session::open(&arq);
        assert_eq!((s2.ed.cursor.row, s2.ed.cursor.col), (2, 1));
    }

    #[test]
    fn snippet_pula_entre_paradas_com_tab_e_troca_o_valor_padrao() {
        use crate::lsp::Item;
        let mut s = Session::new("fn x() {\n    ma\n}\n", None);
        s.resize(80, 20.0);
        s.ed.set_cursor(1, 5);
        s.feed("A");
        assert_eq!(s.ed.cursor.col, 6);
        let item = Item {
            label: "main".into(),
            filtro: "main".into(),
            texto: "main(a, b)".into(),
            detalhe: String::new(),
            tipo: 3,
            inicio: None,
            paradas: vec![(5, 6), (8, 9), (10, 10)],
            extras: vec![((0, 0), (0, 0), "use std::io;\n".into())],
        };
        s.comp = Some(Comp { start: Pos::new(1, 4), items: vec![item], shown: vec![(0, vec![])], sel: 0 });
        s.comp_accept();
        // o import entrou acima (as paradas e o cursor desceram uma linha) e `a` está selecionado
        assert_eq!(s.ed.buf.line(0), "use std::io;");
        assert_eq!(s.ed.buf.line(2), "    main(a, b)");
        let m = modelo(&mut s);
        assert_eq!(m.selection, Some((VKind::Char, (2, 9), (2, 9))));
        // digitar troca o valor padrão; Tab vai para `b`, que vem deslocado
        s.feed("foo<Tab>");
        assert_eq!(s.ed.buf.line(2), "    main(foo, b)");
        let m = modelo(&mut s);
        assert_eq!(m.selection, Some((VKind::Char, (2, 14), (2, 14))));
        s.feed("barra<Tab>");
        assert_eq!(s.ed.buf.line(2), "    main(foo, barra)");
        // última parada (`$0`, o fim do texto inserido): encerra o modo
        assert!(s.snip.is_none());
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (2, 20));
        s.feed("<Esc>");
        assert_eq!(s.ed.mode(), Mode::Normal);
    }

    #[test]
    fn buscador_mostra_o_trecho_do_arquivo_selecionado() {
        let d = projeto("preview");
        std::fs::write(d.join("src/alfa.rs"), (1..=40).map(|i| format!("linha {i}\n")).collect::<String>()).unwrap();
        let mut s = Session::new("x\n", None);
        s.root = d;
        s.resize(80, 20.0);
        s.feed(" ffalfa");
        let pv = espera_term(&mut s, |m| m.picker.as_ref().is_some_and(|p| !p.items.is_empty() && !p.preview.is_empty())).picker.unwrap();
        assert_eq!(pv.preview.len(), PICK_PREVIEW);
        assert_eq!(pv.preview[0], (1, "linha 1".to_string(), false), "arquivo sem posição começa no topo");
        // na busca de texto o trecho se centra na linha achada e a marca
        s.feed("<Esc> fwlinha 30");
        let pv = espera_term(&mut s, |m| m.picker.as_ref().is_some_and(|p| p.items.iter().any(|i| i.text.ends_with(":30")) && !p.preview.is_empty())).picker.unwrap();
        let marcada: Vec<_> = pv.preview.iter().filter(|l| l.2).collect();
        assert_eq!(marcada.len(), 1);
        assert_eq!(marcada[0].1, "linha 30");
    }

    #[test]
    fn explorador_marca_o_estado_do_git() {
        let d = projeto("tree-git");
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git").args(args).current_dir(&d).output().unwrap();
            assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "x"]);
        std::fs::write(d.join("src/alfa.rs"), "mudou\n").unwrap();
        std::fs::write(d.join("novo.txt"), "").unwrap();
        let mut s = Session::new("x\n", None);
        s.root = d.clone();
        s.resize(80, 20.0);
        s.feed(" e");
        let t = espera_term(&mut s, |m| m.tree.as_ref().is_some_and(|t| t.items.iter().any(|i| i.git != ' '))).tree.unwrap();
        let g = |nome: &str| t.items.iter().find(|i| i.name == nome).map(|i| i.git);
        assert_eq!(g("src"), Some('M'), "a pasta herda o estado do que há dentro");
        assert_eq!(g("novo.txt"), Some('?'));
        s.feed("l");
        let t = modelo(&mut s).tree.unwrap();
        assert_eq!(t.items.iter().find(|i| i.name == "alfa.rs").map(|i| i.git), Some('M'));
        assert_eq!(t.items.iter().find(|i| i.name == "beta.rs").map(|i| i.git), Some(' '));
    }

    #[test]
    fn explorador_cria_renomeia_copia_cola_e_apaga() {
        let d = projeto("tree-ops");
        let mut s = Session::new("x\n", None);
        s.root = d.clone();
        s.resize(80, 20.0);
        s.feed(" e");
        // a: arquivo novo dentro de src (a pasta selecionada)
        s.feed("a");
        let m = modelo(&mut s);
        assert_eq!(m.cmd.as_ref().map(|c| c.0), Some('›'));
        assert_eq!(m.mode, ModeKind::Command);
        s.feed("novo.rs<CR>");
        assert!(d.join("src/novo.rs").is_file());
        let t = modelo(&mut s).tree.unwrap();
        assert_eq!(t.items[t.sel].name, "novo.rs", "o novo item fica selecionado");
        // r: renomear (o campo já vem com o nome)
        s.feed("r");
        assert!(modelo(&mut s).cmd.unwrap().1.ends_with("novo.rs"));
        s.feed("<BS><BS>txt<CR>");
        assert!(d.join("src/novo.txt").is_file() && !d.join("src/novo.rs").exists());
        // y e p: copia para o lado
        s.feed("yp");
        assert!(d.join("src/novo_copia.txt").is_file());
        // d: pede confirmação; n cancela, s apaga
        s.feed("dn<CR>");
        assert!(d.join("src/novo_copia.txt").is_file());
        s.feed("ds<CR>");
        assert!(!d.join("src/novo_copia.txt").exists());
        // x e p: move para a raiz (o foco vai para LEIA.md, que é arquivo: cola ao lado dele)
        s.feed("gjjjx");
        s.feed("Gp");
        let t = modelo(&mut s).tree.unwrap();
        assert!(t.items.iter().any(|i| i.name == "novo.txt" && i.level == 0), "{:?}", t.items.iter().map(|i| (&i.name, i.level)).collect::<Vec<_>>());
    }

    #[test]
    fn objetos_de_texto_de_tree_sitter_com_operador_e_no_visual() {
        let d = projeto("objs");
        let arq = d.join("src/obj.rs");
        std::fs::write(&arq, "fn a() {\n    let x = 1;\n    let y = 2;\n}\n\nfn b(p: i32, q: i32) {}\n").unwrap();
        let mut s = Session::open(&arq);
        s.resize(80, 20.0);
        // daf: apaga a função inteira em que o cursor está
        s.feed("jdaf");
        assert_eq!(s.ed.buf.text(), "\nfn b(p: i32, q: i32) {}", "{:?}", s.ed.buf.text());
        s.feed("u");
        // yif: o corpo da função (sem as chaves)
        s.feed("jyif");
        assert_eq!(s.ed.register('"').map(|r| r.text.clone()), Some("let x = 1;\n    let y = 2;".to_string()));
        // visual: vaf seleciona a função toda
        s.feed("vaf");
        let (k, anc) = s.ed.selection().unwrap();
        assert_eq!((k, anc, s.ed.cursor), (VKind::Char, Pos::new(0, 0), Pos::new(3, 0)));
        s.feed("<Esc>");
        // dia: o parâmetro sob o cursor (e a vírgula que o separa)
        s.feed("5Gwwwdia");
        assert!(s.ed.buf.line(5).starts_with("fn b("), "{:?}", s.ed.buf.line(5));
    }

    #[test]
    fn gf_abre_o_arquivo_sob_o_cursor_com_linha_e_coluna() {
        let d = projeto("gf");
        let arq = d.join("src/alfa.rs");
        std::fs::write(&arq, "veja beta.rs:2:3 e LEIA.md depois ./nao_existe.txt\n").unwrap();
        std::fs::write(d.join("src/beta.rs"), "um\ndois\ntres\n").unwrap();
        let mut s = Session::open(&arq);
        s.root = d.clone();
        s.resize(80, 20.0);
        // sobre `beta.rs:2:3` (relativo ao arquivo atual): abre na linha 2, coluna 3
        s.feed("wgf");
        assert_eq!(s.ed.buf.line(0), "um");
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (1, 2));
        // sobre um nome que não existe: avisa
        let mut s = Session::open(&arq);
        s.root = d.clone();
        s.resize(80, 20.0);
        s.feed("$gf");
        assert!(modelo(&mut s).toast.is_some_and(|t| t.2 == ToastKind::Error && t.1.contains("não encontrado")));
        // a raiz do projeto também vale
        s.feed("0fLgf");
        assert_eq!(s.ed.buf.line(0), "oi");
    }

    #[test]
    fn comando_astrograph_saude_atalhos_e_subcomando_desconhecido() {
        let mut w = Workspace::new(None, ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.feed(":Astrograph saude<CR>");
        let m = w.view_model();
        assert_eq!(m.name, "Saúde do Astrograph");
        assert!(m.rope.to_string().contains("Agentes de IA"));
        w.feed(":Astrograph atalhos<CR>");
        let m = w.view_model();
        assert_eq!(m.name, "Atalhos");
        assert!(m.rope.to_string().contains("<Espaço>ff"));
        assert_eq!(m.tabs.len(), 3);
        w.feed(":Astrograph foo<CR>");
        assert!(w.view_model().toast.is_some_and(|t| t.1.contains("desconhecido")));
    }

    #[test]
    fn dobras_fechadas_somem_do_texto_e_as_coordenadas_acompanham() {
        let texto: String = (0..10).map(|i| format!("linha {i}\n")).collect();
        let mut s = Session::new(&texto, None);
        s.resize(80, 20.0);
        // zfj na linha 2 (real 1): dobra das linhas 1 e 2, fechada
        s.feed("jzfj");
        let m = modelo(&mut s);
        assert_eq!(m.lines, 9);
        assert_eq!(m.line(1), "linha 1  ⋯ 2 linhas");
        assert_eq!(m.line(2), "linha 3", "a linha 2 do arquivo está escondida");
        assert_eq!(m.cursor, (1, 0));
        assert_eq!(m.row_map.as_ref().map(|r| r[..4].to_vec()), Some(vec![0, 1, 3, 4]), "a margem usa os números do arquivo");
        // o aviso da dobra vem em cinza (comentário)
        assert!(m.spans.iter().flatten().any(|sp| sp.cls == crate::design::Cls::Comment && sp.start == 7), "{:?}", m.spans);
        // j pula a dobra; k volta nela
        s.feed("j");
        assert_eq!((s.ed.cursor.row, modelo(&mut s).cursor), (3, (2, 0)));
        s.feed("k");
        assert_eq!(s.ed.cursor.row, 1);
        // clicar na linha mostrada 2 leva ao arquivo, linha 3
        s.click(2, 0);
        assert_eq!(s.ed.cursor.row, 3);
        // dd na dobra apaga as duas linhas de uma vez
        s.feed("kdd");
        assert_eq!(s.ed.buf.line(1), "linha 3");
        assert_eq!(modelo(&mut s).lines, 8);
        // zR abre tudo: a tela volta a mostrar as linhas todas
        s.feed("uzR");
        let m = modelo(&mut s);
        assert!(m.row_map.is_none());
        assert_eq!((m.lines, m.line(2).as_str()), (10, "linha 2"));
    }

    #[test]
    fn foldmethod_indent_cria_dobras_abertas_que_zm_fecha() {
        let texto = "fn a() {\n    if x {\n        y();\n        z();\n    }\n    w();\n}\nfim\n";
        let mut s = Session::new(texto, None);
        s.resize(80, 20.0);
        s.feed(":set shiftwidth=4<CR>");
        s.feed(":set foldmethod=indent<CR>");
        assert_eq!(modelo(&mut s).lines, 8, "abertas: nada some");
        assert!(!s.ed.dobras().is_empty());
        // zM fecha tudo: só sobram a linha do `fn a`... (a dobra de nível 1 cobre as linhas 1 a 5)
        s.feed("zM");
        let m = modelo(&mut s);
        assert_eq!(m.line(0), "fn a() {");
        assert_eq!(m.line(1), "    if x {  ⋯ 5 linhas");
        assert_eq!(m.lines, 4);
        // zR abre; o texto editado refaz as dobras
        s.feed("zR");
        assert_eq!(modelo(&mut s).lines, 8);
        s.feed("Gdd");
        assert_eq!(modelo(&mut s).lines, 7);
        // sintaxe: funções fecham em uma linha
        let mut s = Session::open(&{
            let d = projeto("foldsyn");
            let f = d.join("src/f.rs");
            std::fs::write(&f, "fn a() {\n    let x = 1;\n    x\n}\n\nfn b() {\n    2\n}\n").unwrap();
            f
        });
        s.resize(80, 20.0);
        s.feed(":set foldmethod=syntax<CR>");
        s.feed("zM");
        let m = modelo(&mut s);
        assert_eq!(m.line(0), "fn a() {  ⋯ 4 linhas");
        assert_eq!(m.lines, 3, "{:?}", m.rope.to_string());
    }

    /// `cargo test --release bench_arquivo_grande -- --ignored --nocapture`: custo de abrir, rolar, editar, buscar
    /// e trocar texto num arquivo de 200 mil linhas.
    #[test]
    #[ignore]
    fn bench_arquivo_grande() {
        let n = 200_000;
        let texto: String = (0..n).map(|i| format!("    let valor_{i} = calcular({i}, \"texto numero {i}\"); // comentario\n")).collect();
        let t = std::time::Instant::now();
        let mut s = Session::new(&texto, None);
        s.resize(120, 40.0);
        let abrir = t.elapsed();
        let medir = |s: &mut Session, teclas: &str, vezes: u32| {
            let t = std::time::Instant::now();
            for _ in 0..vezes {
                s.feed(teclas);
                let _ = s.view_model();
            }
            t.elapsed() / vezes
        };
        let primeiro = { let t = std::time::Instant::now(); let _ = s.view_model(); t.elapsed() };
        let j = medir(&mut s, "j", 200);
        let pagina = medir(&mut s, "20j", 100);
        let g = medir(&mut s, "Ggg", 20);
        let digitar = medir(&mut s, "Ax<Esc>", 100);
        let desfazer = medir(&mut s, "u", 50);
        let busca = medir(&mut s, "/valor_199999<CR>gg", 3);
        let troca = {
            let t = std::time::Instant::now();
            s.feed(":%s/calcular/computar/g<CR>");
            let _ = s.view_model();
            t.elapsed()
        };
        let dd = medir(&mut s, "dd", 100);
        println!("BENCH {n} linhas: abrir {abrir:?} · 1º quadro {primeiro:?} · j {j:?} · 20j {pagina:?} · G+gg {g:?} · digitar {digitar:?} · u {desfazer:?} · /busca {busca:?} · :%s {troca:?} · dd {dd:?}");
    }

    #[test]
    fn quebra_de_linha_mostra_trechos_e_leva_o_cursor_e_o_clique_ao_lugar_certo() {
        let mut s = Session::new("curta\nabcdefghijklmnopqrstuvwxyz0123456789\nfim\n", None);
        s.resize(20, 20.0);
        // sem wrap: as três linhas
        assert_eq!(modelo(&mut s).lines, 3);
        s.feed(":set wrap<CR>");
        let m = modelo(&mut s);
        // a linha de 36 caracteres vira dois trechos de 20 e 16
        assert_eq!(m.lines, 4);
        assert_eq!(m.line(1), "abcdefghijklmnopqrst");
        assert_eq!(m.line(2), "uvwxyz0123456789");
        assert_eq!(m.row_cont.as_ref().map(|c| c.to_vec()), Some(vec![false, false, true, false]));
        assert_eq!(m.row_map.as_ref().map(|c| c.to_vec()), Some(vec![0, 1, 1, 2]));
        // o cursor no fim da linha longa cai no segundo trecho
        s.feed("j$");
        let m = modelo(&mut s);
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (1, 35));
        assert_eq!(m.cursor, (2, 15));
        assert_eq!(m.cursor_real, (1, 35));
        // clicar no segundo trecho leva ao caractere certo do arquivo
        s.click(2, 3);
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (1, 23));
        // clicar depois do fim do primeiro trecho para no último caractere dele
        s.click(1, 50);
        assert_eq!((s.ed.cursor.row, s.ed.cursor.col), (1, 19));
        // realce de busca e seleção também vão para os trechos
        s.feed("/xyz0<CR>");
        let m = modelo(&mut s);
        assert_eq!(m.hits.iter().map(|h| (h.0, h.1, h.2)).collect::<Vec<_>>(), vec![(2, 3, 7)]);
        // nowrap volta ao normal
        s.feed(":set nowrap<CR>");
        let m = modelo(&mut s);
        assert_eq!((m.lines, m.row_map.is_none()), (3, true));
    }

    #[test]
    fn menu_de_atalhos_aparece_depois_de_esperar_a_lider() {
        let mut s = Session::new("abc\n", None);
        s.resize(80, 20.0);
        s.feed(" ");
        assert!(modelo(&mut s).whichkey.is_empty(), "logo depois da líder ainda não mostra");
        s.leader_desde = Some(std::time::Instant::now() - std::time::Duration::from_millis(LIDER_MENU_MS + 10));
        let m = modelo(&mut s);
        assert!(m.whichkey.contains(&("e".into(), "Explorador de arquivos".into())));
        assert!(m.whichkey.contains(&("f".into(), "+Buscar".into())));
        // um nível abaixo
        s.feed("A");
        let m = modelo(&mut s);
        assert!(m.whichkey.iter().any(|(k, d)| k == "z" && d == "Modo zen"), "{:?}", m.whichkey);
        // esc fecha
        s.feed("<Esc>");
        assert!(modelo(&mut s).whichkey.is_empty());
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

    /// Pasta com agentes falsos (`claude`, `opencode`) que escrevem os argumentos e a pasta e ecoam o que
    /// recebem; vai na frente do PATH enquanto a trava estiver nas mãos.
    #[cfg(unix)]
    fn agentes_falsos(nome: &str) -> (PathBuf, String) {
        use std::os::unix::fs::PermissionsExt;
        let bin = std::env::temp_dir().join(format!("astrograph-bin-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bin);
        std::fs::create_dir_all(&bin).unwrap();
        for a in ["claude", "opencode"] {
            let f = bin.join(a);
            std::fs::write(&f, format!("#!/bin/sh\necho \"{a} ARGS:$* PWD:$(pwd)\"\nexec cat\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let antes = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{antes}", bin.display()));
        (bin, antes)
    }

    #[cfg(unix)]
    #[test]
    fn agente_escolher_modo_e_abrir_no_terminal_com_o_modo() {
        let _t = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let (_bin, path_antes) = agentes_falsos("escolha");
        let cfg = std::env::temp_dir().join(format!("astrograph-cfg-{}-escolha", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        let mut s = Session::new("x\n", None);
        s.root = std::env::temp_dir();
        s.agentes_pasta = Some(cfg.clone());
        s.agentes_tem = Some(vec!["claude", "opencode"]);
        s.resize(80, 20.0);
        // <Líder>Ag lista os instalados, com o atual marcado
        s.feed(" Ag");
        let p = modelo(&mut s).picker.expect("lista de agentes");
        assert_eq!(p.title, "Agente de IA");
        assert_eq!(p.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), vec!["● Claude Code", "  opencode"]);
        s.feed("<C-n><CR>");
        assert_eq!(crate::agentes::ler_config(&cfg)["agent"], "opencode", "a escolha fica no config.json");
        // <Líder>Aw: o modo; escolher Plano
        s.feed(" Aw");
        let p = modelo(&mut s).picker.expect("lista de modos");
        assert_eq!((p.title.as_str(), p.items.len(), p.items[0].text.as_str()), ("Modo do agente", 5, "● Manual"));
        s.feed("<C-n><C-n><CR>");
        assert_eq!(crate::agentes::ler_config(&cfg)["agent_mode"], "plan");
        // abrir: opencode no Plano não tem opção própria (abre no padrão e avisa)
        s.feed(" Al");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l.starts_with("opencode ARGS:")));
        assert!(linhas_do_terminal(&m).iter().any(|l| l.starts_with("opencode ARGS: ")), "{:?}", linhas_do_terminal(&m));
        assert!(m.toast.is_some_and(|t| t.1.contains("não tem o modo")));
        // de volta ao Claude: o modo vira `--permission-mode plan`
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        s.feed(" T");
        s.feed(" Ag");
        let p = modelo(&mut s).picker.expect("lista de novo");
        assert_eq!(p.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), vec!["  Claude Code", "● opencode"], "sel {}", p.sel);
        s.feed("<CR>");
        assert_eq!(crate::agentes::ler_config(&cfg)["agent"], "claude");
        s.feed(" Al");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l.contains("--permission-mode plan")));
        assert!(linhas_do_terminal(&m).iter().any(|l| l.starts_with("claude ARGS:--permission-mode plan")), "{:?}", linhas_do_terminal(&m));
        s.term_close();
        std::env::set_var("PATH", path_antes);
    }

    #[cfg(unix)]
    #[test]
    fn pedidos_prontos_corrigir_explicar_revisar_commit_vao_ao_agente() {
        let _t = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let (_bin, path_antes) = agentes_falsos("pedidos");
        let d = projeto("pedidos");
        let arq = d.join("src/alfa.rs");
        let mut s = Session::open(&arq);
        s.root = d.clone();
        s.agentes_pasta = Some(d.join("cfg"));
        s.agentes_tem = Some(vec!["claude"]);
        s.resize(80, 20.0);
        s.term_resize(14, 100);
        // sem diagnóstico na linha: avisa e não abre o agente
        s.feed(" Ax");
        assert!(modelo(&mut s).term.is_none());
        assert!(modelo(&mut s).toast.is_some_and(|t| t.1.contains("Sem diagnóstico")));
        // com diagnóstico: abre o agente e entrega o pedido quando ele terminar de subir
        s.lsp_diags = std::sync::Arc::new(vec![lsp::Diag { row: 0, col: 3, end_row: 0, end_col: 8, severity: 1, message: "tipo errado aqui".into() }]);
        s.feed(" Ax");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l.contains("tipo errado aqui")));
        let txt = linhas_do_terminal(&m).join("\n");
        assert!(txt.contains("claude ARGS:--permission-mode default"), "{txt}");
        assert!(txt.contains("Corrija o erro na linha 1 de src/alfa.rs: tipo errado aqui"), "{txt}");
        // com o agente já aberto, os outros pedidos vão na hora
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        s.feed(" Ar");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).join("\n").contains("Revise as alterações"));
        assert!(linhas_do_terminal(&m).join("\n").contains("Revise as alterações"));
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        s.feed(" Am");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).join("\n").contains("mensagem de commit"));
        assert!(linhas_do_terminal(&m).join("\n").contains("mensagem de commit"));
        // explicar: a linha do cursor; na seleção visual, o trecho
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        s.feed("Vj<Esc>ggVj");
        s.feed(" Ai");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).join("\n").contains("linhas 1 a 2 de src/alfa.rs"));
        assert!(linhas_do_terminal(&m).join("\n").contains("linhas 1 a 2 de src/alfa.rs"), "{:?}", linhas_do_terminal(&m));
        s.term_close();
        std::env::set_var("PATH", path_antes);
    }

    #[cfg(unix)]
    #[test]
    fn citar_arquivo_e_selecao_no_agente_sem_enviar() {
        let _t = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let (_bin, path_antes) = agentes_falsos("citar");
        let d = projeto("citar");
        let arq = d.join("src/alfa.rs");
        let mut s = Session::open(&arq);
        s.root = d.clone();
        s.agentes_pasta = Some(d.join("cfg"));
        s.agentes_tem = Some(vec!["claude"]);
        s.resize(80, 20.0);
        s.term_resize(14, 100);
        // sem seleção: o arquivo inteiro; o agente abre e recebe a citação (sem Enter)
        s.feed(" Ac");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l.contains("@src/alfa.rs")));
        assert!(linhas_do_terminal(&m).iter().any(|l| l.contains("@src/alfa.rs")), "{:?}", linhas_do_terminal(&m));
        // com seleção: as linhas
        s.feed("<C-Bslash>");
        s.feed("<C-n>");
        s.feed("Vj");
        s.feed(" Ac");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).join("\n").contains("@src/alfa.rs#L1-2"));
        assert!(linhas_do_terminal(&m).join("\n").contains("@src/alfa.rs#L1-2"), "{:?}", linhas_do_terminal(&m));
        s.term_close();
        std::env::set_var("PATH", path_antes);
    }

    #[cfg(unix)]
    #[test]
    fn retomar_conversa_do_claude_na_pasta_dela() {
        let _t = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let (_bin, path_antes) = agentes_falsos("sessoes");
        let home = std::env::temp_dir().join(format!("astrograph-home-{}", std::process::id()));
        let projeto_a = home.join("proj a");
        let dir_claude = home.join(".claude/projects/-proj-a");
        std::fs::create_dir_all(&dir_claude).unwrap();
        std::fs::create_dir_all(&projeto_a).unwrap();
        std::fs::write(
            dir_claude.join("abc-123.jsonl"),
            format!(r#"{{"type":"user","cwd":"{}","message":{{"role":"user","content":"arruma o login"}}}}"#, projeto_a.display()),
        )
        .unwrap();
        let home_antes = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let mut s = Session::new("x\n", None);
        s.root = std::env::temp_dir();
        s.agentes_pasta = Some(home.join("cfg"));
        s.agentes_tem = Some(vec!["claude"]);
        s.resize(80, 20.0);
        s.feed(" AS");
        let p = modelo(&mut s).picker.expect("conversas");
        assert_eq!((p.title.as_str(), p.items.len(), p.items[0].text.as_str()), ("Conversas do agente", 1, "arruma o login"));
        assert!(p.items[0].detail.starts_with("proj a · "), "{}", p.items[0].detail);
        s.feed("<CR>");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l.contains("--resume abc-123")));
        let txt = linhas_do_terminal(&m).join("\n");
        assert!(txt.contains("--resume abc-123"), "{txt}");
        assert!(txt.contains(&format!("PWD:{}", projeto_a.display())) || txt.contains("PWD:") && txt.contains("proj a"), "abre na pasta do projeto: {txt}");
        s.term_close();
        match home_antes {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        std::env::set_var("PATH", path_antes);
    }

    #[test]
    fn arquivo_editado_por_fora_recarrega_sem_perder_alteracoes_nao_salvas() {
        let d = projeto("recarrega");
        let arq = d.join("src/alfa.rs");
        let mut s = Session::open(&arq);
        s.resize(80, 20.0);
        modelo(&mut s);
        // o agente (outro processo) edita o arquivo: sem alterações suas, o texto novo entra
        std::thread::sleep(std::time::Duration::from_millis(800));
        std::fs::write(&arq, "fn novo() {}\nfn outro() {}\n").unwrap();
        let m = espera_term(&mut s, |m| m.rope.to_string().starts_with("fn novo"));
        assert_eq!(m.rope.to_string(), "fn novo() {}\nfn outro() {}");
        assert!(!m.modified, "recarregado = igual ao disco");
        // com alterações não salvas: não recarrega, só avisa
        s.feed("ggx");
        std::thread::sleep(std::time::Duration::from_millis(800));
        std::fs::write(&arq, "fn terceiro() {}\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(800));
        let m = modelo(&mut s);
        assert!(m.rope.to_string().starts_with("n novo"), "{:?}", m.rope.to_string());
        assert!(m.modified);
        assert!(m.toast.is_some_and(|t| t.2 == ToastKind::Error && t.1.contains("mudou no disco")));
        // gravar pelo editor não conta como mudança de fora
        s.feed(":w<CR>");
        std::thread::sleep(std::time::Duration::from_millis(800));
        let m = modelo(&mut s);
        assert!(!m.modified);
        assert!(m.rope.to_string().starts_with("n novo"));
    }

    #[test]
    fn mouse_arrasta_para_selecionar_duplo_clique_palavra_triplo_linha_e_shift_estende() {
        let mut s = Session::new("hello world foo\nsecond line\n", None);
        s.resize(80, 20.0);
        // clique posiciona
        s.mouse_esq("press", 0, 8, false);
        s.mouse_esq("release", 0, 8, false);
        assert_eq!(modelo(&mut s).cursor, (0, 8));
        assert!(modelo(&mut s).selection.is_none());
        // arrastar seleciona do ponto do clique até onde chegou
        s.mouse_esq("press", 0, 2, false);
        s.mouse_esq("drag", 0, 5, false);
        s.mouse_esq("drag", 0, 7, false);
        s.mouse_esq("release", 0, 7, false);
        let m = modelo(&mut s);
        assert_eq!(m.mode, ModeKind::Visual);
        assert_eq!(m.selection.map(|(_, a, b)| (a, b)), Some(((0, 2), (0, 7))));
        // um clique fora desfaz a seleção
        std::thread::sleep(std::time::Duration::from_millis(500));
        s.mouse_esq("press", 1, 3, false);
        s.mouse_esq("release", 1, 3, false);
        let m = modelo(&mut s);
        assert_eq!((m.mode, m.cursor), (ModeKind::Normal, (1, 3)));
        // duplo clique: a palavra
        std::thread::sleep(std::time::Duration::from_millis(500));
        s.mouse_esq("press", 0, 1, false);
        s.mouse_esq("release", 0, 1, false);
        s.mouse_esq("press", 0, 1, false);
        s.mouse_esq("release", 0, 1, false);
        let m = modelo(&mut s);
        assert_eq!(m.selection.map(|(_, a, b)| (a, b)), Some(((0, 0), (0, 4))), "hello");
        // triplo: a linha
        s.mouse_esq("press", 0, 1, false);
        s.mouse_esq("release", 0, 1, false);
        let m = modelo(&mut s);
        assert_eq!(m.selection.map(|(k, _, _)| k), Some(VKind::Line));
        // Shift+clique estende do cursor até o clique
        std::thread::sleep(std::time::Duration::from_millis(500));
        s.mouse_esq("press", 0, 0, false);
        s.mouse_esq("release", 0, 0, false);
        s.mouse_esq("press", 1, 5, true);
        s.mouse_esq("release", 1, 5, true);
        let m = modelo(&mut s);
        assert_eq!(m.selection.map(|(_, a, b)| (a, b)), Some(((0, 0), (1, 5))));
        s.feed("<Esc>");
        // na inserção o clique pode ir para depois do último caractere
        s.feed("i");
        s.mouse_esq("press", 0, 99, false);
        s.mouse_esq("release", 0, 99, false);
        assert_eq!(modelo(&mut s).cursor, (0, 15));
    }

    #[test]
    fn set_number_relativenumber_scrolloff_e_opcoes_desconhecidas() {
        let mut s = Session::new("a\nb\nc\nd\n", None);
        s.resize(80, 20.0);
        let m = modelo(&mut s);
        assert!(m.numero && !m.relativo);
        s.feed(":set nonumber<CR>");
        assert!(!modelo(&mut s).numero);
        s.feed(":set number! rnu<CR>");
        let m = modelo(&mut s);
        assert!(m.numero && m.relativo);
        s.feed(":set norelativenumber<CR>");
        assert!(!modelo(&mut s).relativo);
        // scrolloff aceita número; sem número avisa
        s.feed(":set so=0<CR>");
        assert_eq!(s.scrolloff, 0.0);
        s.feed(":set scrolloff=x<CR>");
        assert!(modelo(&mut s).toast.is_some_and(|t| t.2 == ToastKind::Error));
        // opções do núcleo passam por lá; as desconhecidas avisam; as inofensivas não incomodam
        s.feed(":set sw=2 et nohlsearch<CR>");
        assert_eq!(s.ed.shiftwidth, 2);
        assert!(s.ed.expandtab);
        s.feed(":set bobagem<CR>");
        assert!(modelo(&mut s).toast.is_some_and(|t| t.2 == ToastKind::Error && t.1.contains("bobagem")));
        // clipboard
        s.feed(":set clipboard=unnamedplus<CR>");
        assert!(s.ed.clipboard_unnamedplus);
        s.feed(":set clipboard=<CR>");
        assert!(!s.ed.clipboard_unnamedplus);
        motion_reset();
    }

    fn motion_reset() {
        astrograph_core::motion::set_tabstop(8);
    }

    #[cfg(unix)]
    #[test]
    fn ia_edita_a_selecao_e_responde_perguntas_numa_aba() {
        let d = projeto("ia");
        let arq = d.join("src/ia.py");
        std::fs::write(&arq, "a = 1\nb = 1\nc = 1\n").unwrap();
        let mut w = Workspace::new(Some(arq), ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.atual().root = d.clone();
        // o "agente" devolve o código dentro de uma cerca de markdown, como costumam fazer
        w.atual().ia_cmd = Some(("sh".into(), vec!["-c".into(), "printf 'Claro:\\n```python\\nb = 2\\n```\\n'".into(), "sh".into()]));
        w.feed("jV");
        w.feed(" AE");
        let m = w.view_model();
        assert_eq!(m.cmd.as_ref().map(|c| c.0), Some('✦'));
        assert!(m.cmd.unwrap().1.starts_with("Editar a seleção"));
        w.feed("troque por 2<CR>");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut m = w.view_model();
        while m.rope.to_string() == "a = 1\nb = 1\nc = 1" && std::time::Instant::now() < fim {
            std::thread::sleep(std::time::Duration::from_millis(20));
            m = w.view_model();
        }
        assert_eq!(m.rope.to_string(), "a = 1\nb = 2\nc = 1");
        w.feed("u");
        assert_eq!(w.view_model().rope.to_string(), "a = 1\nb = 1\nc = 1", "um `u` desfaz a edição inteira");
        // sem seleção: pergunta; a resposta abre numa aba
        w.atual().ia_cmd = Some(("sh".into(), vec!["-c".into(), "printf 'Esta função soma.\\n'".into(), "sh".into()]));
        w.feed(" AE");
        assert!(w.view_model().cmd.unwrap().1.starts_with("Perguntar à IA"));
        w.feed("o que faz?<CR>");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut m = w.view_model();
        while m.name != "Resposta da IA" && std::time::Instant::now() < fim {
            std::thread::sleep(std::time::Duration::from_millis(20));
            m = w.view_model();
        }
        assert_eq!(m.name, "Resposta da IA");
        assert_eq!(m.rope.to_string(), "Esta função soma.");
        assert_eq!(m.tabs.len(), 2);
    }

    #[test]
    fn clipboard_do_sistema_vai_no_view_model_e_volta_pelo_clipsync() {
        let mut w = Workspace::new(None, ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.feed("ione two<Esc>0\"+yw");
        let m = w.view_model();
        assert_eq!(m.clip, Some((1, "one ".to_string())), "o app leva isto ao clipboard do sistema");
        // o app diz o que há no clipboard; `"+p` cola de lá
        w.clipboard_do_sistema("ZZ");
        w.feed("$\"+p");
        assert_eq!(w.view_model().rope.to_string(), "one twoZZ");
        // unnamedplus: `yw` comum também copia, e `p` comum cola do clipboard
        w.clipboard_unnamedplus(true);
        w.feed("0yw");
        assert_eq!(w.view_model().clip.map(|c| c.1), Some("one ".to_string()));
        w.clipboard_do_sistema("QQ");
        w.feed("$p");
        assert!(w.view_model().rope.to_string().ends_with("QQ"));
    }

    #[test]
    fn mapeamentos_do_config_com_lider_e_dos_comandos_valem_em_todas_as_abas() {
        use astrograph_core::mapas::{MODO_I, MODO_N};
        let d = projeto("mapas");
        let mut w = Workspace::new(Some(d.join("src/alfa.rs")), ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.aplicar_mapas(&[(MODO_N, "<Leader>d".into(), "dd".into()), (MODO_N, "<Leader>gs".into(), "A!<Esc>".into()), (MODO_I, "jk".into(), "<Esc>".into())]);
        // <Leader>d: o líder abre a espera, mas `d` não é atalho do app: vira o mapeamento do usuário
        w.feed(" d");
        assert_eq!(w.view_model().rope.to_string(), "fn agulha() {}");
        // prefixo maior (<Leader>gs): o `g` espera a próxima tecla
        w.feed(" gs");
        assert_eq!(w.view_model().rope.to_string(), "fn agulha() {}!");
        // imap jk sai da inserção
        w.feed("Aabcjkx");
        assert_eq!(w.view_model().rope.to_string(), "fn agulha() {}!ab");
        // :nnoremap digitado numa aba vale nas outras
        w.feed(":nnoremap Q x<CR>");
        w.feed(&format!(":tabnew {}<CR>", d.join("src/beta.rs").display()));
        assert_eq!(w.n_abas(), 2);
        w.feed("Q");
        assert_eq!(w.view_model().rope.to_string(), "n beta() {}");
        // e os que vieram do config também
        w.feed(" d");
        assert_eq!(w.view_model().rope.to_string(), "");
        // o prefixo pendente aparece na pílula
        w.feed(" g");
        assert!(w.view_model().showcmd.contains('g'), "{:?}", w.view_model().showcmd);
        w.feed("<Esc>");
    }

    #[cfg(unix)]
    fn linhas_do_terminal(m: &ViewModel) -> Vec<String> {
        m.term.as_ref().map(|t| t.screen.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>().trim_end().to_string()).collect()).unwrap_or_default()
    }

    fn espera_term(s: &mut Session, ok: impl Fn(&ViewModel) -> bool) -> ViewModel {
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let m = modelo(s);
            if ok(&m) || std::time::Instant::now() > fim {
                return m;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn varios_terminais_novo_alterna_e_fecha() {
        // as teclas de uma mesma chamada vão juntas para o terminal em foco: `<C-\\><C-n>` sai antes
        fn sair_do_term(s: &mut Session) {
            s.feed("<C-Bslash>");
            s.feed("<C-n>");
        }
        std::env::set_var("SHELL", "/bin/sh");
        let mut s = Session::new("x\n", None);
        s.root = std::env::temp_dir();
        s.resize(80, 20.0);
        s.term_resize(14, 60);
        // a saída do echo: a linha que termina no texto e não é o comando digitado
        let tem = |m: &ViewModel, t: &str| linhas_do_terminal(m).iter().any(|l| l.ends_with(t) && !l.contains("echo"));
        s.feed(" t");
        s.feed("echo AAAA<CR>");
        espera_term(&mut s, |m| tem(m, "AAAA"));
        // <Líder>N: outro terminal, limpo; o primeiro segue vivo no fundo
        sair_do_term(&mut s);
        s.feed(" N");
        s.feed("echo BBBB<CR>");
        let m = espera_term(&mut s, |m| tem(m, "BBBB"));
        assert!(!tem(&m, "AAAA"));
        // <Líder>[ volta ao primeiro; <Líder>] ao segundo
        sair_do_term(&mut s);
        s.feed(" [");
        let m = espera_term(&mut s, |m| tem(m, "AAAA"));
        assert!(!tem(&m, "BBBB"));
        sair_do_term(&mut s);
        s.feed(" ]");
        let m = espera_term(&mut s, |m| tem(m, "BBBB"));
        assert!(!tem(&m, "AAAA"));
        // fechar o atual traz o outro de volta
        sair_do_term(&mut s);
        s.feed(" T");
        let m = espera_term(&mut s, |m| tem(m, "AAAA"));
        assert!(m.term.is_some() && !tem(&m, "BBBB"));
        sair_do_term(&mut s);
        s.feed(" T");
        assert!(modelo(&mut s).term.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn terminal_seleciona_copia_cola_rola_e_avisa_quando_termina() {
        std::env::set_var("SHELL", "/bin/sh");
        let mut s = Session::new("x\n", None);
        s.root = std::env::temp_dir();
        s.resize(80, 20.0);
        s.term_resize(14, 60);
        s.feed(" t");
        s.feed("echo ola mundo<CR>");
        // a saída do echo é a linha (com ou sem o prompt na frente) que termina em "ola mundo" e não é o comando
        let saida = |m: &ViewModel| linhas_do_terminal(m).iter().position(|l| l.ends_with("ola mundo") && !l.starts_with("echo"));
        let m = espera_term(&mut s, |m| saida(m).is_some());
        let r = saida(&m).unwrap_or_else(|| panic!("saída do echo: {:?}", linhas_do_terminal(&m)));
        let c0 = linhas_do_terminal(&m)[r].find("ola mundo").unwrap();
        // arrastar o mouse seleciona e copia ao soltar
        s.term_mouse("press", 0, r, c0, false);
        s.term_mouse("drag", 0, r, c0 + 7, false);
        assert_eq!(modelo(&mut s).term.unwrap().sel, Some(((r, c0), (r, c0 + 7))));
        s.term_mouse("release", 0, r, c0 + 7, false);
        let m = modelo(&mut s);
        assert_eq!(m.clip, Some((1, "ola mund".to_string())));
        // um clique sem arrastar não seleciona nem copia
        s.term_mouse("press", 0, r, c0 + 2, false);
        s.term_mouse("release", 0, r, c0 + 2, false);
        let m = modelo(&mut s);
        assert_eq!((m.term.unwrap().sel, m.clip.map(|c| c.0)), (None, Some(1)));
        // Ctrl+Shift+C sem seleção avisa; com seleção copia de novo
        s.term_copiar();
        assert!(modelo(&mut s).toast.is_some_and(|t| t.2 == ToastKind::Info));
        s.term_mouse("press", 0, r, c0, false);
        s.term_mouse("drag", 0, r, c0 + 2, false);
        s.term_copiar();
        assert_eq!(modelo(&mut s).clip, Some((2, "ola".to_string())));
        s.term_mouse("release", 0, r, c0 + 2, false);
        // colar: o shell recebe a linha inteira
        s.term_mouse("release", 0, r, 2, false);
        s.colar("echo $((6*7))\n");
        let m = espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l == "42"));
        assert!(linhas_do_terminal(&m).iter().any(|l| l == "42"), "{:?}", linhas_do_terminal(&m));
        // histórico: a roda sobe, Shift+PageUp também, digitar volta ao fim
        s.feed("seq 1 90<CR>");
        espera_term(&mut s, |m| linhas_do_terminal(m).iter().any(|l| l == "90"));
        s.term_mouse("wheel", 64, 3, 3, false);
        assert_eq!(modelo(&mut s).term.unwrap().screen.scroll, 3);
        s.term_mouse("wheel", 65, 3, 3, false);
        assert_eq!(modelo(&mut s).term.unwrap().screen.scroll, 0);
        s.feed("<S-PageUp>");
        assert!(modelo(&mut s).term.unwrap().screen.scroll >= 10, "uma página para cima");
        s.feed("echo fim<CR>");
        assert_eq!(modelo(&mut s).term.unwrap().screen.scroll, 0, "digitar volta ao fim");
        // o processo termina: o painel fica com um aviso e uma tecla fecha
        s.feed("exit<CR>");
        let m = espera_term(&mut s, |m| m.term.as_ref().is_some_and(|t| t.screen.exited));
        assert!(m.term.as_ref().is_some_and(|t| t.screen.exited), "painel continua, marcado como terminado");
        s.feed("q");
        assert!(modelo(&mut s).term.is_none(), "uma tecla fecha");
        assert_eq!(s.ed.lines()[0], "x", "a tecla não foi para o texto");
    }

    #[test]
    fn clicar_no_texto_tira_o_teclado_do_terminal() {
        std::env::set_var("SHELL", "/bin/sh");
        let mut s = Session::new("abc\n", None);
        s.root = std::env::temp_dir();
        s.resize(80, 20.0);
        s.feed(" t");
        assert!(modelo(&mut s).term.is_some_and(|t| t.focus));
        s.click(0, 1);
        assert!(modelo(&mut s).term.is_some_and(|t| !t.focus), "o painel segue aberto, sem o teclado");
        s.term_close();
    }

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
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(30);
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
        // o git é lido em segundo plano
        let g = espera_term(&mut s, |m| m.git.is_some()).git.expect("dentro de um repositório");
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

    /// Espaço de trabalho com o servidor falso (python) num arquivo `a.py` de texto `texto`.
    /// Devolve `None` se não há python. A trava serializa o uso da variável de ambiente.
    fn com_falso(nome: &str, texto: &str) -> Option<(std::sync::MutexGuard<'static, ()>, Workspace, PathBuf)> {
        let py = ["python3", "python"].into_iter().find(|p| std::process::Command::new(p).arg("--version").output().is_ok())?;
        let trava = crate::lsp::testes::TRAVA_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let d = projeto(nome);
        let script = d.join("falso.py");
        std::fs::write(&script, crate::lsp::testes::FALSO).unwrap();
        std::env::set_var("ASTROGRAPH_LSP_CMD", format!("{py} {}", script.display()));
        let arq = d.join("a.py");
        std::fs::write(&arq, texto).unwrap();
        let mut w = Workspace::new(Some(arq.clone()), ' ', std::sync::Arc::new(|| {}));
        w.resize(80, 20.0);
        w.atual().root = d;
        // espera o servidor responder (o falso manda um diagnóstico ao abrir)
        espera_ws(&mut w, |m| !m.diags.is_empty());
        Some((trava, w, arq))
    }

    fn espera_ws(w: &mut Workspace, ok: impl Fn(&ViewModel) -> bool) -> ViewModel {
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            let m = w.view_model();
            if ok(&m) || std::time::Instant::now() > fim {
                return m;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    #[test]
    fn lsp_renomear_aplica_a_troca_num_passo_de_desfazer() {
        let Some((_t, mut w, _)) = com_falso("lsp-ren", "x = 1\n") else { return };
        w.feed(":Rename novo<CR>");
        let m = espera_ws(&mut w, |m| m.rope.to_string().starts_with("novo"));
        assert_eq!(m.rope.to_string().trim_end(), "novo = 1");
        assert!(m.modified);
        w.feed("u");
        assert_eq!(w.view_model().rope.to_string().trim_end(), "x = 1", "um desfazer volta o renomear");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn lsp_rn_abre_o_comando_com_a_palavra_sob_o_cursor() {
        let Some((_t, mut w, _)) = com_falso("lsp-rn", "alvo = 1\n") else { return };
        w.feed(" rn");
        assert_eq!(w.view_model().cmd, Some((':', "Rename alvo".to_string())));
        w.feed("<Esc>");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn lsp_acoes_de_codigo_abrem_lista_e_aplicam_a_escolhida() {
        let Some((_t, mut w, _)) = com_falso("lsp-ca", "x = 1\n") else { return };
        w.feed(" ca");
        let m = espera_ws(&mut w, |m| m.picker.is_some());
        let p = m.picker.expect("lista de ações");
        assert_eq!(p.title, "Ações de código");
        assert_eq!(p.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), vec!["Trocar x por y", "Rodar comando"]);
        // Enter aplica a primeira (a edição troca `x` por `y`)
        w.feed("<CR>");
        let m = espera_ws(&mut w, |m| m.rope.to_string().starts_with('y'));
        assert_eq!(m.rope.to_string().trim_end(), "y = 1");
        assert!(m.picker.is_none());
        // a segunda roda um comando: o servidor responde com `applyEdit` (z)
        w.feed(" ca");
        espera_ws(&mut w, |m| m.picker.is_some());
        w.feed("<C-n><CR>");
        let m = espera_ws(&mut w, |m| m.rope.to_string().starts_with('z'));
        assert_eq!(m.rope.to_string().trim_end(), "z = 1", "applyEdit do servidor aplicado");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn lsp_formatar_comando_e_ao_salvar() {
        let Some((_t, mut w, arq)) = com_falso("lsp-fmt", "x = 1\n") else { return };
        // :Format troca o texto (o falso devolve `x = 2`) sem gravar
        w.feed(":Format<CR>");
        let m = espera_ws(&mut w, |m| m.rope.to_string().starts_with("x = 2"));
        assert_eq!(m.rope.to_string().trim_end(), "x = 2");
        assert_eq!(std::fs::read_to_string(&arq).unwrap(), "x = 1\n", "ainda não gravou");
        w.feed("u");
        // com formatar ao salvar, :w formata e grava só depois da resposta
        w.formatar_ao_salvar(true);
        w.feed(":w<CR>");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::fs::read_to_string(&arq).unwrap() != "x = 2\n" && std::time::Instant::now() < fim {
            w.view_model();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        assert_eq!(std::fs::read_to_string(&arq).unwrap(), "x = 2\n", "formatado e gravado");
        assert!(!w.view_model().modified);
        // :wq: grava formatado e sai
        w.feed("ggix<Esc>:wq<CR>");
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !w.sair && std::time::Instant::now() < fim {
            w.view_model();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        assert!(w.sair, ":wq sai depois de gravar");
        // (o falso sempre troca os 5 primeiros caracteres por `x = 2`: "xx = 2" vira "x = 22")
        assert_eq!(std::fs::read_to_string(&arq).unwrap(), "x = 22\n");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn lsp_assinatura_abre_ao_digitar_parenteses_e_fecha_com_o_fim() {
        let Some((_t, mut w, _)) = com_falso("lsp-sig", "\n") else { return };
        w.feed("isoma(1, ");
        let m = espera_ws(&mut w, |m| m.sig.is_some());
        let sig = m.sig.expect("assinatura");
        assert_eq!(sig.texto, "soma(a: int, b: int)");
        assert_eq!(sig.ativo, Some((13, 19)), "o segundo parâmetro");
        assert_eq!(sig.doc, "soma dois numeros");
        w.feed("2)");
        assert!(w.view_model().sig.is_none(), "fecha ao digitar )");
        w.feed("soma(");
        espera_ws(&mut w, |m| m.sig.is_some());
        w.feed("<Esc>");
        assert!(w.view_model().sig.is_none(), "fecha ao sair da inserção");
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
    }

    #[test]
    fn lsp_referencias_simbolos_e_diagnosticos_viram_listas() {
        let Some((_t, mut w, arq)) = com_falso("lsp-lst", "x = 1         \ny = 2         \nz = 3         \nw = 4         \nq = 5         \n") else { return };
        // gr: referências (a primeira no próprio arquivo, a segunda em /tmp/ref dois.py)
        w.feed("gr");
        let m = espera_ws(&mut w, |m| m.picker.is_some());
        let p = m.picker.expect("referências");
        assert_eq!(p.title, "Referências");
        assert_eq!(p.items.len(), 2);
        assert!(p.items[0].text.ends_with("a.py:1"), "{}", p.items[0].text);
        assert_eq!(p.items[0].detail, "x = 1", "linha do buffer");
        w.feed("<Esc>");
        // símbolos: a hierarquia mostra o aninhamento
        w.feed(" ls");
        let m = espera_ws(&mut w, |m| m.picker.is_some());
        let p = m.picker.expect("símbolos");
        assert_eq!(p.title, "Símbolos");
        assert_eq!(p.items.len(), 2);
        assert!(p.items[0].text.contains("Foo") && p.items[1].text.starts_with("  ") && p.items[1].text.contains("bar"), "{:?}", p.items);
        // Enter vai para a linha do símbolo (a 4ª, coluna 6)
        w.feed("<CR>");
        let m = w.view_model();
        assert_eq!(m.cursor, (3, 6));
        // lista de diagnósticos e ]d / [d
        w.feed("gg0");
        w.feed("]d");
        assert_eq!(w.view_model().cursor, (0, 1), "próximo diagnóstico");
        w.feed("[d");
        assert_eq!(w.view_model().cursor, (0, 1), "dá a volta: só há um");
        w.feed(" xx");
        let m = w.view_model();
        let p = m.picker.expect("diagnósticos");
        assert_eq!(p.title, "Diagnósticos");
        assert_eq!((p.items[0].text.as_str(), p.items[0].detail.as_str()), ("1:2 cuidado aqui", "aviso"));
        w.feed("<Esc>");
        let _ = arq;
        std::env::remove_var("ASTROGRAPH_LSP_CMD");
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
        // o Enter vem antes da lista de arquivos: a sessão espera e abre quando ela chega
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while w.n_abas() < 3 && std::time::Instant::now() < fim {
            w.view_model();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(w.n_abas(), 3);
        assert_eq!(w.view_model().name, "LEIA.md");
        w.feed(" ffalfa<CR>");
        espera_ws(&mut w, |m| m.name == "alfa.rs");
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
        s.feed(" hx");
        // <Espaço> move uma coluna e `h` volta uma: não é atalho, volta ao editor; `x` apaga o `a`
        assert_eq!(s.ed.lines()[0], "bc def");
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

    #[test]
    #[cfg(unix)]
    fn ex_r_bang_e_filtro_de_faixa_usam_o_shell() {
        let mut s = Session::new("b\na\nc\n", None);
        s.resize(80, 20.0);
        s.feed(":1r !echo novo<CR>");
        assert_eq!(s.ed.buf.line(1), "novo");
        s.feed(":1,2!sort<CR>");
        assert_eq!(s.ed.buf.line(0), "b");
        assert_eq!(s.ed.buf.line(1), "novo");
        let mut s = Session::new("z\ny\nx\n", None);
        s.resize(80, 20.0);
        s.feed(":%!sort<CR>");
        assert_eq!((s.ed.buf.line(0), s.ed.buf.line(2)), ("x".to_string(), "z".to_string()));
    }
}
