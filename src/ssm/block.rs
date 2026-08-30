//! Bloco recorrente: pré-norma, RG-LRU com portão paralelo, residual.
//!
//! ```text
//! y   = RMSNorm(x)
//! s   = RG-LRU( lin_in(y) )          ramo recorrente (tem memória)
//! g   = silu( lin_gate(y) )          ramo direto (sem memória)
//! out = x + lin_out(s ⊙ g)
//! ```
//!
//! Os dois ramos fazem coisas diferentes de propósito. O recorrente carrega o
//! passado; o direto olha só o instante. O produto deixa o instante **modular** o
//! que da memória passa adiante — é o que dá ao bloco a capacidade de ignorar o
//! próprio estado quando a entrada atual manda ignorar.
//!
//! O estado final (`estado_final`) é o que permite treinar uma sequência longa em
//! janelas: ele vira o `h0` da janela seguinte.

use crate::backend::Ops;
use crate::nn::linear::{Linear, LinearGrad};
use crate::nn::rmsnorm::{RmsNorm, RmsNormCache, RmsNormGrad};
use crate::num::Float;
use crate::rng::Rng;
use crate::ssm::rglru::{RgLru, RgLruCache, RgLruGrad};

#[derive(Clone, Debug)]
pub struct BlocoRec<T: Float> {
    pub d: usize,
    pub h: usize,
    pub norm: RmsNorm<T>,
    pub lin_in: Linear<T>,
    pub lin_gate: Linear<T>,
    pub rglru: RgLru<T>,
    pub lin_out: Linear<T>,
}

#[derive(Clone, Debug)]
pub struct BlocoRecGrad<T: Float> {
    pub norm: RmsNormGrad<T>,
    pub lin_in: LinearGrad<T>,
    pub lin_gate: LinearGrad<T>,
    pub rglru: RgLruGrad<T>,
    pub lin_out: LinearGrad<T>,
}

