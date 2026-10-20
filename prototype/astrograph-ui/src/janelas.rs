//! Janelas divididas: a árvore de layout de uma aba (puro, sem estado de editor nem de tela).
//!
//! Folhas são painéis (`PaneId`). Um nó `Split` põe os filhos lado a lado (`Dir::Row`, o
//! `:vsplit`) ou empilhados (`Dir::Col`, o `:split`), cada um com um peso; o tamanho vem do
//! peso dividido pela soma. O mesmo cálculo roda no editor (para dimensionar as sessões)
//! e na tela (para pintar), por isso fica aqui.

pub type PaneId = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// filhos lado a lado (divisão vertical, `:vsplit`)
    Row,
    /// filhos um sobre o outro (divisão horizontal, `:split`)
    Col,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lado {
    Esquerda,
    Direita,
    Cima,
    Baixo,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Leaf(PaneId),
    Split { dir: Dir, kids: Vec<(Layout, f32)> },
}

/// Barra entre dois filhos de um `Split`: arrastá-la move o peso de um para o outro.
#[derive(Clone, Debug, PartialEq)]
pub struct Divisor {
    /// ordem de aparição (a mesma em `rects` e em `arrastar`)
    pub idx: usize,
    pub rect: Rect,
    pub dir: Dir,
    /// início e comprimento (no eixo de `dir`) do espaço dos dois filhos que ela separa
    pub inicio: f32,
    pub tam: f32,
}

/// Peso mínimo de um painel espremido (para `<C-w>_` e arrastar)
const PESO_MIN: f32 = 0.05;

impl Layout {
    pub fn leaf(id: PaneId) -> Self {
        Layout::Leaf(id)
    }

