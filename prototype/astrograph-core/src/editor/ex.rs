//! Comandos `:` do núcleo: endereços de linha, `:s` (substituir), `:d`, `:y`, `:j`,
//! `:>`, `:<`, `:g`/`:v` e `:normal`. O que não é daqui (`:w`, `:q`, `:e`...) volta
//! para quem hospeda o editor (`take_host_cmd`).

use super::*;

/// Endereços de uma linha de comando: `(inicial, final)` (base 0) e se algum foi dado.
struct Faixa {
    a: usize,
    b: usize,
    dado: bool,
}

/// Opções booleanas que o núcleo trata (para reconhecer o prefixo `no`).
fn bool_do_nucleo(n: &str) -> bool {
    matches!(n, "ignorecase" | "ic" | "smartcase" | "scs" | "expandtab" | "et")
}

impl Editor {
    pub fn cmdline(&self) -> Option<&str> {
        self.cmdline.as_deref()
    }

    /// Comando que o editor não entende (`w`, `q`, `e`...): quem hospeda executa.
    pub fn take_host_cmd(&mut self) -> Option<String> {
        self.host_cmd.take()
    }

    /// Digitação na linha de comando. `true` se a tecla foi consumida.
    pub(super) fn cmdline_key(&mut self, k: Key) -> bool {
        let Some(cl) = self.cmdline.as_mut() else { return false };
        match k {
            Key::Esc | Key::Ctrl('c') => self.cmdline = None,
            Key::Enter => {
                let linha = self.cmdline.take().unwrap_or_default();
                if !linha.is_empty() {
                    self.last_cmdline = Some(linha.clone());
                    self.regs.insert(':', super::Reg { text: linha.clone(), linewise: false, block: false, bwidth: 0 });
                }
                self.run_cmdline(&linha);
            }
            Key::Backspace => {
                if cl.pop().is_none() {
                    self.cmdline = None;
                }
            }
            Key::Char(c) => cl.push(c),
            Key::Tab => cl.push('\t'),
            _ => {}
        }
        true
    }

    /// `:` no modo normal ou visual abre a linha de comando.
    pub(super) fn cmdline_open(&mut self) {
        if self.mode == Mode::Visual {
            if let Some(vs) = self.vis {
                self.vmarks = Some((vs.anchor, self.cursor, vs.kind));
            }
            self.visual_exit();
            self.cmdline = Some("'<,'>".into());
        } else {
            self.cmdline = Some(String::new());
        }
    }

    /// Lê um endereço (`12`, `.`, `$`, `'a`, `+3`...) a partir de `i`.
    fn endereco(&self, s: &[char], i: &mut usize) -> Option<Option<usize>> {
        let n = self.buf.line_count();
        let mut base: Option<i64> = None;
        match s.get(*i) {
            Some(c) if c.is_ascii_digit() => {
                let mut v = 0i64;
                while let Some(d) = s.get(*i).and_then(|c| c.to_digit(10)) {
                    v = v * 10 + d as i64;
                    *i += 1;
                }
                base = Some(v - 1);
            }
            Some('.') => {
                *i += 1;
                base = Some(self.cursor.row as i64);
            }
            Some('$') => {
                *i += 1;
                base = Some(n as i64 - 1);
            }
            Some('\'') => {
                let m = *s.get(*i + 1)?;
                *i += 2;
                match self.mark_pos(m) {
                    Some(p) => base = Some(p.row as i64),
                    None => return None,
                }
            }
            _ => {}
        }
        let mut deslocou = false;
        while let Some(&c) = s.get(*i) {
            if c != '+' && c != '-' {
                break;
            }
            *i += 1;
            let mut v = 0i64;
            let mut tem = false;
            while let Some(d) = s.get(*i).and_then(|c| c.to_digit(10)) {
                v = v * 10 + d as i64;
                *i += 1;
                tem = true;
            }
            let v = if tem { v } else { 1 };
            let b = base.unwrap_or(self.cursor.row as i64);
            base = Some(if c == '+' { b + v } else { b - v });
            deslocou = true;
        }
        let _ = deslocou;
        // fora do arquivo: E16 Invalid range (o endereço `0` vale a primeira linha)
        match base {
            Some(b) if b >= 0 && (b as usize) < n => Some(Some(b as usize)),
            Some(-1) if s.get(0).is_some() && !deslocou => Some(Some(0)),
            Some(_) => None,
            None => Some(None),
        }
    }

