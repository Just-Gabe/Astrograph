//! Tabela dos atalhos da tecla líder (o que o menu de atalhos mostra ao esperar depois dela).

/// (teclas depois da líder, o que faz).
pub const LIDER: &[(&str, &str)] = &[
    ("ff", "Buscar arquivo"),
    ("fw", "Buscar texto"),
    ("fo", "Arquivos recentes"),
    ("fp", "Abrir pasta"),
    ("fc", "Configurações"),
    ("e", "Explorador de arquivos"),
    ("t", "Terminal"),
    ("T", "Fechar o terminal"),
    ("N", "Novo terminal"),
    ("]", "Próximo terminal"),
    ("[", "Terminal anterior"),
    ("Al", "Abrir o agente de IA"),
    ("Ag", "Escolher o agente"),
    ("Aw", "Modo do agente"),
    ("AS", "Conversas do agente"),
    ("Ax", "Corrigir o erro da linha"),
    ("Ai", "Explicar o código"),
    ("Ar", "Revisar as alterações"),
    ("Am", "Mensagem de commit"),
    ("Az", "Modo zen"),
    ("AE", "Editar (seleção) ou perguntar à IA"),
    ("Ac", "Citar o arquivo no agente"),
    ("rn", "Renomear símbolo"),
    ("ca", "Ações de código"),
    ("lf", "Formatar o arquivo"),
    ("ls", "Símbolos do arquivo"),
    ("lS", "Símbolos do projeto"),
    ("li", "Ir para a implementação"),
    ("ly", "Ir para o tipo"),
    ("lr", "Referências"),
    ("xx", "Lista de diagnósticos"),
];

fn grupo(c: char) -> &'static str {
    match c {
        'f' => "Buscar",
        'A' => "Agentes de IA",
        'l' => "Linguagem (LSP)",
        'r' => "Renomear",
        'c' => "Código",
        'x' => "Diagnósticos",
        _ => "Mais",
    }
}

/// O que vem depois de `prefixo` (as teclas já digitadas depois da líder): cada próxima tecla com a
/// descrição, ou `+grupo` quando ela abre outro nível. Em ordem de aparição na tabela.
pub fn proximos(prefixo: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (seq, desc) in LIDER {
        let Some(resto) = seq.strip_prefix(prefixo) else { continue };
        let mut cs = resto.chars();
        let Some(c) = cs.next() else { continue };
        let tecla = c.to_string();
        if out.iter().any(|(t, _)| *t == tecla) {
            continue;
        }
        if cs.next().is_none() {
            out.push((tecla, (*desc).to_string()));
        } else {
            out.push((tecla, format!("+{}", grupo(c))));
        }
    }
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn proximos_niveis_e_grupos() {
        let raiz = proximos("");
        assert!(raiz.contains(&("e".into(), "Explorador de arquivos".into())));
        assert!(raiz.contains(&("f".into(), "+Buscar".into())));
        assert!(raiz.contains(&("A".into(), "+Agentes de IA".into())));
        let f = proximos("f");
        assert_eq!(f.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), vec!["f", "w", "o", "p", "c"]);
        assert_eq!(f[0].1, "Buscar arquivo");
        assert!(proximos("zz").is_empty());
    }

    #[test]
    fn toda_sequencia_da_tabela_e_unica() {
        let mut v: Vec<&str> = LIDER.iter().map(|(s, _)| *s).collect();
        v.sort();
        let n = v.len();
        v.dedup();
        assert_eq!(n, v.len());
        // nenhuma sequência é prefixo de outra (senão a mais curta dispararia antes)
        for a in LIDER {
            for b in LIDER {
                assert!(a.0 == b.0 || !b.0.starts_with(a.0), "{} é prefixo de {}", a.0, b.0);
            }
        }
    }
}
