//! Projeção das dobras fechadas: o texto que a cena desenha sem as linhas escondidas. A primeira linha de
//! cada dobra fecha com "⋯ N linhas"; todas as coordenadas do modelo da tela (cursor, seleção, realce,
//! diagnósticos, sinais do git) passam de linhas do arquivo para linhas da projeção e a cena não precisa
//! saber de dobras (só dos números reais para a margem).

use ropey::Rope;

#[derive(Debug)]
pub struct Proj {
    pub rev: u64,
    pub dobras: Vec<(usize, usize)>,
    /// largura da quebra de linha (colunas visuais), se `wrap` está ligado
    pub larg: Option<usize>,
    /// quebrou nos espaços (`linebreak`)
    pub em_palavra: bool,
    /// o texto projetado
    pub rope: Rope,
    /// linha projetada -> linha do arquivo
    pub mapa: std::sync::Arc<Vec<usize>>,
    /// linha projetada: é a continuação de uma linha quebrada? (a margem não mostra número)
    pub cont: std::sync::Arc<Vec<bool>>,
    /// linha do arquivo -> primeira linha projetada (as escondidas caem na linha da dobra)
    pub inv: Vec<usize>,
    /// de qual caractere da linha do arquivo cada linha projetada começa e onde termina (exclusivo)
    pub ini: Vec<usize>,
    pub fim: Vec<usize>,
    /// onde termina o texto da linha de cada dobra e o tamanho do aviso (em caracteres), por linha projetada
    pub marcas: Vec<Option<(usize, usize)>>,
}

impl Proj {
    /// `larg`: quebra as linhas mais largas que isso (colunas visuais, com tab e caracteres largos).
    pub fn construir(linhas: &dyn Fn(usize) -> String, total: usize, dobras: &[(usize, usize)], rev: u64, larg: Option<usize>, quebra_em_palavra: bool) -> Proj {
        let mut texto = String::new();
        let mut mapa = Vec::with_capacity(total);
        let mut cont = Vec::with_capacity(total);
        let mut ini = Vec::with_capacity(total);
        let mut fim_v = Vec::with_capacity(total);
        let mut inv = vec![0usize; total];
        let mut marcas = Vec::new();
        let mut d = 0;
        let mut r = 0;
        let mut primeira = true;
        let mut poe = |texto: &mut String, s: &str| {
            if !primeira {
                texto.push('\n');
            }
            primeira = false;
            texto.push_str(s);
        };
        while r < total {
            let fold = loop {
                match dobras.get(d) {
                    Some(&(_, fim)) if fim < r => d += 1,
                    Some(&(ini, fim)) if ini == r => break Some((ini, fim)),
                    _ => break None,
                }
            };
            let p = mapa.len();
            let l = linhas(r);
            match fold {
                Some((i0, fim)) => {
                    let n = fim - i0 + 1;
                    let aviso = format!("  ⋯ {n} linhas");
                    let nc = l.chars().count();
                    marcas.push(Some((nc, aviso.chars().count())));
                    poe(&mut texto, &format!("{l}{aviso}"));
                    mapa.push(r);
                    cont.push(false);
                    ini.push(0);
                    fim_v.push(nc);
                    for k in i0..=fim.min(total - 1) {
                        inv[k] = p;
                    }
                    r = fim + 1;
                }
                None => {
                    inv[r] = p;
                    let cs: Vec<char> = l.chars().collect();
                    let cortes = match larg {
                        Some(w) if w > 0 => cortes_da_linha(&cs, w, quebra_em_palavra),
                        _ => vec![0, cs.len()],
                    };
                    for (k, par) in cortes.windows(2).enumerate() {
                        let seg: String = cs[par[0]..par[1]].iter().collect();
                        poe(&mut texto, &seg);
                        marcas.push(None);
                        mapa.push(r);
                        cont.push(k > 0);
                        ini.push(par[0]);
                        fim_v.push(par[1]);
                    }
                    r += 1;
                }
            }
        }
        Proj { rev, dobras: dobras.to_vec(), larg, em_palavra: quebra_em_palavra, rope: Rope::from_str(&texto), mapa: std::sync::Arc::new(mapa), cont: std::sync::Arc::new(cont), inv, ini, fim: fim_v, marcas }
    }

    pub fn a_proj(&self, real: usize) -> usize {
        self.inv[real.min(self.inv.len().saturating_sub(1))]
    }

    pub fn a_real(&self, proj: usize) -> usize {
        self.mapa[proj.min(self.mapa.len().saturating_sub(1))]
    }

    pub fn linhas(&self) -> usize {
        self.mapa.len()
    }

