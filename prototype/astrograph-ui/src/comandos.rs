//! `:Astrograph ...` no motor nativo: atualizar (pelo `astrograph-servicos`), saúde e atalhos.

use std::path::{Path, PathBuf};

/// A pasta do clone do Astrograph (onde ficam `bin/` e `.git`): `ASTROGRAPH_ROOT`, ou a pasta acima da
/// `bin/` onde este executável está.
pub fn raiz_do_clone() -> Option<PathBuf> {
    if let Some(r) = std::env::var_os("ASTROGRAPH_ROOT").map(PathBuf::from).filter(|r| r.join(".git").exists()) {
        return Some(r);
    }
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?;
    let raiz = bin.parent()?;
    (bin.file_name()? == "bin" && raiz.join(".git").exists()).then(|| raiz.to_path_buf())
}

pub fn servicos_bin(raiz: &Path) -> Option<PathBuf> {
    let p = raiz.join("bin").join(if cfg!(windows) { "astrograph-servicos.exe" } else { "astrograph-servicos" });
    p.is_file().then_some(p)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Resultado {
    pub status: String,
    pub mensagem: String,
}

/// A última linha do stdout do serviço é o resultado em JSON.
pub fn ler_resultado(stdout: &str) -> Result<Resultado, String> {
    let linha = stdout.lines().rev().find(|l| !l.trim().is_empty()).ok_or("O serviço de atualização terminou sem resultado.")?;
    let v: serde_json::Value = serde_json::from_str(linha.trim()).map_err(|e| format!("Resposta inesperada do serviço de atualização: {e}"))?;
    let status = v["status"].as_str().ok_or("O serviço não disse o resultado.")?.to_string();
    Ok(Resultado { status, mensagem: v["mensagem"].as_str().unwrap_or("").to_string() })
}

/// Roda a atualização transacional. Sem o Neovim instalado, pula o que depende dele (restaurar plugins e conferir a config).
pub fn atualizar(raiz: &Path) -> Result<Resultado, String> {
    let bin = servicos_bin(raiz).ok_or_else(|| format!("Não achei {}/bin/astrograph-servicos: reinstale o Astrograph.", raiz.display()))?;
    let mut args = vec!["atualizar".to_string(), "--raiz".into(), raiz.display().to_string()];
    if !crate::term::em_path("nvim") {
        args.push("--sem-plugins".into());
        args.push("--sem-verificar".into());
    }
    let saida = crate::agentes::rodar_direto(&bin.to_string_lossy(), &args, raiz, std::time::Duration::from_secs(3600))?;
    ler_resultado(&saida)
}

/// Texto do `:Astrograph saude`.
pub fn saude(motor: &str, lsp_servidores: &[(&str, bool)]) -> String {
    let mut t = String::new();
    t.push_str(&format!("Astrograph {} ({motor})\n\n", env!("CARGO_PKG_VERSION")));
    t.push_str(&format!("Clone:        {}\n", raiz_do_clone().map_or("não encontrado (instalação fora do padrão)".into(), |r| r.display().to_string())));
    t.push_str(&format!("Atualizador:  {}\n", raiz_do_clone().and_then(|r| servicos_bin(&r)).map_or("ausente".into(), |p| p.display().to_string())));
    t.push_str(&format!("Config:       {}\n", crate::config::caminho_config().map_or("(padrão)".into(), |p| p.display().to_string())));
    t.push_str(&format!("Estado:       {}\n", crate::busca::pasta_estado().map_or("(sem)".into(), |p| p.display().to_string())));
    t.push_str("\nFerramentas\n");
    for (nome, ok) in [("git", crate::term::em_path("git")), ("nvim (opcional, espelho no terminal)", crate::term::em_path("nvim")), ("npm (instala os agentes)", crate::term::em_path("npm"))] {
        t.push_str(&format!("  {} {nome}\n", if ok { "✔" } else { "✘" }));
    }
    t.push_str("\nAgentes de IA\n");
    for a in crate::agentes::AGENTES {
        t.push_str(&format!("  {} {}\n", if crate::agentes::instalado(a.bin) { "✔" } else { "✘" }, a.label));
    }
    t.push_str("\nServidores de linguagem (para o arquivo aberto e os mais comuns)\n");
    for (n, ok) in lsp_servidores {
        t.push_str(&format!("  {} {n}\n", if *ok { "✔" } else { "✘" }));
    }
    t
}

/// Texto do `:Astrograph atalhos`: a tabela da líder.
pub fn atalhos(lider: &str) -> String {
    let mut t = format!("Atalhos da tecla líder ({lider})\n\n");
    for (seq, desc) in crate::atalhos::LIDER {
        t.push_str(&format!("  {lider}{seq:<4} {desc}\n"));
    }
    t
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn le_a_ultima_linha_json() {
        let r = ler_resultado("progresso...\n{\"status\":\"atualizado\",\"mensagem\":\"ok\\nreinicie\"}\n\n").unwrap();
        assert_eq!(r, Resultado { status: "atualizado".into(), mensagem: "ok\nreinicie".into() });
        assert!(ler_resultado("").is_err());
        assert!(ler_resultado("lixo\n").is_err());
        assert!(ler_resultado("{\"x\":1}").is_err());
    }

    #[test]
    fn saude_e_atalhos_tem_o_essencial() {
        let s = saude("motor nativo", &[("rust-analyzer", true), ("gopls", false)]);
        assert!(s.contains("motor nativo") && s.contains("✔ rust-analyzer") && s.contains("✘ gopls") && s.contains("Claude Code"));
        let a = atalhos("<Espaço>");
        assert!(a.contains("<Espaço>ff") && a.contains("Buscar arquivo"));
    }

    #[cfg(unix)]
    #[test]
    fn atualizar_com_servico_falso() {
        let d = std::env::temp_dir().join(format!("astrograph-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bin")).unwrap();
        let bin = d.join("bin/astrograph-servicos");
        std::fs::write(&bin, "#!/bin/sh\necho \"args: $*\" >&2\necho '{\"status\":\"ja-atualizado\",\"mensagem\":\"Já está na versão mais nova.\"}'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = atualizar(&d).unwrap();
        assert_eq!(r.status, "ja-atualizado");
        assert!(servicos_bin(&d).is_some());
        assert!(atualizar(&d.join("nao-existe")).is_err());
    }
}
