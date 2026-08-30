//! Estado afetivo: três botões que hoje são constante chutada.
//!
//! Portado do `affect.rs` da nila_mind. Ele tem 540 linhas; vieram **três funções**
//! e a mecânica que as alimenta. O que ficou de fora e por quê está no fim deste
//! comentário — a parte que não veio importa tanto quanto a que veio.
//!
//! ## O que isto é
//!
//! Seis escalares em 0..1 que se atualizam a partir de dois números por tick:
//! **surpresa** (bits/byte do que ela acabou de ver) e **recompensa** (progresso).
//! Deles saem três moduladores:
//!
//! ```text
//! temperatura()   quão ousado é o devaneio do pulso
//! exploracao()    quanto o reforço experimenta em vez de repetir
//! lr_escala()     quanto a consolidação pode mexer nos pesos
//! ```
//!
//! Os três hoje são constante fixa na Teka: `0.9` de temperatura no pulso, épsilon
//! fixo no reforço, `2e-4` de taxa na consolidação.
//!
//! ## Por que não é enfeite
//!
//! A curiosidade sobe quando a surpresa é **média e há progresso** — nem tudo
//! previsto, nem impossível:
//!
//! ```text
//! curiosidade = novidade · (1 − |novidade − 0,55|·0,8) · (0,3 + aprendibilidade·0,7)
//! ```
//!
//! O pico em 0,55 é a zona onde há o que aprender. Isso é motivação por progresso de
//! aprendizado, e é o que faz o laço parar de martelar o que já sabe (tédio sobe,
//! plasticidade cai) e insistir onde está rendendo.
//!
//! ## A adaptação que o número cru exigia
//!
//! A nila_mind mede surpresa em **nats** (`SURPRISE_MAX = 5,545 = ln 256`); a Teka
//! mede em **bits/byte** (`log2 256 = 8`). Copiar o 5,545 daria um limiar 44% errado
//! — a mente acharia tudo surpreendente e ficaria em curiosidade máxima para sempre.
//!
//! O limiar de tédio sobrevive intacto por acaso: ele é "1 nat sobre o máximo", e
//! 1 nat / 5,545 nats = 1,4427 bits / 8 bits = 0,180 nos dois.
//!
//! ## O que NÃO veio, e é decisão, não esquecimento
//!
//! `shape_reward()` soma recompensa por **estar num estado**:
//!
//! ```text
//! moldada = base + curiosidade·0,10 + satisfacao·0,05 − frustracao·0,10 − tedio·0,05
//! ```
//!
//! Na nila_mind a ação é "como estudar" e o risco é baixo. Na Teka a ação é **qual
//! ferramenta rodar na máquina de alguém**, e o Achado 10 deste projeto registra que
//! a tabela de recompensa original já tinha *reward hacking* embutido por moldar
//! assim. Uma política que ganha 0,10 por parecer curiosa aprende a parecer curiosa.
//!
//! Recompensa aqui continua vindo só do desfecho.

/// Surpresa máxima possível, em bits por byte: `log2(256)`.
const SURPRESA_MAX: f32 = 8.0;

/// Abaixo disto (normalizado) o mundo está previsível e o tédio acumula.
///
/// É "1 nat sobre o máximo" da origem, que dá o mesmo número em bits.
const SURPRESA_BAIXA: f32 = 0.180;

/// Onde a curiosidade é máxima. Nem trivial nem impossível.
const ZONA_CURIOSA: f32 = 0.55;
/// Meia-largura da zona: fora dela a curiosidade zera.
///
/// 0,45 faz a curiosidade morrer em surpresa 0,10 (previsível demais) e em 1,00
/// (impossível), com o pico no meio.
const LARGURA_ZONA: f32 = 0.45;

// Velocidades das médias móveis. Quanto menor, mais devagar o estado se move.
// Satisfação e tédio são os mais lentos de propósito: os dois descrevem tendência
// sustentada, e reagir rápido a eles seria confundir um tick ruim com um período.
const A_VALENCIA: f32 = 0.08;
const A_EXCITACAO: f32 = 0.12;
const A_CURIOSIDADE: f32 = 0.10;
const A_SATISFACAO: f32 = 0.06;
const A_FRUSTRACAO: f32 = 0.10;
const A_TEDIO: f32 = 0.04;

