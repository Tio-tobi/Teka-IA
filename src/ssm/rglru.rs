//! RG-LRU — a recorrência linear diagonal com portões (Griffin / Hawk).
//!
//! O núcleo da Teka. É, literalmente, uma GRU com o portão de reset e a matriz
//! recorrente `W_h` removidos:
//!
//! ```text
//! r_t = σ(W_r x_t + b_r)                  portão de recorrência
//! i_t = σ(W_i x_t + b_i)                  portão de entrada
//! p   = ln σ(Λ)                           < 0, por canal, aprendido
//! a_t = exp(c · r_t · p)                  ∈ (0,1), c = 8
//! n_t = sqrt(1 − a_t²)                    normaliza a variância
//! h_t = a_t ⊙ h_{t−1} + n_t ⊙ (i_t ⊙ x_t)
//! ```
//!
//! ## Por que isso e não GRU
//!
//! Na GRU o `W_h·h_{t−1}` é um matmul de d² **dentro** do laço temporal: sequencial,
//! GEMV, ~3-5% do pico da CPU. Aqui o laço temporal é puramente elementwise (d
//! multiplicações, não d²) e **as duas projeções saem do laço**: `W_r` e `W_i` são
//! aplicadas à sequência inteira de uma vez, em dois GEMMs grandes.
//!
//! Isso está explícito no código abaixo — os `lin_r.forward` / `lin_i.forward`
//! acontecem *antes* do laço sobre `t`, sobre `seq*batch` linhas. É a razão de ser
//! desta arquitetura e o motivo de ela caber num Ryzen sem GPU.
//!
//! ## Estabilidade
//!
//! `a_t ∈ (0,1)` por construção — `p < 0`, `r_t > 0`, `c > 0` — então nenhum
//! autovalor explode, para qualquer valor dos pesos. Não existe o modo de falha
//! clássico de RNN em que o estado diverge. `Λ` é inicializado de forma que o
//! decaimento máximo por passo fique em [0,9 , 0,999]: memória longa desde o
//! nascimento.
//!
//! ## Derivadas (BPTT)
//!
//! Percorrendo `t` de trás pra frente, com `dh` = gradiente que chega em `h_t`:
//!
//! ```text
//! da        = dh·h_{t−1} + (dh·u_t)·(−a_t/n_t)      u_t = i_t ⊙ x_t
//! dx       += (dh·n_t)·i_t
//! ds_i      = (dh·n_t·x_t)·i_t·(1−i_t)
//! dla       = da·a_t
//! ds_r      = (dla·c·p)·r_t·(1−r_t)
//! dΛ       += (dla·c·r_t)·(1−σ(Λ))
//! dh_{t−1}  = dh·a_t
//! ```

use crate::backend::Ops;
use crate::nn::linear::{Linear, LinearGrad};
use crate::num::Float;
use crate::rng::Rng;

/// Constante `c` do expoente do decaimento. 8 é o valor do artigo do Griffin.
pub const C_DECAY: f64 = 8.0;

/// Piso de `1 − a²` antes da raiz. Sem ele, `a → 1` produz `n → 0` e o termo
/// `−a/n` do backward vira infinito. O backward usa **a mesma condição**, então
/// forward e gradiente continuam exatamente consistentes na região travada.
const EPS_N: f64 = 1e-8;

#[derive(Clone, Debug)]
pub struct RgLru<T: Float> {
    pub h: usize,
    pub lin_r: Linear<T>,
    pub lin_i: Linear<T>,
    /// `[h]` — parametriza o decaimento por canal.
    pub lambda: Vec<T>,
}

#[derive(Clone, Debug)]
pub struct RgLruGrad<T: Float> {
    pub g_r: LinearGrad<T>,
    pub g_i: LinearGrad<T>,
    pub dlambda: Vec<T>,
}

impl<T: Float> RgLruGrad<T> {
    pub fn zeros(h: usize) -> Self {
        Self {
            g_r: LinearGrad::zeros(h, h),
            g_i: LinearGrad::zeros(h, h),
            dlambda: vec![T::ZERO; h],
        }
    }
    pub fn clear(&mut self) {
        self.g_r.clear();
        self.g_i.clear();
        self.dlambda.fill(T::ZERO);
    }
}

