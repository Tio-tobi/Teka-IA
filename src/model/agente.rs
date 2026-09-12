//! O agente: o modelo de linguagem + as cabeças de decisão + a gramática.
//!
//! ```text
//!   pedido em bytes
//!         │
//!         ▼
//!   [tronco: encoder local → patcher → backbone]      ← compartilhado com o LM
//!         │
//!         ├──▶ [cabeça de intenção]  → qual ferramenta
//!         └──▶ [cabeça de ponteiro]  → quais patches contêm cada argumento
//!                     │
//!                     ▼
//!            recorta os bytes do PEDIDO
//!                     │
//!                     ▼
//!            [gramática] → chamada sintaticamente válida por construção
//!                     │
//!                     ▼
//!            [política] → sandbox por padrão
//! ```
//!
//! Repare que o modelo nunca **escreve** a chamada. Ele escolhe uma ferramenta e
//! aponta para trechos do pedido; a gramática monta o texto. Um modelo que ainda
//! escreve proto-português produz aqui uma chamada perfeita — que é exatamente o
//! objetivo da fase 2.

use crate::backend::Ops;
use crate::grammar::{Automato, Decodificador};
use crate::model::confianca::Confianca;
use crate::model::heads::{Alvo, Cabecas, CabecasCache, CabecasGrad, Placar, MAX_SLOTS};
use crate::model::hierarchy::{Config, Estado, Teka, TekaCache, TekaGrad};
use crate::model::patcher::{Patcher, Plano};
use crate::num::Float;
use crate::rng::Rng;
use crate::tools::{Chamada, Politica, Registro, TipoParam};

#[derive(Clone)]
pub struct Agente<T: Float> {
    pub modelo: Teka<T>,
    pub cabecas: Cabecas<T>,
    pub registro: Registro,
    pub automato: Automato,
    /// Somado ao logit de `perguntar` antes de decidir. Ver
    /// [`Agente::responder_com_confianca`]. Zero = o que o treino aprendeu.
    pub vies_abster: f64,
}

pub struct AgenteGrad<T: Float> {
    pub modelo: TekaGrad<T>,
    pub cabecas: CabecasGrad<T>,
}

impl<T: Float> AgenteGrad<T> {
    pub fn clear(&mut self) {
        self.modelo.clear();
        self.cabecas.clear();
    }
    pub fn slices(&self) -> Vec<&[T]> {
        let mut v = self.modelo.slices();
        v.extend(self.cabecas.slices());
        v
    }
}

/// Por que a chamada não pôde ser escrita.
///
/// Separa "não sei o que fazer" de "sei o que fazer, falta você me dizer com quê" —
/// e a segunda não é falha, é uma pergunta.
#[derive(Clone, Debug, PartialEq)]
pub enum Falta {
    /// A ferramenta está decidida, mas o pedido não traz o argumento obrigatório.
    ///
    /// *"apaga esse arquivo aqui"* → `apagar_arquivo`, faltando `caminho`.
    Argumento { ferramenta: usize, param: String },
    /// Qualquer outra falha, com o texto original.
    Outra(String),
}

impl Falta {
    /// A pergunta que ela faz, em português, quando o que falta é um argumento.
    ///
    /// O texto sai do TIPO do parâmetro e não do nome dele: "caminho" é como o
    /// registro chama, não como uma pessoa pergunta. E nomear a ferramenta junto é
    /// o que transforma a pergunta em prova de que ela entendeu — "qual arquivo
    /// apagar?" diz que ela sabe que é para apagar.
    pub fn pergunta(&self, reg: &Registro) -> Option<String> {
        let Falta::Argumento { ferramenta, param } = self else {
            return None;
        };
        let f = reg.ferramentas.get(*ferramenta)?;
        let p = f.params.iter().find(|p| &p.nome == param)?;
        let o_que = match p.tipo {
            TipoParam::Caminho => "qual arquivo ou pasta",
            TipoParam::Numero => "qual número",
            TipoParam::Texto => match param.as_str() {
                "texto" => "o que escrever",
                "consulta" => "o que procurar",
                "padrao" => "o que procurar dentro dos arquivos",
                "programa" => "qual programa",
                "comando" => "qual comando",
                "nome" => "qual nome",
                _ => "o quê",
            },
        };
        Some(format!("{o_que} para {}?", f.nome))
    }
}

#[derive(Default)]
pub struct AgenteCache<T: Float> {
    pub modelo: TekaCache<T>,
    pub cabecas: CabecasCache<T>,
}

