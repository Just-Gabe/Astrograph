//! Terminal nativo: um pty com o shell (ou um agente) e a emulação VT por
//! `vt100`. A UI só vê um instantâneo da tela (`TermScreen`); o resto roda em
//! uma thread de leitura. Substitui o `:terminal` do Neovim e os terminais
//! flutuantes dos agentes.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

/// Uma célula da tela do terminal. Cores `None` = padrão do tema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TCell {
    pub ch: char,
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// segunda metade de um caractere largo (não desenha)
    pub cont: bool,
}

impl TCell {
    const VAZIA: TCell = TCell { ch: ' ', fg: None, bg: None, bold: false, italic: false, underline: false, cont: false };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermScreen {
    pub rows: Vec<Vec<TCell>>,
    pub cursor: (usize, usize),
    pub cursor_visible: bool,
    pub exited: bool,
}

pub struct Term {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    child: Box<dyn Child + Send + Sync>,
    /// a thread de leitura já avisou a UI e ela ainda não pegou o instantâneo
    pub pending: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    pub rows: u16,
    pub cols: u16,
}

/// Comando do shell do usuário (`$SHELL`, senão `sh`; no Windows PowerShell).
pub fn shell_padrao() -> CommandBuilder {
    #[cfg(windows)]
    {
        let mut c = CommandBuilder::new("powershell.exe");
        c.arg("-NoLogo");
        c
    }
    #[cfg(not(windows))]
    {
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "sh".into()))
    }
}

/// O primeiro programa de IA instalado (claude, opencode, codex, gemini).
pub fn agente_instalado() -> Option<&'static str> {
    ["claude", "opencode", "codex", "gemini"].into_iter().find(|n| em_path(n))
}

fn em_path(nome: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&path).any(|d| exts.iter().any(|e| d.join(format!("{nome}{e}")).is_file()))
}

impl Term {
    /// Abre o pty com `cmd`. `notify` é chamado (de outra thread) quando há saída nova ou o
    /// processo terminou.
    pub fn spawn(mut cmd: CommandBuilder, rows: u16, cols: u16, cwd: Option<&std::path::Path>, notify: Arc<dyn Fn() + Send + Sync>) -> Result<Term, String> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(d) = cwd {
            cmd.cwd(d);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let pending = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let (parser, pending, exited) = (parser.clone(), pending.clone(), exited.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 16384];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            parser.lock().unwrap().process(&buf[..n]);
                            // só avisa uma vez por instantâneo: rajadas de saída não inundam a UI
                            if !pending.swap(true, Ordering::AcqRel) {
                                notify();
                            }
                        }
                    }
                }
                exited.store(true, Ordering::Release);
                pending.store(true, Ordering::Release);
                notify();
            });
        }
        Ok(Term { writer, master: pair.master, parser, child, pending, exited, rows, cols })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if (rows, cols) == (self.rows, self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.parser.lock().unwrap().set_size(rows, cols);
    }

    pub fn terminou(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }

    pub fn matar(&mut self) {
        let _ = self.child.kill();
    }

    /// Instantâneo da tela (e zera o aviso pendente).
    pub fn snapshot(&self) -> TermScreen {
        self.pending.store(false, Ordering::Release);
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        let (rows, cols) = s.size();
        let mut out = Vec::with_capacity(rows as usize);
        for r in 0..rows {
            let mut linha = Vec::with_capacity(cols as usize);
            for c in 0..cols {
                let Some(cell) = s.cell(r, c) else {
                    linha.push(TCell::VAZIA);
                    continue;
                };
                let t = cell.contents();
                let (mut fg, mut bg) = (cor(cell.fgcolor()), cor(cell.bgcolor()));
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                    // inverso com cores padrão: texto escuro sobre claro
                    fg = fg.or(Some(0x1e1e22));
                    bg = bg.or(Some(0xdfdfe0));
                }
                linha.push(TCell {
                    ch: t.chars().next().unwrap_or(' '),
                    fg,
                    bg,
                    bold: cell.bold(),
                    italic: cell.italic(),
                    underline: cell.underline(),
                    cont: cell.is_wide_continuation(),
                });
            }
            out.push(linha);
        }
        let (cr, cc) = s.cursor_position();
        TermScreen { rows: out, cursor: (cr as usize, cc as usize), cursor_visible: !s.hide_cursor(), exited: self.terminou() }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        self.matar();
    }
}

