//! O pulso: ela continua rodando quando ninguém está pedindo nada.
//!
//! ## De onde veio
//!
//! Da nila_mind, e a regra que importa está escrita num comentário dela:
//!
//! > *"A mente PENSA todo tick (barato), mas só APRENDE a cada `train_every` ticks.
//! > PENSAR não muda os pesos (só TREINAR muda), e o treino é sempre sobre a memória
//! > REAL, nunca sobre os próprios devaneios."*
//!
//! Essa separação é o que impede o laço de se comer. Aqui ela **não é comentário**:
//! [`pensar`] recebe `&Agente` e [`Pulso::consolidar`] recebe `&mut Agente`. O
//! compilador recusa um devaneio que tente mexer em peso.
//!
//! ## Por que isso não é preciosismo
//!
//! Este projeto já tentou aprender da própria atividade, em `crate::ambiente`. Três
//! sementes, 1.200 tentativas:
//!
//! ```text
//! no ambiente:  72% -> 81%     (subiu)
//! no benchmark: 108,7 -> 95,0  (caiu 13,7 de 150; -18, -10, -13)
//! ```
//!
//! Sala de espelhos. É o Achado 27, e é exatamente o que a regra da nila_mind evita.
//! Copiar o laço e esquecer a regra seria repetir aquilo com mais passos.
//!
//! ## O que acontece em cada tick
//!
//! ```text
//! PERCEBER    a memória episódica cresceu desde o último tick?
//! PENSAR      devaneia N bytes a partir do fio corrente.  NÃO toca em peso.
//! REVER       quais fatos estão prestes a ser esquecidos (Ebbinghaus)
//! CONSOLIDAR  só quando há percepção nova, ou a cada `consolidar_a_cada` ticks.
//!             E só sobre memória REAL — episódio que aconteceu, correção que
//!             você deu.
//! ```
//!
//! O devaneio vai para o arquivo de pulso e serve para **ver o que está na cabeça
//! dela**. Ele não é raciocínio: é continuação de sequência, igual ao da nila_mind.
//! O que aprende é a consolidação.
//!
//! ## O relógio só vale se houver o que consolidar
//!
//! Um tick sobre memória vazia é um relógio girando sobre nada. O pulso rende na
//! proporção em que ela é **usada** — episódio vivido, correção dada. Não há laço
//! que substitua isso.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::afeto::Afeto;
use crate::backend::Ops;
use crate::learn::consolidacao::{consolidar, CfgConsolidacao};
use crate::learn::dados::Exemplo;
use crate::memory::semantica::MemoriaSemantica;
use crate::memory::MemoriaEpisodica;
use crate::model::agente::Agente;
use crate::model::hierarchy::TekaCache;
use crate::model::patcher::{Patcher, Plano};
use crate::rng::Rng;

/// Quantos bytes do fio servem de semente para o próximo devaneio.
const CAUDA_FIO: usize = 96;

/// Depois de tantos ticks sem percepção nova, o fio é reancorado na memória.
///
/// Sem isto ele deriva: cada devaneio parte do anterior, e em algumas dezenas de
/// ticks o texto perde qualquer relação com o que ela viu. A nila_mind reancora por
/// idade e por acaso; aqui é só por idade, que é o que dá para justificar.
const IDADE_MAXIMA_FIO: u32 = 40;

#[derive(Clone, Debug)]
pub struct CfgPulso {
    /// Segundos entre ticks.
    pub intervalo_s: u64,
    /// Ticks entre consolidações, quando não chega percepção nova.
    pub consolidar_a_cada: u64,
    /// Bytes de devaneio por tick. `0` desliga o devaneio (tick só aprende).
    pub bytes_pensamento: usize,
    /// Temperatura do devaneio. Alta divaga, baixa repete.
    pub temperatura: f64,
    /// Onde escrever o estado a cada tick.
    pub arquivo: PathBuf,
    /// Quantos episódios novos bastam para consolidar fora de hora.
    pub gatilho_episodios: usize,
}