/// Ativações salvas entre forward e backward.
///
/// `s_r`/`s_i` são reaproveitados no backward como buffers de `ds_r`/`ds_i`: depois
/// que `r` e `i` estão guardados, os pré-ativados não servem mais pra nada. Com
/// `seq=256, batch=32, h=384` cada buffer é ~12 MB — reciclar dois deles não é
/// economia decorativa.
#[derive(Clone, Debug, Default)]
pub struct RgLruCache<T: Float> {
    pub r: Vec<T>,
    pub i: Vec<T>,
    pub a: Vec<T>,
    pub n: Vec<T>,
    pub s_r: Vec<T>,
    pub s_i: Vec<T>,
}

impl<T: Float> RgLruCache<T> {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure(&mut self, len: usize) {
        for buf in [
            &mut self.r,
            &mut self.i,
            &mut self.a,
            &mut self.n,
            &mut self.s_r,
            &mut self.s_i,
        ] {
            buf.resize(len, T::ZERO);
        }
    }
}

impl<T: Float> RgLru<T> {
    pub fn new(h: usize, rng: &mut Rng) -> Self {
        let mut lambda = vec![T::ZERO; h];
        for v in lambda.iter_mut() {
            // Amostra o decaimento máximo por passo (com r=1) uniformemente em
            // [0,9 , 0,999], e inverte pra achar Λ:
            //     a_max = σ(Λ)^c   →   Λ = logit( a_max^(1/c) )
            let a_max = rng.uniform(0.9, 0.999);
            let s = a_max.powf(1.0 / C_DECAY);
            *v = T::from_f64((s / (1.0 - s)).ln());
        }
        Self {
            h,
            lin_r: Linear::new(h, h, rng),
            lin_i: Linear::new(h, h, rng),
            lambda,
        }
    }

    pub fn grad(&self) -> RgLruGrad<T> {
        RgLruGrad::zeros(self.h)
    }

    pub fn n_params(&self) -> usize {
        self.lin_r.n_params() + self.lin_i.n_params() + self.lambda.len()
    }

    /// Estado inicial zerado para um lote.
    pub fn zero_state(&self, batch: usize) -> Vec<T> {
        vec![T::ZERO; batch * self.h]
    }

