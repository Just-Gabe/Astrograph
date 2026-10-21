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
    /// linhas de histórico acima da tela que estão à mostra (0 = no fim)
    pub scroll: usize,
    /// o programa pediu eventos de mouse (vim, htop...)
    pub mouse: Mouse,
    /// o programa aceita colar entre `\e[200~` e `\e[201~`
    pub bracketed: bool,
    /// tela alternativa (vim, less...): sem histórico
    pub alternada: bool,
}

/// O que o programa no terminal quer receber do mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Mouse {
    pub modo: ModoMouse,
    pub sgr: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ModoMouse {
    #[default]
    Nenhum,
    /// só cliques (`?1000`)
    Cliques,
    /// cliques e arrasto com botão (`?1002`)
    Arrasto,
    /// qualquer movimento (`?1003`)
    Tudo,
}

impl TermScreen {
    /// Texto entre dois pontos (linha, coluna) da tela, na ordem de leitura; o fim de cada linha
    /// sem espaços sobrando, linhas separadas por `\n`.
    pub fn texto(&self, a: (usize, usize), b: (usize, usize)) -> String {
        let (ini, fim) = if a <= b { (a, b) } else { (b, a) };
        let mut out = Vec::new();
        for r in ini.0..=fim.0.min(self.rows.len().saturating_sub(1)) {
            let linha = &self.rows[r];
            let c0 = if r == ini.0 { ini.1 } else { 0 };
            let c1 = if r == fim.0 { (fim.1 + 1).min(linha.len()) } else { linha.len() };
            let t: String = linha.get(c0..c1.max(c0)).unwrap_or(&[]).iter().filter(|c| !c.cont).map(|c| c.ch).collect();
            out.push(t.trim_end().to_string());
        }
        out.join("\n")
    }
}

