//! Cabeças de **intenção**, **ponteiro** e **presença** — onde a Teka deixa de
//! prever texto e passa a decidir uma ação.
//!
//! ## Por que discriminar em vez de gerar
//!
//! Gerar `{"acao":"listar_pasta","caminho":"C:\Users"}` byte a byte exige um modelo
//! grande e muito treino. Mas *decidir qual ferramenta* é uma classificação sobre 9
//! saídas, e *achar o argumento* é apontar para um trecho do próprio pedido. Os dois
//! são ordens de grandeza mais baratos em dados e em parâmetros. É a jogada central
//! da Teka: ela age bem muito antes de falar bem.
//!
//! ## Ponteiro: os argumentos são COPIADOS, não gerados
//!
//! > *"abre o relatorio_final_v3.txt"*
//!
//! O modelo não precisa nunca ter visto esse nome. Duas softmaxes sobre as posições
//! — início e fim — recortam o intervalo, e os bytes saem literalmente do pedido.
//!
//! ## Grosso para fino
//!
//! O ponteiro trabalha em **dois níveis**, e isso não é refinamento acadêmico: é
//! conserto de uma falha medida.
//!
//! A primeira versão apontava só para **patches** e chegava a 95% de acerto. Mas o
//! teto do patcher parte palavras longas, e aí o índice de patch deixa de ser uma
//! âncora confiável:
//!
//! ```text
//! "faz essa multiplicacao 78*3"
//!   patch 0: "faz"       patch 3: "icacao"
//!   patch 1: " essa"     patch 4: " 78*3"   <- o argumento e este
//!   patch 2: " multipl"
//!
//! apontando so por patch:  {"expressao":"icacao 78*3"}   <- escolheu o patch 3
//! ```
//!
//! Repare que o erro é de **escolha de patch**, não de offset dentro dele — então um
//! refinamento confinado ao patch escolhido não consertaria nada. A solução é somar
//! os dois níveis **antes** do argmax:
//!
//! ```text
//! score_byte(t) = score_patch(patch_de(t))  +  <q_byte, e_t> / sqrt(d)
//!                 |___ o prior grosseiro __|    |__ a correcao fina __|
//! ```
//!
//! O prior mantém o acerto que o nível de patch já tinha; o termo fino pode puxar o
//! argmax para o byte certo mesmo quando o patch escolhido é o vizinho. E como o
//! prior entra por soma, o gradiente da perda por byte **volta** para os scores de
//! patch — o nível grosseiro acaba treinado pelo objetivo que realmente importa.
//!
//! `consulta_byte` nasce **zerada**: no primeiro passo o score de byte é exatamente
//! o score de patch, então o nível fino começa neutro e só aprende a corrigir.
//!
//! ## Custo
//!
//! Um pedido tem ~40 patches e ~200 bytes. As softmaxes rodam sobre isso. É atenção,
//! mas **confinada ao pedido atual** — nunca sobre o histórico —, então o custo O(1)
//! por passo do SSM continua valendo onde importa.

use crate::backend::Ops;
use crate::nn::linear::{Linear, LinearGrad};
use crate::num::Float;
use crate::rng::Rng;

/// Teto de parâmetros por ferramenta. Duas softmaxes (início e fim) por slot.
pub const MAX_SLOTS: usize = 4;

/// Peso da perda do crítico dentro da perda total.
///
/// Abaixo de 1 de propósito: o crítico é um meio (reduzir a variância do gradiente
/// da política), não um fim. Deixá-lo dominar faria o tronco se especializar em
/// prever recompensa em vez de entender pedidos.
pub const COEF_CRITICO: f64 = 0.5;

/// Erro quadrático do crítico contra um alvo 0/1, e o gradiente que vai com ele.
///
/// Vive fora do laço porque é chamado de DOIS lugares: nos alvos `apenas_intencao`
/// a chamada inteira é a ferramenta, e nos completos a correção só é conhecida
/// depois do ponteiro. Duplicar as quatro linhas era como as duas versões
/// divergiriam.
///
/// ## Sigmoide com entropia cruzada, e não regressão — e por quê
///
/// A primeira versão era regressão quadrática direto na saída linear. Medido na
/// semente 19: o crítico saiu de morto (0,000 para tudo) para vivo — e **saturado**.
///
/// ```text
/// previsão média:  acertos 0,976   erros 0,964     separação 0,012
/// ```
///
/// Ele prevê "vou acertar" quase sempre, inclusive quando erra. A causa é o
/// desequilíbrio de classe: o alvo é a correção do modelo **nos exemplos de treino,
/// que ele já ajustou** — então o alvo é 1 na esmagadora maioria, e a saída que
/// minimiza o erro quadrático é a taxa-base.
///
/// Duas mudanças, e as duas atacam isso:
///
/// - **sigmoide + entropia cruzada**: a saída fica em `[0,1]` por construção, e o
///   gradiente não desaparece quando a previsão está saturada — que é justamente
///   onde a regressão quadrática desiste.
/// - **peso pela classe rara**: o erro (alvo 0) pesa `PESO_ERRO` vezes mais. Sem
///   isto, prever 1 sempre continua sendo a melhor aposta bruta.
fn critico<T: Float>(
    cache: &mut CabecasCache<T>,
    b: usize,
    certo: bool,
    inv_b: T,
    com_grad: bool,
) -> f64 {
    let x = cache.valor[b].to_f64();
    // Sigmoide estável: para x muito negativo, `exp(x)` não estoura.
    let p = if x >= 0.0 { 1.0 / (1.0 + (-x).exp()) } else { let e = x.exp(); e / (1.0 + e) };
    let w = if certo { 1.0 } else { PESO_ERRO };
    // −ln σ(x) ou −ln σ(−x), na forma que não perde precisão longe de zero.
    let perda = if certo { softplus(-x) } else { softplus(x) };
    if com_grad {
        // d/dx da entropia cruzada com sigmoide é `σ(x) − alvo`. Limpo assim
        // justamente por causa da forma escolhida.
        let alvo = if certo { 1.0 } else { 0.0 };
        // `=` e nao `+=`: o bloco de `alvo_valor` la em cima e o reforco, e um alvo
        // nunca e os dois (`auto_critico` e falso quando ha recompensa observada).
        cache.dvalor[b] = T::from_f64((p - alvo) * w * COEF_CRITICO) * inv_b;
    }
    COEF_CRITICO * w * perda
}

