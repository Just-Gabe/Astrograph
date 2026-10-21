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

#[cfg(test)]
mod testes {
    use super::*;

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
