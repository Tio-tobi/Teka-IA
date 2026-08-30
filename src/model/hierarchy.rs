//! O modelo hierárquico completo.
//!
//! ```text
//! bytes ─▶ [emb] ─▶ [ENCODER local] ──┬─────────────────────────────┐
//!                                     │ pool no fim de cada patch   │
//!                                     ▼                             │
//!                            [proj] ▶ [BACKBONE por patch] ▶ [proj] │
//!                                                              │    │
//!                                                   broadcast   ▼    ▼
//!                                                          (+) ──▶ [DECODER local]
//!                                                                     │
//!                                                                     ▼
//!                                                              [norm] ▶ [head] ▶ 256 logits
//! ```
//!
//! ## Onde está o ganho
//!
//! O backbone — que tem ~85% dos parâmetros — roda **uma vez por patch**, não por
//! byte. Com patches de ~4 bytes ele roda 4x menos. É isso que torna um modelo
//! byte-level viável numa CPU sem GPU.
//!
//! ## Causalidade
//!
//! O ponto delicado da hierarquia. Ao prever o byte `t+1`, o decoder pode usar:
//! - o estado do encoder em `t` (resume os bytes `0..=t`), e
//! - a saída do backbone do último patch **completo** em `t`.
//!
//! Quem garante o segundo é o campo `ctx` do [`Plano`]: dentro de um patch ainda em
//! curso, o contexto disponível é o patch *anterior*. Sem isso o modelo veria o
//! futuro dentro do próprio patch e o bits/byte ficaria lindo e mentiroso.

use crate::backend::Ops;
use crate::model::patcher::{Plano, SEM_PATCH};
use crate::model::stack::{Pilha, PilhaCache, PilhaGrad};
use crate::nn::embed::{Embed, EmbedGrad, VOCAB};
use crate::nn::linear::{Linear, LinearGrad};
use crate::nn::rmsnorm::{RmsNorm, RmsNormCache, RmsNormGrad};
use crate::num::Float;
use crate::rng::Rng;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub d_loc: usize,
    pub h_loc: usize,
    pub f_loc: usize,
    pub n_enc: usize,
    pub n_dec: usize,
    pub d_bb: usize,
    pub h_bb: usize,
    pub f_bb: usize,
    pub n_bb: usize,
}

impl Config {
    /// Alvo da fase 1: ~11,6M parâmetros, ~46 MB em f32.
    pub fn padrao() -> Self {
        Self {
            d_loc: 192,
            h_loc: 192,
            f_loc: 384,
            n_enc: 2,
            n_dec: 2,
            d_bb: 384,
            h_bb: 384,
            f_bb: 768,
            n_bb: 6,
        }
    }

    /// Para iterar rápido em CPU enquanto se depura o treino.
    pub fn pequeno() -> Self {
        Self {
            d_loc: 96,
            h_loc: 96,
            f_loc: 192,
            n_enc: 1,
            n_dec: 1,
            d_bb: 192,
            h_bb: 192,
            f_bb: 384,
            n_bb: 3,
        }
    }

