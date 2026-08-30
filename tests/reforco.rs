//! Fase 4 — aprender do desfecho, sem ninguém dizer a resposta.
//!
//! A fase 3 aprende de `/certo` e `/errado`. Na prática você não vai responder a
//! maioria das vezes. O que este arquivo testa é a única coisa que a fase 4
//! acrescenta de verdade: **episódios sem feedback nenhum ainda ensinam**, desde
//! que o mundo tenha dado um sinal.

use teka::backend::Paralelo;
use teka::learn::dados::{dividir_por_frase, gerar, Exemplo};
use teka::learn::reforco::{treinar_por_reforco, CfgReforco, Recozimento};
use teka::learn::supervisionado::{treinar_agente, CfgSup};
use teka::memory::{Feedback, MemoriaEpisodica, Resultado};
use teka::model::agente::{Agente, AgenteCache};
use teka::model::hierarchy::Config;
use teka::model::patcher::PorPalavra;
use teka::rng::Rng;
use teka::tools::Registro;

struct Cenario {
    ag: Agente<f32>,
    base: Vec<Exemplo>,
    validacao: Vec<Exemplo>,
    patcher: PorPalavra,
}

fn preparar(semente: u64, epocas: usize) -> Cenario {
    let patcher = PorPalavra::default();
    let mut rng = Rng::new(semente);
    let mut ag = Agente::<f32>::novo(Config::pequeno(), Registro::padrao(), &mut rng);
    let ops = Paralelo::auto();

    let mut r = Rng::new(semente + 1);
    let exs = gerar(&ag.registro, &patcher, 5000, &mut r);
    let (base, validacao) = dividir_por_frase(exs, 4);
    treinar_agente(
        &mut ag,
        &ops,
        &patcher,
        &base,
        &validacao,
        &CfgSup {
            epocas,
            log_cada: epocas,
            lr: 1.5e-3,
            ..Default::default()
        },
    );
    Cenario {
        ag,
        base,
        validacao,
        patcher,
    }
}

/// Frases do proprio corpus — a supervisao TEM o que dizer sobre elas.
const PEDIDOS_CONHECIDOS: &[&str] = &[
    "lista os arquivos de src",
    "abre o arquivo notas.md",
    "quanto e 45+55",
    "procura um arquivo chamado backup",
    "quanto de espaco tem no disco",
    "quanta memoria esta em uso",
];

/// Roda os pedidos e devolve a ferramenta escolhida em cada um.
fn decisoes(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &PorPalavra,
    pedidos: &[&str],
    cache: &mut AgenteCache<f32>,
) -> Vec<usize> {
    pedidos
        .iter()
        .map(|p| {
            ag.responder(ops, patcher, p, cache)
                .map(|c| c.ferramenta)
                .unwrap_or(usize::MAX)
        })
        .collect()
}

/// Grava os pedidos como tendo FALHADO, sem feedback nenhum.
fn memoria_de_falhas(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &PorPalavra,
    pedidos: &[&str],
    repeticoes: usize,
    cache: &mut AgenteCache<f32>,
) -> MemoriaEpisodica {
    let mut mem = MemoriaEpisodica::nova();
    for _ in 0..repeticoes {
        for pedido in pedidos {
            let escolhida = ag
                .responder(ops, patcher, pedido, cache)
                .map(|c| c.ferramenta)
                .unwrap_or(0);
            let assinatura = ag.assinatura(cache);
            mem.gravar(
                pedido,
                escolhida,
                vec![],
                Resultado::Falhou,
                Feedback::Nenhum,
                assinatura,
            );
        }
    }
    mem
}

fn nome(ag: &Agente<f32>, f: usize) -> &str {
    ag.registro
        .ferramentas
        .get(f)
        .map(|x| x.nome.as_str())
        .unwrap_or("?")
}

/// O ambiente do teste: pedido → ferramenta que de fato funciona.
///
/// Nenhuma destas frases existe no gerador, então a supervisão não tem o que dizer
/// sobre elas. O único sinal disponível é o desfecho.
const AMBIENTE: &[(&str, &str)] = &[
    ("modo turbo agora", "memoria"),
    ("status vital da maquina", "memoria"),
    ("qual o folego que sobrou", "disco"),
    ("respiracao do armazenamento", "disco"),
    ("escaneia o cofre inteiro", "procurar_arquivo"),
    ("marca no diario que terminei", "escrever_arquivo"),
];

