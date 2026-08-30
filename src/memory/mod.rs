//! Memória episódica — o que faz a Teka aprender com o uso real.
//!
//! Cada interação vira um episódio: o pedido em bytes, o que ela decidiu, o que
//! aconteceu, e o que você disse depois. Guardado junto vai a **assinatura**: o
//! estado do backbone no último patch, que é a leitura que ela fez do pedido.
//!
//! ## Uma estrutura, duas funções
//!
//! A mesma lista serve como **memória de longo prazo** (buscar por similaridade o
//! que já aconteceu antes) e como ***replay buffer*** (amostrar lotes para
//! consolidar os pesos). Não é economia de código — é o desenho: consolidar a
//! partir de lotes que misturam episódios novos e antigos é exatamente o que
//! impede o esquecimento catastrófico.
//!
//! ## Três escalas de tempo
//!
//! | escala | onde mora | muda quando |
//! |---|---|---|
//! | agora | estado do SSM | a cada byte |
//! | episódico | esta memória | a cada interação |
//! | pesos | o modelo | na consolidação ("sono") |
//!
//! Treinar continuamente só no que acabou de chegar sobrescreve o que foi aprendido
//! semana passada. É o modo de falha que derrubaria um agente que "aprende com o
//! tempo", e o motivo desta estrutura existir antes de qualquer treino online.

pub mod contexto;
pub mod io;
pub mod semantica;

use crate::model::heads::MAX_SLOTS;
use crate::rng::Rng;

