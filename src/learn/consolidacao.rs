//! Consolidação — o "sono" da Teka.
//!
//! Pega o que aconteceu desde a última vez e fixa nos pesos. O ponto delicado não é
//! aprender o novo: é **não desaprender o velho**.
//!
//! ## O mecanismo anti-esquecimento
//!
//! Todo lote mistura duas fontes:
//!
//! ```text
//! lote = [ episódios reais amostrados da memória ]  +  [ replay do corpus base ]
//!          └── o que você corrigiu ──────────────┘     └── o que ela já sabia ──┘
//! ```
//!
//! Treinar só nas correções recentes é a receita clássica do esquecimento
//! catastrófico: o modelo passa a acertar as dez frases novas e perde as mil
//! antigas. A `fracao_episodios` controla essa mistura, e o padrão é conservador —
//! o novo é minoria no lote, porque o novo já é raro e o gradiente dele já pesa
//! proporcionalmente mais.
//!
//! ## Por que a taxa é baixa
//!
//! Consolidar não é treinar do zero: os pesos já estão bons. Uma taxa alta com
//! poucos exemplos novos move o modelo para longe do que ele sabia antes de os
//! novos dados terem estatística para justificar. O padrão aqui é uma ordem de
//! grandeza abaixo da do treino inicial.

use crate::backend::Ops;
use crate::learn::adam::Adam;
use crate::learn::dados::Exemplo;
use crate::learn::supervisionado::{avaliar, montar_lote};
use crate::memory::{como_exemplo, MemoriaEpisodica};
use crate::model::agente::{Agente, AgenteCache};
use crate::model::heads::Placar;
use crate::model::patcher::Patcher;
use crate::rng::Rng;

#[derive(Clone, Debug)]
pub struct CfgConsolidacao {
    pub passos: usize,
    pub batch: usize,
    pub seq: usize,
    pub lr: f64,
    pub clip: f64,
    /// Fração do lote vinda da memória episódica; o resto é replay do corpus base.
    pub fracao_episodios: f64,
    /// Fração da parte episódica reservada aos episódios mais recentes.
    ///
    /// Só vale quando `minerar_a_cada` é 0. Com mineração ligada, a prioridade passa
    /// a ser dificuldade em vez de recência.
    pub fracao_recente: f64,
    /// De quantos em quantos passos reordenar os episódios por dificuldade.
    ///
    /// `0` desliga e volta à amostragem por recência. O modelo muda devagar dentro de
    /// uma consolidação, então reordenar todo passo seria pagar 120x por uma ordem
    /// quase igual.
    pub minerar_a_cada: usize,
    pub semente: u64,
}

impl Default for CfgConsolidacao {
    fn default() -> Self {
        Self {
            passos: 120,
            batch: 16,
            seq: 64,
            // Uma ordem de grandeza abaixo do treino inicial — ver a nota do módulo.
            lr: 2e-4,
            clip: 1.0,
            fracao_episodios: 0.35,
            fracao_recente: 0.4,
            minerar_a_cada: 20,
            semente: 4242,
        }
    }
}

pub struct RelatorioConsolidacao {
    pub passos: usize,
    pub episodios_usados: usize,
    pub antes: Placar,
    pub depois: Placar,
}

/// Ordena os exemplos do mais difícil para o mais fácil, pela perda do modelo.
///
/// Portado do `pick_hardest` da nila_mind. A ideia é simples e é a que faz o tick
/// render: treinar no que **ela erra**, não no que caiu na amostra.
///
/// A consolidação daqui amostrava por RECÊNCIA (`fracao_recente`), que é uma boa
/// heurística quando não se sabe o que é difícil — mas dá para saber, e custa uma
/// passagem de frente por candidato. Com dezenas de episódios ensináveis isso é
/// nada; se um dia forem milhares, `minerar_a_cada` existe para não pagar isso todo
/// passo.
///
/// **Sem gradiente**: recebe `&Agente`. Medir dificuldade não pode mexer em peso.
fn ordenar_por_dificuldade<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &P,
    exs: &[Exemplo],
    seq: usize,
) -> Vec<usize> {
    let mut cache = AgenteCache::new();
    let mut perdas: Vec<(usize, f64)> = Vec::with_capacity(exs.len());
    for (i, e) in exs.iter().enumerate() {
        let refs = [e];
        // Exemplo que nao monta lote vai para o FIM, nao some.
        //
        // `continue` aqui era um filtro silencioso: a fila saia menor que a entrada e
        // aquele exemplo nunca mais seria consolidado. E o mesmo defeito que o
        // Achado 32 achou no gerador, e o teste pegou pelo tamanho da fila.
        let Some((bytes, plano, alvos)) = montar_lote(&refs, patcher, seq) else {
            perdas.push((i, f64::NEG_INFINITY));
            continue;
        };
        let est = ag.modelo.estado_zero(1);
        let (perda, _) = ag.compreender(ops, &bytes, &plano, &est, &alvos, &mut cache, None);
        // NaN vai para o fim: exemplo que quebra a passagem de frente nao e "o mais
        // dificil", e treinar nele primeiro espalharia o NaN pelos pesos.
        perdas.push((i, if perda.is_finite() { perda } else { f64::NEG_INFINITY }));
    }
    perdas.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    perdas.into_iter().map(|(i, _)| i).collect()
}

