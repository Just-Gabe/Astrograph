//! Animação: curvas de easing e molas. Tudo em função do tempo (dt em segundos),
//! para a mesma sensação em 60, 120 ou 240 Hz.

/// Desacelera no fim (a mais usada na interface).
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Entrada e saída suaves.
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Parte rápida e assenta devagar: para janelas que aparecem.
pub fn ease_out_expo(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t >= 1.0 { 1.0 } else { 1.0 - 2f32.powf(-10.0 * t) }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Mola com amortecimento: `omega` (rad/s) é a rapidez, `zeta` o amortecimento
/// (1 = sem quicar). Integra em passos pequenos para ser estável com dt grande.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
    omega: f32,
    zeta: f32,
}

impl Spring {
    pub fn new(x: f32, omega: f32, zeta: f32) -> Self {
        Spring { x, v: 0.0, target: x, omega, zeta }
    }

    /// Troca a rapidez da mola (1/s) sem mexer na posição.
    pub fn set_omega(&mut self, omega: f32) {
        self.omega = omega;
    }

    /// Rapidez de uma mola criticamente amortecida que chega (a ~1% do caminho) em `ms` milissegundos.
    pub fn omega_para_ms(ms: u64) -> f32 {
        4.6 / (ms.max(1) as f32 / 1000.0)
    }

    #[cfg(test)]
    pub fn omega_para_teste(&self) -> f32 {
        self.omega
    }

    pub fn snap(&mut self, x: f32) {
        self.x = x;
        self.target = x;
        self.v = 0.0;
    }

    pub fn step(&mut self, dt: f32) {
        let mut left = dt.clamp(0.0, 0.25);
        let inicio = self.x;
        while left > 0.0 {
            let h = left.min(0.004);
            let a = self.omega * self.omega * (self.target - self.x) - 2.0 * self.zeta * self.omega * self.v;
            self.v += a * h;
            self.x += self.v * h;
            left -= h;
        }
        // não andou nada no quadro inteiro (não só no último sub-passo, que pode ser minúsculo)
        let parada = dt > 0.0 && self.x == inicio;
        // perto o bastante: encaixa no alvo. Sem isso a cena declara "parou" a uma fração do alvo e o próximo quadro
        // dá um tranco. Longe da origem (rolagem num arquivo grande, em pixels) o f32 não anda menos que um passo:
        // a posição para a alguns passos do alvo com velocidade residual e a cena repintaria para sempre; se ela
        // parou de andar e está perto (relativo ao valor), também encaixa
        let d = (self.target - self.x).abs();
        let perto = d < 0.01 && self.v.abs() < 0.1;
        // presa: a velocidade residual não move a posição (menos de meio passo do f32 por sub-passo)
        let presa = parada && d < (self.target.abs() * 1e-5).max(0.01);
        if perto || presa {
            self.x = self.target;
            self.v = 0.0;
        }
    }

    pub fn settled(&self) -> bool {
        self.x == self.target && self.v == 0.0
    }
}

/// Valor que persegue um alvo com suavização exponencial (fade, presença).
#[derive(Clone, Copy, Debug)]
pub struct Chase {
    pub v: f32,
    pub target: f32,
    /// 1/segundo: quanto maior, mais rápido
    pub rate: f32,
}

impl Chase {
    pub fn new(v: f32, rate: f32) -> Self {
        Chase { v, target: v, rate }
    }

    pub fn step(&mut self, dt: f32) {
        let k = 1.0 - (-dt * self.rate).exp();
        self.v += (self.target - self.v) * k;
        if (self.target - self.v).abs() < 0.002 {
            self.v = self.target;
        }
    }

    pub fn settled(&self) -> bool {
        self.v == self.target
    }
}

/// Mistura duas cores 0xRRGGBB (t em 0..1).
pub fn mix_color(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 0xff) as f32, ((b >> s) & 0xff) as f32);
        (x + (y - x) * t).round() as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn easings_vao_de_0_a_1() {
        for f in [ease_out_cubic, ease_in_out_cubic, ease_out_expo] {
            assert!(f(0.0).abs() < 1e-6 && (f(1.0) - 1.0).abs() < 1e-6);
            assert!(f(0.5) > 0.0 && f(0.5) < 1.0);
        }
        assert!(ease_out_cubic(0.2) > 0.2, "desacelera: começa rápido");
    }

    #[test]
    fn mola_critica_converge_sem_passar_do_alvo() {
        let mut s = Spring::new(0.0, 22.0, 1.0);
        s.target = 100.0;
        let mut max: f32 = 0.0;
        for _ in 0..240 {
            s.step(1.0 / 120.0);
            max = max.max(s.x);
        }
        assert!(s.settled(), "convergiu: {}", s.x);
        assert_eq!(s.x, 100.0, "assentada, fica exatamente no alvo (sem tranco no próximo quadro)");
        // longe da origem (rolagem num arquivo enorme, alvo em pixels): também assenta
        for alvo in [83_886.0f32, 520_000.0, 5_200_000.0] {
            let mut s = Spring::new(alvo - 3_000.0, 16.0, 1.0);
            s.target = alvo;
            for _ in 0..600 {
                s.step(1.0 / 120.0);
            }
            assert!(s.settled() && s.x == alvo, "alvo {alvo}: parou em {} (v {})", s.x, s.v);
        }
        // e longe da origem um movimento pequeno ainda anima (não encaixa no primeiro quadro)
        for (alvo, passo) in [(6_600_000.0f32, 20.0f32), (400_000.0, 20.0), (6_600_000.0, 300.0)] {
            let mut s = Spring::new(alvo - passo, 22.0, 1.0);
            s.target = alvo;
            s.step(1.0 / 120.0);
            assert!(s.x < alvo && s.x > alvo - passo, "alvo {alvo}, passo {passo}: anda sem pular ({})", s.x);
            let mut quadros = 1;
            while !s.settled() && quadros < 600 {
                s.step(1.0 / 120.0);
                quadros += 1;
            }
            assert!(s.settled() && quadros > 5, "alvo {alvo}, passo {passo}: assentou em {quadros} quadros");
        }
        assert!(max <= 100.5, "sem quicar: {max}");
        // mesmo resultado com passos diferentes
        let mut a = Spring::new(0.0, 22.0, 1.0);
        let mut b = Spring::new(0.0, 22.0, 1.0);
        a.target = 50.0;
        b.target = 50.0;
        for _ in 0..60 {
            a.step(1.0 / 60.0);
        }
        for _ in 0..240 {
            b.step(1.0 / 240.0);
        }
        assert!((a.x - b.x).abs() < 0.5, "independente do passo: {} vs {}", a.x, b.x);
    }

    #[test]
    fn mola_subamortecida_quica() {
        let mut s = Spring::new(0.0, 30.0, 0.4);
        s.target = 10.0;
        let mut passou = false;
        for _ in 0..200 {
            s.step(1.0 / 120.0);
            passou |= s.x > 10.2;
        }
        assert!(passou);
    }

    #[test]
    fn chase_e_cores() {
        let mut c = Chase::new(0.0, 14.0);
        c.target = 1.0;
        for _ in 0..120 {
            c.step(1.0 / 60.0);
        }
        assert!(c.settled());
        assert_eq!(mix_color(0x000000, 0xffffff, 0.5), 0x808080);
        assert_eq!(mix_color(0x102030, 0x102030, 0.7), 0x102030);
    }
}