/// O que o usuário disse sobre a decisão.
#[derive(Clone, Debug, PartialEq)]
pub enum Feedback {
    /// Ninguém falou nada. Não dá pra supervisionar — vira material da fase 4 (RL).
    Nenhum,
    /// "Isso mesmo."
    Aprovado,
    /// "Não, era esta ferramenta, e o argumento é este trecho."
    Corrigido {
        ferramenta: usize,
        /// `(slot, (byte_inicial, byte_final_exclusivo))` no texto do pedido.
        args: Vec<(usize, (usize, usize))>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resultado {
    Executou,
    Falhou,
    NaoTentou,
}

#[derive(Clone, Debug)]
pub struct Episodio {
    pub pedido: String,
    /// A ferramenta que ela escolheu (não necessariamente a certa).
    pub ferramenta: usize,
    pub args: Vec<(usize, (usize, usize))>,
    pub resultado: Resultado,
    pub feedback: Feedback,
    pub quando: u64,
    /// Estado do backbone no último patch — a leitura que ela fez do pedido.
    /// É por aqui que se busca "o que já aconteceu parecido com isto".
    pub assinatura: Vec<f32>,
    /// Quantos episódios este representa depois da destilação.
    pub peso: u32,
}

impl Episodio {
    /// A decisão que se quer ensinar: a correção, se houve; senão o que ela fez.
    pub fn alvo_supervisionado(&self) -> Option<(usize, Vec<(usize, (usize, usize))>)> {
        match &self.feedback {
            Feedback::Corrigido { ferramenta, args } => Some((*ferramenta, args.clone())),
            Feedback::Aprovado => Some((self.ferramenta, self.args.clone())),
            Feedback::Nenhum => None,
        }
    }

    pub fn ensinavel(&self) -> bool {
        self.alvo_supervisionado().is_some()
    }
}

#[derive(Clone, Debug, Default)]
pub struct MemoriaEpisodica {
    pub episodios: Vec<Episodio>,
    proximo_id: u64,
}

impl MemoriaEpisodica {
    pub fn nova() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.episodios.len()
    }

    pub fn is_empty(&self) -> bool {
        self.episodios.is_empty()
    }

    /// Quantos episódios têm supervisão utilizável.
    pub fn n_ensinaveis(&self) -> usize {
        self.episodios.iter().filter(|e| e.ensinavel()).count()
    }

    /// Quantas vezes este pedido já foi tentado.
    ///
    /// É o contador que governa a exploração: um pedido novo merece ser
    /// experimentado, um pedido já tentado dez vezes merece a melhor resposta
    /// conhecida. Contar por PEDIDO, e não globalmente, é o que evita ficar
    /// explorando algo que já se sabe enquanto outra coisa nunca foi tentada.
    pub fn tentativas(&self, pedido: &str) -> usize {
        self.episodios.iter().filter(|e| e.pedido == pedido).count()
    }

    /// Reposiciona o contador de `quando` — usado ao carregar do disco, para que
    /// episódios novos não nasçam com carimbo repetido.
    pub fn recomecar_contador(&mut self, proximo: u64) {
        self.proximo_id = proximo;
    }

    pub fn gravar(
        &mut self,
        pedido: &str,
        ferramenta: usize,
        args: Vec<(usize, (usize, usize))>,
        resultado: Resultado,
        feedback: Feedback,
        assinatura: Vec<f32>,
    ) -> u64 {
        let id = self.proximo_id;
        self.proximo_id += 1;
        self.episodios.push(Episodio {
            pedido: pedido.to_string(),
            ferramenta,
            args,
            resultado,
            feedback,
            quando: id,
            assinatura,
            peso: 1,
        });
        id
    }

    /// Anota feedback no episódio mais recente que ainda não tem nenhum.
    ///
    /// É o fluxo natural de conversa: ela responde, você diz "não, era outra", e a
    /// correção precisa achar sozinha a que decisão você se referia.
    pub fn anotar_ultimo(&mut self, feedback: Feedback) -> bool {
        for ep in self.episodios.iter_mut().rev() {
            if ep.feedback == Feedback::Nenhum {
                ep.feedback = feedback;
                return true;
            }
        }
        false
    }

    /// Os `k` episódios mais parecidos com esta assinatura, por cosseno.
    pub fn parecidos(&self, assinatura: &[f32], k: usize) -> Vec<(&Episodio, f32)> {
        let mut pontuados: Vec<(&Episodio, f32)> = self
            .episodios
            .iter()
            .map(|e| (e, cosseno(&e.assinatura, assinatura)))
            .collect();
        pontuados.sort_by(|a, b| b.1.total_cmp(&a.1));
        pontuados.truncate(k);
        pontuados
    }

    /// Amostra episódios ensináveis para um lote de consolidação.
    ///
    /// Os mais recentes entram com prioridade — é o que a Teka acabou de aprender e
    /// ainda não fixou — mas o resto vem sorteado do histórico inteiro, senão a
    /// consolidação vira treino só no que chegou por último.
    pub fn amostrar(&self, n: usize, fracao_recente: f64, rng: &mut Rng) -> Vec<&Episodio> {
        let ensinaveis: Vec<&Episodio> = self.episodios.iter().filter(|e| e.ensinavel()).collect();
        if ensinaveis.is_empty() {
            return Vec::new();
        }
        let n_recente = ((n as f64) * fracao_recente) as usize;
        let mut saida = Vec::with_capacity(n);
        for e in ensinaveis.iter().rev().take(n_recente) {
            saida.push(*e);
        }
        while saida.len() < n {
            let i = (rng.uniform01() * ensinaveis.len() as f64) as usize % ensinaveis.len();
            saida.push(ensinaveis[i]);
        }
        saida
    }

    /// Colapsa episódios quase idênticos num só, somando o peso.
    ///
    /// É a **compressão semântica** do §6 da arquitetura: cinquenta pedidos de "ver
    /// espaço em disco" viram um padrão com peso 50, não cinquenta cópias. Resolve
    /// dois problemas de uma vez — a memória para de inchar, e o replay deixa de ser
    /// dominado por repetição.
    ///
    /// Só colapsa quando a decisão ensinada é a MESMA. Dois pedidos parecidos com
    /// decisões diferentes são justamente o par que a Teka mais precisa ver.
    pub fn destilar(&mut self, limiar: f32) -> usize {
        let antes = self.episodios.len();
        let mut mantidos: Vec<Episodio> = Vec::with_capacity(antes);

        for ep in std::mem::take(&mut self.episodios) {
            let alvo = ep.alvo_supervisionado();
            let achou = mantidos.iter_mut().find(|m| {
                m.alvo_supervisionado() == alvo
                    && cosseno(&m.assinatura, &ep.assinatura) >= limiar
            });
            match achou {
                Some(m) => {
                    m.peso += ep.peso;
                    // Mantém o mais recente como representante: a fraseologia muda
                    // com o tempo, e o protótipo deve acompanhar.
                    m.quando = m.quando.max(ep.quando);
                }
                None => mantidos.push(ep),
            }
        }
        self.episodios = mantidos;
        antes - self.episodios.len()
    }
}

pub fn cosseno(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        ab += a[i] * b[i];
        aa += a[i] * a[i];
        bb += b[i] * b[i];
    }
    if aa <= 0.0 || bb <= 0.0 {
        return 0.0;
    }
    ab / (aa.sqrt() * bb.sqrt())
}