impl<T: Float> AgenteCache<T> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: Float> Agente<T> {
    pub fn novo(cfg: Config, registro: Registro, rng: &mut Rng) -> Self {
        let automato = Automato::compilar(&registro);
        let n = registro.n();
        Self {
            modelo: Teka::new(cfg, rng),
            cabecas: Cabecas::new(cfg.d_bb, cfg.d_loc, n, rng),
            registro,
            automato,
            vies_abster: 0.0,
        }
    }

    /// Constrói sobre um modelo já treinado no corpus de linguagem.
    pub fn sobre(modelo: Teka<T>, registro: Registro, rng: &mut Rng) -> Self {
        let automato = Automato::compilar(&registro);
        let cabecas = Cabecas::new(modelo.cfg.d_bb, modelo.cfg.d_loc, registro.n(), rng);
        Self {
            modelo,
            cabecas,
            registro,
            automato,
            vies_abster: 0.0,
        }
    }

    pub fn grad(&self) -> AgenteGrad<T> {
        AgenteGrad {
            modelo: self.modelo.grad(),
            cabecas: self.cabecas.grad(),
        }
    }

    pub fn n_params(&self) -> usize {
        self.modelo.n_params() + self.cabecas.n_params()
    }

    pub fn params_mut(&mut self) -> Vec<&mut [T]> {
        let mut v = self.modelo.params_mut();
        v.extend(self.cabecas.params_mut());
        v
    }

    pub fn descritores(&self) -> Vec<(String, Vec<usize>)> {
        let mut v = self.modelo.descritores();
        v.extend(self.cabecas.descritores());
        v
    }

    /// Por ferramenta: `(quantos slots existem, quantos sao obrigatorios)`.
    ///
    /// A separacao importa: obrigatorio sempre entra na chamada, opcional so entra
    /// se a cabeca de presenca disser que sim.
    pub fn slots_por_ferramenta(&self) -> Vec<(usize, usize)> {
        self.registro
            .ferramentas
            .iter()
            .map(|f| {
                (
                    f.params.len().min(MAX_SLOTS),
                    f.params.iter().filter(|p| p.obrigatorio).count().min(MAX_SLOTS),
                )
            })
            .collect()
    }

    /// A leitura que ela fez do pedido: a média do estado do backbone sobre a
    /// frase inteira.
    ///
    /// É a chave da memória episódica — dois pedidos que ela lê parecido têm
    /// assinaturas próximas, e é por cosseno entre elas que se recupera "o que já
    /// aconteceu parecido com isto". Vale para `batch = 1`.
    ///
    /// ## Por que a média, e não o último patch
    ///
    /// Era o último patch, e o argumento parecia bom: o backbone é recorrente,
    /// então no fim já passou tudo. Mas o estado tem porta e **decai** — o último
    /// patch pesa o FIM da frase, e o assunto costuma estar no meio:
    ///
    /// ```text
    /// "como esta a ram do computador"   termina em "computador", assunto "ram"
    /// "quanto de memoria esta em uso"   termina em "uso",        assunto "memoria"
    /// ```
    ///
    /// Com 20 ferramentas isso dava certo; com 22 parou. Medido no mesmo modelo
    /// treinado, cosseno com o episódio CERTO contra os três distratores:
    ///
    /// ```text
    ///                        certo   distratores       recuperou
    /// último patch           0,043   0,079..0,158      ERRADO
    /// média da frase         0,612   0,370..0,465      certo
    /// ```
    ///
    /// 0,043 no episódio certo não é "quase" — os quatro candidatos ficavam entre
    /// 0,04 e 0,16, que é argmax sobre ruído. Não era falta de treino: a acurácia
    /// era idêntica com 20 e com 22 ferramentas (53,9% contra 54,0%), então o que
    /// mudou foi a geometria, não a competência.
    ///
    /// Média simples, sem descartar cauda. Descartar os dois últimos patches dava
    /// margem melhor (+0,217 contra +0,147), mas esse `k` seria uma constante
    /// escolhida em cima de uma consulta só — régua feita para o teste passar.
    pub fn assinatura(&self, cache: &AgenteCache<T>) -> Vec<f32> {
        let d = self.cabecas.d;
        let np = cache.cabecas.n_patches.first().copied().unwrap_or(0);
        // Sem patches não há leitura; cair no `zf` mantém o contrato de devolver
        // sempre `d` números em vez de um vetor vazio que viraria cosseno zero.
        if np == 0 {
            return cache.cabecas.zf.iter().take(d).map(|v| v.to_f64() as f32).collect();
        }
        let z = cache.modelo.z();
        let mut m = vec![0.0f32; d];
        for p in 0..np {
            // `z` é `[p_max, batch, d]`; com `batch = 1` o patch `p` começa em `p·d`.
            let ini = p * d;
            for (i, x) in m.iter_mut().enumerate() {
                *x += z[ini + i].to_f64() as f32;
            }
        }
        for x in m.iter_mut() {
            *x /= np as f32;
        }
        m
    }