impl<T: Float> BlocoRecGrad<T> {
    pub fn clear(&mut self) {
        self.norm.clear();
        self.lin_in.clear();
        self.lin_gate.clear();
        self.rglru.clear();
        self.lin_out.clear();
    }
    pub fn slices(&self) -> Vec<&[T]> {
        vec![
            &self.norm.dg[..],
            &self.lin_in.dw[..],
            &self.lin_in.db[..],
            &self.lin_gate.dw[..],
            &self.lin_gate.db[..],
            &self.rglru.g_r.dw[..],
            &self.rglru.g_r.db[..],
            &self.rglru.g_i.dw[..],
            &self.rglru.g_i.db[..],
            &self.rglru.dlambda[..],
            &self.lin_out.dw[..],
            &self.lin_out.db[..],
        ]
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlocoRecCache<T: Float> {
    pub norm: RmsNormCache<T>,
    pub rglru: RgLruCache<T>,
    pub y: Vec<T>,
    pub a: Vec<T>,
    pub gpre: Vec<T>,
    pub sg: Vec<T>,
    pub s: Vec<T>,
    pub prod: Vec<T>,
    dprod: Vec<T>,
    ds: Vec<T>,
    dg: Vec<T>,
    da: Vec<T>,
    dy: Vec<T>,
}

impl<T: Float> BlocoRecCache<T> {
    pub fn new() -> Self {
        Self::default()
    }
    fn ensure(&mut self, rows: usize, d: usize, h: usize) {
        self.y.resize(rows * d, T::ZERO);
        self.dy.resize(rows * d, T::ZERO);
        for buf in [
            &mut self.a,
            &mut self.gpre,
            &mut self.sg,
            &mut self.s,
            &mut self.prod,
            &mut self.dprod,
            &mut self.ds,
            &mut self.dg,
            &mut self.da,
        ] {
            buf.resize(rows * h, T::ZERO);
        }
    }

    /// Estado recorrente ao final da janela — vira o `h0` da janela seguinte.
    pub fn estado_final(&self, seq: usize, batch: usize, h: usize) -> Vec<T> {
        self.s[(seq - 1) * batch * h..seq * batch * h].to_vec()
    }
}

impl<T: Float> BlocoRec<T> {
    pub fn new(d: usize, h: usize, rng: &mut Rng) -> Self {
        Self {
            d,
            h,
            norm: RmsNorm::new(d),
            lin_in: Linear::new(d, h, rng),
            lin_gate: Linear::new(d, h, rng),
            rglru: RgLru::new(h, rng),
            lin_out: Linear::new(h, d, rng),
        }
    }

    pub fn grad(&self) -> BlocoRecGrad<T> {
        BlocoRecGrad {
            norm: self.norm.grad(),
            lin_in: self.lin_in.grad(),
            lin_gate: self.lin_gate.grad(),
            rglru: self.rglru.grad(),
            lin_out: self.lin_out.grad(),
        }
    }

    pub fn n_params(&self) -> usize {
        self.norm.n_params()
            + self.lin_in.n_params()
            + self.lin_gate.n_params()
            + self.rglru.n_params()
            + self.lin_out.n_params()
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        vec![
            &mut self.norm.g[..],
            &mut self.lin_in.w[..],
            &mut self.lin_in.b[..],
            &mut self.lin_gate.w[..],
            &mut self.lin_gate.b[..],
            &mut self.rglru.lin_r.w[..],
            &mut self.rglru.lin_r.b[..],
            &mut self.rglru.lin_i.w[..],
            &mut self.rglru.lin_i.b[..],
            &mut self.rglru.lambda[..],
            &mut self.lin_out.w[..],
            &mut self.lin_out.b[..],
        ]
    }

    pub fn descritores(&self, p: &str) -> Vec<(String, Vec<usize>)> {
        let mut v = self.norm.descritores(&format!("{p}.norm"));
        v.extend(self.lin_in.descritores(&format!("{p}.lin_in")));
        v.extend(self.lin_gate.descritores(&format!("{p}.lin_gate")));
        v.extend(self.rglru.lin_r.descritores(&format!("{p}.rglru.lin_r")));
        v.extend(self.rglru.lin_i.descritores(&format!("{p}.rglru.lin_i")));
        v.push((format!("{p}.rglru.lambda"), vec![self.rglru.lambda.len()]));
        v.extend(self.lin_out.descritores(&format!("{p}.lin_out")));
        v
    }

    pub fn zero_state(&self, batch: usize) -> Vec<T> {
        vec![T::ZERO; batch * self.h]
    }

    #[allow(clippy::too_many_arguments)]
    pub fn forward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        h0: &[T],
        seq: usize,
        batch: usize,
        out: &mut [T],
        cache: &mut BlocoRecCache<T>,
    ) {
        let rows = seq * batch;
        cache.ensure(rows, self.d, self.h);

        self.norm.forward(x, rows, &mut cache.y, &mut cache.norm);
        self.lin_in.forward(ops, &cache.y, rows, &mut cache.a);
        self.lin_gate.forward(ops, &cache.y, rows, &mut cache.gpre);

        self.rglru.forward(
            ops,
            &cache.a,
            h0,
            seq,
            batch,
            &mut cache.s,
            &mut cache.rglru,
        );

        for k in 0..rows * self.h {
            cache.sg[k] = cache.gpre[k].silu();
            cache.prod[k] = cache.s[k] * cache.sg[k];
        }
        self.lin_out.forward(ops, &cache.prod, rows, out);
        for k in 0..rows * self.d {
            out[k] += x[k]; // residual
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn backward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        h0: &[T],
        dout: &[T],
        dh_last: Option<&[T]>,
        seq: usize,
        batch: usize,
        cache: &mut BlocoRecCache<T>,
        dx: &mut [T],
        accum_dx: bool,
        dh0: Option<&mut [T]>,
        grad: &mut BlocoRecGrad<T>,
    ) {
        let rows = seq * batch;

        self.lin_out.backward(
            ops,
            &cache.prod,
            dout,
            rows,
            Some(&mut cache.dprod),
            false,
            &mut grad.lin_out,
        );
        for k in 0..rows * self.h {
            cache.ds[k] = cache.dprod[k] * cache.sg[k];
            cache.dg[k] = cache.dprod[k] * cache.s[k] * cache.gpre[k].d_silu();
        }

        self.rglru.backward(
            ops,
            &cache.a,
            &cache.s,
            h0,
            &cache.ds,
            dh_last,
            seq,
            batch,
            &mut cache.rglru,
            &mut cache.da,
            false,
            dh0,
            &mut grad.rglru,
        );

        self.lin_in.backward(
            ops,
            &cache.y,
            &cache.da,
            rows,
            Some(&mut cache.dy),
            false,
            &mut grad.lin_in,
        );
        self.lin_gate.backward(
            ops,
            &cache.y,
            &cache.dg,
            rows,
            Some(&mut cache.dy),
            true,
            &mut grad.lin_gate,
        );

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