    /// (linha, coluna em caracteres) do arquivo -> (linha projetada, coluna dentro dela). Uma linha escondida
    /// por dobra cai no começo da dobra.
    pub fn pos_a_proj(&self, real: usize, col: usize) -> (usize, usize) {
        let mut p = self.a_proj(real);
        if self.mapa[p] != real {
            return (p, 0);
        }
        while p + 1 < self.mapa.len() && self.mapa[p + 1] == real && self.ini[p + 1] <= col {
            p += 1;
        }
        (p, col.saturating_sub(self.ini[p]))
    }

    /// (linha projetada, coluna dentro dela) -> (linha, coluna em caracteres) do arquivo.
    pub fn pos_a_real(&self, proj: usize, col: usize) -> (usize, usize) {
        let p = proj.min(self.mapa.len().saturating_sub(1));
        (self.mapa[p], self.ini[p] + col)
    }
}

/// Pontos onde uma linha de caracteres `cs` quebra para caber em `w` colunas: `[0, c1, c2, ..., len]`.
fn cortes_da_linha(cs: &[char], w: usize, em_palavra: bool) -> Vec<usize> {
    let mut cortes = vec![0];
    let mut v = 0usize;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let cw = astrograph_core::motion::char_width(c, v);
        if v + cw > w && i > *cortes.last().unwrap() {
            // `linebreak`: quebra depois do último espaço do trecho, se houver (a palavra inteira desce)
            let ini = *cortes.last().unwrap();
            let corte = if em_palavra && !cs[i].is_whitespace() {
                cs[ini..i].iter().rposition(|x| x.is_whitespace()).map(|k| ini + k + 1).filter(|&k| k > ini).unwrap_or(i)
            } else {
                i
            };
            cortes.push(corte);
            // recomeça a contar a partir do corte
            v = 0;
            i = corte;
            continue;
        }
        v += cw;
        i += 1;
    }
    cortes.push(cs.len());
    cortes
}

/// O método de dobra com que as sessões novas começam (`[editor] foldmethod` do config).
pub static METODO_INICIAL: std::sync::OnceLock<Metodo> = std::sync::OnceLock::new();

/// `[editor] linebreak` do config: a quebra de linha respeita as palavras.
pub static LINEBREAK_INICIAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// `[editor] wrap` do config: as sessões novas começam com a quebra de linha ligada.
pub static WRAP_INICIAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Como as dobras automáticas são calculadas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Metodo {
    #[default]
    Manual,
    Indent,
    Syntax,
}

impl Metodo {
    pub fn de(nome: &str) -> Option<Metodo> {
        match nome.trim().to_lowercase().as_str() {
            "manual" => Some(Metodo::Manual),
            "indent" => Some(Metodo::Indent),
            "syntax" | "expr" | "treesitter" => Some(Metodo::Syntax),
            _ => None,
        }
    }
}

