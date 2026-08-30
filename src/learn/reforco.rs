//! Aprendizado por reforço sobre uso real.
//!
//! A fase 3 só aprende de episódios com `/certo` ou `/errado`. Na prática você não
//! vai responder a maioria das vezes — e é justamente aí que mora quase todo o
//! sinal. Esta fase é sobre **aprender dos episódios sem resposta**.
//!
//! ## REINFORCE é entropia cruzada com peso
//!
//! ```text
//! entropia cruzada, alvo a:   ∇ −log π(a)
//! REINFORCE, ação a:          ∇ −(r − b)·log π(a)
//! ```
//!
//! O mesmo gradiente, escalado pela **vantagem**. Então não há caminho novo de
//! backward para escrever nem para provar: basta um peso por exemplo
//! ([`Alvo::reforco`]), e todo o percurso já verificado por diferenças finitas
//! continua sendo o único percurso. Vantagem negativa empurra para longe da ação
//! tomada — é assim que uma falha vira aprendizado **sem ninguém dizer qual era a
//! resposta certa**.
//!
//! ## O buraco na tabela de recompensa, e o conserto
//!
//! A tabela original do §8 dava recompensa positiva para "a ferramenta executou sem
//! erro". Isso é *reward hacking* construído de fábrica: `hora` **nunca** falha, e
//! `memoria` também não. Recompensar sucesso ensina a chamar as ferramentas
//! infalíveis para tudo.
//!
//! O conserto é assimetria:
//!
//! > **Falha é informativa. Sucesso não é.**
//!
//! `ler_arquivo` falhando num caminho inexistente diz que a escolha ou o argumento
//! estavam errados. `hora` executando não diz nada — ela executaria de qualquer
//! jeito. Então sucesso vale **zero**, e só o feedback explícito dá positivo.
//!
//! ## O sinal implícito
//!
//! Quando você reformula o pedido logo em seguida — mesma coisa, com outras
//! palavras —, isso é evidência de que a resposta anterior não serviu. Dá para
//! detectar por cosseno entre assinaturas de episódios consecutivos, sem você
//! precisar dizer nada. É o "resultado relevante" do §8, medido em vez de suposto.
//!
//! ## O baseline tem que ser POR PEDIDO
//!
//! Esta parte nasceu de um bug que só apareceu relendo o próprio teste. A primeira
//! versão usava uma média móvel global das recompensas. Como todas as transições do
//! teste eram falhas, a EMA convergiu para o valor da falha e **toda vantagem virou
//! zero** — o reforço não fez absolutamente nada, e o teste passou por acidente,
//! movido pelo replay supervisionado.
//!
//! REINFORCE aprende de diferenças **relativas**. Sem variância na recompensa, não
//! há o que comparar.
//!
//! A correção não é remover o baseline: é torná-lo **específico da situação**. A
//! primeira tentativa foi a média das recompensas daquele mesmo pedido, e ela
//! resolvia o caso do pedido repetido — mas num pedido visto uma vez a média é a
//! própria recompensa, a vantagem sai zero, e nada é aprendido. Foi essa limitação
//! que apareceu nos números do laço fechado, e é ela que o crítico resolve.
//!
//! ## E depois: o crítico
//!
//! A média por pedido só existe para pedidos **repetidos**. Num pedido visto uma
//! vez, a média é a própria recompensa, a vantagem é zero, e nada é aprendido — a
//! limitação que apareceu nos números do laço fechado.
//!
//! O crítico `V(s)` conserta isso porque é aprendido **a partir do estado**: um
//! pedido novo herda a estimativa dos pedidos parecidos que já aconteceram.
//!
//! ```text
//! vantagem = r − V(s)
//! ```
//!
//! Três propriedades que importam:
//!
//! 1. **Não anula nada no começo.** `V` nasce prevendo exatamente zero (a projeção
//!    é inicializada com escala 0), então no primeiro passo a vantagem é a própria
//!    recompensa. É o oposto da EMA global, que convergia para a média e zerava
//!    tudo.
//! 2. **Generaliza.** Pedido novo, parecido com outros já vistos, já nasce com
//!    baseline razoável.
//! 3. **Continua matando o reward hacking.** Sucesso vale zero e `V` aprende a
//!    prever zero onde só houve sucesso, então a vantagem fica em zero. A
//!    ferramenta infalível não acumula crédito.
//!
//! A vantagem é **destacada** do grafo: ela é calculada como número antes de virar
//! peso, então o gradiente da política não flui pelo crítico. Isso é essencial —
//! sem destacar, a política aprenderia a manipular o próprio baseline.