/// Converte um episódio ensinável no formato que o treino supervisionado consome.
pub fn como_exemplo(ep: &Episodio, indice: usize) -> Option<crate::learn::dados::Exemplo> {
    let (ferramenta, args) = ep.alvo_supervisionado()?;
    if args.iter().any(|&(slot, _)| slot >= MAX_SLOTS) {
        return None;
    }
    Some(crate::learn::dados::Exemplo {
        pedido: ep.pedido.clone(),
        ferramenta,
        args,
        // Identidade de frase reservada aos episódios: `usize::MAX` no primeiro
        // campo garante que nunca colida com um molde do gerador, então a divisão
        // treino/validação por frase continua válida.
        frase: (usize::MAX, indice),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(pedido: &str, ferr: usize, assin: Vec<f32>, fb: Feedback) -> Episodio {
        Episodio {
            pedido: pedido.into(),
            ferramenta: ferr,
            args: vec![],
            resultado: Resultado::Executou,
            feedback: fb,
            quando: 0,
            assinatura: assin,
            peso: 1,
        }
    }

    #[test]
    fn cosseno_se_comporta() {
        assert!((cosseno(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosseno(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!((cosseno(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        assert_eq!(cosseno(&[], &[]), 0.0);
        assert_eq!(cosseno(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosseno(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn recupera_o_mais_parecido() {
        let mut m = MemoriaEpisodica::nova();
        m.gravar("a", 0, vec![], Resultado::Executou, Feedback::Aprovado, vec![1.0, 0.0]);
        m.gravar("b", 1, vec![], Resultado::Executou, Feedback::Aprovado, vec![0.0, 1.0]);
        m.gravar("c", 2, vec![], Resultado::Executou, Feedback::Aprovado, vec![0.9, 0.1]);

        let top = m.parecidos(&[1.0, 0.05], 2);
        assert_eq!(top.len(), 2);
        assert!(top[0].0.pedido == "a" || top[0].0.pedido == "c");
        assert_eq!(top[1].0.pedido == "b", false);
    }

    #[test]
    fn feedback_vai_para_a_decisao_mais_recente_sem_resposta() {
        let mut m = MemoriaEpisodica::nova();
        m.gravar("um", 0, vec![], Resultado::Executou, Feedback::Aprovado, vec![1.0]);
        m.gravar("dois", 1, vec![], Resultado::Executou, Feedback::Nenhum, vec![1.0]);
        assert!(m.anotar_ultimo(Feedback::Corrigido {
            ferramenta: 5,
            args: vec![]
        }));
        assert_eq!(m.episodios[0].feedback, Feedback::Aprovado);
        assert!(matches!(
            m.episodios[1].feedback,
            Feedback::Corrigido { ferramenta: 5, .. }
        ));
        // Sem nenhum episodio pendente, nao ha o que anotar.
        assert!(!m.anotar_ultimo(Feedback::Aprovado));
    }

    #[test]
    fn destilar_colapsa_repeticao_mas_preserva_discordancia() {
        let mut m = MemoriaEpisodica::nova();
        // Trinta pedidos praticamente iguais, mesma decisao.
        for i in 0..30 {
            let d = i as f32 * 1e-4;
            m.episodios.push(ep("ver disco", 6, vec![1.0, d], Feedback::Aprovado));
        }
        // Um pedido parecidissimo, mas com decisao DIFERENTE: e o par mais
        // informativo que existe, nao pode sumir.
        m.episodios.push(ep("ver disco", 1, vec![1.0, 0.0], Feedback::Aprovado));

        let colapsados = m.destilar(0.999);
        assert_eq!(m.len(), 2, "deveria sobrar o prototipo e a discordancia");
        assert_eq!(colapsados, 29);
        let prot = m.episodios.iter().find(|e| e.ferramenta == 6).unwrap();
        assert_eq!(prot.peso, 30, "o peso tem que somar os colapsados");
        assert!(m.episodios.iter().any(|e| e.ferramenta == 1));
    }

    #[test]
    fn amostra_prioriza_o_recente_mas_nao_so_ele() {
        let mut m = MemoriaEpisodica::nova();
        for i in 0..50 {
            m.gravar(
                &format!("p{i}"),
                i % 9,
                vec![],
                Resultado::Executou,
                Feedback::Aprovado,
                vec![1.0],
            );
        }
        let mut rng = Rng::new(1);
        let lote = m.amostrar(20, 0.25, &mut rng);
        assert_eq!(lote.len(), 20);
        // Os 5 mais recentes (25% de 20) entram garantidos.
        assert!(lote.iter().take(5).all(|e| e.quando >= 45));
        // E o resto vem de todo o historico, nao so do fim.
        assert!(lote.iter().any(|e| e.quando < 40), "amostra ficou presa no fim");
    }

    #[test]
    fn episodio_sem_feedback_nao_e_ensinavel() {
        let e = ep("x", 0, vec![1.0], Feedback::Nenhum);
        assert!(!e.ensinavel());
        assert!(como_exemplo(&e, 0).is_none());

        let e = ep("x", 3, vec![1.0], Feedback::Aprovado);
        assert!(e.ensinavel());
        let ex = como_exemplo(&e, 7).unwrap();
        assert_eq!(ex.ferramenta, 3);
        assert_eq!(ex.frase.0, usize::MAX, "episodio nao pode colidir com molde");
    }
}
