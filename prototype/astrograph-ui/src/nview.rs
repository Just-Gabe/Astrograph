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
    pub name: String,
    pub modified: bool,
    /// `:`, `/` ou `?` com o texto digitado
    pub cmd: Option<(char, String)>,
    pub toast: Option<(u64, String, ToastKind)>,
    pub showcmd: String,
    /// trecho que acabou de ser copiado (clarão): (início, fim, id)
    /// ocorrências da busca nas linhas em volta da tela: (linha, início, fim) em caracteres
    pub hits: Vec<(usize, usize, usize)>,
    /// muda quando o padrão muda (0 = sem realce); a cena reinicia o fade
    pub hits_id: u64,
    pub flash: Option<((usize, usize), (usize, usize), u64)>,
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
            name: String::new(),
            modified: false,
            cmd: None,
            toast: None,
            showcmd: String::new(),
            hits: Vec::new(),
            hits_id: 0,
            flash: None,
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
pub type Shared = Arc<Mutex<Arc<ViewModel>>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(Arc::new(ViewModel::empty())))
}