    /// Um passo de compreensão: tronco → cabeças. Com `grad`, treina.
    #[allow(clippy::too_many_arguments)]
    pub fn compreender<O: Ops<T>>(
        &self,
        ops: &O,
        bytes: &[u8],
        plano: &Plano,
        est: &Estado<T>,
        alvos: &[Alvo],
        cache: &mut AgenteCache<T>,
        grad: Option<&mut AgenteGrad<T>>,
    ) -> (f64, Placar) {
        let slots = self.slots_por_ferramenta();
        self.modelo.tronco(ops, bytes, plano, est, &mut cache.modelo);

        match grad {
            None => {
                let (z, e) = (cache.modelo.z(), cache.modelo.e());
                // Empréstimos imutáveis de campos distintos coexistem sem problema;
                // só o caminho com gradiente precisa do acessor conjunto.
                let (z, e) = (z.to_vec(), e.to_vec());
                self.cabecas.passo(
                    ops,
                    &z,
                    &e,
                    &plano.n_patches,
                    plano.p_max,
                    plano.seq,
                    &plano.comprimentos,
                    &plano.patch_de_byte,
                    alvos,
                    &slots,
                    &mut cache.cabecas,
                    None,
                )
            }
            Some(g) => {
                // As cabeças ACUMULAM em `dz` e `de`, então zerar antes é parte do
                // contrato.
                cache.modelo.zerar_portas();
                let (perda, placar) = {
                    // Os quatro saem de um empréstimo só: são campos distintos, mas
                    // o compilador só sabe disso se vierem juntos.
                    let (z, e, dz, de) = cache.modelo.portas();
                    self.cabecas.passo(
                        ops,
                        z,
                        e,
                        &plano.n_patches,
                        plano.p_max,
                        plano.seq,
                        &plano.comprimentos,
                        &plano.patch_de_byte,
                        alvos,
                        &slots,
                        &mut cache.cabecas,
                        Some((&mut g.cabecas, dz, de)),
                    )
                };
                self.modelo
                    .tronco_backward(ops, bytes, plano, est, &mut cache.modelo, &mut g.modelo);
                (perda, placar)
            }
        }
    }

    /// Do pedido à chamada, **amostrando** a ferramenta da política.
    ///
    /// `temperatura = 0` é o comportamento normal (argmax). Acima disso ela
    /// experimenta, e é o que permite ao reforço descobrir uma ação melhor em vez
    /// de só fugir da que falhou. Não é o caminho de produção: é o modo de
    /// aprendizado.
    pub fn responder_explorando<O: Ops<T>, P: Patcher + ?Sized>(
        &self,
        ops: &O,
        patcher: &P,
        pedido: &str,
        temperatura: f64,
        rng: &mut Rng,
        cache: &mut AgenteCache<T>,
    ) -> Result<Chamada, String> {
        if temperatura <= 0.0 {
            return self.responder(ops, patcher, pedido, cache);
        }
        let bytes = pedido.as_bytes().to_vec();
        let seq = bytes.len();
        if seq == 0 {
            return Err("pedido vazio".into());
        }
        let plano = Plano::novo(patcher, &bytes, seq, 1);
        let est = self.modelo.estado_zero(1);
        let vazio = vec![Alvo::vazio(0)];
        self.compreender(ops, &bytes, &plano, &est, &vazio, cache, None);

        let p = self.cabecas.politica(&cache.cabecas, 0, temperatura);
        let mut acum = 0.0;
        let sorteio = rng.uniform01();
        let mut escolhida = p.len() - 1;
        for (i, v) in p.iter().enumerate() {
            acum += v;
            if sorteio <= acum {
                escolhida = i;
                break;
            }
        }
        let decisao = &self.cabecas.decidir_forcando(
            &cache.cabecas,
            &plano.n_patches,
            plano.seq,
            &self.slots_por_ferramenta(),
            &[escolhida],
        )[0];
        self.escrever(&bytes, decisao)
    }

