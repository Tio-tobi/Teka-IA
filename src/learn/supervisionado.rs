//! Treino supervisionado das cabeças de decisão.
//!
//! É o passo barato do projeto e o que mais muda o que a Teka consegue fazer.
//! Enquanto o modelo de linguagem precisa de horas para tirar bits/byte de 8 para 2,
//! as cabeças precisam de minutos para sair de "chuta entre 9 ferramentas" para
//! "acerta quase sempre" — porque **discriminar é muito mais barato que gerar**.
//!
//! ## Lote retangular com pedidos de tamanhos diferentes
//!
//! Os pedidos são preenchidos com espaço até `seq`, e o [`Plano`] é então
//! restringido ao comprimento real de cada um ([`Plano::limitar`]). Sem isso, "o
//! estado do último patch" seria o estado depois de ler preenchimento.

use crate::backend::Ops;
use crate::learn::adam::Adam;
use crate::learn::dados::Exemplo;
use crate::model::agente::{Agente, AgenteCache};
use crate::model::heads::{Alvo, Placar, MAX_SLOTS};
use crate::model::patcher::{Patcher, Plano};
use crate::rng::Rng;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct CfgSup {
    pub seq: usize,
    pub batch: usize,
    pub lr: f64,
    pub clip: f64,
    pub epocas: usize,
    pub log_cada: usize,
    pub semente: u64,
    /// Multiplicador da taxa de aprendizado do TRONCO em relacao as cabecas.
    ///
    /// 1,0 treina tudo igual (certo quando o tronco nasce aleatorio). Com tronco
    /// pre-treinado, use algo como 0,05: as cabecas precisam aprender do zero, o
    /// tronco so precisa se ajustar.
    pub lr_tronco: f64,
}

impl Default for CfgSup {
    fn default() -> Self {
        Self {
            // 64 nao comporta pedido verboso: as dez frases que um amigo do John
            // escreveu para testa-la tinham mediana de 106 bytes, e ela acertava o
            // argumento em ZERO delas. Sem a janela maior, os exemplos longos que o
            // gerador agora produz seriam truncados no proprio treino.
            seq: 128,
            batch: 16,
            lr: 1e-3,
            clip: 1.0,
            epocas: 12,
            log_cada: 20,
            semente: 99,
            lr_tronco: 1.0,
        }
    }
}

/// Monta um lote: bytes preenchidos, plano restringido, alvos.
pub fn montar_lote<P: Patcher + ?Sized>(
    exs: &[&Exemplo],
    patcher: &P,
    seq: usize,
) -> Option<(Vec<u8>, Plano, Vec<Alvo>)> {
    let batch = exs.len();
    let mut bytes = vec![b' '; seq * batch];
    let mut comprimentos = vec![0usize; batch];
    let mut alvos = Vec::with_capacity(batch);

    for (b, e) in exs.iter().enumerate() {
        let p = e.pedido.as_bytes();
        // Rede de segurança: quem chama já filtrou com `cabe_na_janela`. Continua
        // aqui porque escrever fora do buffer seria pior que perder um lote — mas
        // não é mais este `return` que decide o que entra no treino.
        if !cabe_na_janela(e, seq) {
            return None;
        }
        for (t, &c) in p.iter().enumerate() {
            bytes[t * batch + b] = c;
        }
        comprimentos[b] = p.len();
        alvos.push(e.alvo(patcher)?);
    }
    let mut plano = Plano::novo(patcher, &bytes, seq, batch);
    plano.limitar(&comprimentos);
    Some((bytes, plano, alvos))
}

/// O pedido cabe na janela, com o byte de folga que o `Plano` precisa?
///
/// Isto era uma condição solta dentro de `montar_lote` que devolvia `None` — e
/// `None` ali derrubava o **lote inteiro**, não o exemplo. Com 15,6% de exemplos
/// longos e lote de 16, isso virava 93,4% dos lotes descartados em silêncio: o
/// teste de integração treinava com 6,6% dos dados e chegava a 17,9% de intenção
/// contra 11,1% de chute cego, sem uma linha de log explicando.
///
/// A perda cresce com o TAMANHO DO LOTE, não com a fração de exemplos longos. É o
/// que torna o defeito desproporcional à sua causa, e o que o fez passar meses
/// invisível.
pub fn cabe_na_janela(e: &Exemplo, seq: usize) -> bool {
    !e.pedido.is_empty() && e.pedido.len() <= seq - 1
}

