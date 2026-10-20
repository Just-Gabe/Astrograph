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

    pub fn snap(&mut self, x: f32) {
        self.x = x;
        self.target = x;
        self.v = 0.0;
    }

    pub fn step(&mut self, dt: f32) {
        let mut left = dt.clamp(0.0, 0.25);
        while left > 0.0 {
            let h = left.min(0.004);
            let a = self.omega * self.omega * (self.target - self.x) - 2.0 * self.zeta * self.omega * self.v;
            self.v += a * h;
            self.x += self.v * h;
            left -= h;
        }
    }

    pub fn settled(&self) -> bool {
        (self.target - self.x).abs() < 0.05 && self.v.abs() < 0.5
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