/// Ticks de fracasso contínuo para frustração máxima.
const TICKS_FRUSTRACAO: f32 = 50.0;
/// Ticks de mundo previsível para tédio máximo.
const TICKS_TEDIO: f32 = 100.0;

fn ema(atual: f32, alvo: f32, alfa: f32) -> f32 {
    (1.0 - alfa) * atual + alfa * alvo
}

#[derive(Clone, Copy, Debug)]
pub struct Afeto {
    /// −1..1. Como foi o desfecho recente.
    pub valencia: f32,
    /// 0..1. Intensidade do que está acontecendo.
    pub excitacao: f32,
    /// 0..1. Há algo aprendível agora?
    pub curiosidade: f32,
    /// 0..1. Progresso sustentado.
    pub satisfacao: f32,
    /// 0..1. Muita surpresa e nenhum avanço.
    pub frustracao: f32,
    /// 0..1. Nada novo há muito tempo.
    pub tedio: f32,

    surpresa_ema: f32,
    progresso_ema: f32,
    seguidas_sem_surpresa: u32,
    seguidas_sem_progresso: u32,
}

impl Default for Afeto {
    fn default() -> Self {
        Self::novo()
    }
}

impl Afeto {
    pub const fn novo() -> Self {
        Self {
            valencia: 0.0,
            excitacao: 0.3,
            // Começa curiosa: no primeiro tick ela não sabe nada, e o estado inicial
            // que descreve isso é curiosidade, não tédio.
            curiosidade: 0.5,
            satisfacao: 0.0,
            frustracao: 0.0,
            tedio: 0.0,
            surpresa_ema: 0.0,
            progresso_ema: 0.0,
            seguidas_sem_surpresa: 0,
            seguidas_sem_progresso: 0,
        }
    }