use crate::backend::Ops;
use crate::learn::adam::Adam;
use crate::learn::dados::Exemplo;
use crate::learn::supervisionado::{avaliar, montar_lote};
use crate::memory::{cosseno, como_exemplo, Episodio, Feedback, MemoriaEpisodica, Resultado};
use crate::model::agente::{Agente, AgenteCache};
use crate::model::heads::{Alvo, Placar};
use crate::model::patcher::{Patcher, Plano};
use crate::rng::Rng;

/// Pesos da recompensa. Ver a nota sobre assimetria no topo do módulo.
#[derive(Clone, Debug)]
pub struct Pesos {
    pub aprovado: f64,
    pub corrigido: f64,
    pub falhou: f64,
    /// Sucesso vale zero de propósito — não é evidência de acerto.
    pub executou: f64,
    /// Você reformulou o pedido logo depois: evidência implícita de que não serviu.
    pub reformulou: f64,
    /// Cosseno acima do qual dois pedidos consecutivos contam como reformulação.
    pub limiar_reformulacao: f32,
}

impl Default for Pesos {
    fn default() -> Self {
        Self {
            aprovado: 1.0,
            corrigido: -1.0,
            falhou: -0.5,
            executou: 0.0,
            reformulou: -0.4,
            limiar_reformulacao: 0.9,
        }
    }
}

/// Recompensa do episódio `i`, olhando o vizinho seguinte para o sinal implícito.
///
/// Devolve `None` quando não há sinal nenhum — não existe "recompensa zero" que
/// valha um passo de gradiente.
pub fn recompensa(mem: &MemoriaEpisodica, i: usize, p: &Pesos) -> Option<f64> {
    let ep = &mem.episodios[i];
    let mut r = 0.0;
    let mut houve_sinal = false;

    match &ep.feedback {
        Feedback::Aprovado => {
            r += p.aprovado;
            houve_sinal = true;
        }
        Feedback::Corrigido { .. } => {
            r += p.corrigido;
            houve_sinal = true;
        }
        Feedback::Nenhum => {}
    }
    match ep.resultado {
        Resultado::Falhou => {
            r += p.falhou;
            houve_sinal = true;
        }
        // Sucesso entra como transição valendo ZERO. Não é recompensa — é o ponto
        // de comparação sem o qual a falha não significa nada. Ver a nota sobre o
        // baseline por pedido.
        Resultado::Executou => {
            r += p.executou;
            houve_sinal = true;
        }
        Resultado::NaoTentou => {}
    }
    if reformulou(mem, i, p.limiar_reformulacao) {
        r += p.reformulou;
        houve_sinal = true;
    }
    houve_sinal.then_some(r)
}

/// O episódio seguinte é o mesmo pedido dito de outro jeito?
///
/// Exige decisão **diferente**: repetir o mesmo pedido e receber a mesma resposta
/// pode ser só você conferindo, não reclamando.
pub fn reformulou(mem: &MemoriaEpisodica, i: usize, limiar: f32) -> bool {
    let Some(prox) = mem.episodios.get(i + 1) else {
        return false;
    };
    let ep = &mem.episodios[i];
    prox.ferramenta != ep.ferramenta
        && cosseno(&ep.assinatura, &prox.assinatura) >= limiar
}

