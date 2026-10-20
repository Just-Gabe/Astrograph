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
use crate::nview::{Shared, Span, ToastKind, ViewModel};

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
            hl_pat: String::new(),
            hl_id: 0,
            hl_off: false,
            toast: None,
            toast_id: 0,
            seq: 0,
            quit: false,
        }
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

    pub fn feed(&mut self, keys: &str) {
        for k in parse_keys(keys) {
            self.key(k);
        }
    }

    fn key(&mut self, k: Key) {
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
        let r = row.min(self.ed.buf.line_count() - 1);
        let line = self.ed.buf.line(r);
        let len = line.chars().count();
        let c = if vcol >= vcol_start(&line, len) { len.saturating_sub(1) } else { col_for_vcol(&line, vcol) };
        self.ed.set_cursor(r, c);
    }

    /// Rolagem em linhas (pode ser fracionária): `lines > 0` desce.
    pub fn scroll(&mut self, lines: f32) {
        let max = self.ed.buf.line_count().saturating_sub(1) as f32;
        self.top = (self.top + lines).clamp(0.0, max);
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
                    let (cols, rows) = (self.cols, self.rows);
                    *self = Session::open(Path::new(arg));
                    self.resize(cols, rows);
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
            showcmd: if self.ed.search_line().is_some() { String::new() } else { self.ed.showcmd() },
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
) {
    use crate::UiCommand;
    let mut s = match &file {
        Some(p) => Session::open(p),
        None => Session::new("", None),
    };
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

#[cfg(test)]
mod testes {
    use super::*;

    fn modelo(s: &mut Session) -> ViewModel {
        s.view_model()
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
