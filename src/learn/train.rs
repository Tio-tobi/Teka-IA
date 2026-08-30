//! Laço de treino do modelo de linguagem por byte.
//!
//! ## Por que as janelas são contíguas, e não amostradas ao acaso
//!
//! Cada posição do lote (*lane*) é um **cursor que caminha pelo corpus**. A janela
//! seguinte de uma lane começa exatamente onde a anterior terminou, e o estado
//! recorrente é carregado entre elas. É o que dá ao modelo memória mais longa que a
//! janela de BPTT — o gradiente para na fronteira, a memória não.
//!
//! Amostrar janelas aleatórias a cada passo, como se faz com Transformer, jogaria
//! fora exatamente essa propriedade. Quando uma lane chega ao fim do corpus ela
//! pula para uma posição aleatória **e tem o estado zerado** — carregar estado
//! através de uma descontinuidade seria ensinar o modelo uma transição que não
//! existe.

use std::path::Path;
use std::time::Instant;

use crate::backend::Ops;
use crate::learn::adam::Adam;
use crate::model::hierarchy::{Estado, Teka, TekaCache};
use crate::model::patcher::{Patcher, Plano};
use crate::rng::Rng;

pub const LN2: f64 = std::f64::consts::LN_2;

#[derive(Clone, Debug)]
pub struct CfgTreino {
    pub seq: usize,
    pub batch: usize,
    pub lr: f64,
    pub clip: f64,
    pub passos: usize,
    pub minutos: f64,
    pub log_cada: usize,
    pub semente: u64,
}

impl Default for CfgTreino {
    fn default() -> Self {
        Self {
            seq: 256,
            // 24, e nao 8 nem 48. Medido com 1200s de relogio para cada um, e
            // julgado por bits/byte em `dados/val_tec.txt`, que fica FORA do treino:
            //
            //   batch  passos  bytes/s   treino    val_tec
            //     8     1633    2787     2.2753    2.8343
            //    24      679    3475     2.4714    2.5776   <- melhor
            //    48      350    3578     2.8507    2.9480
            //
            // Duas metricas erradas ja escolheram numero aqui. `bytes/s` mede vazao
            // e aponta 48; perda de TREINO mede decoreba e aponta 8 — e 8 e o pior
            // dos tres onde importa, com distancia treino->validacao de 0,559 contra
            // 0,106 do 24.
            //
            // A causa esta no `Fluxo`: ele reparte o corpus em `batch` faixas. Batch
            // menor nao e so gradiente mais ruidoso, e MENOS TEXTO DISTINTO — com 8
            // o modelo le 8 pedacos do corpus, com 24 le 24. E 48 tem a variedade
            // mas nao tem passo de gradiente que chegue no mesmo tempo.
            batch: 24,
            lr: 1e-3,
            clip: 1.0,
            passos: usize::MAX,
            minutos: 0.0,
            log_cada: 20,
            semente: 1234,
        }
    }
}

/// Cursores caminhando pelo corpus, um por lane do lote.
pub struct Fluxo {
    corpus: Vec<u8>,
    cursores: Vec<usize>,
    seq: usize,
    batch: usize,
    rng: Rng,
}

impl Fluxo {
    pub fn novo(corpus: Vec<u8>, seq: usize, batch: usize, semente: u64) -> Self {
        assert!(
            corpus.len() > seq + 1,
            "corpus curto demais ({} bytes) para janela de {seq}",
            corpus.len()
        );
        let passo = corpus.len() / batch;
        let cursores = (0..batch).map(|b| b * passo).collect();
        Self {
            corpus,
            cursores,
            seq,
            batch,
            rng: Rng::new(semente),
        }
    }

    pub fn tamanho(&self) -> usize {
        self.corpus.len()
    }

    /// Preenche `bytes` e `alvos` em ordem *time-major* e devolve as lanes que
    /// deram a volta (e portanto precisam ter o estado zerado).
    pub fn proximo(&mut self, bytes: &mut Vec<u8>, alvos: &mut Vec<u8>) -> Vec<usize> {
        bytes.resize(self.seq * self.batch, 0);
        alvos.resize(self.seq * self.batch, 0);
        let mut reiniciadas = Vec::new();
        let limite = self.corpus.len() - self.seq - 1;

        for b in 0..self.batch {
            if self.cursores[b] > limite {
                self.cursores[b] = (self.rng.uniform01() * limite as f64) as usize;
                reiniciadas.push(b);
            }
            let c = self.cursores[b];
            for t in 0..self.seq {
                bytes[t * self.batch + b] = self.corpus[c + t];
                alvos[t * self.batch + b] = self.corpus[c + t + 1];
            }
            self.cursores[b] = c + self.seq;
        }
        reiniciadas
    }
}