    /// Do pedido à chamada: compreende, recorta os argumentos, e escreve pela
    /// gramática. O resultado é sempre sintaticamente válido — ou é erro explícito.
    pub fn responder<O: Ops<T>, P: Patcher + ?Sized>(
        &self,
        ops: &O,
        patcher: &P,
        pedido: &str,
        cache: &mut AgenteCache<T>,
    ) -> Result<Chamada, String> {
        self.responder_com_confianca(ops, patcher, pedido, cache)
            .map(|(ch, _)| ch)
    }

    /// Tudo até a decisão: bytes do pedido, o alvo escolhido, e a confiança.
    ///
    /// Privado e compartilhado por `responder_com_confianca` e `responder_ou_falta`
    /// — os dois fazem o mesmo forward e só divergem no que fazem quando a chamada
    /// não pode ser escrita.
    fn decidir<O: Ops<T>, P: Patcher + ?Sized>(
        &self,
        ops: &O,
        patcher: &P,
        pedido: &str,
        cache: &mut AgenteCache<T>,
    ) -> Result<(Vec<u8>, Alvo, Confianca), String> {
        let bytes = pedido.as_bytes().to_vec();
        let seq = bytes.len();
        if seq == 0 {
            return Err("pedido vazio".into());
        }
        let plano = Plano::novo(patcher, &bytes, seq, 1);
        let est = self.modelo.estado_zero(1);
        let vazio = vec![Alvo::vazio(0)];
        self.compreender(ops, &bytes, &plano, &est, &vazio, cache, None);

        // -- viés de abstenção --
        //
        // `perguntar` é uma classe "todo o resto": nenhuma quantidade de exemplos
        // delimita "tudo que não são as outras nove", então a fronteira dela nasce
        // frouxa e ela recusa coisa que sabe fazer. Medido: 8 abstenções falsas em
        // 59 pedidos válidos, algumas com margem 1,000 — recusa **convicta**.
        //
        // Isto não é um conserto, é um **botão**: viés positivo faz duvidar mais
        // (erra menos, recusa mais), negativo faz duvidar menos. Onde parar depende
        // de quanto custa cada erro, e isso é decisão de quem usa, não do modelo.
        if self.vies_abster != 0.0 {
            if let Some(i) = self.registro.indice("perguntar") {
                let l = &mut cache.cabecas.logits_int[i];
                *l = *l + T::from_f64(self.vies_abster);
            }
        }

        let conf = Confianca::ler(&cache.cabecas, 0, self.registro.n());

        let decisao = self
            .cabecas
            .decidir(
                &cache.cabecas,
                &plano.n_patches,
                plano.seq,
                &self.slots_por_ferramenta(),
            )
            .swap_remove(0);

        Ok((bytes, decisao, conf))
    }

    /// Igual a [`Teka::responder`], mas devolve também o quanto ela estava certa
    /// disso.
    pub fn responder_com_confianca<O: Ops<T>, P: Patcher + ?Sized>(
        &self,
        ops: &O,
        patcher: &P,
        pedido: &str,
        cache: &mut AgenteCache<T>,
    ) -> Result<(Chamada, Confianca), String> {
        let (bytes, decisao, conf) = self.decidir(ops, patcher, pedido, cache)?;
        self.escrever(&bytes, &decisao).map(|ch| (ch, conf))
    }

    /// Como [`Teka::responder_com_confianca`], mas quando a chamada não pode ser
    /// escrita ela diz **por quê** em vez de só falhar.
    ///
    /// ## O caso que isto existe para atender
    ///
    /// Das 196 frases que o John escreveu em 11/09, **59 — quase um terço — pedem
    /// uma ferramenta cujo argumento obrigatório é um caminho, e não trazem caminho
    /// nenhum**: *"apaga esse arquivo aqui"*, *"me descreve essa foto"*.
    ///
    /// O ponteiro COPIA um trecho do pedido; ele não inventa `C:\...oto.jpg`. E a
    /// gramática proíbe fechar a chamada com obrigatório faltando — `fechar()` só é
    /// alcançável quando o parâmetro e todos os seguintes são opcionais. As duas
    /// coisas estão certas, e juntas produzem `Err("chamada incompleta")`.
    ///
    /// Erro é a resposta errada para isso. Ela **sabe** que é `apagar_arquivo`; o
    /// que falta é o John dizer qual. A resposta certa é perguntar.
    ///
    /// Não é conserto de acurácia: nenhum treino faz o ponteiro copiar o que não
    /// está escrito. É mecanismo.
    pub fn responder_ou_falta<O: Ops<T>, P: Patcher + ?Sized>(
        &self,
        ops: &O,
        patcher: &P,
        pedido: &str,
        cache: &mut AgenteCache<T>,
    ) -> Result<(Chamada, Confianca), Falta> {
        let (bytes, decisao, conf) = self
            .decidir(ops, patcher, pedido, cache)
            .map_err(Falta::Outra)?;
        match self.escrever(&bytes, &decisao) {
            Ok(ch) => Ok((ch, conf)),
            Err(e) => match self.obrigatorio_ausente(&bytes, &decisao) {
                Some(param) => Err(Falta::Argumento {
                    ferramenta: decisao.ferramenta,
                    param,
                }),
                None => Err(Falta::Outra(e)),
            },
        }
    }