    /// Painéis em ordem de leitura (esquerda→direita, cima→baixo): a ordem do `<C-w>w`.
    pub fn ids(&self) -> Vec<PaneId> {
        match self {
            Layout::Leaf(i) => vec![*i],
            Layout::Split { kids, .. } => kids.iter().flat_map(|(k, _)| k.ids()).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.ids().len()
    }

    pub fn contem(&self, id: PaneId) -> bool {
        self.ids().contains(&id)
    }

    /// Divide o painel `alvo`: `novo` entra antes (`depois = false`: acima/à esquerda, o padrão do
    /// Vim) ou depois dele. O espaço do alvo é repartido meio a meio. `false` se `alvo` não existe.
    pub fn dividir(&mut self, alvo: PaneId, dir: Dir, novo: PaneId, depois: bool) -> bool {
        match self {
            Layout::Leaf(i) if *i == alvo => {
                let (a, b) = if depois { (alvo, novo) } else { (novo, alvo) };
                *self = Layout::Split { dir, kids: vec![(Layout::Leaf(a), 1.0), (Layout::Leaf(b), 1.0)] };
                true
            }
            Layout::Leaf(_) => false,
            Layout::Split { dir: d, kids } => {
                if let Some(p) = kids.iter().position(|(k, _)| *k == Layout::Leaf(alvo)) {
                    if *d == dir {
                        // mesma direção: entra ao lado, tirando metade do espaço do alvo
                        let peso = kids[p].1 / 2.0;
                        kids[p].1 = peso;
                        kids.insert(if depois { p + 1 } else { p }, (Layout::Leaf(novo), peso));
                        return true;
                    }
                }
                kids.iter_mut().any(|(k, _)| k.dividir(alvo, dir, novo, depois))
            }
        }
    }

    /// Tira o painel `id`: o espaço vai para o vizinho. Um `Split` que fica com um filho some.
    /// `false` se `id` não existe ou é o único painel.
    pub fn fechar(&mut self, id: PaneId) -> bool {
        if self.len() < 2 || !self.contem(id) {
            return false;
        }
        self.tirar(id);
        true
    }

    fn tirar(&mut self, id: PaneId) {
        let Layout::Split { kids, .. } = self else { return };
        if let Some(p) = kids.iter().position(|(k, _)| *k == Layout::Leaf(id)) {
            let (_, peso) = kids.remove(p);
            // o vizinho (o anterior; ou o seguinte, se era o primeiro) fica com o espaço
            let v = p.saturating_sub(1).min(kids.len() - 1);
            kids[v].1 += peso;
        } else {
            for (k, _) in kids.iter_mut() {
                k.tirar(id);
            }
        }
        // colapsa: um filho só vira o próprio nó (e junta Splits da mesma direção)
        if kids.len() == 1 {
            let (unico, _) = kids.remove(0);
            *self = unico;
            return;
        }
        let dir = match self {
            Layout::Split { dir, .. } => *dir,
            _ => return,
        };
        if let Layout::Split { kids, .. } = self {
            let mut novos = Vec::new();
            for (k, p) in kids.drain(..) {
                match k {
                    Layout::Split { dir: d2, kids: k2 } if d2 == dir => {
                        let soma: f32 = k2.iter().map(|(_, q)| q).sum();
                        novos.extend(k2.into_iter().map(|(kk, q)| (kk, p * q / soma)));
                    }
                    outro => novos.push((outro, p)),
                }
            }
            *kids = novos;
        }
    }

    /// Deixa só `id` (`<C-w>o`).
    pub fn so(&mut self, id: PaneId) {
        if self.contem(id) {
            *self = Layout::Leaf(id);
        }
    }

    /// Tudo do mesmo tamanho (`<C-w>=`).
    pub fn igualar(&mut self) {
        if let Layout::Split { kids, .. } = self {
            for (k, p) in kids.iter_mut() {
                *p = 1.0;
                k.igualar();
            }
        }
    }

    /// `<C-w>_` (altura) e `<C-w>|` (largura): `id` fica com quase todo o espaço no eixo `dir`.
    pub fn maximizar(&mut self, id: PaneId, dir: Dir) {
        fn rec(l: &mut Layout, id: PaneId, dir: Dir) -> bool {
            let Layout::Split { dir: d, kids } = l else { return l == &Layout::Leaf(id) };
            let Some(p) = kids.iter_mut().position(|(k, _)| rec(k, id, dir)) else { return false };
            if *d == dir {
                for (i, (_, peso)) in kids.iter_mut().enumerate() {
                    *peso = if i == p { 1.0 } else { PESO_MIN };
                }
            }
            true
        }
        rec(self, id, dir);
    }

    /// Retângulos dos painéis dentro de `area`, com `gap` pixels entre vizinhos.
    pub fn rects(&self, area: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.rec_rects(area, gap, &mut out);
        out
    }

    fn rec_rects(&self, a: Rect, gap: f32, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Layout::Leaf(i) => out.push((*i, a)),
            Layout::Split { dir, kids } => {
                for (k, r) in Self::fatias(*dir, kids, a, gap) {
                    k.rec_rects(r, gap, out);
                }
            }
        }
    }