/// bits/byte num trecho fixo, com estado zerado — a métrica de validação.
///
/// Zerar o estado é de propósito: mede o quanto o modelo prevê **sem** memória
/// herdada, que é a comparação justa entre execuções diferentes.
pub fn avaliar<O: Ops<f32>, P: Patcher + ?Sized>(
    modelo: &Teka<f32>,
    ops: &O,
    patcher: &P,
    trecho: &[u8],
    seq: usize,
    cache: &mut TekaCache<f32>,
) -> f64 {
    let batch = 1;
    let mut est = modelo.estado_zero(batch);
    let mut total = 0.0f64;
    let mut n = 0usize;
    let mut pos = 0usize;

    while pos + seq + 1 <= trecho.len() {
        let bytes = &trecho[pos..pos + seq];
        let alvos = &trecho[pos + 1..pos + seq + 1];
        let plano = Plano::novo(patcher, bytes, seq, batch);
        total += modelo.passo(ops, bytes, alvos, &plano, &est, cache, None);
        est = modelo.estados_finais(cache, &plano);
        n += 1;
        pos += seq;
    }
    if n == 0 {
        return f64::NAN;
    }
    total / n as f64 / LN2
}

pub struct Relatorio {
    pub passos: usize,
    pub bits_por_byte: f64,
    pub bytes_por_patch: f64,
    pub bytes_por_s: f64,
    pub segundos: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn treinar<O: Ops<f32>, P: Patcher + ?Sized>(
    modelo: &mut Teka<f32>,
    ops: &O,
    patcher: &P,
    corpus: Vec<u8>,
    cfg: &CfgTreino,
    validacao: &[u8],
    salvar_em: Option<&Path>,
) -> Relatorio {
    let tamanhos: Vec<usize> = modelo.params_mut().iter().map(|p| p.len()).collect();
    let mut adam = Adam::novo(&tamanhos, cfg.lr);
    let mut grad = modelo.grad();
    let mut cache = TekaCache::new();
    let mut cache_val = TekaCache::new();
    let mut est: Estado<f32> = modelo.estado_zero(cfg.batch);
    let mut fluxo = Fluxo::novo(corpus, cfg.seq, cfg.batch, cfg.semente);

    let mut bytes = Vec::new();
    let mut alvos = Vec::new();

    println!(
        "  corpus {:.2} MB | {} params | backbone {:.0}% | otimizador +{:.1} MB",
        fluxo.tamanho() as f64 / 1e6,
        modelo.n_params(),
        modelo.fracao_backbone() * 100.0,
        adam.n_estados() as f64 * 8.0 / 1e6,
    );
    println!(
        "  seq={} batch={} lr={} clip={} patcher={}",
        cfg.seq,
        cfg.batch,
        cfg.lr,
        cfg.clip,
        patcher.nome()
    );
    println!();
    println!("     passo   bits/byte   b/patch   |grad|      bytes/s   tempo");
    println!("  ─────────────────────────────────────────────────────────────");

    let t0 = Instant::now();
    let mut ema = f64::NAN;
    let mut bpp = 0.0;
    let mut passo = 0usize;
    let mut bytes_vistos = 0usize;

    while passo < cfg.passos {
        if cfg.minutos > 0.0 && t0.elapsed().as_secs_f64() >= cfg.minutos * 60.0 {
            break;
        }

        let reiniciadas = fluxo.proximo(&mut bytes, &mut alvos);
        for b in reiniciadas {
            est.zerar_lane(b, cfg.batch);
        }

        let plano = Plano::novo(patcher, &bytes, cfg.seq, cfg.batch);
        bpp = plano.bytes_por_patch();

        grad.clear();
        let perda = modelo.passo(
            ops,
            &bytes,
            &alvos,
            &plano,
            &est,
            &mut cache,
            Some(&mut grad),
        );
        est = modelo.estados_finais(&cache, &plano);

        let norma = {
            let fatias = grad.slices();
            let mut params = modelo.params_mut();
            adam.passo(&mut params, &fatias, cfg.clip)
        };

        let bpb = perda / LN2;
        ema = if ema.is_nan() { bpb } else { 0.98 * ema + 0.02 * bpb };
        passo += 1;
        bytes_vistos += cfg.seq * cfg.batch;

        if passo % cfg.log_cada == 0 || passo == 1 {
            let dt = t0.elapsed().as_secs_f64();
            println!(
                "  {passo:>8}   {ema:>9.4}   {bpp:>7.2}   {norma:>6.2}   {:>10.0}   {:>5.0}s",
                bytes_vistos as f64 / dt,
                dt
            );
        }
    }

    let dt = t0.elapsed().as_secs_f64();

    if !validacao.is_empty() {
        let v = avaliar(modelo, ops, patcher, validacao, cfg.seq, &mut cache_val);
        println!("\n  validação (estado zerado): {v:.4} bits/byte");
    }

    if let Some(p) = salvar_em {
        match modelo.salvar(p) {
            Ok(n) => println!("  cérebro salvo em {} ({:.1} MB)", p.display(), n as f64 / 1e6),
            Err(e) => eprintln!("  falha ao salvar: {e}"),
        }
    }

    Relatorio {
        passos: passo,
        bits_por_byte: ema,
        bytes_por_patch: bpp,
        bytes_por_s: bytes_vistos as f64 / dt,
        segundos: dt,
    }
}