fn quantos_acerta(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &PorPalavra,
    ambiente: &[(String, usize)],
    cache: &mut AgenteCache<f32>,
) -> usize {
    ambiente
        .iter()
        .filter(|(pedido, correto)| {
            ag.responder(ops, patcher, pedido, cache)
                .map(|c| c.ferramenta)
                == Ok(*correto)
        })
        .count()
}

/// **O laço fechado.** Ela age, o mundo responde, ela ajusta — sem ninguém dizer
/// qual era a resposta certa em momento nenhum.
///
/// ## O que mudou quando o crítico entrou
///
/// Antes do crítico este teste não convergia: 4/6 oscilando, terminando em 4, com a
/// validação caindo 12 pontos no meio. A causa era o baseline por pedido — ele só
/// existe para pedidos **repetidos**, e num pedido visto uma vez a vantagem saía
/// zero. Medido: 20 de 30 transições úteis.
///
/// Com `V(s)` aprendido: **30 de 30** transições úteis, e a curva vira:
///
/// ```text
/// antes:    4/6 | intencao(val) 77,3%
/// rodada 1: 5/6 | erro critico 0,033 | 86,4%
/// rodada 3: 5/6 | erro critico 0,032 | 83,0%
/// rodada 5: 5/6 | erro critico 0,023 | 84,1%
/// ```
///
/// ## O que a instabilidade ensinou
///
/// Com `lr = 5e-4` (5x o padrão) o crítico DIVERGE — erro saltando para 171 — e
/// arrasta a política junto, mesmo estando destacado do tronco: a vantagem é
/// `r − V`, então um `V` disparado vira empurrão constante no teto do recorte.
///
/// A lição não é "abaixar a taxa até parar de quebrar": é que **um baseline ruim é
/// pior que baseline nenhum**, porque ele injeta sinal errado em vez de apenas
/// deixar de reduzir variância. O erro do crítico é monitorado no relatório por
/// isso.
#[test]
fn o_laco_fechado_aprende_do_desfecho() {
    let cen = preparar(2026, 6);
    let ops = Paralelo::auto();
    let mut cache = AgenteCache::new();
    let mut rng = Rng::new(1234);

    let ambiente: Vec<(String, usize)> = AMBIENTE
        .iter()
        .map(|(p, f)| {
            (
                p.to_string(),
                cen.ag.registro.indice(f).expect("ferramenta do ambiente"),
            )
        })
        .collect();
    // Validacao curta: este teste roda o laco varias vezes, e a avaliacao completa
    // dominaria o tempo sem mudar a conclusao.
    let val: Vec<Exemplo> = cen.validacao.iter().take(320).cloned().collect();

    let mut ag = cen.ag.clone();
    let inicial = quantos_acerta(&ag, &ops, &cen.patcher, &ambiente, &mut cache);
    let intencao_inicial;
    {
        let vazia = teka::memory::MemoriaEpisodica::nova();
        let r = treinar_por_reforco(
            &mut ag.clone(),
            &ops,
            &cen.patcher,
            &vazia,
            &cen.base,
            &val,
            &CfgReforco::default(),
        );
        intencao_inicial = r.antes.acuracia_intencao();
    }
    println!(
        "\n  antes do laco: acerta {inicial}/{} do ambiente | intencao(val) {:.1}%",
        ambiente.len(),
        intencao_inicial * 100.0
    );

    let recozimento = Recozimento::default();
    let mut mem = MemoriaEpisodica::nova();
    let mut ultimo = inicial;
    let mut ultima_intencao = intencao_inicial;
    let mut erro_final = f64::NAN;

    for rodada in 1..=5 {
        // --- age explorando: varias tentativas por pedido, pra o baseline por
        //     pedido ter alternativas com que comparar ---
        for _ in 0..5 {
            for (pedido, correto) in &ambiente {
                // A temperatura cai com o numero de tentativas DAQUELE pedido:
                // pedido novo se experimenta, pedido conhecido se explota.
                let t = recozimento.temperatura(mem.tentativas(pedido));
                let escolhida = ag
                    .responder_explorando(&ops, &cen.patcher, pedido, t, &mut rng, &mut cache)
                    .map(|c| c.ferramenta)
                    .unwrap_or(usize::MAX);
                let assinatura = ag.assinatura(&cache);
                let resultado = if escolhida == *correto {
                    Resultado::Executou
                } else {
                    Resultado::Falhou
                };
                mem.gravar(
                    pedido,
                    escolhida.min(cen.ag.registro.n() - 1),
                    vec![],
                    resultado,
                    Feedback::Nenhum, // ninguem falou nada
                    assinatura,
                );
            }
        }

        // --- o mundo ja respondeu; agora ajusta ---
        let rel = treinar_por_reforco(
            &mut ag,
            &ops,
            &cen.patcher,
            &mem,
            &cen.base,
            &val,
            &CfgReforco {
                passos: 40,
                epocas: 3,
                lr: 1e-4,
                fracao_reforco: 0.4,
                ..Default::default()
            },
        );
        ultimo = quantos_acerta(&ag, &ops, &cen.patcher, &ambiente, &mut cache);
        ultima_intencao = rel.depois.acuracia_intencao();
        erro_final = rel.erro_critico;
        println!(
            "  rodada {rodada}: acerta {ultimo}/{} | T={:.2} | {} transicoes ({} uteis) | recompensa {:.3} | erro critico {:.3} | intencao(val) {:.1}%",
            ambiente.len(),
            recozimento.temperatura(mem.tentativas(&ambiente[0].0)),
            rel.transicoes,
            rel.transicoes_uteis,
            rel.recompensa_media,
            rel.erro_critico,
            ultima_intencao * 100.0
        );
    }

    // 1. Aprendeu do desfecho, sem rotulo nenhum.
    assert!(
        ultimo > inicial,
        "o laco fechado nao melhorou: {inicial} -> {ultimo}"
    );
    // 2. Sem custo no que ela ja sabia. (Na pratica ate sobe: o replay
    //    supervisionado continua treinando junto.)
    let queda = intencao_inicial - ultima_intencao;
    assert!(
        queda < 0.05,
        "aprender o ambiente custou {:.1} pontos do que ela ja sabia",
        queda * 100.0
    );
    // 3. O critico convergiu em vez de divergir. E o sinal que separa "reforco
    //    funcionando" de "reforco empurrando ruido" — ver a nota no topo.
    assert!(
        erro_final < 0.5,
        "o critico divergiu (erro {erro_final:.3}) — a vantagem vira ruido"
    );
}

