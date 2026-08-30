//! Pilha de camadas: `L × (BlocoRec → BlocoMlp)`.
//!
//! É o corpo genérico usado pelos três níveis da hierarquia — encoder local,
//! backbone e decoder local só diferem em largura e profundidade.
//!
//! ## Estado e BPTT truncado
//!
//! Cada `BlocoRec` carrega seu próprio estado recorrente entre janelas. O gradiente,
//! porém, **não** atravessa a fronteira da janela: `estado_final` é lido e passado
//! adiante como valor, sem histórico. É o BPTT truncado clássico, e é o que torna
//! possível treinar um fluxo contínuo com memória limitada.
//!
//! Na prática isso significa que a Teka aprende dependências de até uma janela de
//! comprimento, mas *carrega* memória indefinidamente — que é exatamente a
//! separação entre "pesos" e "estado" que a arquitetura descreve.

use crate::backend::Ops;
use crate::nn::mlp::{BlocoMlp, BlocoMlpCache, BlocoMlpGrad};
use crate::num::Float;
use crate::rng::Rng;
use crate::ssm::block::{BlocoRec, BlocoRecCache, BlocoRecGrad};

#[derive(Clone, Debug)]
pub struct Pilha<T: Float> {
    pub d: usize,
    pub camadas: Vec<(BlocoRec<T>, BlocoMlp<T>)>,
}

#[derive(Clone, Debug)]
pub struct PilhaGrad<T: Float> {
    pub camadas: Vec<(BlocoRecGrad<T>, BlocoMlpGrad<T>)>,
}

impl<T: Float> PilhaGrad<T> {
    pub fn clear(&mut self) {
        for (r, m) in self.camadas.iter_mut() {
            r.clear();
            m.clear();
        }
    }
    pub fn slices(&self) -> Vec<&[T]> {
        let mut v = Vec::new();
        for (r, m) in &self.camadas {
            v.extend(r.slices());
            v.extend(m.slices());
        }
        v
    }
}

#[derive(Clone, Debug, Default)]
pub struct PilhaCache<T: Float> {
    /// `2·L` buffers de `rows·d`, contíguos: saída do rec e do mlp de cada camada.
    flat: Vec<T>,
    dflat: Vec<T>,
    rec: Vec<BlocoRecCache<T>>,
    mlp: Vec<BlocoMlpCache<T>>,
}

impl<T: Float> PilhaCache<T> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: Float> Pilha<T> {
    pub fn new(d: usize, h: usize, f: usize, n_camadas: usize, rng: &mut Rng) -> Self {
        let camadas = (0..n_camadas)
            .map(|_| (BlocoRec::new(d, h, rng), BlocoMlp::new(d, f, rng)))
            .collect();
        Self { d, camadas }
    }

    pub fn grad(&self) -> PilhaGrad<T> {
        PilhaGrad {
            camadas: self.camadas.iter().map(|(r, m)| (r.grad(), m.grad())).collect(),
        }
    }

    pub fn n_params(&self) -> usize {
        self.camadas.iter().map(|(r, m)| r.n_params() + m.n_params()).sum()
    }

    pub fn n_camadas(&self) -> usize {
        self.camadas.len()
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        let mut v = Vec::new();
        for (r, m) in self.camadas.iter_mut() {
            v.extend(r.params_mut());
            v.extend(m.params_mut());
        }
        v
    }

    pub fn descritores(&self, p: &str) -> Vec<(String, Vec<usize>)> {
        let mut v = Vec::new();
        for (i, (r, m)) in self.camadas.iter().enumerate() {
            v.extend(r.descritores(&format!("{p}.{i}.ssm")));
            v.extend(m.descritores(&format!("{p}.{i}.mlp")));
        }
        v
    }

    pub fn zero_states(&self, batch: usize) -> Vec<Vec<T>> {
        self.camadas.iter().map(|(r, _)| r.zero_state(batch)).collect()
    }

    fn preparar(&self, cache: &mut PilhaCache<T>, rows: usize) {
        let l = self.camadas.len();
        cache.flat.resize(2 * l * rows * self.d, T::ZERO);
        cache.dflat.resize(2 * l * rows * self.d, T::ZERO);
        while cache.rec.len() < l {
            cache.rec.push(BlocoRecCache::new());
            cache.mlp.push(BlocoMlpCache::new());
        }
    }

