//! Tipo numérico genérico.
//!
//! Todo o núcleo matemático da Teka é genérico sobre `Float`. Em produção roda em
//! `f32`; nos testes roda em `f64`, o que torna a checagem de gradiente por
//! diferenças finitas confiável — o erro de truncamento cai de ~1e-3 (f32) para
//! ~1e-10 (f64), a diferença entre "o teste passa" e "o teste prova".
//!
//! As funções de ativação estão aqui, e não espalhadas, por um motivo: cada uma tem
//! uma armadilha numérica conhecida, e o lugar de resolver isso é uma vez só.

use std::fmt::Debug;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

pub trait Float:
    Copy
    + Debug
    + Default
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + AddAssign
    + SubAssign
    + MulAssign
    + DivAssign
    + Send
    + Sync
    + 'static
{
    const ZERO: Self;
    const ONE: Self;
    const TWO: Self;

    fn from_f64(v: f64) -> Self;
    fn to_f64(self) -> f64;

    fn exp(self) -> Self;
    fn ln(self) -> Self;
    fn sqrt(self) -> Self;
    fn abs(self) -> Self;

    #[inline]
    fn max(self, other: Self) -> Self {
        if self > other {
            self
        } else {
            other
        }
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        if self < other {
            self
        } else {
            other
        }
    }

    /// σ(x) = 1/(1+e⁻ˣ).
    ///
    /// Os dois ramos evitam overflow: `exp(-x)` estoura para x muito negativo,
    /// `exp(x)` estoura para x muito positivo. Cada ramo usa o lado seguro.
    #[inline]
    fn sigmoid(self) -> Self {
        if self >= Self::ZERO {
            Self::ONE / (Self::ONE + (-self).exp())
        } else {
            let e = self.exp();
            e / (Self::ONE + e)
        }
    }

    /// softplus(x) = ln(1+eˣ), estável nos dois extremos.
    #[inline]
    fn softplus(self) -> Self {
        let big = Self::from_f64(30.0);
        if self > big {
            // ln(1+eˣ) ≈ x  (o 1 some na precisão do float)
            self
        } else if self < -big {
            // ln(1+eˣ) ≈ eˣ
            self.exp()
        } else {
            (Self::ONE + self.exp()).ln()
        }
    }

    /// ln σ(x), calculado direto em vez de `sigmoid().ln()`.
    ///
    /// Isso importa muito aqui: no RG-LRU o decaimento é inicializado com σ(Λ) ≈ 0,999,
    /// e `ln(0.999)` calculado depois de arredondar σ(Λ) perde quase toda a precisão
    /// significativa. A identidade ln σ(x) = −softplus(−x) preserva ela.
    #[inline]
    fn ln_sigmoid(self) -> Self {
        -((-self).softplus())
    }

    /// Derivada de `ln_sigmoid`: d/dx ln σ(x) = 1 − σ(x).
    #[inline]
    fn d_ln_sigmoid(self) -> Self {
        Self::ONE - self.sigmoid()
    }

    #[inline]
    fn silu(self) -> Self {
        self * self.sigmoid()
    }

    /// Derivada de `silu`: σ(x)·(1 + x·(1−σ(x))).
    #[inline]
    fn d_silu(self) -> Self {
        let s = self.sigmoid();
        s * (Self::ONE + self * (Self::ONE - s))
    }
}

macro_rules! impl_float {
    ($t:ty) => {
        impl Float for $t {
            const ZERO: Self = 0.0;
            const ONE: Self = 1.0;
            const TWO: Self = 2.0;

            #[inline]
            fn from_f64(v: f64) -> Self {
                v as $t
            }
            #[inline]
            fn to_f64(self) -> f64 {
                self as f64
            }
            #[inline]
            fn exp(self) -> Self {
                <$t>::exp(self)
            }
            #[inline]
            fn ln(self) -> Self {
                <$t>::ln(self)
            }
            #[inline]
            fn sqrt(self) -> Self {
                <$t>::sqrt(self)
            }
            #[inline]
            fn abs(self) -> Self {
                <$t>::abs(self)
            }
        }
    };
}

impl_float!(f32);
impl_float!(f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmoid_nao_estoura_nos_extremos() {
        assert!(f64::sigmoid(1000.0) > 0.999999);
        assert!(f64::sigmoid(-1000.0) < 1e-6);
        assert!(f64::sigmoid(1000.0).is_finite());
        assert!(f64::sigmoid(-1000.0).is_finite());
    }

    #[test]
    fn ln_sigmoid_bate_com_a_definicao() {
        for &x in &[-8.0f64, -1.0, 0.0, 1.0, 5.0] {
            let direto = x.ln_sigmoid();
            let ingenuo = x.sigmoid().ln();
            assert!((direto - ingenuo).abs() < 1e-9, "x={x}");
        }
        // No regime que o RG-LRU usa de verdade (σ(Λ) ≈ 0,999) a versão direta
        // continua finita e precisa.
        let x = 7.0f64;
        assert!(x.ln_sigmoid().is_finite());
        assert!(x.ln_sigmoid() < 0.0);
    }

    #[test]
    fn derivada_de_ln_sigmoid_bate_por_diferenca_finita() {
        let h = 1e-6;
        for &x in &[-3.0f64, -0.5, 0.0, 2.0, 6.0] {
            let num = ((x + h).ln_sigmoid() - (x - h).ln_sigmoid()) / (2.0 * h);
            let ana = x.d_ln_sigmoid();
            assert!((num - ana).abs() < 1e-7, "x={x} num={num} ana={ana}");
        }
    }
}