    /// Divide `a` entre os filhos conforme os pesos.
    fn fatias<'a>(dir: Dir, kids: &'a [(Layout, f32)], a: Rect, gap: f32) -> Vec<(&'a Layout, Rect)> {
        let n = kids.len() as f32;
        let total = match dir {
            Dir::Row => a.w,
            Dir::Col => a.h,
        };
        let livre = (total - gap * (n - 1.0)).max(0.0);
        let soma: f32 = kids.iter().map(|(_, p)| p.max(0.0)).sum::<f32>().max(1e-6);
        let mut pos = match dir {
            Dir::Row => a.x,
            Dir::Col => a.y,
        };
        kids.iter()
            .map(|(k, p)| {
                let t = livre * p.max(0.0) / soma;
                let r = match dir {
                    Dir::Row => Rect::new(pos, a.y, t, a.h),
                    Dir::Col => Rect::new(a.x, pos, a.w, t),
                };
                pos += t + gap;
                (k, r)
            })
            .collect()
    }

    /// As barras entre painéis (para arrastar com o mouse).
    pub fn divisores(&self, area: Rect, gap: f32) -> Vec<Divisor> {
        let mut out = Vec::new();
        self.rec_div(area, gap, &mut out);
        out
    }

    fn rec_div(&self, a: Rect, gap: f32, out: &mut Vec<Divisor>) {
        let Layout::Split { dir, kids } = self else { return };
        let fatias = Self::fatias(*dir, kids, a, gap);
        let (inicio, tam) = match dir {
            Dir::Row => (a.x, a.w),
            Dir::Col => (a.y, a.h),
        };
        for w in fatias.windows(2) {
            let (r1, r2) = (w[0].1, w[1].1);
            let rect = match dir {
                Dir::Row => Rect::new(r1.x + r1.w, a.y, r2.x - r1.x - r1.w, a.h),
                Dir::Col => Rect::new(a.x, r1.y + r1.h, a.w, r2.y - r1.y - r1.h),
            };
            let idx = out.len();
            out.push(Divisor { idx, rect, dir: *dir, inicio, tam });
        }
        for (k, r) in fatias {
            k.rec_div(r, gap, out);
        }
    }

    /// Arrasta o divisor `idx` (ordem de `divisores`) para a posição `pos` (pixels, no eixo dele).
    pub fn arrastar(&mut self, idx: usize, area: Rect, gap: f32, pos: f32) {
        let mut cont = 0;
        self.rec_arrastar(idx, area, gap, pos, &mut cont);
    }

    fn rec_arrastar(&mut self, idx: usize, a: Rect, gap: f32, pos: f32, cont: &mut usize) -> bool {
        let Layout::Split { dir, kids } = self else { return false };
        let dir = *dir;
        let rects: Vec<Rect> = Self::fatias(dir, kids, a, gap).into_iter().map(|(_, r)| r).collect();
        let n = kids.len();
        if idx < *cont + (n - 1) {
            let j = idx - *cont; // separa kids[j] e kids[j+1]
            let (ini, fim) = match dir {
                Dir::Row => (rects[j].x, rects[j + 1].x + rects[j + 1].w),
                Dir::Col => (rects[j].y, rects[j + 1].y + rects[j + 1].h),
            };
            let tam_filhos = fim - ini - gap;
            let soma_par = kids[j].1 + kids[j + 1].1;
            // fração do par que o primeiro filho ocupa, limitada para não sumir com nenhum
            let f = ((pos - gap / 2.0 - ini) / tam_filhos.max(1.0)).clamp(0.08, 0.92);
            kids[j].1 = soma_par * f;
            kids[j + 1].1 = soma_par * (1.0 - f);
            return true;
        }
        *cont += n - 1;
        let fatias = rects;
        for (k, r) in kids.iter_mut().zip(fatias) {
            if k.0.rec_arrastar(idx, r, gap, pos, cont) {
                return true;
            }
        }
        false
    }
}

/// Vizinho de `id` no `lado`: entre os painéis que encostam na borda daquele lado e se
/// sobrepõem no outro eixo, o que mais se sobrepõe (empate: o mais perto do centro de `id`).
pub fn vizinho(rects: &[(PaneId, Rect)], id: PaneId, lado: Lado) -> Option<PaneId> {
    let r = rects.iter().find(|(i, _)| *i == id)?.1;
    let sobrepoe = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.0);
    let mut melhor: Option<(f32, f32, PaneId)> = None;
    for (i, o) in rects {
        if *i == id {
            continue;
        }
        let (alinhado, ov, dist) = match lado {
            Lado::Esquerda => (o.x + o.w <= r.x + 1.0, sobrepoe(r.y, r.y + r.h, o.y, o.y + o.h), r.x - (o.x + o.w)),
            Lado::Direita => (o.x >= r.x + r.w - 1.0, sobrepoe(r.y, r.y + r.h, o.y, o.y + o.h), o.x - (r.x + r.w)),
            Lado::Cima => (o.y + o.h <= r.y + 1.0, sobrepoe(r.x, r.x + r.w, o.x, o.x + o.w), r.y - (o.y + o.h)),
            Lado::Baixo => (o.y >= r.y + r.h - 1.0, sobrepoe(r.x, r.x + r.w, o.x, o.x + o.w), o.y - (r.y + r.h)),
        };
        if !alinhado || ov <= 0.0 {
            continue;
        }
        // mais perto primeiro, depois a maior sobreposição
        let chave = (dist, -ov);
        if melhor.is_none_or(|(d, o2, _)| chave < (d, o2)) {
            melhor = Some((chave.0, chave.1, *i));
        }
    }
    melhor.map(|(_, _, i)| i)
}

