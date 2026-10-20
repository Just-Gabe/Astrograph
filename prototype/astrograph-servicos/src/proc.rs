//! Executa comandos externos com tempo-limite, sem travar na saida cheia.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct Saida {
    /// codigo de saida (-1: nao iniciou, foi morto por sinal ou estourou o tempo)
    pub codigo: i32,
    pub stdout: String,
    pub stderr: String,
    pub estourou: bool,
}

impl Saida {
    pub fn ok(&self) -> bool {
        self.codigo == 0
    }
    /// stdout sem espacos nas pontas
    pub fn texto(&self) -> String {
        self.stdout.trim().to_string()
    }
}

/// Variaveis que tiram do git qualquer pergunta ou trava e fixam a lingua das
/// mensagens (o codigo as reconhece em ingles).
pub const AMBIENTE_GIT: [(&str, &str); 4] =
    [("GIT_TERMINAL_PROMPT", "0"), ("GIT_OPTIONAL_LOCKS", "0"), ("LC_ALL", "C"), ("LANG", "C")];

pub fn rodar(prog: &str, args: &[&str], dir: Option<&Path>, env: &[(&str, &str)], limite: Duration) -> Saida {
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut filho = match cmd.spawn() {
        Ok(f) => f,
        Err(e) => return Saida { codigo: -1, stderr: format!("nao consegui iniciar {prog}: {e}"), ..Default::default() },
    };
    let ler = |mut r: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let t_out = ler(Box::new(filho.stdout.take().unwrap()));
    let t_err = ler(Box::new(filho.stderr.take().unwrap()));
    let fim = Instant::now() + limite;
    let (codigo, estourou) = loop {
        match filho.try_wait() {
            Ok(Some(st)) => break (st.code().unwrap_or(-1), false),
            Ok(None) if Instant::now() >= fim => {
                let _ = filho.kill();
                let _ = filho.wait();
                break (-1, true);
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => break (-1, false),
        }
    };
    Saida { codigo, stdout: t_out.join().unwrap_or_default(), stderr: t_err.join().unwrap_or_default(), estourou }
}

/// `git -C <raiz> --no-optional-locks <args>`
pub fn git(raiz: &Path, args: &[&str], limite: Duration) -> Saida {
    let mut todos: Vec<&str> = vec!["--no-optional-locks"];
    todos.extend_from_slice(args);
    rodar("git", &todos, Some(raiz), &AMBIENTE_GIT, limite)
}

pub const LIMITE_GIT: Duration = Duration::from_secs(120);