    /// Faixa de linhas no começo do comando; devolve também onde o nome do comando começa.
    fn faixa(&self, s: &[char]) -> Option<(Faixa, usize)> {
        let mut i = 0;
        while s.get(i) == Some(&' ') || s.get(i) == Some(&':') {
            i += 1;
        }
        let n = self.buf.line_count();
        if s.get(i) == Some(&'%') {
            return Some((Faixa { a: 0, b: n - 1, dado: true }, i + 1));
        }
        let mut dados: Vec<usize> = Vec::new();
        loop {
            match self.endereco(s, &mut i)? {
                Some(v) => dados.push(v),
                None => {
                    // `,` sem endereço antes: usa a linha atual
                    if s.get(i) == Some(&',') {
                        dados.push(self.cursor.row);
                    }
                }
            }
            if s.get(i) == Some(&',') || s.get(i) == Some(&';') {
                i += 1;
                if s.get(i).is_none_or(|c| c.is_alphabetic()) && dados.len() == 1 {
                    dados.push(self.cursor.row);
                    break;
                }
                continue;
            }
            break;
        }
        let dado = !dados.is_empty();
        let (a, b) = match dados.len() {
            0 => (self.cursor.row, self.cursor.row),
            1 => (dados[0], dados[0]),
            _ => (dados[dados.len() - 2], dados[dados.len() - 1]),
        };
        // faixa invertida: o Vim pergunta; aqui troca
        let (a, b) = if a > b { (b, a) } else { (a, b) };
        Some((Faixa { a, b, dado }, i))
    }