/// `ln(1 + e^x)`, sem estourar para `x` grande.
fn softplus(x: f64) -> f64 {
    if x > 0.0 { x + (-x).exp().ln_1p() } else { x.exp().ln_1p() }
}

/// Quanto o exemplo ERRADO pesa a mais na perda do crítico.
///
/// O modelo acerta a maioria dos exemplos de treino, então o alvo é 1 quase sempre e
/// prever 1 direto minimiza a perda sem discriminar nada. Medido antes deste peso:
/// 0,976 contra 0,964. Cinco é a razão aproximada entre as classes num modelo que
/// acerta ~80% do treino.
pub const PESO_ERRO: f64 = 5.0;

/// O que se quer que a Teka responda a um pedido.
#[derive(Clone, Debug, PartialEq)]
pub struct Alvo {
    pub ferramenta: usize,
    /// Por slot: `(patch_inicial, patch_final)` — o nível grosseiro (auxiliar).
    pub spans: Vec<Option<(usize, usize)>>,
    /// Por slot: `(byte_inicial, byte_final)`, ambos inclusivos — o nível fino.
    /// **É este que decide se a chamada funciona.**
    pub bytes: Vec<Option<(usize, usize)>>,
    /// Multiplicador do gradiente da cabeça de intenção.
    ///
    /// É o que transforma a mesma entropia cruzada em REINFORCE. O gradiente de
    /// `−log π(a)` é o da entropia cruzada com alvo `a`; o de REINFORCE é
    /// `−(r − b)·log π(a)`. São o mesmo gradiente escalado pela **vantagem** —
    /// então um peso por exemplo basta, e o caminho já provado por diferenças
    /// finitas continua sendo o único caminho.
    ///
    /// Peso negativo empurra PARA LONGE da ação tomada: é assim que uma falha vira
    /// aprendizado sem ninguém precisar dizer qual era a resposta certa.
    pub peso: f32,
    /// Recompensa observada, para treinar o crítico.
    ///
    /// `None` em exemplos supervisionados: não houve ação no mundo, então não há
    /// retorno a prever.
    pub alvo_valor: Option<f32>,
    /// Treinar o crítico com a **própria correção deste passo** como alvo.
    ///
    /// ## Por que existe
    ///
    /// A cabeça de crítico tinha ZERO referências em `supervisionado.rs` — só
    /// `reforco.rs` a alimentava, e a corrida padrão é supervisionada. Ela tinha
    /// parâmetros e produzia ruído, e o benchmark já media isso sem ninguém notar:
    ///
    /// ```text
    /// margem     melhor limiar   0.70  saldo +11
    /// critico    melhor limiar  -0.30  saldo  +0     em 12 de 12 sementes
    /// ```
    ///
    /// ## O que ela passa a prever
    ///
    /// Não a recompensa do mundo (não houve ação), e sim **"a minha própria escolha
    /// vai estar certa?"** — 1 se a chamada que ela emitiria bate com o rótulo, 0
    /// se não. Alvo discreto, calculado no mesmo passo, sem forward extra.
    ///
    /// Isso é um sinal de **capacidade**, não de superfície. Hoje ela abstém porque
    /// o verbo é estranho (medido: `perguntar` é aprendida como o complemento das
    /// superfícies dos moldes). Com o crítico treinado ela pode abster porque
    /// **prevê que vai errar** — que é outra coisa.
    ///
    /// ## Por que um campo novo em vez de reusar `alvo_valor`
    ///
    /// `e_reforco = alvo_valor.is_some()` governa a contabilidade do placar. Encher
    /// `alvo_valor` no supervisionado ligaria aquele caminho e corromperia as
    /// métricas reportadas, silenciosamente.
    ///
    /// ## O alvo se move
    ///
    /// É a correção do modelo ATUAL, então ele muda enquanto o modelo aprende: cedo
    /// no treino quase tudo é 0. É o normal de cabeça de calibração, e o preço é
    /// convergir mais devagar que o resto.
    pub auto_critico: bool,
    /// Quando `true`, só a intenção entra na perda.
    ///
    /// Passar `None` para ponteiro e presença ensinaria "este pedido não tem
    /// argumento", que é falso e destruiria o que a fase 2 acertou — então um alvo
    /// sem span anotado **precisa** ligar isto.
    ///
    /// O que mudou: um episódio de reforço não traz o argumento *certo*, mas traz o
    /// que ela **escolheu** ([`crate::memory::Episodio::args`]). Para REINFORCE é
    /// justamente essa a informação necessária — a ação tomada, pesada pela
    /// vantagem — igualzinho à cabeça de intenção. Ver [`Alvo::reforco_com_args`].
    ///
    /// Enquanto isto foi sempre `true` no reforço, corrigir um argumento errado não
    /// ensinava **nada**: o gradiente ia só para a intenção, e o platô de 5/6 do
    /// laço fechado era `escrever_arquivo`, a única ferramenta de dois argumentos.
    pub apenas_intencao: bool,
}

