//! Processos filhos sem a janela de console que o Windows abriria (e fecharia) a cada um.

/// `Command::...sem_janela()`: no Windows liga `CREATE_NO_WINDOW`; nos outros sistemas não faz nada.
pub trait SemJanela {
    fn sem_janela(&mut self) -> &mut Self;
}

impl SemJanela for std::process::Command {
    fn sem_janela(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        self
    }
}

#[cfg(test)]
mod testes {
    /// Todo processo filho do app (fora dos testes) tem de pedir "sem janela": no Windows, sem isso cada
    /// `git`, servidor de linguagem ou comando abre e fecha uma janela de console na cara do usuário.
    #[test]
    fn todo_processo_filho_do_app_nasce_sem_janela() {
        let pasta = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut faltam = Vec::new();
        for e in std::fs::read_dir(pasta).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "rs") || p.file_name().is_some_and(|n| n == "proc.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&p).unwrap();
            // só o código de produção: o que vem antes do primeiro `#[cfg(test)]`
            let prod = texto.split("#[cfg(test)]").next().unwrap_or("");
            let linhas: Vec<&str> = prod.lines().collect();
            for (i, l) in linhas.iter().enumerate() {
                // só chamadas reais (não comentários nem textos)
                if l.trim_start().starts_with("//") || !l.contains("Command::new(") {
                    continue;
                }
                // CommandBuilder do pty não abre console: só o std/tokio Command
                if l.contains("CommandBuilder") {
                    continue;
                }
                let janela = linhas[i..(i + 10).min(linhas.len())].iter().any(|x| x.contains("sem_janela") || x.contains("creation_flags"));
                if !janela {
                    faltam.push(format!("{}:{}", p.file_name().unwrap().to_string_lossy(), i + 1));
                }
            }
        }
        assert!(faltam.is_empty(), "processos sem `.sem_janela()`: {faltam:?}");
    }
}