    pub fn forward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        estados: &[Vec<T>],
        seq: usize,
        batch: usize,
        out: &mut [T],
        cache: &mut PilhaCache<T>,
    ) {
        let rows = seq * batch;
        let passo = rows * self.d;
        self.preparar(cache, rows);

        for i in 0..self.camadas.len() {
            let (esq, dir) = cache.flat.split_at_mut(2 * i * passo);
            let entrada: &[T] = if i == 0 {
                x
            } else {
                &esq[(2 * i - 1) * passo..2 * i * passo]
            };
            let (saida_rec, resto) = dir.split_at_mut(passo);
            self.camadas[i].0.forward(
                ops,
                entrada,
                &estados[i],
                seq,
                batch,
                saida_rec,
                &mut cache.rec[i],
            );
            let (saida_mlp, _) = resto.split_at_mut(passo);
            self.camadas[i]
                .1
                .forward(ops, saida_rec, rows, saida_mlp, &mut cache.mlp[i]);
        }

        let l = self.camadas.len();
        out.copy_from_slice(&cache.flat[(2 * l - 1) * passo..2 * l * passo]);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn backward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        estados: &[Vec<T>],
        dout: &[T],
        seq: usize,
        batch: usize,
        cache: &mut PilhaCache<T>,
        dx: &mut [T],
        accum_dx: bool,
        grad: &mut PilhaGrad<T>,
    ) {
        let rows = seq * batch;
        let passo = rows * self.d;
        let l = self.camadas.len();

        // dflat[2i]   = gradiente na saída do rec da camada i
        // dflat[2i+1] = gradiente na saída do mlp da camada i
        cache.dflat[(2 * l - 1) * passo..2 * l * passo].copy_from_slice(dout);

        for i in (0..l).rev() {
            // Passo 1 — MLP: dout = dflat[2i+1], escreve em dflat[2i].
            // As duas faixas são do mesmo vetor, então `split_at_mut` separa a
            // leitura da escrita sem cópia.
            {
                let saida_rec: &[T] = &cache.flat[2 * i * passo..(2 * i + 1) * passo];
                let (desq, ddir) = cache.dflat.split_at_mut((2 * i + 1) * passo);
                let d_saida_mlp: &[T] = &ddir[..passo];
                let alvo = &mut desq[2 * i * passo..];
                self.camadas[i].1.backward(
                    ops,
                    saida_rec,
                    d_saida_mlp,
                    rows,
                    &mut cache.mlp[i],
                    alvo,
                    false,
                    &mut grad.camadas[i].1,
                );
            }

            // Passo 2 — recorrente: dout = dflat[2i], escreve em dflat[2i-1] (ou em
            // `dx`, se for a primeira camada).
            if i == 0 {
                let d_saida_rec: &[T] = &cache.dflat[0..passo];
                self.camadas[0].0.backward(
                    ops,
                    x,
                    &estados[0],
                    d_saida_rec,
                    None,
                    seq,
                    batch,
                    &mut cache.rec[0],
                    dx,
                    accum_dx,
                    None,
                    &mut grad.camadas[0].0,
                );
            } else {
                let entrada_rec: &[T] = &cache.flat[(2 * i - 1) * passo..2 * i * passo];
                let (desq, ddir) = cache.dflat.split_at_mut(2 * i * passo);
                let d_saida_rec: &[T] = &ddir[..passo];
                let alvo = &mut desq[(2 * i - 1) * passo..];
                self.camadas[i].0.backward(
                    ops,
                    entrada_rec,
                    &estados[i],
                    d_saida_rec,
                    None,
                    seq,
                    batch,
                    &mut cache.rec[i],
                    alvo,
                    false,
                    None,
                    &mut grad.camadas[i].0,
                );
            }
        }
    }

    /// Estado de cada camada ao final da janela, pra alimentar a janela seguinte.
    pub fn estados_finais(&self, cache: &PilhaCache<T>, seq: usize, batch: usize) -> Vec<Vec<T>> {
        self.camadas
            .iter()
            .enumerate()
            .map(|(i, (r, _))| cache.rec[i].estado_final(seq, batch, r.h))
            .collect()
    }
}