impl Default for CfgPulso {
    fn default() -> Self {
        Self {
            // Cinco minutos. O tick de consolidação leva ~1 min; menos que isso e
            // ela passa mais tempo consolidando do que existindo.
            intervalo_s: 300,
            consolidar_a_cada: 12,
            bytes_pensamento: 120,
            temperatura: 0.9,
            arquivo: PathBuf::from("pulso.json"),
            gatilho_episodios: 3,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Relatorio {
    pub tick: u64,
    /// O que uma regra permanente fez neste tick, se fez alguma coisa.
    ///
    /// `None` na esmagadora maioria dos ticks — regra desligada, ou mundo ja como
    /// se quer. Existe porque **regra que age em silencio e regra que ninguem
    /// consegue depurar** quando comeca a fazer besteira sozinha.
    pub vigia: Option<String>,
    pub episodios_novos: usize,
    pub consolidou: bool,
    /// Chegou a hora de consolidar (por relogio ou por percepcao nova)?
    ///
    /// Separado de `consolidou` porque os dois casos parecem iguais no log e nao
    /// sao: "ainda nao e hora" e um laco saudavel; "e hora e nao ha o que aprender"
    /// e um laco girando em falso, e isso precisa aparecer.
    pub tentou: bool,
    /// Episodios com correcao ou aprovacao — os unicos que viram treino.
    pub ensinaveis: usize,
    pub passos: usize,
    /// Acurácia de intenção na validação, antes e depois da consolidação.
    pub antes: f64,
    pub depois: f64,
    /// Fatos abaixo do piso de esquecimento agora.
    pub esquecendo: usize,
    pub pensamento: String,
    pub segundos: f64,
    /// Surpresa do tick, em bits por byte.
    pub surpresa: f32,
    /// Estado afetivo depois deste tick.
    pub afeto: String,
    /// Temperatura que ESTE tick usou (vinda do afeto do tick anterior).
    pub temperatura: f64,
    /// Multiplicador de taxa que a consolidacao deste tick usou.
    pub lr_escala: f64,
}

/// Devaneia `n` bytes a partir de `semente`.
///
/// **Recebe `&Agente` de propósito.** É a regra da nila_mind virada em tipo: pensar
/// não pode mudar peso, e aqui o compilador é quem garante.
///
/// O custo é quadrático em `n` (cada byte reprocessa o prefixo), o que para 120
/// bytes uma vez a cada cinco minutos é irrelevante — e evita ter de expor e
/// carregar o estado recorrente por fora.
pub fn pensar<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &P,
    semente: &str,
    n: usize,
    temperatura: f64,
    rng: &mut Rng,
) -> String {
    if n == 0 {
        return String::new();
    }
    let mut bytes: Vec<u8> = semente.as_bytes().to_vec();
    if bytes.is_empty() {
        bytes.push(b' ');
    }
    let inicio = bytes.len();
    let mut cache = TekaCache::new();

    for _ in 0..n {
        let seq = bytes.len();
        let plano = Plano::novo(patcher, &bytes, seq, 1);
        let est = ag.modelo.estado_zero(1);
        // `alvos` só é usado para a perda, que aqui se descarta.
        let alvos = vec![b' '; seq];
        let _ = ag
            .modelo
            .passo(ops, &bytes, &alvos, &plano, &est, &mut cache, None);

        let base = (seq - 1) * 256;
        let linha = &cache.logits[base..base + 256];
        bytes.push(amostrar(linha, temperatura, rng));
    }
    String::from_utf8_lossy(&bytes[inicio..]).into_owned()
}

/// Amostra um byte dos logits, com temperatura.
fn amostrar(logits: &[f32], temperatura: f64, rng: &mut Rng) -> u8 {
    let t = temperatura.max(0.05);
    let maxi = logits.iter().fold(f32::NEG_INFINITY, |m, &v| m.max(v)) as f64;
    let exps: Vec<f64> = logits
        .iter()
        .map(|&v| ((v as f64 - maxi) / t).exp())
        .collect();
    let soma: f64 = exps.iter().sum();
    let mut alvo = rng.uniform01() * soma;
    for (i, e) in exps.iter().enumerate() {
        alvo -= e;
        if alvo <= 0.0 {
            return i as u8;
        }
    }
    b' '
}

/// Quanto de ganho de acuracia conta como uma recompensa cheia.
///
/// Dois pontos de intencao num tick e um tick muito bom; e a escala que transforma
/// o delta da consolidacao em algo que o afeto consegue ler (-1..1).
const GANHO_CHEIO: f64 = 0.02;

/// Surpresa em bits por byte sobre a cauda da memoria episodica.
///
/// Serve de "o que esta acontecendo" para o afeto. Ela cai naturalmente conforme a
/// consolidacao aprende esses episodios — e isso e a mecanica, nao um defeito: menos
/// surpresa alimenta o tedio, tedio empurra exploracao e reduz plasticidade. E a
/// mente dizendo "ja tirei o que dava daqui".
fn surpresa_bits<O: Ops<f32>, P: Patcher + ?Sized>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &P,
    mem: &MemoriaEpisodica,
) -> f32 {
    let texto: String = mem
        .episodios
        .iter()
        .rev()
        .take(8)
        .map(|e| e.pedido.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if texto.trim().is_empty() {
        // Nada aconteceu ainda. Surpresa zero alimenta tedio, que e o estado certo
        // para um laco girando sobre memoria vazia.
        return 0.0;
    }
    let bytes = texto.as_bytes().to_vec();
    let seq = bytes.len();
    let plano = Plano::novo(patcher, &bytes, seq, 1);
    let est = ag.modelo.estado_zero(1);
    // Alvo = o proximo byte de verdade: e disso que sai a perda de modelo de
    // linguagem. `passo` devolve em nats; `/ LN2` converte para bits.
    let mut alvos = bytes[1..].to_vec();
    alvos.push(b' ');
    let mut cache = TekaCache::new();
    let perda = ag
        .modelo
        .passo(ops, &bytes, &alvos, &plano, &est, &mut cache, None);
    let bits = perda / crate::learn::train::LN2;
    if bits.is_finite() {
        bits.clamp(0.0, 8.0) as f32
    } else {
        0.0
    }
}

/// O laço. Guarda o fio de pensamento entre ticks.
pub struct Pulso {
    ticks: u64,
    vistos: usize,
    fio: String,
    idade_fio: u32,
    rng: Rng,
    /// O estado afetivo. Modula a temperatura do devaneio e a plasticidade da
    /// consolidacao — dois numeros que antes eram constante chutada.
    pub afeto: Afeto,
}

impl Pulso {
    pub fn novo(semente: u64) -> Self {
        Self {
            ticks: 0,
            vistos: 0,
            fio: String::new(),
            idade_fio: 0,
            rng: Rng::new(semente),
            afeto: Afeto::novo(),
        }
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Reancora o fio no que ela viu de verdade.
    fn reancorar(&mut self, mem: &MemoriaEpisodica) {
        let cauda: String = mem
            .episodios
            .iter()
            .rev()
            .take(4)
            .map(|e| e.pedido.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.fio = if cauda.is_empty() { String::from("teka") } else { cauda };
        self.idade_fio = 0;
    }

    /// Roda as regras permanentes, se houver alguma ligada.
    ///
    /// Devolve o que aconteceu, para o relatorio — uma regra que age em silencio e
    /// uma regra que ninguem consegue depurar quando der errado.
    /// Publica de proposito: e o unico ponto do tick que AGE no mundo, e precisa
    /// ser exercitavel sozinha — testar automacao so pelo laco inteiro e como
    /// testar um freio dirigindo o carro.
    pub fn vigiar(&mut self) -> Option<String> {
        // `ativas`, e nao `tabela`: o arquivo diz o PADRAO, mas ligar uma regra e
        // um pedido falado ("toda vez que a musica parar, retoma") e o estado que
        // vale e o desta execucao.
        let regras = crate::tools::regras::ativas();
        let regra = regras.first()?;

        // So agora fala com a ponte. Se ela nao estiver de pe, a regra fica quieta
        // em vez de encher o log a cada tick — o Spotify pode simplesmente nao
        // estar aberto, e isso nao e erro.
        let ponte = crate::tools::ponte::global().ok()?;
        let resposta = ponte.comando("getdata", None).ok()?;
        let tocando = resposta.contains("\"tocando\":true");

        let passo = {
            let mut v = crate::tools::regras::vigia().lock().ok()?;
            v.olhar(tocando, Instant::now())
        };
        use crate::tools::regras::Passo;
        match passo {
            Passo::Age => {
                let r = crate::tools::teclado::mandar(&regra.acao);
                Some(match r {
                    Ok(s) => format!("{}: {s}", regra.nome),
                    Err(e) => format!("{}: falhou ({e})", regra.nome),
                })
            }
            Passo::Desistiu => Some(format!("{}: desisti, a acao nao resolve", regra.nome)),
            Passo::Suspensa | Passo::Quieto => None,
        }
    }

    /// Um tick.
    ///
    /// `ag` é `&mut` porque a consolidação muda peso. O devaneio, dentro, só recebe
    /// `&*ag` — a fronteira entre pensar e aprender é verificada pelo compilador.
    #[allow(clippy::too_many_arguments)]
    pub fn tick<O: Ops<f32>, P: Patcher + ?Sized>(
        &mut self,
        ag: &mut Agente<f32>,
        ops: &O,
        patcher: &P,
        mem: &MemoriaEpisodica,
        sem: &MemoriaSemantica,
        agora_dias: f64,
        base: &[Exemplo],
        validacao: &[Exemplo],
        cfg: &CfgPulso,
    ) -> Relatorio {
        let t0 = Instant::now();
        self.ticks += 1;

        // ---- PERCEBER ----
        let novos = mem.len().saturating_sub(self.vistos);
        self.vistos = mem.len();

        // ---- VIGIAR (regras permanentes) ----
        //
        // O unico lugar do tick em que ela AGE no mundo. Fica antes de tudo de
        // proposito: o devaneio e a consolidacao gastam tempo, e uma regra que so
        // dispara depois deles reagiria com atraso de segundos.
        //
        // Sai barato quando nao ha regra ligada — nem chega a falar com a ponte.
        let vigia = self.vigiar();

        // ---- reancorar o fio ----
        if novos > 0 || self.fio.is_empty() || self.idade_fio >= IDADE_MAXIMA_FIO {
            self.reancorar(mem);
        }

        // ---- PENSAR (todo tick, barato, sem tocar em peso) ----
        let semente: String = {
            let b = self.fio.as_bytes();
            let ini = b.len().saturating_sub(CAUDA_FIO);
            String::from_utf8_lossy(&b[ini..]).into_owned()
        };
        // A temperatura vem do afeto do tick ANTERIOR, nao de `cfg`. Excitacao e
        // tedio a sobem (pensamento mais ousado), satisfacao a desce. `cfg.temperatura`
        // segue existindo como piso para quem quiser fixar.
        let temperatura = self.afeto.temperatura().max(cfg.temperatura.min(0.7));
        let pensamento = pensar(
            &*ag,
            ops,
            patcher,
            &semente,
            cfg.bytes_pensamento,
            temperatura,
            &mut self.rng,
        );
        self.fio.push_str(&pensamento);
        self.idade_fio += 1;

        // ---- REVER: o que está prestes a sumir ----
        let esquecendo = sem
            .fatos
            .iter()
            .filter(|f| f.vigente() && f.forca(agora_dias) < f.camada.piso())
            .count();

        // ---- CONSOLIDAR: só sobre memória REAL ----
        let vai = novos >= cfg.gatilho_episodios
            || (cfg.consolidar_a_cada > 0 && self.ticks % cfg.consolidar_a_cada == 0);
        let mut rel = Relatorio {
            tick: self.ticks,
            vigia,
            episodios_novos: novos,
            esquecendo,
            pensamento,
            tentou: vai,
            ensinaveis: mem.n_ensinaveis(),
            ..Default::default()
        };
        let lr_escala = self.afeto.lr_escala();
        if vai && mem.len() > 0 && !base.is_empty() {
            // Frustracao REDUZ a taxa: quando nada esta funcionando, mexer mais nos
            // pesos costuma piorar. Curiosidade a aumenta. E o "parar e respirar" da
            // origem, e o unico lugar do laco onde o afeto toca em peso.
            let padrao = CfgConsolidacao::default();
            let r = consolidar(
                ag,
                ops,
                patcher,
                mem,
                base,
                validacao,
                &CfgConsolidacao {
                    lr: padrao.lr * lr_escala,
                    ..padrao
                },
            );
            rel.consolidou = r.passos > 0;
            rel.passos = r.passos;
            rel.antes = r.antes.acuracia_intencao();
            rel.depois = r.depois.acuracia_intencao();
        }
        // SENTIR, por ultimo — a ordem da nila_mind. O afeto deste tick so vale a
        // partir do proximo, o que evita um laco em que sentir muda o que se sente.
        let surpresa = surpresa_bits(&*ag, ops, patcher, mem);
        // Recompensa = quanto a consolidacao de fato ganhou. Sem consolidar, zero:
        // "nada aconteceu" nao e progresso nem fracasso.
        let recompensa = if rel.consolidou {
            ((rel.depois - rel.antes) / GANHO_CHEIO).clamp(-1.0, 1.0) as f32
        } else {
            0.0
        };
        self.afeto.atualizar(recompensa, surpresa);

        rel.surpresa = surpresa;
        rel.afeto = self.afeto.resumo();
        rel.temperatura = temperatura;
        rel.lr_escala = lr_escala;
        rel.segundos = t0.elapsed().as_secs_f64();
        rel
    }
}

/// Escreve o estado do tick, para dar para olhar de fora enquanto ela roda.
pub fn escrever_pulso(caminho: &std::path::Path, r: &Relatorio) -> std::io::Result<()> {
    let escapar = |s: &str| {
        s.chars()
            .flat_map(|c| match c {
                '"' => vec!['\\', '"'],
                '\\' => vec!['\\', '\\'],
                '\n' | '\r' | '\t' => vec![' '],
                c if (c as u32) < 0x20 => vec![' '],
                c => vec![c],
            })
            .collect::<String>()
    };
    let mut f = std::fs::File::create(caminho)?;
    write!(
        f,
        "{{\n  \"tick\": {},\n  \"episodios_novos\": {},\n  \"consolidou\": {},\n  \
         \"passos\": {},\n  \"intencao_antes\": {:.4},\n  \"intencao_depois\": {:.4},\n  \
         \"esquecendo\": {},\n  \"tentou\": {},\n  \"ensinaveis\": {},\n  \"surpresa_bits\": {:.3},\n  \"temperatura\": {:.2},\n  \"lr_escala\": {:.2},\n  \"afeto\": \"{}\",\n  \"segundos\": {:.1},\n  \"pensamento\": \"{}\"\n}}\n",
        r.tick,
        r.episodios_novos,
        r.consolidou,
        r.passos,
        r.antes,
        r.depois,
        r.esquecendo,
        r.tentou,
        r.ensinaveis,
        r.surpresa,
        r.temperatura,
        r.lr_escala,
        escapar(&r.afeto),
        r.segundos,
        escapar(&r.pensamento)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Scalar;
    use crate::model::hierarchy::Config;
    use crate::model::patcher::PorPalavra;
    use crate::tools::Registro;

    fn agente() -> Agente<f32> {
        let mut r = Rng::new(1);
        Agente::<f32>::novo(Config::minusculo(), Registro::padrao(), &mut r)
    }

    #[test]
    fn pensar_devolve_o_tanto_pedido_e_nao_estoura() {
        let ag = agente();
        let mut rng = Rng::new(2);
        let t = pensar(&ag, &Scalar, &PorPalavra::default(), "le o notas", 24, 0.9, &mut rng);
        // Bytes crus podem formar UTF-8 invalido, que vira U+FFFD; o que se cobra e
        // que saiu algo e que nao explodiu.
        assert!(!t.is_empty(), "devaneio vazio");
        assert!(t.len() <= 24 * 4, "devaneio maior que o pedido: {}", t.len());
    }

    #[test]
    fn devaneio_zero_nao_pensa() {
        let ag = agente();
        let mut rng = Rng::new(3);
        assert_eq!(
            pensar(&ag, &Scalar, &PorPalavra::default(), "oi", 0, 0.9, &mut rng),
            ""
        );
    }

    #[test]
    fn a_temperatura_controla_o_acaso() {
        // Testa `amostrar` ISOLADA, com logits conhecidos.
        //
        // Duas versoes anteriores testavam isto atraves de `pensar`, num modelo sem
        // treino, e as duas falharam por medir o modelo em vez do amostrador: os
        // logits de pesos aleatorios saem quase iguais entre si, entao a distribuicao
        // fica achatada mesmo dividida por 0,05 e a semente continua mandando.
        //
        // Com logits de verdade a propriedade e nitida: temperatura baixa segue o
        // maximo, temperatura alta sorteia.
        let mut logits = [0.0f32; 256];
        logits[42] = 10.0;

        let mut rng = Rng::new(1);
        for _ in 0..64 {
            assert_eq!(
                amostrar(&logits, 0.05, &mut rng),
                42,
                "temperatura baixa nao seguiu o maximo"
            );
        }

        let mut rng = Rng::new(1);
        let quente: std::collections::HashSet<u8> =
            (0..256).map(|_| amostrar(&logits, 8.0, &mut rng)).collect();
        assert!(
            quente.len() > 32,
            "temperatura alta quase nao sorteia: so {} valores distintos",
            quente.len()
        );
    }

    #[test]
    fn amostrar_respeita_a_proporcao() {
        // Dois bytes com o mesmo logit tem de sair com frequencia parecida. Um erro
        // de sinal ou um `<=` trocado no laco cumulativo passaria pelo teste acima
        // (o maximo continuaria ganhando) e apareceria aqui.
        let mut logits = [f32::NEG_INFINITY; 256];
        logits[10] = 0.0;
        logits[200] = 0.0;
        let mut rng = Rng::new(5);
        let mut c10 = 0;
        let n = 2000;
        for _ in 0..n {
            match amostrar(&logits, 1.0, &mut rng) {
                10 => c10 += 1,
                200 => {}
                outro => panic!("saiu {outro}, que tinha probabilidade zero"),
            }
        }
        let f = c10 as f64 / n as f64;
        assert!((0.42..0.58).contains(&f), "proporcao torta: {f:.3}");
    }

    #[test]
    fn o_tick_sem_memoria_nao_consolida() {
        // O laco girando sobre nada tem de ser inofensivo: pensa, reporta, e nao
        // toca em peso nenhum.
        let mut ag = agente();
        let antes: Vec<f32> = ag.params_mut().iter().flat_map(|p| p.to_vec()).collect();

        let mut p = Pulso::novo(7);
        let mem = MemoriaEpisodica::nova();
        let sem = MemoriaSemantica::nova(0);
        let cfg = CfgPulso {
            bytes_pensamento: 8,
            ..Default::default()
        };
        let r = p.tick(
            &mut ag,
            &Scalar,
            &PorPalavra::default(),
            &mem,
            &sem,
            0.0,
            &[],
            &[],
            &cfg,
        );
        assert_eq!(r.tick, 1);
        assert!(!r.consolidou, "consolidou sem memoria");

        let depois: Vec<f32> = ag.params_mut().iter().flat_map(|p| p.to_vec()).collect();
        assert_eq!(antes, depois, "o tick mexeu nos pesos sem consolidar");
    }

    #[test]
    fn pensar_nunca_muda_peso() {
        // A regra da nila_mind, cobrada. O compilador ja impede (pensar recebe
        // `&Agente`), mas se alguem trocar a assinatura um dia, isto cai junto.
        let mut ag = agente();
        let antes: Vec<f32> = ag.params_mut().iter().flat_map(|p| p.to_vec()).collect();
        let mut rng = Rng::new(9);
        for _ in 0..5 {
            let _ = pensar(&ag, &Scalar, &PorPalavra::default(), "teka", 16, 1.0, &mut rng);
        }
        let depois: Vec<f32> = ag.params_mut().iter().flat_map(|p| p.to_vec()).collect();
        assert_eq!(antes, depois);
    }

    /// O afeto tem de MUDAR o tick, e nao so ser reportado.
    ///
    /// Com memoria vazia a surpresa e 0, o tedio sobe, e tedio soma na temperatura
    /// (`+0,20`). Se alguem desligar a ligacao um dia — voltando `pensar` a usar
    /// `cfg.temperatura` — a temperatura fica parada e isto cai.
    #[test]
    fn o_afeto_move_a_temperatura_ao_longo_dos_ticks() {
        let mut ag = agente();
        let mut p = Pulso::novo(7);
        let mem = MemoriaEpisodica::nova();
        let sem = MemoriaSemantica::nova(0);
        let cfg = CfgPulso { bytes_pensamento: 4, ..Default::default() };

        let mut temps = Vec::new();
        let mut tedios = Vec::new();
        for _ in 0..40 {
            let r = p.tick(&mut ag, &Scalar, &PorPalavra::default(), &mem, &sem, 0.0, &[], &[], &cfg);
            temps.push(r.temperatura);
            tedios.push(p.afeto.tedio);
        }
        assert!(
            tedios.last().unwrap() > &tedios[0],
            "o tedio nao acumulou num mundo sem nada: {:.3} -> {:.3}",
            tedios[0], tedios.last().unwrap()
        );
        assert!(
            (temps.last().unwrap() - temps[0]).abs() > 1e-6,
            "a temperatura nao se moveu: o afeto nao esta ligado ({:.3} -> {:.3})",
            temps[0], temps.last().unwrap()
        );
    }

    /// A surpresa sai em BITS por byte, dentro de 0..8.
    #[test]
    fn a_surpresa_sai_em_bits_e_na_faixa() {
        let ag = agente();
        let mut mem = MemoriaEpisodica::nova();
        for t in ["le o notas.md", "quanto de ram", "que horas sao"] {
            mem.gravar(t, 0, vec![], crate::memory::Resultado::Executou,
                       crate::memory::Feedback::Nenhum, vec![]);
        }
        let s = surpresa_bits(&ag, &Scalar, &PorPalavra::default(), &mem);
        assert!((0.0..=8.0).contains(&s), "surpresa fora de 0..8 bits: {s}");
        // Um modelo sem treino nao tem como prever byte nenhum: tem de estar perto
        // do teto. Se sair perto de 0, a perda voltou em nats ou o alvo esta errado.
        assert!(s > 4.0, "modelo aleatorio com surpresa baixa demais: {s} bits");

        // Memoria vazia: nada aconteceu, surpresa zero, tedio come.
        let vazia = MemoriaEpisodica::nova();
        assert_eq!(surpresa_bits(&ag, &Scalar, &PorPalavra::default(), &vazia), 0.0);
    }
    #[test]
    fn o_fio_reancora_quando_envelhece() {
        let mut p = Pulso::novo(11);
        let mut mem = MemoriaEpisodica::nova();
        mem.gravar(
            "le o notas.md",
            0,
            vec![],
            crate::memory::Resultado::Executou,
            crate::memory::Feedback::Nenhum,
            vec![],
        );
        p.idade_fio = IDADE_MAXIMA_FIO;
        p.fio = "deriva total sem relacao nenhuma".into();
        p.reancorar(&mem);
        assert!(p.fio.contains("notas.md"), "nao reancorou na memoria: {:?}", p.fio);
        assert_eq!(p.idade_fio, 0);
    }

    /// Sem regra ligada, o vigia sai barato: nem tenta falar com a ponte.
    ///
    /// Importa porque o tick roda o tempo todo. Se cada tick abrisse socket e
    /// esperasse resposta, o pulso viraria uma metralhadora de conexoes contra um
    /// Spotify que talvez nem esteja aberto.
    #[test]
    fn sem_regra_ligada_o_vigia_nao_faz_nada() {
        let ligadas = crate::tools::regras::tabela().iter().filter(|r| r.ligada).count();
        assert_eq!(ligadas, 0, "o padrao do repositorio tem de vir DESLIGADO");

        let mut p = Pulso::novo(7);
        let t0 = std::time::Instant::now();
        assert_eq!(p.vigiar(), None);
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(200),
            "demorou {:?} — sinal de que foi falar com a ponte a toa",
            t0.elapsed()
        );
    }

    #[test]
    fn o_pulso_escrito_e_json_valido_mesmo_com_byte_cru() {
        // O devaneio sai do modelo em bytes: aspas, barra e controle aparecem. Sem
        // escapar, o arquivo de pulso quebra o parser de quem for ler.
        let r = Relatorio {
            tick: 3,
            pensamento: "aspas \" barra \\ \n tab \t fim".into(),
            ..Default::default()
        };
        let p = std::env::temp_dir().join("teka_pulso_teste.json");
        escrever_pulso(&p, &r).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains(r#"\""#) && t.contains(r#"\\"#));
        // (Contar linhas do arquivo era o que havia aqui, e quebrou assim que o
        // relatorio ganhou dois campos. O que o teste quer saber e se o VALOR vazou
        // de linha — e isso a checagem abaixo cobre sozinha.)
        // Nenhuma quebra dentro do valor: o texto do pensamento vira uma linha so.
        let linha = t.lines().find(|l| l.contains("pensamento")).unwrap();
        assert!(linha.ends_with('"'), "pensamento vazou de linha: {linha:?}");
        std::fs::remove_file(&p).ok();
    }
}
