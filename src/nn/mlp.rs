//! Bloco MLP com portão (SwiGLU), pré-norma e residual.
//!
//! ```text
//! y   = RMSNorm(x)
//! p   = up(y) ⊙ silu(gate(y))
//! out = x + down(p)
//! ```
//!
//! O portão custa uma projeção a mais e paga: a rede escolhe por canal quanto de
//! cada feature deixa passar, em vez de aplicar a mesma não-linearidade em tudo.
//!
//! `f = 2·d` em vez do 4·d habitual. Numa arquitetura limitada por banda de memória
//! na CPU, cada parâmetro é lido do zero a cada passo — o MLP é o maior consumidor
//! de peso do bloco, e 2·d com portão rende mais que 4·d sem.

use crate::backend::Ops;
use crate::nn::linear::{Linear, LinearGrad};
use crate::nn::rmsnorm::{RmsNorm, RmsNormCache, RmsNormGrad};
use crate::num::Float;
use crate::rng::Rng;

#[derive(Clone, Debug)]
pub struct BlocoMlp<T: Float> {
    pub d: usize,
    pub f: usize,
    pub norm: RmsNorm<T>,
    pub up: Linear<T>,
    pub gate: Linear<T>,
    pub down: Linear<T>,
}

#[derive(Clone, Debug)]
pub struct BlocoMlpGrad<T: Float> {
    pub norm: RmsNormGrad<T>,
    pub up: LinearGrad<T>,
    pub gate: LinearGrad<T>,
    pub down: LinearGrad<T>,
}

impl<T: Float> BlocoMlpGrad<T> {
    pub fn clear(&mut self) {
        self.norm.clear();
        self.up.clear();
        self.gate.clear();
        self.down.clear();
    }
    pub fn slices(&self) -> Vec<&[T]> {
        vec![
            &self.norm.dg[..],
            &self.up.dw[..],
            &self.up.db[..],
            &self.gate.dw[..],
            &self.gate.db[..],
            &self.down.dw[..],
            &self.down.db[..],
        ]
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlocoMlpCache<T: Float> {
    pub norm: RmsNormCache<T>,
    pub y: Vec<T>,
    pub u: Vec<T>,
    pub gpre: Vec<T>,
    pub sg: Vec<T>,
    pub p: Vec<T>,
    // scratch do backward, mora aqui pra não alocar no laço quente
    dp: Vec<T>,
    du: Vec<T>,
    dg: Vec<T>,
    dy: Vec<T>,
}

impl<T: Float> BlocoMlpCache<T> {
    pub fn new() -> Self {
        Self::default()
    }
    fn ensure(&mut self, rows: usize, d: usize, f: usize) {
        self.y.resize(rows * d, T::ZERO);
        self.dy.resize(rows * d, T::ZERO);
        for buf in [
            &mut self.u,
            &mut self.gpre,
            &mut self.sg,
            &mut self.p,
            &mut self.dp,
            &mut self.du,
            &mut self.dg,
        ] {
            buf.resize(rows * f, T::ZERO);
        }
    }
}

impl<T: Float> BlocoMlp<T> {
    pub fn new(d: usize, f: usize, rng: &mut Rng) -> Self {
        Self {
            d,
            f,
            norm: RmsNorm::new(d),
            up: Linear::new(d, f, rng),
            gate: Linear::new(d, f, rng),
            down: Linear::new(f, d, rng),
        }
    }

    pub fn grad(&self) -> BlocoMlpGrad<T> {
        BlocoMlpGrad {
            norm: self.norm.grad(),
            up: self.up.grad(),
            gate: self.gate.grad(),
            down: self.down.grad(),
        }
    }

    pub fn n_params(&self) -> usize {
        self.norm.n_params() + self.up.n_params() + self.gate.n_params() + self.down.n_params()
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        vec![
            &mut self.norm.g[..],
            &mut self.up.w[..],
            &mut self.up.b[..],
            &mut self.gate.w[..],
            &mut self.gate.b[..],
            &mut self.down.w[..],
            &mut self.down.b[..],
        ]
    }

    pub fn descritores(&self, p: &str) -> Vec<(String, Vec<usize>)> {
        let mut v = self.norm.descritores(&format!("{p}.norm"));
        v.extend(self.up.descritores(&format!("{p}.up")));
        v.extend(self.gate.descritores(&format!("{p}.gate")));
        v.extend(self.down.descritores(&format!("{p}.down")));
        v
    }

    pub fn forward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        rows: usize,
        out: &mut [T],
        cache: &mut BlocoMlpCache<T>,
    ) {
        cache.ensure(rows, self.d, self.f);
        self.norm.forward(x, rows, &mut cache.y, &mut cache.norm);
        self.up.forward(ops, &cache.y, rows, &mut cache.u);
        self.gate.forward(ops, &cache.y, rows, &mut cache.gpre);
        for k in 0..rows * self.f {
            cache.sg[k] = cache.gpre[k].silu();
            cache.p[k] = cache.u[k] * cache.sg[k];
        }
        self.down.forward(ops, &cache.p, rows, out);
        for k in 0..rows * self.d {
            out[k] += x[k]; // residual
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn backward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        dout: &[T],
        rows: usize,
        cache: &mut BlocoMlpCache<T>,
        dx: &mut [T],
        accum_dx: bool,
        grad: &mut BlocoMlpGrad<T>,
    ) {
        self.down.backward(
            ops,
            &cache.p,
            dout,
            rows,
            Some(&mut cache.dp),
            false,
            &mut grad.down,
        );
        for k in 0..rows * self.f {
            cache.du[k] = cache.dp[k] * cache.sg[k];
            cache.dg[k] = cache.dp[k] * cache.u[k] * cache.gpre[k].d_silu();
        }
        self.up.backward(
            ops,
            &cache.y,
            &cache.du,
            rows,
            Some(&mut cache.dy),
            false,
            &mut grad.up,
        );
        self.gate.backward(
            ops,
            &cache.y,
            &cache.dg,
            rows,
            Some(&mut cache.dy),
            true,
            &mut grad.gate,
        );

        // Residual: o gradiente chega em `x` por dois caminhos — direto e pela norma.
        if accum_dx {
            for k in 0..rows * self.d {
                dx[k] += dout[k];
            }
        } else {
            dx.copy_from_slice(dout);
        }
        self.norm
            .backward(x, &cache.dy, rows, &cache.norm, dx, true, &mut grad.norm);
    }
}