/// Média móvel exponencial das recompensas — usada só como diagnóstico.
///
/// O baseline que entra no gradiente é o crítico `V(s)`; este aqui serve para
/// relatar a tendência geral da recompensa ao longo do tempo.
#[derive(Clone, Debug)]
pub struct Baseline {
    pub valor: f64,
    pub taxa: f64,
    pub n: usize,
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            valor: 0.0,
            taxa: 0.02,
            n: 0,
        }
    }
}

impl Baseline {
    pub fn observar(&mut self, r: f64) {
        if self.n == 0 {
            self.valor = r;
        } else {
            self.valor += self.taxa * (r - self.valor);
        }
        self.n += 1;
    }
    pub fn vantagem(&self, r: f64) -> f64 {
        r - self.valor
    }
}

/// Recozimento da temperatura de exploração.
///
/// Sem isto a exploração fica fixa, e a amostragem continua quase uniforme mesmo
/// depois de a política já saber a resposta — ela desaprende o que acabou de
/// aprender. Medido: com `T = 1,0` fixo, o laço fechado estacionava em 5 de 6.
///
/// A temperatura cai com o número de tentativas **daquele pedido**, não com o
/// tempo. Um pedido novo merece ser experimentado mesmo que a Teka já esteja velha
/// de casa; um pedido tentado dez vezes merece a melhor resposta conhecida.
///
/// ```text
/// T(n) = t_final + (t_inicial − t_final) · exp(−n / escala)
/// ```
#[derive(Clone, Debug)]
pub struct Recozimento {
    pub t_inicial: f64,
    pub t_final: f64,
    /// Tentativas para a temperatura percorrer ~63% do caminho até o piso.
    pub escala: f64,
}

impl Default for Recozimento {
    fn default() -> Self {
        Self {
            t_inicial: 1.0,
            // Não vai a zero: um piso pequeno mantém a porta aberta para o mundo
            // mudar de ideia sobre qual ferramenta funciona.
            t_final: 0.15,
            escala: 8.0,
        }
    }
}

impl Recozimento {
    pub fn temperatura(&self, tentativas: usize) -> f64 {
        self.t_final
            + (self.t_inicial - self.t_final) * (-(tentativas as f64) / self.escala).exp()
    }
}

#[derive(Clone, Debug)]
pub struct CfgReforco {
    /// Passos de gradiente por época.
    pub passos: usize,
    /// Quantas vezes reestimar `V(s)` e recalcular as vantagens.
    ///
    /// `V` muda enquanto treina, então vantagens calculadas no início da época
    /// ficam obsoletas. Reestimar entre épocas é o mesmo padrão do PPO.
    pub epocas: usize,
    pub batch: usize,
    pub seq: usize,
    pub lr: f64,
    pub clip: f64,
    /// Fração do lote vinda do reforço; o resto é replay supervisionado.
    pub fracao_reforco: f64,
    /// Teto do valor absoluto da vantagem — um episódio isolado não pode dominar.
    pub teto_vantagem: f64,
    pub semente: u64,
    pub pesos: Pesos,
}

impl Default for CfgReforco {
    fn default() -> Self {
        Self {
            passos: 50,
            epocas: 3,
            batch: 16,
            seq: 64,
            // Ainda mais baixa que a consolidação: o gradiente de REINFORCE tem
            // variância alta, e aqui não há rótulo certo para corrigir um exagero.
            lr: 1e-4,
            clip: 1.0,
            fracao_reforco: 0.3,
            teto_vantagem: 2.0,
            semente: 909,
            pesos: Pesos::default(),
        }
    }
}