pub fn avaliar<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &P,
    exs: &[Exemplo],
    seq: usize,
    batch: usize,
    cache: &mut AgenteCache<f32>,
) -> (f64, Placar) {
    let mut placar = Placar::default();
    let mut perda = 0.0;
    let mut n = 0;
    for pedaco in exs.chunks(batch) {
        // Filtra por EXEMPLO, nao por lote. Ver `cabe_na_janela`: um pedido comprido
        // no meio do lote nao pode apagar os outros quinze da medicao — isso
        // enviesaria o placar para frases curtas sem nada no log dizendo.
        let refs: Vec<&Exemplo> = pedaco.iter().filter(|e| cabe_na_janela(e, seq)).collect();
        if refs.is_empty() {
            continue;
        }
        let Some((bytes, plano, alvos)) = montar_lote(&refs, patcher, seq) else {
            continue;
        };
        let est = ag.modelo.estado_zero(refs.len());
        let (p, pl) = ag.compreender(ops, &bytes, &plano, &est, &alvos, cache, None);
        perda += p;
        placar.somar(&pl);
        n += 1;
    }
    (if n == 0 { f64::NAN } else { perda / n as f64 }, placar)
}

#[allow(clippy::too_many_arguments)]
pub fn treinar_agente<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &mut Agente<f32>,
    ops: &O,
    patcher: &P,
    treino: &[Exemplo],
    validacao: &[Exemplo],
    cfg: &CfgSup,
) -> Placar {
    let tamanhos: Vec<usize> = ag.params_mut().iter().map(|p| p.len()).collect();
    let n_tronco = ag.modelo.params_mut().len();
    let mut adam = Adam::novo(&tamanhos, cfg.lr);
    if cfg.lr_tronco != 1.0 {
        adam.escalar_primeiros(n_tronco, cfg.lr_tronco);
    }
    let mut grad = ag.grad();
    let mut cache = AgenteCache::new();
    let mut cache_val = AgenteCache::new();
    let mut rng = Rng::new(cfg.semente);

    println!(
        "  {} exemplos de treino, {} de validacao | {} ferramentas | {} params",
        treino.len(),
        validacao.len(),
        ag.registro.n(),
        ag.n_params()
    );
    println!(
        "  seq={} batch={} lr={} (tronco x{}) epocas={}",
        cfg.seq, cfg.batch, cfg.lr, cfg.lr_tronco, cfg.epocas
    );

    // Quanto a janela esta jogando fora, DITO EM VOZ ALTA.
    //
    // `montar_lote` devolve `None` quando um unico exemplo passa de `seq - 1`, e o
    // laco pula o lote INTEIRO. Um exemplo longo mata os outros quinze junto, entao
    // a perda cresce com o tamanho do lote e nao com a fracao de exemplos longos:
    // 15,6% de exemplos longos viram 93,4% de lotes perdidos com batch 16.
    //
    // Isso aconteceu de verdade e passou meses invisivel. O teste de integracao
    // fixava `seq: 64` enquanto o treino real subia para 128; ele treinava com 6,6%
    // dos dados, chegava a 17,9% de intencao (chute cego: 11,1%) e ninguem sabia por
    // que, porque a unica pista era um `continue` silencioso.
    let longos = treino.iter().filter(|e| e.pedido.len() > cfg.seq - 1).count();
    if longos > 0 {
        let frac = longos as f64 / treino.len().max(1) as f64;
        let perdidos = 1.0 - (1.0 - frac).powi(cfg.batch as i32);
        let maior = treino.iter().map(|e| e.pedido.len()).max().unwrap_or(0);
        println!();
        println!("  AVISO: {longos} exemplos ({:.1}%) passam de seq-1={}", frac * 100.0, cfg.seq - 1);
        println!("  o maior tem {maior} bytes — cada um mata o lote inteiro em que cai,");
        println!("  entao ~{:.1}% dos lotes nao vao treinar NADA.", perdidos * 100.0);
        println!("  aumente --seq para pelo menos {}.", maior + 1);
    }
    println!();
    println!("    epoca   perda   intencao   argumento   ponta-a-ponta   tempo");
    println!("  ─────────────────────────────────────────────────────────────");

    let t0 = Instant::now();
    let mut ordem: Vec<usize> = (0..treino.len()).collect();
    let mut ultimo = Placar::default();
    // A perda de validacao sobe muito antes da acuracia parar de subir; sem
    // registrar o melhor ponto, o numero final e o de um modelo ja passado do ponto.
    let mut melhor = Placar::default();
    let mut melhor_epoca = 0usize;
    // Instantaneo dos pesos na melhor epoca.
    //
    // Sem isto o treino reporta a melhor epoca e SALVA a ultima -- e como a perda
    // de validacao sobe bem antes da acuracia parar de subir, as duas divergem
    // muito: medido, 75,1% na melhor epoca contra 67,7% na ultima. O numero
    // reportado descrevia um modelo que ninguem ia rodar.
    let mut melhores_pesos: Vec<Vec<f32>> = Vec::new();

    for epoca in 1..=cfg.epocas {
        // Embaralha a cada época (Fisher-Yates): sem isso o lote sempre traz as
        // mesmas ferramentas juntas e o gradiente fica correlacionado.
        for i in (1..ordem.len()).rev() {
            let j = (rng.uniform01() * (i + 1) as f64) as usize % (i + 1);
            ordem.swap(i, j);
        }

        for pedaco in ordem.chunks(cfg.batch) {
            // Filtra por EXEMPLO, nao por lote — ver `cabe_na_janela`.
            let refs: Vec<&Exemplo> = pedaco
                .iter()
                .map(|&i| &treino[i])
                .filter(|e| cabe_na_janela(e, cfg.seq))
                .collect();
            if refs.is_empty() {
                continue;
            }
            let Some((bytes, plano, alvos)) = montar_lote(&refs, patcher, cfg.seq) else {
                continue;
            };
            let est = ag.modelo.estado_zero(refs.len());
            grad.clear();
            ag.compreender(ops, &bytes, &plano, &est, &alvos, &mut cache, Some(&mut grad));
            let fatias = grad.slices();
            let mut params = ag.params_mut();
            adam.passo(&mut params, &fatias, cfg.clip);
        }

        let (pv, placar) = avaliar(ag, ops, patcher, validacao, cfg.seq, cfg.batch, &mut cache_val);
        ultimo = placar;
        // `melhor_epoca == 0` precisa vir primeiro: `Placar::default()` tem n=0, e
        // `acuracia_total()` devolve NaN. Qualquer comparacao com NaN e falsa, entao
        // sem esta guarda o melhor NUNCA e atualizado e o relatorio sai como NaN.
        if melhor_epoca == 0 || placar.acuracia_total() > melhor.acuracia_total() {
            melhor = placar;
            melhor_epoca = epoca;
            melhores_pesos = ag.params_mut().iter().map(|t| t.to_vec()).collect();
        }
        if epoca % cfg.log_cada == 0 || epoca == 1 || epoca == cfg.epocas {
            println!(
                "  {epoca:>8}   {pv:>5.3}   {:>7.1}%   {:>8.1}%   {:>12.1}%   {:>4.0}s",
                placar.acuracia_intencao() * 100.0,
                placar.acuracia_span() * 100.0,
                placar.acuracia_total() * 100.0,
                t0.elapsed().as_secs_f64()
            );
        }
    }
    println!(
        "\n  melhor epoca: {melhor_epoca} — intencao {:.1}%  argumento {:.1}%  ponta-a-ponta {:.1}%",
        melhor.acuracia_intencao() * 100.0,
        melhor.acuracia_span() * 100.0,
        melhor.acuracia_total() * 100.0
    );
    println!(
        "  argumento quando a ferramenta saiu certa: {:.1}%",
        melhor.acuracia_span_dado_intencao() * 100.0
    );
    if !melhores_pesos.is_empty() {
        for (t, guardado) in ag.params_mut().iter_mut().zip(&melhores_pesos) {
            t.copy_from_slice(guardado);
        }
        println!("  pesos restaurados para a epoca {melhor_epoca}");
    }
    let _ = ultimo;
    melhor
}

