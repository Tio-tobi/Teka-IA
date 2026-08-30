//! Fase 3 — aprender por correção sem esquecer.
//!
//! O teste central deste arquivo é um experimento com controle. Não basta mostrar
//! que a Teka aprende o que foi corrigido: qualquer treino faz isso. O que precisa
//! ser demonstrado é que o **mecanismo anti-esquecimento carrega peso** — e a única
//! forma honesta é desligá-lo e ver o estrago.

use teka::backend::Paralelo;
use teka::learn::consolidacao::{consolidar, CfgConsolidacao};
use teka::learn::dados::{dividir_por_frase, gerar, Exemplo};
use teka::learn::supervisionado::{treinar_agente, CfgSup};
use teka::memory::{Feedback, MemoriaEpisodica, Resultado};
use teka::model::agente::{Agente, AgenteCache};
use teka::model::hierarchy::Config;
use teka::model::patcher::PorPalavra;
use teka::rng::Rng;
use teka::tools::Registro;

/// Frases que o gerador nunca produz, mapeadas a ferramentas que a Teka não teria
/// como adivinhar. É o "vocabulário particular" que só o uso real ensina.
const CORRECOES: &[(&str, &str, Option<&str>)] = &[
    ("modo turbo", "memoria", None),
    ("status vital da maquina", "memoria", None),
    ("pulso do sistema", "memoria", None),
    ("qual o folego que sobrou", "disco", None),
    ("respiracao do armazenamento", "disco", None),
    ("escaneia o cofre", "procurar_arquivo", Some("cofre")),
];

fn acertos_nas_correcoes<O: teka::backend::Ops<f32>>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &PorPalavra,
    cache: &mut AgenteCache<f32>,
) -> usize {
    CORRECOES
        .iter()
        .filter(|(pedido, esperado, _)| {
            match ag.responder(ops, patcher, pedido, cache) {
                Ok(ch) => ag.registro.ferramentas[ch.ferramenta].nome == *esperado,
                Err(_) => false,
            }
        })
        .count()
}

/// Constrói a memória como se você tivesse corrigido a Teka algumas vezes.
fn memoria_de_correcoes<O: teka::backend::Ops<f32>>(
    ag: &Agente<f32>,
    ops: &O,
    patcher: &PorPalavra,
    repeticoes: usize,
) -> MemoriaEpisodica {
    let mut mem = MemoriaEpisodica::nova();
    let mut cache = AgenteCache::new();
    for _ in 0..repeticoes {
        for (pedido, esperado, arg) in CORRECOES {
            let alvo = ag.registro.indice(esperado).expect("ferramenta do teste");
            // O que ela decidiu (errado, presumivelmente) e como ela leu o pedido.
            let escolhida = ag
                .responder(ops, patcher, pedido, &mut cache)
                .map(|c| c.ferramenta)
                .unwrap_or(0);
            let assinatura = ag.assinatura(&cache);

            let args = match arg {
                Some(a) => {
                    let ini = pedido.find(a).expect("argumento tem que estar no pedido");
                    vec![(0usize, (ini, ini + a.len()))]
                }
                None => vec![],
            };
            mem.gravar(
                pedido,
                escolhida,
                vec![],
                Resultado::Falhou,
                Feedback::Corrigido {
                    ferramenta: alvo,
                    args,
                },
                assinatura,
            );
        }
    }
    mem
}

struct Cenario {
    ag: Agente<f32>,
    base: Vec<Exemplo>,
    validacao: Vec<Exemplo>,
    patcher: PorPalavra,
}

/// Treina um agente pequeno o suficiente para o teste rodar em ~1 minuto.
fn preparar() -> Cenario {
    let patcher = PorPalavra::default();
    let mut rng = Rng::new(2026);
    let mut ag = Agente::<f32>::novo(Config::pequeno(), Registro::padrao(), &mut rng);
    let ops = Paralelo::auto();

    let mut r = Rng::new(31);
    let exs = gerar(&ag.registro, &patcher, 5000, &mut r);
    let (base, validacao) = dividir_por_frase(exs, 4);

    let cfg = CfgSup {
        seq: 64,
        batch: 16,
        lr: 1.5e-3,
        epocas: 5,
        log_cada: 5,
        ..Default::default()
    };
    treinar_agente(&mut ag, &ops, &patcher, &base, &validacao, &cfg);
    Cenario {
        ag,
        base,
        validacao,
        patcher,
    }
}