/// Bytes de um evento de mouse para o programa. `botao`: 0 esquerdo, 1 meio, 2 direito, 64 roda
/// para cima, 65 roda para baixo; `solta`: soltou o botão; `mov`: movimento com botão apertado.
/// Colunas e linhas contam a partir de 0.
pub fn mouse_bytes(m: Mouse, botao: u8, col: usize, row: usize, solta: bool, mov: bool) -> Vec<u8> {
    let b = botao + if mov { 32 } else { 0 };
    if m.sgr {
        return format!("\x1b[<{};{};{}{}", b, col + 1, row + 1, if solta { 'm' } else { 'M' }).into_bytes();
    }
    // formato antigo: só até a coluna/linha 222; soltar vira botão 3
    let (b, c, r) = (if solta { 3 } else { b }, col.min(222) as u8 + 33, row.min(222) as u8 + 33);
    vec![0x1b, b'[', b'M', 32 + b, c, r]
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

/// Acha `nome` nas pastas `dirs` do jeito que o sistema o executa. No Windows só serve `.exe`, `.com`,
/// `.cmd` ou `.bat` (nessa ordem): o arquivo sem extensão que o npm deixa ao lado do `.cmd` é um script de
/// shell e dá "não é um aplicativo Win32 válido" (erro 193) se for aberto direto.
pub(crate) fn achar_programa(nome: &str, dirs: &[std::path::PathBuf], windows: bool) -> Option<std::path::PathBuf> {
    const EXTS: [&str; 4] = [".exe", ".com", ".cmd", ".bat"];
    for d in dirs {
        if windows {
            let ja_tem = EXTS.iter().any(|e| nome.to_lowercase().ends_with(e));
            if ja_tem && d.join(nome).is_file() {
                return Some(d.join(nome));
            }
            if let Some(e) = EXTS.iter().find(|e| d.join(format!("{nome}{e}")).is_file()) {
                return Some(d.join(format!("{nome}{e}")));
            }
        } else if d.join(nome).is_file() {
            return Some(d.join(nome));
        }
    }
    None
}

fn dirs_do_path() -> Vec<std::path::PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

pub(crate) fn em_path(nome: &str) -> bool {
    achar_programa(nome, &dirs_do_path(), cfg!(windows)).is_some()
}

/// Caminho do programa a executar (o do PATH, com a extensão certa no Windows); sem achar, o próprio nome.
pub(crate) fn programa(nome: &str) -> String {
    achar_programa(nome, &dirs_do_path(), cfg!(windows)).map_or_else(|| nome.to_string(), |p| p.to_string_lossy().into_owned())
}

/// Comando que roda `cmd` no shell e termina junto com ele (o painel então avisa que o processo acabou).
pub(crate) fn comando_unico(cmd: &str) -> CommandBuilder {
    let (prog, flag) = if cfg!(windows) { ("cmd.exe", "/C") } else { ("sh", "-c") };
    let mut c = CommandBuilder::new(prog);
    c.arg(flag);
    c.arg(cmd);
    c
}

/// Comando para abrir `nome` num pty. No Windows os `.cmd`/`.bat` (como os do npm) rodam por `cmd.exe /C`.
pub(crate) fn comando_pty(nome: &str) -> CommandBuilder {
    let caminho = programa(nome);
    let lower = caminho.to_lowercase();
    if cfg!(windows) && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        let mut c = CommandBuilder::new("cmd.exe");
        c.arg("/C");
        c.arg(caminho);
        c
    } else {
        CommandBuilder::new(caminho)
    }
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
        // digitar leva de volta ao fim do histórico
        {
            let mut p = self.parser.lock().unwrap();
            if p.screen().scrollback() > 0 {
                p.set_scrollback(0);
                self.pending.store(true, Ordering::Release);
            }
        }
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Escreve sem mexer na rolagem (eventos de mouse).
    pub fn escrever_cru(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
        self.pending.store(true, Ordering::Release);
    }

    /// Cola texto: com `bracketed paste` o programa recebe tudo de uma vez, sem executar linhas.
    pub fn colar(&mut self, texto: &str) {
        let bracketed = self.parser.lock().unwrap().screen().bracketed_paste();
        let t = texto.replace("\r\n", "\n").replace('\n', "\r");
        let mut b = Vec::new();
        if bracketed {
            b.extend_from_slice(b"\x1b[200~");
        }
        b.extend_from_slice(t.as_bytes());
        if bracketed {
            b.extend_from_slice(b"\x1b[201~");
        }
        self.write(&b);
    }

    /// Rola o histórico: `d > 0` sobe (para o passado). Devolve o deslocamento atual.
    pub fn rolar(&mut self, d: isize) -> usize {
        let mut p = self.parser.lock().unwrap();
        let atual = p.screen().scrollback() as isize;
        // pede mais do que existe; o `vt100` limita ao tamanho do histórico
        p.set_scrollback((atual + d).max(0) as usize);
        let novo = p.screen().scrollback();
        drop(p);
        self.pending.store(true, Ordering::Release);
        novo
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
        let scroll = s.scrollback();
        let mouse = Mouse {
            modo: match s.mouse_protocol_mode() {
                vt100::MouseProtocolMode::None => ModoMouse::Nenhum,
                vt100::MouseProtocolMode::Press | vt100::MouseProtocolMode::PressRelease => ModoMouse::Cliques,
                vt100::MouseProtocolMode::ButtonMotion => ModoMouse::Arrasto,
                vt100::MouseProtocolMode::AnyMotion => ModoMouse::Tudo,
            },
            sgr: s.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr,
        };
        TermScreen {
            rows: out,
            cursor: (cr as usize, cc as usize),
            // com o histórico à mostra o cursor não está na tela
            cursor_visible: !s.hide_cursor() && scroll == 0,
            exited: self.terminou(),
            scroll,
            mouse,
            bracketed: s.bracketed_paste(),
            alternada: s.alternate_screen(),
        }
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

    fn tela(linhas: &[&str]) -> TermScreen {
        let rows = linhas
            .iter()
            .map(|l| {
                let mut v: Vec<TCell> = l.chars().map(|ch| TCell { ch, ..TCell::VAZIA }).collect();
                v.resize(12, TCell::VAZIA);
                v
            })
            .collect();
        TermScreen { rows, cursor: (0, 0), cursor_visible: true, exited: false, scroll: 0, mouse: Mouse::default(), bracketed: false, alternada: false }
    }

    #[test]
    fn texto_da_selecao_vai_em_ordem_de_leitura_sem_espacos_no_fim() {
        let s = tela(&["abcdef", "ghi", "jklmno"]);
        // uma linha: colunas 1..=3
        assert_eq!(s.texto((0, 1), (0, 3)), "bcd");
        // várias linhas: resto da primeira, a do meio inteira (sem o espaço do fim), começo da última
        assert_eq!(s.texto((0, 4), (2, 1)), "ef\nghi\njk");
        // pontas trocadas dão o mesmo texto
        assert_eq!(s.texto((2, 1), (0, 4)), "ef\nghi\njk");
        // além da última linha: limita
        assert_eq!(s.texto((2, 0), (9, 99)), "jklmno");
    }

    #[test]
    fn eventos_de_mouse_em_sgr_e_no_formato_antigo() {
        let sgr = Mouse { modo: ModoMouse::Cliques, sgr: true };
        assert_eq!(mouse_bytes(sgr, 0, 4, 9, false, false), b"\x1b[<0;5;10M");
        assert_eq!(mouse_bytes(sgr, 0, 4, 9, true, false), b"\x1b[<0;5;10m", "soltar usa m minúsculo");
        assert_eq!(mouse_bytes(sgr, 0, 0, 0, false, true), b"\x1b[<32;1;1M", "arrasto soma 32");
        assert_eq!(mouse_bytes(sgr, 64, 1, 1, false, false), b"\x1b[<64;2;2M", "roda para cima");
        let antigo = Mouse { modo: ModoMouse::Cliques, sgr: false };
        assert_eq!(mouse_bytes(antigo, 0, 0, 0, false, false), vec![0x1b, b'[', b'M', 32, 33, 33]);
        assert_eq!(mouse_bytes(antigo, 0, 0, 0, true, false), vec![0x1b, b'[', b'M', 35, 33, 33], "soltar é o botão 3");
        assert_eq!(mouse_bytes(antigo, 0, 999, 0, false, false)[4], 222 + 33, "coluna limitada");
    }

    #[cfg(unix)]
    #[test]
    fn historico_rola_e_digitar_volta_ao_fim() {
        let mut c = CommandBuilder::new("sh");
        c.args(["-c", "i=1; while [ $i -le 60 ]; do echo linha$i; i=$((i+1)); done; cat"]);
        let mut t = Term::spawn(c, 10, 30, None, Arc::new(|| {})).unwrap();
        let s = esperar(&t, |s| texto(s).contains("linha60"));
        assert_eq!(s.scroll, 0);
        let n = t.rolar(20);
        assert_eq!(n, 20);
        let s = t.snapshot();
        assert_eq!(s.scroll, 20);
        assert!(!s.cursor_visible, "com o histórico à mostra o cursor some");
        let primeira: String = s.rows[0].iter().map(|c| c.ch).collect();
        let num: usize = primeira.trim().trim_start_matches("linha").parse().expect("uma linha do histórico");
        assert!(num < 51, "subiu ao passado: {primeira}");
        // não passa do começo do histórico
        let max = t.rolar(100_000);
        assert!(max >= 50 && max < 100_000, "{max}");
        assert_eq!(t.rolar(-100_000), 0, "desce até o fim");
        // digitar leva ao fim
        t.rolar(10);
        t.write(b"x");
        assert_eq!(t.snapshot().scroll, 0);
        t.write(&[3]);
    }

    #[cfg(unix)]
    #[test]
    fn programa_pede_mouse_e_colar_protegido() {
        let mut c = CommandBuilder::new("sh");
        c.args(["-c", "printf '\\033[?1000h\\033[?1006h\\033[?2004h'; cat"]);
        let mut t = Term::spawn(c, 8, 40, None, Arc::new(|| {})).unwrap();
        let s = esperar(&t, |s| s.bracketed);
        assert!(s.bracketed);
        assert_eq!(s.mouse, Mouse { modo: ModoMouse::Cliques, sgr: true });
        // colar protegido: as quebras de linha viram \r e vem entre \e[200~ e \e[201~
        t.colar("a\nb");
        let s = esperar(&t, |s| texto(s).contains("200~"));
        assert!(texto(&s).contains("200~a") && texto(&s).contains("201~"), "{}", texto(&s));
        t.write(&[3]);
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

    #[test]
    fn no_windows_o_script_sem_extensao_do_npm_nao_vale() {
        let d = std::env::temp_dir().join(format!("astrograph-npm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in ["opencode", "opencode.cmd", "outro", "tool", "tool.exe", "tool.cmd"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        let dirs = vec![d.clone()];
        // Windows: o .cmd, nunca o arquivo sem extensão; .exe vem antes de .cmd; só o script sem extensão = não achou
        assert_eq!(achar_programa("opencode", &dirs, true), Some(d.join("opencode.cmd")));
        assert_eq!(achar_programa("tool", &dirs, true), Some(d.join("tool.exe")));
        assert_eq!(achar_programa("outro", &dirs, true), None);
        assert_eq!(achar_programa("opencode.cmd", &dirs, true), Some(d.join("opencode.cmd")));
        // Linux/macOS: o arquivo com o nome exato
        assert_eq!(achar_programa("outro", &dirs, false), Some(d.join("outro")));
        assert_eq!(achar_programa("nada", &dirs, false), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