    pub(super) fn run_cmdline(&mut self, linha: &str) {
        let s: Vec<char> = linha.chars().collect();
        let Some((faixa, mut i)) = self.faixa(&s) else {
            self.failed = true;
            return;
        };
        while s.get(i) == Some(&' ') {
            i += 1;
        }
        let nome_ini = i;
        while s.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
            i += 1;
        }
        let mut nome: String = s[nome_ini..i].iter().collect();
        // `s/x/y/`, `g/x/d`, `>`, `<`: o nome é uma letra seguida do delimitador
        if nome.is_empty() {
            if let Some(&c) = s.get(i) {
                if c == '>' || c == '<' || c == '&' || c == '~' {
                    nome = c.to_string();
                    i += 1;
                }
            }
        } else if !matches!(nome.as_str(), "d" | "delete" | "y" | "yank" | "j" | "join" | "normal" | "norm" | "noh" | "nohlsearch" | "w" | "write" | "q" | "quit" | "wq" | "x" | "xit" | "e" | "edit" | "set" | "syntax" | "filetype" | "sp" | "split" | "vs" | "vsp" | "vsplit" | "vne" | "vnew" | "new" | "clo" | "close" | "on" | "only" | "vm" | "vmap" | "vn" | "vnoremap" | "vu" | "vunmap" | "t" | "co" | "copy" | "m" | "move" | "sort" | "sor") {
            // `sx`, `gx`: nome de uma letra colado ao delimitador (`s#a#b#`, `g!/x/d`)
            let primeiro = nome.chars().next().unwrap();
            if matches!(primeiro, 's' | 'g' | 'v') && nome.len() > 1 && !nome.starts_with("set") && !nome.starts_with("sy") {
                // `sub`, `substitute`, `global`, `vglobal`: nomes completos
                if !matches!(nome.as_str(), "sub" | "substitute" | "global" | "vglobal") {
                    i = nome_ini + 1;
                    nome = primeiro.to_string();
                }
            }
        }
        let resto: String = s[i.min(s.len())..].iter().collect();
        let resto_t = resto.trim_start().to_string();
        match nome.as_str() {
            "" => {
                // só endereço: vai para a linha
                if faixa.dado {
                    // 'nostartofline': o cursor mantém a coluna desejada
                    self.cursor = Pos::new(faixa.b, self.col_for_want(faixa.b));
                    self.fix_cursor();
                }
            }
            "s" | "sub" | "substitute" => {
                if resto.is_empty() || resto.starts_with(|c: char| c.is_ascii_alphabetic() || c == '&') {
                    // `:s` e `:s g`: repete a última substituição
                    self.repetir_substituicao(faixa.a, faixa.b, &resto, false);
                } else {
                    self.substituir(faixa.a, faixa.b, &resto);
                }
            }
            "&" => self.repetir_substituicao(faixa.a, faixa.b, &resto, false),
            "~" => self.repetir_substituicao(faixa.a, faixa.b, &resto, true),
            "d" | "delete" => self.ex_apagar(faixa.a, faixa.b),
            "y" | "yank" => {
                let texto = (faixa.a..=faixa.b).map(|r| self.buf.line(r)).collect::<Vec<_>>().join("\n");
                self.store(None, texto, true, false);
            }
            "j" | "join" => {
                let b = if faixa.a == faixa.b { (faixa.b + 1).min(self.last_row()) } else { faixa.b };
                if b > faixa.a {
                    self.cursor = Pos::new(faixa.a, 0);
                    self.join(b - faixa.a + 1);
                    let r = self.cursor.row;
                    self.cursor = Pos::new(r, self.buf.first_nonblank(r));
                    self.fix_cursor();
                    self.curswant = self.want_here();
                }
            }
            ">" | "<" => {
                let n = 1 + resto.chars().take_while(|c| *c == nome.chars().next().unwrap()).count();
                self.checkpoint();
                for r in faixa.a..=faixa.b {
                    for _ in 0..n {
                        self.shift_line(r, nome == ">");
                    }
                }
 self.cursor = Pos::new(faixa.b, self.buf.first_nonblank(faixa.b));
                self.fix_cursor();
            }
            "set" | "se" => self.ex_set(&resto),
            "t" | "co" | "copy" => self.ex_copiar(faixa.a, faixa.b, &resto),
            "m" | "move" => self.ex_mover(faixa.a, faixa.b, &resto),
            "sort" | "sor" => {
                let (a, b) = if faixa.dado { (faixa.a, faixa.b) } else { (0, self.last_row()) };
                self.ex_ordenar(a, b, &resto);
            }
            "map" | "nm" | "nmap" | "im" | "imap" | "vm" | "vmap" | "xm" | "xmap" | "no" | "noremap" | "nn" | "nnoremap" | "ino" | "inoremap" | "vn" | "vnoremap" | "xn" | "xnoremap" | "unm" | "unmap" | "nun" | "nunmap"
            | "iu" | "iunmap" | "vu" | "vunmap" | "xu" | "xunmap" | "mapc" | "mapclear" => self.ex_mapa(&nome, &resto),
            "normal" | "norm" => {
                let teclas = resto.strip_prefix(' ').unwrap_or(&resto).to_string();
                self.ex_normal(faixa.a, faixa.b, &teclas, faixa.dado);
            }
            "g" | "global" | "v" | "vglobal" => {
                let inverso = nome.starts_with('v') || resto.starts_with('!');
                let r = resto.strip_prefix('!').unwrap_or(&resto).to_string();
                let (a, b) = if faixa.dado { (faixa.a, faixa.b) } else { (0, self.last_row()) };
                self.global(a, b, &r, inverso);
            }
            _ => self.host_cmd = Some(linha.to_string()),
        }
        let _ = resto_t;
    }

    /// `:set opção`, `:set noopção`, `:set opção!`, `:set opção=valor` (vários na mesma linha). As que o
    /// núcleo não conhece (`number`, `scrolloff`, `clipboard`...) voltam para quem hospeda (`take_host_cmd`).
    /// `:set` vindo de fora (`.editorconfig`): só as opções que o núcleo conhece; as outras são ignoradas.
    pub fn set_option(&mut self, args: &str) {
        self.ex_set(args);
        self.host_cmd = None;
    }

    fn ex_set(&mut self, args: &str) {
        let mut fora: Vec<&str> = Vec::new();
        for t in args.split_whitespace() {
            let (nome, valor) = match t.split_once(['=', ':']) {
                Some((n, v)) => (n, Some(v)),
                None => (t, None),
            };
            let (nome, inverte) = match nome.strip_suffix('!') {
                Some(n) => (n, true),
                None => (nome, false),
            };
            let (base, liga) = match nome.strip_prefix("no").filter(|r| !r.is_empty()) {
                Some(r) if bool_do_nucleo(r) => (r, false),
                _ => (nome, true),
            };
            let num = valor.and_then(|v| v.parse::<usize>().ok());
            match (base, num) {
                ("shiftwidth" | "sw", Some(n)) if n > 0 => self.shiftwidth = n,
                ("tabstop" | "ts", Some(n)) if n > 0 => motion::set_tabstop(n),
                ("ignorecase" | "ic", _) => self.ignorecase = if inverte { !self.ignorecase } else { liga },
                ("smartcase" | "scs", _) => self.smartcase = if inverte { !self.smartcase } else { liga },
                ("expandtab" | "et", _) => self.expandtab = if inverte { !self.expandtab } else { liga },
                _ => fora.push(t),
            }
        }
        if !fora.is_empty() {
            self.host_cmd = Some(format!("set {}", fora.join(" ")));
        }
    }

    /// `:map`, `:nnoremap <silent> <Leader>w :w<CR>`, `:unmap`, `:mapclear`...
    fn ex_mapa(&mut self, nome: &str, args: &str) {
        use super::mapas::{MODO_I, MODO_N, MODO_V};
        let modos = match nome.chars().next() {
            Some('n') if nome != "no" && !nome.starts_with("noremap") => MODO_N,
            Some('i') => MODO_I,
            Some('v' | 'x') => MODO_V,
            _ => MODO_N | MODO_V,
        };
        let desmapear = nome.contains("unm") || matches!(nome, "nun" | "iu" | "iunmap" | "vu" | "xu" | "xunmap" | "vunmap" | "nunmap");
        if nome.starts_with("mapc") {
            self.limpar_mapas(MODO_N | MODO_V);
            return;
        }
        let recursivo = !nome.contains("no") || nome.starts_with("nm");
        // tira <silent>, <buffer>, <nowait>, <unique>
        let mut resto = args.trim_start();
        loop {
            let low = resto.to_lowercase();
            let Some(n) = ["<silent>", "<buffer>", "<nowait>", "<unique>"].iter().find(|o| low.starts_with(*o)).map(|o| o.len()) else { break };
            resto = resto[n..].trim_start();
        }
        let (lhs, rhs) = match resto.split_once(char::is_whitespace) {
            Some((l, r)) => (l, r.trim_start()),
            None => (resto, ""),
        };
        if desmapear {
            if lhs.is_empty() || !self.desmapear(modos, lhs) {
                self.failed = true;
            }
            return;
        }
        if lhs.is_empty() {
            // `:map` sozinho lista (a primeira linha de `msg`, para quem quiser mostrar)
            self.msg = self.listar_mapas().join("\n");
            return;
        }
        if rhs.is_empty() {
            return;
        }
        self.mapear(modos, lhs, rhs, recursivo);
    }

    /// Destino de `:t` e `:m`: o número da linha depois da qual inserir (`0` = antes da primeira).
    fn ex_destino(&self, resto: &str) -> Option<i64> {
        let s: Vec<char> = resto.trim().chars().collect();
        if s.is_empty() {
            return None;
        }
        if s.iter().all(|c| *c == '0') {
            return Some(-1);
        }
        let mut i = 0;
        let r = self.endereco(&s, &mut i)??;
        (i == s.len()).then_some(r as i64)
    }

    fn ex_copiar(&mut self, a: usize, b: usize, resto: &str) {
        let Some(dest) = self.ex_destino(resto) else {
            self.failed = true;
            return;
        };
        let old = self.buf.rope.clone();
        self.checkpoint();
        let linhas: Vec<String> = (a..=b).map(|r| self.buf.line(r)).collect();
        let ini = (dest + 1) as usize;
        for (k, l) in linhas.iter().enumerate() {
            self.buf.insert_line(ini + k, l);
        }
        self.adjust_marks(&old);
        let r = ini + linhas.len() - 1;
        self.cursor = Pos::new(r, self.col_for_want(r));
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    fn ex_mover(&mut self, a: usize, b: usize, resto: &str) {
        let Some(dest) = self.ex_destino(resto) else {
            self.failed = true;
            return;
        };
        // E134: não dá para mover para dentro do próprio trecho
        if dest >= a as i64 && dest < b as i64 {
            self.failed = true;
            return;
        }
        let n = b - a + 1;
        let fim = if dest >= b as i64 {
            let linhas: Vec<String> = (a..=b).map(|r| self.buf.line(r)).collect();
            self.checkpoint();
            let ini = (dest + 1) as usize;
            for (k, l) in linhas.iter().enumerate() {
                self.buf.insert_line(ini + k, l);
            }
            self.buf.delete_lines(a, b);
            dest as usize
        } else if dest + 1 == a as i64 {
            b
        } else {
            let linhas: Vec<String> = (a..=b).map(|r| self.buf.line(r)).collect();
            self.checkpoint();
            self.buf.delete_lines(a, b);
            let ini = (dest + 1) as usize;
            for (k, l) in linhas.iter().enumerate() {
                self.buf.insert_line(ini + k, l);
            }
            ini + n - 1
        };
        let r = fim.min(self.last_row());
        self.cursor = Pos::new(r, self.col_for_want(r));
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    /// `:sort[!] [n] [i] [u]` (sem `/padrão/` nem `x`/`o`/`b`/`f`).
    fn ex_ordenar(&mut self, a: usize, b: usize, args: &str) {
        let (mut rev, mut num, mut ic, mut uniq) = (false, false, false, false);
        let args = args.trim_start();
        let args = match args.strip_prefix('!') {
            Some(r) => {
                rev = true;
                r
            }
            None => args,
        };
        for c in args.chars() {
            match c {
                'n' => num = true,
                'i' => ic = true,
                'u' => uniq = true,
                ' ' => {}
                _ => {
                    self.failed = true;
                    return;
                }
            }
        }
        let numero = |l: &str| -> Option<i128> {
            let cs: Vec<char> = l.chars().collect();
            let i = cs.iter().position(|c| c.is_ascii_digit())?;
            let neg = i > 0 && cs[i - 1] == '-';
            let v: i128 = cs[i..].iter().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
            Some(if neg { -v } else { v })
        };
        let chave = |l: &str| -> String { if ic { l.to_lowercase() } else { l.to_string() } };
        let mut linhas: Vec<String> = (a..=b).map(|r| self.buf.line(r)).collect();
        let original = linhas.clone();
        if num {
            linhas.sort_by(|x, y| numero(x).cmp(&numero(y)));
        } else {
            linhas.sort_by(|x, y| chave(x).as_bytes().cmp(chave(y).as_bytes()));
        }
        if rev {
            // o Vim percorre o resultado de trás para frente (empates também invertem)
            linhas.reverse();
        }
        if uniq {
            let mut fora: Vec<String> = Vec::new();
            for l in linhas {
                let igual = fora.last().is_some_and(|p| if num { numero(p) == numero(&l) } else { chave(p) == chave(&l) });
                if !igual {
                    fora.push(l);
                }
            }
            linhas = fora;
        }
        if linhas != original {
            let old = self.buf.rope.clone();
            self.checkpoint();
            self.buf.delete_lines(a, b);
            if self.buf.line_count() == 1 && self.buf.line(0).is_empty() && a == 0 && b + 1 >= old.len_lines() {
                // buffer todo apagado: sobrou uma linha vazia
                self.buf.delete(Pos::new(0, 0), Pos::new(0, 0));
                for (k, l) in linhas.iter().enumerate() {
                    if k == 0 {
                        self.buf.insert(Pos::new(0, 0), l);
                    } else {
                        self.buf.insert_line(k, l);
                    }
                }
            } else {
                for (k, l) in linhas.iter().enumerate() {
                    self.buf.insert_line(a + k, l);
                }
            }
            self.adjust_marks(&old);
        }
        self.cursor = Pos::new(a.min(self.last_row()), 0);
        let r = self.cursor.row;
        self.cursor = Pos::new(r, self.buf.first_nonblank(r));
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    fn ex_apagar(&mut self, a: usize, b: usize) {
        let (ca, cb) = (Pos::new(a, 0), Pos::new(b, 0));
        self.cursor = ca;
        self.apply_operator('d', ca, cb, Kind::Line, None);
        let r = self.cursor.row.min(self.last_row());
        self.cursor = Pos::new(r, self.buf.first_nonblank(r));
        self.fix_cursor();
        self.curswant = self.want_here();
    }

    fn ex_normal(&mut self, a: usize, b: usize, teclas: &str, com_faixa: bool) {
        let n0 = self.buf.line_count();
        let u0 = self.undo.len();
        let mut row = a;
        let mut fim = b;
        loop {
            if row > fim.min(self.last_row()) {
                break;
            }
            if com_faixa || row != a {
                self.cursor = Pos::new(row, 0);
                self.fix_cursor();
            }
            let antes = self.buf.line_count();
            let em_macro = std::mem::replace(&mut self.in_macro, true);
            self.feed_str(teclas);
            // um comando que ficou pela metade ou em inserção é encerrado (como o :normal do Vim)
            if self.mode != Mode::Normal || !self.pending.is_empty() {
                self.key_raw(Key::Esc);
                self.pending.clear();
            }
            self.in_macro = em_macro;
            let depois = self.buf.line_count() as i64 - antes as i64;
            fim = (fim as i64 + depois).max(0) as usize;
            row += 1;
        }
        let _ = n0;
        // o :normal inteiro é um passo só no desfazer
        if self.undo.len() > u0 + 1 {
            self.undo.truncate(u0 + 1);
        }
    }

    /// `:g/padrão/comando` e `:v/padrão/comando`.
    fn global(&mut self, a: usize, b: usize, resto: &str, inverso: bool) {
        let chars: Vec<char> = resto.chars().collect();
        let Some(&delim) = chars.first() else {
            self.failed = true;
            return;
        };
        let (pat, fim) = partir(&chars, 1, delim);
        let cmd: String = chars[fim.min(chars.len())..].iter().collect();
        let cmd = if cmd.trim().is_empty() { "p".to_string() } else { cmd };
        let pat = if pat.is_empty() { self.last_search.as_ref().map(|(p, _)| p.clone()).unwrap_or_default() } else { pat };
        let Some(re) = search::compile_opts(&pat, self.ignorecase, self.smartcase) else {
            self.failed = true;
            return;
        };
        self.last_search = Some((pat, self.last_search.as_ref().map_or(true, |(_, f)| *f)));
        let mut marcadas: Vec<usize> = (a..=b.min(self.last_row())).filter(|&r| re.is_match(&self.buf.line(r)) != inverso).collect();
        if marcadas.is_empty() {
            self.failed = true;
            return;
        }
        self.checkpoint();
        let u0 = self.undo.len();
        let mut k = 0;
        while k < marcadas.len() {
            let r = marcadas[k];
            if r > self.last_row() {
                break;
            }
            let antes = self.buf.line_count() as i64;
            self.cursor = Pos::new(r, 0);
            self.fix_cursor();
            let sem_registro_de_undo = self.undo.len();
            let c = cmd.clone();
            if c.trim() == "p" || c.trim() == "print" {
                // só posiciona o cursor
            } else {
                self.run_cmdline(&format!(":{}", c.trim_start()));
            }
            let _ = sem_registro_de_undo;
            let delta = self.buf.line_count() as i64 - antes;
            if delta != 0 {
                for m in marcadas.iter_mut().skip(k + 1) {
                    *m = (*m as i64 + delta).max(0) as usize;
                }
            }
            k += 1;
        }
        // o comando inteiro é um passo só no desfazer
        self.undo.truncate(u0.max(1));
        let _ = &mut marcadas;
    }

    /// `:[faixa]s/padrão/substituição/flags`.
    /// `:&`, `:&&`, `:~` e `:s` sem argumentos. `~` usa o último padrão de busca.
    fn repetir_substituicao(&mut self, a: usize, b: usize, flags: &str, padrao_de_busca: bool) {
        let Some((pat, rep, antigas)) = self.last_sub.clone() else {
            self.failed = true;
            return;
        };
        let pat = if padrao_de_busca { self.last_search.as_ref().map(|(p, _)| p.clone()).unwrap_or(pat) } else { pat };
        let flags = flags.trim_start();
        let flags = match flags.strip_prefix('&') {
            Some(r) => format!("{antigas}{r}"),
            None => flags.to_string(),
        };
        let d = '\u{1f}';
        self.substituir(a, b, &format!("{d}{pat}{d}{rep}{d}{flags}"));
    }

    fn substituir(&mut self, a: usize, b: usize, resto: &str) {
        let chars: Vec<char> = resto.chars().collect();
        let Some(&delim) = chars.first() else {
            self.failed = true;
            return;
        };
        if delim.is_alphanumeric() || delim == ' ' || delim == '\\' || delim == '"' || delim == '|' {
            self.failed = true;
            return;
        }
        let (pat, i1) = partir(&chars, 1, delim);
        let (rep, i2) = partir(&chars, i1, delim);
        let flags: String = chars[i2.min(chars.len())..].iter().collect();
        let (mut global, mut ic, mut cs, mut quieto) = (false, false, false, false);
        for f in flags.trim().chars() {
            match f {
                'g' => global = true,
                'i' => ic = true,
                'I' => cs = true,
                'e' => quieto = true,
                'c' | 'n' | 'p' | '#' | 'l' | '&' | 'r' | ' ' => {}
                _ => {
                    self.failed = true;
                    return;
                }
            }
        }
        let pat = if pat.is_empty() { self.last_search.as_ref().map(|(p, _)| p.clone()).unwrap_or_default() } else { pat };
        if pat.is_empty() {
            self.failed = true;
            return;
        }
        if delim != '\u{1f}' {
            self.last_sub = Some((pat.clone(), rep.clone(), flags.trim().to_string()));
        }
        let contar = flags.contains('n');
        let (padrao, ic_vim) = search::vim_to_regex(&pat);
        let prefixo = if (ic || ic_vim) && !cs { "(?i)" } else { "" };
        let Ok(re) = regex::Regex::new(&format!("{prefixo}{padrao}")) else {
            self.failed = true;
            return;
        };
        self.last_search = Some((pat, self.last_search.as_ref().map_or(true, |(_, f)| *f)));
        let a = a.min(self.last_row());
        let b = b.min(self.last_row());
        let mut row = a;
        let mut fim = b;
        let mut ultimo: Option<usize> = None;
        let mut mudou = false;
        while row <= fim {
            let linha = self.buf.line(row);
            let mut novo = String::new();
            let mut pos = 0usize;
            let mut achou = false;
            for caps in re.captures_iter(&linha) {
                let m = caps.get(0).unwrap();
                achou = true;
                novo.push_str(&linha[pos..m.start()]);
                novo.push_str(&expandir(&rep, &caps));
                pos = m.end();
                if !global {
                    break;
                }
            }
            if !achou {
                row += 1;
                continue;
            }
            if contar {
                ultimo = Some(row);
                row += 1;
                continue;
            }
            novo.push_str(&linha[pos..]);
            if !mudou {
                self.checkpoint();
                mudou = true;
            }
            let linhas: Vec<&str> = novo.split('\n').collect();
            let len = self.buf.line_len(row);
            self.buf.delete(Pos::new(row, 0), Pos::new(row, len));
            self.buf.insert(Pos::new(row, 0), linhas[0]);
            for (k, l) in linhas.iter().enumerate().skip(1) {
                self.buf.insert_line(row + k, l);
            }
            let extra = linhas.len() - 1;
            fim += extra;
            ultimo = Some(row + extra);
            row += extra + 1;
        }
        match ultimo {
            Some(_) if contar => {}
            Some(r) => {
                self.cursor = Pos::new(r, self.buf.first_nonblank(r));
                self.fix_cursor();
                self.curswant = self.want_here();
            }
            None => {
                if !quieto {
                    self.failed = true;
                }
            }
        }
    }
}

/// Parte `texto` no próximo delimitador sem barra; `\<delim>` vira o próprio delimitador.
fn partir(c: &[char], inicio: usize, delim: char) -> (String, usize) {
    let mut out = String::new();
    let mut i = inicio;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            if c[i + 1] == delim && delim != '\\' {
                out.push(delim);
            } else {
                out.push('\\');
                out.push(c[i + 1]);
            }
            i += 2;
            continue;
        }
        if c[i] == delim {
            return (out, i + 1);
        }
        out.push(c[i]);
        i += 1;
    }
    (out, i)
}