#[test]
fn aprende_por_correcao_e_o_replay_e_o_que_impede_o_esquecimento() {
    let cen = preparar();
    let ops = Paralelo::auto();
    let mut cache = AgenteCache::new();

    let antes_correcoes = acertos_nas_correcoes(&cen.ag, &ops, &cen.patcher, &mut cache);
    println!(
        "\n  antes de qualquer correcao: {antes_correcoes}/{} das frases novas",
        CORRECOES.len()
    );

    let mem = memoria_de_correcoes(&cen.ag, &ops, &cen.patcher, 6);
    println!("  memoria: {} episodios, {} ensinaveis", mem.len(), mem.n_ensinaveis());
    assert_eq!(mem.n_ensinaveis(), CORRECOES.len() * 6);

    // ---------------- A: consolidacao COM replay do corpus base ----------------
    let mut com_replay = cen.ag.clone();
    let cfg_a = CfgConsolidacao {
        passos: 200,
        fracao_episodios: 0.35,
        ..Default::default()
    };
    let rel_a = consolidar(
        &mut com_replay,
        &ops,
        &cen.patcher,
        &mem,
        &cen.base,
        &cen.validacao,
        &cfg_a,
    );
    let acertos_a = acertos_nas_correcoes(&com_replay, &ops, &cen.patcher, &mut cache);

    // ---------------- B: consolidacao SEM replay (so as correcoes) -------------
    // O controle. Se o replay nao estivesse fazendo nada, os dois lados dariam a
    // mesma coisa.
    let mut sem_replay = cen.ag.clone();
    let cfg_b = CfgConsolidacao {
        passos: 200,
        fracao_episodios: 1.0,
        ..Default::default()
    };
    let rel_b = consolidar(
        &mut sem_replay,
        &ops,
        &cen.patcher,
        &mem,
        &cen.base,
        &cen.validacao,
        &cfg_b,
    );
    let acertos_b = acertos_nas_correcoes(&sem_replay, &ops, &cen.patcher, &mut cache);

    println!("\n                          correcoes   intencao(val)   ponta-a-ponta(val)");
    println!(
        "  antes                  {antes_correcoes}/{}         {:>6.1}%          {:>6.1}%",
        CORRECOES.len(),
        rel_a.antes.acuracia_intencao() * 100.0,
        rel_a.antes.acuracia_total() * 100.0
    );
    println!(
        "  consolidou COM replay  {acertos_a}/{}         {:>6.1}%          {:>6.1}%",
        CORRECOES.len(),
        rel_a.depois.acuracia_intencao() * 100.0,
        rel_a.depois.acuracia_total() * 100.0
    );
    println!(
        "  consolidou SEM replay  {acertos_b}/{}         {:>6.1}%          {:>6.1}%",
        CORRECOES.len(),
        rel_b.depois.acuracia_intencao() * 100.0,
        rel_b.depois.acuracia_total() * 100.0
    );

    // 1. Ela aprendeu o que foi corrigido.
    assert!(
        acertos_a > antes_correcoes,
        "consolidar nao ensinou nada: {antes_correcoes} -> {acertos_a}"
    );
    assert!(
        acertos_a * 2 >= CORRECOES.len(),
        "aprendeu menos da metade das correcoes: {acertos_a}/{}",
        CORRECOES.len()
    );

    // 2. E nao esqueceu o que ja sabia.
    let queda_a = rel_a.antes.acuracia_intencao() - rel_a.depois.acuracia_intencao();
    assert!(
        queda_a < 0.10,
        "com replay a intencao caiu {:.1} pontos — o mecanismo nao segurou",
        queda_a * 100.0
    );

    // 3. O controle: sem replay, o estrago tem que aparecer. Se este assert falhar,
    //    o replay nao esta fazendo o trabalho que se atribui a ele — e a conclusao
    //    seria que o teste 2 passou por outro motivo qualquer.
    let queda_b = rel_b.antes.acuracia_intencao() - rel_b.depois.acuracia_intencao();
    println!(
        "\n  queda de intencao: com replay {:.1} pontos, sem replay {:.1} pontos",
        queda_a * 100.0,
        queda_b * 100.0
    );
    assert!(
        queda_b > queda_a,
        "sem replay deveria esquecer MAIS que com replay ({:.1} vs {:.1})",
        queda_b * 100.0,
        queda_a * 100.0
    );
}

#[test]
fn a_memoria_recupera_o_episodio_certo_pela_leitura_do_pedido() {
    // A assinatura e o estado do backbone: dois pedidos que ela LE parecido tem que
    // ficar proximos, mesmo escritos diferente.
    let patcher = PorPalavra::default();
    let mut rng = Rng::new(9);
    let mut ag = Agente::<f32>::novo(Config::pequeno(), Registro::padrao(), &mut rng);
    let ops = Paralelo::auto();

    let mut r = Rng::new(5);
    let exs = gerar(&ag.registro, &patcher, 4000, &mut r);
    let (base, val) = dividir_por_frase(exs, 4);
    treinar_agente(
        &mut ag,
        &ops,
        &patcher,
        &base,
        &val,
        &CfgSup {
            // 10, e nao 4.
            //
            // Com 4 epocas a assinatura de "como esta a ram do computador" caia em
            // "quanto e 2+2" (cos 0,320) — nao por a assinatura ter piorado, mas por
            // estar no meio da subida. Com 10 ela recupera o episodio certo.
            //
            // E o terceiro teste a cair pelo mesmo motivo depois que a variacao de
            // superficie e a familia "sem objeto" entraram: os dados ficaram mais
            // variados, o modelo demora mais para assentar, e limiar calibrado no
            // regime antigo passou a medir outra coisa. Achado 23 de novo.
            epocas: 10,
            log_cada: 10,
            ..Default::default()
        },
    );

    let mut cache = AgenteCache::new();
    let mut mem = MemoriaEpisodica::nova();
    for (pedido, ferr) in [
        ("quanto de memoria esta em uso", "memoria"),
        ("lista os arquivos de src", "listar_pasta"),
        ("quanto e 2+2", "calcular"),
        ("que horas sao", "hora"),
    ] {
        let alvo = ag.registro.indice(ferr).unwrap();
        let _ = ag.responder(&ops, &patcher, pedido, &mut cache);
        let assinatura = ag.assinatura(&cache);
        mem.gravar(
            pedido,
            alvo,
            vec![],
            Resultado::Executou,
            Feedback::Aprovado,
            assinatura,
        );
    }

    // Um pedido NOVO, semanticamente colado num dos guardados.
    let _ = ag.responder(&ops, &patcher, "como esta a ram do computador", &mut cache);
    let consulta = ag.assinatura(&cache);
    let top = mem.parecidos(&consulta, 1);
    println!(
        "\n  \"como esta a ram do computador\" recuperou: {:?} (cos {:.3})",
        top[0].0.pedido, top[0].1
    );
    assert_eq!(
        top[0].0.pedido, "quanto de memoria esta em uso",
        "a recuperacao por assinatura trouxe um episodio de outro assunto"
    );
}