#[test]
fn a_ancora_protege_contra_recompensa_mentirosa() {
    // Um sinal de recompensa pode simplesmente MENTIR: aqui, decisoes corretas sao
    // marcadas como falhas. O replay supervisionado tem que vencer.
    //
    // Este teste nasceu de um erro meu de desenho — a primeira versao do teste de
    // cima usava estas frases e falhava. A "falha" era o comportamento certo.
    // 12 epocas, e nao 6.
    //
    // Com 6 este teste caiu (1/6 mantidas, validacao 74,3% -> 47,1%) e parecia
    // regressao da ancora. Nao era: com 12 ele passa. O modelo-base e que nao tinha
    // convergido, e ancora nenhuma defende decisao que ainda esta errada — metade
    // das decisoes "corretas" de partida ja eram erradas ("abre o arquivo notas.md"
    // -> escrever_arquivo).
    //
    // Quarto teste do projeto a cair pelo mesmo motivo depois que a variacao de
    // superficie, a familia "sem objeto" e o pool de pastas entraram: dados mais
    // variados, convergencia mais lenta, e limiar calibrado no regime antigo
    // passando a medir outra coisa. Achado 23.
    let cen = preparar(2026, 12);
    let ops = Paralelo::auto();
    let mut cache = AgenteCache::new();

    let antes = decisoes(&cen.ag, &ops, &cen.patcher, PEDIDOS_CONHECIDOS, &mut cache);
    let mem = memoria_de_falhas(&cen.ag, &ops, &cen.patcher, PEDIDOS_CONHECIDOS, 8, &mut cache);

    let mut depois_ag = cen.ag.clone();
    let rel = treinar_por_reforco(
        &mut depois_ag,
        &ops,
        &cen.patcher,
        &mem,
        &cen.base,
        &cen.validacao,
        &CfgReforco {
            passos: 100,
            epocas: 3,
            lr: 5e-4,
            fracao_reforco: 0.4,
            ..Default::default()
        },
    );
    let depois = decisoes(&depois_ag, &ops, &cen.patcher, PEDIDOS_CONHECIDOS, &mut cache);

    let mantidas = antes.iter().zip(&depois).filter(|(a, d)| a == d).count();
    println!(
        "\n  recompensa mentirosa em {} pedidos conhecidos: {mantidas}/{} decisoes mantidas",
        PEDIDOS_CONHECIDOS.len(),
        PEDIDOS_CONHECIDOS.len()
    );
    for (i, pedido) in PEDIDOS_CONHECIDOS.iter().enumerate() {
        println!(
            "    {} {:<34} {} -> {}",
            if antes[i] == depois[i] { "segurou" } else { "CEDEU  " },
            pedido,
            nome(&cen.ag, antes[i]),
            nome(&cen.ag, depois[i])
        );
    }
    println!(
        "  validacao: intencao {:.1}% -> {:.1}%",
        rel.antes.acuracia_intencao() * 100.0,
        rel.depois.acuracia_intencao() * 100.0
    );
    assert!(
        mantidas * 2 >= PEDIDOS_CONHECIDOS.len(),
        "a maioria das decisoes corretas cedeu a um sinal mentiroso: {mantidas}/{}",
        PEDIDOS_CONHECIDOS.len()
    );
    let queda = rel.antes.acuracia_intencao() - rel.depois.acuracia_intencao();
    assert!(queda < 0.15, "a validacao caiu {:.1} pontos", queda * 100.0);
}

