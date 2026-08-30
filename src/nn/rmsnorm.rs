//! RMSNorm — normalização por raiz quadrática média.
//!
//! ```text
//! inv = 1 / sqrt(mean(x²) + eps)
//! y   = g ⊙ x ⊙ inv
//! ```
//!
//! Escolhida no lugar de LayerNorm porque não subtrai a média: uma passada a menos
//! sobre os dados, um gradiente a menos, e nenhuma perda de qualidade demonstrada.
//! Numa arquitetura que roda em CPU e é limitada por banda de memória, uma passada
//! a menos não é detalhe.
//!
//! Backward:
//! ```text
//! S      = Σⱼ dy[j]·g[j]·x[j]
//! dx[i]  = g[i]·inv·dy[i] − x[i]·inv³·S / dim
//! dg[i] += dy[i]·x[i]·inv
//! ```

use crate::num::Float;

#[derive(Clone, Debug)]
pub struct RmsNorm<T: Float> {
    pub dim: usize,
    /// Ganho por canal `[dim]`, inicializado em 1.
    pub g: Vec<T>,
    pub eps: T,
}

#[derive(Clone, Debug)]
pub struct RmsNormGrad<T: Float> {
    pub dg: Vec<T>,
}

impl<T: Float> RmsNormGrad<T> {
    pub fn zeros(dim: usize) -> Self {
        Self {
            dg: vec![T::ZERO; dim],
        }
    }
    pub fn clear(&mut self) {
        self.dg.fill(T::ZERO);
    }
}

/// Guarda `inv` por linha — recomputar no backward custaria outra passada completa.
#[derive(Clone, Debug, Default)]
pub struct RmsNormCache<T: Float> {
    pub inv: Vec<T>,
}

impl<T: Float> RmsNorm<T> {

    pub fn descritores(&self, p: &str) -> Vec<(String, Vec<usize>)> {
        vec![(format!("{p}.g"), vec![self.dim])]
    }
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            g: vec![T::ONE; dim],
            eps: T::from_f64(1e-6),
        }
    }

    pub fn grad(&self) -> RmsNormGrad<T> {
        RmsNormGrad::zeros(self.dim)
    }

    pub fn n_params(&self) -> usize {
        self.g.len()
    }

    pub fn forward(&self, x: &[T], rows: usize, y: &mut [T], cache: &mut RmsNormCache<T>) {
        debug_assert_eq!(x.len(), rows * self.dim);
        debug_assert_eq!(y.len(), rows * self.dim);
        cache.inv.resize(rows, T::ZERO);

        let n = T::from_f64(self.dim as f64);
        for r in 0..rows {
            let xr = &x[r * self.dim..(r + 1) * self.dim];
            let mut ss = T::ZERO;
            for &v in xr {
                ss += v * v;
            }
            let inv = T::ONE / (ss / n + self.eps).sqrt();
            cache.inv[r] = inv;

            let yr = &mut y[r * self.dim..(r + 1) * self.dim];
            for i in 0..self.dim {
                yr[i] = self.g[i] * xr[i] * inv;
            }
        }
    }

    pub fn backward(
        &self,
        x: &[T],
        dy: &[T],
        rows: usize,
        cache: &RmsNormCache<T>,
        dx: &mut [T],
        accum_dx: bool,
        grad: &mut RmsNormGrad<T>,
    ) {
        debug_assert_eq!(dx.len(), rows * self.dim);
        let n = T::from_f64(self.dim as f64);

        for r in 0..rows {
            let xr = &x[r * self.dim..(r + 1) * self.dim];
            let dyr = &dy[r * self.dim..(r + 1) * self.dim];
            let inv = cache.inv[r];

            let mut s = T::ZERO;
            for i in 0..self.dim {
                s += dyr[i] * self.g[i] * xr[i];
            }
            let coef = inv * inv * inv * s / n;

            let dxr = &mut dx[r * self.dim..(r + 1) * self.dim];
            for i in 0..self.dim {
                let v = self.g[i] * inv * dyr[i] - xr[i] * coef;
                if accum_dx {
                    dxr[i] += v;
                } else {
                    dxr[i] = v;
                }
                grad.dg[i] += dyr[i] * xr[i] * inv;
            }
        }
    }
}