/// `foldmethod=indent`: o nível de uma linha é o recuo dividido por `shiftwidth`; linhas seguidas do mesmo
/// nível ou mais fundo formam uma dobra por nível (a linha de cima, menos recuada, fica de fora). Linhas em
/// branco ficam no nível do menor dos vizinhos.
pub fn por_recuo(linhas: &[String], shiftwidth: usize, tabstop: usize) -> Vec<(usize, usize)> {
    let sw = shiftwidth.max(1);
    let ts = tabstop.max(1);
    let largura = |l: &str| l.chars().take_while(|c| *c == ' ' || *c == '\t').fold(0usize, |w, c| if c == '\t' { (w / ts + 1) * ts } else { w + 1 });
    let em_branco = |l: &String| l.trim().is_empty();
    let n = linhas.len();
    let mut nivel: Vec<Option<usize>> = linhas.iter().map(|l| if em_branco(l) { None } else { Some(largura(l) / sw) }).collect();
    // branco: o menor entre o último e o próximo nível conhecidos
    let mut anterior: Option<usize> = None;
    let mut proximo: Vec<Option<usize>> = vec![None; n];
    let mut seguinte: Option<usize> = None;
    for i in (0..n).rev() {
        proximo[i] = seguinte;
        if let Some(v) = nivel[i] {
            seguinte = Some(v);
        }
    }
    for i in 0..n {
        match nivel[i] {
            Some(v) => anterior = Some(v),
            None => {
                nivel[i] = Some(match (anterior, proximo[i]) {
                    (Some(a), Some(b)) => a.min(b),
                    (Some(_), None) => 0,
                    _ => 0,
                });
            }
        }
    }
    let nivel: Vec<usize> = nivel.into_iter().map(|v| v.unwrap_or(0)).collect();
    let mut out = Vec::new();
    let max = nivel.iter().copied().max().unwrap_or(0);
    for l in 1..=max {
        let mut ini: Option<usize> = None;
        for i in 0..=n {
            let dentro = i < n && nivel[i] >= l;
            match (ini, dentro) {
                (None, true) => ini = Some(i),
                (Some(a), false) => {
                    // linhas em branco no fim não entram na dobra
                    let mut fim = i - 1;
                    while fim > a && em_branco(&linhas[fim]) {
                        fim -= 1;
                    }
                    if fim > a {
                        out.push((a, fim));
                    }
                    ini = None;
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn quebra_de_linha_corta_na_largura_e_mapeia_posicoes() {
        let linhas = ["abcdefghij".to_string(), "".to_string(), "xyz".to_string(), "日本語日本語".to_string()];
        let l = |r: usize| linhas[r].clone();
        let p = Proj::construir(&l, 4, &[], 0, Some(4), false);
        // 10 letras em 4 colunas: 3 trechos; vazia: 1; curta: 1; cada ideograma ocupa 2 colunas: 2 por linha
        assert_eq!(p.rope.to_string(), "abcd\nefgh\nij\n\nxyz\n日本\n語日\n本語");
        assert_eq!(*p.mapa, vec![0, 0, 0, 1, 2, 3, 3, 3]);
        assert_eq!(*p.cont, vec![false, true, true, false, false, false, true, true]);
        assert_eq!(p.inv, vec![0, 3, 4, 5]);
        // (linha 0, coluna 6) -> terceiro trecho começa na coluna 8? não: o segundo (4..8), coluna 2 nele
        assert_eq!(p.pos_a_proj(0, 6), (1, 2));
        assert_eq!(p.pos_a_proj(0, 9), (2, 1));
        assert_eq!(p.pos_a_real(2, 1), (0, 9));
        assert_eq!(p.pos_a_proj(3, 3), (6, 1));
        // `linebreak`: a palavra inteira desce em vez de partir no meio
        let palavras = ["aaa bbb cccc dd".to_string()];
        let lp = |r: usize| palavras[r].clone();
        let p = Proj::construir(&lp, 1, &[], 0, Some(8), true);
        assert_eq!(p.rope.to_string(), "aaa bbb \ncccc dd");
        // palavra maior que a linha: parte mesmo
        let longa = ["abcdefghijkl mn".to_string()];
        let ll = |r: usize| longa[r].clone();
        let p = Proj::construir(&ll, 1, &[], 0, Some(5), true);
        assert_eq!(p.rope.to_string(), "abcde\nfghij\nkl mn");
        // com dobra: a linha da dobra não quebra e leva o aviso
        let p = Proj::construir(&l, 4, &[(0, 1)], 0, Some(4), false);
        assert_eq!(p.rope.to_string().lines().next(), Some("abcdefghij  ⋯ 2 linhas"));
    }

    #[test]
    fn dobras_por_recuo() {
        let t = |s: &str| s.lines().map(str::to_string).collect::<Vec<_>>();
        let l = t("fn a() {\n    if x {\n        y();\n        z();\n    }\n    w();\n}\n\nfn b() {\n    q();\n}");
        let f = por_recuo(&l, 4, 4);
        // nível 1: as linhas 1 a 5; nível 2: as linhas 2 e 3; e o corpo de b
        assert!(f.contains(&(1, 5)), "{f:?}");
        assert!(f.contains(&(2, 3)), "{f:?}");
        assert!(f.contains(&(9, 9)) == false, "uma linha só não é dobra");
        assert_eq!(f.len(), 2);
        // tabs contam pelo tabstop
        let l = t("a:\n\tb\n\tc\nd");
        assert_eq!(por_recuo(&l, 4, 4), vec![(1, 2)]);
        assert_eq!(Metodo::de("INDENT"), Some(Metodo::Indent));
        assert_eq!(Metodo::de("x"), None);
    }

    #[test]
    fn projeta_as_linhas_visiveis_e_o_aviso_da_dobra() {
        let l = |r: usize| format!("linha {r}");
        let p = Proj::construir(&l, 8, &[(1, 3), (5, 6)], 0, None, false);
        assert_eq!(*p.mapa, vec![0, 1, 4, 5, 7]);
        assert_eq!(p.pos_a_proj(2, 5), (1, 0), "linha escondida cai no começo da dobra");
        assert_eq!(p.pos_a_real(2, 3), (4, 3));
        assert_eq!(p.inv, vec![0, 1, 1, 1, 2, 3, 3, 4]);
        assert_eq!(p.rope.to_string(), "linha 0\nlinha 1  ⋯ 3 linhas\nlinha 4\nlinha 5  ⋯ 2 linhas\nlinha 7");
        assert_eq!(p.marcas[1], Some((7, 12)));
        assert_eq!((p.a_proj(2), p.a_real(2), p.linhas()), (1, 4, 5));
    }
}