pub struct RelatorioReforco {
    pub passos: usize,
    pub transicoes: usize,
    /// Erro quadrático médio do crítico ao final — se não cair, ele não aprendeu.
    pub erro_critico: f64,
    /// Quantas transições têm vantagem diferente de zero — ou seja, quantas
    /// realmente movem alguma coisa. Um pedido tentado só de um jeito não move.
    pub transicoes_uteis: usize,
    pub recompensa_media: f64,
    pub baseline: f64,
    pub antes: Placar,
    pub depois: Placar,
}

/// Uma transição pronta para o REINFORCE.
pub struct Transicao {
    pub pedido: String,
    pub acao: usize,
    pub recompensa: f64,
    /// Os spans que ela **escolheu** — não os certos.
    ///
    /// Para REINFORCE é essa a informação que falta: a ação tomada. O gradiente de
    /// `−(r − b)·log π(a)` não precisa saber a resposta certa, só para onde empurrar
    /// e com que sinal. Sem isto, o reforço treinava metade da decisão.
    pub args: Vec<(usize, (usize, usize))>,
}

/// Monta o alvo de reforço de uma transição, incluindo o ponteiro quando dá.
///
/// Reaproveita [`Exemplo::alvo`] de propósito: é ela que converte span de byte em
/// span de patch e **valida por ida e volta**. Se o recorte não reproduz o valor, o
/// alvo cai para só-intenção em vez de ensinar um ponteiro torto — a mesma regra
/// conservadora que já vale para os dados escritos à mão.
pub fn alvo_de_transicao<P: Patcher + ?Sized>(
    t: &Transicao,
    vantagem: f32,
    patcher: &P,
) -> Alvo {
    let so_intencao = || Alvo::reforco(t.acao, vantagem, t.recompensa as f32);
    if t.args.is_empty() {
        return so_intencao();
    }
    let molde = Exemplo {
        pedido: t.pedido.clone(),
        ferramenta: t.acao,
        args: t.args.clone(),
        frase: (usize::MAX, 0),
    };
    match molde.alvo(patcher) {
        Some(a) => Alvo {
            peso: vantagem,
            alvo_valor: Some(t.recompensa as f32),
            apenas_intencao: false,
            ..a
        },
        None => so_intencao(),
    }
}

/// Extrai as transições com sinal da memória.
pub fn transicoes(mem: &MemoriaEpisodica, p: &Pesos) -> Vec<Transicao> {
    (0..mem.episodios.len())
        .filter_map(|i| {
            let r = recompensa(mem, i, p)?;
            let ep: &Episodio = &mem.episodios[i];
            Some(Transicao {
                pedido: ep.pedido.clone(),
                acao: ep.ferramenta,
                recompensa: r,
                args: ep.args.clone(),
            })
        })
        .collect()
}

