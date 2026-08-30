//! Gerador de números aleatórios próprio (splitmix64 + xorshift128+).
//!
//! Implementado à mão pra manter o núcleo com zero dependências. Determinístico
//! por semente — o que importa muito nos testes de gradiente: um teste que falha
//! precisa falhar de novo do mesmo jeito.

use crate::num::Float;

pub struct Rng {
    s0: u64,
    s1: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 pra espalhar a semente antes de alimentar o xorshift:
        // sementes pequenas e sequenciais (0, 1, 2...) geram estados iniciais
        // pobres num xorshift cru.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut next = || {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            x ^ (x >> 31)
        };
        Self {
            s0: next(),
            s1: next(),
        }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s0;
        let y = self.s1;
        self.s0 = y;
        x ^= x << 23;
        self.s1 = x ^ y ^ (x >> 17) ^ (y >> 26);
        self.s1.wrapping_add(y)
    }

    /// Uniforme em [0,1).
    #[inline]
    pub fn uniform01(&mut self) -> f64 {
        // 53 bits de mantissa: o máximo que um f64 representa sem repetir.
        ((self.next_u64() >> 11) as f64) * (1.0 / 9007199254740992.0)
    }

    /// Uniforme em [lo, hi).
    #[inline]
    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform01()
    }

    /// Normal padrão via Box-Muller.
    pub fn normal(&mut self) -> f64 {
        // Evita u = 0 exato, que levaria ln(0) = -inf.
        let u1 = self.uniform01().max(1e-300);
        let u2 = self.uniform01();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }

    /// Preenche um buffer com N(0, std²).
    pub fn fill_normal<T: Float>(&mut self, buf: &mut [T], std: f64) {
        for v in buf.iter_mut() {
            *v = T::from_f64(self.normal() * std);
        }
    }

    /// Preenche um buffer com U[lo, hi).
    pub fn fill_uniform<T: Float>(&mut self, buf: &mut [T], lo: f64, hi: f64) {
        for v in buf.iter_mut() {
            *v = T::from_f64(self.uniform(lo, hi));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinismo_por_semente() {
        let a: Vec<u64> = (0..8).map(|_| Rng::new(42).next_u64()).collect();
        let mut r = Rng::new(42);
        assert!(a.iter().all(|&x| x == a[0]));
        assert_eq!(r.next_u64(), a[0]);
    }

    #[test]
    fn normal_tem_media_zero_e_variancia_um() {
        let mut r = Rng::new(7);
        let n = 200_000;
        let xs: Vec<f64> = (0..n).map(|_| r.normal()).collect();
        let media = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - media) * (x - media)).sum::<f64>() / n as f64;
        assert!(media.abs() < 0.02, "media={media}");
        assert!((var - 1.0).abs() < 0.03, "var={var}");
    }

    #[test]
    fn uniform01_fica_no_intervalo() {
        let mut r = Rng::new(3);
        for _ in 0..10_000 {
            let u = r.uniform01();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
