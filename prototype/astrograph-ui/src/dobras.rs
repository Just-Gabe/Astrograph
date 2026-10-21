//! Projeção das dobras fechadas: o texto que a cena desenha sem as linhas escondidas. A primeira linha de
//! cada dobra fecha com "⋯ N linhas"; todas as coordenadas do modelo da tela (cursor, seleção, realce,
//! diagnósticos, sinais do git) passam de linhas do arquivo para linhas da projeção e a cena não precisa
//! saber de dobras (só dos números reais para a margem).

use ropey::Rope;

#[derive(Debug)]
pub struct Proj {
    pub rev: u64,
    pub dobras: Vec<(usize, usize)>,
    /// o texto projetado
    pub rope: Rope,
    /// linha projetada -> linha do arquivo
    pub mapa: std::sync::Arc<Vec<usize>>,
    /// linha do arquivo -> linha projetada (as escondidas caem na linha da dobra)
    pub inv: Vec<usize>,
    /// onde termina o texto da linha de cada dobra e o tamanho do aviso (em caracteres), por linha projetada
    pub marcas: Vec<Option<(usize, usize)>>,
}

impl Proj {
    pub fn construir(linhas: &dyn Fn(usize) -> String, total: usize, dobras: &[(usize, usize)], rev: u64) -> Proj {
        let mut texto = String::new();
        let mut mapa = Vec::with_capacity(total);
        let mut inv = vec![0usize; total];
        let mut marcas = Vec::new();
        let mut d = 0;
        let mut r = 0;
        while r < total {
            let fold = loop {
                match dobras.get(d) {
                    Some(&(_, fim)) if fim < r => d += 1,
                    Some(&(ini, fim)) if ini == r => break Some((ini, fim)),
                    _ => break None,
                }
            };
            let p = mapa.len();
            if !texto.is_empty() || p > 0 {
                texto.push('\n');
            }
            let l = linhas(r);
            match fold {
                Some((ini, fim)) => {
                    let n = fim - ini + 1;
                    let aviso = format!("  ⋯ {n} linhas");
                    marcas.push(Some((l.chars().count(), aviso.chars().count())));
                    texto.push_str(&l);
                    texto.push_str(&aviso);
                    mapa.push(r);
                    for k in ini..=fim.min(total - 1) {
                        inv[k] = p;
                    }
                    r = fim + 1;
                }
                None => {
                    marcas.push(None);
                    texto.push_str(&l);
                    mapa.push(r);
                    inv[r] = p;
                    r += 1;
                }
            }
        }
        Proj { rev, dobras: dobras.to_vec(), rope: Rope::from_str(&texto), mapa: std::sync::Arc::new(mapa), inv, marcas }
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
}

/// O método de dobra com que as sessões novas começam (`[editor] foldmethod` do config).
pub static METODO_INICIAL: std::sync::OnceLock<Metodo> = std::sync::OnceLock::new();

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
        let p = Proj::construir(&l, 8, &[(1, 3), (5, 6)], 0);
        assert_eq!(*p.mapa, vec![0, 1, 4, 5, 7]);
        assert_eq!(p.inv, vec![0, 1, 1, 1, 2, 3, 3, 4]);
        assert_eq!(p.rope.to_string(), "linha 0\nlinha 1  ⋯ 3 linhas\nlinha 4\nlinha 5  ⋯ 2 linhas\nlinha 7");
        assert_eq!(p.marcas[1], Some((7, 12)));
        assert_eq!((p.a_proj(2), p.a_real(2), p.linhas()), (1, 4, 5));
    }
}
