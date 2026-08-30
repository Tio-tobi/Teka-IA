//! Quando calar a boca: a Teka poder dizer "não entendi".
//!
//! Ideia tomada do bite3.0. O corpus de decisão dele tem 11.025 linhas, e **35% não
//! são "usar ferramenta"**: `ask_user` (1.464), `observe` (1.337), `wait` (1.003).
//! Perguntar é uma ação de primeira classe lá.
//!
//! A Teka **sempre** escolhia uma ferramenta. E é literalmente assim que ela erra:
//!
//! ```text
//! manda um whoami ai              → {"acao":"hora"}
//! o consumo de ram esta alto      → {"acao":"procurar_arquivo","nome":"alto"}
//! chama o ipconfig no terminal    → {"acao":"procurar_arquivo","nome":"g"}
//! ```
//!
//! Executar a coisa errada é pior que não executar nada — e num agente que escreve
//! arquivo e roda comando, é muito pior.
//!
//! Esta é também a coisa que um despacho por palavra-chave **não consegue ter**: um
//! `any(k in texto for k in [...])` ou casa ou não casa, não existe estar 40%
//! confiante. Ter uma política probabilística é o que dá o direito de duvidar.
//!
//! ## Por que não basta olhar a maior probabilidade
//!
//! Rede neural é otimista: erra com confiança alta. O que separa melhor é a
//! **margem** — quanto o primeiro colocado ganha do segundo. Estar 0,95 contra 0,04
//! é decisão; estar 0,45 contra 0,42 é sorteio, mesmo com 0,45 sendo alto.
//!
//! Nada aqui é calibrado por chute: o limiar sai da medição em
//! [`Confianca::separacao`], comparando a margem dos acertos com a dos erros.

use crate::num::Float;
use crate::model::heads::CabecasCache;

/// Leitura da certeza da Teka sobre um pedido.
#[derive(Clone, Copy, Debug)]
pub struct Confianca {
    /// Índice da ferramenta escolhida.
    pub ferramenta: usize,
    /// Probabilidade dela.
    pub p1: f64,
    /// Probabilidade da segunda colocada.
    pub p2: f64,
    /// Indice da segunda colocada.
    ///
    /// Guardar o indice, e nao so a probabilidade, e o que permite ao contexto de
    /// conversa saber o que ela IA fazer quando abstem por falta de objeto. Ver
    /// `crate::memory::contexto`.
    pub segunda: usize,
    /// Previsão do crítico para este pedido.
    pub valor: f64,
}

impl Confianca {
    /// Lê a confiança do cache, para o exemplo `b` do lote.
    pub fn ler<T: Float>(cache: &CabecasCache<T>, b: usize, n_ferramentas: usize) -> Self {
        let row = &cache.logits_int[b * n_ferramentas..(b + 1) * n_ferramentas];
        let maxi = row.iter().fold(f64::NEG_INFINITY, |m, v| m.max(v.to_f64()));
        let exps: Vec<f64> = row.iter().map(|v| (v.to_f64() - maxi).exp()).collect();
        let soma: f64 = exps.iter().sum();

        let (mut i1, mut i2) = (0usize, 0usize);
        let (mut p1, mut p2) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (i, e) in exps.iter().enumerate() {
            let p = e / soma;
            if p > p1 {
                p2 = p1;
                i2 = i1;
                p1 = p;
                i1 = i;
            } else if p > p2 {
                p2 = p;
                i2 = i;
            }
        }
        Self {
            ferramenta: i1,
            p1,
            p2: if p2.is_finite() { p2 } else { 0.0 },
            segunda: i2,
            valor: cache.valor.get(b).map(|v| v.to_f64()).unwrap_or(0.0),
        }
    }

    /// Quanto o primeiro ganha do segundo.
    pub fn margem(&self) -> f64 {
        self.p1 - self.p2
    }

    /// Deve perguntar em vez de agir?
    pub fn duvidosa(&self, limiar: f64) -> bool {
        self.margem() < limiar
    }
}

/// Como a margem separa acertos de erros num conjunto medido.
///
/// É isto que justifica um limiar. Se as duas distribuições se sobrepõem
/// completamente, **não existe limiar bom** e abster só troca erro por silêncio —
/// e é melhor saber disso do que escolher um número bonito.
#[derive(Debug, Default)]
pub struct Separacao {
    pub margens_certas: Vec<f64>,
    pub margens_erradas: Vec<f64>,
}