/// Consolida a memória episódica nos pesos.
///
/// `base` é o corpus que ela já sabia — entra como replay para segurar o
/// esquecimento. `validacao` mede, antes e depois, se o que ela já sabia continua
/// de pé.
#[allow(clippy::too_many_arguments)]
pub fn consolidar<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &mut Agente<f32>,
    ops: &O,
    patcher: &P,
    memoria: &MemoriaEpisodica,
    base: &[Exemplo],
    validacao: &[Exemplo],
    cfg: &CfgConsolidacao,
) -> RelatorioConsolidacao {
    let mut cache = AgenteCache::new();
    let (_, antes) = avaliar(ag, ops, patcher, validacao, cfg.seq, cfg.batch, &mut cache);

    let episodios: Vec<Exemplo> = memoria
        .episodios
        .iter()
        .enumerate()
        .filter_map(|(i, e)| como_exemplo(e, i))
        .collect();

    if episodios.is_empty() || base.is_empty() {
        return RelatorioConsolidacao {
            passos: 0,
            episodios_usados: 0,
            antes,
            depois: antes,
        };
    }

    let tamanhos: Vec<usize> = ag.params_mut().iter().map(|p| p.len()).collect();
    let mut adam = Adam::novo(&tamanhos, cfg.lr);
    let mut grad = ag.grad();
    let mut rng = Rng::new(cfg.semente);
    let mut treino = AgenteCache::new();

    let n_ep = ((cfg.batch as f64) * cfg.fracao_episodios).round().max(1.0) as usize;
    let n_ep = n_ep.min(cfg.batch.saturating_sub(1)).max(1);

    // Ordem por dificuldade, refeita de tempos em tempos.
    let mut ordem: Vec<usize> = Vec::new();

    for passo in 0..cfg.passos {
        let mut lote: Vec<Exemplo> = if cfg.minerar_a_cada > 0 {
            if passo % cfg.minerar_a_cada == 0 {
                ordem = ordenar_por_dificuldade(ag, ops, patcher, &episodios, cfg.seq);
            }
            // Sorteia dentro do terço mais difícil em vez de pegar sempre os n
            // piores: repetir exatamente os mesmos n por 120 passos os decoraria, que
            // é o oposto do que a mineração quer.
            let janela = (n_ep * 3).min(ordem.len()).max(1);
            (0..n_ep)
                .map(|_| {
                    let k = (rng.uniform01() * janela as f64) as usize % janela;
                    episodios[ordem[k]].clone()
                })
                .collect()
        } else {
            // Sem mineração: o que você corrigiu, com prioridade para o recente.
            memoria
                .amostrar(n_ep, cfg.fracao_recente, &mut rng)
                .iter()
                .enumerate()
                .filter_map(|(i, e)| como_exemplo(e, i))
                .collect()
        };
        // Parte de replay: o que ela já sabia.
        while lote.len() < cfg.batch {
            let i = (rng.uniform01() * base.len() as f64) as usize % base.len();
            lote.push(base[i].clone());
        }

        let refs: Vec<&Exemplo> = lote.iter().collect();
        let Some((bytes, plano, alvos)) = montar_lote(&refs, patcher, cfg.seq) else {
            continue;
        };
        let est = ag.modelo.estado_zero(refs.len());
        grad.clear();
        ag.compreender(ops, &bytes, &plano, &est, &alvos, &mut treino, Some(&mut grad));
        let fatias = grad.slices();
        let mut params = ag.params_mut();
        adam.passo(&mut params, &fatias, cfg.clip);
    }

    let (_, depois) = avaliar(ag, ops, patcher, validacao, cfg.seq, cfg.batch, &mut cache);
    RelatorioConsolidacao {
        passos: cfg.passos,
        episodios_usados: episodios.len(),
        antes,
        depois,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Paralelo`, e nao `Scalar`.
    //
    // O `Scalar` e o ORACULO — existe para ser referencia de correcao, e ha teste em
    // `backend::parallel` provando que os dois dao resultado bit a bit identico.
    // Usa-lo para TREINAR num teste custou a suite inteira: de 1,3s para 41 min.
    use crate::backend::Paralelo;
    use crate::learn::dados::gerar;
    use crate::learn::supervisionado::{treinar_agente, CfgSup};
    use crate::model::hierarchy::Config;
    use crate::model::patcher::PorPalavra;
    use crate::tools::Registro;

    /// Treina de leve para haver exemplo facil e dificil de verdade.
    fn agente_meio_treinado() -> (Agente<f32>, PorPalavra, Vec<Exemplo>) {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut r = Rng::new(3);
        let mut ag = Agente::<f32>::novo(Config::minusculo(), reg.clone(), &mut r);
        let ops = Paralelo::auto();
        let mut g = Rng::new(11);
        let exs = gerar(&reg, &patcher, 500, &mut g);
        let cfg = CfgSup {
            seq: 64,
            batch: 8,
            epocas: 3,
            ..Default::default()
        };
        let _ = treinar_agente(&mut ag, &ops, &patcher, &exs[..400], &exs[400..], &cfg);
        (ag, patcher, exs)
    }

    /// Espelha a implementacao: o que nao monta lote vale NEG_INFINITY, e nao
    /// explode. Um exemplo longo demais para `seq` existe entre os gerados, e a
    /// primeira versao deste ajudante fazia `unwrap` nele.
    fn perda_de(ag: &Agente<f32>, patcher: &PorPalavra, e: &Exemplo) -> f64 {
        let ops = Paralelo::auto();
        let mut c = AgenteCache::new();
        let refs = [e];
        let Some((b, p, a)) = montar_lote(&refs, patcher, 64) else {
            return f64::NEG_INFINITY;
        };
        let est = ag.modelo.estado_zero(1);
        let v = ag.compreender(&ops, &b, &p, &est, &a, &mut c, None).0;
        if v.is_finite() { v } else { f64::NEG_INFINITY }
    }

    /// A mineracao tem de por o DIFICIL na frente, nao so mexer na ordem.
    ///
    /// O erro classico aqui e inverter o `partial_cmp` e treinar no que ela ja
    /// acerta — que passaria despercebido, porque a consolidacao continuaria
    /// "funcionando" e so pararia de render.
    #[test]
    fn a_mineracao_ordena_pelo_que_ela_erra() {
        let (ag, patcher, exs) = agente_meio_treinado();
        let alvo: Vec<Exemplo> = exs[..40].to_vec();
        let ops = Paralelo::auto();
        let ordem = ordenar_por_dificuldade(&ag, &ops, &patcher, &alvo, 64);
        assert_eq!(ordem.len(), alvo.len(), "perdeu exemplo na ordenacao");

        let primeiro = perda_de(&ag, &patcher, &alvo[ordem[0]]);
        let ultimo = perda_de(&ag, &patcher, &alvo[ordem[ordem.len() - 1]]);
        assert!(
            primeiro > ultimo,
            "ordem invertida ou tudo igual: primeiro {primeiro:.4}, ultimo {ultimo:.4}"
        );

        // A ordem inteira tem de ser decrescente, nao so as pontas.
        let perdas: Vec<f64> = ordem.iter().map(|&i| perda_de(&ag, &patcher, &alvo[i])).collect();
        for par in perdas.windows(2) {
            assert!(
                par[0] >= par[1] - 1e-9,
                "ordem quebrada: {:.4} antes de {:.4}",
                par[0],
                par[1]
            );
        }
    }

    /// Medir dificuldade NAO pode mexer nos pesos.
    #[test]
    fn ordenar_nao_altera_o_modelo() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut r = Rng::new(4);
        let mut ag = Agente::<f32>::novo(Config::minusculo(), reg.clone(), &mut r);
        let ops = Paralelo::auto();
        let mut g = Rng::new(12);
        let exs = gerar(&reg, &patcher, 60, &mut g);

        let antes: Vec<f32> = ag.params_mut().iter().flat_map(|p| p.to_vec()).collect();
        let _ = ordenar_por_dificuldade(&ag, &ops, &patcher, &exs, 64);
        let depois: Vec<f32> = ag.params_mut().iter().flat_map(|p| p.to_vec()).collect();
        assert_eq!(antes, depois, "medir dificuldade mexeu no modelo");
    }

    /// Exemplo que quebra a passagem de frente vai para o FIM, nao para o comeco.
    ///
    /// Se NaN ordenasse como "o mais dificil", a consolidacao comecaria por ele e
    /// espalharia NaN pelos pesos no primeiro passo. E o Achado 7 — `NaN` nunca e
    /// maior que nada, entao `partial_cmp` devolve `None` e o desempate decide.
    #[test]
    fn exemplo_quebrado_nao_vira_o_mais_dificil() {
        let (ag, patcher, exs) = agente_meio_treinado();
        let mut alvo: Vec<Exemplo> = exs[..10].to_vec();
        // Um pedido vazio nao monta lote: `ordenar` o ignora e ele nao entra.
        alvo.push(Exemplo {
            pedido: String::new(),
            ferramenta: 0,
            args: vec![],
            frase: (usize::MAX, 999),
        });
        let ops = Paralelo::auto();
        let ordem = ordenar_por_dificuldade(&ag, &ops, &patcher, &alvo, 64);
        assert!(
            !ordem.is_empty() && ordem[0] != alvo.len() - 1,
            "o exemplo quebrado foi para a frente da fila"
        );
    }
}
