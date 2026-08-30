//! Backend paralelo — o mesmo `Scalar`, espalhado pelos 6 cores.
//!
//! Prova de que a fronteira do §9 da arquitetura funciona: nem o modelo nem o
//! treino sabem que isto existe. Trocar `Scalar` por `Paralelo` é um parâmetro.
//!
//! ## Como cada variante é fatiada
//!
//! `nn` e `nt` produzem `C[m,n]` percorrendo `A` linha por linha — fatiar por linhas
//! de saída dá blocos disjuntos e contíguos, e cada thread só lê `B` inteiro. Direto.
//!
//! `tn` é o caso chato: a contração é sobre `k` (o eixo do lote), então fatiar por
//! `k` exigiria um buffer parcial por thread e uma redução no fim — alocação e
//! tráfego de memória que anulariam boa parte do ganho. Em vez disso, fatia-se por
//! linhas de saída também, com cada thread lendo uma faixa **strided** de `A`. Custa
//! um kernel próprio de 15 linhas e não custa memória nenhuma.

use super::{Ops, Scalar};
use crate::num::Float;

#[derive(Clone, Copy, Debug)]
pub struct Paralelo {
    pub threads: usize,
}

impl Paralelo {
    pub fn new(threads: usize) -> Self {
        Self {
            threads: threads.max(1),
        }
    }

    /// Usa todos os cores lógicos disponíveis.
    pub fn auto() -> Self {
        Self::new(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        )
    }

    /// Abaixo de um certo tamanho o custo de criar threads domina.
    fn fatias(&self, m: usize) -> usize {
        if m < 64 {
            1
        } else {
            self.threads.min(m)
        }
    }
}

impl<T: Float> Ops<T> for Paralelo {
    fn name(&self) -> &'static str {
        "paralelo"
    }

    fn gemm_nn(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool) {
        let nt = self.fatias(m);
        if nt <= 1 {
            return Scalar.gemm_nn(m, k, n, a, b, c, accumulate);
        }
        let chunk = m.div_ceil(nt);
        std::thread::scope(|s| {
            for (idx, cpart) in c.chunks_mut(chunk * n).enumerate() {
                let linhas = cpart.len() / n;
                let apart = &a[idx * chunk * k..idx * chunk * k + linhas * k];
                s.spawn(move || Scalar.gemm_nn(linhas, k, n, apart, b, cpart, accumulate));
            }
        });
    }

    fn gemm_nt(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool) {
        let nt = self.fatias(m);
        if nt <= 1 {
            return Scalar.gemm_nt(m, k, n, a, b, c, accumulate);
        }
        let chunk = m.div_ceil(nt);
        std::thread::scope(|s| {
            for (idx, cpart) in c.chunks_mut(chunk * n).enumerate() {
                let linhas = cpart.len() / n;
                let apart = &a[idx * chunk * k..idx * chunk * k + linhas * k];
                s.spawn(move || Scalar.gemm_nt(linhas, k, n, apart, b, cpart, accumulate));
            }
        });
    }

    fn gemm_tn(&self, m: usize, k: usize, n: usize, a: &[T], b: &[T], c: &mut [T], accumulate: bool) {
        let nt = self.fatias(m);
        if nt <= 1 {
            return Scalar.gemm_tn(m, k, n, a, b, c, accumulate);
        }
        let chunk = m.div_ceil(nt);
        std::thread::scope(|s| {
            for (idx, cpart) in c.chunks_mut(chunk * n).enumerate() {
                let m0 = idx * chunk;
                let mm = cpart.len() / n;
                s.spawn(move || {
                    if !accumulate {
                        cpart.fill(T::ZERO);
                    }
                    // `A` é [k, m]: a faixa desta thread é a coluna m0..m0+mm de
                    // cada uma das k linhas. Strided, mas cada linha lida é
                    // contígua dentro de si.
                    for p in 0..k {
                        let afaixa = &a[p * m + m0..p * m + m0 + mm];
                        let brow = &b[p * n..(p + 1) * n];
                        for i in 0..mm {
                            let v = afaixa[i];
                            if v == T::ZERO {
                                continue;
                            }
                            let crow = &mut cpart[i * n..(i + 1) * n];
                            for j in 0..n {
                                crow[j] += v * brow[j];
                            }
                        }
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    /// O contrato do backend: o paralelo tem que dar exatamente o mesmo que o
    /// oráculo. "Exatamente" é literal — as três variantes fatiam por linhas de
    /// saída, então nenhuma soma muda de ordem.
    #[test]
    fn paralelo_bate_com_o_oraculo() {
        let mut rng = Rng::new(99);
        let par = Paralelo::new(6);

        for &(m, k, n) in &[(256usize, 64usize, 96usize), (300, 33, 17), (64, 8, 8)] {
            let mut a = vec![0.0f64; m * k.max(n)];
            let mut b = vec![0.0f64; k * n.max(m)];
            rng.fill_normal(&mut a, 1.0);
            rng.fill_normal(&mut b, 1.0);

            for acc in [false, true] {
                let mut c1 = vec![0.5f64; m * n];
                let mut c2 = c1.clone();
                Scalar.gemm_nn(m, k, n, &a[..m * k], &b[..k * n], &mut c1, acc);
                par.gemm_nn(m, k, n, &a[..m * k], &b[..k * n], &mut c2, acc);
                assert_eq!(c1, c2, "gemm_nn m={m} k={k} n={n} acc={acc}");

                let mut c1 = vec![0.5f64; m * n];
                let mut c2 = c1.clone();
                Scalar.gemm_nt(m, k, n, &a[..m * k], &b[..n * k], &mut c1, acc);
                par.gemm_nt(m, k, n, &a[..m * k], &b[..n * k], &mut c2, acc);
                assert_eq!(c1, c2, "gemm_nt m={m} k={k} n={n} acc={acc}");

                let mut c1 = vec![0.5f64; m * n];
                let mut c2 = c1.clone();
                Scalar.gemm_tn(m, k, n, &a[..k * m], &b[..k * n], &mut c1, acc);
                par.gemm_tn(m, k, n, &a[..k * m], &b[..k * n], &mut c2, acc);
                assert_eq!(c1, c2, "gemm_tn m={m} k={k} n={n} acc={acc}");
            }
        }
    }
}