    /// O primeiro parâmetro obrigatório que o pedido não traz, se houver.
    ///
    /// Duas formas de faltar, e as duas contam: a cabeça de presença disse que o
    /// argumento não aparece (`bytes[slot] == None`), ou o ponteiro apontou para um
    /// trecho vazio. A gramática rejeita as duas do mesmo jeito.
    fn obrigatorio_ausente(&self, bytes: &[u8], decisao: &Alvo) -> Option<String> {
        let f = &self.registro.ferramentas[decisao.ferramenta];
        f.params
            .iter()
            .enumerate()
            .take(MAX_SLOTS)
            .find(|(slot, p)| {
                p.obrigatorio
                    && match decisao.bytes[*slot] {
                        None => true,
                        Some((bi, bf)) => recortar(bytes, bi, bf).is_empty(),
                    }
            })
            .map(|(_, p)| p.nome.clone())
    }

    /// Escreve a chamada pela gramática a partir de uma decisão já tomada.
    /// O `plano` deixou de ser necessario quando o ponteiro passou a apontar para
    /// bytes: o recorte virou uma fatia direta do pedido.
    fn escrever(&self, bytes: &[u8], decisao: &Alvo) -> Result<Chamada, String> {
        let f = &self.registro.ferramentas[decisao.ferramenta];
        let mut d = Decodificador::novo(&self.automato);
        d.avancar_todos(format!("{{\"acao\":\"{}\"", f.nome).as_bytes())?;

        for (slot, p) in f.params.iter().enumerate().take(MAX_SLOTS) {
            let Some((bi, bf)) = decisao.bytes[slot] else {
                continue;
            };
            let valor = recortar(&bytes, bi, bf);
            if valor.is_empty() {
                if p.obrigatorio {
                    return Err(format!("{}: nao consegui recortar {}", f.nome, p.nome));
                }
                continue;
            }
            d.avancar_todos(format!(",\"{}\":\"", p.nome).as_bytes())?;
            // Se a gramática recusar algum byte do recorte, o erro aparece aqui —
            // e não vira uma chamada torta.
            d.avancar_todos(valor.as_bytes())?;
            d.avancar_todos(b"\"")?;
        }
        d.avancar_todos(b"}")?;
        d.chamada(&self.registro)
    }

    /// Atende de ponta a ponta: entende, monta a chamada, executa.
    pub fn agir<O: Ops<T>, P: Patcher + ?Sized>(
        &self,
        ops: &O,
        patcher: &P,
        pedido: &str,
        pol: &Politica,
        cache: &mut AgenteCache<T>,
    ) -> (Result<Chamada, String>, Result<String, String>) {
        match self.responder(ops, patcher, pedido, cache) {
            Ok(c) => {
                let saida = self.registro.executar(&c, pol);
                (Ok(c), saida)
            }
            Err(e) => (Err(e.clone()), Err(e)),
        }
    }
}

/// Recorta `bytes[bi..=bf]` do pedido, sem espaço nas pontas.
///
/// Com o ponteiro apontando direto para bytes, o recorte é literalmente uma fatia.
/// A aparação continua por segurança: o modelo pode escolher uma extremidade que
/// caia num espaço, e um argumento com espaço sobrando quase sempre quebra a
/// ferramenta.
pub fn recortar(bytes: &[u8], bi: usize, bf: usize) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let bi = bi.min(bytes.len() - 1);
    let bf = bf.min(bytes.len() - 1).max(bi);
    let (bi, bf) = encaixar_na_palavra(bytes, bi, bf);
    let bruto = String::from_utf8_lossy(&bytes[bi..=bf]).trim().to_string();
    aparar_pontuacao(&bruto)
}

