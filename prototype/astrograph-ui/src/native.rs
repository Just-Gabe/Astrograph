//! Editor nativo: o motor modal em Rust (`astrograph-core`) sem Neovim.
//! `ASTROGRAPH_ENGINE=native [arquivo]`.
//!
//! `Session` não conhece janela nem threads: recebe teclas, redimensionamentos,
//! cliques e rolagem, e publica um `ViewModel` (texto, cores, cursor, estado). Quem
//! desenha e anima é a cena (`nscene`). Por isso é testável sem GUI.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use astrograph_core::motion::{col_for_vcol, vcol_cursor, vcol_start};
use astrograph_core::{parse_keys, Editor, Key, Mode, VKind};
use astrograph_syntax::{detect, lang, Document, Lang};

use crate::design::{classify, ModeKind};
use crate::arvore::Arvore;
use crate::busca;
use crate::nview::{PickItem, PickerView, Shared, Span, ToastKind, TreeItem, TreeView, ViewModel};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickKind {
    Arquivos,
    Texto,
    Recentes,
}

struct Picker {
    kind: PickKind,
    query: String,
    /// candidatos (arquivos/recentes); no texto, fica vazio
    base: Vec<String>,
    items: Vec<PickItem>,
    achados: Vec<busca::Achado>,
    total: usize,
    sel: usize,
}

const PICK_LINHAS: usize = 10;

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
    cmdline: Option<String>,
    toast: Option<(u64, String, ToastKind)>,
    toast_id: u64,
    seq: u64,
    dash: bool,
    picker: Option<Picker>,
    tree: Option<Arvore>,
    tree_focus: bool,
    leader: Vec<Key>,
    /// tecla líder (padrão Espaço)
    leader_key: char,
    root: PathBuf,
    bump: Option<(u64, f32)>,
    hl_pat: String,
    hl_id: u64,
    hl_off: bool,
    pub quit: bool,
}