/// Ajusta a política pela experiência, misturando replay supervisionado.
///
/// O replay é o mesmo mecanismo anti-esquecimento da fase 3, e aqui ele é ainda
/// mais necessário: o gradiente de REINFORCE não sabe qual era a resposta certa,
/// só que a tomada foi boa ou ruim. Sem uma âncora supervisionada, a política
/// deriva.
#[allow(clippy::too_many_arguments)]
pub fn treinar_por_reforco<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &mut Agente<f32>,
    ops: &O,
    patcher: &P,
    mem: &MemoriaEpisodica,
    base: &[Exemplo],
    validacao: &[Exemplo],
    cfg: &CfgReforco,
) -> RelatorioReforco {
    let mut cache = AgenteCache::new();
    let (_, antes) = avaliar(ag, ops, patcher, validacao, cfg.seq, cfg.batch, &mut cache);

    let trans = transicoes(mem, &cfg.pesos);
    if trans.is_empty() || base.is_empty() {
        return RelatorioReforco {
            passos: 0,
            transicoes: 0,
            erro_critico: f64::NAN,
            transicoes_uteis: 0,
            recompensa_media: f64::NAN,
            baseline: 0.0,
            antes,
            depois: antes,
        };
    }

    let mut baseline = Baseline::default();
    for t in &trans {
        baseline.observar(t.recompensa);
    }
    let recompensa_media = trans.iter().map(|t| t.recompensa).sum::<f64>() / trans.len() as f64;
    let mut n_uteis = 0usize;
    let mut erro_critico = f64::NAN;

    let tamanhos: Vec<usize> = ag.params_mut().iter().map(|p| p.len()).collect();
    let mut adam = Adam::novo(&tamanhos, cfg.lr);
    let mut grad = ag.grad();
    let mut rng = Rng::new(cfg.semente);
    let mut treino = AgenteCache::new();

    let n_rl = ((cfg.batch as f64) * cfg.fracao_reforco).round().max(1.0) as usize;
    let n_rl = n_rl.min(cfg.batch.saturating_sub(1)).max(1);

    let pedidos_trans: Vec<String> = trans.iter().map(|t| t.pedido.clone()).collect();
    let mut vants: Vec<f64>;

    for _epoca in 0..cfg.epocas.max(1) {
        // Reestima V(s) e recalcula as vantagens. A vantagem sai daqui como NUMERO
        // — destacada do grafo, entao o gradiente da politica nao flui pelo critico.
        let v = estimar_valores(ag, ops, patcher, &pedidos_trans, cfg.seq, cfg.batch, &mut cache);
        vants = trans
            .iter()
            .zip(&v)
            .map(|(t, vi)| t.recompensa - vi)
            .collect();
        n_uteis = vants.iter().filter(|x| x.abs() > 1e-6).count();
        erro_critico = trans
            .iter()
            .zip(&v)
            .map(|(t, vi)| (t.recompensa - vi).powi(2))
            .sum::<f64>()
            / trans.len() as f64;

        for _ in 0..cfg.passos {
        // ---- parte de reforço: pedido real + ação tomada + vantagem ----
        let mut pedidos: Vec<String> = Vec::with_capacity(cfg.batch);
        let mut alvos: Vec<Alvo> = Vec::with_capacity(cfg.batch);
        for _ in 0..n_rl {
            let i = (rng.uniform01() * trans.len() as f64) as usize % trans.len();
            let t = &trans[i];
            let v = vants[i].clamp(-cfg.teto_vantagem, cfg.teto_vantagem);
            pedidos.push(t.pedido.clone());
            // A recompensa vai junto: e o alvo do critico.
            alvos.push(alvo_de_transicao(t, v as f32, patcher));
        }
        // ---- parte supervisionada: a âncora ----
        let mut sup: Vec<Exemplo> = Vec::new();
        while pedidos.len() + sup.len() < cfg.batch {
            let i = (rng.uniform01() * base.len() as f64) as usize % base.len();
            sup.push(base[i].clone());
        }
        for e in &sup {
            pedidos.push(e.pedido.clone());
            match e.alvo(patcher) {
                Some(a) => alvos.push(a),
                None => alvos.push(Alvo::reforco(e.ferramenta, 0.0, 0.0)),
            }
        }

        let Some((bytes, plano)) = montar_pedidos(&pedidos, patcher, cfg.seq) else {
            continue;
        };
        let est = ag.modelo.estado_zero(pedidos.len());
        grad.clear();
        ag.compreender(ops, &bytes, &plano, &est, &alvos, &mut treino, Some(&mut grad));
        let fatias = grad.slices();
        let mut params = ag.params_mut();
        adam.passo(&mut params, &fatias, cfg.clip);
        }
    }

    let (_, depois) = avaliar(ag, ops, patcher, validacao, cfg.seq, cfg.batch, &mut cache);
    RelatorioReforco {
        passos: cfg.passos * cfg.epocas.max(1),
        erro_critico,
        transicoes: trans.len(),
        transicoes_uteis: n_uteis,
        recompensa_media,
        baseline: baseline.valor,
        antes,
        depois,
    }
}