    /// `x`, `y`: `[seq, batch, h]` em ordem *time-major*. `h0`: `[batch, h]`.
    ///
    /// Time-major não é capricho: mantém a fatia `[batch, h]` de cada instante
    /// contígua, que é exatamente o que o laço do scan percorre.
    pub fn forward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        h0: &[T],
        seq: usize,
        batch: usize,
        y: &mut [T],
        cache: &mut RgLruCache<T>,
    ) {
        let h = self.h;
        let rows = seq * batch;
        debug_assert_eq!(x.len(), rows * h);
        debug_assert_eq!(y.len(), rows * h);
        debug_assert_eq!(h0.len(), batch * h);
        cache.ensure(rows * h);

        // ---- Fora do laço temporal: os dois GEMMs sobre a sequência inteira. ----
        self.lin_r.forward(ops, x, rows, &mut cache.s_r);
        self.lin_i.forward(ops, x, rows, &mut cache.s_i);

        // p[c] = ln σ(Λ[c]) — constante no tempo, calculado uma vez.
        let mut p = vec![T::ZERO; h];
        for c in 0..h {
            p[c] = self.lambda[c].ln_sigmoid();
        }

        let cd = T::from_f64(C_DECAY);
        let eps_n = T::from_f64(EPS_N);

        // ---- Dentro do laço temporal: só elementwise. ----
        for t in 0..seq {
            for b in 0..batch {
                let base = (t * batch + b) * h;
                let prev = if t == 0 {
                    b * h
                } else {
                    ((t - 1) * batch + b) * h
                };

                for c in 0..h {
                    let k = base + c;
                    let r = cache.s_r[k].sigmoid();
                    let i = cache.s_i[k].sigmoid();
                    let a = (cd * r * p[c]).exp();
                    let sq = T::ONE - a * a;
                    let n = sq.max(eps_n).sqrt();

                    let h_prev = if t == 0 { h0[prev + c] } else { y[prev + c] };
                    y[k] = a * h_prev + n * (i * x[k]);

                    cache.r[k] = r;
                    cache.i[k] = i;
                    cache.a[k] = a;
                    cache.n[k] = n;
                }
            }
        }
    }

    /// BPTT. `y` é a saída do forward (serve de `h_{t−1}` para `t > 0`).
    ///
    /// `dh_last`: gradiente que chega no estado final, além do que vem por `dy`.
    /// Usado quando a sequência é um pedaço de uma sequência maior.
    /// `dh0`: onde escrever o gradiente em relação ao estado inicial.
    #[allow(clippy::too_many_arguments)]
    pub fn backward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        y: &[T],
        h0: &[T],
        dy: &[T],
        dh_last: Option<&[T]>,
        seq: usize,
        batch: usize,
        cache: &mut RgLruCache<T>,
        dx: &mut [T],
        accum_dx: bool,
        mut dh0: Option<&mut [T]>,
        grad: &mut RgLruGrad<T>,
    ) {
        let h = self.h;
        let rows = seq * batch;
        debug_assert_eq!(dx.len(), rows * h);
        debug_assert_eq!(dy.len(), rows * h);

        if !accum_dx {
            dx.fill(T::ZERO);
        }

        let mut p = vec![T::ZERO; h];
        let mut dp_lam = vec![T::ZERO; h];
        for c in 0..h {
            p[c] = self.lambda[c].ln_sigmoid();
            dp_lam[c] = self.lambda[c].d_ln_sigmoid();
        }

        let cd = T::from_f64(C_DECAY);
        let eps_n = T::from_f64(EPS_N);

        // Gradiente que desce de t+1 para t.
        let mut dh_carry = match dh_last {
            Some(v) => v.to_vec(),
            None => vec![T::ZERO; batch * h],
        };

        for t in (0..seq).rev() {
            for b in 0..batch {
                let base = (t * batch + b) * h;
                let prev = if t == 0 {
                    b * h
                } else {
                    ((t - 1) * batch + b) * h
                };

                for c in 0..h {
                    let k = base + c;
                    let a = cache.a[k];
                    let n = cache.n[k];
                    let r = cache.r[k];
                    let i = cache.i[k];
                    let xv = x[k];
                    let u = i * xv;

                    let dh = dy[k] + dh_carry[b * h + c];
                    let h_prev = if t == 0 { h0[prev + c] } else { y[prev + c] };

                    // Caminho pelo termo de entrada: h = ... + n·(i·x)
                    let du = dh * n; // ∂L/∂u
                    dx[k] += du * i;
                    let di = du * xv;
                    cache.s_i[k] = di * i * (T::ONE - i);

                    // Caminho pelo decaimento: h = a·h_prev + n(a)·u
                    let mut da = dh * h_prev;
                    let sq = T::ONE - a * a;
                    if sq > eps_n {
                        // n = sqrt(1−a²)  →  ∂n/∂a = −a/n, e ∂L/∂n = dh·u
                        da += (dh * u) * (-(a / n));
                    }
                    // na região travada n é constante, logo dn/da = 0

                    let dla = da * a; // a = exp(la)
                    cache.s_r[k] = (dla * cd * p[c]) * r * (T::ONE - r);
                    grad.dlambda[c] += (dla * cd * r) * dp_lam[c];

                    dh_carry[b * h + c] = dh * a;
                }
            }
        }

        if let Some(d) = &mut dh0 {
            d.copy_from_slice(&dh_carry);
        }

        // ---- Fora do laço: os dois GEMMs do gradiente, sobre a sequência inteira.
        self.lin_r
            .backward(ops, x, &cache.s_r, rows, Some(dx), true, &mut grad.g_r);
        self.lin_i
            .backward(ops, x, &cache.s_i, rows, Some(dx), true, &mut grad.g_i);
    }
}
