//! O laço: praticar, verificar, aprender. Sem ninguém olhando.
//!
//! ```text
//!   sorteia tarefa  →  Teka age (explorando)  →  executa no mundo isolado
//!         ▲                                              │
//!         │                                              ▼
//!    consolida ← treina por reforço ← grava episódio ← VERIFICA o efeito
//! ```
//!
//! ## De onde vem o sinal
//!
//! De [`Mundo::verificar`], que olha o **efeito** e não a chamada. Isso é o que
//! separa isto de supervisionado disfarçado: se ela conseguir o resultado por outro
//! caminho, conta como acerto.
//!
//! ## Por que a recompensa é assimétrica
//!
//! O [`crate::learn::reforco`] já registra o motivo (Achado 10): recompensar
//! sucesso ensina a chamar a ferramenta infalível para tudo. `hora` nunca falha.
//!
//! Aqui vale igual, e mais forte: no ambiente, `hora` **também acerta o critério
//! dela** com frequência, porque a saída tem dois-pontos. Se acertar rendesse
//! prêmio, o caminho mais curto para a recompensa máxima seria responder `hora`
//! sempre e ganhar 1/7 das vezes de graça.
//!
//! Então o acerto vale **zero** — é o ponto de comparação — e o erro custa. O que a
//! política aprende é a **fugir do que não funciona**, que é o mesmo mecanismo do
//! REPL, só que sem precisar do usuário para dizer.
//!
//! ## O laço não decide sozinho o que é "melhor"
//!
//! Ele mede a taxa de acerto no ambiente e o benchmark **não entra nisso**. São
//! coisas diferentes: o ambiente tem 7 famílias sorteadas; o benchmark tem 150
//! frases escritas à mão. Um modelo que fica ótimo no ambiente e pior no benchmark
//! decorou o ambiente — e o relatório mostra os dois para essa suspeita ser
//! visível em vez de silenciosa.

use std::path::Path;

use crate::ambiente::dinamico::MundoDinamico;
use crate::ambiente::Tarefa;
use crate::backend::Paralelo;
use crate::learn::dados::Exemplo;
use crate::learn::reforco::{treinar_por_reforco, CfgReforco, Recozimento};
use crate::memory::{Feedback, MemoriaEpisodica, Resultado};
use crate::model::agente::{Agente, AgenteCache};
use crate::model::patcher::Patcher;
use crate::rng::Rng;
use crate::tools::execucao::Executor;
use crate::tools::seguranca::{Modo, Politica};

pub struct CfgLaco {
    /// Tentativas antes de cada rodada de treino.
    pub tentativas_por_rodada: usize,
    pub rodadas: usize,
    /// Semente do sorteio de tarefas.
    pub semente: u64,
    pub reforco: CfgReforco,
    pub recozimento: Recozimento,
}