/// Paleta ANSI de 16 cores, num tom que combina com o tema escuro.
const ANSI: [u32; 16] = [
    0x2c2c31, 0xff453a, 0x30d158, 0xffd60a, 0x0a84ff, 0xbf5af2, 0x64d2ff, 0xd1d1d6, //
    0x58585f, 0xff6961, 0x4be36f, 0xffe14d, 0x5eb0ff, 0xd28cff, 0x8fe0ff, 0xf5f5f7,
];

fn cor(c: vt100::Color) -> Option<u32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Rgb(r, g, b) => Some(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
        vt100::Color::Idx(i) => Some(match i {
            0..=15 => ANSI[i as usize],
            16..=231 => {
                let n = i as u32 - 16;
                let lv = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
                (lv(n / 36) << 16) | (lv((n / 6) % 6) << 8) | lv(n % 6)
            }
            _ => {
                let g = 8 + (i as u32 - 232) * 10;
                (g << 16) | (g << 8) | g
            }
        }),
    }
}

/// Teclas em notação do editor (`ls<CR>`, `<C-c>`, `<Up>`) -> bytes para o pty.
pub fn teclas_para_bytes(keys: &str) -> Vec<u8> {
    let chars: Vec<char> = keys.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(j) = chars[i..].iter().take(14).position(|&c| c == '>') {
                let nome: String = chars[i + 1..i + j].iter().collect::<String>();
                if let Some(b) = nomeada(&nome) {
                    out.extend(b);
                    i += j + 1;
                    continue;
                }
            }
        }
        let mut b = [0u8; 4];
        out.extend(chars[i].encode_utf8(&mut b).as_bytes());
        i += 1;
    }
    out
}