#[cfg(test)]
mod testes {
    use super::*;

    const A: Rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };

    fn rect_de(l: &Layout, id: PaneId, gap: f32) -> Rect {
        l.rects(A, gap).into_iter().find(|(i, _)| *i == id).unwrap().1
    }

    #[test]
    fn um_painel_ocupa_a_area_toda() {
        let l = Layout::leaf(1);
        assert_eq!(l.rects(A, 2.0), vec![(1, A)]);
    }

    #[test]
    fn vsplit_poe_o_novo_a_esquerda_e_divide_meio_a_meio() {
        let mut l = Layout::leaf(1);
        assert!(l.dividir(1, Dir::Row, 2, false));
        assert_eq!(l.ids(), vec![2, 1]);
        let (r2, r1) = (rect_de(&l, 2, 0.0), rect_de(&l, 1, 0.0));
        assert_eq!((r2.x, r2.w, r1.x, r1.w), (0.0, 50.0, 50.0, 50.0));
        assert_eq!((r2.h, r1.h), (60.0, 60.0));
    }

    #[test]
    fn split_empilha_e_o_gap_sai_do_espaco() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Col, 2, true);
        let (r1, r2) = (rect_de(&l, 1, 4.0), rect_de(&l, 2, 4.0));
        assert_eq!((r1.y, r1.h), (0.0, 28.0));
        assert_eq!((r2.y, r2.h), (32.0, 28.0));
    }

    #[test]
    fn dividir_na_mesma_direcao_reparte_so_o_espaco_do_alvo() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Row, 2, true);
        l.dividir(2, Dir::Row, 3, true);
        assert_eq!(l.ids(), vec![1, 2, 3]);
        let w: Vec<f32> = [1, 2, 3].iter().map(|i| rect_de(&l, *i, 0.0).w).collect();
        assert_eq!(w, vec![50.0, 25.0, 25.0], "o 1 não encolhe");
    }

    #[test]
    fn dividir_em_outra_direcao_aninha() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Row, 2, true);
        l.dividir(2, Dir::Col, 3, true);
        assert_eq!(l.ids(), vec![1, 2, 3]);
        let (r2, r3) = (rect_de(&l, 2, 0.0), rect_de(&l, 3, 0.0));
        assert_eq!((r2.x, r2.w, r2.h), (50.0, 50.0, 30.0));
        assert_eq!((r3.x, r3.y, r3.h), (50.0, 30.0, 30.0));
        assert!(!l.dividir(99, Dir::Row, 4, true), "alvo inexistente");
    }

    #[test]
    fn fechar_devolve_o_espaco_ao_vizinho_e_colapsa() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Row, 2, true);
        l.dividir(2, Dir::Col, 3, true);
        assert!(l.fechar(3));
        assert_eq!(l.ids(), vec![1, 2]);
        assert_eq!(rect_de(&l, 2, 0.0).h, 60.0, "o 2 ocupa de novo a altura toda");
        assert!(l.fechar(1));
        assert_eq!(l, Layout::Leaf(2));
        assert!(!l.fechar(2), "o último painel não fecha");
        assert!(!l.fechar(9));
    }

    #[test]
    fn fechar_junta_splits_da_mesma_direcao() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Row, 2, true);
        l.dividir(2, Dir::Col, 3, true);
        l.dividir(3, Dir::Row, 4, true);
        // fechar o 2 deixa Col(3,4)... que vira filho direto do Row com o 1
        assert!(l.fechar(2));
        assert_eq!(l.ids(), vec![1, 3, 4]);
        let soma: f32 = [1, 3, 4].iter().map(|i| rect_de(&l, *i, 0.0).w).sum();
        assert!((soma - 100.0).abs() < 0.01);
    }

    #[test]
    fn so_igualar_e_maximizar() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Col, 2, true);
        l.dividir(2, Dir::Col, 3, true);
        l.maximizar(2, Dir::Col);
        let (h1, h2, h3) = (rect_de(&l, 1, 0.0).h, rect_de(&l, 2, 0.0).h, rect_de(&l, 3, 0.0).h);
        assert!(h2 > 45.0 && h1 < 5.0 && h3 < 5.0, "{h1} {h2} {h3}");
        l.igualar();
        let h: Vec<f32> = [1, 2, 3].iter().map(|i| rect_de(&l, *i, 0.0).h).collect();
        assert!(h.iter().all(|x| (x - 20.0).abs() < 0.01), "{h:?}");
        // maximizar na outra direção não mexe nas alturas
        l.maximizar(2, Dir::Row);
        assert!((rect_de(&l, 1, 0.0).h - 20.0).abs() < 0.01);
        l.so(2);
        assert_eq!(l, Layout::Leaf(2));
    }

    #[test]
    fn vizinho_acha_o_painel_certo_nas_quatro_direcoes() {
        //  1 | 2
        //  1 | 3
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Row, 2, true);
        l.dividir(2, Dir::Col, 3, true);
        let r = l.rects(A, 2.0);
        assert_eq!(vizinho(&r, 1, Lado::Direita), Some(2), "empate de sobreposição: o primeiro (mais acima)");
        assert_eq!(vizinho(&r, 2, Lado::Esquerda), Some(1));
        assert_eq!(vizinho(&r, 3, Lado::Esquerda), Some(1));
        assert_eq!(vizinho(&r, 2, Lado::Baixo), Some(3));
        assert_eq!(vizinho(&r, 3, Lado::Cima), Some(2));
        assert_eq!(vizinho(&r, 1, Lado::Esquerda), None);
        assert_eq!(vizinho(&r, 3, Lado::Baixo), None);
        assert_eq!(vizinho(&r, 1, Lado::Cima), None);
    }

    #[test]
    fn arrastar_o_divisor_move_o_peso_e_respeita_os_limites() {
        let mut l = Layout::leaf(1);
        l.dividir(1, Dir::Row, 2, true);
        l.dividir(2, Dir::Col, 3, true);
        let d = l.divisores(A, 2.0);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].dir, Dir::Row, "o divisor de fora vem primeiro");
        assert_eq!(d[1].dir, Dir::Col);
        // leva o divisor vertical para x = 30
        l.arrastar(0, A, 2.0, 30.0);
        let (r1, r2) = (rect_de(&l, 1, 2.0), rect_de(&l, 2, 2.0));
        assert!((r1.w - 29.0).abs() < 1.5, "{r1:?}");
        assert!((r2.x - 31.0).abs() < 1.5, "{r2:?}");
        // além do limite: não some com o painel
        l.arrastar(0, A, 2.0, 1000.0);
        assert!(rect_de(&l, 1, 2.0).w > 80.0 && rect_de(&l, 2, 2.0).w > 4.0);
        // o divisor horizontal (dentro do filho 2)
        l.arrastar(1, A, 2.0, 20.0);
        assert!(rect_de(&l, 2, 2.0).h < 30.0, "o 2 encolheu");
    }
}
