//! O editor modal: estado, análise de comandos do modo normal, operadores,
//! registradores, inserção, desfazer e repetição (`.`).

use std::collections::HashMap;

use ropey::Rope;

use crate::buffer::{Buffer, Pos};
use crate::keys::Key;
use crate::motion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
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
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.buf.lines()
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
            self.key(k);
        }
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
        match self.mode {
            Mode::Insert => self.insert_key(k),
            Mode::Normal => {
                if k == Key::Esc && !self.pending.is_empty() {
                    self.pending.clear();
                    return;
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
        let done = |cmd: Cmd| Parse::Done(Parsed { reg, count, cmd, prefix });
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
                    Some(_) => match parse_motion(&keys[i..]) {
                        MParse::Incomplete => Parse::Incomplete,
                        MParse::Invalid => Parse::Invalid,
                        MParse::Done(m, _) => done(Cmd::Op { op, count2, target: Target::Motion(m) }),
                    },
                }
            }
            _ => match parse_motion(&keys[i - 1..]) {
                MParse::Done(m, _) => done(Cmd::Motion(m)),
                MParse::Incomplete => Parse::Incomplete,
                MParse::Invalid => match k {
                    Key::Char(c @ ('x' | 'X' | 'D' | 'C' | 's' | 'S' | 'Y' | 'p' | 'P' | 'u' | 'J' | 'i' | 'a' | 'I' | 'A' | 'o' | 'O' | '~' | '.')) => {
                        done(Cmd::Simple(c, None))
                    }
                    Key::Char('r') => match keys.get(i) {
                        Some(Key::Char(c)) => done(Cmd::Simple('r', Some(*c))),
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
        let is_change = match &p.cmd {
            Cmd::Motion(_) => false,
            Cmd::Op { op, .. } => *op != 'y',
            Cmd::Simple(c, _) => !matches!(c, 'u' | 'R' | '.' | 'Y'),
        };
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
            'J' => self.join(count1.max(2)),
            'r' => self.replace_chars(arg.unwrap(), count1),
            '~' => self.toggle_case(count1),
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => self.start_insert(c, count1),
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
        if len == 0 {
            0
        } else if self.curswant == MAXWANT {
            len - 1
        } else {
            motion::col_for_vcol(&self.buf.line(row), self.curswant).min(len - 1)
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
                inc(Pos::new(row, if op_pending { l.saturating_sub(1).max(0) } else { l.saturating_sub(1) }))
            }
            '_' => {
                let row = (cur.row + c1 - 1).min(self.last_row());
                Some(Res { pos: Pos::new(row, b.first_nonblank(row).min(b.line_len(row).saturating_sub(1))), kind: Kind::Line, vertical: false, fail: false })
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
            _ => None,
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
                if op == 'd' && kind != Kind::Line && b.row > a.row {
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
        self.checkpoint();
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
            if text.contains('\n') {
                self.cursor = at;
            } else {
                self.cursor = Pos::new(end.row, end.col.saturating_sub(1));
            }
            self.fix_cursor();
            self.curswant = self.want_here();
        }
    }

    fn join(&mut self, count: usize) {
        let row = self.cursor.row;
        let mut count = count;
        if row + count - 1 > self.last_row() {
            // na última linha com 1 ou 2 o Vim apita; com mais, junta o que houver
            if count <= 2 {
                self.failed = true;
                return;
            }
            count = self.last_row() - row + 1;
        }
        self.checkpoint();
        let mut col = 0;
        for _ in 1..count {
            let cur = self.buf.line(row);
            let next = self.buf.line(row + 1);
            let trimmed: String = next.trim_start_matches([' ', '\t']).to_string();
            let cur_len = cur.chars().count();
            let ends_ws = cur.ends_with(' ') || cur.ends_with('\t');
            // sem espaço se a linha já acaba em branco, a próxima é vazia ou começa com ")"
            let add_space = !trimmed.is_empty() && !trimmed.starts_with(')') && !cur.is_empty() && !ends_ws;
            // o cursor fica onde a junção aconteceu (no espaço inserido, ou no último branco)
            col = if add_space {
                cur_len
            } else if ends_ws && !trimmed.is_empty() {
                cur_len - 1
            } else if trimmed.is_empty() {
                cur_len.saturating_sub(1)
            } else {
                cur_len
            };
            let joined = format!("{cur}{}{trimmed}", if add_space { " " } else { "" });
            let len = self.buf.line_len(row);
            self.buf.delete(Pos::new(row, 0), Pos::new(row, len));
            self.buf.insert(Pos::new(row, 0), &joined);
            self.buf.delete_lines(row + 1, row + 1);
        }
        self.cursor = Pos::new(row, col);
        self.fix_cursor();
        self.curswant = self.want_here();
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
        self.undo.push(Snap { rope: self.buf.rope.clone(), cursor: self.cursor });
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
            self.redo.push(Snap { rope: self.buf.rope.clone(), cursor: self.cursor });
            self.restore(snap, true);
        }
    }

    fn redo_step(&mut self, count: usize) {
        for _ in 0..count {
            let Some(snap) = self.redo.pop() else {
                self.failed = true;
                return;
            };
            self.undo.push(Snap { rope: self.buf.rope.clone(), cursor: self.cursor });
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
        if self.mode == Mode::Normal {
            let len = self.buf.line_len(self.cursor.row);
            self.cursor.col = self.cursor.col.min(len.saturating_sub(1));
        }
    }
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
        Key::Char(c @ ('f' | 'F' | 't' | 'T')) => match keys.get(1) {
            Some(Key::Char(a)) => MParse::Done(Mo { c, g: false, arg: Some(*a) }, 2),
            Some(_) => MParse::Invalid,
            None => MParse::Incomplete,
        },
        Key::Char('g') => match keys.get(1) {
            Some(Key::Char(c @ ('g' | 'e' | 'E' | '_'))) => MParse::Done(Mo { c: *c, g: true, arg: None }, 2),
            Some(_) => MParse::Invalid,
            None => MParse::Incomplete,
        },
        _ => MParse::Invalid,
    }
}
