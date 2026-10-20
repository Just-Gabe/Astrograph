//! O editor modal: estado, análise de comandos do modo normal, operadores,
//! registradores, inserção, desfazer e repetição (`.`).

use std::collections::HashMap;

use ropey::Rope;

use crate::buffer::{Buffer, Pos};
use crate::keys::Key;
use crate::motion;
use crate::search;
use crate::textobj;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VKind {
    Char,
    Line,
}

#[derive(Debug, Clone, Copy)]
struct VState {
    kind: VKind,
    anchor: Pos,
}

enum VParse {
    Incomplete,
    Invalid,
    Done(VCmd),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VCmd {
    Motion(Mo, Option<usize>),
    Op(char, Option<usize>),
    Obj(bool, char, Option<usize>),
    Replace(char),
    /// `p` / `P` sobre a seleção
    Put(bool, Option<usize>, bool),
    Swap,
    Kind(VKind),
    Esc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reg {
    pub text: String,
    pub linewise: bool,
}

#[derive(Debug, Clone)]
struct Snap {
    rope: Rope,
    cursor: Pos,
    marks: HashMap<char, Pos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Excl,
    Incl,
    Line,
}

/// Um movimento: tecla, se tem prefixo `g`, e o argumento (`f`/`t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mo {
    c: char,
    g: bool,
    arg: Option<char>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Line,
    Motion(Mo),
    /// objeto de texto: (inclui os delimitadores, caractere)
    Obj(bool, char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cmd {
    Motion(Mo),
    Op { op: char, count2: Option<usize>, target: Target },
    Simple(char, Option<char>),
}

struct Parsed {
    reg: Option<char>,
    count: Option<usize>,
    cmd: Cmd,
    /// teclas do prefixo (registrador e contagem), fora do que o `.` repete
    prefix: usize,
    /// padrão digitado em `/texto<CR>` ou `?texto<CR>`
    search: Option<(String, bool)>,
}

enum Parse {
    Incomplete,
    Invalid,
    Done(Parsed),
}

struct Res {
    pos: Pos,
    kind: Kind,
    /// movimento vertical: mantém a coluna desejada
    vertical: bool,
    /// o Vim apita (e descarta o resto das teclas), mas o cursor já andou
    fail: bool,
}

#[derive(Default)]
struct Insert {
    /// comando que abriu a inserção (`i a I A o O c s S C`)
    cmd: char,
    count: usize,
    typed: Vec<Key>,
    /// linha que recebeu só recuo automático (some se nada for digitado)
    ai_row: Option<usize>,
}

pub struct Editor {
    pub buf: Buffer,
    pub cursor: Pos,
    curswant: usize,
    pub mode: Mode,
    pending: Vec<Key>,
    regs: HashMap<char, Reg>,
    unnamed: Option<Reg>,
    /// Último trecho copiado: (contador, início, fim exclusivo, por linhas). Para efeitos visuais.
    pub last_yank: (u64, Pos, Pos, bool),
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_find: Option<(char, bool, bool)>,
    dot: Vec<Key>,
    dot_count: Option<usize>,
    dot_recording: bool,
    replaying: bool,
    ins: Insert,
    /// colunas de `shiftwidth`
    pub shiftwidth: usize,
    /// operador em curso é `c` (para `cw` agir como `ce`)
    ops_change: bool,
    /// o último comando falhou: o Vim descarta o resto das teclas já digitadas
    failed: bool,
    /// todas as linhas foram apagadas (buffer "vazio" do Vim): `dd` não faz mais nada
    emptied: bool,
    /// último padrão buscado e a direção (para `n`/`N`)
    last_search: Option<(String, bool)>,
    /// padrão do comando em execução (`/texto<CR>`)
    search_now: Option<(String, bool)>,
    /// mensagem da última busca ("search hit BOTTOM..." ou erro)
    pub msg: String,
    /// seleção do modo visual
    vis: Option<VState>,
    /// o operador em curso vem do modo visual (sem a regra de `d` multilinha → linhas)
    visual_op: bool,
    /// gravando macro: (registrador, teclas)
    rec: Option<(char, Vec<Key>)>,
    /// apertou `q` e espera o nome do registrador
    q_wait: bool,
    /// executando um macro (as teclas executadas não são regravadas)
    in_macro: bool,
    last_macro: Option<char>,
    macro_depth: usize,
    /// marcas `a-z`/`A-Z`
    marks: HashMap<char, Pos>,
    /// marca de contexto (`''` e ``` `` ```): de onde o último salto saiu
    pcmark: Option<Pos>,
    /// seleção visual mais recente (para `'<` e `'>`)
    vmarks: Option<(Pos, Pos, VKind)>,
    /// marcas que o desfazer/refazer devolve (aplicadas depois do reajuste por linhas)
    marks_back: Option<HashMap<char, Pos>>,
    /// onde o último `p` terminou: (por linhas?, posição logo depois do texto) — para `gp`
    put_end: Option<(bool, Pos)>,
}

const MAXWANT: usize = usize::MAX;

impl Editor {
    pub fn new<S: AsRef<str>>(lines: &[S]) -> Self {
        Editor {
            buf: Buffer::from_lines(lines),
            cursor: Pos::new(0, 0),
            curswant: 0,
            mode: Mode::Normal,
            pending: Vec::new(),
            regs: HashMap::new(),
            unnamed: None,
            last_yank: (0, Pos::new(0, 0), Pos::new(0, 0), false),
            undo: Vec::new(),
            redo: Vec::new(),
            last_find: None,
            dot: Vec::new(),
            dot_count: None,
            dot_recording: false,
            replaying: false,
            ins: Insert::default(),
            shiftwidth: 8,
            ops_change: false,
            failed: false,
            emptied: false,
            last_search: None,
            search_now: None,
            msg: String::new(),
            vis: None,
            visual_op: false,
            rec: None,
            q_wait: false,
            in_macro: false,
            last_macro: None,
            macro_depth: 0,
            marks: HashMap::new(),
            pcmark: Some(Pos::new(0, 0)),
            vmarks: None,
            marks_back: None,
            put_end: None,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.buf.lines()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Padrão da última busca (`/`, `?`, `*`, `#`).
    pub fn last_pattern(&self) -> Option<&str> {
        self.last_search.as_ref().map(|(p, _)| p.as_str())
    }

    /// Texto do que está sendo digitado: `/padrão` ou `?padrão` durante uma busca.
    pub fn search_line(&self) -> Option<String> {
        let k = &self.pending;
        let mut i = 0;
        while i < k.len() {
            match k[i] {
                Key::Char('"') => i += 2,
                Key::Char(c) if c.is_ascii_digit() || matches!(c, 'd' | 'c' | 'y' | '<' | '>') => i += 1,
                _ => break,
            }
        }
        match k.get(i) {
            Some(Key::Char(c @ ('/' | '?'))) => {
                let text: String = k[i + 1..].iter().filter_map(|x| x.as_char()).collect();
                Some(format!("{c}{text}"))
            }
            _ => None,
        }
    }

    /// Teclas pendentes para mostrar (`2d`, `"a`...), como o `showcmd` do Vim.
    pub fn showcmd(&self) -> String {
        self.pending
            .iter()
            .map(|k| match k {
                Key::Char(c) => c.to_string(),
                Key::Esc => "^[".into(),
                Key::Enter => "^M".into(),
                Key::Ctrl(c) => format!("^{}", c.to_ascii_uppercase()),
                _ => String::new(),
            })
            .collect()
    }

    /// Há teclas de um comando ainda incompleto (`d`, `2`, `"`...).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Seleção do modo visual: (tipo, ponta fixa).
    pub fn selection(&self) -> Option<(VKind, Pos)> {
        self.vis.map(|v| (v.kind, v.anchor))
    }

    /// Texto do buffer inteiro com `\n` no fim (como o arquivo).
    pub fn to_file_text(&self) -> String {
        let mut t = self.buf.text();
        t.push('\n');
        t
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor = Pos::new(row.min(self.buf.line_count() - 1), col);
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    pub fn register(&self, c: char) -> Option<&Reg> {
        if c == '"' {
            self.unnamed.as_ref()
        } else {
            self.regs.get(&c)
        }
    }

    /// Processa as teclas. Um comando que falha (movimento impossível, tecla
    /// desconhecida...) só levanta `beeped()`; as teclas seguintes seguem valendo,
    /// como na digitação de verdade.
    pub fn feed(&mut self, keys: &[Key]) {
        for &k in keys {
            self.feed_one(k);
        }
    }

    /// Entrada digitada: aqui o `q` liga/desliga a gravação e as teclas são gravadas.
    fn feed_one(&mut self, k: Key) {
        if self.mode == Mode::Normal && self.pending.is_empty() {
            if self.q_wait {
                self.q_wait = false;
                match k {
                    Key::Char(r) if r.is_ascii_alphanumeric() || r == '"' => self.rec = Some((r, Vec::new())),
                    _ => self.failed = true,
                }
                return;
            }
            if k == Key::Char('q') {
                if let Some((r, keys)) = self.rec.take() {
                    let text = macro_text(&keys);
                    let lower = r.to_ascii_lowercase();
                    let prev = if r.is_ascii_uppercase() { self.regs.get(&lower).map(|x| x.text.clone()) } else { None };
                    self.regs.insert(lower, Reg { text: prev.unwrap_or_default() + &text, linewise: false });
                } else {
                    self.q_wait = true;
                }
                return;
            }
        }
        if let Some((_, ks)) = self.rec.as_mut() {
            ks.push(k);
        }
        self.key(k);
    }

    fn set_mark(&mut self, c: char) {
        match c {
            'a'..='z' | 'A'..='Z' => {
                self.marks.insert(c, self.cursor);
            }
            '\'' | '`' => self.pcmark = Some(self.cursor),
            _ => self.failed = true,
        }
    }

    /// Posição da marca `c` (a-z, A-Z, `'`, `` ` ``, `<`, `>`).
    fn mark_pos(&self, c: char) -> Option<Pos> {
        match c {
            'a'..='z' | 'A'..='Z' => self.marks.get(&c).copied(),
            '\'' | '`' => self.pcmark,
            '<' | '>' => {
                let (a, b, kind) = self.vmarks?;
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                Some(match (c, kind) {
                    ('<', VKind::Line) => Pos::new(lo.row, 0),
                    ('<', _) => lo,
                    ('>', VKind::Line) => Pos::new(hi.row, usize::MAX),
                    _ => hi,
                })
            }
            _ => None,
        }
    }

    /// Reajusta as marcas depois de uma edição que inseriu ou apagou linhas.
    fn adjust_marks(&mut self, old: &Rope) {
        let (n0, n1) = (old.len_lines(), self.buf.rope.len_lines());
        if n0 == n1 {
            return;
        }
        let old_line = |r: usize| -> String {
            let l = old.line(r).to_string();
            l.strip_suffix('\n').unwrap_or(&l).to_string()
        };
        let common = n0.min(n1);
        let f = (0..common).find(|&r| old_line(r) != self.buf.line(r)).unwrap_or(common);
        let shift = |row: usize, d: i64| (row as i64 + d).max(0) as usize;
        if n1 < n0 {
            let k = n0 - n1;
            let pure = f >= n1 || old_line(f + k) == self.buf.line(f);
            let adj = |p: Pos, keep: bool| -> Option<Pos> {
                if pure && p.row >= f && p.row < f + k {
                    return if keep { Some(Pos::new(f.min(n1 - 1), p.col)) } else { None };
                }
                if !pure && p.row > f && p.row <= f + k {
                    return Some(Pos::new(f, p.col));
                }
                if p.row >= f + k { Some(Pos::new(shift(p.row, -(k as i64)), p.col)) } else { Some(p) }
            };
            self.marks = self.marks.iter().filter_map(|(c, p)| adj(*p, false).map(|q| (*c, q))).collect();
            self.pcmark = self.pcmark.and_then(|p| adj(p, false));
        } else {
            let n = n1 - n0;
            let pure = f < n0 && old_line(f) == self.buf.line(f + n);
            let adj = |p: Pos| -> Pos {
                if (pure && p.row >= f) || (!pure && p.row > f) { Pos::new(p.row + n, p.col) } else { p }
            };
            self.marks = self.marks.iter().map(|(c, p)| (*c, adj(*p))).collect();
            self.pcmark = self.pcmark.map(adj);
        }
    }

    /// `@r`: executa o conteúdo do registrador como teclas; um comando que falha encerra o macro.
    fn run_macro(&mut self, r: char, count: usize) {
        let r = if r == '@' {
            match self.last_macro {
                Some(l) => l,
                None => {
                    self.failed = true;
                    return;
                }
            }
        } else {
            r
        };
        let Some(reg) = self.regs.get(&r.to_ascii_lowercase()).cloned() else {
            self.failed = true;
            return;
        };
        self.last_macro = Some(r);
        // macro que chama a si mesmo: para na profundidade 100 como erro (o Vim para no primeiro erro)
        if self.macro_depth >= 100 {
            self.failed = true;
            return;
        }
        self.macro_depth += 1;
        let keys = macro_keys(&reg.text);
        let was = std::mem::replace(&mut self.in_macro, true);
        'fora: for _ in 0..count {
            for &k in &keys {
                self.failed = false;
                self.key(k);
                if self.failed {
                    break 'fora;
                }
            }
        }
        self.in_macro = was;
        self.macro_depth -= 1;
    }

    /// O último comando falhou (a interface pode apitar). Lê e zera.
    pub fn beeped(&mut self) -> bool {
        std::mem::take(&mut self.failed)
    }

    pub fn feed_str(&mut self, s: &str) {
        self.feed(&crate::keys::parse_keys(s));
    }

    // ------------------------------------------------------------------
    // entrada
    // ------------------------------------------------------------------

    fn key(&mut self, k: Key) {
        // marcas acompanham linhas inseridas/apagadas: compara o texto de antes e depois
        let old = (!self.marks.is_empty() || self.pcmark.is_some()).then(|| self.buf.rope.clone());
        self.key_raw(k);
        if let Some(old) = old {
            self.adjust_marks(&old);
        }
        if let Some(back) = self.marks_back.take() {
            for (c, p) in back {
                self.marks.insert(c, p);
            }
        }
    }

    fn key_raw(&mut self, k: Key) {
        match self.mode {
            Mode::Insert => self.insert_key(k),
            Mode::Visual => {
                if k == Key::Esc && !self.pending.is_empty() {
                    self.pending.clear();
                    return;
                }
                self.pending.push(k);
                match self.vparse(&self.pending.clone()) {
                    VParse::Incomplete => {}
                    VParse::Invalid => {
                        self.pending.clear();
                        self.failed = true;
                    }
                    VParse::Done(c) => {
                        self.pending.clear();
                        self.visual_exec(c);
                    }
                }
            }
            Mode::Normal => {
                if k == Key::Esc && !self.pending.is_empty() {
                    self.pending.clear();
                    return;
                }
                // apagar durante `/texto`: tira uma tecla; sem texto, cancela a busca
                if k == Key::Backspace {
                    if let Some(l) = self.search_line() {
                        if l.chars().count() <= 1 {
                            self.pending.clear();
                        } else {
                            self.pending.pop();
                        }
                        return;
                    }
                }
                self.pending.push(k);
                match self.parse(&self.pending.clone()) {
                    Parse::Incomplete => {}
                    Parse::Invalid => {
                        self.pending.clear();
                        self.failed = true;
                    }
                    Parse::Done(p) => {
                        let keys = std::mem::take(&mut self.pending);
                        self.exec(p, &keys);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // análise do modo normal
    // ------------------------------------------------------------------

    fn parse(&self, keys: &[Key]) -> Parse {
        let mut i = 0;
        let mut reg = None;
        let mut count: Option<usize> = None;
        loop {
            match keys.get(i) {
                Some(Key::Char('"')) => match keys.get(i + 1) {
                    Some(Key::Char(r)) => {
                        reg = Some(*r);
                        i += 2;
                    }
                    Some(_) => return Parse::Invalid,
                    None => return Parse::Incomplete,
                },
                Some(Key::Char(d)) if d.is_ascii_digit() && (*d != '0' || count.is_some()) => {
                    count = Some(count.unwrap_or(0) * 10 + d.to_digit(10).unwrap() as usize);
                    i += 1;
                }
                _ => break,
            }
        }
        let prefix = i;
        let Some(&k) = keys.get(i) else { return Parse::Incomplete };
        i += 1;
        let done = |cmd: Cmd| Parse::Done(Parsed { reg, count, cmd, prefix, search: None });
        let done_s = |cmd: Cmd, search: Option<(String, bool)>| Parse::Done(Parsed { reg, count, cmd, prefix, search });
        match k {
            Key::Char(op @ ('d' | 'c' | 'y' | '<' | '>')) => {
                let mut count2: Option<usize> = None;
                loop {
                    match keys.get(i) {
                        Some(Key::Char(d)) if d.is_ascii_digit() && (*d != '0' || count2.is_some()) => {
                            count2 = Some(count2.unwrap_or(0) * 10 + d.to_digit(10).unwrap() as usize);
                            i += 1;
                        }
                        _ => break,
                    }
                }
                match keys.get(i) {
                    None => Parse::Incomplete,
                    Some(Key::Char(c)) if *c == op => done(Cmd::Op { op, count2, target: Target::Line }),
                    Some(Key::Char(ia @ ('i' | 'a'))) => match keys.get(i + 1) {
                        Some(Key::Char(o)) if "wWpP()[]{}<>bB\"'`".contains(*o) => {
                            done(Cmd::Op { op, count2, target: Target::Obj(*ia == 'a', *o) })
                        }
                        Some(_) => Parse::Invalid,
                        None => Parse::Incomplete,
                    },
                    Some(_) => match parse_motion(&keys[i..]) {
                        MParse::Incomplete => Parse::Incomplete,
                        MParse::Invalid => Parse::Invalid,
                        MParse::Done(m, _) => done_s(Cmd::Op { op, count2, target: Target::Motion(m) }, extract_search(&keys[i..], m)),
                    },
                }
            }
            _ => match parse_motion(&keys[i - 1..]) {
                MParse::Done(m, _) => done_s(Cmd::Motion(m), extract_search(&keys[i - 1..], m)),
                MParse::Incomplete => Parse::Incomplete,
                MParse::Invalid => match k {
                    Key::Char(c @ ('x' | 'X' | 'D' | 'C' | 's' | 'S' | 'Y' | 'p' | 'P' | 'u' | 'J' | 'i' | 'a' | 'I' | 'A' | 'o' | 'O' | '~' | '.')) => {
                        done(Cmd::Simple(c, None))
                    }
                    Key::Char(c @ ('v' | 'V')) => done(Cmd::Simple(c, None)),
                    Key::Char('r') => match keys.get(i) {
                        Some(Key::Char(c)) => done(Cmd::Simple('r', Some(*c))),
                        Some(_) => Parse::Invalid,
                        None => Parse::Incomplete,
                    },
                    Key::Char('m') => match keys.get(i) {
                        Some(Key::Char(c)) => done(Cmd::Simple('m', Some(*c))),
                        Some(_) => Parse::Invalid,
                        None => Parse::Incomplete,
                    },
                    Key::Char('@') => match keys.get(i) {
                        Some(Key::Char(c)) => done(Cmd::Simple('@', Some(*c))),
                        Some(_) => Parse::Invalid,
                        None => Parse::Incomplete,
                    },
                    Key::Ctrl('r') => done(Cmd::Simple('R', None)),
                    _ => Parse::Invalid,
                },
            },
        }
    }

    // ------------------------------------------------------------------
    // execução
    // ------------------------------------------------------------------

    fn exec(&mut self, p: Parsed, keys: &[Key]) {
        let count1 = p.count.unwrap_or(1);
        let pc_before = self.cursor;
        let had_search = self.last_search.is_some();
        let jump = match &p.cmd {
            Cmd::Motion(m) | Cmd::Op { target: Target::Motion(m), .. } => is_jump(m),
            _ => false,
        };
        let is_change = match &p.cmd {
            Cmd::Motion(_) => false,
            Cmd::Op { op, .. } => *op != 'y',
            Cmd::Simple(c, _) => !matches!(c, 'u' | 'R' | '.' | 'Y' | 'v' | 'V' | '@' | 'm'),
        };
        self.search_now = p.search.clone();
        // `/`, `?`, `*` e `#` gravam o padrão mesmo quando não acham
        let sm = match &p.cmd {
            Cmd::Motion(m) => Some(*m),
            Cmd::Op { target: Target::Motion(m), .. } => Some(*m),
            _ => None,
        };
        if let Some(m) = sm {
            if matches!(m.c, '/' | '?' | '*' | '#') {
                if let Some(t) = self.search_target(m) {
                    self.last_search = Some(t);
                }
            }
        }
        // `failed` de comandos anteriores não pode contaminar este
        let failed_before = std::mem::take(&mut self.failed);
        // guarda o anterior: se este comando falhar, o `.` continua repetindo o de antes
        let prev_dot = (self.dot.clone(), self.dot_count);
        if is_change && !self.replaying {
            self.dot = keys[p.prefix..].to_vec();
            self.dot_count = p.count;
            self.dot_recording = true;
        }
        match p.cmd {
            Cmd::Motion(m) => {
                if !m.g && matches!(m.c, 'f' | 'F' | 't' | 'T') {
                    if let Some(ch) = m.arg {
                        self.last_find = Some((ch, matches!(m.c, 'f' | 't'), matches!(m.c, 't' | 'T')));
                    }
                }
                match self.motion(m, p.count, false) {
                    Some(r) => {
                        self.cursor = r.pos;
                        self.fix_cursor();
                        if !r.vertical {
                            self.curswant = if m.c == '$' && !m.g { MAXWANT } else { self.want_here() };
                        }
                        if r.fail {
                            self.failed = true;
                        }
                    }
                    None => {
                        if m.c == '$' && !m.g {
                            self.curswant = MAXWANT;
                        }
                        // uma busca que falha também refaz a coluna desejada
                        if matches!(m.c, '/' | '?' | 'n' | 'N' | '*' | '#') {
                            self.curswant = self.want_here();
                        }
                        self.failed = true;
                    }
                }
            }
            Cmd::Op { op, count2, target } => {
                let total = match (p.count, count2) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(1) * b.unwrap_or(1)),
                };
                self.operator(op, total, target, p.reg);
            }
            Cmd::Simple(c, arg) => self.simple(c, arg, p.count, count1, p.reg),
        }
        if self.mode == Mode::Normal {
            self.dot_recording = false;
        }
        if is_change && !self.replaying && self.failed {
            self.dot = prev_dot.0;
            self.dot_count = prev_dot.1;
        }
        // movimento por marca que falha (marca não definida) não grava o contexto
        let falhou_marca = (self.failed && matches!(&p.cmd, Cmd::Motion(m) | Cmd::Op { target: Target::Motion(m), .. } if matches!(m.c, '\'' | '`')))
            || (!had_search && matches!(&p.cmd, Cmd::Motion(m) | Cmd::Op { target: Target::Motion(m), .. } if matches!(m.c, 'n' | 'N')));
        if jump && !falhou_marca {
            self.pcmark = Some(pc_before);
        }
        self.failed |= failed_before;
    }

    fn simple(&mut self, c: char, arg: Option<char>, count: Option<usize>, count1: usize, reg: Option<char>) {
        match c {
            'x' => self.operator('d', count, Target::Motion(Mo { c: 'l', g: false, arg: None }), reg),
            'X' => self.operator('d', count, Target::Motion(Mo { c: 'h', g: false, arg: None }), reg),
            'D' => self.operator('d', count, Target::Motion(Mo { c: '$', g: false, arg: None }), reg),
            'C' => self.operator('c', count, Target::Motion(Mo { c: '$', g: false, arg: None }), reg),
            's' => self.operator('c', count, Target::Motion(Mo { c: 'l', g: false, arg: None }), reg),
            'S' => self.operator('c', count, Target::Line, reg),
            'Y' => self.operator('y', count, Target::Motion(Mo { c: '$', g: false, arg: None }), reg),
            'p' => self.put(true, count1, reg),
            'P' => self.put(false, count1, reg),
            'u' => self.undo_step(count1),
            'R' => self.redo_step(count1),
            '.' => self.dot_repeat(count),
            'J' => {
                self.join(count1.max(2));
            }
            'm' => self.set_mark(arg.unwrap()),
            '@' => self.run_macro(arg.unwrap(), count1),
            'r' => self.replace_chars(arg.unwrap(), count1),
            '~' => self.toggle_case(count1),
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => self.start_insert(c, count1),
            'v' | 'V' => {
                self.vis = Some(VState { kind: if c == 'v' { VKind::Char } else { VKind::Line }, anchor: self.cursor });
                self.mode = Mode::Visual;
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // movimentos
    // ------------------------------------------------------------------

    fn last_row(&self) -> usize {
        self.buf.line_count() - 1
    }

    fn col_for_want(&self, row: usize) -> usize {
        let len = self.buf.line_len(row);
        let visual = self.mode == Mode::Visual;
        if len == 0 {
            0
        } else if self.curswant == MAXWANT {
            if visual { len } else { len - 1 }
        } else {
            let line = self.buf.line(row);
            let total = motion::vcol_start(&line, len);
            // além do fim da linha: no visual cai no NUL (inclui a quebra), senão no último caractere
            if visual && self.curswant >= total {
                len
            } else {
                motion::col_for_vcol(&line, self.curswant).min(len - 1)
            }
        }
    }

    /// Coluna desejada do cursor atual, em colunas de tela.
    fn want_here(&self) -> usize {
        motion::vcol_cursor(&self.buf.line(self.cursor.row), self.cursor.col)
    }

    fn motion(&self, m: Mo, count: Option<usize>, op_pending: bool) -> Option<Res> {
        let c1 = count.unwrap_or(1);
        let cur = self.cursor;
        let b = &self.buf;
        let len = b.line_len(cur.row);
        let ex = |pos: Pos| Some(Res { pos, kind: Kind::Excl, vertical: false, fail: false });
        let inc = |pos: Pos| Some(Res { pos, kind: Kind::Incl, vertical: false, fail: false });
        if m.g {
            return match m.c {
                'g' => {
                    let row = count.map_or(0, |n| n.saturating_sub(1)).min(self.last_row());
                    Some(Res { pos: Pos::new(row, self.col_for_want(row)), kind: Kind::Line, vertical: true, fail: false })
                }
                'e' | 'E' => {
                    let mut p = cur;
                    let ok = motion::bckend_word(b, &mut p, c1, m.c == 'E', false);
                    Some(Res { pos: p, kind: Kind::Incl, vertical: false, fail: !ok })
                }
                '_' => {
                    let row = (cur.row + c1 - 1).min(self.last_row());
                    Some(Res { pos: Pos::new(row, b.line_len(row).saturating_sub(1)), kind: Kind::Incl, vertical: false, fail: false })
                }
                '*' | '#' => {
                    let (pat, fwd) = self.search_target(m)?;
                    let re = search::compile(&pat)?;
                    let mut pos = self.word_start();
                    for _ in 0..c1 {
                        pos = search::find(b, &re, pos, fwd)?.0;
                    }
                    Some(Res { pos, kind: Kind::Excl, vertical: false, fail: false })
                }
                _ => None,
            };
        }
        match m.c {
            'h' => {
                if cur.col == 0 {
                    // com operador o Vim não apita: a região só fica vazia
                    return if op_pending { ex(cur) } else { None };
                }
                ex(Pos::new(cur.row, cur.col.saturating_sub(c1)))
            }
            'l' | ' ' => {
                if op_pending {
                    if len == 0 || cur.col >= len {
                        return ex(cur);
                    }
                    ex(Pos::new(cur.row, (cur.col + c1).min(len)))
                } else if self.mode == Mode::Visual {
                    // no visual `l` anda até a quebra de linha
                    if cur.col >= len {
                        return None;
                    }
                    ex(Pos::new(cur.row, (cur.col + c1).min(len)))
                } else {
                    if cur.col + 1 >= len {
                        return None;
                    }
                    ex(Pos::new(cur.row, (cur.col + c1).min(len - 1)))
                }
            }
            'j' | '+' | '\r' => {
                if cur.row >= self.last_row() {
                    return None;
                }
                let row = (cur.row + c1).min(self.last_row());
                let col = if m.c == 'j' { self.col_for_want(row) } else { b.first_nonblank(row).min(b.line_len(row).saturating_sub(1)) };
                Some(Res { pos: Pos::new(row, col), kind: Kind::Line, vertical: m.c == 'j', fail: false })
            }
            'k' | '-' => {
                if cur.row == 0 {
                    return None;
                }
                let row = cur.row.saturating_sub(c1);
                let col = if m.c == 'k' { self.col_for_want(row) } else { b.first_nonblank(row).min(b.line_len(row).saturating_sub(1)) };
                Some(Res { pos: Pos::new(row, col), kind: Kind::Line, vertical: m.c == 'k', fail: false })
            }
            '0' => ex(Pos::new(cur.row, 0)),
            '^' => ex(Pos::new(cur.row, b.first_nonblank(cur.row).min(len.saturating_sub(1)))),
            '$' => {
                let row = cur.row + c1 - 1;
                if row > self.last_row() {
                    return None;
                }
                let l = b.line_len(row);
                let col = if self.mode == Mode::Visual { l } else { l.saturating_sub(1) };
                inc(Pos::new(row, col))
            }
            '_' => {
                let row = (cur.row + c1 - 1).min(self.last_row());
                Some(Res { pos: Pos::new(row, b.first_nonblank(row).min(b.line_len(row).saturating_sub(1))), kind: Kind::Line, vertical: false, fail: false })
            }
            '\'' | '`' => {
                let p = self.mark_pos(m.arg?)?;
                let row = p.row.min(self.last_row());
                if m.c == '\'' {
                    let col = b.first_nonblank(row).min(b.line_len(row).saturating_sub(1));
                    Some(Res { pos: Pos::new(row, col), kind: Kind::Line, vertical: false, fail: false })
                } else {
                    ex(Pos::new(row, p.col.min(b.line_len(row).saturating_sub(1))))
                }
            }
            'G' => {
                let row = count.map_or(self.last_row(), |n| n.saturating_sub(1).min(self.last_row()));
                Some(Res { pos: Pos::new(row, self.col_for_want(row)), kind: Kind::Line, vertical: true, fail: false })
            }
            'w' | 'W' => {
                let big = m.c == 'W';
                let mut p = cur;
                let mut word_end = false;
                let mut incl = false;
                let mut stop = false;
                // cw em um não-branco age como ce
                if op_pending && self.ops_change {
                    if let Some(ch) = b.char_at(cur) {
                        if ch != ' ' && ch != '\t' {
                            word_end = true;
                            stop = true;
                            incl = true;
                        }
                    }
                }
                let ok = if word_end {
                    motion::end_word(b, &mut p, c1, big, stop, false)
                } else {
                    motion::fwd_word(b, &mut p, c1, big, op_pending)
                };
                // não deixa o cursor no NUL, a menos que nem tenha andado
                let mut kind = if incl { Kind::Incl } else { Kind::Excl };
                if cur < p && b.char_at(p).is_none() && p.col > 0 && p.col == b.line_len(p.row) && (op_pending || true) {
                    p.col -= 1;
                    kind = Kind::Incl;
                }
                Some(Res { pos: p, kind, vertical: false, fail: !ok })
            }
            'b' | 'B' => {
                let mut p = cur;
                let ok = motion::bck_word(b, &mut p, c1, m.c == 'B', false);
                Some(Res { pos: p, kind: Kind::Excl, vertical: false, fail: !ok })
            }
            'e' | 'E' => {
                let mut p = cur;
                let ok = motion::end_word(b, &mut p, c1, m.c == 'E', false, false);
                if b.char_at(p).is_none() && p.col > 0 {
                    p.col -= 1;
                }
                Some(Res { pos: p, kind: Kind::Incl, vertical: false, fail: !ok })
            }
            'f' | 'F' | 't' | 'T' => {
                let ch = m.arg?;
                let col = motion::find_char(b, cur, ch, m.c == 'f' || m.c == 't', m.c == 't' || m.c == 'T', c1, false)?;
                if m.c == 'f' || m.c == 't' {
                    inc(Pos::new(cur.row, col))
                } else {
                    ex(Pos::new(cur.row, col))
                }
            }
            ';' | ',' => {
                let (ch, fwd, till) = self.last_find?;
                let forward = if m.c == ';' { fwd } else { !fwd };
                let col = motion::find_char(b, cur, ch, forward, till, c1, true)?;
                if forward {
                    inc(Pos::new(cur.row, col))
                } else {
                    ex(Pos::new(cur.row, col))
                }
            }
            '%' => {
                // o Vim apita mas não descarta o que vem depois num `%` sem par
                match motion::match_pair(b, cur) {
                    Some(pos) => inc(pos),
                    None => Some(Res { pos: cur, kind: Kind::Incl, vertical: false, fail: false }),
                }
            }
            '}' | '{' => {
                let (pos, incl) = motion::paragraph(b, cur, m.c == '}', c1)?;
                Some(Res { pos, kind: if incl { Kind::Incl } else { Kind::Excl }, vertical: false, fail: false })
            }
            '/' | '?' | 'n' | 'N' | '*' | '#' => {
                let (pat, fwd) = self.search_target(m)?;
                let re = search::compile(&pat)?;
                let mut pos = if matches!(m.c, '*' | '#') { self.word_start() } else { cur };
                for _ in 0..c1 {
                    let (p, _wrapped) = search::find(b, &re, pos, fwd)?;
                    pos = p;
                }
                if pos == cur && !matches!(m.c, 'n' | 'N' | '*' | '#' | '/' | '?') {
                    return None;
                }
                Some(Res { pos, kind: Kind::Excl, vertical: false, fail: false })
            }
            _ => None,
        }
    }

    /// Início da palavra sob o cursor (ou da próxima na linha): de onde `*` e `#` buscam.
    fn word_start(&self) -> Pos {
        let line: Vec<char> = self.buf.line(self.cursor.row).chars().collect();
        let kw = |c: char| c.is_alphanumeric() || c == '_';
        if line.is_empty() {
            return self.cursor;
        }
        let mut i = self.cursor.col.min(line.len() - 1);
        if !kw(line[i]) {
            match (i..line.len()).find(|&j| kw(line[j])) {
                Some(j) => i = j,
                None => return self.cursor,
            }
        }
        while i > 0 && kw(line[i - 1]) {
            i -= 1;
        }
        Pos::new(self.cursor.row, i)
    }

    /// Padrão e direção de um movimento de busca (e grava como último).
    fn search_target(&self, m: Mo) -> Option<(String, bool)> {
        match m.c {
            '/' | '?' => {
                let (mut pat, fwd) = self.search_now.clone()?;
                if pat.is_empty() {
                    pat = self.last_search.as_ref()?.0.clone();
                }
                Some((pat, fwd))
            }
            'n' => self.last_search.clone(),
            'N' => self.last_search.clone().map(|(p, f)| (p, !f)),
            _ => {
                // * e #: a palavra sob o cursor (ou depois dele, na linha)
                let line: Vec<char> = self.buf.line(self.cursor.row).chars().collect();
                let kw = |c: char| c.is_alphanumeric() || c == '_';
                let mut i = self.cursor.col.min(line.len().saturating_sub(1));
                if line.is_empty() {
                    return None;
                }
                if !kw(line[i]) {
                    i = (i..line.len()).find(|&j| kw(line[j]))?;
                }
                let (mut a, mut b) = (i, i);
                while a > 0 && kw(line[a - 1]) {
                    a -= 1;
                }
                while b + 1 < line.len() && kw(line[b + 1]) {
                    b += 1;
                }
                let word: String = line[a..=b].iter().collect();
                let esc: String = word.chars().flat_map(|c| if "\\/.*$^~[]".contains(c) { vec!['\\', c] } else { vec![c] }).collect();
                let pat = if m.g { esc } else { format!("\\<{esc}\\>") };
                Some((pat, m.c == '*'))
            }
        }
    }

    // ------------------------------------------------------------------
    // operadores
    // ------------------------------------------------------------------

    fn operator(&mut self, op: char, count: Option<usize>, target: Target, reg: Option<char>) {
        // lembra f/t para ; e ,
        if let Target::Motion(m) = target {
            if !m.g && matches!(m.c, 'f' | 'F' | 't' | 'T') {
                if let Some(ch) = m.arg {
                    self.last_find = Some((ch, matches!(m.c, 'f' | 't'), matches!(m.c, 't' | 'T')));
                }
            }
        }
        self.ops_change = op == 'c';
        let (a, b, kind) = match target {
            Target::Line => {
                let n = count.unwrap_or(1);
                let r1 = self.cursor.row;
                // `3dd` na última linha falha; em outras o excesso é cortado
                if n > 1 && r1 >= self.last_row() {
                    self.ops_change = false;
                    self.failed = true;
                    return;
                }
                let r2 = (r1 + n - 1).min(self.last_row());
                (Pos::new(r1, 0), Pos::new(r2, 0), Kind::Line)
            }
            Target::Obj(include, ch) => {
                let n = count.unwrap_or(1);
                let obj = match ch {
                    'w' | 'W' => match textobj::current_word(&self.buf, self.cursor, n, include, ch == 'W') {
                        Ok(o) => Some(o),
                        Err(p) => {
                            // falhou, mas o Vim deixa o cursor onde chegou
                            self.cursor = p;
                            self.fix_cursor();
                            self.curswant = self.want_here();
                            None
                        }
                    },
                    'p' | 'P' => textobj::current_par(&self.buf, self.cursor, n, include),
                    '(' | ')' | 'b' => textobj::current_block(&self.buf, self.cursor, n, include, '('),
                    '[' | ']' => textobj::current_block(&self.buf, self.cursor, n, include, '['),
                    '{' | '}' | 'B' => textobj::current_block(&self.buf, self.cursor, n, include, '{'),
                    '<' | '>' => textobj::current_block(&self.buf, self.cursor, n, include, '<'),
                    q => textobj::current_quote(&self.buf, self.cursor, n, include, q),
                };
                let Some(o) = obj else {
                    self.ops_change = false;
                    self.failed = true;
                    return;
                };
                let mut kind = if o.linewise { Kind::Line } else if o.inclusive { Kind::Incl } else { Kind::Excl };
                let (mut s, mut e) = (o.start, o.end);
                if kind == Kind::Excl && e.col == 0 && e.row > s.row {
                    e = Pos::new(e.row - 1, self.buf.line_len(e.row - 1));
                    kind = if s.col <= self.buf.first_nonblank(s.row) { Kind::Line } else { Kind::Excl };
                }
                if e < s {
                    std::mem::swap(&mut s, &mut e);
                }
                // `yip`/`yap`: o cursor vai para o começo do objeto (coluna 0)
                if kind == Kind::Line && op == 'y' {
                    self.cursor = Pos::new(self.cursor.row, 0);
                    self.curswant = 0;
                }
                (s, e, kind)
            }
            Target::Motion(m) => {
                let Some(r) = self.motion(m, count, true) else {
                    self.ops_change = false;
                    self.failed = true;
                    return;
                };
                if r.fail && !matches!(m.c, 'w' | 'W' | 'e' | 'E') {
                    self.ops_change = false;
                    self.failed = true;
                    return;
                }
                let (mut s, mut e) = (self.cursor, r.pos);
                if e < s {
                    std::mem::swap(&mut s, &mut e);
                }
                let mut kind = r.kind;
                // exclusivo terminando na coluna 0 de outra linha: volta ao fim da anterior
                if kind == Kind::Excl && e.col == 0 && e.row > s.row {
                    e = Pos::new(e.row - 1, self.buf.line_len(e.row - 1));
                    // ...e se o início está no recuo, vira por linhas
                    if s.col <= self.buf.first_nonblank(s.row) {
                        kind = Kind::Line;
                    } else {
                        kind = Kind::Excl;
                    }
                }
                (s, e, kind)
            }
        };
        self.ops_change = false;
        self.apply_operator(op, a, b, kind, reg);
    }

    fn apply_operator(&mut self, op: char, a: Pos, b: Pos, kind: Kind, reg: Option<char>) {
        match op {
            'y' => {
                let (text, linewise) = self.range_text(a, b, kind);
                // região vazia: com movimento inclusivo (y$ em linha vazia) nada muda; com
                // exclusivo (y0 na coluna 0) o registrador fica vazio
                if !linewise && text.is_empty() && kind == Kind::Incl {
                    return;
                }
                self.store(reg, text, linewise, false);
                let end = match kind {
                    Kind::Line => Pos::new(b.row, self.buf.line_len(b.row)),
                    Kind::Incl => Pos::new(b.row, (b.col + 1).min(self.buf.line_len(b.row))),
                    Kind::Excl => b,
                };
                let start = if kind == Kind::Line { Pos::new(a.row, 0) } else { a };
                self.last_yank = (self.last_yank.0 + 1, start, end, linewise);
                // o cursor vai para o início do trecho
                match kind {
                    Kind::Line => {
                        if a.row < self.cursor.row {
                            self.cursor = Pos::new(a.row, self.col_for_want(a.row));
                        }
                    }
                    _ => self.cursor = a,
                }
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            'd' | 'c' => {
                let mut kind = kind;
                // região vazia: `d` não faz nada (nem mexe no registrador); `c` só abre a inserção
                if kind != Kind::Line {
                    let (t, _) = self.range_text(a, b, kind);
                    if t.is_empty() {
                        // o Vim prepara o undo mesmo sem nada a apagar
                        self.checkpoint();
                        if op == 'c' {
                            self.cursor = a;
                            self.begin_insert_state('c', 1, None);
                        }
                        return;
                    }
                }
                // `d` charwise em várias linhas, começando no recuo e acabando no fim da
                // linha, vira por linhas (regra do op_delete)
                if op == 'd' && kind != Kind::Line && b.row > a.row && !self.visual_op {
                    let line_b: Vec<char> = self.buf.line(b.row).chars().collect();
                    let after = if kind == Kind::Incl { b.col + 1 } else { b.col };
                    let rest_blank = line_b.iter().skip(after).all(|c| *c == ' ' || *c == '\t');
                    let in_indent = self.buf.line(a.row).chars().take(a.col).all(|c| c == ' ' || c == '\t');
                    if rest_blank && in_indent {
                        kind = Kind::Line;
                    }
                }
                if kind == Kind::Line && self.emptied && b.row == 0 {
                    // buffer já "vazio": o Vim não apaga nem guarda nada
                    if op == 'c' {
                        self.checkpoint();
                        self.cursor = Pos::new(0, 0);
                        self.begin_insert_state('c', 1, None);
                    }
                    return;
                }
                self.checkpoint();
                let (text, linewise) = self.range_text(a, b, kind);
                self.store(reg, text, linewise, true);
                match kind {
                    Kind::Line => {
                        if op == 'c' {
                            let indent: String = self.buf.line(a.row).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                            if b.row > a.row {
                                self.buf.delete_lines(a.row + 1, b.row);
                            }
                            let len = self.buf.line_len(a.row);
                            self.buf.delete(Pos::new(a.row, 0), Pos::new(a.row, len));
                            self.buf.insert(Pos::new(a.row, 0), &indent);
                            self.cursor = Pos::new(a.row, indent.chars().count());
                            self.begin_insert_state('c', 1, Some(a.row).filter(|_| !indent.is_empty()));
                        } else {
                            self.emptied = a.row == 0 && b.row + 1 >= self.buf.line_count();
                            self.buf.delete_lines(a.row, b.row);
                            let row = a.row.min(self.last_row());
                            // 'nostartofline': a coluna é a de antes, não o primeiro não-branco
                            self.cursor = Pos::new(row, self.col_for_want(row));
                            self.fix_cursor();
                            self.curswant = self.want_here();
                        }
                    }
                    _ => {
                        let end = if kind == Kind::Incl { Pos::new(b.row, (b.col + 1).min(self.buf.line_len(b.row))) } else { b };
                        self.buf.delete(a, end);
                        self.cursor = a;
                        if op == 'c' {
                            self.begin_insert_state('c', 1, None);
                        } else {
                            self.fix_cursor();
                            self.curswant = self.want_here();
                        }
                    }
                }
            }
            '>' | '<' => {
                self.checkpoint();
                let (r1, r2) = (a.row, b.row);
                for r in r1..=r2 {
                    self.shift_line(r, op == '>');
                }
                self.cursor = Pos::new(r1, self.col_for_want(r1));
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            _ => {}
        }
    }

    fn range_text(&self, a: Pos, b: Pos, kind: Kind) -> (String, bool) {
        match kind {
            Kind::Line => ((a.row..=b.row).map(|r| self.buf.line(r)).collect::<Vec<_>>().join("\n"), true),
            _ => {
                let end = if kind == Kind::Incl { Pos::new(b.row, (b.col + 1).min(self.buf.line_len(b.row))) } else { b };
                let (i, j) = (self.buf.idx(a), self.buf.idx(end));
                (self.buf.rope.slice(i..j.max(i)).to_string(), false)
            }
        }
    }

    fn store(&mut self, reg: Option<char>, text: String, linewise: bool, delete: bool) {
        let r = Reg { text, linewise };
        match reg {
            Some(c) if c != '"' => {
                self.regs.insert(c.to_ascii_lowercase(), r.clone());
            }
            _ => {
                if delete {
                    if r.linewise || r.text.contains('\n') {
                        for n in (1..9).rev() {
                            if let Some(prev) = self.regs.get(&char::from_digit(n, 10).unwrap()).cloned() {
                                self.regs.insert(char::from_digit(n + 1, 10).unwrap(), prev);
                            }
                        }
                        self.regs.insert('1', r.clone());
                    } else {
                        self.regs.insert('-', r.clone());
                    }
                } else {
                    self.regs.insert('0', r.clone());
                }
            }
        }
        self.unnamed = Some(r);
    }

    fn shift_line(&mut self, row: usize, right: bool) {
        let line = self.buf.line(row);
        if line.is_empty() && right {
            return;
        }
        let ts = 8;
        let indent_chars: usize = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let width: usize = line.chars().take(indent_chars).fold(0, |w, c| if c == '\t' { (w / ts + 1) * ts } else { w + 1 });
        let new_w = if right { width + self.shiftwidth } else { width.saturating_sub(self.shiftwidth) };
        let new_indent = format!("{}{}", "\t".repeat(new_w / ts), " ".repeat(new_w % ts));
        let rest: String = line.chars().skip(indent_chars).collect();
        let len = self.buf.line_len(row);
        self.buf.delete(Pos::new(row, 0), Pos::new(row, len));
        self.buf.insert(Pos::new(row, 0), &format!("{new_indent}{rest}"));
    }

    // ------------------------------------------------------------------
    // colar, juntar, trocar
    // ------------------------------------------------------------------

    fn put(&mut self, after: bool, count: usize, reg: Option<char>) {
        let r = match reg {
            Some(c) if c != '"' => self.regs.get(&c).cloned(),
            _ => self.unnamed.clone(),
        };
        let Some(r) = r else { return };
        self.put_reg(r, after, count);
    }

    /// `p`/`P` sobre a seleção visual (regras do `nv_put` do Vim). `replace_reg`: `p` leva o texto
    /// apagado para o registrador; `P` o deixa como estava.
    fn visual_put(&mut self, replace_reg: bool, count: usize, cursor_after: bool) {
        let Some(vs) = self.vis else { return };
        // registrador vazio: o Vim apaga a seleção mesmo assim e avisa que não há o que colar
        let vazio = self.unnamed.is_none();
        let r = self.unnamed.clone().unwrap_or(Reg { text: String::new(), linewise: false });
        // `P` simples deixa o registrador; `p`, `gp` e `gP` o trocam pelo texto apagado
        let keep = !replace_reg && !cursor_after;
        let saved = keep.then(|| (self.regs.clone(), self.unnamed.clone()));
        let undo_len = self.undo.len();
        let (a, _, _) = self.visual_range();
        let kind = vs.kind;
        self.visual_operate('d', None);
        let esvaziou = self.emptied;
        if let Some((regs, un)) = saved {
            self.regs = regs;
            self.unnamed = un;
        }
        if vazio {
            self.failed = true;
        }
        // depois de apagar, o cursor "voltou" para o fim da linha/arquivo? então põe depois
        let after = if kind == VKind::Line { self.cursor.row < a.row } else { self.cursor.col < a.col && self.cursor.row == a.row };
        let mut r = r;
        let mut split = false;
        if kind == VKind::Char && r.linewise {
            split = true;
        }
        if kind == VKind::Line && !r.linewise {
            r.linewise = true;
        }
        self.put_reg_opts(r, after, count, split);
        if cursor_after && !vazio {
            if let Some((linewise, end)) = self.put_end {
                self.cursor = if linewise { Pos::new(end.row.min(self.last_row()), 0) } else { end };
                self.fix_cursor();
                self.curswant = self.want_here();
            }
        }
        // todas as linhas foram apagadas: a linha vazia que sobrou depois do put sai
        if esvaziou && self.buf.line_count() > 1 && self.buf.line_len(self.last_row()) == 0 {
            let last = self.last_row();
            self.buf.delete_lines(last, last);
            if self.cursor.row > self.last_row() {
                let r = self.last_row();
                self.cursor = Pos::new(r, self.buf.line_len(r).saturating_sub(1));
            }
        }
        // apagar e colar contam como um passo só no desfazer
        self.undo.truncate(undo_len + 1);
        self.mode = Mode::Normal;
        self.vis = None;
    }

    fn put_reg(&mut self, r: Reg, after: bool, count: usize) {
        self.put_reg_opts(r, after, count, false);
    }

    fn put_reg_opts(&mut self, r: Reg, after: bool, count: usize, split: bool) {
        self.checkpoint();
        let mut after = after;
        if split {
            // `p` de linhas sobre uma seleção de caracteres: parte a linha e põe as linhas no meio
            let row = self.cursor.row;
            let line = self.buf.line(row);
            let len = line.chars().count();
            let col = if after && len > 0 { self.cursor.col + 1 } else { self.cursor.col }.min(len);
            let (first, rest): (String, String) = (line.chars().take(col).collect(), line.chars().skip(col).collect());
            let end = Pos::new(row, len);
            self.buf.delete(Pos::new(row, 0), end);
            self.buf.insert(Pos::new(row, 0), &first);
            self.buf.insert_line(row + 1, &rest);
            self.cursor = Pos::new(row, 0);
            after = true;
        }
        if r.linewise {
            let lines: Vec<&str> = r.text.split('\n').collect();
            let at = if after { self.cursor.row + 1 } else { self.cursor.row };
            let mut row = at;
            for _ in 0..count {
                for l in &lines {
                    self.buf.insert_line(row, l);
                    row += 1;
                }
            }
            self.put_end = Some((true, Pos::new(row, 0)));
            self.cursor = Pos::new(at, self.buf.first_nonblank(at));
            self.fix_cursor();
            self.curswant = self.want_here();
        } else {
            let text = r.text.repeat(count);
            let len = self.buf.line_len(self.cursor.row);
            let col = if after && len > 0 { self.cursor.col + 1 } else { self.cursor.col };
            let at = Pos::new(self.cursor.row, col);
            self.buf.insert(at, &text);
            let n = text.chars().count();
            let end = self.buf.pos_of(self.buf.idx(at) + n);
            self.put_end = Some((false, end));
            if text.contains('\n') {
                self.cursor = at;
            } else {
                self.cursor = Pos::new(end.row, end.col.saturating_sub(1));
            }
            self.fix_cursor();
            self.curswant = self.want_here();
        }
    }

    /// `J`, igual ao `do_join` do Vim: o espaço depende do fim de CADA linha original.
    fn join(&mut self, count: usize) -> bool {
        let row = self.cursor.row;
        let mut count = count;
        if row + count - 1 > self.last_row() {
            // na última linha com 1 ou 2 o Vim apita; com mais, junta o que houver
            if count <= 2 {
                self.failed = true;
                return false;
            }
            count = self.last_row() - row + 1;
        }
        self.checkpoint();
        let mut out = String::new();
        let mut sum = 0usize; // comprimento acumulado (para a regra "sum != 0")
        let (mut e1, mut e2): (Option<char>, Option<char>);
        let mut last_size = 0usize;
        let mut last_spaces = 0usize;
        e1 = None;
        e2 = None;
        for t in 0..count {
            let line = self.buf.line(row + t);
            let mut curr: String = line.clone();
            let mut spaces = 0usize;
            if t > 0 {
                curr = line.trim_start_matches([' ', '\t']).to_string();
                if !curr.is_empty() && !curr.starts_with(')') && sum != 0 && e1 != Some('\t') {
                    // não põe espaço se a linha anterior já acaba em espaço
                    if e1 == Some(' ') {
                        e1 = e2;
                    } else {
                        spaces += 1;
                    }
                }
            }
            let size = curr.chars().count();
            sum += size + spaces;
            out.push_str(&" ".repeat(spaces));
            out.push_str(&curr);
            last_size = size;
            last_spaces = spaces;
            e1 = None;
            e2 = None;
            if size > 0 {
                let chars: Vec<char> = curr.chars().collect();
                e1 = chars.last().copied();
                if size > 1 {
                    e2 = Some(chars[size - 2]);
                }
            }
        }
        let col = sum - last_size - last_spaces;
        let len = self.buf.line_len(row);
        self.buf.delete(Pos::new(row, 0), Pos::new(row, len));
        self.buf.insert(Pos::new(row, 0), &out);
        if count > 1 {
            self.buf.delete_lines(row + 1, row + count - 1);
        }
        self.cursor = Pos::new(row, col);
        self.fix_cursor();
        self.curswant = self.want_here();
        true
    }

    fn replace_chars(&mut self, ch: char, count: usize) {
        let len = self.buf.line_len(self.cursor.row);
        if self.cursor.col + count > len {
            self.failed = true;
            return;
        }
        self.checkpoint();
        let a = self.cursor;
        let b = Pos::new(a.row, a.col + count);
        self.buf.delete(a, b);
        self.buf.insert(a, &ch.to_string().repeat(count));
        self.cursor = Pos::new(a.row, a.col + count - 1);
        self.curswant = self.want_here();
    }

    fn toggle_case(&mut self, count: usize) {
        let len = self.buf.line_len(self.cursor.row);
        if len == 0 {
            self.failed = true;
            return;
        }
        self.checkpoint();
        let n = count.min(len - self.cursor.col);
        let a = self.cursor;
        let b = Pos::new(a.row, a.col + n);
        let text = self.buf.delete(a, b);
        let flipped: String = text.chars().map(|c| if c.is_lowercase() { c.to_uppercase().next().unwrap_or(c) } else if c.is_uppercase() { c.to_lowercase().next().unwrap_or(c) } else { c }).collect();
        self.buf.insert(a, &flipped);
        self.cursor = Pos::new(a.row, a.col + n);
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    // ------------------------------------------------------------------
    // modo visual
    // ------------------------------------------------------------------

    fn vparse(&self, keys: &[Key]) -> VParse {
        let mut i = 0;
        let mut count: Option<usize> = None;
        while let Some(Key::Char(d)) = keys.get(i) {
            if d.is_ascii_digit() && (*d != '0' || count.is_some()) {
                count = Some(count.unwrap_or(0) * 10 + d.to_digit(10).unwrap() as usize);
                i += 1;
            } else {
                break;
            }
        }
        let Some(&k) = keys.get(i) else { return VParse::Incomplete };
        match k {
            Key::Esc | Key::Ctrl('c') => VParse::Done(VCmd::Esc),
            Key::Char('v') => VParse::Done(VCmd::Kind(VKind::Char)),
            Key::Char('V') => VParse::Done(VCmd::Kind(VKind::Line)),
            Key::Char('o' | 'O') => VParse::Done(VCmd::Swap),
            Key::Char(c @ ('p' | 'P')) => VParse::Done(VCmd::Put(c == 'p', count, false)),
            Key::Char('g') if matches!(keys.get(i + 1), Some(Key::Char('p' | 'P'))) => {
                let Some(Key::Char(c)) = keys.get(i + 1) else { unreachable!() };
                VParse::Done(VCmd::Put(*c == 'p', count, true))
            }
            Key::Char(op @ ('d' | 'x' | 'X' | 'D' | 'y' | 'Y' | 'c' | 's' | 'S' | 'C' | 'R' | '<' | '>' | '~' | 'u' | 'U' | 'J')) => VParse::Done(VCmd::Op(op, count)),
            Key::Delete => VParse::Invalid,
            Key::Char('r') => match keys.get(i + 1) {
                Some(Key::Char(c)) => VParse::Done(VCmd::Replace(*c)),
                Some(_) => VParse::Invalid,
                None => VParse::Incomplete,
            },
            Key::Char(ia @ ('i' | 'a')) => match keys.get(i + 1) {
                Some(Key::Char(o)) if "wWpP()[]{}<>bB\"'`".contains(*o) => VParse::Done(VCmd::Obj(ia == 'a', *o, count)),
                Some(_) => VParse::Invalid,
                None => VParse::Incomplete,
            },
            _ => match parse_motion(&keys[i..]) {
                MParse::Done(m, _) => VParse::Done(VCmd::Motion(m, count)),
                MParse::Incomplete => VParse::Incomplete,
                MParse::Invalid => VParse::Invalid,
            },
        }
    }

    fn visual_exit(&mut self) {
        self.vis = None;
        self.mode = Mode::Normal;
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    fn visual_exec(&mut self, c: VCmd) {
        let Some(vs) = self.vis else {
            self.mode = Mode::Normal;
            return;
        };
        self.vmarks = Some((vs.anchor, self.cursor, vs.kind));
        match c {
            VCmd::Esc => self.visual_exit(),
            VCmd::Kind(k) => {
                if vs.kind == k {
                    self.visual_exit();
                } else {
                    self.vis = Some(VState { kind: k, ..vs });
                }
            }
            VCmd::Swap => {
                let a = self.cursor;
                self.cursor = vs.anchor;
                self.vis = Some(VState { anchor: a, ..vs });
                self.curswant = self.want_here();
            }
            VCmd::Motion(m, count) => match self.motion(m, count, false) {
                Some(r) => {
                    self.cursor = r.pos;
                    self.fix_cursor();
                    if !r.vertical {
                        self.curswant = if m.c == '$' && !m.g { MAXWANT } else { self.want_here() };
                    }
                    if r.fail {
                        self.failed = true;
                    }
                }
                None => {
                    if m.c == '$' && !m.g {
                        self.curswant = MAXWANT;
                    }
                    self.failed = true;
                }
            },
            VCmd::Obj(include, ch, count) => self.visual_object(include, ch, count.unwrap_or(1)),
            VCmd::Replace(ch) => self.visual_replace(ch),
            VCmd::Op(op, count) => self.visual_operate(op, count),
            VCmd::Put(replace, count, cursor_after) => self.visual_put(replace, count.unwrap_or(1), cursor_after),
        }
    }

    fn visual_object(&mut self, include: bool, ch: char, n: usize) {
        let Some(vs) = self.vis else { return };
        let obj = match ch {
            'w' | 'W' => textobj::current_word(&self.buf, self.cursor, n, include, ch == 'W').ok(),
            'p' | 'P' => textobj::current_par(&self.buf, self.cursor, n, include),
            '(' | ')' | 'b' => textobj::current_block(&self.buf, self.cursor, n, include, '('),
            '[' | ']' => textobj::current_block(&self.buf, self.cursor, n, include, '['),
            '{' | '}' | 'B' => textobj::current_block(&self.buf, self.cursor, n, include, '{'),
            '<' | '>' => textobj::current_block(&self.buf, self.cursor, n, include, '<'),
            q => textobj::current_quote(&self.buf, self.cursor, n, include, q),
        };
        // ip/ap no visual: pode estender a seleção em vez de recomeçar (current_par do Vim)
        if matches!(ch, 'p' | 'P') {
            if let Some(o) = obj {
                let top = o.start.row;
                let extend = top != vs.anchor.row || (vs.kind == VKind::Line && top == self.cursor.row);
                if extend {
                    let back = top < vs.anchor.row;
                    let mut row = top;
                    let last = self.last_row();
                    let white = |r: usize| self.buf.line(r).chars().all(|c| c == ' ' || c == '\t');
                    for _ in 0..n {
                        if row == if back { 0 } else { last } {
                            self.failed = true;
                            break;
                        }
                        let mut prev: Option<bool> = None;
                        for _t in 0..2 {
                            row = if back { row - 1 } else { row + 1 };
                            let sw = white(row);
                            if prev == Some(sw) {
                                row = if back { row + 1 } else { row - 1 };
                                break;
                            }
                            loop {
                                if row == if back { 0 } else { last } {
                                    break;
                                }
                                let nxt = if back { row - 1 } else { row + 1 };
                                if sw != white(nxt) {
                                    break;
                                }
                                row = nxt;
                            }
                            if !include || row == if back { 0 } else { last } {
                                break;
                            }
                            prev = Some(sw);
                        }
                    }
                    self.vis = Some(VState { kind: VKind::Line, ..vs });
                    self.cursor = Pos::new(row, 0);
                    self.fix_cursor();
                    self.curswant = self.want_here();
                    return;
                }
            }
        }
        let Some(o) = obj else {
            self.failed = true;
            return;
        };
        if o.linewise {
            self.vis = Some(VState { kind: VKind::Line, anchor: Pos::new(o.start.row, 0) });
            self.cursor = Pos::new(o.end.row, 0);
        } else {
            // se não for inclusivo o último caractere é o anterior ao fim
            let mut end = o.end;
            if !o.inclusive && o.end > o.start {
                if end.col > 0 {
                    end.col -= 1;
                } else if end.row > 0 {
                    // fim exclusivo na coluna 0: a seleção termina na quebra da linha de cima
                    end = Pos::new(end.row - 1, self.buf.line_len(end.row - 1));
                }
            }
            self.vis = Some(VState { kind: if vs.kind == VKind::Line { VKind::Char } else { vs.kind }, anchor: o.start });
            self.cursor = end;
        }
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    /// Limites da seleção: (início, fim exclusivo, linhas inteiras?).
    fn visual_range(&self) -> (Pos, Pos, bool) {
        let vs = self.vis.unwrap();
        let (a, b) = if vs.anchor <= self.cursor { (vs.anchor, self.cursor) } else { (self.cursor, vs.anchor) };
        if vs.kind == VKind::Line {
            return (Pos::new(a.row, 0), Pos::new(b.row, 0), true);
        }
        let len = self.buf.line_len(b.row);
        let on_nl = b.col >= len;
        let end = if on_nl {
            if b.row < self.last_row() { Pos::new(b.row + 1, 0) } else { Pos::new(b.row, len) }
        } else {
            Pos::new(b.row, b.col + 1)
        };
        (a, end, false)
    }

    fn visual_operate(&mut self, op: char, count: Option<usize>) {
        let (a, end, linewise) = self.visual_range();
        let vs = self.vis.unwrap();
        let sel_start = if vs.anchor <= self.cursor { vs.anchor } else { self.cursor };
        // por linhas: se o começo é a ponta fixa a coluna vira 0; se é o cursor, fica a do cursor
        let lw_start = Pos::new(sel_start.row, if vs.anchor <= self.cursor { 0 } else { sel_start.col });
        let b_row = if vs.anchor <= self.cursor { self.cursor.row } else { vs.anchor.row };
        let rows = b_row - a.row + 1;
        let reg = None;
        // X, D, Y, C, S, R trabalham em linhas
        let (op, as_lines) = match op {
            'x' => ('d', linewise),
            'X' | 'D' => ('d', true),
            'Y' => ('y', true),
            'C' | 'S' | 'R' => ('c', true),
            's' => ('c', linewise),
            o => (o, linewise),
        };
        self.visual_op = true;
        match op {
            'd' | 'y' | 'c' => {
                let (s, e, kind) = if as_lines {
                    let last = if linewise { end.row } else { a.row + rows - 1 };
                    (Pos::new(a.row, 0), Pos::new(last.min(self.last_row()), 0), Kind::Line)
                } else {
                    (a, end, Kind::Excl)
                };
                let cur_before = self.cursor;
                self.mode = Mode::Normal;
                self.vis = None;
                let _ = cur_before;
                if kind == Kind::Line && op == 'y' {
                    self.cursor = lw_start;
                }
                self.apply_operator(op, s, e, kind, reg);
            }
            '>' | '<' => {
                self.vis = None;
                self.mode = Mode::Normal;
                self.checkpoint();
                for _ in 0..count.unwrap_or(1) {
                    for r in a.row..a.row + rows {
                        self.shift_line(r, op == '>');
                    }
                }
                self.cursor = Pos::new(a.row, self.col_for_want(a.row));
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            '~' | 'u' | 'U' => {
                self.vis = None;
                self.mode = Mode::Normal;
                self.checkpoint();
                let (s, e) = if linewise { (Pos::new(a.row, 0), Pos::new(a.row + rows - 1, self.buf.line_len(a.row + rows - 1))) } else { (a, end) };
                let text = self.buf.delete(s, e);
                let f: String = text
                    .chars()
                    .map(|c| match op {
                        'u' => c.to_lowercase().next().unwrap_or(c),
                        'U' => c.to_uppercase().next().unwrap_or(c),
                        _ => {
                            if c.is_lowercase() {
                                c.to_uppercase().next().unwrap_or(c)
                            } else if c.is_uppercase() {
                                c.to_lowercase().next().unwrap_or(c)
                            } else {
                                c
                            }
                        }
                    })
                    .collect();
                self.buf.insert(s, &f);
                self.cursor = if linewise { lw_start } else { sel_start };
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            'J' => {
                self.vis = None;
                self.mode = Mode::Normal;
                self.cursor = if linewise { lw_start } else { sel_start };
                self.fix_cursor();
                let c = self.cursor;
                self.cursor = Pos::new(a.row, 0);
                if !self.join(rows.max(2)) {
                    // não juntou (última linha): o cursor fica no começo da seleção
                    self.cursor = c;
                }
            }
            _ => {}
        }
        self.visual_op = false;
    }

    fn visual_replace(&mut self, ch: char) {
        let (a, end, linewise) = self.visual_range();
        self.vis = None;
        self.mode = Mode::Normal;
        self.checkpoint();
        let (s, e) = if linewise { (Pos::new(a.row, 0), Pos::new(end.row, self.buf.line_len(end.row))) } else { (a, end) };
        let text = self.buf.delete(s, e);
        let f: String = text.chars().map(|c| if c == '\n' { c } else { ch }).collect();
        self.buf.insert(s, &f);
        self.cursor = s;
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    // ------------------------------------------------------------------
    // inserção
    // ------------------------------------------------------------------

    fn start_insert(&mut self, c: char, count: usize) {
        let len = self.buf.line_len(self.cursor.row);
        match c {
            'i' => {}
            'a' => {
                if len > 0 {
                    self.cursor.col += 1;
                }
            }
            'I' => self.cursor.col = self.buf.first_nonblank(self.cursor.row),
            'A' => self.cursor.col = len,
            'o' | 'O' => {
                // o/O guardam o cursor de antes de abrir a linha
                self.checkpoint();
                self.open_line(c == 'o');
                self.begin_insert_state(c, count, Some(self.cursor.row).filter(|r| self.buf.line_len(*r) > 0));
                return;
            }
            _ => {}
        }
        // i/a/I/A guardam o cursor já na posição de inserir
        self.checkpoint();
        self.begin_insert_state(c, count, None);
    }

    fn open_line(&mut self, below: bool) {
        let indent: String = self.buf.line(self.cursor.row).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let row = if below { self.cursor.row + 1 } else { self.cursor.row };
        self.buf.insert_line(row, &indent);
        self.cursor = Pos::new(row, indent.chars().count());
    }

    fn begin_insert_state(&mut self, cmd: char, count: usize, ai_row: Option<usize>) {
        self.mode = Mode::Insert;
        self.ins = Insert { cmd, count, typed: Vec::new(), ai_row };
    }

    fn insert_key(&mut self, k: Key) {
        if self.dot_recording && !self.replaying && k != Key::Esc {
            self.dot.push(k);
        }
        if self.dot_recording && !self.replaying && k == Key::Esc {
            self.dot.push(k);
        }
        match k {
            Key::Esc => {
                self.clear_autoindent();
                // repete o texto digitado (3ia<Esc>)
                if self.ins.count > 1 {
                    let typed = self.ins.typed.clone();
                    let cmd = self.ins.cmd;
                    for _ in 1..self.ins.count {
                        if cmd == 'o' || cmd == 'O' {
                            self.open_line(true);
                        }
                        for key in &typed {
                            self.insert_raw(*key);
                        }
                    }
                }
                self.mode = Mode::Normal;
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
                self.fix_cursor();
                self.curswant = self.want_here();
                self.dot_recording = false;
            }
            _ => {
                self.ins.typed.push(k);
                self.ins.ai_row = if k == Key::Enter { self.ins.ai_row } else { None };
                self.insert_raw(k);
            }
        }
    }

    fn clear_autoindent(&mut self) {
        if let Some(r) = self.ins.ai_row.take() {
            if r == self.cursor.row && self.buf.line(r).chars().all(|c| c == ' ' || c == '\t') {
                let len = self.buf.line_len(r);
                self.buf.delete(Pos::new(r, 0), Pos::new(r, len));
                self.cursor.col = 0;
            }
        }
    }

    fn insert_raw(&mut self, k: Key) {
        match k {
            Key::Char(_) | Key::Tab => {
                let ch = k.as_char().unwrap();
                self.buf.insert(self.cursor, &ch.to_string());
                self.cursor.col += 1;
            }
            Key::Enter => {
                self.clear_autoindent();
                let line = self.buf.line(self.cursor.row);
                // o recuo vem da parte que fica antes do cursor
                let indent: String = line.chars().take(self.cursor.col).take_while(|c| *c == ' ' || *c == '\t').collect();
                let rest: String = line.chars().skip(self.cursor.col).collect();
                let len = self.buf.line_len(self.cursor.row);
                self.buf.delete(self.cursor, Pos::new(self.cursor.row, len));
                let lead = if rest.trim_start().is_empty() || true { indent.clone() } else { String::new() };
                let rest_trim = rest.trim_start_matches([' ', '\t']).to_string();
                self.buf.insert_line(self.cursor.row + 1, &format!("{lead}{rest_trim}"));
                self.cursor = Pos::new(self.cursor.row + 1, lead.chars().count());
                if rest_trim.is_empty() && !lead.is_empty() {
                    self.ins.ai_row = Some(self.cursor.row);
                }
            }
            Key::Backspace => {
                let line = self.buf.line(self.cursor.row);
                let in_indent = self.cursor.col > 0 && line.chars().take(self.cursor.col).all(|c| c == ' ' || c == '\t');
                if in_indent {
                    // 'smarttab': no recuo, volta até o múltiplo anterior de 'shiftwidth'
                    let vcol = motion::vcol_start(&line, self.cursor.col);
                    let target = (vcol - 1) / self.shiftwidth * self.shiftwidth;
                    let mut col = self.cursor.col;
                    while col > 0 && motion::vcol_start(&line, col) > target {
                        col -= 1;
                    }
                    let from = Pos::new(self.cursor.row, col);
                    self.buf.delete(from, self.cursor);
                    self.cursor = from;
                    let have = motion::vcol_start(&line, col);
                    if have < target {
                        self.buf.insert(self.cursor, &" ".repeat(target - have));
                        self.cursor.col += target - have;
                    }
                } else if self.cursor.col > 0 {
                    self.buf.delete(Pos::new(self.cursor.row, self.cursor.col - 1), self.cursor);
                    self.cursor.col -= 1;
                } else if self.cursor.row > 0 {
                    let prev_len = self.buf.line_len(self.cursor.row - 1);
                    let line = self.buf.line(self.cursor.row);
                    self.buf.delete_lines(self.cursor.row, self.cursor.row);
                    self.buf.insert(Pos::new(self.cursor.row - 1, prev_len), &line);
                    self.cursor = Pos::new(self.cursor.row - 1, prev_len);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // desfazer / repetir
    // ------------------------------------------------------------------

    fn checkpoint(&mut self) {
        self.emptied = false;
        if self.replaying && false {
            return;
        }
        self.undo.push(Snap { rope: self.buf.rope.clone(), cursor: self.cursor, marks: self.marks.clone() });
        self.redo.clear();
    }

    /// Linhas iguais no começo e no fim dos dois textos (prefixo, sufixo).
    fn common_lines(a: &Rope, b: &Rope) -> (usize, usize) {
        let (la, lb) = (a.len_lines(), b.len_lines());
        let mut p = 0;
        while p < la.min(lb) && a.line(p) == b.line(p) {
            p += 1;
        }
        let mut q = 0;
        while q < la.min(lb) - p.min(la.min(lb)) && a.line(la - 1 - q) == b.line(lb - 1 - q) {
            q += 1;
        }
        (p, q)
    }

    fn restore(&mut self, snap: Snap, from_undo: bool) {
        self.emptied = false;
        // nada mudou entre os dois textos: volta o cursor guardado
        if self.buf.rope == snap.rope && from_undo {
            self.buf.rope = snap.rope;
            self.buf.rev += 1;
            self.cursor = snap.cursor;
            self.fix_cursor();
            self.curswant = self.want_here();
            return;
        }
        let (p, q) = Self::common_lines(&self.buf.rope, &snap.rope);
        let new_len = snap.rope.len_lines();
        // linhas do trecho que volta (as que o Vim chama de `newsize`)
        let newsize = new_len.saturating_sub(p + q);
        self.buf.rope = snap.rope;
        self.buf.rev += 1;
        let last = self.last_row();
        // se o cursor guardado cai dentro do trecho, volta exatamente para ele;
        // senão vai para a primeira linha que mudou, na coluna desejada
        let (sr, sc) = (snap.cursor.row as i64, snap.cursor.col);
        let inside = from_undo && sr >= p as i64 - 1 && sr <= (p + newsize) as i64;
        if inside {
            self.cursor = Pos::new((snap.cursor.row).min(last), sc);
        } else {
            let row = p.min(last);
            let col = if from_undo && snap.cursor.row == row { sc } else { self.col_for_want(row) };
            self.cursor = Pos::new(row, col);
        }
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    fn undo_step(&mut self, count: usize) {
        for _ in 0..count {
            // sem mais o que desfazer o Vim apita (e descarta as teclas seguintes)
            let Some(snap) = self.undo.pop() else {
                self.failed = true;
                return;
            };
            self.redo.push(Snap { rope: self.buf.rope.clone(), cursor: self.cursor, marks: self.marks.clone() });
            self.marks_back = Some(snap.marks.clone());
            self.restore(snap, true);
        }
    }

    fn redo_step(&mut self, count: usize) {
        for _ in 0..count {
            let Some(snap) = self.redo.pop() else {
                self.failed = true;
                return;
            };
            self.undo.push(Snap { rope: self.buf.rope.clone(), cursor: self.cursor, marks: self.marks.clone() });
            self.marks_back = Some(snap.marks.clone());
            self.restore(snap, false);
        }
    }

    fn dot_repeat(&mut self, count: Option<usize>) {
        if self.dot.is_empty() {
            self.failed = true;
            return;
        }
        let keys = self.dot.clone();
        let n = count.or(self.dot_count);
        if count.is_some() {
            self.dot_count = count;
        }
        let mut full: Vec<Key> = Vec::new();
        if let Some(n) = n {
            full.extend(n.to_string().chars().map(Key::Char));
        }
        full.extend(keys);
        self.replaying = true;
        self.feed(&full);
        self.replaying = false;
    }

    // ------------------------------------------------------------------

    /// No modo normal o cursor fica sobre um caractere (ou na coluna 0 de linha vazia).
    fn fix_cursor(&mut self) {
        self.cursor.row = self.cursor.row.min(self.last_row());
        let len = self.buf.line_len(self.cursor.row);
        match self.mode {
            Mode::Normal => self.cursor.col = self.cursor.col.min(len.saturating_sub(1)),
            // no visual o cursor pode ficar sobre a quebra de linha (col == len)
            Mode::Visual => self.cursor.col = self.cursor.col.min(len),
            Mode::Insert => {}
        }
    }
}

/// Movimentos que gravam a posição anterior na marca de contexto (`''`).
fn is_jump(m: &Mo) -> bool {
    match m.c {
        'G' | 'n' | 'N' | '*' | '#' | '%' | '{' | '}' | '/' | '?' | '\'' | '`' => true,
        'g' => m.g,
        _ => false,
    }
}

/// Teclas gravadas -> texto do registrador (como o Vim: `<Esc>` é ^[, `<CR>` é ^M).
fn macro_text(keys: &[Key]) -> String {
    keys.iter()
        .map(|k| match *k {
            Key::Char(c) => c,
            Key::Esc => '\x1b',
            Key::Enter => '\r',
            Key::Backspace => '\x7f',
            Key::Tab => '\t',
            Key::Ctrl(c) => ((c.to_ascii_lowercase() as u8) & 0x1f) as char,
            Key::Delete => '\u{e000}',
        })
        .collect()
}

/// Texto do registrador -> teclas.
fn macro_keys(text: &str) -> Vec<Key> {
    text.chars()
        .map(|c| match c {
            '\x1b' => Key::Esc,
            '\r' | '\n' => Key::Enter,
            '\x7f' | '\x08' => Key::Backspace,
            '\t' => Key::Tab,
            '\u{e000}' => Key::Delete,
            c if (c as u32) < 0x20 => Key::Ctrl((c as u8 + 0x60) as char),
            c => Key::Char(c),
        })
        .collect()
}

/// Padrão de um movimento `/texto<CR>` / `?texto<CR>` (as teclas começam na barra).
fn extract_search(keys: &[Key], m: Mo) -> Option<(String, bool)> {
    if m.g || !matches!(m.c, '/' | '?') {
        return None;
    }
    let text: String = keys[1..].iter().take_while(|k| **k != Key::Enter).filter_map(|k| k.as_char()).collect();
    Some((text, m.c == '/'))
}

enum MParse {
    Incomplete,
    Invalid,
    Done(Mo, usize),
}

fn parse_motion(keys: &[Key]) -> MParse {
    let Some(&k) = keys.first() else { return MParse::Incomplete };
    let mo = |c: char| MParse::Done(Mo { c, g: false, arg: None }, 1);
    match k {
        Key::Char(c @ ('h' | 'j' | 'k' | 'l' | 'w' | 'W' | 'b' | 'B' | 'e' | 'E' | '0' | '^' | '$' | '_' | 'G' | ';' | ',' | '%' | '{' | '}' | '+' | '-' | ' ')) => mo(c),
        Key::Enter => mo('\r'),
        Key::Char(c @ ('n' | 'N' | '*' | '#')) => mo(c),
        Key::Char(c @ ('/' | '?')) => match keys[1..].iter().position(|k| *k == Key::Enter) {
            Some(n) => MParse::Done(Mo { c, g: false, arg: None }, n + 2),
            None => MParse::Incomplete,
        },
        Key::Char(c @ ('\'' | '`')) => match keys.get(1) {
            Some(Key::Char(a)) => MParse::Done(Mo { c, g: false, arg: Some(*a) }, 2),
            Some(_) => MParse::Invalid,
            None => MParse::Incomplete,
        },
        Key::Char(c @ ('f' | 'F' | 't' | 'T')) => match keys.get(1) {
            Some(Key::Char(a)) => MParse::Done(Mo { c, g: false, arg: Some(*a) }, 2),
            Some(_) => MParse::Invalid,
            None => MParse::Incomplete,
        },
        Key::Char('g') => match keys.get(1) {
            Some(Key::Char(c @ ('g' | 'e' | 'E' | '_' | '*' | '#'))) => MParse::Done(Mo { c: *c, g: true, arg: None }, 2),
            Some(_) => MParse::Invalid,
            None => MParse::Incomplete,
        },
        _ => MParse::Invalid,
    }
}
