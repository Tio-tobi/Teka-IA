//! Tabela de embedding de bytes.
//!
//! Aqui mora uma vantagem de byte-level que quase ninguém comenta: o vocabulário
//! tem **256 entradas**. Com `dim = 384` em f32 a tabela inteira ocupa 384 KB e cabe
//! folgada no L2 (512 KB por core no Zen 3). Nenhum cache miss em lookup — contra
//! dezenas de MB de tabela num modelo com vocabulário de 128 mil.
//!
//! O forward é *gather* e o backward é *scatter-add*: nada de GEMM contra um one-hot.

use crate::num::Float;
use crate::rng::Rng;

pub const VOCAB: usize = 256;

#[derive(Clone, Debug)]
pub struct Embed<T: Float> {
    pub dim: usize,
    /// `[VOCAB, dim]`
    pub w: Vec<T>,
}

#[derive(Clone, Debug)]
pub struct EmbedGrad<T: Float> {
    pub dw: Vec<T>,
}

impl<T: Float> EmbedGrad<T> {
    pub fn zeros(dim: usize) -> Self {
        Self {
            dw: vec![T::ZERO; VOCAB * dim],
        }
    }
    pub fn clear(&mut self) {
        self.dw.fill(T::ZERO);
    }
}

impl<T: Float> Embed<T> {
    pub fn new(dim: usize, rng: &mut Rng) -> Self {
        let mut w = vec![T::ZERO; VOCAB * dim];
        // Desvio pequeno: o embedding entra direto no primeiro RMSNorm, então
        // escala grande aqui só desperdiça o começo do treino.
        rng.fill_normal(&mut w, 0.02);
        Self { dim, w }
    }

    pub fn grad(&self) -> EmbedGrad<T> {
        EmbedGrad::zeros(self.dim)
    }

    pub fn n_params(&self) -> usize {
        self.w.len()
    }

    /// `ids: [n]` → `out: [n, dim]`
    pub fn forward(&self, ids: &[u8], out: &mut [T]) {
        debug_assert_eq!(out.len(), ids.len() * self.dim);
        for (i, &id) in ids.iter().enumerate() {
            let src = id as usize * self.dim;
            out[i * self.dim..(i + 1) * self.dim]
                .copy_from_slice(&self.w[src..src + self.dim]);
        }
    }

    /// Scatter-add. Bytes repetidos acumulam no mesmo lugar — é o gradiente certo.
    pub fn backward(&self, ids: &[u8], dout: &[T], grad: &mut EmbedGrad<T>) {
        debug_assert_eq!(dout.len(), ids.len() * self.dim);
        for (i, &id) in ids.iter().enumerate() {
            let dst = id as usize * self.dim;
            let src = i * self.dim;
            for c in 0..self.dim {
                grad.dw[dst + c] += dout[src + c];
            }
        }
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        vec![&mut self.w[..]]
    }

    /// `(nome, forma)` na mesma ordem de [`Self::params_mut`].
    pub fn descritores(&self, p: &str) -> Vec<(String, Vec<usize>)> {
        vec![(format!("{p}.w"), vec![self.w.len() / self.dim.max(1), self.dim])]
    }
}

impl<T: Float> EmbedGrad<T> {
    pub fn slices(&self) -> Vec<&[T]> {
        vec![&self.dw[..]]
    }
}