/// Estima `V(s)` para uma lista de pedidos, em lotes.
///
/// É uma passada de forward sem gradiente: o crítico já está no caminho normal de
/// `compreender`, então basta ler `cache.cabecas.valor`.
fn estimar_valores<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &P,
    pedidos: &[String],
    seq: usize,
    batch: usize,
    cache: &mut AgenteCache<f32>,
) -> Vec<f64> {
    let mut saida = Vec::with_capacity(pedidos.len());
    for pedaco in pedidos.chunks(batch) {
        let Some((bytes, plano)) = montar_pedidos(pedaco, patcher, seq) else {
            saida.extend(std::iter::repeat_n(0.0, pedaco.len()));
            continue;
        };
        let alvos: Vec<Alvo> = (0..pedaco.len()).map(|_| Alvo::vazio(0)).collect();
        let est = ag.modelo.estado_zero(pedaco.len());
        ag.compreender(ops, &bytes, &plano, &est, &alvos, cache, None);
        for b in 0..pedaco.len() {
            saida.push(cache.cabecas.valor[b] as f64);
        }
    }
    saida
}

/// Monta o lote a partir de pedidos crus (os alvos vêm de fora).
fn montar_pedidos<P: Patcher + ?Sized>(
    pedidos: &[String],
    patcher: &P,
    seq: usize,
) -> Option<(Vec<u8>, Plano)> {
    let batch = pedidos.len();
    let mut bytes = vec![b' '; seq * batch];
    let mut comprimentos = vec![0usize; batch];
    for (b, p) in pedidos.iter().enumerate() {
        let bs = p.as_bytes();
        if bs.is_empty() || bs.len() > seq - 1 {
            return None;
        }
        for (t, &c) in bs.iter().enumerate() {
            bytes[t * batch + b] = c;
        }
        comprimentos[b] = bs.len();
    }
    let mut plano = Plano::novo(patcher, &bytes, seq, batch);
    plano.limitar(&comprimentos);
    Some((bytes, plano))
}

