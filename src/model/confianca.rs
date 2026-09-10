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
            // `auto`, e nao `valor`: quem decide se pergunta e o AUTO-CRITICO
            // ("acertei?"), nao o previsor de recompensa. Enquanto era uma cabeca
            // so, um laco de reforco reescrevia isto como estimativa de recompensa
            // e a abstencao passava a ler outra coisa.
            valor: cache.auto.get(b).map(|v| v.to_f64()).unwrap_or(0.0),
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

    /// Limiares tirados dos PRÓPRIOS valores, e não de uma grade escrita à mão.
    ///
    /// ## Por que isto existe
    ///
    /// A grade do crítico era `[-0,30 … 0,10]`, feita para o crítico do reforço, que
    /// prevê recompensa perto de zero. Quando o crítico passou a prever "vou
    /// acertar?", os valores foram para perto de 0,97 — **todos acima do maior
    /// candidato**. Nenhum limiar abstinha nada, e a tabela imprimia saldo `+0` em
    /// todas as sementes.
    ///
    /// Aquele zero não era resultado: era a régua não alcançando o objeto. E eu quase
    /// li como "a hipótese morreu".
    ///
    /// Grade fixa é uma suposição sobre a escala do sinal, escrita meses antes de o
    /// sinal existir. Quantis não supõem nada.
    pub fn candidatos_dos_dados(&self, n: usize) -> Vec<f64> {
        let mut todos: Vec<f64> = self
            .margens_certas
            .iter()
            .chain(self.margens_erradas.iter())
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        if todos.is_empty() || n == 0 {
            return Vec::new();
        }
        todos.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // PRIMEIRO CANDIDATO: ABAIXO DE TUDO — a opcao de NAO ABSTER.
        //
        // Sem ela a grade obriga a abster em alguma coisa, e o "melhor" limiar vira
        // o menos ruim em vez do melhor. Medido: o braco do critico deu saldo -4,17
        // contra +0 do base, e aquilo nao media o critico -- media a grade. O base
        // tinha a opcao (limiar abaixo de todos os valores) e o tratado nao.
        //
        // Nao abster tem saldo 0 por definicao: zero erro evitado, zero acerto
        // perdido. E o piso honesto de qualquer regra de abstencao, e toda
        // comparacao precisa dele dos dois lados.
        let mut fora = Vec::with_capacity(n + 1);
        let menor = todos[0];
        fora.push(menor - 1.0);
        for i in 1..=n {
            // Quantis internos: o 0 e o 1 nao servem de limiar (abstem nada ou tudo).
            let q = i as f64 / (n + 1) as f64;
            let idx = ((todos.len() - 1) as f64 * q).round() as usize;
            let v = todos[idx];
            if !fora.contains(&v) {
                fora.push(v);
            }
        }
        fora
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

    /// Sinal que NAO separa nao pode dar saldo negativo: nao abster e sempre opcao.
    ///
    /// Foi o defeito que invalidou o primario do passo 1 do critico. A grade por
    /// quantil comecava DENTRO da distribuicao, entao toda opcao abstinha em alguma
    /// coisa, e o melhor de um conjunto de escolhas ruins parece uma escolha ruim.
    #[test]
    fn nao_abster_e_sempre_uma_opcao() {
        let mut s = Separacao::default();
        // Sinal INVERTIDO de proposito: o erro pontua ALTO e o acerto pontua baixo.
        // Como `em` abstem no que fica ABAIXO do limiar, aqui abster sempre pega
        // acerto antes de pegar erro — nenhum limiar compensa.
        for i in 0..50 {
            s.anotar(true, 0.50 + i as f64 * 0.01);
        }
        for i in 0..10 {
            s.anotar(false, 1.20 + i as f64 * 0.01);
        }
        let cand = s.candidatos_dos_dados(7);
        let (limiar, saldo) = s.melhor_limiar(&cand);
        assert_eq!(saldo, 0, "com sinal invertido o melhor e nao abster, e nao {saldo}");
        assert!(limiar < 0.50, "o limiar de nao abster tem de ficar abaixo de tudo");
    }

    /// A grade sai dos dados, entao nao tem como ficar fora de escala.
    ///
    /// O caso que motivou: valores perto de 0,97 contra uma grade que ia ate 0,10.
    /// Saldo +0 em toda semente, e o zero era a regua nao alcancando o objeto.
    #[test]
    fn a_grade_acompanha_a_escala_do_sinal() {
        let mut s = Separacao::default();
        for _ in 0..90 {
            s.anotar(true, 0.976);
        }
        for _ in 0..10 {
            s.anotar(false, 0.964);
        }
        let cand = s.candidatos_dos_dados(7);
        assert!(!cand.is_empty(), "grade vazia");
        assert!(
            cand.iter().any(|c| *c > 0.96 && *c <= 0.976),
            "a grade {cand:?} nao alcanca os valores"
        );
        // E TEM DE OFERECER NAO ABSTER, senao o "melhor" limiar e so o menos ruim.
        let (_, saldo) = s.melhor_limiar(&cand);
        assert!(saldo >= 0, "sem a opcao de nao abster, o saldo virou {saldo}");
        // E o limiar entre os dois grupos separa mesmo: evita os 10 erros sem
        // perder nenhum acerto.
        let (evitados, perdidos) = s.em(0.970);
        assert_eq!((evitados, perdidos), (10, 0));
    }

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