fn nomeada(nome: &str) -> Option<Vec<u8>> {
    let low = nome.to_lowercase();
    let seq = |s: &str| Some(s.as_bytes().to_vec());
    match low.as_str() {
        "esc" => seq("\x1b"),
        "cr" | "enter" | "return" => seq("\r"),
        "bs" | "backspace" => seq("\x7f"),
        "tab" => seq("\t"),
        "s-tab" => seq("\x1b[Z"),
        "space" => seq(" "),
        "c-bslash" => Some(vec![0x1c]),
        "lt" => seq("<"),
        "bar" => seq("|"),
        "bslash" => seq("\\"),
        "del" | "delete" => seq("\x1b[3~"),
        "up" => seq("\x1b[A"),
        "down" => seq("\x1b[B"),
        "right" => seq("\x1b[C"),
        "left" => seq("\x1b[D"),
        "home" => seq("\x1b[H"),
        "end" => seq("\x1b[F"),
        "pageup" => seq("\x1b[5~"),
        "pagedown" => seq("\x1b[6~"),
        "insert" => seq("\x1b[2~"),
        n if n.len() >= 3 && n.starts_with("c-") && n.chars().count() == 3 => {
            let c = n.chars().nth(2)?;
            match c {
                'a'..='z' => Some(vec![c as u8 - b'a' + 1]),
                '[' => seq("\x1b"),
                '\\' => Some(vec![0x1c]),
                ']' => Some(vec![0x1d]),
                '^' => Some(vec![0x1e]),
                '_' => Some(vec![0x1f]),
                ' ' | '@' => Some(vec![0]),
                _ => None,
            }
        }
        n if n.starts_with("a-") || n.starts_with("m-") => {
            let rest = &nome[2..];
            let mut v = vec![0x1b];
            v.extend(teclas_para_bytes(&if rest.chars().count() == 1 { rest.to_string() } else { format!("<{rest}>") }));
            Some(v)
        }
        n if n.starts_with('f') && n[1..].parse::<u8>().is_ok() => {
            let f: u8 = n[1..].parse().ok()?;
            match f {
                1..=4 => Some(vec![0x1b, b'O', b'P' + f - 1]),
                5 => seq("\x1b[15~"),
                6 => seq("\x1b[17~"),
                7 => seq("\x1b[18~"),
                8 => seq("\x1b[19~"),
                9 => seq("\x1b[20~"),
                10 => seq("\x1b[21~"),
                11 => seq("\x1b[23~"),
                12 => seq("\x1b[24~"),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::{Duration, Instant};

    fn esperar(t: &Term, f: impl Fn(&TermScreen) -> bool) -> TermScreen {
        let fim = Instant::now() + Duration::from_secs(5);
        loop {
            let s = t.snapshot();
            if f(&s) || Instant::now() > fim {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn texto(s: &TermScreen) -> String {
        s.rows.iter().map(|r| r.iter().map(|c| c.ch).collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
    }

    #[cfg(unix)]
    #[test]
    fn roda_um_programa_e_mostra_a_saida_colorida() {
        let mut c = CommandBuilder::new("sh");
        c.args(["-c", "printf 'ola \\033[31mvermelho\\033[0m\\n'; echo $((20+22)); sleep 0.2"]);
        let t = Term::spawn(c, 10, 40, None, Arc::new(|| {})).unwrap();
        let s = esperar(&t, |s| texto(s).contains("42"));
        let tx = texto(&s);
        assert!(tx.contains("ola vermelho") && tx.contains("42"), "{tx}");
        // a palavra vermelho está em ANSI 1 (vermelho do tema)
        let c = s.rows[0][4];
        assert_eq!((c.ch, c.fg), ('v', Some(0xff453a)));
        assert_eq!(s.rows[0][0].fg, None);
    }

    #[cfg(unix)]
    #[test]
    fn digitar_no_pty_ecoa_e_redimensiona() {
        let mut t = Term::spawn(CommandBuilder::new("cat"), 8, 30, None, Arc::new(|| {})).unwrap();
        t.write(&teclas_para_bytes("abc<CR>"));
        let s = esperar(&t, |s| texto(s).lines().filter(|l| l.contains("abc")).count() >= 2);
        assert!(texto(&s).lines().filter(|l| l.contains("abc")).count() >= 2, "{}", texto(&s));
        t.resize(12, 50);
        assert_eq!((t.snapshot().rows.len(), t.snapshot().rows[0].len()), (12, 50));
        t.write(&teclas_para_bytes("<C-d>"));
        let s = esperar(&t, |s| s.exited);
        assert!(s.exited);
    }

    #[test]
    fn teclas_viram_bytes() {
        assert_eq!(teclas_para_bytes("ls<CR>"), b"ls\r");
        assert_eq!(teclas_para_bytes("<C-c>"), vec![3]);
        assert_eq!(teclas_para_bytes("<Up><Esc>"), b"\x1b[A\x1b");
        assert_eq!(teclas_para_bytes("a<lt>b"), b"a<b");
        assert_eq!(teclas_para_bytes("é"), "é".as_bytes());
        assert_eq!(teclas_para_bytes("<A-x>"), b"\x1bx");
        assert_eq!(teclas_para_bytes("<F5>"), b"\x1b[15~");
        assert_eq!(teclas_para_bytes("a < b"), b"a < b");
    }

    #[test]
    fn cores_256_e_cinza() {
        assert_eq!(cor(vt100::Color::Idx(1)), Some(ANSI[1]));
        assert_eq!(cor(vt100::Color::Idx(16)), Some(0x000000));
        assert_eq!(cor(vt100::Color::Idx(231)), Some(0xffffff));
        assert_eq!(cor(vt100::Color::Idx(244)), Some(0x808080));
        assert_eq!(cor(vt100::Color::Default), None);
    }
}
