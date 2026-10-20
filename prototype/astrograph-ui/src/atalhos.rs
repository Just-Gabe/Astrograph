//! Atalhos do app nativo: a tabela que alimenta o menu da líder (aparece enquanto você digita
//! `<Espaço>...`) e a lista pesquisável `<Espaço>fk` / `:Atalhos`.

pub struct Atalho {
    /// `true`: as teclas vêm depois da líder
    pub lider: bool,
    pub teclas: &'static str,
    pub desc: &'static str,
}

const fn l(teclas: &'static str, desc: &'static str) -> Atalho {
    Atalho { lider: true, teclas, desc }
}
const fn g(teclas: &'static str, desc: &'static str) -> Atalho {
    Atalho { lider: false, teclas, desc }
}

/// Nome do grupo de cada prefixo da líder.
pub const GRUPOS: &[(&str, &str)] = &[
    ("f", "Buscar"),
    ("l", "Código (LSP)"),
    ("x", "Diagnósticos"),
    ("c", "Código"),
    ("r", "Renomear"),
    ("A", "Agentes de IA"),
];

pub const ATALHOS: &[Atalho] = &[
    l("ff", "Buscar arquivo"),
    l("fw", "Buscar texto no projeto"),
    l("fo", "Arquivos recentes"),
    l("fk", "Lista de atalhos (esta)"),
    l("e", "Explorador de arquivos"),
    l("t", "Terminal"),
    l("T", "Fechar o terminal"),
    l("rn", "Renomear o símbolo"),
    l("ca", "Ações de código"),
    l("lf", "Formatar o arquivo"),
    l("ls", "Símbolos do arquivo"),
    l("li", "Ir para a implementação"),
    l("ly", "Ir para o tipo"),
    l("lr", "Referências"),
    l("xx", "Lista de diagnósticos"),
    l("Al", "Abrir o agente no terminal"),
    l("Ag", "Escolher o agente"),
    l("Aw", "Modo do agente"),
    l("AS", "Conversas do agente"),
    l("Ax", "Agente: corrigir o erro da linha"),
    l("Ai", "Agente: explicar a seleção"),
    l("Ar", "Agente: revisar as alterações"),
    l("Am", "Agente: mensagem de commit"),
    l("Az", "Modo zen (agente em tela cheia)"),
    g("gd", "Ir para a definição"),
    g("K", "Documentação sob o cursor"),
    g("]d", "Próximo diagnóstico"),
    g("[d", "Diagnóstico anterior"),
    g(":w<CR>", "Salvar"),
    g(":q<CR>", "Fechar"),
    g(":wq<CR>", "Salvar e fechar"),
    g(":tabnew<CR>", "Nova aba"),
    g("gt", "Próxima aba"),
    g("gT", "Aba anterior"),
    g(":vsplit<CR>", "Dividir ao lado"),
    g(":split<CR>", "Dividir embaixo"),
    g("<C-w>w", "Próxima janela"),
    g("u", "Desfazer"),
    g("<C-r>", "Refazer"),
    g("/", "Buscar no arquivo"),
    g("*", "Buscar a palavra sob o cursor"),
];

/// O que a líder oferece depois de `prefixo` (ex.: "" ou "f"): a próxima tecla e o que ela faz,
/// com os grupos primeiro.
pub fn proximos(prefixo: &str) -> Vec<(String, String)> {
    let mut grupos: Vec<(String, String)> = Vec::new();
    let mut folhas: Vec<(String, String)> = Vec::new();
    for a in ATALHOS.iter().filter(|a| a.lider && a.teclas.starts_with(prefixo) && a.teclas.len() > prefixo.len()) {
        let resto = &a.teclas[prefixo.len()..];
        let tecla = resto.chars().next().map(String::from).unwrap_or_default();
        if resto.chars().count() > 1 {
            if !grupos.iter().any(|(t, _)| *t == tecla) {
                let nome = GRUPOS.iter().find(|(p, _)| *p == format!("{prefixo}{tecla}")).map_or("mais...", |(_, n)| n);
                grupos.push((tecla, format!("+{nome}")));
            }
        } else {
            folhas.push((tecla, a.desc.to_string()));
        }
    }
    grupos.extend(folhas);
    grupos
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn menu_da_lider_agrupa_e_desce_um_nivel() {
        let raiz = proximos("");
        assert!(raiz.iter().any(|(t, d)| t == "f" && d == "+Buscar"));
        assert!(raiz.iter().any(|(t, d)| t == "e" && d == "Explorador de arquivos"));
        let f = proximos("f");
        assert_eq!(f.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), vec!["f", "w", "o", "k"]);
        assert!(proximos("zz").is_empty());
    }

    #[test]
    fn nao_ha_teclas_repetidas() {
        let mut v: Vec<_> = ATALHOS.iter().map(|a| (a.lider, a.teclas)).collect();
        v.sort();
        let n = v.len();
        v.dedup();
        assert_eq!(n, v.len());
    }
}