    /// Um tick de sentimento.
    ///
    /// `surpresa` em **bits por byte**; `recompensa` em −1..1, onde positivo é
    /// progresso.
    pub fn atualizar(&mut self, recompensa: f32, surpresa_bits: f32) {
        let s = (surpresa_bits / SURPRESA_MAX).clamp(0.0, 1.0);
        let r = recompensa.clamp(-1.0, 1.0);

        self.surpresa_ema = ema(self.surpresa_ema, surpresa_bits, A_CURIOSIDADE);
        self.progresso_ema = ema(self.progresso_ema, r, A_SATISFACAO);

        // Sequências. Sobem de 1 em 1 e caem mais rápido: qualquer novidade quebra o
        // tédio, qualquer avanço alivia a frustração. Assimétrico de propósito — sair
        // do estado tem de ser mais fácil que entrar.
        if s < SURPRESA_BAIXA + 0.05 {
            self.seguidas_sem_surpresa = self.seguidas_sem_surpresa.saturating_add(1);
        } else {
            self.seguidas_sem_surpresa = self.seguidas_sem_surpresa.saturating_sub(3);
        }
        if s > 0.50 && r < 0.005 {
            self.seguidas_sem_progresso = self.seguidas_sem_progresso.saturating_add(1);
        } else {
            self.seguidas_sem_progresso = self.seguidas_sem_progresso.saturating_sub(2);
        }

        self.valencia = ema(self.valencia, r, A_VALENCIA).clamp(-1.0, 1.0);
        self.excitacao =
            ema(self.excitacao, (r.abs() * 0.6 + s * 0.4).clamp(0.0, 1.0), A_EXCITACAO)
                .clamp(0.0, 1.0);

        // A zona: novidade vezes aprendibilidade, com pico em ZONA_CURIOSA.
        //
        // A formula da origem era `s * (1 - |s - 0,55|*0,8)` e o comentario dela diz
        // "curiosidade maxima em torno de surpresa 0.55". **Ela nao faz isso.**
        // Derivando o ramo direito, `f(s) = 1,44s - 0,8s^2`, o maximo cai em s = 0,9:
        //
        //     f(0,55) = 0,550      f(0,90) = 0,648      f(1,00) = 0,640
        //
        // O `s` multiplicando por fora empurra o pico para cima, e a zona de
        // aprendizado acaba em quase-caos — justamente a regiao que o proprio
        // comentario diz que deveria virar frustracao. O teste
        // `a_curiosidade_pica_no_meio_e_nao_nos_extremos` pegou.
        //
        // Aqui e um triangulo que pica onde esta escrito e zera nas pontas.
        let aprendivel = r.clamp(0.0, 1.0);
        let dist = (s - ZONA_CURIOSA).abs() / LARGURA_ZONA;
        let novidade_util = (1.0 - dist).max(0.0);
        let alvo = novidade_util * (0.3 + aprendivel * 0.7);
        self.curiosidade = ema(self.curiosidade, alvo.clamp(0.0, 1.0), A_CURIOSIDADE).clamp(0.0, 1.0);

        let alvo = if self.progresso_ema > 0.0 {
            (self.progresso_ema * 3.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.satisfacao = ema(self.satisfacao, alvo, A_SATISFACAO).clamp(0.0, 1.0);

        let alvo = (self.seguidas_sem_progresso as f32 / TICKS_FRUSTRACAO).clamp(0.0, 1.0) * s;
        self.frustracao = ema(self.frustracao, alvo, A_FRUSTRACAO).clamp(0.0, 1.0);

        let alvo = (self.seguidas_sem_surpresa as f32 / TICKS_TEDIO).clamp(0.0, 1.0) * (1.0 - s);
        self.tedio = ema(self.tedio, alvo, A_TEDIO).clamp(0.0, 1.0);
    }

    /// Temperatura do devaneio, 0,7 a 1,6.
    pub fn temperatura(&self) -> f64 {
        let t = 1.0 + self.excitacao * 0.35 + self.curiosidade * 0.25 + self.tedio * 0.20
            - self.satisfacao * 0.30
            - (1.0 - self.excitacao) * 0.15;
        t.clamp(0.7, 1.6) as f64
    }

    /// Épsilon de exploração do reforço, 0,02 a 0,5.
    ///
    /// Frustração e tédio empurram exploração pelo mesmo motivo por caminhos
    /// diferentes: os dois dizem "o que está sendo feito não está rendendo".
    pub fn exploracao(&self) -> f64 {
        let e = self.curiosidade * 0.40 + self.frustracao * 0.30 + self.tedio * 0.25
            - self.satisfacao * 0.20
            + 0.05;
        e.clamp(0.02, 0.5) as f64
    }

    /// Multiplicador da taxa de aprendizado, 0,5 a 2,0.
    ///
    /// Frustração **reduz**: quando nada está funcionando, mexer mais nos pesos
    /// costuma piorar. É o "parar e respirar".
    pub fn lr_escala(&self) -> f64 {
        let l = 1.0 + self.curiosidade * 0.7 + self.excitacao * 0.5
            - self.frustracao * 0.8
            - self.tedio * 0.3
            - self.satisfacao * 0.2;
        l.clamp(0.5, 2.0) as f64
    }

    /// Uma linha para o log do pulso.
    pub fn resumo(&self) -> String {
        format!(
            "val{:+.2} exc{:.2} cur{:.2} fru{:.2} sat{:.2} ted{:.2}",
            self.valencia, self.excitacao, self.curiosidade, self.frustracao,
            self.satisfacao, self.tedio
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Surpresa media COM progresso e a zona de aprender.
    #[test]
    fn a_curiosidade_pica_no_meio_e_nao_nos_extremos() {
        let media = |s: f32| {
            let mut a = Afeto::novo();
            for _ in 0..60 {
                a.atualizar(0.5, s);
            }
            a.curiosidade
        };
        // 4,4 bits/byte = 0,55 normalizado, o pico.
        let pico = media(0.55 * SURPRESA_MAX);
        let previsivel = media(0.05 * SURPRESA_MAX);
        let impossivel = media(1.0 * SURPRESA_MAX);
        assert!(
            pico > previsivel && pico > impossivel,
            "a zona nao e uma zona: pico {pico:.3}, previsivel {previsivel:.3}, \\
             impossivel {impossivel:.3}"
        );
    }

    /// Surpresa alta SEM progresso nao e curiosidade, e frustracao.
    #[test]
    fn surpresa_sem_progresso_vira_frustracao_nao_curiosidade() {
        let mut a = Afeto::novo();
        for _ in 0..120 {
            a.atualizar(0.0, 0.9 * SURPRESA_MAX);
        }
        assert!(a.frustracao > 0.3, "frustracao ficou em {:.3}", a.frustracao);
        assert!(
            a.frustracao > a.curiosidade,
            "confundiu impasse com curiosidade: fru {:.3} cur {:.3}",
            a.frustracao,
            a.curiosidade
        );
        // E o efeito que importa: parar de mexer nos pesos quando nada funciona.
        assert!(a.lr_escala() < 1.0, "lr subiu no impasse: {:.3}", a.lr_escala());
    }

    /// Mundo previsivel por muito tempo vira tedio, e tedio empurra exploracao.
    #[test]
    fn mundo_previsivel_vira_tedio_e_tedio_explora() {
        let mut a = Afeto::novo();
        let calma = {
            for _ in 0..300 {
                a.atualizar(0.0, 0.05 * SURPRESA_MAX);
            }
            a.exploracao()
        };
        assert!(a.tedio > 0.3, "tedio ficou em {:.3}", a.tedio);
        let mut b = Afeto::novo();
        for _ in 0..300 {
            b.atualizar(0.5, 0.55 * SURPRESA_MAX);
        }
        assert!(
            calma > 0.02,
            "tedio nao empurrou exploracao nenhuma: {calma:.3}"
        );
        assert!(b.tedio < a.tedio, "mundo variado entediou mais que mundo parado");
    }

    /// Sair de um estado tem de ser mais facil que entrar.
    #[test]
    fn a_novidade_quebra_o_tedio_mais_rapido_do_que_ele_acumula() {
        let mut a = Afeto::novo();
        for _ in 0..300 {
            a.atualizar(0.0, 0.05 * SURPRESA_MAX);
        }
        let pico = a.tedio;
        // Um terco do tempo de novidade tem de derrubar o tedio bem abaixo do pico.
        for _ in 0..100 {
            a.atualizar(0.3, 0.6 * SURPRESA_MAX);
        }
        assert!(
            a.tedio < pico * 0.5,
            "tedio grudou: pico {pico:.3}, depois de novidade {:.3}",
            a.tedio
        );
    }

    /// Os tres moduladores tem de respeitar as faixas, sempre.
    #[test]
    fn os_moduladores_nunca_saem_da_faixa() {
        // Varre combinacoes extremas de recompensa e surpresa.
        for r in [-1.0f32, -0.5, 0.0, 0.5, 1.0, 9.0, -9.0] {
            for s in [0.0f32, 1.0, 4.0, 8.0, 99.0] {
                let mut a = Afeto::novo();
                for _ in 0..200 {
                    a.atualizar(r, s);
                }
                let (t, e, l) = (a.temperatura(), a.exploracao(), a.lr_escala());
                assert!((0.7..=1.6).contains(&t), "temperatura {t} fora da faixa (r={r} s={s})");
                // A tolerancia existe porque o clamp e em f32 e a leitura em f64:
                // `0.02f32 as f64` da 0,019999999552965164.
                assert!(
                    (0.02 - 1e-6..=0.5 + 1e-6).contains(&e),
                    "exploracao {e} fora da faixa (r={r} s={s})"
                );
                assert!((0.5..=2.0).contains(&l), "lr_escala {l} fora da faixa (r={r} s={s})");
                for v in [a.curiosidade, a.satisfacao, a.frustracao, a.tedio, a.excitacao] {
                    assert!((0.0..=1.0).contains(&v), "estado {v} fora de 0..1");
                }
                assert!((-1.0..=1.0).contains(&a.valencia));
            }
        }
    }

    /// A escala e em BITS, nao em nats. Copiar o 5,545 da origem daria um limiar 44%
    /// errado, e a mente acharia tudo surpreendente para sempre.
    #[test]
    fn a_surpresa_e_medida_em_bits() {
        assert_eq!(SURPRESA_MAX, 8.0, "log2(256) = 8; 5,545 e ln(256), em nats");
        // 1 nat = 1,4427 bits. Normalizado pelo maximo de cada escala, da o mesmo.
        let em_bits = 1.442_695_f32 / 8.0;
        assert!(
            (em_bits - SURPRESA_BAIXA).abs() < 0.005,
            "o limiar de tedio nao bate: {em_bits:.4} contra {SURPRESA_BAIXA}"
        );
    }
}