/// Texto da substituição: `&`, `\0`..`\9`, `\n`, `\r`, `\t`, `\u \U \l \L \e \E`.
fn expandir(rep: &str, caps: &regex::Captures) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Caixa {
        Nada,
        Alta,
        Baixa,
    }
    let (mut fixo, mut uma) = (Caixa::Nada, Caixa::Nada);
    let mut out = String::new();
    let mut poe = |s: &str, fixo: Caixa, uma: &mut Caixa, out: &mut String| {
        for ch in s.chars() {
            let m = if *uma != Caixa::Nada { *uma } else { fixo };
            *uma = Caixa::Nada;
            match m {
                Caixa::Alta => out.extend(ch.to_uppercase()),
                Caixa::Baixa => out.extend(ch.to_lowercase()),
                Caixa::Nada => out.push(ch),
            }
        }
    };
    let c: Vec<char> = rep.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '&' => {
                let t = caps.get(0).map_or("", |m| m.as_str());
                poe(t, fixo, &mut uma, &mut out);
            }
            '\\' if i + 1 < c.len() => {
                i += 1;
                match c[i] {
                    d @ '0'..='9' => {
                        let t = caps.get(d.to_digit(10).unwrap() as usize).map_or("", |m| m.as_str());
                        poe(t, fixo, &mut uma, &mut out);
                    }
                    'n' => out.push('\0'),
                    'r' => out.push('\n'),
                    't' => out.push('\t'),
                    'u' => uma = Caixa::Alta,
                    'l' => uma = Caixa::Baixa,
                    'U' => fixo = Caixa::Alta,
                    'L' => fixo = Caixa::Baixa,
                    'e' | 'E' => {
                        fixo = Caixa::Nada;
                        uma = Caixa::Nada;
                    }
                    '&' => poe("&", fixo, &mut uma, &mut out),
                    outro => poe(&outro.to_string(), fixo, &mut uma, &mut out),
                }
            }
            outro => poe(&outro.to_string(), fixo, &mut uma, &mut out),
        }
        i += 1;
    }
    out
}