impl Default for CfgLaco {
    fn default() -> Self {
        Self {
            tentativas_por_rodada: 200,
            rodadas: 10,
            semente: 20260826,
            reforco: CfgReforco::default(),
            recozimento: Recozimento::default(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Rodada {
    pub tentativas: usize,
    pub acertos: usize,
    /// Acertos por família de tarefa, na ordem em que apareceram.
    pub por_familia: Vec<(&'static str, usize, usize)>,
    pub erro_critico: f64,
    pub recompensa_media: f64,
}

impl Rodada {
    pub fn taxa(&self) -> f64 {
        if self.tentativas == 0 {
            return f64::NAN;
        }
        self.acertos as f64 / self.tentativas as f64
    }
}

/// Uma rodada de prática: N tentativas no mundo, gravando episódios.
///
/// O mundo é **reconstruído** a cada rodada. Sem isso, os arquivos que ela escreveu
/// na rodada anterior ficariam lá e a tarefa "escreve X em saida.txt" passaria de
/// graça na segunda vez, ensinando exatamente a coisa errada.
#[allow(clippy::too_many_arguments)]
pub fn praticar(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &dyn Patcher,
    raiz: &Path,
    mem: &mut MemoriaEpisodica,
    cfg: &CfgLaco,
    exemplos: &[crate::learn::dados::Exemplo],
    rng: &mut Rng,
) -> std::io::Result<Rodada> {
    let mundo = MundoDinamico::novo(raiz)?;
    // Real, mas confinado à pasta do mundo. As guardas de `seguranca` continuam
    // valendo por cima: a pasta é conveniência, não é a proteção.
    let pol = Politica {
        modo: Modo::Real,
        raiz: Some(mundo.raiz().to_path_buf()),
        ..Default::default()
    };
    let mut exec = Executor::novo(mundo.raiz().join(".diario.log"), pol)?;
    let mut cache = AgenteCache::new();
    let mut r = Rodada::default();
    let mut fam: Vec<(&'static str, usize, usize)> = Vec::new();

    let mut tentadas = 0usize;
    // Percorre exemplos ate juntar as tentativas pedidas. Nem todo exemplo vira
    // tarefa — `memoria`, `disco` e argumento fora da caixa sao descartados.
    let mut i = (rng.uniform01() * exemplos.len() as f64) as usize % exemplos.len().max(1);
    while tentadas < cfg.tentativas_por_rodada {
        let ex = &exemplos[i % exemplos.len()];
        i += 1;
        mundo.limpar()?;
        let Some(t): Option<Tarefa> = mundo.preparar(&ag.registro, ex, rng)? else {
            continue;
        };
        tentadas += 1;
        // A temperatura cai com quantas vezes ESTE pedido já foi tentado: pedido
        // novo merece experimento, pedido velho merece a melhor resposta conhecida.
        let temp = cfg.recozimento.temperatura(mem.tentativas(&t.pedido));
        let chamada = ag.responder_explorando(ops, patcher, &t.pedido, temp, rng, &mut cache);

        let (ok, ferramenta, saida) = match &chamada {
            Ok(c) => {
                let s = exec.executar(&ag.registro, c);
                (
                    crate::ambiente::verificar_em(mundo.raiz(), &ag.registro, &t, c, &s),
                    c.ferramenta,
                    s,
                )
            }
            Err(e) => (false, 0usize, Err(e.clone())),
        };
        let assinatura = ag.assinatura(&cache);

        mem.gravar(
            &t.pedido,
            ferramenta,
            Vec::new(),
            // O ambiente sabe se deu certo, então o resultado é honesto — ao
            // contrário do REPL, onde "executou" só diz que não deu erro.
            if ok { Resultado::Executou } else { Resultado::Falhou },
            Feedback::Nenhum,
            assinatura,
        );
        let _ = saida;

        r.tentativas += 1;
        r.acertos += ok as usize;
        match fam.iter_mut().find(|(n, _, _)| *n == t.ferramenta_esperada) {
            Some((_, a, n)) => {
                *a += ok as usize;
                *n += 1;
            }
            None => fam.push((t.ferramenta_esperada, ok as usize, 1)),
        }
    }
    fam.sort_by_key(|(n, _, _)| *n);
    r.por_familia = fam;
    mundo.destruir()?;
    Ok(r)
}

/// O laço completo: pratica, treina por reforço, repete.
///
/// Devolve uma linha por rodada. O chamador decide o que fazer com o modelo —
/// inclusive descartá-lo, se o benchmark tiver piorado.
#[allow(clippy::too_many_arguments)]
pub fn rodar(
    ag: &mut Agente<f32>,
    ops: &Paralelo,
    patcher: &dyn Patcher,
    raiz: &Path,
    mem: &mut MemoriaEpisodica,
    base: &[Exemplo],
    validacao: &[Exemplo],
    cfg: &CfgLaco,
) -> std::io::Result<Vec<Rodada>> {
    let mut rng = Rng::new(cfg.semente);
    let mut saida = Vec::new();

    for n in 0..cfg.rodadas {
        let mut r = praticar(ag, ops, patcher, raiz, mem, cfg, base, &mut rng)?;

        // Só treina se houve sinal. Sem episódio com recompensa, o reforço roda em
        // falso e o único efeito é o replay supervisionado — que já aconteceu.
        let rel = treinar_por_reforco(ag, ops, patcher, mem, base, validacao, &cfg.reforco);
        r.erro_critico = rel.erro_critico;
        r.recompensa_media = rel.recompensa_media;

        println!(
            "  rodada {:>2}: {:>3}/{:<3} = {:>4.0}%   critico {:.3}   recompensa {:+.3}",
            n + 1,
            r.acertos,
            r.tentativas,
            r.taxa() * 100.0,
            r.erro_critico,
            r.recompensa_media
        );
        for (f, a, t) in &r.por_familia {
            print!("    {f} {a}/{t}");
        }
        println!();
        saida.push(r);
    }
    Ok(saida)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambiente::dinamico::MundoDinamico;
    use crate::learn::dados::gerar;
    use crate::model::hierarchy::Config;
    use crate::tools::Registro;

    fn temp(nome: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("teka_laco_{nome}"))
    }

    fn cenario() -> (Agente<f32>, Paralelo, crate::model::patcher::PorPalavra, Vec<Exemplo>) {
        let mut rng = Rng::new(1);
        let reg = Registro::padrao();
        let patcher = crate::model::patcher::PorPalavra::default();
        let exs = gerar(&reg, &patcher, 1500, &mut rng);
        let ag = Agente::<f32>::novo(Config::minusculo(), reg, &mut rng);
        (ag, Paralelo::new(2), patcher, exs)
    }

    #[test]
    fn praticar_grava_um_episodio_por_tentativa() {
        let (ag, ops, patcher, exs) = cenario();
        let mut mem = MemoriaEpisodica::nova();
        let cfg = CfgLaco {
            tentativas_por_rodada: 12,
            rodadas: 1,
            ..Default::default()
        };
        let raiz = temp("grava");
        let mut rng = Rng::new(5);
        let r = praticar(&ag, &ops, &patcher, &raiz, &mut mem, &cfg, &exs, &mut rng).unwrap();

        assert_eq!(r.tentativas, 12);
        assert_eq!(mem.len(), 12, "cada tentativa tem de virar episodio");
        assert!(!raiz.exists(), "o mundo ficou para tras");
    }

    #[test]
    fn a_variedade_de_frase_praticada_e_grande() {
        // O conserto do fracasso: a versao antiga praticava sobre 26 frases e o
        // benchmark caiu 13,7 de 150 em tres sementes. A fonte agora e a mesma do
        // treino supervisionado.
        let (ag, ops, patcher, exs) = cenario();
        let mut mem = MemoriaEpisodica::nova();
        let cfg = CfgLaco {
            tentativas_por_rodada: 150,
            rodadas: 1,
            ..Default::default()
        };
        let mut rng = Rng::new(6);
        praticar(&ag, &ops, &patcher, &temp("var"), &mut mem, &cfg, &exs, &mut rng).unwrap();
        let distintas: std::collections::HashSet<&str> =
            mem.episodios.iter().map(|e| e.pedido.as_str()).collect();
        assert!(
            distintas.len() > 100,
            "so {} frases distintas em 150 tentativas — sala de espelhos de novo",
            distintas.len()
        );
    }

    #[test]
    fn o_mundo_e_limpo_entre_exemplos() {
        // Sem isto, "escreve X em saida.txt" passaria de graca na proxima vez que
        // aparecesse, ensinando exatamente a coisa errada.
        let raiz = temp("limpo");
        let m = MundoDinamico::novo(&raiz).unwrap();
        std::fs::write(m.raiz().join("saida.txt"), "t1234").unwrap();
        m.limpar().unwrap();
        assert!(!m.raiz().join("saida.txt").exists());
        m.destruir().unwrap();
    }

    #[test]
    fn um_modelo_nao_treinado_nao_acerta_tudo() {
        // Sanidade da regua: se um modelo aleatorio pontuasse alto, o criterio
        // estaria frouxo demais para ensinar qualquer coisa.
        let (ag, ops, patcher, exs) = cenario();
        let mut mem = MemoriaEpisodica::nova();
        let cfg = CfgLaco {
            tentativas_por_rodada: 60,
            ..Default::default()
        };
        let mut rng = Rng::new(7);
        let r = praticar(&ag, &ops, &patcher, &temp("aleat"), &mut mem, &cfg, &exs, &mut rng)
            .unwrap();
        assert!(
            r.taxa() < 0.6,
            "modelo nao treinado acertou {:.0}% — criterio frouxo demais",
            r.taxa() * 100.0
        );
    }
}