    /// Minúsculo, só para a checagem de gradiente (que é O(nº de parâmetros)).
    pub fn minusculo() -> Self {
        Self {
            d_loc: 4,
            h_loc: 4,
            f_loc: 6,
            n_enc: 1,
            n_dec: 1,
            d_bb: 5,
            h_bb: 5,
            f_bb: 7,
            n_bb: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Teka<T: Float> {
    pub cfg: Config,
    pub emb: Embed<T>,
    pub enc: Pilha<T>,
    pub proj_pool: Linear<T>,
    pub bb: Pilha<T>,
    pub proj_ctx: Linear<T>,
    pub dec: Pilha<T>,
    pub norm_f: RmsNorm<T>,
    pub head: Linear<T>,
}

#[derive(Clone, Debug)]
pub struct TekaGrad<T: Float> {
    pub emb: EmbedGrad<T>,
    pub enc: PilhaGrad<T>,
    pub proj_pool: LinearGrad<T>,
    pub bb: PilhaGrad<T>,
    pub proj_ctx: LinearGrad<T>,
    pub dec: PilhaGrad<T>,
    pub norm_f: RmsNormGrad<T>,
    pub head: LinearGrad<T>,
}

impl<T: Float> TekaGrad<T> {
    pub fn clear(&mut self) {
        self.emb.clear();
        self.enc.clear();
        self.proj_pool.clear();
        self.bb.clear();
        self.proj_ctx.clear();
        self.dec.clear();
        self.norm_f.clear();
        self.head.clear();
    }

    /// **Mesma ordem** de `Teka::params_mut`. O teste `checar_modulo` prova isso.
    pub fn slices(&self) -> Vec<&[T]> {
        let mut v = self.emb.slices();
        v.extend(self.enc.slices());
        v.push(&self.proj_pool.dw[..]);
        v.push(&self.proj_pool.db[..]);
        v.extend(self.bb.slices());
        v.push(&self.proj_ctx.dw[..]);
        v.push(&self.proj_ctx.db[..]);
        v.extend(self.dec.slices());
        v.push(&self.norm_f.dg[..]);
        v.push(&self.head.dw[..]);
        v.push(&self.head.db[..]);
        v
    }
}

/// Estado recorrente vivo: três relógios diferentes.
///
/// `enc` e `dec` avançam por byte; `bb` avança por patch. Persistem entre janelas —
/// é a memória de trabalho da Teka.
#[derive(Clone, Debug)]
pub struct Estado<T: Float> {
    pub enc: Vec<Vec<T>>,
    pub bb: Vec<Vec<T>>,
    pub dec: Vec<Vec<T>>,
}

impl<T: Float> Estado<T> {
    /// Zera o estado de uma única lane do lote — usado quando o cursor daquela lane
    /// pula para outro ponto do corpus e a continuidade se quebra.
    pub fn zerar_lane(&mut self, b: usize, batch: usize) {
        for v in self
            .enc
            .iter_mut()
            .chain(self.bb.iter_mut())
            .chain(self.dec.iter_mut())
        {
            let h = v.len() / batch;
            v[b * h..(b + 1) * h].fill(T::ZERO);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TekaCache<T: Float> {
    pub enc: PilhaCache<T>,
    pub bb: PilhaCache<T>,
    pub dec: PilhaCache<T>,
    pub norm_f: RmsNormCache<T>,

    e0: Vec<T>,
    e: Vec<T>,
    pool_in: Vec<T>,
    bb_in: Vec<T>,
    z: Vec<T>,
    zc: Vec<T>,
    ctx_b: Vec<T>,
    dec_in: Vec<T>,
    hdec: Vec<T>,
    hn: Vec<T>,
    pub logits: Vec<T>,

    dlogits: Vec<T>,
    dhn: Vec<T>,
    dhdec: Vec<T>,
    ddec_in: Vec<T>,
    dzc: Vec<T>,
    dz: Vec<T>,
    dbb_in: Vec<T>,
    dpool_in: Vec<T>,
    de: Vec<T>,
    de0: Vec<T>,
}

impl<T: Float> TekaCache<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Saída do backbone, `[p_max·batch, d_bb]` — a entrada das cabeças de decisão.
    pub fn z(&self) -> &[T] {
        &self.z
    }

    /// `z` e `dz` de uma vez. Devolver os dois juntos é necessário: são campos
    /// distintos, mas o compilador não sabe disso através de dois métodos.
    pub fn z_e_dz(&mut self) -> (&[T], &mut [T]) {
        (&self.z, &mut self.dz)
    }

    /// Zera a contribuição externa ao gradiente do encoder. Quem usa o tronco sem
    /// o decoder (as cabeças de decisão) precisa chamar isto antes do backward.
    pub fn zerar_de(&mut self) {
        self.de.fill(T::ZERO);
    }

    /// Saída do encoder local, `[seq·batch, d_loc]` — a entrada do nível fino do
    /// ponteiro.
    pub fn e(&self) -> &[T] {
        &self.e
    }

    /// Os quatro buffers que as cabeças de decisão precisam, de uma vez: elas leem
    /// `z` e `e` e escrevem em `dz` e `de`. São campos distintos, mas o compilador
    /// só sabe disso se saírem juntos de um único empréstimo.
    pub fn portas(&mut self) -> (&[T], &[T], &mut [T], &mut [T]) {
        (&self.z, &self.e, &mut self.dz, &mut self.de)
    }

    /// Zera os gradientes que as cabeças acumulam.
    pub fn zerar_portas(&mut self) {
        self.dz.fill(T::ZERO);
        self.de.fill(T::ZERO);
    }
}

impl<T: Float> Teka<T> {
    pub fn new(cfg: Config, rng: &mut Rng) -> Self {
        Self {
            emb: Embed::new(cfg.d_loc, rng),
            enc: Pilha::new(cfg.d_loc, cfg.h_loc, cfg.f_loc, cfg.n_enc, rng),
            proj_pool: Linear::new(cfg.d_loc, cfg.d_bb, rng),
            bb: Pilha::new(cfg.d_bb, cfg.h_bb, cfg.f_bb, cfg.n_bb, rng),
            proj_ctx: Linear::new(cfg.d_bb, cfg.d_loc, rng),
            dec: Pilha::new(cfg.d_loc, cfg.h_loc, cfg.f_loc, cfg.n_dec, rng),
            norm_f: RmsNorm::new(cfg.d_loc),
            head: Linear::nova_com_escala(cfg.d_loc, VOCAB, 0.05, rng),
            cfg,
        }
    }

    pub fn n_params(&self) -> usize {
        self.emb.n_params()
            + self.enc.n_params()
            + self.proj_pool.n_params()
            + self.bb.n_params()
            + self.proj_ctx.n_params()
            + self.dec.n_params()
            + self.norm_f.n_params()
            + self.head.n_params()
    }

    /// Fração dos parâmetros que mora no backbone — ou seja, a fração que só é lida
    /// uma vez por patch em vez de uma vez por byte.
    pub fn fracao_backbone(&self) -> f64 {
        self.bb.n_params() as f64 / self.n_params() as f64
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        let mut v = self.emb.params_mut();
        v.extend(self.enc.params_mut());
        v.push(&mut self.proj_pool.w[..]);
        v.push(&mut self.proj_pool.b[..]);
        v.extend(self.bb.params_mut());
        v.push(&mut self.proj_ctx.w[..]);
        v.push(&mut self.proj_ctx.b[..]);
        v.extend(self.dec.params_mut());
        v.push(&mut self.norm_f.g[..]);
        v.push(&mut self.head.w[..]);
        v.push(&mut self.head.b[..]);
        v
    }

    /// `(nome, forma)` de cada tensor, na MESMA ordem de [`Self::params_mut`].
    ///
    /// Existe para exportar em safetensors, que precisa de nome e forma — o `.bin`
    /// nativo guarda so o comprimento. As formas vem dos proprios objetos
    /// (`Linear` tem `n_in`/`n_out`), nunca do `Config`: deduzir criaria uma segunda
    /// fonte de verdade que sai de sincronia sem avisar.
    ///
    /// O que sobra de risco e a ORDEM, e o teste `os_descritores_batem_com_os_pesos`
    /// a cobra posicao por posicao.
    pub fn descritores(&self) -> Vec<(String, Vec<usize>)> {
        let mut v = self.emb.descritores("emb");
        v.extend(self.enc.descritores("enc"));
        v.extend(self.proj_pool.descritores("proj_pool"));
        v.extend(self.bb.descritores("bb"));
        v.extend(self.proj_ctx.descritores("proj_ctx"));
        v.extend(self.dec.descritores("dec"));
        v.extend(self.norm_f.descritores("norm_f"));
        v.extend(self.head.descritores("head"));
        v
    }

    pub fn grad(&self) -> TekaGrad<T> {
        TekaGrad {
            emb: self.emb.grad(),
            enc: self.enc.grad(),
            proj_pool: self.proj_pool.grad(),
            bb: self.bb.grad(),
            proj_ctx: self.proj_ctx.grad(),
            dec: self.dec.grad(),
            norm_f: self.norm_f.grad(),
            head: self.head.grad(),
        }
    }

    pub fn estado_zero(&self, batch: usize) -> Estado<T> {
        Estado {
            enc: self.enc.zero_states(batch),
            bb: self.bb.zero_states(batch),
            dec: self.dec.zero_states(batch),
        }
    }

    fn preparar(&self, cache: &mut TekaCache<T>, rows: usize, prows: usize) {
        let (dl, db) = (self.cfg.d_loc, self.cfg.d_bb);
        for buf in [
            &mut cache.e0,
            &mut cache.e,
            &mut cache.ctx_b,
            &mut cache.dec_in,
            &mut cache.hdec,
            &mut cache.hn,
            &mut cache.dhn,
            &mut cache.dhdec,
            &mut cache.ddec_in,
            &mut cache.de,
            &mut cache.de0,
        ] {
            buf.resize(rows * dl, T::ZERO);
        }
        for buf in [&mut cache.pool_in, &mut cache.dpool_in, &mut cache.zc, &mut cache.dzc] {
            buf.resize(prows * dl, T::ZERO);
        }
        for buf in [&mut cache.bb_in, &mut cache.z, &mut cache.dz, &mut cache.dbb_in] {
            buf.resize(prows * db, T::ZERO);
        }
        cache.logits.resize(rows * VOCAB, T::ZERO);
        cache.dlogits.resize(rows * VOCAB, T::ZERO);
    }


    /// Encoder local → pooling nos fins de patch → backbone. Termina em `cache.z`.
    ///
    /// Compartilhado entre a previsão do próximo byte (fase 1) e as cabeças de
    /// decisão (fase 2) — as duas leem o mesmo tronco. Manter isto num lugar só não
    /// é higiene: são duas cadeias de forward/backward escritas à mão, e se elas
    /// divergirem o modelo treina uma coisa e decide com outra.
    pub fn tronco<O: Ops<T>>(
        &self,
        ops: &O,
        bytes: &[u8],
        plano: &Plano,
        est: &Estado<T>,
        cache: &mut TekaCache<T>,
    ) {
        let (seq, batch, p_max) = (plano.seq, plano.batch, plano.p_max);
        let (rows, prows) = (seq * batch, p_max * batch);
        let dl = self.cfg.d_loc;
        self.preparar(cache, rows, prows);
        self.emb.forward(bytes, &mut cache.e0);
        self.enc.forward(
            ops,
            &cache.e0,
            &est.enc,
            seq,
            batch,
            &mut cache.e,
            &mut cache.enc,
        );

        // Pooling: o estado do encoder no ÚLTIMO byte do patch resume o patch — o
        // encoder é recorrente, então esse estado já viu tudo que veio antes.
        cache.pool_in.fill(T::ZERO);
        for p in 0..p_max {
            for b in 0..batch {
                let ub = plano.ultimo_byte[p * batch + b];
                if ub == SEM_PATCH {
                    continue;
                }
                let dst = (p * batch + b) * dl;
                let src = (ub * batch + b) * dl;
                cache.pool_in[dst..dst + dl].copy_from_slice(&cache.e[src..src + dl]);
            }
        }
        self.proj_pool
            .forward(ops, &cache.pool_in, prows, &mut cache.bb_in);
        self.bb.forward(
            ops,
            &cache.bb_in,
            &est.bb,
            p_max,
            batch,
            &mut cache.z,
            &mut cache.bb,
        );

    }

    /// O espelho de [`Teka::tronco`].
    ///
    /// Espera `cache.dz` com ∂L/∂z e `cache.de` já semeado com a contribuição que
    /// vem de fora do tronco (zero, se não houver).
    pub fn tronco_backward<O: Ops<T>>(
        &self,
        ops: &O,
        bytes: &[u8],
        plano: &Plano,
        est: &Estado<T>,
        cache: &mut TekaCache<T>,
        g: &mut TekaGrad<T>,
    ) {
        let (seq, batch, p_max) = (plano.seq, plano.batch, plano.p_max);
        let prows = p_max * batch;
        let dl = self.cfg.d_loc;
        self.bb.backward(
            ops,
            &cache.bb_in,
            &est.bb,
            &cache.dz,
            p_max,
            batch,
            &mut cache.bb,
            &mut cache.dbb_in,
            false,
            &mut g.bb,
        );
        self.proj_pool.backward(
            ops,
            &cache.pool_in,
            &cache.dbb_in,
            prows,
            Some(&mut cache.dpool_in),
            false,
            &mut g.proj_pool,
        );

        // Espelho do pooling: o gradiente volta para o byte de onde veio.
        for p in 0..p_max {
            for b in 0..batch {
                let ub = plano.ultimo_byte[p * batch + b];
                if ub == SEM_PATCH {
                    continue;
                }
                let src = (p * batch + b) * dl;
                let dst = (ub * batch + b) * dl;
                for i in 0..dl {
                    cache.de[dst + i] += cache.dpool_in[src + i];
                }
            }
        }

        self.enc.backward(
            ops,
            &cache.e0,
            &est.enc,
            &cache.de,
            seq,
            batch,
            &mut cache.enc,
            &mut cache.de0,
            false,
            &mut g.enc,
        );
        self.emb.backward(bytes, &cache.de0, &mut g.emb);

    }

    /// Um passo completo: forward, perda e (se `grad` for `Some`) backward.
    ///
    /// `bytes` e `alvos` em ordem *time-major* `[seq·batch]`. Devolve a perda média
    /// em **nats por byte** (divida por `ln 2` para bits/byte).
    ///
    /// O estado em `est` é **lido**, não atualizado — use [`Teka::estados_finais`]
    /// depois do forward para obter o estado da janela seguinte.
    #[allow(clippy::too_many_arguments)]
    pub fn passo<O: Ops<T>>(
        &self,
        ops: &O,
        bytes: &[u8],
        alvos: &[u8],
        plano: &Plano,
        est: &Estado<T>,
        cache: &mut TekaCache<T>,
        mut grad: Option<&mut TekaGrad<T>>,
    ) -> f64 {
        let (seq, batch, p_max) = (plano.seq, plano.batch, plano.p_max);
        let rows = seq * batch;
        let prows = p_max * batch;
        let dl = self.cfg.d_loc;
        self.preparar(cache, rows, prows);

        self.tronco(ops, bytes, plano, est, cache);

        // Projeta ANTES de espalhar: p_max·batch linhas em vez de seq·batch.
        self.proj_ctx.forward(ops, &cache.z, prows, &mut cache.zc);

        cache.ctx_b.fill(T::ZERO);
        for t in 0..seq {
            for b in 0..batch {
                let c = plano.ctx[t * batch + b];
                if c == SEM_PATCH {
                    continue;
                }
                let dst = (t * batch + b) * dl;
                let src = (c * batch + b) * dl;
                cache.ctx_b[dst..dst + dl].copy_from_slice(&cache.zc[src..src + dl]);
            }
        }
        for k in 0..rows * dl {
            cache.dec_in[k] = cache.e[k] + cache.ctx_b[k];
        }

        self.dec.forward(
            ops,
            &cache.dec_in,
            &est.dec,
            seq,
            batch,
            &mut cache.hdec,
            &mut cache.dec,
        );
        self.norm_f
            .forward(&cache.hdec, rows, &mut cache.hn, &mut cache.norm_f);
        self.head.forward(ops, &cache.hn, rows, &mut cache.logits);

        // ---------------- perda ----------------
        let inv_n = T::from_f64(1.0 / rows as f64);
        let mut perda = 0.0f64;
        for k in 0..rows {
            let row = &cache.logits[k * VOCAB..(k + 1) * VOCAB];
            let mut maxi = row[0];
            for &v in row.iter() {
                maxi = maxi.max(v);
            }
            let mut soma = T::ZERO;
            for &v in row.iter() {
                soma += (v - maxi).exp();
            }
            let log_z = maxi + soma.ln();
            let alvo = alvos[k] as usize;
            perda += (log_z - row[alvo]).to_f64();

            if grad.is_some() {
                let drow = &mut cache.dlogits[k * VOCAB..(k + 1) * VOCAB];
                for j in 0..VOCAB {
                    drow[j] = (row[j] - log_z).exp() * inv_n;
                }
                drow[alvo] -= inv_n;
            }
        }
        perda /= rows as f64;

        let Some(g) = grad.as_deref_mut() else {
            return perda;
        };

        // ---------------- backward ----------------
        self.head.backward(
            ops,
            &cache.hn,
            &cache.dlogits,
            rows,
            Some(&mut cache.dhn),
            false,
            &mut g.head,
        );
        self.norm_f.backward(
            &cache.hdec,
            &cache.dhn,
            rows,
            &cache.norm_f,
            &mut cache.dhdec,
            false,
            &mut g.norm_f,
        );
        self.dec.backward(
            ops,
            &cache.dec_in,
            &est.dec,
            &cache.dhdec,
            seq,
            batch,
            &mut cache.dec,
            &mut cache.ddec_in,
            false,
            &mut g.dec,
        );

        // dec_in = e + ctx_b → o gradiente vai inteiro para os dois.
        cache.de.copy_from_slice(&cache.ddec_in);

        cache.dzc.fill(T::ZERO);
        for t in 0..seq {
            for b in 0..batch {
                let c = plano.ctx[t * batch + b];
                if c == SEM_PATCH {
                    continue;
                }
                let src = (t * batch + b) * dl;
                let dst = (c * batch + b) * dl;
                for i in 0..dl {
                    cache.dzc[dst + i] += cache.ddec_in[src + i];
                }
            }
        }

        self.proj_ctx.backward(
            ops,
            &cache.z,
            &cache.dzc,
            prows,
            Some(&mut cache.dz),
            false,
            &mut g.proj_ctx,
        );
        self.tronco_backward(ops, bytes, plano, est, cache, g);

        perda
    }

    /// Estado ao fim da janela, para alimentar a janela seguinte (BPTT truncado).
    pub fn estados_finais(&self, cache: &TekaCache<T>, plano: &Plano) -> Estado<T> {
        Estado {
            enc: self.enc.estados_finais(&cache.enc, plano.seq, plano.batch),
            bb: self.bb.estados_finais(&cache.bb, plano.p_max, plano.batch),
            dec: self.dec.estados_finais(&cache.dec, plano.seq, plano.batch),
        }
    }
}