impl Alvo {
    pub fn vazio(ferramenta: usize) -> Self {
        Self {
            ferramenta,
            spans: vec![None; MAX_SLOTS],
            bytes: vec![None; MAX_SLOTS],
            peso: 1.0,
            apenas_intencao: false,
            alvo_valor: None,
            auto_critico: true,
        }
    }

    /// Alvo de reforço sem argumento anotado: só intenção.
    ///
    /// Use [`crate::learn::reforco::alvo_de_transicao`] quando o episódio trouxer os
    /// spans escolhidos — ela reforça o ponteiro também.
    pub fn reforco(acao: usize, vantagem: f32, recompensa: f32) -> Self {
        Self {
            peso: vantagem,
            apenas_intencao: true,
            alvo_valor: Some(recompensa),
            // No reforco o alvo e a recompensa OBSERVADA, que e melhor: ela mede o
            // que aconteceu no mundo, nao o que o proprio modelo acha.
            auto_critico: false,
            ..Self::vazio(acao)
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cabecas<T: Float> {
    pub d: usize,
    pub d_loc: usize,
    pub n_ferramentas: usize,
    pub intencao: Linear<T>,
    pub chaves: Linear<T>,
    pub consulta: Linear<T>,
    /// Um logit por slot: "este argumento aparece no pedido?".
    ///
    /// Sem isto, parâmetros OPCIONAIS eram sempre preenchidos, porque o ponteiro só
    /// sabe apontar — não sabe se abster. Medido: `disco` recebia
    /// `"caminho":"quanto"` recortado do próprio pedido, e a chamada falhava.
    /// A cabeça de ponteiro decide ONDE; esta decide SE.
    pub presenca: Linear<T>,
    /// Consulta do nível fino, no espaço do encoder local.
    pub consulta_byte: Linear<T>,
    /// O **crítico**: prevê a recompensa esperada do pedido, `V(s)`.
    ///
    /// Existe porque a medição pediu. O baseline por pedido (média das recompensas
    /// daquele pedido) só funciona quando o pedido foi tentado várias vezes; num
    /// pedido visto uma vez a vantagem sai zero e nada é aprendido. `V(s)` é
    /// aprendido a partir do estado, então **generaliza** para pedidos parecidos que
    /// nunca foram repetidos.
    pub valor: Linear<T>,
    /// `[2·MAX_SLOTS, d]` — uma consulta aprendida por (slot, extremidade).
    /// Somar um vetor por slot custa `8·d` parâmetros em vez dos `8·d²` de uma
    /// projeção por slot, e faz o mesmo trabalho.
    pub emb_slot: Vec<T>,
    /// `[2·MAX_SLOTS, d_loc]` — idem, para o nível fino.
    pub emb_byte: Vec<T>,
}

#[derive(Clone, Debug)]
pub struct CabecasGrad<T: Float> {
    pub intencao: LinearGrad<T>,
    pub chaves: LinearGrad<T>,
    pub consulta: LinearGrad<T>,
    pub presenca: LinearGrad<T>,
    pub consulta_byte: LinearGrad<T>,
    pub valor: LinearGrad<T>,
    pub demb_slot: Vec<T>,
    pub demb_byte: Vec<T>,
}

impl<T: Float> CabecasGrad<T> {
    pub fn clear(&mut self) {
        self.intencao.clear();
        self.chaves.clear();
        self.consulta.clear();
        self.presenca.clear();
        self.consulta_byte.clear();
        self.valor.clear();
        self.demb_slot.fill(T::ZERO);
        self.demb_byte.fill(T::ZERO);
    }
    pub fn slices(&self) -> Vec<&[T]> {
        vec![
            &self.intencao.dw[..],
            &self.intencao.db[..],
            &self.chaves.dw[..],
            &self.chaves.db[..],
            &self.consulta.dw[..],
            &self.consulta.db[..],
            &self.presenca.dw[..],
            &self.presenca.db[..],
            &self.consulta_byte.dw[..],
            &self.consulta_byte.db[..],
            &self.valor.dw[..],
            &self.valor.db[..],
            &self.demb_slot[..],
            &self.demb_byte[..],
        ]
    }
}

#[derive(Clone, Debug, Default)]
pub struct CabecasCache<T: Float> {
    pub zf: Vec<T>,         // [batch, d]  estado do último patch de cada pedido
    pub logits_int: Vec<T>, // [batch, n_ferramentas]
    pub q_base: Vec<T>,     // [batch, d]
    pub qb_base: Vec<T>,    // [batch, d_loc]
    pub keys: Vec<T>,       // [p_max·batch, d]
    pub score: Vec<T>,      // [2·MAX_SLOTS, batch, p_max]
    pub score_byte: Vec<T>, // [2·MAX_SLOTS, batch, seq]
    pub logit_pres: Vec<T>, // [batch, MAX_SLOTS]
    pub valor: Vec<T>,      // [batch] — a previsão do crítico
    dvalor: Vec<T>,
    dpres: Vec<T>,
    dlogits: Vec<T>,
    dscore: Vec<T>,
    dscore_byte: Vec<T>,
    dq_base: Vec<T>,
    dqb_base: Vec<T>,
    dkeys: Vec<T>,
    dzf: Vec<T>,
}

impl<T: Float> CabecasCache<T> {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Métricas do que interessa de verdade — bits/byte não diz se ela agiu certo.
#[derive(Clone, Copy, Debug, Default)]
pub struct Placar {
    pub n: usize,
    pub intencao_certa: usize,
    /// Nível grosseiro (patch). Auxiliar — serve para diagnosticar.
    pub spans_certos: usize,
    pub spans_totais: usize,
    /// Nível fino (byte). **É este que decide se a chamada funciona.**
    pub bytes_certos: usize,
    pub bytes_totais: usize,
    /// Ferramenta certa **e** todos os argumentos certos, no nível de byte.
    pub tudo_certo: usize,
    /// Argumentos certos **entre os casos em que a ferramenta estava certa**.
    ///
    /// Separar isto de `bytes_certos` responde a pergunta que decide onde investir:
    /// quando o argumento sai errado, é o ponteiro que falhou, ou é consequência de
    /// o modelo ter escolhido a ferramenta errada e o ponteiro estar procurando um
    /// argumento que nem existe no pedido?
    pub bytes_certos_com_int_ok: usize,
    pub bytes_totais_com_int_ok: usize,
}

impl Placar {
    pub fn acuracia_intencao(&self) -> f64 {
        razao(self.intencao_certa, self.n)
    }
    /// Acurácia do argumento no nível que importa: byte.
    pub fn acuracia_span(&self) -> f64 {
        razao(self.bytes_certos, self.bytes_totais)
    }
    /// Nível grosseiro, só para diagnóstico.
    pub fn acuracia_patch(&self) -> f64 {
        razao(self.spans_certos, self.spans_totais)
    }
    pub fn acuracia_total(&self) -> f64 {
        razao(self.tudo_certo, self.n)
    }
    /// Acurácia do argumento restrita aos casos em que a ferramenta saiu certa.
    pub fn acuracia_span_dado_intencao(&self) -> f64 {
        razao(self.bytes_certos_com_int_ok, self.bytes_totais_com_int_ok)
    }
    pub fn somar(&mut self, o: &Placar) {
        self.n += o.n;
        self.intencao_certa += o.intencao_certa;
        self.spans_certos += o.spans_certos;
        self.spans_totais += o.spans_totais;
        self.bytes_certos += o.bytes_certos;
        self.bytes_totais += o.bytes_totais;
        self.tudo_certo += o.tudo_certo;
        self.bytes_certos_com_int_ok += o.bytes_certos_com_int_ok;
        self.bytes_totais_com_int_ok += o.bytes_totais_com_int_ok;
    }
}

fn razao(a: usize, b: usize) -> f64 {
    if b == 0 {
        f64::NAN
    } else {
        a as f64 / b as f64
    }
}

impl<T: Float> Cabecas<T> {
    pub fn new(d: usize, d_loc: usize, n_ferramentas: usize, rng: &mut Rng) -> Self {
        let mut emb_slot = vec![T::ZERO; 2 * MAX_SLOTS * d];
        rng.fill_normal(&mut emb_slot, 0.02);
        let mut emb_byte = vec![T::ZERO; 2 * MAX_SLOTS * d_loc];
        rng.fill_normal(&mut emb_byte, 0.02);
        Self {
            d,
            d_loc,
            n_ferramentas,
            // Cabeças de decisão nascem quase zeradas, pelo mesmo motivo da cabeça
            // de bytes: começar em distribuição uniforme e não gastar passos
            // desfazendo o próprio ruído de inicialização.
            intencao: Linear::nova_com_escala(d, n_ferramentas, 0.05, rng),
            chaves: Linear::new(d, d, rng),
            consulta: Linear::nova_com_escala(d, d, 0.05, rng),
            presenca: Linear::nova_com_escala(d, MAX_SLOTS, 0.05, rng),
            // Zerada de propósito — ver a nota sobre "grosso para fino" no topo.
            consulta_byte: Linear::nova_com_escala(d, d_loc, 0.0, rng),
            // Nasce prevendo zero: sem experiência nenhuma, a melhor estimativa da
            // recompensa é "nada acontece".
            valor: Linear::nova_com_escala(d, 1, 0.0, rng),
            emb_slot,
            emb_byte,
        }
    }

    pub fn grad(&self) -> CabecasGrad<T> {
        CabecasGrad {
            intencao: self.intencao.grad(),
            chaves: self.chaves.grad(),
            consulta: self.consulta.grad(),
            presenca: self.presenca.grad(),
            consulta_byte: self.consulta_byte.grad(),
            valor: self.valor.grad(),
            demb_slot: vec![T::ZERO; self.emb_slot.len()],
            demb_byte: vec![T::ZERO; self.emb_byte.len()],
        }
    }

    pub fn n_params(&self) -> usize {
        self.intencao.n_params()
            + self.chaves.n_params()
            + self.consulta.n_params()
            + self.presenca.n_params()
            + self.consulta_byte.n_params()
            + self.valor.n_params()
            + self.emb_slot.len()
            + self.emb_byte.len()
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        vec![
            &mut self.intencao.w[..],
            &mut self.intencao.b[..],
            &mut self.chaves.w[..],
            &mut self.chaves.b[..],
            &mut self.consulta.w[..],
            &mut self.consulta.b[..],
            &mut self.presenca.w[..],
            &mut self.presenca.b[..],
            &mut self.consulta_byte.w[..],
            &mut self.consulta_byte.b[..],
            &mut self.valor.w[..],
            &mut self.valor.b[..],
            &mut self.emb_slot[..],
            &mut self.emb_byte[..],
        ]
    }

    pub fn descritores(&self) -> Vec<(String, Vec<usize>)> {
        let mut v = self.intencao.descritores("cabecas.intencao");
        v.extend(self.chaves.descritores("cabecas.chaves"));
        v.extend(self.consulta.descritores("cabecas.consulta"));
        v.extend(self.presenca.descritores("cabecas.presenca"));
        v.extend(self.consulta_byte.descritores("cabecas.consulta_byte"));
        v.extend(self.valor.descritores("cabecas.valor"));
        v.push(("cabecas.emb_slot".into(), vec![self.emb_slot.len()]));
        v.push(("cabecas.emb_byte".into(), vec![self.emb_byte.len()]));
        v
    }

    fn preparar(&self, c: &mut CabecasCache<T>, batch: usize, p_max: usize, seq: usize) {
        let (d, dl) = (self.d, self.d_loc);
        for b in [&mut c.zf, &mut c.q_base, &mut c.dq_base, &mut c.dzf] {
            b.resize(batch * d, T::ZERO);
        }
        for b in [&mut c.qb_base, &mut c.dqb_base] {
            b.resize(batch * dl, T::ZERO);
        }
        for b in [&mut c.keys, &mut c.dkeys] {
            b.resize(p_max * batch * d, T::ZERO);
        }
        for b in [&mut c.logits_int, &mut c.dlogits] {
            b.resize(batch * self.n_ferramentas, T::ZERO);
        }
        for b in [&mut c.score, &mut c.dscore] {
            b.resize(2 * MAX_SLOTS * batch * p_max, T::ZERO);
        }
        for b in [&mut c.score_byte, &mut c.dscore_byte] {
            b.resize(2 * MAX_SLOTS * batch * seq, T::ZERO);
        }
        for b in [&mut c.logit_pres, &mut c.dpres] {
            b.resize(batch * MAX_SLOTS, T::ZERO);
        }
        for b in [&mut c.valor, &mut c.dvalor] {
            b.resize(batch, T::ZERO);
        }
    }

    /// `z`: saída do backbone `[p_max·batch, d]`. `e`: saída do encoder local
    /// `[seq·batch, d_loc]`.
    ///
    /// Com `grad = Some((g, dz, de))`, treina e **acumula** em `dz` e `de` — quem
    /// chama é responsável por zerá-los antes.
    #[allow(clippy::too_many_arguments)]
    pub fn passo<O: Ops<T>>(
        &self,
        ops: &O,
        z: &[T],
        e: &[T],
        n_patches: &[usize],
        p_max: usize,
        seq: usize,
        comprimentos: &[usize],
        patch_de_byte: &[usize],
        alvos: &[Alvo],
        slots: &[(usize, usize)],
        cache: &mut CabecasCache<T>,
        mut grad: Option<(&mut CabecasGrad<T>, &mut [T], &mut [T])>,
    ) -> (f64, Placar) {
        let batch = n_patches.len();
        let (d, dl) = (self.d, self.d_loc);
        let nf = self.n_ferramentas;
        self.preparar(cache, batch, p_max, seq);

        // O estado do ÚLTIMO patch resume o pedido inteiro: o backbone é recorrente,
        // então ali já passou tudo.
        for b in 0..batch {
            let up = n_patches[b].saturating_sub(1);
            let src = (up * batch + b) * d;
            cache.zf[b * d..(b + 1) * d].copy_from_slice(&z[src..src + d]);
        }

        self.intencao
            .forward(ops, &cache.zf, batch, &mut cache.logits_int);
        self.consulta.forward(ops, &cache.zf, batch, &mut cache.q_base);
        self.presenca
            .forward(ops, &cache.zf, batch, &mut cache.logit_pres);
        self.consulta_byte
            .forward(ops, &cache.zf, batch, &mut cache.qb_base);
        self.valor.forward(ops, &cache.zf, batch, &mut cache.valor);
        self.chaves.forward(ops, z, p_max * batch, &mut cache.keys);

        let inv = T::from_f64(1.0 / (d as f64).sqrt());
        let inv_l = T::from_f64(1.0 / (dl as f64).sqrt());
        let menos_inf = T::from_f64(-1e30);

        // --- nível grosseiro: um score por patch ---
        for s in 0..2 * MAX_SLOTS {
            for b in 0..batch {
                for p in 0..p_max {
                    let idx = (s * batch + b) * p_max + p;
                    if p >= n_patches[b] {
                        cache.score[idx] = menos_inf;
                        continue;
                    }
                    let k0 = (p * batch + b) * d;
                    let mut acc = T::ZERO;
                    for i in 0..d {
                        acc += (cache.q_base[b * d + i] + self.emb_slot[s * d + i])
                            * cache.keys[k0 + i];
                    }
                    cache.score[idx] = acc * inv;
                }
            }
        }

        // --- nível fino: prior do patch + correção por byte ---
        for s in 0..2 * MAX_SLOTS {
            for b in 0..batch {
                for t in 0..seq {
                    let idx = (s * batch + b) * seq + t;
                    if t >= comprimentos[b] {
                        cache.score_byte[idx] = menos_inf;
                        continue;
                    }
                    let p = patch_de_byte[t * batch + b].min(n_patches[b].saturating_sub(1));
                    let prior = cache.score[(s * batch + b) * p_max + p];
                    let e0 = (t * batch + b) * dl;
                    let mut acc = T::ZERO;
                    for i in 0..dl {
                        acc += (cache.qb_base[b * dl + i] + self.emb_byte[s * dl + i]) * e[e0 + i];
                    }
                    cache.score_byte[idx] = prior + acc * inv_l;
                }
            }
        }

        // ---------------- perda ----------------
        let mut perda = 0.0f64;
        let mut placar = Placar::default();
        let inv_b = T::from_f64(1.0 / batch as f64);

        if grad.is_some() {
            cache.dlogits.fill(T::ZERO);
            cache.dscore.fill(T::ZERO);
            cache.dscore_byte.fill(T::ZERO);
            cache.dpres.fill(T::ZERO);
            cache.dvalor.fill(T::ZERO);
        }

        for b in 0..batch {
            let alvo = &alvos[b];
            // -- intenção --
            let row = &cache.logits_int[b * nf..(b + 1) * nf];
            let log_z = log_sum_exp(row);
            // O peso entra na PERDA e não só no gradiente. Com peso 1 as duas
            // formas coincidem, então o erro passou despercebido até o gradcheck
            // rodar com um alvo de reforço (peso negativo) e acusar 2,9% de erro
            // relativo — longe demais de ruído numérico para ser outra coisa.
            //
            // A perda vira um SUBSTITUTO (pode ficar negativa): o que importa dela
            // é o gradiente, não o valor.
            perda += (T::from_f64(alvo.peso as f64) * (log_z - row[alvo.ferramenta])).to_f64();
            let int_ok = argmax(row) == alvo.ferramenta;
            if int_ok {
                placar.intencao_certa += 1;
            }
            if grad.is_some() {
                let w = inv_b * T::from_f64(alvo.peso as f64);
                let drow = &mut cache.dlogits[b * nf..(b + 1) * nf];
                for j in 0..nf {
                    drow[j] = (row[j] - log_z).exp() * w;
                }
                drow[alvo.ferramenta] -= w;
            }

            // -- crítico: prever a recompensa daquele pedido --
            if let Some(alvo_v) = alvo.alvo_valor {
                let d = cache.valor[b] - T::from_f64(alvo_v as f64);
                perda += (COEF_CRITICO * 0.5 * (d * d).to_f64()).to_f64();
                if grad.is_some() {
                    cache.dvalor[b] = d * T::from_f64(COEF_CRITICO) * inv_b;
                }
            }

            if alvo.apenas_intencao {
                placar.n += 1;
                if int_ok {
                    placar.tudo_certo += 1;
                }
                // Aqui a chamada inteira E a ferramenta: nao ha argumento a conferir.
                if alvo.auto_critico {
                    perda += critico(cache, b, int_ok, inv_b, grad.is_some());
                }
                continue;
            }

            // -- presença: este slot aparece no pedido? --
            //
            // O peso da vantagem vale aqui e no ponteiro pelo mesmo motivo que vale
            // na intenção: com `peso = 1,0` (todo alvo supervisionado) isto é
            // idêntico ao que era, e com vantagem negativa empurra para longe do que
            // ela fez. É o que torna o reforço capaz de corrigir argumento.
            let w = T::from_f64(alvo.peso as f64);
            let (n_slots, n_obrig) = slots.get(alvo.ferramenta).copied().unwrap_or((0, 0));
            let e_reforco = alvo.alvo_valor.is_some();
            let mut tudo_ok = true;
            for slot in 0..n_slots.min(MAX_SLOTS) {
                let k = b * MAX_SLOTS + slot;
                let x = cache.logit_pres[k];
                let presente = alvo.bytes[slot].is_some();
                // Entropia cruzada binária na forma estável: −ln σ(x) ou −ln σ(−x).
                perda += (w * if presente {
                    -x.ln_sigmoid()
                } else {
                    -(-x).ln_sigmoid()
                })
                .to_f64();
                if (slot < n_obrig || x > T::ZERO) != presente {
                    tudo_ok = false;
                }
                if grad.is_some() {
                    let alvo_y = if presente { T::ONE } else { T::ZERO };
                    cache.dpres[k] = (x.sigmoid() - alvo_y) * inv_b * w;
                }
            }

            // -- ponteiro, nos dois níveis --
            for slot in 0..MAX_SLOTS {
                // grosseiro (auxiliar): mantém o prior forte
                if let Some((ini, fim)) = alvo.spans[slot] {
                    for (extremo, alvo_p) in [(0usize, ini), (1usize, fim)] {
                        let s = slot * 2 + extremo;
                        let base = (s * batch + b) * p_max;
                        let row = &cache.score[base..base + p_max];
                        let log_z = log_sum_exp(row);
                        perda += (w * (log_z - row[alvo_p])).to_f64();
                        // Alvo de reforço não é gabarito: contá-lo aqui poluiria o
                        // relatório de treino com a própria escolha dela.
                        if !e_reforco {
                            if argmax(row) == alvo_p {
                                placar.spans_certos += 1;
                            }
                            placar.spans_totais += 1;
                        }
                        if grad.is_some() {
                            let dw = inv_b * w;
                            let drow = &mut cache.dscore[base..base + p_max];
                            for p in 0..p_max {
                                drow[p] = if p < n_patches[b] {
                                    (row[p] - log_z).exp() * dw
                                } else {
                                    T::ZERO
                                };
                            }
                            drow[alvo_p] -= dw;
                        }
                    }
                }
                // fino: o que decide se a chamada funciona
                if let Some((ini, fim)) = alvo.bytes[slot] {
                    let mut par_ok = true;
                    for (extremo, alvo_t) in [(0usize, ini), (1usize, fim)] {
                        let s = slot * 2 + extremo;
                        let base = (s * batch + b) * seq;
                        let row = &cache.score_byte[base..base + seq];
                        let log_z = log_sum_exp(row);
                        perda += (w * (log_z - row[alvo_t])).to_f64();
                        if argmax(row) != alvo_t {
                            par_ok = false;
                        }
                        if grad.is_some() {
                            let dw = inv_b * w;
                            let drow = &mut cache.dscore_byte[base..base + seq];
                            for t in 0..seq {
                                drow[t] = if t < comprimentos[b] {
                                    (row[t] - log_z).exp() * dw
                                } else {
                                    T::ZERO
                                };
                            }
                            drow[alvo_t] -= dw;
                        }
                    }
                    if !par_ok {
                        tudo_ok = false;
                    }
                    if !e_reforco {
                        placar.bytes_certos += par_ok as usize;
                        placar.bytes_totais += 1;
                        if int_ok {
                            placar.bytes_totais_com_int_ok += 1;
                            placar.bytes_certos_com_int_ok += par_ok as usize;
                        }
                    }
                }
            }
            placar.n += 1;
            if int_ok && tudo_ok {
                placar.tudo_certo += 1;
            }
            // So aqui a correcao COMPLETA e conhecida — ferramenta e argumento. Por
            // isso o critico automatico fica no fim do laco, e nao junto do bloco de
            // `alvo_valor` la em cima.
            if alvo.auto_critico {
                perda += critico(cache, b, int_ok && tudo_ok, inv_b, grad.is_some());
            }
        }
        perda /= batch as f64;

        let Some((g, dz, de)) = grad.as_mut() else {
            return (perda, placar);
        };

        // ---------------- backward ----------------
        cache.dq_base.fill(T::ZERO);
        cache.dqb_base.fill(T::ZERO);
        cache.dkeys.fill(T::ZERO);

        // O fino vem primeiro: ele empurra gradiente PARA o score de patch (o prior
        // entra por soma), então o nível grosseiro acaba treinado também pelo
        // objetivo fino.
        for s in 0..2 * MAX_SLOTS {
            for b in 0..batch {
                for t in 0..comprimentos[b].min(seq) {
                    let ds = cache.dscore_byte[(s * batch + b) * seq + t];
                    if ds == T::ZERO {
                        continue;
                    }
                    let p = patch_de_byte[t * batch + b].min(n_patches[b].saturating_sub(1));
                    cache.dscore[(s * batch + b) * p_max + p] += ds;

                    let dsl = ds * inv_l;
                    let e0 = (t * batch + b) * dl;
                    for i in 0..dl {
                        let qi = cache.qb_base[b * dl + i] + self.emb_byte[s * dl + i];
                        de[e0 + i] += dsl * qi;
                        let dq = dsl * e[e0 + i];
                        cache.dqb_base[b * dl + i] += dq;
                        g.demb_byte[s * dl + i] += dq;
                    }
                }
            }
        }

        for s in 0..2 * MAX_SLOTS {
            for b in 0..batch {
                for p in 0..n_patches[b].min(p_max) {
                    let ds = cache.dscore[(s * batch + b) * p_max + p] * inv;
                    if ds == T::ZERO {
                        continue;
                    }
                    let k0 = (p * batch + b) * d;
                    for i in 0..d {
                        let qi = cache.q_base[b * d + i] + self.emb_slot[s * d + i];
                        cache.dkeys[k0 + i] += ds * qi;
                        let dq = ds * cache.keys[k0 + i];
                        cache.dq_base[b * d + i] += dq;
                        g.demb_slot[s * d + i] += dq;
                    }
                }
            }
        }

        // As chaves veem TODOS os patches → o gradiente vai direto para `dz`.
        self.chaves.backward(
            ops,
            z,
            &cache.dkeys,
            p_max * batch,
            Some(dz),
            true,
            &mut g.chaves,
        );

        // As demais veem só o último patch.
        self.intencao.backward(
            ops,
            &cache.zf,
            &cache.dlogits,
            batch,
            Some(&mut cache.dzf),
            false,
            &mut g.intencao,
        );
        self.consulta.backward(
            ops,
            &cache.zf,
            &cache.dq_base,
            batch,
            Some(&mut cache.dzf),
            true,
            &mut g.consulta,
        );
        self.presenca.backward(
            ops,
            &cache.zf,
            &cache.dpres,
            batch,
            Some(&mut cache.dzf),
            true,
            &mut g.presenca,
        );
        self.consulta_byte.backward(
            ops,
            &cache.zf,
            &cache.dqb_base,
            batch,
            Some(&mut cache.dzf),
            true,
            &mut g.consulta_byte,
        );
        // O crítico NÃO propaga para o tronco.
        //
        // Ele compartilha a representação, mas não deve moldá-la: o tronco existe
        // para entender pedidos, e deixá-lo ser puxado por "prever recompensa"
        // troca a tarefa que importa por uma auxiliar. Medido — com o gradiente
        // passando, o erro do crítico disparou de 0,05 para 171 e arrastou a
        // acurácia de intenção de 77% para 56% em cinco rodadas.
        //
        // Consequência para os testes: um caminho destacado é invisível para
        // diferenças finitas (perturbar um peso do tronco muda `zf` e muda `V`,
        // mesmo que o gradiente não flua de volta). Por isso o gradcheck do agente
        // roda com `alvo_valor = None`, e a corretude desta cabeça vem do gradcheck
        // do `Linear`, que é o que ela é.
        self.valor
            .backward(ops, &cache.zf, &cache.dvalor, batch, None, false, &mut g.valor);
        for b in 0..batch {
            let up = n_patches[b].saturating_sub(1);
            let dst = (up * batch + b) * d;
            for i in 0..d {
                dz[dst + i] += cache.dzf[b * d + i];
            }
        }

        (perda, placar)
    }

    /// A distribuição da política para um item do lote — `π(a | pedido)`.
    ///
    /// É o que a exploração amostra. REINFORCE sem exploração só sabe **desaprender**:
    /// empurra para longe da ação tomada e espalha a probabilidade sobre as outras,
    /// sem nunca experimentar qual delas funcionaria.
    pub fn politica(&self, cache: &CabecasCache<T>, b: usize, temperatura: f64) -> Vec<f64> {
        let nf = self.n_ferramentas;
        let row = &cache.logits_int[b * nf..(b + 1) * nf];
        let t = temperatura.max(1e-6);
        let maxi = row.iter().fold(f64::NEG_INFINITY, |m, v| m.max(v.to_f64()));
        let mut p: Vec<f64> = row.iter().map(|v| ((v.to_f64() - maxi) / t).exp()).collect();
        let soma: f64 = p.iter().sum();
        for v in p.iter_mut() {
            *v /= soma;
        }
        p
    }

    /// Decodifica a resposta: qual ferramenta e qual intervalo de **bytes** por slot.
    pub fn decidir(
        &self,
        cache: &CabecasCache<T>,
        n_patches: &[usize],
        seq: usize,
        slots: &[(usize, usize)],
    ) -> Vec<Alvo> {
        self.decidir_forcando(cache, n_patches, seq, slots, &[])
    }

    /// Igual a [`Cabecas::decidir`], mas com a ferramenta imposta de fora.
    ///
    /// Serve à exploração: a ferramenta vem amostrada da política, e o ponteiro
    /// recorta os argumentos **daquela** ferramenta — senão a ação explorada sairia
    /// com os argumentos de outra.
    pub fn decidir_forcando(
        &self,
        cache: &CabecasCache<T>,
        n_patches: &[usize],
        seq: usize,
        slots: &[(usize, usize)],
        forcadas: &[usize],
    ) -> Vec<Alvo> {
        let batch = n_patches.len();
        let nf = self.n_ferramentas;
        (0..batch)
            .map(|b| {
                let ferramenta = forcadas
                    .get(b)
                    .copied()
                    .unwrap_or_else(|| argmax(&cache.logits_int[b * nf..(b + 1) * nf]));
                let (n_slots, n_obrig) = slots.get(ferramenta).copied().unwrap_or((0, 0));
                let mut alvo = Alvo::vazio(ferramenta);
                for slot in 0..n_slots.min(MAX_SLOTS) {
                    // Obrigatório sempre entra (a gramática exige). Opcional só entra
                    // se a cabeça de presença disser que sim — é isso que evita
                    // preencher `disco` com um pedaço qualquer do pedido.
                    if slot >= n_obrig && cache.logit_pres[b * MAX_SLOTS + slot] <= T::ZERO {
                        continue;
                    }
                    let pega = |extremo: usize| {
                        let base = ((slot * 2 + extremo) * batch + b) * seq;
                        argmax(&cache.score_byte[base..base + seq])
                    };
                    let (i, f) = (pega(0), pega(1));
                    // Intervalo invertido é o modelo dizendo bobagem; um único byte
                    // é a leitura conservadora.
                    alvo.bytes[slot] = Some(if f >= i { (i, f) } else { (i, i) });
                }
                alvo
            })
            .collect()
    }
}

fn log_sum_exp<T: Float>(row: &[T]) -> T {
    let mut maxi = row[0];
    for &v in row.iter() {
        maxi = maxi.max(v);
    }
    let mut soma = T::ZERO;
    for &v in row.iter() {
        soma += (v - maxi).exp();
    }
    maxi + soma.ln()
}

fn argmax<T: Float>(row: &[T]) -> usize {
    let mut melhor = 0;
    for (i, v) in row.iter().enumerate() {
        if *v > row[melhor] {
            melhor = i;
        }
    }
    melhor
}

#[cfg(test)]
mod testes_critico {
    use super::*;

    /// O supervisionado alimenta o critico; o reforco usa a recompensa observada.
    ///
    /// Este teste existe porque a cabeca ficou com ZERO referencias em
    /// `supervisionado.rs` por meses. Ela tinha parametros, produzia numero, e o
    /// numero era ruido -- e o benchmark ja media aquilo (saldo +0 em 12 de 12
    /// sementes) sem ninguem ler.
    #[test]
    fn so_o_supervisionado_usa_o_alvo_automatico() {
        let sup = Alvo::vazio(0);
        assert!(sup.auto_critico, "supervisionado tem de alimentar o critico");
        assert!(sup.alvo_valor.is_none(), "nao ha recompensa observada aqui");

        let rf = Alvo::reforco(0, 1.0, 0.7);
        assert!(!rf.auto_critico, "no reforco o alvo e a recompensa do mundo");
        assert_eq!(rf.alvo_valor, Some(0.7));
    }

    /// A perda cai quando a previsao se aproxima, e o gradiente aponta pra la.
    ///
    /// `cache.valor` agora e LOGIT, nao probabilidade: a perda e entropia cruzada
    /// com sigmoide. Entao ela nunca chega a zero exato com logit finito -- so
    /// encolhe. O teste anterior exigia zero e falhou quando a forma mudou, que e
    /// exatamente o que ele tinha de fazer.
    #[test]
    fn o_critico_persegue_a_propria_correcao() {
        let mut c = CabecasCache::<f64>::new();
        c.valor = vec![0.0];
        c.dvalor = vec![0.0];
        let mut medir = |c: &mut CabecasCache<f64>, logit: f64, certo: bool| {
            c.valor[0] = logit;
            critico(c, 0, certo, 1.0, true)
        };

        // Acertou: logit alto doi menos que logit baixo.
        let longe = medir(&mut c, -3.0, true);
        let perto = medir(&mut c, 3.0, true);
        assert!(perto < longe, "perto={perto} deveria doer menos que longe={longe}");
        assert!(perto < 0.05, "logit 3 com alvo 1 deveria quase nao doer, deu {perto}");

        // Gradiente empurra o logit PARA CIMA quando ele esta baixo e o alvo e 1.
        medir(&mut c, -3.0, true);
        assert!(c.dvalor[0] < 0.0, "dvalor={} deveria ser negativo", c.dvalor[0]);

        // Errou: alvo 0, e agora empurra para BAIXO.
        medir(&mut c, 3.0, false);
        assert!(c.dvalor[0] > 0.0, "dvalor={} deveria ser positivo", c.dvalor[0]);

        // E o ERRO pesa mais que o acerto, que e o conserto da saturacao.
        let no_erro = medir(&mut c, 3.0, false);
        let no_acerto = medir(&mut c, -3.0, true);
        assert!(
            (no_erro / no_acerto - PESO_ERRO).abs() < 1e-6,
            "o erro deveria pesar {PESO_ERRO}x: {no_erro} contra {no_acerto}"
        );
    }
}