#[test]
fn sucesso_silencioso_nao_move_a_politica() {
    // O outro lado da assimetria: se ela acerta e ninguem fala nada, nao ha o que
    // aprender. Se este teste falhar, `hora` vira a resposta pra tudo.
    let cen = preparar(77, 4);
    let ops = Paralelo::auto();
    let mut cache = AgenteCache::new();

    let mut mem = MemoriaEpisodica::nova();
    for _ in 0..8 {
        for pedido in PEDIDOS_CONHECIDOS {
            let escolhida = cen
                .ag
                .responder(&ops, &cen.patcher, pedido, &mut cache)
                .map(|c| c.ferramenta)
                .unwrap_or(0);
            let assinatura = cen.ag.assinatura(&cache);
            mem.gravar(
                pedido,
                escolhida,
                vec![],
                Resultado::Executou, // deu certo
                Feedback::Nenhum,    // e ninguem falou nada
                assinatura,
            );
        }
    }

    let antes = decisoes(&cen.ag, &ops, &cen.patcher, PEDIDOS_CONHECIDOS, &mut cache);

    let mut depois_ag = cen.ag.clone();
    let rel = treinar_por_reforco(
        &mut depois_ag,
        &ops,
        &cen.patcher,
        &mem,
        &cen.base,
        &cen.validacao,
        &CfgReforco::default(),
    );

    println!(
        "\n  48 sucessos silenciosos: {} transicoes, {} com vantagem nao-nula",
        rel.transicoes, rel.transicoes_uteis
    );
    // A propriedade continua, mas o mecanismo mudou com o baseline por pedido.
    // Antes: sucesso nao virava transicao. Agora vira, valendo ZERO — e como todas
    // as transicoes de um mesmo pedido valem zero, a media daquele pedido tambem e
    // zero, e a vantagem de todas fica exatamente zero. Nada se move.
    //
    // Esta e a formulacao mais forte: o sucesso ENTRA na comparacao (sem ele a
    // falha nao teria com o que ser comparada) e ainda assim nao gera pressao.
    assert!(rel.transicoes > 0, "sucesso deveria entrar como ponto de comparacao");
    assert_eq!(
        rel.transicoes_uteis, 0,
        "sucesso sem feedback gerou vantagem — isso e reward hacking"
    );

    // O replay supervisionado continua rodando: `fracao_reforco` diz que so PARTE
    // do lote vem do reforco, e o resto e replay. Entao os pesos se movem um pouco
    // mesmo com vantagem zero — isso e o replay fazendo o trabalho dele, e nao
    // pressao de recompensa.
    //
    // A versao anterior exigia `antes == depois`, o que passava por sorte: bastou o
    // pool de pastas mudar (14 -> 28 entradas) para o replay virar UMA das seis
    // decisoes e o teste cair, com `transicoes_uteis == 0` intacto. O proxy quebrou;
    // a propriedade nao.
    //
    // O medo declarado no topo e concreto — "`hora` viraria a resposta pra tudo" —
    // e e isso que se cobra: sem sinal, a politica nao COLAPSA.
    let depois = decisoes(&depois_ag, &ops, &cen.patcher, PEDIDOS_CONHECIDOS, &mut cache);
    let mantidas = antes.iter().zip(&depois).filter(|(a, d)| a == d).count();
    assert!(
        mantidas * 3 >= PEDIDOS_CONHECIDOS.len() * 2,
        "sem sinal nenhum, {mantidas}/{} decisoes ficaram de pe — o replay nao deveria          mexer nisso tudo",
        PEDIDOS_CONHECIDOS.len()
    );
    let distintas: std::collections::HashSet<_> = depois.iter().collect();
    assert!(
        distintas.len() >= 3,
        "a politica colapsou em {} ferramenta(s): {:?}",
        distintas.len(),
        depois
    );
}