impl Separacao {
    pub fn anotar(&mut self, certo: bool, margem: f64) {
        if certo {
            self.margens_certas.push(margem);
        } else {
            self.margens_erradas.push(margem);
        }
    }

    /// Para um limiar: (erros evitados, acertos perdidos).
    ///
    /// Os dois números importam. Abster com limiar 1,0 evita 100% dos erros e
    /// destrói 100% dos acertos — um limiar só presta se o primeiro número for
    /// muito maior que o segundo.
    pub fn em(&self, limiar: f64) -> (usize, usize) {
        (
            self.margens_erradas.iter().filter(|m| **m < limiar).count(),
            self.margens_certas.iter().filter(|m| **m < limiar).count(),
        )
    }

    /// O melhor limiar dentre os candidatos, pelo saldo `erros evitados − acertos
    /// perdidos`.
    ///
    /// O empate é resolvido pelo **menor** limiar: entre duas regras que trocam a
    /// mesma coisa, a que fala menos "não sei" é a preferível.
    pub fn melhor_limiar(&self, candidatos: &[f64]) -> (f64, i64) {
        let mut melhor = (0.0f64, i64::MIN);
        for &l in candidatos {
            let (evitados, perdidos) = self.em(l);
            let saldo = evitados as i64 - perdidos as i64;
            if saldo > melhor.1 {
                melhor = (l, saldo);
            }
        }
        melhor
    }

    pub fn media(v: &[f64]) -> f64 {
        if v.is_empty() {
            return f64::NAN;
        }
        v.iter().sum::<f64>() / v.len() as f64
    }

    /// Tabela para o relatório.
    pub fn tabela(&self, candidatos: &[f64]) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "  margem media: acertos {:.3}  erros {:.3}\n\n",
            Self::media(&self.margens_certas),
            Self::media(&self.margens_erradas)
        ));
        s.push_str("  limiar   erros evitados   acertos perdidos   saldo\n");
        for &l in candidatos {
            let (e, p) = self.em(l);
            s.push_str(&format!(
                "  {l:<8.2} {e:>10}     {p:>14}   {:>6}\n",
                e as i64 - p as i64
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_margem_e_a_diferenca_para_o_segundo() {
        let c = Confianca {
            ferramenta: 0,
            p1: 0.45,
            p2: 0.42,
            segunda: 1,
            valor: 0.0,
        };
        assert!((c.margem() - 0.03).abs() < 1e-9);
        // Confiança alta com margem baixa continua sendo dúvida.
        assert!(c.duvidosa(0.10));

        let d = Confianca {
            ferramenta: 0,
            p1: 0.51,
            p2: 0.04,
            segunda: 1,
            valor: 0.0,
        };
        assert!(!d.duvidosa(0.10), "0,51 contra 0,04 e decisao, nao sorteio");
    }

    #[test]
    fn separacao_conta_evitados_e_perdidos() {
        let mut s = Separacao::default();
        s.anotar(true, 0.90);
        s.anotar(true, 0.80);
        s.anotar(true, 0.05); // acerto por pouco: um limiar alto o destroi
        s.anotar(false, 0.03);
        s.anotar(false, 0.10);

        assert_eq!(s.em(0.06), (1, 1));
        assert_eq!(s.em(0.20), (2, 1));
        // Limiar que pega tudo evita todos os erros e perde todos os acertos.
        assert_eq!(s.em(1.01), (2, 3));
    }

    #[test]
    fn melhor_limiar_maximiza_o_saldo_e_desempata_pelo_menor() {
        let mut s = Separacao::default();
        for _ in 0..5 {
            s.anotar(true, 0.90);
        }
        for _ in 0..3 {
            s.anotar(false, 0.05);
        }
        let (l, saldo) = s.melhor_limiar(&[0.02, 0.10, 0.50, 0.95]);
        assert_eq!(saldo, 3, "3 erros evitados, 0 acertos perdidos");
        assert!((l - 0.10).abs() < 1e-9, "esperava 0,10, veio {l}");
    }

    #[test]
    fn quando_nao_ha_separacao_o_saldo_nao_compensa() {
        // Erros e acertos com a mesma margem: nenhum limiar ajuda.
        let mut s = Separacao::default();
        for _ in 0..10 {
            s.anotar(true, 0.5);
            s.anotar(false, 0.5);
        }
        let (_, saldo) = s.melhor_limiar(&[0.1, 0.4, 0.6, 0.9]);
        assert!(saldo <= 0, "sem separacao, abster nao pode dar saldo positivo");
    }
}