/// Estende o recorte até as fronteiras de palavra que o contêm.
///
/// ## O defeito que isto conserta
///
/// O ponteiro fino escolhe dois bytes por produto escalar contra o embedding de cada
/// posição. Para uma palavra vista no treino, os embeddings das bordas são
/// familiares; para uma inédita, não. Ele acerta o patch e **escorrega dentro dele**:
///
/// ```text
/// chama o ipconfig no terminal   → {"comando":"g"}
/// quero acionar o comando hostname → {"comando":"hostnam"}
/// salva a observacao urgente ...  → {"texto":"rgente"}
/// ```
///
/// Nos três casos a decisão estava certa e o recorte errado por um punhado de bytes.
///
/// ## Por que isto não é o viés de fronteira que já falhou
///
/// O Achado 8 registra uma tentativa de somar um viés de fronteira **ao score
/// durante o treino**, e ela piorou o argumento de 94,3% para 88,4%. Aquilo mexia no
/// que o modelo aprende. Isto é determinístico e roda **depois** da decisão: não
/// altera gradiente nenhum, e sobre um recorte já correto é a identidade.
///
/// Essa última propriedade não é suposição — está medida em
/// `o_encaixe_e_identidade_nos_argumentos_reais`, que roda os 357 argumentos do
/// corpus e do benchmark e exige que nenhum mude.
fn encaixar_na_palavra(bytes: &[u8], bi: usize, bf: usize) -> (usize, usize) {
    let e_espaco = |b: u8| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
    let mut i = bi;
    while i > 0 && !e_espaco(bytes[i - 1]) {
        i -= 1;
    }
    let mut f = bf;
    while f + 1 < bytes.len() && !e_espaco(bytes[f + 1]) {
        f += 1;
    }
    (i, f)
}

