//! O que a sessão do editor publica para a tela: um instantâneo do texto, das
//! cores, do cursor e do estado da interface. A tela (thread da UI) anima a
//! partir dele; a sessão nunca desenha.

use std::sync::{Arc, Mutex};

use astrograph_core::VKind;
use ropey::Rope;

use crate::design::{Cls, ModeKind};

#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub cls: Cls,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
    Done,
}

/// Uma linha do buscador: texto, posições destacadas (em caracteres) e detalhe à direita.
#[derive(Clone, Debug, PartialEq)]
pub struct PickItem {
    pub text: String,
    pub hits: Vec<usize>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickerView {
    pub title: String,
    pub query: String,
    pub items: Vec<PickItem>,
    pub sel: usize,
    /// quantos itens existem no total (os mostrados são os melhores)
    pub total: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeItem {
    pub name: String,
    pub level: usize,
    pub dir: bool,
    pub open: bool,
}

/// Explorador: janela de itens (`first` é o índice absoluto do primeiro) e seleção absoluta.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeView {
    pub root: String,
    pub items: Vec<TreeItem>,
    pub first: usize,
    pub sel: usize,
    pub total: usize,
    pub focus: bool,
}

/// Terminal nativo: o instantâneo da tela e se ele tem o foco do teclado.
#[derive(Clone)]
pub struct TermView {
    pub screen: std::sync::Arc<crate::term::TermScreen>,
    pub focus: bool,
    /// modo zen: o terminal (agente) ocupa a janela, numa coluna centralizada
    pub zen: bool,
}

/// Git do arquivo aberto: branch e sinais da margem (ordenados por linha).
#[derive(Clone)]
pub struct GitView {
    pub branch: String,
    pub signs: std::sync::Arc<Vec<(usize, crate::git::Sign)>>,
}

/// Uma aba aberta.
#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub name: String,
    pub modified: bool,
}

/// Uma linha do popup de completar.
#[derive(Clone, Debug, PartialEq)]
pub struct CompItem {
    pub label: String,
    pub detail: String,
    pub kind: u8,
    /// posições (em caracteres) do rótulo que casaram com o que foi digitado
    pub hits: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompView {
    pub items: Vec<CompItem>,
    pub sel: usize,
    /// onde a palavra começa (linha, coluna): o popup alinha com ela
    pub anchor: (usize, usize),
}

#[derive(Clone)]
pub struct ViewModel {
    /// sobe a cada publicação (a tela só repinta o que mudou)
    pub seq: u64,
    pub rev: u64,
    pub rope: Rope,
    pub lines: usize,
    /// realce das linhas `span_lo..span_lo + spans.len()`
    pub span_lo: usize,
    pub spans: Vec<Vec<Span>>,
    /// linha de cima desejada (a tela suaviza até lá)
    pub top: f32,
    /// colunas visuais já roladas na horizontal
    pub left: usize,
    pub cursor: (usize, usize),
    pub mode: ModeKind,
    pub selection: Option<(VKind, (usize, usize), (usize, usize))>,
    /// seleção em bloco com `$`: vai até o fim de cada linha
    pub sel_eol: bool,
    pub name: String,
    pub modified: bool,
    /// `:`, `/` ou `?` com o texto digitado
    pub cmd: Option<(char, String)>,
    pub toast: Option<(u64, String, ToastKind)>,
    pub showcmd: String,
    /// trecho que acabou de ser copiado (clarão): (início, fim, id)
    /// ocorrências da busca nas linhas em volta da tela: (linha, início, fim) em caracteres
    /// tentativa de rolar além do fim: (id, linhas; < 0 = acima do início)
    pub bump: Option<(u64, f32)>,
    pub hits: Vec<(usize, usize, usize)>,
    /// muda quando o padrão muda (0 = sem realce); a cena reinicia o fade
    pub hits_id: u64,
    /// tela inicial (nenhum arquivo, buffer vazio) em vez do texto
    pub dash: bool,
    pub picker: Option<PickerView>,
    pub tree: Option<TreeView>,
    pub term: Option<TermView>,
    pub git: Option<GitView>,
    /// diagnósticos do servidor de linguagem para o arquivo (colunas em caracteres)
    pub diags: std::sync::Arc<Vec<crate::lsp::Diag>>,
    /// texto do hover (`K`) aberto
    pub hover: Option<String>,
    pub completion: Option<CompView>,
    /// abas abertas (a barra só aparece com duas ou mais) e a ativa
    pub tabs: Vec<TabInfo>,
    pub tab_active: usize,
    /// muda ao trocar de arquivo/aba: a cena zera as animações
    pub doc_id: u64,
    pub flash: Option<((usize, usize), (usize, usize), u64)>,
    /// janelas divididas: o layout da aba, o painel focado (este modelo) e os outros painéis
    pub layout: Option<crate::janelas::Layout>,
    pub pane: crate::janelas::PaneId,
    pub panes: Vec<PaneView>,
}

/// Um painel que não tem o foco.
#[derive(Clone)]
pub struct PaneView {
    pub id: crate::janelas::PaneId,
    pub vm: Box<ViewModel>,
}

impl ViewModel {
    pub fn empty() -> Self {
        ViewModel {
            seq: 0,
            rev: 0,
            rope: Rope::new(),
            lines: 1,
            span_lo: 0,
            spans: Vec::new(),
            top: 0.0,
            left: 0,
            cursor: (0, 0),
            mode: ModeKind::Normal,
            selection: None,
            sel_eol: false,
            name: String::new(),
            modified: false,
            cmd: None,
            toast: None,
            showcmd: String::new(),
            bump: None,
            hits: Vec::new(),
            hits_id: 0,
            dash: false,
            picker: None,
            tree: None,
            term: None,
            git: None,
            diags: std::sync::Arc::new(Vec::new()),
            hover: None,
            completion: None,
            tabs: Vec::new(),
            tab_active: 0,
            doc_id: 0,
            flash: None,
            layout: None,
            pane: 0,
            panes: Vec::new(),
        }
    }

    /// Texto da linha sem a quebra.
    pub fn line(&self, row: usize) -> String {
        if row >= self.rope.len_lines() {
            return String::new();
        }
        let l = self.rope.line(row).to_string();
        l.strip_suffix('\n').unwrap_or(&l).to_string()
    }

    pub fn spans_of(&self, row: usize) -> &[Span] {
        row.checked_sub(self.span_lo).and_then(|i| self.spans.get(i)).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Ponte entre a thread da sessão e a da UI.
/// Instante em que o processo começou (para o "pronto em N ms" da tela inicial)
pub static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub type Shared = Arc<Mutex<Arc<ViewModel>>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(Arc::new(ViewModel::empty())))
}
