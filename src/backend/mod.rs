//! Backend numérico — a fronteira que torna a Teka evolucional.
//!
//! Toda a matemática pesada passa por três variantes de GEMM. Nada mais. Isso é
//! deliberado: com essa fronteira, trocar a implementação
//!
//! ```text
//! escalar  →  AVX2 à mão  →  int8 quantizado  →  GPU, um dia
//! ```
//!
//! não toca em uma linha do modelo, do agente ou da cognição.
//!
//! `Scalar` é o **oráculo**: lenta, óbvia, e a referência de correção contra a qual
//! toda implementação rápida é validada. Ela nunca é removida. Mesmo espírito da
//! checagem de gradiente por diferenças finitas — a verdade precisa de uma âncora
//! que não dependa de otimização nenhuma.
//!
//! ## Convenção
//!
//! Tudo é *row-major*. As três variantes cobrem exatamente o que uma camada linear
//! precisa (forward + os dois gradientes), e os nomes seguem a convenção do BLAS:
//! a letra indica se o operando entra transposto.
//!
//! | variante | A | B | C |
//! |---|---|---|---|
//! | `gemm_nn` | `[m,k]` | `[k,n]` | `[m,n] = A·B` |
//! | `gemm_nt` | `[m,k]` | `[n,k]` | `[m,n] = A·Bᵀ` |
//! | `gemm_tn` | `[k,m]` | `[k,n]` | `[m,n] = Aᵀ·B` |
//!
//! `accumulate = true` faz `C += ...`; `false` sobrescreve.

use crate::num::Float;

pub mod parallel;
pub mod scalar;

pub use parallel::Paralelo;
pub use scalar::Scalar;

pub trait Ops<T: Float>: Copy + Send + Sync {
    /// Nome da implementação, pra aparecer em benchmark e log.
    fn name(&self) -> &'static str;

    /// `C[m,n] = A[m,k] · B[k,n]`
    fn gemm_nn(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool);

    /// `C[m,n] = A[m,k] · B[n,k]ᵀ` — o forward de uma camada linear, onde os pesos
    /// são guardados como `[saída, entrada]`.
    fn gemm_nt(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool);

    /// `C[m,n] = A[k,m]ᵀ · B[k,n]` — o gradiente dos pesos, onde a contração é
    /// sobre o eixo do lote.
    fn gemm_tn(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool);
}