/// Tira parênteses que envolvem o valor inteiro, sem tocar nos que fazem parte dele.
fn descascar_parenteses(s: &str) -> String {
    let mut atual = s.to_string();
    loop {
        let b = atual.as_bytes();
        if b.len() < 3 || b[0] != b'(' || b[b.len() - 1] != b')' {
            return atual;
        }
        // O par externo só envolve tudo se o miolo fechar sozinho.
        let miolo = &atual[1..atual.len() - 1];
        let mut nivel = 0i32;
        let mut equilibrado = true;
        for c in miolo.chars() {
            match c {
                '(' => nivel += 1,
                ')' => {
                    nivel -= 1;
                    if nivel < 0 {
                        equilibrado = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !equilibrado || nivel != 0 {
            return atual;
        }
        atual = miolo.to_string();
    }
}

/// Tira a pontuação que grudou na palavra mas não faz parte do valor.
///
/// 4,5% dos argumentos reais aparecem colados a `,`, `:` ou `?` no pedido
/// (`o texto de notas.md, por favor`). Estender até o espaço traz a vírgula junto, e
/// `notas.md,` não abre.
///
/// O `:` tem exceção: **`C:` é um argumento válido que termina em dois-pontos.**
/// Aparar sem olhar o tamanho transformaria o disco `C:` em `C`.
fn aparar_pontuacao(s: &str) -> String {
    // Parênteses que ENVOLVEM o valor inteiro não fazem parte dele. O caso real:
    // `(1024*8) tá complicado, resolve pra mim` — o argumento é `1024*8`, e
    // estender até o espaço traz o par junto.
    //
    // A condição é envolver **tudo**: `(12+8)*5` também começa com `(`, mas ali o
    // parêntese é parte da conta e não pode sair. Por isso o teste é "abre no
    // primeiro byte e fecha no último, e o miolo fica equilibrado".
    let s = descascar_parenteses(s);
    let s = s.as_str();
    let mut fim = s.len();
    while fim > 0 {
        let c = s[..fim].chars().next_back().unwrap_or(' ');
        let apara = match c {
            ',' | ';' | '?' | '!' | '.' => true,
            // Só apara o dois-pontos quando não pode ser letra de unidade.
            ':' => fim > 2,
            _ => false,
        };
        if !apara {
            break;
        }
        fim -= c.len_utf8();
    }
    let mut ini = 0;
    while ini < fim {
        let c = s[ini..].chars().next().unwrap_or(' ');
        // `.` de abertura fica: `.gitignore` é nome de arquivo.
        if matches!(c, ',' | ';' | '?' | '!' | ':') {
            ini += c.len_utf8();
        } else {
            break;
        }
    }
    s[ini..fim].trim().to_string()
}

#[cfg(test)]
mod testes_falta {
    use super::*;
    use crate::model::heads::Alvo;

    fn reg() -> Registro {
        Registro::padrao()
    }

    /// A pergunta sai do TIPO do parametro, nao do nome dele.
    ///
    /// "qual caminho para apagar_arquivo?" e como o registro fala. Uma pessoa
    /// pergunta "qual arquivo ou pasta". O nome do parametro e detalhe de
    /// implementacao vazando para a cara do usuario.
    #[test]
    fn a_pergunta_nomeia_a_ferramenta_e_fala_como_gente() {
        let r = reg();
        let i = r.indice("apagar_arquivo").expect("apagar_arquivo existe");
        let f = Falta::Argumento { ferramenta: i, param: "caminho".into() };
        let q = f.pergunta(&r).expect("tem pergunta");
        assert!(q.contains("arquivo ou pasta"), "fala de caminho como gente: {q}");
        assert!(
            q.contains("apagar_arquivo"),
            "nomear a ferramenta E a prova de que ela entendeu o pedido: {q}"
        );
    }

    /// `Outra` nao gera pergunta: nao ha o que perguntar quando ela nem sabe o que
    /// fazer. Sem isto, uma falha qualquer viraria "o que para ?" na cara de quem usa.
    #[test]
    fn falha_que_nao_e_argumento_nao_vira_pergunta() {
        assert_eq!(Falta::Outra("pedido vazio".into()).pergunta(&reg()), None);
    }

    /// As DUAS formas de faltar contam.
    ///
    /// A cabeca de presenca pode dizer "o argumento nao aparece" (`None`), ou o
    /// ponteiro pode apontar para um trecho vazio. A gramatica rejeita as duas
    /// igual, entao o diagnostico tem de pegar as duas — senao metade dos casos
    /// deiticos continuaria virando erro cru.
    #[test]
    fn ausente_pega_slot_vazio_e_slot_sem_ponteiro() {
        let r = reg();
        let ag = Agente::<f32>::novo(Config::pequeno(), r, &mut Rng::new(1));
        let i = ag.registro.indice("apagar_arquivo").unwrap();
        let pedido = b"apaga esse arquivo aqui";

        let mut sem_ponteiro = Alvo::vazio(i);
        sem_ponteiro.ferramenta = i;
        sem_ponteiro.bytes[0] = None;
        assert_eq!(
            ag.obrigatorio_ausente(pedido, &sem_ponteiro).as_deref(),
            Some("caminho")
        );

        // O segundo caso precisou ser MEDIDO para o teste fazer sentido. Minha
        // primeira versao apontava para o espaco em `(5, 5)` e falhou -- `recortar`
        // ENCAIXA NA PALAVRA e devolveu "apaga esse", que nao e vazio. O recorte so
        // sai vazio quando o trecho e pontuacao pura, porque `aparar_pontuacao`
        // come tudo:
        //
        //     "apaga esse arquivo aqui" (5,5) -> "apaga esse"
        //     "apaga , aqui"            (6,6) -> ""
        let com_virgula = b"apaga , aqui";
        let mut trecho_vazio = Alvo::vazio(i);
        trecho_vazio.ferramenta = i;
        trecho_vazio.bytes[0] = Some((6, 6));
        assert_eq!(
            ag.obrigatorio_ausente(com_virgula, &trecho_vazio).as_deref(),
            Some("caminho")
        );
    }

    /// E o outro lado, que e o que impede o mecanismo de virar desculpa: quando o
    /// argumento ESTA la, nao ha falta nenhuma. Sem esta metade, um bug que zerasse
    /// todos os ponteiros viraria "ela esta perguntando educadamente".
    #[test]
    fn com_o_argumento_presente_nao_falta_nada() {
        let r = reg();
        let ag = Agente::<f32>::novo(Config::pequeno(), r, &mut Rng::new(1));
        let i = ag.registro.indice("apagar_arquivo").unwrap();
        let pedido = b"apaga notas.md";
        let mut cheio = Alvo::vazio(i);
        cheio.ferramenta = i;
        cheio.bytes[0] = Some((6, 14));
        assert_eq!(&pedido[6..14], b"notas.md");
        assert_eq!(ag.obrigatorio_ausente(pedido, &cheio), None);
    }

    /// Ferramenta sem parametro nunca falta argumento.
    #[test]
    fn ferramenta_sem_parametro_nao_falta() {
        let r = reg();
        let ag = Agente::<f32>::novo(Config::pequeno(), r, &mut Rng::new(1));
        let i = ag.registro.indice("hora").unwrap();
        let mut a = Alvo::vazio(i);
        a.ferramenta = i;
        assert_eq!(ag.obrigatorio_ausente(b"que horas sao", &a), None);
    }
}

#[cfg(test)]
mod testes_recorte {
    use super::*;

    /// Todo argumento real, recortado exatamente, tem de sobreviver intacto.
    ///
    /// É este teste que autoriza o encaixe a existir. Se ele quebrar, o encaixe está
    /// estragando recortes que já estavam certos — que é exatamente como o viés de
    /// fronteira do Achado 8 piorou as coisas.
    #[test]
    fn o_encaixe_e_identidade_nos_argumentos_reais() {
        let corpus = concat!(
            include_str!("../../dados/exemplos_teka.txt"),
            "\n",
            include_str!("../../dados/frases_teste.txt")
        );
        let (mut testados, mut mudaram) = (0usize, Vec::new());
        for linha in corpus.lines() {
            let linha = linha.trim();
            if linha.is_empty() || linha.starts_with('#') {
                continue;
            }
            let campos: Vec<&str> = linha.split('|').map(str::trim).collect();
            if campos.len() < 3 {
                continue;
            }
            let pedido = campos[1];
            for arg in &campos[2..] {
                if arg.is_empty() {
                    continue;
                }
                let Some(i) = pedido.find(arg) else { continue };
                testados += 1;
                let saida = recortar(pedido.as_bytes(), i, i + arg.len() - 1);
                if saida != *arg {
                    mudaram.push(format!("{pedido:?}: {arg:?} -> {saida:?}"));
                }
            }
        }
        assert!(testados > 300, "esperava centenas de argumentos, vi {testados}");
        assert!(
            mudaram.is_empty(),
            "o encaixe estragou {} recorte(s) que estavam certos:\n{}",
            mudaram.len(),
            mudaram.join("\n")
        );
    }

    #[test]
    fn conserta_o_escorregao_dentro_da_palavra() {
        let p = "chama o ipconfig no terminal";
        // O ponteiro escolheu so o 'g' de ipconfig.
        let i = p.find('g').unwrap();
        assert_eq!(recortar(p.as_bytes(), i, i), "ipconfig");

        let p2 = "quero acionar o comando hostname";
        let i2 = p2.find("hostnam").unwrap();
        assert_eq!(recortar(p2.as_bytes(), i2, i2 + 6), "hostname");

        let p3 = "salva a observacao urgente em avisos.txt";
        let i3 = p3.find("rgente").unwrap();
        assert_eq!(recortar(p3.as_bytes(), i3, i3 + 5), "urgente");
    }

    #[test]
    fn a_pontuacao_colada_nao_entra_no_valor() {
        let p = "o texto de notas.md, por favor";
        let i = p.find("notas").unwrap();
        assert_eq!(recortar(p.as_bytes(), i, i + 2), "notas.md");

        let p2 = "anota isso em lista_de_compras.txt: teste";
        let i2 = p2.find("lista").unwrap();
        assert_eq!(recortar(p2.as_bytes(), i2, i2 + 4), "lista_de_compras.txt");
    }

    #[test]
    fn a_letra_de_unidade_sobrevive() {
        // O caso que uma aparadura ingenua de ':' destruiria.
        let p = "quanto tem livre em C:";
        let i = p.find("C:").unwrap();
        assert_eq!(recortar(p.as_bytes(), i, i + 1), "C:");
        // E o caminho completo tambem.
        let p2 = "lista C:\\temp por favor";
        let j = p2.find("C:").unwrap();
        assert_eq!(recortar(p2.as_bytes(), j, j + 1), "C:\\temp");
    }

    #[test]
    fn acento_nao_e_partido_ao_meio() {
        // "configuração.ini" tem bytes multibyte; encaixar nao pode cortar dentro.
        let p = "o que diz o arquivo configuração.ini";
        let i = p.find("ão").unwrap();
        assert_eq!(recortar(p.as_bytes(), i, i + 4), "configuração.ini");
    }

    #[test]
    fn parentese_que_envolve_sai_e_o_que_faz_parte_fica() {
        // Caso real do corpus.
        let p = "(1024*8) ta complicado, resolve pra mim";
        assert_eq!(recortar(p.as_bytes(), 1, 6), "1024*8");
        // Mas aqui o parentese e a conta: nao pode sair.
        let p2 = "responde esse calculo (12+8)*5";
        let i = p2.find("(12").unwrap();
        assert_eq!(recortar(p2.as_bytes(), i, i + 7), "(12+8)*5");
        // Nao equilibrado fica como esta.
        assert_eq!(descascar_parenteses("(a+b)*(c"), "(a+b)*(c");
    }

    #[test]
    fn recorte_que_atravessa_palavras_e_preservado() {
        // Argumento de duas palavras continua de duas palavras.
        let p = "registra comprar pao no notas.md";
        let i = p.find("comprar").unwrap();
        assert_eq!(recortar(p.as_bytes(), i, i + 10), "comprar pao");
    }
}
