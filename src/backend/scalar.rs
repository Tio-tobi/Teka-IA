//! Implementação escalar de referência — o **oráculo**.
//!
//! Sem SIMD explícito, sem blocking de cache, sem packing. O objetivo aqui não é
//! velocidade: é ser tão óbvia que dá pra ler e concluir que está certa. Toda
//! implementação rápida futura é validada contra esta.
//!
//! Os laços ainda assim são ordenados pra não serem gratuitamente ruins: `nn` e `tn`
//! usam ordem `i-k-j`, que mantém a linha de saída e a linha de B contíguas no laço
//! interno e deixa o autovetorizador do LLVM fazer algo útil. `nt` é produto interno
//! puro, com as duas linhas contíguas.

use super::Ops;
use crate::num::Float;

#[derive(Clone, Copy, Debug, Default)]
pub struct Scalar;

impl<T: Float> Ops<T> for Scalar {
    fn name(&self) -> &'static str {
        "scalar"
    }

    fn gemm_nn(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool) {
        debug_assert_eq!(a.len(), m * k);
        debug_assert_eq!(b.len(), k * n);
        debug_assert_eq!(c.len(), m * n);

        if !accumulate {
            c.fill(T::ZERO);
        }
        for i in 0..m {
            let crow = &mut c[i * n..(i + 1) * n];
            for p in 0..k {
                let aip = a[i * k + p];
                if aip == T::ZERO {
                    continue;
                }
                let brow = &b[p * n..(p + 1) * n];
                for j in 0..n {
                    crow[j] += aip * brow[j];
                }
            }
        }
    }

    fn gemm_nt(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool) {
        debug_assert_eq!(a.len(), m * k);
        debug_assert_eq!(b.len(), n * k);
        debug_assert_eq!(c.len(), m * n);

        // Esta é a única variante cujo laço interno é uma REDUÇÃO, e isso tem uma
        // consequência que custou um benchmark absurdo pra achar: soma de ponto
        // flutuante não é associativa, então o compilador não tem permissão de
        // vetorizar `acc += a[p]*b[p]` — cada FMA espera a latência da anterior.
        // Com um acumulador só, esta função rodava ~5x mais devagar que as outras
        // duas (que são `axpy` e vetorizam sozinhas), e o forward de uma camada
        // ficava mais lento que o backward inteiro.
        //
        // Quatro acumuladores independentes quebram a cadeia de dependência. A
        // ordem de soma muda — mas é uma escolha explícita, e somar em blocos é
        // numericamente MELHOR que somar em fila, não pior.
        const U: usize = 4;
        for i in 0..m {
            let arow = &a[i * k..(i + 1) * k];
            for j in 0..n {
                let brow = &b[j * k..(j + 1) * k];
                let mut acc = [T::ZERO; U];
                let corte = k - k % U;
                let mut p = 0;
                while p < corte {
                    for (u, item) in acc.iter_mut().enumerate() {
                        *item += arow[p + u] * brow[p + u];
                    }
                    p += U;
                }
                let mut total = (acc[0] + acc[1]) + (acc[2] + acc[3]);
                while p < k {
                    total += arow[p] * brow[p];
                    p += 1;
                }
                let slot = &mut c[i * n + j];
                *slot = if accumulate { *slot + total } else { total };
            }
        }
    }

    fn gemm_tn(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool) {
        debug_assert_eq!(a.len(), k * m);
        debug_assert_eq!(b.len(), k * n);
        debug_assert_eq!(c.len(), m * n);

        if !accumulate {
            c.fill(T::ZERO);
        }
        for p in 0..k {
            let arow = &a[p * m..(p + 1) * m];
            let brow = &b[p * n..(p + 1) * n];
            for i in 0..m {
                let api = arow[i];
                if api == T::ZERO {
                    continue;
                }
                let crow = &mut c[i * n..(i + 1) * n];
                for j in 0..n {
                    crow[j] += api * brow[j];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    /// Referência ingênua, escrita direto da definição matemática. Existe só para
    /// os testes: se ela e o `Scalar` divergirem, um dos dois está errado.
    fn ref_nn(m: usize, k: usize, n: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
        let mut c = vec![0.0; m * n];
        for i in 0..m {
            for j in 0..n {
                let mut s = 0.0;
                for p in 0..k {
                    s += a[i * k + p] * b[p * n + j];
                }
                c[i * n + j] = s;
            }
        }
        c
    }

    fn transpose(x: &[f64], rows: usize, cols: usize) -> Vec<f64> {
        let mut t = vec![0.0; rows * cols];
        for i in 0..rows {
            for j in 0..cols {
                t[j * rows + i] = x[i * cols + j];
            }
        }
        t
    }

    fn rand_vec(rng: &mut Rng, n: usize) -> Vec<f64> {
        (0..n).map(|_| rng.uniform(-1.0, 1.0)).collect()
    }

    #[test]
    fn gemm_nn_bate_com_a_definicao() {
        let mut rng = Rng::new(1);
        for &(m, k, n) in &[(1, 1, 1), (3, 4, 5), (8, 8, 8), (7, 2, 9)] {
            let a = rand_vec(&mut rng, m * k);
            let b = rand_vec(&mut rng, k * n);
            let mut c = vec![0.0; m * n];
            Scalar.gemm_nn(m, k, n, &a, &b, &mut c, false);
            let esperado = ref_nn(m, k, n, &a, &b);
            for i in 0..m * n {
                assert!((c[i] - esperado[i]).abs() < 1e-12, "m={m} k={k} n={n} i={i}");
            }
        }
    }

    #[test]
    fn gemm_nt_equivale_a_nn_com_b_transposto() {
        let mut rng = Rng::new(2);
        let (m, k, n) = (5, 3, 6);
        let a = rand_vec(&mut rng, m * k);
        let bt = rand_vec(&mut rng, n * k); // [n,k]
        let b = transpose(&bt, n, k); // [k,n]

        let mut c = vec![0.0; m * n];
        Scalar.gemm_nt(m, k, n, &a, &bt, &mut c, false);
        let esperado = ref_nn(m, k, n, &a, &b);
        for i in 0..m * n {
            assert!((c[i] - esperado[i]).abs() < 1e-12, "i={i}");
        }
    }

    #[test]
    fn gemm_tn_equivale_a_nn_com_a_transposto() {
        let mut rng = Rng::new(3);
        let (m, k, n) = (4, 7, 3);
        let at = rand_vec(&mut rng, k * m); // [k,m]
        let a = transpose(&at, k, m); // [m,k]
        let b = rand_vec(&mut rng, k * n);

        let mut c = vec![0.0; m * n];
        Scalar.gemm_tn(m, k, n, &at, &b, &mut c, false);
        let esperado = ref_nn(m, k, n, &a, &b);
        for i in 0..m * n {
            assert!((c[i] - esperado[i]).abs() < 1e-12, "i={i}");
        }
    }

    #[test]
    fn accumulate_soma_em_vez_de_sobrescrever() {
        let mut rng = Rng::new(4);
        let (m, k, n) = (3, 3, 3);
        let a = rand_vec(&mut rng, m * k);
        let b = rand_vec(&mut rng, k * n);

        let mut uma = vec![0.0; m * n];
        Scalar.gemm_nn(m, k, n, &a, &b, &mut uma, false);

        let mut duas = uma.clone();
        Scalar.gemm_nn(m, k, n, &a, &b, &mut duas, true);

        for i in 0..m * n {
            assert!((duas[i] - 2.0 * uma[i]).abs() < 1e-12);
        }

        // Idem para as outras duas variantes.
        let mut c1 = vec![0.0; m * n];
        Scalar.gemm_nt(m, k, n, &a, &b, &mut c1, false);
        let mut c2 = c1.clone();
        Scalar.gemm_nt(m, k, n, &a, &b, &mut c2, true);
        for i in 0..m * n {
            assert!((c2[i] - 2.0 * c1[i]).abs() < 1e-12);
        }

        let mut d1 = vec![0.0; m * n];
        Scalar.gemm_tn(m, k, n, &a, &b, &mut d1, false);
        let mut d2 = d1.clone();
        Scalar.gemm_tn(m, k, n, &a, &b, &mut d2, true);
        for i in 0..m * n {
            assert!((d2[i] - 2.0 * d1[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn funciona_em_f32_tambem() {
        let a: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        let b: Vec<f32> = vec![5.0, 6.0, 7.0, 8.0];
        let mut c = vec![0.0f32; 4];
        Scalar.gemm_nn(2, 2, 2, &a, &b, &mut c, false);
        assert_eq!(c, vec![19.0, 22.0, 43.0, 50.0]);
    }
}