const SCROLLOFF: f32 = 3.0;
/// linhas de realce calculadas além da tela, para a rolagem suave não ficar sem cor
const SPAN_MARGIN: usize = 80;

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
            cmdline: None,
            dash: false,
            picker: None,
            tree: None,
            tree_focus: false,
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
        }
    }

    /// Define a tecla líder (as menções a ela mostram o nome da tecla escolhida).
    pub fn set_leader(&mut self, c: char) {
        self.leader_key = c;
    }

    /// Sem arquivo: abre na tela inicial (some ao editar, ao `n` ou ao abrir um arquivo).
    pub fn welcome() -> Self {
        let mut s = Session::new("", None);
        s.dash = true;
        s
    }

    pub fn open(path: &Path) -> Self {
        let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let mut s = Session::new(&text, Some(path.to_path_buf()));
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
        if self.cmdline.is_some() || self.ed.has_pending() {
            return;
        }
        let (row, col) = (self.ed.cursor.row, self.ed.cursor.col);
        let last = self.ed.buf.line_count() - 1;
        let page = (self.rows - 2.0).max(1.0) as usize;
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
        let livre = self.cmdline.is_none() && !self.ed.has_pending() && self.ed.mode() == Mode::Normal;
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
        let livre = self.cmdline.is_none() && !self.ed.has_pending() && self.ed.mode() == Mode::Normal;
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
                Key::Char('a' | 's') => {
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
            PickKind::Texto => Vec::new(),
        };
        self.picker = Some(Picker { kind, query: String::new(), total: base.len(), base, items: Vec::new(), achados: Vec::new(), sel: 0 });
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
            PickKind::Arquivos => (self.root.join(&item.text), None),
            PickKind::Recentes => (p.base.iter().find(|b| relativo(&self.root, b) == item.text).map(PathBuf::from).unwrap_or_default(), None),
            PickKind::Texto => match p.achados.get(p.sel) {
                Some(a) => (self.root.join(&a.arquivo), Some((a.linha, a.col))),
                None => return,
            },
        };
        self.open_file(&path, goto);
    }

    /// Abre `path` no lugar do buffer atual (pede para salvar antes, como o `:e`).
    fn open_file(&mut self, path: &Path, goto: Option<(usize, usize)>) {
        if self.modified() {
            self.say("Alterações não salvas. Salve com :w antes de abrir outro arquivo.".into(), ToastKind::Error);
            return;
        }
        let (cols, rows, root) = (self.cols, self.rows, self.root.clone());
        let (tree, foco, lider) = (self.tree.take(), self.tree_focus, self.leader_key);
        *self = Session::open(path);
        self.root = root;
        self.leader_key = lider;
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
        if let Some(cmd) = self.cmdline.as_mut() {
            match k {
                Key::Esc | Key::Ctrl('c') => self.cmdline = None,
                Key::Enter => {
                    let c = self.cmdline.take().unwrap_or_default();
                    self.exec_ex(&c);
                }
                Key::Backspace => {
                    if cmd.pop().is_none() {
                        self.cmdline = None;
                    }
                }
                Key::Char(c) => cmd.push(c),
                Key::Tab => cmd.push('\t'),
                _ => {}
            }
            return;
        }
        if k == Key::Char(':') && !self.ed.has_pending() && self.ed.mode() != Mode::Insert {
            if self.ed.mode() == Mode::Visual {
                self.ed.feed(&[Key::Esc]);
            }
            self.cmdline = Some(String::new());
            return;
        }
        self.ed.feed(&[k]);
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

    fn exec_ex(&mut self, line: &str) {
        let line = line.trim();
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
            "w" | "write" => self.write(arg),
            "q" | "quit" => {
                if self.modified() && !bang {
                    self.say("Alterações não salvas. Use :q! para sair sem salvar.".into(), ToastKind::Error);
                } else {
                    self.quit = true;
                }
            }
            "wq" | "x" | "xit" | "exit" => {
                self.write(arg);
                if !self.modified() {
                    self.quit = true;
                }
            }
            "e" | "edit" => {
                if arg.is_empty() {
                    return;
                }
                if self.modified() && !bang {
                    self.say("Alterações não salvas. Use :e! para descartar.".into(), ToastKind::Error);
                } else {
                    self.saved_rev = self.ed.buf.rev;
                    self.open_file(Path::new(arg), None);
                }
            }
            "noh" | "nohlsearch" | "set" | "syntax" | "filetype" => {}
            _ => self.say(format!("Comando desconhecido: {name}"), ToastKind::Error),
        }
    }

    fn write(&mut self, arg: &str) {
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
            let (lo, hi) = if a <= c { (a, c) } else { (c, a) };
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
        let mode = if self.cmdline.is_some() {
            ModeKind::Command
        } else if search.is_some() {
            ModeKind::Search
        } else {
            match self.ed.mode() {
                Mode::Normal => ModeKind::Normal,
                Mode::Insert => ModeKind::Insert,
                Mode::Visual => match sel.map(|s| s.0) {
                    Some(VKind::Line) => ModeKind::VisualLine,
                    _ => ModeKind::Visual,
                },
            }
        };
        let cmd = self
            .cmdline
            .as_ref()
            .map(|c| (':', c.clone()))
            .or_else(|| search.map(|s| (s.chars().next().unwrap_or('/'), s.chars().skip(1).collect())));
        ViewModel {
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
        }
    }
}

/// Tarefa do editor nativo: lê a fila de comandos da UI e publica o modelo.
pub async fn native_task(
    shared: Shared,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::UiCommand>,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
    file: Option<PathBuf>,
    leader: char,
) {
    use crate::UiCommand;
    let mut s = match &file {
        Some(p) => Session::open(p),
        None => Session::welcome(),
    };
    s.set_leader(leader);
    while let Some(cmd) = rx.recv().await {
        match cmd {
            UiCommand::Keys(k) => s.feed(&k),
            UiCommand::Resize(c, r) => s.resize(c.max(1) as usize, r.max(1) as f32),
            UiCommand::Focus(_) => continue,
            UiCommand::Scroll(lines) => s.scroll(lines as f32),
            UiCommand::Mouse { button, action, row, col, .. } => match (button, action) {
                ("left", "press") => s.click(row.max(0) as usize, col.max(0) as usize),
                _ => continue,
            },
        }
        if s.quit {
            let _ = proxy.send_event(crate::UserEvent::Quit);
            return;
        }
        *shared.lock().unwrap() = Arc::new(s.view_model());
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