/// Só para manter `montar_lote` e `como_exemplo` em uso na superfície do módulo.
#[allow(dead_code)]
fn _usos(exs: &[&Exemplo], p: &dyn Patcher, ep: &Episodio) {
    let _ = montar_lote(exs, p, 64);
    let _ = como_exemplo(ep, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Feedback, MemoriaEpisodica, Resultado};

    fn mem_com(eps: &[(&str, usize, Resultado, Feedback, Vec<f32>)]) -> MemoriaEpisodica {
        let mut m = MemoriaEpisodica::nova();
        for (pedido, ferr, res, fb, assin) in eps {
            m.gravar(pedido, *ferr, vec![], *res, fb.clone(), assin.clone());
        }
        m
    }

    /// A propriedade que impede o reward hacking: `hora` executando com sucesso e
    /// sem ninguem falar nada NAO e recompensa. Se isto virar positivo, ela aprende
    /// a chamar as ferramentas infaliveis para tudo.
    /// A propriedade que impede o reward hacking: sucesso silencioso vale ZERO.
    ///
    /// Entra como transição — sem ele a falha não teria com o que ser comparada —
    /// mas sem crédito. Como o crítico nasce prevendo zero e aprende a prever zero
    /// onde só houve sucesso, a vantagem fica em zero e nada se move. Se `executou`
    /// virasse positivo, ela aprenderia a chamar as ferramentas infalíveis (`hora`,
    /// `memoria`) para qualquer coisa.
    #[test]
    fn sucesso_sem_feedback_vale_exatamente_zero() {
        let p = Pesos::default();
        let m = mem_com(&[(
            "que horas sao",
            0,
            Resultado::Executou,
            Feedback::Nenhum,
            vec![1.0, 0.0],
        )]);
        assert_eq!(recompensa(&m, 0, &p), Some(0.0));
    }

    #[test]
    fn falha_gera_sinal_negativo_sem_ninguem_dizer_a_resposta() {
        let m = mem_com(&[(
            "abre o arquivo sumido.txt",
            2,
            Resultado::Falhou,
            Feedback::Nenhum,
            vec![1.0, 0.0],
        )]);
        let r = recompensa(&m, 0, &Pesos::default()).expect("falha tem que dar sinal");
        assert!(r < 0.0, "falha deveria ser negativa, deu {r}");
    }

    #[test]
    fn feedback_explicito_domina_o_desfecho() {
        let p = Pesos::default();
        // Executou, mas voce disse que estava errado: o negativo tem que vencer.
        let m = mem_com(&[(
            "x",
            1,
            Resultado::Executou,
            Feedback::Corrigido {
                ferramenta: 5,
                args: vec![],
            },
            vec![1.0],
        )]);
        assert!(recompensa(&m, 0, &p).unwrap() < 0.0);

        // Falhou, mas voce aprovou (a ferramenta era certa, o mundo e que nao
        // colaborou): o positivo tem que vencer.
        let m = mem_com(&[("x", 1, Resultado::Falhou, Feedback::Aprovado, vec![1.0])]);
        assert!(recompensa(&m, 0, &p).unwrap() > 0.0);
    }

    #[test]
    fn reformular_o_pedido_conta_como_sinal_implicito() {
        let p = Pesos::default();
        // Mesmo assunto (assinaturas coladas), decisao diferente logo em seguida.
        let m = mem_com(&[
            ("quanto de disco sobrou", 1, Resultado::Executou, Feedback::Nenhum, vec![1.0, 0.02]),
            ("espaco livre no hd", 6, Resultado::Executou, Feedback::Nenhum, vec![1.0, 0.0]),
        ]);
        assert!(reformulou(&m, 0, p.limiar_reformulacao));
        assert!(recompensa(&m, 0, &p).unwrap() < 0.0, "reformulacao deveria penalizar");
        // O ultimo episodio nao tem sucessor: sem sinal implicito, fica no zero do
        // sucesso — transicao existe, mas nao penaliza.
        assert!(!reformulou(&m, 1, p.limiar_reformulacao));
        assert_eq!(recompensa(&m, 1, &p), Some(0.0));

        // Repetir o pedido e receber a MESMA decisao nao e reclamacao.
        let m = mem_com(&[
            ("quanto de disco", 6, Resultado::Executou, Feedback::Nenhum, vec![1.0, 0.0]),
            ("quanto de disco mesmo", 6, Resultado::Executou, Feedback::Nenhum, vec![1.0, 0.0]),
        ]);
        assert!(!reformulou(&m, 0, p.limiar_reformulacao));
    }

    #[test]
    fn a_ema_de_diagnostico_converge() {
        // A EMA nao entra mais no gradiente (quem faz isso e o critico), mas segue
        // sendo relatada como tendencia da recompensa. Tem que convergir.
        let mut b = Baseline::default();
        for _ in 0..500 {
            b.observar(1.0);
        }
        assert!((b.valor - 1.0).abs() < 1e-3, "baseline nao convergiu: {}", b.valor);
        assert!(b.vantagem(2.0) > 0.9);
        assert!(b.vantagem(-1.0) < -1.9);
    }

    #[test]
    fn episodio_sem_acao_nao_gera_transicao() {
        let p = Pesos::default();
        let m = mem_com(&[
            ("a", 0, Resultado::NaoTentou, Feedback::Nenhum, vec![1.0, 0.0]),
            ("b", 1, Resultado::Falhou, Feedback::Nenhum, vec![0.0, 1.0]),
            ("c", 2, Resultado::Executou, Feedback::Aprovado, vec![0.0, 0.0]),
        ]);
        let t = transicoes(&m, &p);
        assert_eq!(t.len(), 2, "um episodio em que nada foi tentado nao ensina nada");
        assert!(t.iter().any(|x| x.pedido == "b" && x.recompensa < 0.0));
        assert!(t.iter().any(|x| x.pedido == "c" && x.recompensa > 0.0));
    }
}