pub const _MAX_SLOTS: usize = MAX_SLOTS;

#[cfg(test)]
mod tests_janela {
    use super::*;
    use crate::learn::dados::gerar;
    use crate::model::patcher::PorPalavra;
    use crate::rng::Rng;
    use crate::tools::Registro;

    /// A janela padrão precisa caber TODO exemplo que o gerador produz.
    ///
    /// Hoje a folga é de **um byte**: o maior exemplo tem 127 e o limite é
    /// `seq - 1 = 127`. Um sufixo de enchimento a mais e o treino real cai no mesmo
    /// buraco que derrubou o teste de integração — 93% dos lotes descartados em
    /// silêncio, e nada no log dizendo isso.
    ///
    /// Este teste é a trava. Se ele quebrar, a resposta é aumentar `CfgSup::seq`,
    /// **nunca** encurtar as frases: o comprimento veio de uma falha real que o
    /// teste de um amigo do John encontrou (mediana de 106 bytes contra uma janela
    /// de 64), e desfazê-lo é reabrir aquilo.
    ///
    /// Mede a **fração** perdida, não o máximo. Com o filtro por exemplo, um pedido
    /// de 128 bytes custa um exemplo em dezoito mil, e gastar 50% a mais de tempo de
    /// treino (seq 128 → 192) para salvá-lo seria caro pelo motivo errado. O que não
    /// pode acontecer é a fração crescer sem ninguém ver — e os exemplos perdidos
    /// são justamente os mais longos, ou seja, os que o teste do amigo mostrou serem
    /// os que ela erra.
    #[test]
    fn quase_nenhum_exemplo_fica_fora_da_janela_padrao() {
        const TETO: f64 = 0.005;
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let seq = CfgSup::default().seq;
        // Várias sementes: o enchimento longo é sorteado, e uma semente sozinha
        // pode não trazer a cauda.
        let (mut fora, mut total, mut maior) = (0usize, 0usize, 0usize);
        for s in [1u64, 77, 2026] {
            let mut rng = Rng::new(s);
            for e in gerar(&reg, &patcher, 6000, &mut rng) {
                maior = maior.max(e.pedido.len());
                fora += !cabe_na_janela(&e, seq) as usize;
                total += 1;
            }
        }
        let frac = fora as f64 / total as f64;
        println!(
            "\n  janela {seq} (limite {}): maior exemplo {maior} bytes, \
             {fora} de {total} fora ({:.3}%)",
            seq - 1,
            frac * 100.0
        );
        assert!(
            frac <= TETO,
            "{:.2}% dos exemplos nao cabem na janela padrao (teto {:.1}%). \
             Aumente CfgSup::seq — nao encurte as frases.",
            frac * 100.0,
            TETO * 100.0
        );
    }

    /// Quantos LOTES a janela descarta, e quantos exemplos ficam longos demais?
    #[test]
    fn quanto_a_janela_custa() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(77);
        let exs = gerar(&reg, &patcher, 6000, &mut rng);

        let mut comp: Vec<usize> = exs.iter().map(|e| e.pedido.len()).collect();
        comp.sort_unstable();
        let pct = |p: f64| comp[((comp.len() - 1) as f64 * p) as usize];
        println!(
            "\n  {} exemplos — mediana {} bytes, p90 {}, p99 {}, maior {}",
            comp.len(), pct(0.5), pct(0.9), pct(0.99), comp[comp.len() - 1]
        );

        for seq in [64usize, 128, 192, 256] {
            let longos = comp.iter().filter(|&&c| c > seq - 1).count();
            let frac = longos as f64 / comp.len() as f64;
            // Um lote morre se QUALQUER um dos 16 for longo demais.
            let lote_vive = (1.0 - frac).powi(16);
            println!(
                "  seq={:3}  exemplos longos {:5} ({:5.2}%)  ->  lotes perdidos {:5.1}%",
                seq, longos, frac * 100.0, (1.0 - lote_vive) * 100.0
            );
        }
    }
}
