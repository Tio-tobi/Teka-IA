//! Teka — linha de comando.
//!
//! ```bash
//! cargo run --release                       # benchmark do backend
//! cargo run --release -- treinar --minutos 30
//! cargo run --release -- treinar --preset padrao --minutos 120 --saida cerebro.bin
//! cargo test  --release -- --nocapture      # corretude (gradientes, causalidade)
//! ```

use std::path::PathBuf;
use std::time::Instant;

use teka::backend::{Ops, Paralelo, Scalar};
use teka::learn::dados::{dividir_por_frase, gerar};
use teka::learn::supervisionado::{treinar_agente, CfgSup};
use teka::learn::train::{treinar, CfgTreino};
use teka::memory::contexto::Contexto;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::hierarchy::{Config, Teka, TekaCache};
use teka::model::patcher::{Fixo, Patcher, Plano, PorClasse, PorEntropia, PorPalavra};
use teka::rng::Rng;
use teka::ssm::{RgLru, RgLruCache};
use teka::learn::consolidacao::{consolidar, CfgConsolidacao};
use teka::learn::reforco::{treinar_por_reforco, CfgReforco, Recozimento};
use teka::memory::{Feedback, MemoriaEpisodica, Resultado};
use teka::gerador::{formatar, gerar_propostas, CfgGerador};
use teka::model::confianca::Separacao;
use teka::coletor::{self, fonte};
use teka::tools::execucao::Executor;
use teka::tools::{Politica, Registro};

fn cronometrar<F: FnMut()>(min_segundos: f64, mut f: F) -> (usize, f64) {
    f(); // aquece: a primeira passada paga fault de página e cache frio
    let t0 = Instant::now();
    let mut n = 0usize;
    loop {
        f();
        n += 1;
        let dt = t0.elapsed().as_secs_f64();
        if dt >= min_segundos {
            return (n, dt);
        }
    }
}

// ---------------------------------------------------------------------------
// benchmark
// ---------------------------------------------------------------------------

fn bench_gemm(threads: usize) {
    println!("\n─── GEMM (f32) ─────────────────────────────────────────────────");
    let mut rng = Rng::new(1);
    let (m, k, n) = (1024usize, 384usize, 384usize);
    let mut a = vec![0.0f32; m * k];
    let mut b = vec![0.0f32; k.max(n) * n.max(k)];
    rng.fill_normal(&mut a, 1.0);
    rng.fill_normal(&mut b, 1.0);
    let mut c = vec![0.0f32; m * n];
    let flop = 2.0 * m as f64 * k as f64 * n as f64;
    let par = Paralelo::new(threads);

    let linha = |nome: &str, f: &mut dyn FnMut()| {
        let (iters, dt) = cronometrar(0.4, || f());
        print!("  {:>9.2}", flop * iters as f64 / dt / 1e9);
        let _ = nome;
    };

    println!("             gemm_nn    gemm_tn    gemm_nt");
    print!("  escalar ");
    linha("nn", &mut || Scalar.gemm_nn(m, k, n, &a, &b, &mut c, false));
    linha("tn", &mut || Scalar.gemm_tn(m, k, n, &a, &b, &mut c, false));
    linha("nt", &mut || Scalar.gemm_nt(m, k, n, &a, &b, &mut c, false));
    println!("   GFLOP/s");
    print!("  x{threads:<7}");
    linha("nn", &mut || par.gemm_nn(m, k, n, &a, &b, &mut c, false));
    linha("tn", &mut || par.gemm_tn(m, k, n, &a, &b, &mut c, false));
    linha("nt", &mut || par.gemm_nt(m, k, n, &a, &b, &mut c, false));
    println!("   GFLOP/s");
    println!("  A `nt` fica pra tras: laco interno de REDUCAO, que o compilador nao");
    println!("  pode vetorizar (soma em ponto flutuante nao e associativa). Por isso");
    println!("  os pesos sao [n_in, n_out] -- ver src/nn/linear.rs.");
}

fn bench_rglru(threads: usize) {
    println!("\n─── RG-LRU, uma camada (f32) ───────────────────────────────────");
    let (seq, batch, h) = (256usize, 8usize, 384usize);
    let mut rng = Rng::new(2);
    let lru = RgLru::<f32>::new(h, &mut rng);
    let n = seq * batch * h;
    let mut x = vec![0.0f32; n];
    rng.fill_normal(&mut x, 1.0);
    let h0 = lru.zero_state(batch);
    let mut y = vec![0.0f32; n];
    let mut dy = vec![0.0f32; n];
    rng.fill_normal(&mut dy, 1.0);
    let mut cache = RgLruCache::new();
    let par = Paralelo::new(threads);

    println!("  h={h}, {} params, seq={seq}, batch={batch}", lru.n_params());
    for (nome, usar_par) in [("escalar", false), ("paralelo", true)] {
        let (iters, dt) = if usar_par {
            cronometrar(0.4, || {
                lru.forward(&par, &x, &h0, seq, batch, &mut y, &mut cache)
            })
        } else {
            cronometrar(0.4, || {
                lru.forward(&Scalar, &x, &h0, seq, batch, &mut y, &mut cache)
            })
        };
        println!(
            "  forward  {nome:<9} {:>10.0} passos/s",
            (seq * batch * iters) as f64 / dt
        );
    }
    lru.forward(&par, &x, &h0, seq, batch, &mut y, &mut cache);
    let mut grad = lru.grad();
    let mut dx = vec![0.0f32; n];
    let (iters, dt) = cronometrar(0.4, || {
        grad.clear();
        lru.backward(
            &par, &x, &y, &h0, &dy, None, seq, batch, &mut cache, &mut dx, false, None, &mut grad,
        );
    });
    println!(
        "  backward {:<9} {:>10.0} passos/s",
        "paralelo",
        (seq * batch * iters) as f64 / dt
    );
}

fn bench_patcher() {
    println!("\n─── Patcher: compressao em portugues real ──────────────────────");
    let corpus = match std::fs::read("dados/corpus_pt.txt") {
        Ok(c) => c,
        Err(_) => {
            println!("  (dados/corpus_pt.txt nao encontrado, pulando)");
            return;
        }
    };
    let (seq, batch) = (512usize, 8usize);
    let bytes: Vec<u8> = corpus[..seq * batch].to_vec();
    for p in [
        &PorPalavra { max: 8 } as &dyn Patcher,
        &PorClasse { max: 8 } as &dyn Patcher,
        &Fixo { p: 4 } as &dyn Patcher,
    ] {
        let plano = Plano::novo(p, &bytes, seq, batch);
        println!(
            "  {:<12} {:.2} bytes/patch  →  backbone roda {:.1}x menos",
            p.nome(),
            plano.bytes_por_patch(),
            plano.bytes_por_patch()
        );
    }
}

fn bench_modelo(threads: usize) {
    println!("\n─── Modelo completo, um passo de treino ────────────────────────");
    let par = Paralelo::new(threads);
    let (seq, batch) = (256usize, 8usize);
    let corpus = std::fs::read("dados/corpus_pt.txt").unwrap_or_else(|_| {
        "a teka pensa byte a byte e age com ferramentas. "
            .bytes()
            .cycle()
            .take(1 << 16)
            .collect()
    });

    for (nome, cfg) in [("pequeno", Config::pequeno()), ("padrao", Config::padrao())] {
        let mut rng = Rng::new(9);
        let modelo = Teka::<f32>::new(cfg, &mut rng);
        let bytes: Vec<u8> = (0..seq * batch).map(|k| corpus[k % corpus.len()]).collect();
        let alvos: Vec<u8> = (0..seq * batch)
            .map(|k| corpus[(k + 1) % corpus.len()])
            .collect();
        let plano = Plano::novo(&PorPalavra::default(), &bytes, seq, batch);
        let est = modelo.estado_zero(batch);
        let mut cache = TekaCache::new();
        let mut grad = modelo.grad();

        let (iters, dt) = cronometrar(0.6, || {
            grad.clear();
            modelo.passo(
                &par,
                &bytes,
                &alvos,
                &plano,
                &est,
                &mut cache,
                Some(&mut grad),
            );
        });
        let bps = (seq * batch * iters) as f64 / dt;
        println!(
            "  {nome:<8} {:>9} params ({:.0}% backbone), {:.1} MB f32",
            modelo.n_params(),
            modelo.fracao_backbone() * 100.0,
            modelo.n_params() as f64 * 4.0 / 1e6
        );
        println!(
            "           treino: {bps:>8.0} bytes/s   ({:.1} MB/hora)",
            bps * 3600.0 / 1e6
        );
    }
}

// ---------------------------------------------------------------------------
// treino
// ---------------------------------------------------------------------------

struct Args {
    modo: String,
    preset: String,
    corpus: PathBuf,
    saida: Option<PathBuf>,
    carregar: Option<PathBuf>,
    threads: usize,
    cfg: CfgTreino,
    sup: CfgSup,
    exemplos: usize,
    pedido: Option<String>,
    repetir_escritos: usize,
    vies_abster: f64,
    semente: u64,
    corpus_agente: Option<PathBuf>,
    categorias: Option<String>,
    aleatorio: usize,
    todas: usize,
    pausa: u64,
    sem_confirmar: bool,
    patcher: String,
    patcher_pedido: bool,
    ngrama: Option<PathBuf>,
    limiar: f32,
    ordem: usize,
    bits_tabela: u32,
    val: Option<PathBuf>,
    completo: bool,
    texto_val: Option<PathBuf>,
    tentativas: usize,
    rodadas: usize,
    servir: bool,
    porta_servico: u16,
    /// Nao carrega nem grava a memoria episodica.
    ///
    /// Existe porque MEDIR estava sujando o que se mede: o `medir_conversa.py`
    /// usa o modo interativo, que grava a memoria ao sair, e duas corridas
    /// deixaram 938 episodios de artefato. Nenhum e ensinavel (feedback
    /// `Nenhum`), entao nao contaminam o treino — mas afogam o sinal de
    /// percepcao nova do pulso e mentem sobre quanto ela foi usada.
    sem_memoria: bool,
    pulso: teka::pulso::CfgPulso,
    /// Quantos ticks rodar. `0` = sem fim, ate Ctrl-C.
    ticks: u64,
    agir: bool,
    paginas: usize,
    raiz_real: Option<PathBuf>,
    cerebro: Option<PathBuf>,
    avaliar: bool,
    benchmark: bool,
    ger: CfgGerador,
}

fn parse_args() -> Args {
    let brutos: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args {
        modo: "bench".into(),
        preset: "pequeno".into(),
        // Medido: o corpus de literatura de 1898 (herdado da nila_mind) faz
        // 5,52 bits/byte em texto tecnico, contra 1,98 do misto — e nele a palavra
        // "arquivo" aparece ZERO vezes. Um corpus que ensina portugues a uma agente
        // de arquivos sem a palavra arquivo. O misto e 74% daquele + 26% de
        // Wikipedia tecnica coletada, e ganha nos DOIS dominios.
        corpus: PathBuf::from("dados/corpus_misto.txt"),
        saida: None,
        carregar: None,
        threads: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        cfg: CfgTreino::default(),
        sup: CfgSup::default(),
        exemplos: 6000,
        pedido: None,
        repetir_escritos: 10,
        vies_abster: 0.0,
        semente: 7,
        corpus_agente: None,
        categorias: None,
        aleatorio: 0,
        todas: 0,
        pausa: 1_000,
        sem_confirmar: false,
        patcher: "entropia".into(),
        patcher_pedido: false,
        ngrama: Some(PathBuf::from("dados/ngrama.bin")),
        limiar: 5.5,
        ordem: 4,
        bits_tabela: 17,
        val: None,
        completo: false,
        texto_val: None,
        tentativas: 200,
        rodadas: 10,
        servir: false,
        porta_servico: 8931,
        sem_memoria: false,
        pulso: teka::pulso::CfgPulso::default(),
        ticks: 0,
        agir: false,
        paginas: 500,
        raiz_real: None,
        cerebro: None,
        avaliar: false,
        benchmark: false,
        ger: CfgGerador::default(),
    };
    let mut i = 0;
    while i < brutos.len() {
        let arg = brutos[i].as_str();
        let mut prox = || -> String {
            i += 1;
            brutos.get(i).cloned().unwrap_or_default()
        };
        match arg {
            "treinar" | "bench" | "agente" | "gerar" | "coletar" | "medir-lm" | "quantizar"
            | "praticar" | "pulsar" | "exportar" | "ngrama" => {
                a.modo = arg.into()
            }
            "--texto" => a.texto_val = Some(PathBuf::from(prox())),
            "--categorias" => a.categorias = Some(prox()),
            "--aleatorio" => a.aleatorio = prox().parse().unwrap_or(a.aleatorio),
            "--todas" => a.todas = prox().parse().unwrap_or(a.todas),
            "--pausa" => a.pausa = prox().parse().unwrap_or(a.pausa),
            "--sem-confirmar" => a.sem_confirmar = true,
            "--patcher" => {
                a.patcher = prox();
                a.patcher_pedido = true;
            }
            "--ngrama" => a.ngrama = Some(PathBuf::from(prox())),
            "--limiar" => a.limiar = prox().parse().unwrap_or(a.limiar),
            "--ordem" => a.ordem = prox().parse().unwrap_or(a.ordem),
            "--bits" => a.bits_tabela = prox().parse().unwrap_or(a.bits_tabela),
            "--val" => a.val = Some(PathBuf::from(prox())),
            "--paginas" => a.paginas = prox().parse().unwrap_or(a.paginas),
            "--completo" => a.completo = true,
            "--modelo" => a.ger.modelo = prox(),
            // Renomeada. Havia DOIS `--rodadas` no mesmo `match`: o do gerador e o
            // da pratica. O primeiro vencia em silencio, e um pedido de 8 rodadas
            // virava 10 sem aviso — bug que engana medicao, que e o pior tipo.
            "--rodadas-gerador" => a.ger.rodadas = prox().parse().unwrap_or(a.ger.rodadas),
            "--por-chamada" => {
                a.ger.por_chamada = prox().parse().unwrap_or(a.ger.por_chamada)
            }
            "--porta" => a.ger.porta = prox().parse().unwrap_or(a.ger.porta),
            "--carregar" => a.carregar = Some(PathBuf::from(prox())),
            "--exemplos" => a.exemplos = prox().parse().unwrap_or(a.exemplos),
            "--epocas" => a.sup.epocas = prox().parse().unwrap_or(a.sup.epocas),
            "--pedido" => a.pedido = Some(prox()),
            "--vies-abster" => a.vies_abster = prox().parse().unwrap_or(a.vies_abster),
            "--servir" => a.servir = true,
            "--porta-servico" => a.porta_servico = prox().parse().unwrap_or(a.porta_servico),
            "--sem-memoria" => a.sem_memoria = true,
            "--intervalo" => a.pulso.intervalo_s = prox().parse().unwrap_or(a.pulso.intervalo_s),
            "--pensar" => {
                a.pulso.bytes_pensamento = prox().parse().unwrap_or(a.pulso.bytes_pensamento)
            }
            "--consolidar-a-cada" => {
                a.pulso.consolidar_a_cada = prox().parse().unwrap_or(a.pulso.consolidar_a_cada)
            }
            "--ticks" => a.ticks = prox().parse().unwrap_or(a.ticks),
            "--agir" => a.agir = true,
            "--semente" => a.semente = prox().parse().unwrap_or(a.semente),
            "--corpus-agente" => a.corpus_agente = Some(PathBuf::from(prox())),
            "--tentativas" => a.tentativas = prox().parse().unwrap_or(a.tentativas),
            "--rodadas" => a.rodadas = prox().parse().unwrap_or(a.rodadas),
            "--repetir-escritos" => {
                a.repetir_escritos = prox().parse().unwrap_or(a.repetir_escritos)
            }
            "--real" => a.raiz_real = Some(PathBuf::from(prox())),
            "--cerebro" => a.cerebro = Some(PathBuf::from(prox())),
            "--avaliar" => a.avaliar = true,
            "--benchmark" => a.benchmark = true,
            "--lr-tronco" => a.sup.lr_tronco = prox().parse().unwrap_or(a.sup.lr_tronco),
            "--preset" => a.preset = prox(),
            "--corpus" => a.corpus = PathBuf::from(prox()),
            "--saida" => a.saida = Some(PathBuf::from(prox())),
            "--threads" => a.threads = prox().parse().unwrap_or(a.threads),
            "--seq" => a.cfg.seq = prox().parse().unwrap_or(a.cfg.seq),
            // O comprimento do TREINO DA AGENTE, que nao tinha flag.
            //
            // Nao e limite de arquitetura: `Config` nao tem campo de comprimento, e
            // `estado_zero` depende do lote e nao do tamanho da entrada. Um SSM
            // carrega a ordem na recorrencia, entao nao ha posicao codificada nem
            // atencao — o estado tem tamanho constante e o custo e LINEAR no
            // comprimento, nao quadratico.
            //
            // Na inferencia ela ja aceita qualquer tamanho: `responder` faz
            // `seq = bytes.len()`. Medido, 654 bytes entram sem erro — o que sai e
            // ruim porque ela nunca treinou nesse regime, nao porque nao caiba.
            //
            // Subir isto so faz sentido quando os DADOS crescerem. Hoje a mediana
            // das frases e 29 bytes e nenhuma das 500 do John passa de 56: treinar
            // com 512 seria 94% de enchimento, custo linear pago por nada.
            "--seq-agente" => a.sup.seq = prox().parse().unwrap_or(a.sup.seq),
            "--batch" => a.cfg.batch = prox().parse().unwrap_or(a.cfg.batch),
            "--lr" => {
                let v = prox().parse().unwrap_or(a.cfg.lr);
                a.cfg.lr = v;
                a.sup.lr = v;
            }
            "--clip" => a.cfg.clip = prox().parse().unwrap_or(a.cfg.clip),
            "--passos" => a.cfg.passos = prox().parse().unwrap_or(a.cfg.passos),
            "--minutos" => a.cfg.minutos = prox().parse().unwrap_or(a.cfg.minutos),
            "--log" => a.cfg.log_cada = prox().parse().unwrap_or(a.cfg.log_cada),
            "-h" | "--help" => {
                ajuda();
                std::process::exit(0);
            }
            outro => eprintln!("  aviso: argumento desconhecido {outro}"),
        }
        i += 1;
    }
    a
}

fn ajuda() {
    println!(
        "\nTeka — agente cognitivo byte a byte, 100% local\n\n\
         USAR NO DIA A DIA\n  \
           teka agente --carregar teka.bin           modo interativo\n  \
           teka agente --carregar teka.bin --pedido \"que horas sao\"\n  \
           teka agente --carregar teka.bin --real C:\\teka\\area   sai do sandbox\n\n\
         COMANDOS DO MODO INTERATIVO\n  \
           /certo                 a ultima decisao estava certa\n  \
           /errado <ferramenta>   corrige a ultima decisao\n  \
           /consolidar            fixa nos pesos o que voce corrigiu\n  \
           /explorar              liga/desliga experimentar respostas novas\n  \
           /reforcar              aprende do desfecho (executou / falhou)\n  \
           /aprender              consolidar + reforcar, na ordem certa\n  \
           /parecido <texto>      o que ja aconteceu parecido com isso\n  \
           /memoria               estado da memoria\n  \
           /sair\n\n\
         TREINAR O AGENTE (decidir ferramenta e argumentos)\n  \
           teka agente --epocas 12 --exemplos 16000 --saida teka.bin\n  \
           --preset pequeno|padrao    tamanho do modelo (padrao: pequeno)\n  \
           --exemplos <n>             exemplos sinteticos gerados\n  \
           --epocas <n>               epocas do treino supervisionado\n  \
           --lr <x>                   taxa de aprendizado\n  \
           --aleatorio <n>            titulos sorteados pela API (largura de registro)\n  \
           --todas <n>                enumera o acervo, com cursor que continua entre rodadas\n  \
           --pausa <ms>               intervalo base entre pedidos ao coletar\n  \
           --patcher <por_palavra|entropia>  qual patcher usar (padrao: entropia)\n  \
           --ngrama <arquivo>         tabela de entropia (exigida por --patcher entropia)\n  \
           --limiar <bits>            entropia acima da qual abre patch novo\n  \
           --cerebro <arquivo>        parte de um tronco ja treinado em portugues\n  \
           --lr-tronco <x>            taxa do tronco em relacao as cabecas\n  \
           --repetir-escritos <n>     peso dos exemplos escritos a mao (padrao 10)\n\n\
         GERAR EXEMPLOS COM O LLM LOCAL (offline; a Teka nao usa isso em runtime)\n  \
           teka gerar                                  escreve dados/propostas.txt\n  \
           --modelo <id>              padrao: qwen2.5-7b-instruct\n  \
           --rodadas-gerador <n>      chamadas por ferramenta (padrao 2)\n  \
           --por-chamada <n>          frases por chamada (padrao 15)\n  \
           --porta <n>                padrao: 1234\n\n\
         MEDIR\n  \
           teka pulsar --carregar teka.bin           ela roda sozinha: pensa e consolida\n  \
           teka pulsar --carregar teka.bin --intervalo 60 --ticks 5\n\n  \
           teka agente --carregar teka.bin --benchmark   24 frases escritas a mao\n  \
           teka agente --carregar teka.bin --avaliar     validacao completa\n  \
           teka bench                                    velocidade do backend\n\n\
         TREINAR O MODELO DE LINGUAGEM (prever o proximo byte)\n  \
           teka treinar --minutos 30 --saida cerebro.bin\n  \
           --corpus <arquivo>         padrao: dados/corpus_misto.txt\n  \
           --minutos <n> | --passos <n>\n  \
           --seq <n> --batch <n> --clip <x>\n\n\
         GERAL\n  \
           --threads <n>              padrao: todos os cores logicos\n  \
           -h, --help\n\n\
         No REPL: /buscar procura na web o ultimo pedido que ela NAO entendeu,\n\
         mostra o resultado e guarda como fato em semantica.bin.\n\
         A memoria fica em memoria.bin, no diretorio atual, e sobrevive a reinicio.\n\
         Por padrao tudo roda em SANDBOX: escrever e executar nao fazem nada.\n"
    );
}

/// Dias desde a epoca, fracionarios. E o relogio que a memoria semantica usa para
/// meia-vida e esquecimento — ela nao chama o sistema por conta propria, quem decide
/// que horas sao e quem a usa.
fn agora_em_dias() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() / 86_400.0)
        .unwrap_or(0.0)
}

/// Responde um pedido e grava o episodio.
///
/// E funcao livre, e nao closure, de proposito: um closure que capture `ag` mantem
/// o emprestimo vivo pelo REPL inteiro, e `/consolidar` precisa de `&mut ag`.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn responder(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &dyn Patcher,
    exec: &mut Executor,
    pedido: &str,
    temperatura: f64,
    rng: &mut Rng,
    mem: &mut MemoriaEpisodica,
    cache: &mut AgenteCache<f32>,
    ctx: &mut Contexto,
    // `Some((pedido, indice))` quando o turno anterior foi uma abstencao: o pedido
    // que ela nao entendeu e onde o episodio dele esta na memoria.
    pendente: &mut Option<(String, usize)>,
    // O que ela SABE, separado do que ela FEZ. Alimentada por `/buscar`.
    sem: &mut teka::memory::semantica::MemoriaSemantica,
    agora: f64,
) {
    ctx.avancar();
    // Com temperatura > 0 ela AMOSTRA em vez de pegar a melhor. Fica desligado por
    // padrao: exploracao e modo de aprendizado, nao de uso.
    //
    // O contexto de conversa so entra no caminho normal: explorar existe para gerar
    // variedade de treino, e completar o pedido por contexto ali mascararia o que a
    // politica de fato escolheu.
    let (chamada, conf) = if temperatura > 0.0 {
        (ag.responder_explorando(ops, patcher, pedido, temperatura, rng, cache), None)
    } else {
        match ag.responder_com_confianca(ops, patcher, pedido, cache) {
            Ok((c, cf)) => (Ok(c), Some(cf)),
            Err(e) => (Err(e), None),
        }
    };

    // ---- "esse arquivo": o objeto ficou no turno anterior ----
    //
    // Ela abstem porque o ponteiro nao acha o que copiar, e faz certo: a entrada nao
    // tem trecho nenhum para copiar. Quem sabe o que "esse" quer dizer e a conversa.
    //
    // O contexto NAO escolhe ferramenta. Ele troca o pronome pelo objeto lembrado e
    // a pergunta e refeita ao modelo, que decide de novo sobre uma frase que agora
    // tem objeto. Ver `memory::contexto` para o desenho que foi tentado antes deste
    // e por que a segunda colocada da cabeca de intencao nao serve.
    let abstem = |c: &teka::tools::Chamada| {
        ag.registro.ferramentas.get(c.ferramenta).map(|f| f.nome.as_str()) == Some("perguntar")
    };
    let mut via_contexto: Option<String> = None;
    let chamada = match chamada {
        Ok(c) if conf.is_some() && abstem(&c) => {
            // Tenta cada leitura possivel, da mais provavel para a menos.
            //
            // Anafora tipada ("esse arquivo") da um candidato so. Deitico puro
            // ("isso", "isso ai") da um por gaveta cheia, em ordem de quem foi
            // mencionado por ultimo — porque "isso" nao diz o que aponta, e quem
            // desambigua e o VERBO. Escrever regra de verbo aqui seria o Achado 19;
            // oferecer os referentes e deixar o modelo escolher, nao.
            //
            // Vence a primeira reescrita que ele resolve sem abster.
            let resolvido = ctx.candidatos(pedido).into_iter().find_map(|r| {
                let (c2, _) = ag.responder_com_confianca(ops, patcher, &r.pedido, cache).ok()?;
                (!abstem(&c2)).then_some((c2, r.valor))
            });
            match resolvido {
                // A reescrita resolveu: ela escolheu uma ferramenta de verdade.
                Some((c2, valor)) => {
                    let efeito = ag
                        .registro
                        .ferramentas
                        .get(c2.ferramenta)
                        .map(|f| f.prim.efeito_colateral())
                        .unwrap_or(true);
                    if efeito {
                        // Escrever e executar nao sao adivinhados a partir de um
                        // turno anterior: a conveniencia nao paga o estrago quando
                        // erra. Oferece e para por aqui.
                        let nome = ag
                            .registro
                            .ferramentas
                            .get(c2.ferramenta)
                            .map(|f| f.nome.as_str())
                            .unwrap_or("?");
                        println!("  → voce quer dizer: {nome} em {valor:?}? (repete com o nome que eu faco)");
                        return;
                    }
                    via_contexto = Some(valor);
                    Ok(c2)
                }
                // Ou nao havia anafora, ou nao havia lembranca, ou ela continuou sem
                // entender com nenhum dos objetos oferecidos. Nos tres a abstencao fica.
                None => {
                    if std::env::var("TEKA_DEBUG").is_ok() {
                        if let Some(cf) = &conf {
                            let nome = |i: usize| {
                                ag.registro
                                    .ferramentas
                                    .get(i)
                                    .map(|f| f.nome.as_str())
                                    .unwrap_or("?")
                            };
                            eprintln!(
                                "    [debug] 1o {} p={:.3} | 2o {} p={:.3} | contexto: {:?}",
                                nome(cf.ferramenta),
                                cf.p1,
                                nome(cf.segunda),
                                cf.p2,
                                ctx.candidatos(pedido)
                                    .into_iter()
                                    .map(|r| r.pedido)
                                    .collect::<Vec<_>>()
                            );
                        }
                    }
                    Ok(c)
                }
            }
        }
        outro => outro,
    };
    // O executor e quem poe as guardas: diario para o que nao tem volta, oficina
    // para o que tem. Chamar `registro.executar` direto daqui pularia as duas.
    let saida = match &chamada {
        Ok(c) => exec.executar(&ag.registro, c),
        Err(e) => Err(e.clone()),
    };
    let assinatura = ag.assinatura(cache);
    match chamada {
        Ok(c) => {
            match &via_contexto {
                // Sempre visivel. Errar barato e as claras e aceitavel; em silencio
                // nao e — voce precisa poder dizer "nao, o outro arquivo".
                Some(v) => println!("  → {}   [pelo contexto: {v:?}]", c.texto(&ag.registro)),
                None => println!("  → {}", c.texto(&ag.registro)),
            }
            let ok = match saida {
                Ok(s) => {
                    for linha in s.lines().take(14) {
                        println!("    {linha}");
                    }
                    true
                }
                Err(e) => {
                    println!("    (erro) {e}");
                    false
                }
            };
            // So lembra do que deu certo: guardar o alvo de uma chamada que
            // falhou faria "esse arquivo" apontar para um caminho que nao existe.
            if ok {
                ctx.observar(&ag.registro, &c);
            }

            // ---- ensinar por explicacao ----
            //
            // O turno anterior foi uma abstencao e ESTE resolveu numa ferramenta de
            // verdade: entao este turno era o esclarecimento daquele. Liga os dois.
            //
            // Quem interpretou a explicacao foi a propria cabeca de intencao, nao
            // uma regra de verbo escrita a mao — e o mesmo principio do
            // `memory::contexto`, e pelo mesmo motivo (Achado 19).
            if !abstem(&c) {
                if let Some((original, indice)) = pendente.take() {
                    if let Some(ep) = mem.episodios.get_mut(indice) {
                        ep.feedback = Feedback::Corrigido {
                            ferramenta: c.ferramenta,
                            args: vec![],
                        };
                        let nome = ag
                            .registro
                            .ferramentas
                            .get(c.ferramenta)
                            .map(|f| f.nome.as_str())
                            .unwrap_or("?");
                        println!(
                            "    [aprendi] {original:?} queria {nome}. \
                             /consolidar fixa nos pesos, /desaprender desfaz."
                        );
                    }
                }
            }
            mem.gravar(
                pedido,
                c.ferramenta,
                vec![],
                if ok { Resultado::Executou } else { Resultado::Falhou },
                Feedback::Nenhum,
                assinatura.clone(),
            );

            // Abstencao ARMA a licao: o proximo turno, se resolver, ensina este.
            //
            // Nao ha marcador nem comando — e o que gente faz. O risco e voce mudar
            // de assunto logo depois de ela nao entender, e a ligacao sair errada;
            // por isso ela ANUNCIA o que aprendeu e ha `/desaprender`.
            *pendente = if abstem(&c) {
                // Antes de desistir, ela consulta o que JA consultou um dia. Este e
                // o unico ponto em que a memoria semantica devolve valor: sem ele,
                // gravar fato seria escrever num arquivo que ninguem le.
                //
                // O limiar de 0,80 e alto de proposito. Fato lembrado e mostrado
                // como resposta; devolver o fato errado com confianca e pior que
                // dizer "nao sei", que e o que ela ja fazia de graca.
                let lembrados = sem.lembrar(&assinatura, 2, agora);
                let bom = lembrados.iter().find(|(_, s)| *s >= 0.80);
                match bom {
                    Some((texto, escore)) => {
                        println!("  → do que eu ja consultei ({escore:.2}): {texto}");
                        println!("    (se nao servir, /buscar procura de novo)");
                    }
                    None => {
                        println!("    (me explique o que eu deveria fazer, e eu aprendo)");
                        println!("    (ou /buscar, que eu procuro na web e guardo)");
                    }
                }
                Some((pedido.to_string(), mem.len() - 1))
            } else {
                None
            };
        }
        Err(e) => println!("  → nao consegui montar a chamada: {e}"),
    }
}

fn rodar_agente(args: &Args) {
    // Sem `--saida`, salva de volta no arquivo que foi carregado.
    //
    // O contrario e a armadilha obvia: voce corrige, consolida, ve a acuracia subir
    // — e perde tudo ao sair, porque os PESOS nao foram para o disco (so a memoria
    // episodica foi). Um agente que aprende tem que persistir por padrao.
    let saida = args.saida.clone().or_else(|| args.carregar.clone());
    let ops = Paralelo::new(args.threads);
    let patcher_dono = montar_patcher(args);
    let patcher: &dyn Patcher = &*patcher_dono;
    let registro = Registro::padrao();
    // Semente do modelo. Existe como flag por um motivo especifico: sem poder
    // trocar a semente nao ha como medir o CHAO DE RUIDO — quanto o resultado
    // muda sozinho, sem mudar nada. E sem esse numero, toda comparacao A/B e um
    // chute sobre se a diferenca era real.
    let mut rng = Rng::new(args.semente);

    let mut ag = match &args.carregar {
        Some(caminho) => match Agente::<f32>::carregar(caminho, registro.clone()) {
            Ok(a) => {
                println!("  agente carregado de {} ({} params)", caminho.display(), a.n_params());
                a
            }
            Err(e) => {
                eprintln!("  falha ao carregar {}: {e}", caminho.display());
                std::process::exit(1);
            }
        },
        None => {
            let cfg_modelo = resolver_preset(&args.preset);
            // Sobre um cerebro ja treinado em portugues, se houver.
            let mut ag = match &args.cerebro {
                Some(caminho) => match Teka::<f32>::carregar(caminho) {
                    Ok(m) => {
                        println!(
                            "  tronco pre-treinado carregado de {} ({} params)",
                            caminho.display(),
                            m.n_params()
                        );
                        Agente::sobre(m, registro.clone(), &mut rng)
                    }
                    Err(e) => {
                        eprintln!("  falha ao carregar {}: {e}", caminho.display());
                        std::process::exit(1);
                    }
                },
                None => Agente::<f32>::novo(cfg_modelo, registro.clone(), &mut rng),
            };
            let mut r = Rng::new(4242);
            let mut exs = gerar(&ag.registro, &patcher, args.exemplos, &mut r);

            // Exemplos escritos a mao (destilacao), REPETIDOS.
            //
            // Sao ~240 contra ~12.000 gerados por molde. Somados uma vez so eles
            // virariam 2% do lote e sumiriam no ruido. Repetir e o jeito honesto de
            // dar peso a eles sem inventar amostragem ponderada — e o benchmark de
            // frases ineditas diz se ajudou ou se so decorou.
            // O corpus normal e compilado junto (`include_str!`), para o binario ser
            // autossuficiente. `--corpus` le do disco no lugar dele, o que permite
            // comparar variantes sem recompilar nem renomear arquivo — trocar
            // arquivo na mao para medir e como se perde de qual numero veio de onde.
            let texto_corpus: String = match &args.corpus_agente {
                Some(caminho) => match std::fs::read_to_string(caminho) {
                    Ok(t) => {
                        println!("  corpus alternativo: {}", caminho.display());
                        t
                    }
                    Err(e) => {
                        eprintln!("  nao consegui ler {}: {e}", caminho.display());
                        std::process::exit(1);
                    }
                },
                None => include_str!("../dados/exemplos_teka.txt").to_string(),
            };
            let escritos = teka::learn::dados::ler_exemplos(&texto_corpus, &ag.registro);
            if args.repetir_escritos > 0 && !escritos.is_empty() {
                println!(
                    "  + {} exemplos escritos a mao, repetidos {}x",
                    escritos.len(),
                    args.repetir_escritos
                );
                for _ in 0..args.repetir_escritos {
                    exs.extend(escritos.iter().cloned());
                }
            }
            // Divisao por FRASE: as frases da validacao nunca aparecem no treino.
            // Cada frase escrita a mao e a sua propria frase, entao 1 em 4 delas
            // tambem fica de fora — e como sao mais realistas que os moldes, a
            // validacao fica mais dura, nao mais facil.
            let (treino, val) = dividir_por_frase(exs, 4);
            println!("\n  TEKA — treino do agente (preset {})\n", args.preset);
            treinar_agente(&mut ag, &ops, &patcher, &treino, &val, &args.sup);
            if let Some(caminho) = &saida {
                match ag.salvar(caminho) {
                    Ok(n) => println!(
                        "\n  agente salvo em {} ({:.1} MB)",
                        caminho.display(),
                        n as f64 / 1e6
                    ),
                    Err(e) => eprintln!("  falha ao salvar: {e}"),
                }
            }
            ag
        }
    };
    ag.vies_abster = args.vies_abster;
    if args.vies_abster != 0.0 {
        println!("\n  vies de abstencao: {:+.1}", args.vies_abster);
    }

    if args.benchmark {
        let texto = include_str!("../dados/frases_teste.txt");
        let casos = teka::learn::dados::ler_casos_teste(texto);
        let mut c = AgenteCache::new();
        let (mut ok_ferr, mut ok_arg, mut n_arg) = (0usize, 0usize, 0usize);
        let mut sep = Separacao::default();
        // Tres sinais candidatos, medidos lado a lado. Nao ha razao para supor que
        // a margem seja o melhor — ela so era o mais obvio.
        let mut sep_critico = Separacao::default();
        let mut sep_arg = Separacao::default();
        println!("\n  {} frases escritas a mao, nenhuma no gerador\n", casos.len());
        for caso in &casos {
            let r = ag.responder_com_confianca(&ops, &patcher, &caso.pedido, &mut c);
            let (marca, texto_ch, margem) = match &r {
                Ok((ch, conf)) => {
                    let nome = &ag.registro.ferramentas[ch.ferramenta].nome;
                    let certo = *nome == caso.ferramenta;
                    ok_ferr += certo as usize;
                    sep.anotar(certo, conf.margem());
                    // O critico preve o retorno: quanto MAIOR, mais ele espera dar
                    // certo. Mesma orientacao da margem, entao entra direto.
                    sep_critico.anotar(certo, conf.valor);
                    // Argumento degenerado: os erros vinham com recorte de 1 byte
                    // ("g" de ipconfig, "r", "c"). Normaliza para "quanto maior,
                    // mais confiavel", em fracao do pedido.
                    let menor = ch
                        .args
                        .iter()
                        .map(|(_, v)| v.len())
                        .min()
                        .unwrap_or(caso.pedido.len());
                    sep_arg.anotar(certo, menor as f64 / caso.pedido.len().max(1) as f64);
                    if let Some(esperado) = &caso.argumento {
                        n_arg += 1;
                        if ch.args.iter().any(|(_, v)| v == esperado) {
                            ok_arg += 1;
                        }
                    }
                    (
                        if certo { "ok " } else { "ERR" },
                        ch.texto(&ag.registro),
                        conf.margem(),
                    )
                }
                Err(e) => ("ERR", e.clone(), 0.0),
            };
            println!("  {marca} {:<48} {:<44} m={margem:.3}", caso.pedido, texto_ch);
        }
        println!(
            "\n  ferramenta certa: {ok_ferr}/{} ({:.0}%)",
            casos.len(),
            100.0 * ok_ferr as f64 / casos.len() as f64
        );
        if n_arg > 0 {
            println!(
                "  argumento certo:  {ok_arg}/{n_arg} ({:.0}%)",
                100.0 * ok_arg as f64 / n_arg as f64
            );
        }

        // -- abster: vale a pena? --
        //
        // A tabela e a resposta honesta. Se nenhum limiar der saldo positivo, a
        // margem nao separa acerto de erro neste modelo, e abster so trocaria erro
        // por silencio.
        const CANDIDATOS: &[f64] = &[0.05, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70];
        const CAND_CRIT: &[f64] = &[-0.30, -0.20, -0.10, -0.05, 0.0, 0.05, 0.10];
        const CAND_ARG: &[f64] = &[0.02, 0.04, 0.06, 0.08, 0.10, 0.15, 0.20];

        println!("\n  ABSTENCAO — qual sinal separa acerto de erro?\n");
        println!("  [1] MARGEM (p1 - p2)");
        print!("{}", sep.tabela(CANDIDATOS));
        println!("\n  [2] CRITICO V(s)");
        print!("{}", sep_critico.tabela(CAND_CRIT));
        println!("\n  [3] MENOR ARGUMENTO / TAMANHO DO PEDIDO");
        print!("{}", sep_arg.tabela(CAND_ARG));
        for (nome, s, cand) in [
            ("margem", &sep, CANDIDATOS),
            ("critico", &sep_critico, CAND_CRIT),
            ("argumento", &sep_arg, CAND_ARG),
        ] {
            let (l, saldo) = s.melhor_limiar(cand);
            println!("  {nome:<10} melhor limiar {l:>6.2}  saldo {saldo:+}");
        }
        let (limiar, saldo) = sep.melhor_limiar(CANDIDATOS);
        if saldo > 0 {
            let (evitados, perdidos) = sep.em(limiar);
            println!(
                "\n  melhor limiar {limiar:.2}: evita {evitados} erros, custa {perdidos} acertos \
                 (saldo +{saldo})"
            );
            println!(
                "  ferramenta certa entre as que ela responde: {}/{} ({:.0}%)",
                ok_ferr - perdidos,
                casos.len() - evitados - perdidos,
                100.0 * (ok_ferr - perdidos) as f64
                    / (casos.len() - evitados - perdidos).max(1) as f64
            );
        } else {
            println!("\n  nenhum limiar da saldo positivo — a margem NAO separa aqui.");
        }
        return;
    }

    if args.avaliar {
        let mut r = Rng::new(4242);
        let exs = gerar(&ag.registro, &patcher, args.exemplos, &mut r);
        let (_, val) = dividir_por_frase(exs, 4);
        let mut c = AgenteCache::new();
        let (perda, p) = teka::learn::supervisionado::avaliar(
            &ag, &ops, &patcher, &val, args.sup.seq, args.sup.batch, &mut c,
        );
        println!("\n  {} exemplos de validacao (frases ineditas)", val.len());
        println!("  perda .................. {perda:.3}");
        println!("  intencao ............... {:.1}%", p.acuracia_intencao() * 100.0);
        println!("  argumento (byte exato) . {:.1}%", p.acuracia_span() * 100.0);
        println!("  argumento | ferr. certa  {:.1}%", p.acuracia_span_dado_intencao() * 100.0);
        println!("  patch (auxiliar) ....... {:.1}%", p.acuracia_patch() * 100.0);
        println!("  ponta a ponta .......... {:.1}%", p.acuracia_total() * 100.0);
        return;
    }

    let pol = match &args.raiz_real {
        Some(raiz) => {
            println!("\n  ATENCAO: modo REAL, preso a {}", raiz.display());
            Politica::real_em(raiz.clone())
        }
        None => Politica::default(), // sandbox
    };

    // O diario e por raiz: dois projetos diferentes nao compartilham historico de
    // efeito, senao escrever `notas.md` num deles bloquearia o outro.
    let caminho_diario = args
        .raiz_real
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".teka-diario.log");
    // Confirmacao LIGADA aqui, que e o unico lugar com gente para responder. O
    // benchmark, o `--pedido` e o ambiente de treino constroem o executor sem ela —
    // uma pergunta sem ninguem do outro lado travaria tudo.
    //
    // `--sem-confirmar` desliga. Quem passa essa flag esta dizendo que sabe.
    let mut exec = match Executor::novo(&caminho_diario, pol) {
        Ok(e) => e.com_confirmacao(!args.sem_confirmar),
        Err(e) => {
            eprintln!("  nao consegui abrir o diario em {}: {e}", caminho_diario.display());
            std::process::exit(1);
        }
    };
    if !args.sem_confirmar && args.raiz_real.is_some() {
        println!("  confirmacao ligada: ela pergunta antes de escrever, mover, apagar ou executar");
    }
    if exec.quantas_no_diario() > 0 {
        println!(
            "  diario: {} execucoes ja registradas ({})",
            exec.quantas_no_diario(),
            caminho_diario.display()
        );
    }

    let mut cache = AgenteCache::new();
    let mut rng_repl = Rng::new(20260824);
    let caminho_memoria = PathBuf::from("memoria.bin");
    let mut memoria = if args.sem_memoria {
        MemoriaEpisodica::nova()
    } else {
        MemoriaEpisodica::carregar(&caminho_memoria).unwrap_or_else(|_| MemoriaEpisodica::nova())
    };
    // A semantica anda em arquivo separado da episodica de proposito: episodio sai
    // de cada turno e envelhece rapido; fato sai do que ela consultou e deve
    // sobreviver a uma limpeza de episodios.
    let caminho_semantica = PathBuf::from("semantica.bin");
    let mut semantica = if args.sem_memoria {
        teka::memory::semantica::MemoriaSemantica::nova(0)
    } else {
        teka::memory::semantica::MemoriaSemantica::carregar(&caminho_semantica)
            .unwrap_or_else(|_| teka::memory::semantica::MemoriaSemantica::nova(0))
    };
    if !semantica.fatos.is_empty() {
        let pendentes = semantica.precisa_reindexar().len();
        println!(
            "  sabe: {} fatos{}",
            semantica.fatos.len(),
            if pendentes > 0 {
                format!(" ({pendentes} precisam reindexar — o modelo mudou)")
            } else {
                String::new()
            }
        );
    }
    if !memoria.is_empty() {
        println!(
            "  memoria: {} episodios ({} ensinaveis)",
            memoria.len(),
            memoria.n_ensinaveis()
        );
    }

    if args.servir {
        // Residente: paga a carga de 6 MB uma vez, nao a cada pedido.
        let cfg = teka::servidor::CfgServidor {
            porta: args.porta_servico,
            executar: args.agir,
        };
        if let Err(e) = teka::servidor::servir(&ag, &ops, &patcher, &mut exec, &mut memoria, &cfg) {
            eprintln!("  servidor falhou: {e}");
            std::process::exit(1);
        }
        // Ctrl-C nao chega aqui, mas encerrar limpo salva a memoria.
        let _ = memoria.salvar(&caminho_memoria);
        if !semantica.fatos.is_empty() {
            let _ = semantica.salvar(&caminho_semantica);
        }
        return;
    }

    // Um turno so: o contexto nasce e morre aqui, e nunca tem o que lembrar. Existe
    // para o caminho ser o mesmo do interativo, sem um segundo `responder`.
    let mut contexto = Contexto::novo();
    // O pedido que ela nao entendeu no turno anterior, esperando explicacao.
    let mut licao_pendente: Option<(String, usize)> = None;

    if let Some(p) = &args.pedido {
        responder(
            &ag, &ops, &patcher, &mut exec, p, 0.0, &mut rng_repl, &mut memoria, &mut cache,
            &mut contexto,
            &mut licao_pendente,
            &mut semantica,
            agora_em_dias(),
        );
        return;
    }

    println!("\n  Escreva um pedido. Comandos:");
    println!("    /certo               a ultima decisao estava certa");
    println!("    /errado <ferramenta> corrige a ultima decisao");
    println!("    /parecido <texto>    o que ja aconteceu parecido com isso");
    println!("    /consolidar          fixa nos pesos o que voce CORRIGIU");
    println!("    /reforcar            aprende do DESFECHO (executou / falhou)");
    println!("    /aprender            os dois, na ordem certa");
    println!("    /explorar            liga/desliga experimentar respostas novas");
    println!("    /desaprender         desfaz a ultima licao que ela tirou de voce");
    println!("    /esquecer            esquece o assunto (\"esse arquivo\" perde o dono)");
    println!("    /memoria             estado da memoria");
    println!("    /sair");
    println!("\n  Ferramentas:");
    for f in &registro.ferramentas {
        println!("    {:<18} {}", f.nome, f.descricao);
    }
    println!();

    let mut explorando = false;
    let recozimento = Recozimento::default();
    let entrada = std::io::stdin();
    loop {
        print!("{}", if explorando { "teka*> " } else { "teka> " });
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        let mut linha = String::new();
        if entrada.read_line(&mut linha).unwrap_or(0) == 0 {
            break;
        }
        let pedido = linha.trim();
        if pedido.is_empty() || pedido == "/sair" {
            break;
        }

        // ---- oficina: o degrau entre fingir e apostar ----
        if let Some(alvo) = pedido.strip_prefix("/oficina") {
            let alvo = alvo.trim();
            let origem = if alvo.is_empty() {
                exec.politica().raiz.clone().unwrap_or_else(|| PathBuf::from("."))
            } else {
                PathBuf::from(alvo)
            };
            let destino = std::env::temp_dir().join(format!(
                "teka-oficina-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ));
            match exec.abrir_oficina(&origem, &destino) {
                Ok(raiz) => {
                    println!("  oficina aberta sobre {}", origem.display());
                    println!("  ela vai mexer em {} — a original nao sera tocada", raiz.display());
                    println!("  depois: /diff para ver, /aplicar ou /descartar para decidir");
                }
                Err(e) => println!("  (erro) {e}"),
            }
            continue;
        }
        if pedido == "/diff" {
            match exec.diff() {
                Ok(m) if m.is_empty() => println!("  nada mudou ainda"),
                Ok(m) => {
                    println!("  {} mudanca(s):", m.len());
                    for x in &m {
                        println!("{x}");
                    }
                }
                Err(e) => println!("  (erro) {e}"),
            }
            continue;
        }
        if pedido == "/aplicar" {
            if !exec.tem_oficina() {
                println!("  nao ha oficina aberta");
                continue;
            }
            let backup = std::env::temp_dir().join("teka-backup");
            match exec.aplicar_oficina(&backup) {
                Ok(m) if m.is_empty() => println!("  nada a aplicar"),
                Ok(m) => println!(
                    "  {} mudanca(s) aplicadas; backup do que foi sobrescrito em {}",
                    m.len(),
                    backup.display()
                ),
                Err(e) => println!("  (erro) {e}"),
            }
            continue;
        }
        if pedido == "/descartar" {
            match exec.descartar_oficina() {
                Ok(()) => println!("  oficina descartada; a pasta original nunca foi tocada"),
                Err(e) => println!("  (erro) {e}"),
            }
            continue;
        }

        if pedido == "/desaprender" {
            // Desfaz a ultima licao. Existe porque a ligacao e automatica: se voce
            // mudou de assunto logo depois de uma abstencao, ela ligou coisas que
            // nao tem relacao, e precisa haver como dizer isso.
            let mut desfez = false;
            for ep in memoria.episodios.iter_mut().rev() {
                if matches!(ep.feedback, Feedback::Corrigido { .. }) {
                    println!("  desaprendido: {:?}", ep.pedido);
                    ep.feedback = Feedback::Nenhum;
                    desfez = true;
                    break;
                }
            }
            if !desfez {
                println!("  nao ha licao para desfazer.");
            }
            continue;
        }

        if pedido == "/esquecer" {
            contexto.limpar();
            println!("  contexto limpo. \"esse arquivo\" volta a nao ter dono.");
            continue;
        }

        if pedido == "/certo" {
            if memoria.anotar_ultimo(Feedback::Aprovado) {
                println!("  anotado.");
            } else {
                println!("  nao ha decisao pendente de resposta.");
            }
            continue;
        }
        if let Some(nome) = pedido.strip_prefix("/errado ") {
            match registro.indice(nome.trim()) {
                Some(ferramenta) => {
                    if memoria.anotar_ultimo(Feedback::Corrigido {
                        ferramenta,
                        args: vec![],
                    }) {
                        println!("  corrigido para {}. Use /consolidar para fixar.", nome.trim());
                    } else {
                        println!("  nao ha decisao pendente de resposta.");
                    }
                }
                None => println!("  nao conheco a ferramenta {:?}", nome.trim()),
            }
            continue;
        }
        if let Some(texto) = pedido.strip_prefix("/parecido ") {
            let _ = ag.responder(&ops, &patcher, texto.trim(), &mut cache);
            let assinatura = ag.assinatura(&cache);
            let top = memoria.parecidos(&assinatura, 3);
            if top.is_empty() {
                println!("  memoria vazia.");
            }
            for (ep, cos) in top {
                println!(
                    "  {:.3}  {:<40} -> {}",
                    cos,
                    ep.pedido,
                    registro.ferramentas[ep.ferramenta].nome
                );
            }
            continue;
        }
        if pedido == "/explorar" {
            explorando = !explorando;
            if explorando {
                println!("  explorando: ela vai EXPERIMENTAR respostas em vez de sempre");
                println!("  dar a melhor conhecida. A temperatura cai sozinha conforme");
                println!("  cada pedido vai sendo tentado. Use /reforcar depois.");
            } else {
                println!("  exploracao desligada: sempre a melhor resposta conhecida.");
            }
            continue;
        }
        if pedido == "/reforcar" || pedido == "/aprender" {
            let mut r = Rng::new(7);
            let exs = gerar(&ag.registro, &patcher, args.exemplos, &mut r);
            let (base, val) = dividir_por_frase(exs, 4);

            if pedido == "/aprender" && memoria.n_ensinaveis() > 0 {
                // A correcao explicita vem primeiro: e o sinal mais forte que existe,
                // e o reforco parte de uma politica ja corrigida.
                let rel = consolidar(
                    &mut ag,
                    &ops,
                    &patcher,
                    &memoria,
                    &base,
                    &val,
                    &CfgConsolidacao::default(),
                );
                println!(
                    "  consolidado: {} episodios | intencao {:.1}% -> {:.1}%",
                    rel.episodios_usados,
                    rel.antes.acuracia_intencao() * 100.0,
                    rel.depois.acuracia_intencao() * 100.0
                );
            }

            let rel = treinar_por_reforco(
                &mut ag,
                &ops,
                &patcher,
                &memoria,
                &base,
                &val,
                &CfgReforco::default(),
            );
            if rel.transicoes == 0 {
                println!("  nada pra reforcar — nenhum episodio com desfecho ainda.");
            } else {
                println!(
                    "  reforcado: {} transicoes ({} uteis) | recompensa media {:.3} | erro do critico {:.3}",
                    rel.transicoes, rel.transicoes_uteis, rel.recompensa_media, rel.erro_critico
                );
                if rel.erro_critico > 0.5 {
                    println!("  ATENCAO: o critico esta divergindo. A vantagem vira ruido —");
                    println!("  vale reduzir a taxa antes de continuar.");
                }
                println!(
                    "  intencao {:.1}% -> {:.1}% | ponta-a-ponta {:.1}% -> {:.1}%",
                    rel.antes.acuracia_intencao() * 100.0,
                    rel.depois.acuracia_intencao() * 100.0,
                    rel.antes.acuracia_total() * 100.0,
                    rel.depois.acuracia_total() * 100.0
                );
            }
            if let Some(caminho) = &saida {
                match ag.salvar(caminho) {
                    Ok(_) => println!("  agente salvo em {}", caminho.display()),
                    Err(e) => eprintln!("  falha ao salvar: {e}"),
                }
            }
            continue;
        }
        if pedido == "/buscar" {
            // A opcao 1 do desenho: ela NUNCA busca sozinha.
            //
            // A Teka roda 100% local; buscar manda o texto do pedido para fora da
            // maquina. Automatizar isso na duvida trocaria uma propriedade do
            // projeto por conveniencia, sem o dono decidir. Entao a abstencao
            // PROPOE e este comando executa — e so o que ela nao entendeu, nunca
            // texto arbitrario.
            let Some((consulta, _)) = licao_pendente.clone() else {
                println!("  nao ha pedido pendente para procurar.");
                continue;
            };
            let Some(fi) = ag.registro.indice("buscar_web") else {
                println!("  buscar_web nao esta no registro.");
                continue;
            };
            let chamada = teka::tools::Chamada {
                ferramenta: fi,
                args: vec![("consulta".to_string(), consulta.clone())],
            };
            match exec.executar(&ag.registro, &chamada) {
                Ok(texto) => {
                    println!("  → {texto}");
                    // Guarda o que achou. A procedencia fica gravada como
                    // `Ferramenta("buscar_web")`, e nao como `Usuario`: o peso da
                    // fonte e o que impede a memoria de tratar o que a web disse com
                    // a mesma confianca do que voce disse.
                    let agora = agora_em_dias();
                    let i = semantica.gravar(&texto, agora);
                    semantica.fatos[i].fonte =
                        teka::memory::semantica::Fonte::Ferramenta("buscar_web".into());
                    // Indexa pela assinatura do PEDIDO, nao do resultado: o que vai
                    // ser perguntado de novo e a pergunta, e e por ela que o
                    // `lembrar` procura.
                    let _ = ag.responder(&ops, &patcher, &consulta, &mut cache);
                    let assinatura = ag.assinatura(&cache);
                    semantica.indexar(i, assinatura);
                    println!("    [guardei] pergunte de novo e eu respondo sem buscar.");
                }
                Err(e) => println!("  nao consegui buscar: {e}"),
            }
            continue;
        }

        if pedido == "/memoria" {
            let com_desfecho = memoria
                .episodios
                .iter()
                .filter(|e| e.resultado != Resultado::NaoTentou)
                .count();
            println!(
                "  {} episodios | {} com correcao/aprovacao | {} com desfecho | explorando: {}",
                memoria.len(),
                memoria.n_ensinaveis(),
                com_desfecho,
                if explorando { "sim" } else { "nao" }
            );
            continue;
        }
        if pedido == "/consolidar" {
            if memoria.n_ensinaveis() == 0 {
                println!("  nada pra consolidar — use /certo ou /errado antes.");
                continue;
            }
            let colapsados = memoria.destilar(0.995);
            if colapsados > 0 {
                println!("  destilacao colapsou {colapsados} episodios repetidos");
            }
            let mut r = Rng::new(7);
            let exs = gerar(&ag.registro, &patcher, args.exemplos, &mut r);
            let (base, val) = dividir_por_frase(exs, 4);
            let rel = consolidar(
                &mut ag,
                &ops,
                &patcher,
                &memoria,
                &base,
                &val,
                &CfgConsolidacao::default(),
            );
            println!(
                "  consolidado: {} episodios | intencao {:.1}% -> {:.1}% | ponta-a-ponta {:.1}% -> {:.1}%",
                rel.episodios_usados,
                rel.antes.acuracia_intencao() * 100.0,
                rel.depois.acuracia_intencao() * 100.0,
                rel.antes.acuracia_total() * 100.0,
                rel.depois.acuracia_total() * 100.0
            );
            if let Some(caminho) = &saida {
                match ag.salvar(caminho) {
                    Ok(_) => println!("  agente salvo em {}", caminho.display()),
                    Err(e) => eprintln!("  falha ao salvar: {e}"),
                }
            }
            continue;
        }

        let t = if explorando {
            recozimento.temperatura(memoria.tentativas(pedido))
        } else {
            0.0
        };
        responder(
            &ag, &ops, &patcher, &mut exec, pedido, t, &mut rng_repl, &mut memoria, &mut cache,
            &mut contexto,
            &mut licao_pendente,
            &mut semantica,
            agora_em_dias(),
        );
    }

    if args.sem_memoria {
        return;
    }
    if !semantica.fatos.is_empty() {
        match semantica.salvar(&caminho_semantica) {
            Ok(n) => println!("\n  sabe: {n} fatos em {}", caminho_semantica.display()),
            Err(e) => eprintln!("\n  falha ao salvar o que ela sabe: {e}"),
        }
    }
    match memoria.salvar(&caminho_memoria) {
        Ok(n) => println!("\n  memoria salva: {n} episodios em {}", caminho_memoria.display()),
        Err(e) => eprintln!("\n  falha ao salvar a memoria: {e}"),
    }
}

/// Gera propostas de exemplos com o LLM local e escreve em quarentena.
fn rodar_gerador(args: &Args) {
    let reg = Registro::padrao();
    let corpus = std::fs::read_to_string("dados/exemplos_teka.txt").unwrap_or_default();
    let bench = std::fs::read_to_string("dados/frases_teste.txt").unwrap_or_default();
    let quarentena = std::fs::read_to_string("dados/propostas.txt").unwrap_or_default();
    let anteriores: Vec<String> = teka::gerador::ler_quarentena(&quarentena)
        .into_iter()
        .map(|(_, linha)| linha)
        .collect();
    if !anteriores.is_empty() {
        println!(
            "  {} propostas ja em quarentena — serao preservadas",
            anteriores.len()
        );
    }
    let mut rng = Rng::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(7),
    );

    println!(
        "\n  gerador — {}:{} modelo {}\n",
        args.ger.host, args.ger.porta, args.ger.modelo
    );
    let (propostas, c) =
        match gerar_propostas(&args.ger, &reg, &corpus, &bench, &quarentena, &mut rng) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("\n  falhou: {e}");
                eprintln!("  o servidor do LM Studio esta no ar? o modelo esta carregado?");
                std::process::exit(1);
            }
        };

    println!("\n  pedidas ............ {}", c.pedidas);
    println!("  recebidas .......... {}", c.recebidas);
    println!("  fora de idioma ..... {}", c.fora_de_idioma);
    println!("  fora de forma ...... {}", c.fora_de_forma);
    println!("  duplicadas ......... {}", c.duplicadas);
    println!("  sem argumento ...... {}", c.sem_argumento);
    println!("  fora de sentido .... {}", c.fora_de_sentido);
    println!("  ACEITAS ............ {}", c.aceitas);
    if c.recebidas > 0 {
        println!(
            "  taxa de aproveitamento: {:.0}%",
            100.0 * c.aceitas as f64 / c.recebidas as f64
        );
    }

    let caminho = std::path::Path::new("dados/propostas.txt");
    match std::fs::write(caminho, formatar(&propostas, &anteriores)) {
        Ok(_) => {
            println!(
                "\n  escrito em {} — {} linhas ({} novas). REVISE antes de aprovar.",
                caminho.display(),
                anteriores.len() + propostas.len(),
                propostas.len()
            );
            println!("  Filtro automatico nao pega erro de SENTIDO. Apague as linhas");
            println!("  ruins e anexe o resto a dados/exemplos_teka.txt.");
        }
        Err(e) => eprintln!("  falha ao escrever: {e}"),
    }
}

/// Resolve `--preset`, recusando o que não reconhece.
///
/// Isto era um `_ => Config::pequeno()`. O fall-through silencioso fez com que
/// `pequeno` — criado só para iterar rápido enquanto se depurava o treino — virasse
/// o cérebro de produção sem ninguém decidir isso: 1.515.904 parâmetros no lugar dos
/// 11.640.832 que a fase 1 projetou. Um erro de digitação em `--preset` levava ao
/// modelo de depuração sem nem um aviso.
fn resolver_preset(nome: &str) -> Config {
    match nome {
        "pequeno" => Config::pequeno(),
        "padrao" => Config::padrao(),
        outro => {
            eprintln!("  preset desconhecido: {outro:?}");
            eprintln!("  use  --preset pequeno   (1,5M params, rapido de treinar)");
            eprintln!("  ou   --preset padrao    (11,6M params, o alvo da fase 1)");
            std::process::exit(1);
        }
    }
}

/// Coleta corpus de portugues moderno. **Ferramenta offline.**
/// Monta o patcher escolhido na linha de comando.
///
/// A escolha e em tempo de EXECUCAO de proposito: a comparacao entre `por_palavra` e
/// `por_entropia` tem de ser a mesma execucao com uma flag trocada. Recompilar entre
/// os bracos meteria o compilador dentro do experimento — e o projeto ja tem historico
/// de efeito aparente que era so variacao entre execucoes.
///
/// `entropia` sem `--ngrama` FALHA em vez de cair calado no `por_palavra`. Cair calado
/// produziria uma medicao que parece testar entropia e nao testa, que e o pior tipo de
/// resultado: parece dado e e ruido.
fn montar_patcher(args: &Args) -> Box<dyn Patcher> {
    if args.patcher != "entropia" {
        return Box::new(PorPalavra::default());
    }
    let Some(caminho) = &args.ngrama else {
        eprintln!("  --patcher entropia exige --ngrama <arquivo.bin>");
        std::process::exit(1);
    };
    match teka::model::ngrama::NGrama::carregar(caminho) {
        Ok(ng) => {
            println!(
                "  patcher por entropia: ordem {}, {} caselas, limiar {:.1} bits",
                ng.ordem(),
                ng.caselas(),
                args.limiar
            );
            Box::new(PorEntropia { fonte: ng, max: 8, min: 2, limiar: args.limiar })
        }
        // Faltar a tabela significa coisas opostas nos dois casos, e tratar os dois
        // igual estragaria um deles.
        //
        // PEDIDO explicitamente: falha. Cair calado no `por_palavra` produziria uma
        // medicao que parece testar entropia e nao testa — o pior tipo de resultado,
        // porque parece dado e e ruido.
        //
        // PADRAO: avisa alto e segue no `por_palavra`. Uma copia recem-clonada nao
        // tem a tabela de 16 MB, e morrer na largada por causa de um arquivo gerado
        // seria hostil. O aviso e alto porque o modelo TREINADO com entropia lido com
        // `por_palavra` responde pior — quem vir isso precisa saber por que.
        Err(e) if !args.patcher_pedido => {
            eprintln!("  AVISO: sem {} ({e})", caminho.display());
            eprintln!("  seguindo com por_palavra. Um modelo treinado com entropia vai");
            eprintln!("  responder PIOR assim. Para gerar a tabela:");
            eprintln!("    teka ngrama --texto dados/corpus_misto.txt --ordem 5 \\");
            eprintln!("      --bits 22 --saida dados/ngrama.bin");
            Box::new(PorPalavra::default())
        }
        Err(e) => {
            eprintln!("  nao consegui carregar {}: {e}", caminho.display());
            std::process::exit(1);
        }
    }
}

fn rodar_coletor(args: &Args) {
    use std::io::Write;

    // As categorias sao escolhidas pelo vocabulario que a Teka precisa, nao pelo
    // que rende mais texto. O corpus atual e literatura de 1898 onde "arquivo"
    // aparece ZERO vezes — o problema nunca foi volume, foi dominio.
    let padrao = "Informática,Software,Sistemas operativos,\
                  Sistemas de ficheiros,Armazenamento de dados,\
                  Memória de computador,Interpretadores de comandos,Hardware";
    let categorias: Vec<String> = args
        .categorias
        .clone()
        .unwrap_or_else(|| padrao.replace(' ', " "))
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();

    let saida = args
        .saida
        .clone()
        .unwrap_or_else(|| PathBuf::from("dados/corpus_tec.txt"));
    // `--completo` ESCOLHE o modo, nao acrescenta uma segunda passada. Colher os
    // mesmos titulos duas vezes gravava o texto da introducao dentro do texto do
    // artigo inteiro, e com retomada por titulo nao havia como distinguir os dois.
    // Introducao e artigo inteiro agora sao duas execucoes, cada uma no seu arquivo.
    let inteiro = args.completo;

    println!("\n  coletor de corpus — dominios permitidos: {:?}", coletor::DOMINIOS);
    println!("  categorias: {}", categorias.join(", "));
    println!(
        "  por categoria: {} | sorteadas: {} | enumeradas: {}",
        args.paginas, args.aleatorio, args.todas
    );
    println!(
        "  modo: {}",
        if inteiro {
            "artigo inteiro — 1 por pedido, ~2,5s cada"
        } else {
            "introducao — lote de 20, ~1s cada"
        }
    );
    println!("  saida: {} (ACRESCENTA ao que ja existe)", saida.display());

    // 0. Retomada. Um trabalho de horas nao pode perder tudo se a rede cair no meio,
    //    e recolher o que ja esta em disco gasta banda alheia a toa. A chave e o
    //    titulo codificado, do jeito exato que `formatar` grava a procedencia.
    let ja: std::collections::HashSet<String> = std::fs::read_to_string(&saida)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.strip_prefix("# fonte: pt.wikipedia.org/wiki/")
                .map(str::to_string)
        })
        .collect();
    if !ja.is_empty() {
        println!("  retomando: {} paginas ja gravadas serao puladas", ja.len());
    }

    // 1. Descobre os titulos. Categoria da vocabulario dirigido; sorteio da largura.
    let mut titulos: Vec<String> = Vec::new();
    for c in &categorias {
        let url = fonte::url_categoria(c, args.paginas.max(10));
        match fonte::buscar_teimoso(&url, 60, 6) {
            Ok(corpo) => {
                let t = fonte::extrair_titulos(&corpo);
                println!("  {c}: {} paginas", t.len());
                titulos.extend(t);
            }
            Err(e) => eprintln!("  {c}: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(fonte::PAUSA_MS));
    }
    let mut faltam = args.aleatorio;
    while faltam > 0 {
        let n = faltam.min(100);
        match fonte::buscar_teimoso(&fonte::url_aleatorio(n), 60, 6) {
            Ok(corpo) => {
                let t = fonte::extrair_titulos(&corpo);
                println!("  sorteio: +{} paginas", t.len());
                titulos.extend(t);
            }
            Err(e) => eprintln!("  sorteio: {e}"),
        }
        faltam -= n;
        // Sorteio pede mais devagar que o resto. 500 titulos por pedido a 1s de
        // intervalo rendeu 437 recusas em 560 tentativas; 100 titulos a 3s e o que
        // sobrevive a franquia inicial.
        std::thread::sleep(std::time::Duration::from_millis(3 * fonte::PAUSA_MS));
    }

    // Enumeracao do acervo, em FAIXAS alfabeticas com um cursor cada.
    //
    // E a unica das tres fontes que nao se esgota nem se repete: categoria devolve os
    // mesmos titulos toda rodada (e a retomada os pula, entao a segunda rodada rende
    // zero) e sorteio traz stub em 60% das vezes.
    //
    // As faixas existem porque a enumeracao alfabetica crua custou uma noite inteira:
    // ela comeca nos titulos numericos, e a Wikipedia em portugues ali e dominada por
    // stub de asteroide — o cursor passou horas moendo `(38620) 2000 AQ186` e irmaos,
    // todos abaixo do filtro de 400 bytes. Vinte e seis faixas varrem o acervo sem
    // afundar num bolsao de lixo.
    if args.todas > 0 {
        let cursor_arq = std::path::PathBuf::from("dados/.cursores_faixa");
        // Formato: uma linha `faixa<TAB>cursor` por faixa. Faixa ausente comeca do
        // proprio inicio dela; faixa esgotada some do arquivo.
        let mut cursores: std::collections::HashMap<String, String> =
            std::fs::read_to_string(&cursor_arq)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.split_once('\t'))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        let mut esgotadas: std::collections::HashSet<String> = std::collections::HashSet::new();

        let mut colhidos = 0usize;
        let mut faixa_atual = 0usize;
        while colhidos < args.todas && esgotadas.len() < fonte::FAIXAS.len() {
            let (de_padrao, ate) = fonte::FAIXAS[faixa_atual % fonte::FAIXAS.len()];
            faixa_atual += 1;
            if esgotadas.contains(de_padrao) {
                continue;
            }
            let de = cursores
                .get(de_padrao)
                .cloned()
                .unwrap_or_else(|| de_padrao.to_string());

            let url = fonte::url_todas_paginas(&de, ate, fonte::POR_ENUMERACAO);
            match fonte::buscar_teimoso(&url, 60, 6) {
                Ok(corpo) => {
                    let t = fonte::extrair_titulos(&corpo);
                    // Descontar o RECEBIDO, nao o pedido. Descontar o pedido foi como
                    // um orcamento de 20.000 titulos virou 1.400 sem ninguem notar:
                    // `aplimit` para anonimo e 50, nao os 500 que a documentacao cita.
                    colhidos += t.len();
                    println!("  faixa {de_padrao} de {de:?}: +{} (total {colhidos})", t.len());
                    titulos.extend(t);
                    match fonte::extrair_continuacao(&corpo) {
                        Some(c) => {
                            cursores.insert(de_padrao.to_string(), c);
                        }
                        None => {
                            println!("  faixa {de_padrao}: esgotada");
                            esgotadas.insert(de_padrao.to_string());
                            cursores.remove(de_padrao);
                        }
                    }
                    // Grava a cada pedido, nao no fim: se cair no meio, a rodada
                    // seguinte continua daqui em vez de recomecar.
                    let despejo: String = cursores
                        .iter()
                        .map(|(k, v)| format!("{k}\t{v}\n"))
                        .collect();
                    let _ = std::fs::write(&cursor_arq, despejo);
                }
                Err(e) => {
                    eprintln!("  faixa {de_padrao}: {e}");
                    esgotadas.insert(de_padrao.to_string());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(args.pausa.max(fonte::PAUSA_MS)));
        }
    }

    titulos.sort();
    titulos.dedup();
    let achados = titulos.len();
    titulos.retain(|t| !ja.contains(&fonte::codificar(t)));
    println!(
        "\n  {achados} titulos distintos, {} novos ({} ja estavam)",
        titulos.len(),
        achados - titulos.len()
    );
    if titulos.is_empty() {
        eprintln!("  nada novo para colher — a rede esta acessivel?");
        return;
    }

    // 2. Colhe e GRAVA por grupo. Gravar a cada grupo e o que faz um trabalho de
    //    horas sobreviver a uma queda: o que ja veio esta em disco, e a execucao
    //    seguinte o pula no passo 0.
    let mut arquivo = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&saida)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("  falha ao abrir {}: {e}", saida.display());
            std::process::exit(1);
        }
    };
    const GRUPO: usize = 200;
    let grupos = (titulos.len() + GRUPO - 1) / GRUPO;
    let (mut n_paginas, mut n_bytes) = (0usize, 0usize);
    for (i, grupo) in titulos.chunks(GRUPO).enumerate() {
        let pedacos = fonte::colher(grupo, 60, !inteiro, args.pausa);
        let (aceitos, cont) = coletor::filtrar(pedacos, 400);
        let texto = coletor::formatar(&aceitos);
        if let Err(e) = arquivo
            .write_all(texto.as_bytes())
            .and_then(|_| arquivo.flush())
        {
            eprintln!("  falha ao gravar: {e}");
            break;
        }
        n_paginas += aceitos.len();
        n_bytes += texto.len();
        println!(
            "  grupo {}/{grupos}: +{} paginas, +{} KB | acumulado {n_paginas} paginas, {:.1} MB",
            i + 1,
            aceitos.len(),
            texto.len() / 1024,
            n_bytes as f64 / 1e6
        );
        // O relatorio inteiro so no primeiro grupo: serve pra ver se o filtro esta
        // fazendo sentido, e repetido 80 vezes viraria ruido no log.
        if i == 0 {
            println!("{}", cont.relatorio());
        }
    }

    println!(
        "\n  gravado em {} — {n_paginas} paginas novas, {:.1} MB",
        saida.display(),
        n_bytes as f64 / 1e6
    );
    println!("  a procedencia de cada bloco esta no proprio arquivo (linhas #).");
    println!("\n  para juntar num corpus so:");
    println!("    cat dados/corpus_pt.txt dados/corpus_*.txt > dados/corpus_misto.txt");
}

/// Constroi o n-grama de bytes que alimenta o patcher por entropia, e mede as duas
/// coisas que decidem se ele presta: quanta posicao real ele cobre, e qual limiar
/// deixa a compressao igual a do `PorPalavra`.
fn rodar_ngrama(args: &Args) {
    use teka::model::ngrama::NGrama;
    use teka::model::patcher::{Patcher, PorEntropia, PorPalavra};

    // As linhas `# fonte:` sao procedencia, nao portugues. Contariam como texto que a
    // Teka nunca vai ver em uso e enviesariam as contagens.
    fn ler_limpo(caminho: &std::path::Path) -> Vec<u8> {
        let bruto = match std::fs::read(caminho) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("  nao consegui ler {}: {e}", caminho.display());
                std::process::exit(1);
            }
        };
        String::from_utf8_lossy(&bruto)
            .lines()
            .filter(|l| !l.starts_with("# fonte:"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
    }

    let caminho_corpus = args
        .texto_val
        .clone()
        .unwrap_or_else(|| PathBuf::from("dados/corpus_misto.txt"));
    let caminho_val = args
        .val
        .clone()
        .unwrap_or_else(|| PathBuf::from("dados/val_tec.txt"));

    let corpus = ler_limpo(&caminho_corpus);
    let validacao = ler_limpo(&caminho_val);

    println!("\n  corpus ... {} ({:.2} MB)", caminho_corpus.display(), corpus.len() as f64 / 1e6);
    println!("  ordem .... {}", args.ordem);
    let distintos = NGrama::contextos_distintos(&corpus, args.ordem);
    let caselas = 1usize << args.bits_tabela;
    println!(
        "  tabela ... 2^{} = {} caselas para {} contextos distintos ({:.2}x)",
        args.bits_tabela, caselas, distintos, distintos as f64 / caselas as f64
    );
    if distintos > caselas {
        println!("  AVISO: mais contextos que caselas — a colisao vai saturar a tabela");
    }

    let inicio = std::time::Instant::now();
    let ng = NGrama::treinar(&corpus, args.ordem, args.bits_tabela);
    println!("  construido em {:.1}s\n", inicio.elapsed().as_secs_f64());

    // A cobertura no texto de VALIDACAO e o numero que decide se o corpus e grande o
    // bastante. Medir no proprio corpus de treino responderia "decorei".
    println!("  ocupacao de tabela ..... {:.1}% sem dados", ng.fracao_sem_dados() * 100.0);
    println!(
        "  posicoes sem dados ..... {:.1}% no treino | {:.1}% em {}",
        ng.fracao_sem_dados_em(&corpus) * 100.0,
        ng.fracao_sem_dados_em(&validacao) * 100.0,
        caminho_val.display()
    );

    // Calibracao. Acrescentar fronteira encurta o patch, e o patch medio E o
    // multiplicador de velocidade do backbone — comparar acuracia com limiar baixo
    // mediria "onde as fronteiras estao" e "quantas sao" ao mesmo tempo. O limiar
    // certo e o que empata o bytes/patch com o do PorPalavra; so ai a unica variavel
    // que sobra e o LUGAR da fronteira.
    //
    // A calibracao roda em linhas do CORPUS, nunca em frases de teste: escolher
    // limiar olhando a regua e vazamento.
    let linhas: Vec<&[u8]> = corpus
        .split(|b| *b == b'\n')
        .filter(|l| l.len() > 20 && l.len() < 200)
        .take(4000)
        .collect();
    if linhas.is_empty() {
        eprintln!("  corpus sem linhas utilizaveis para calibrar");
    } else {
        let taxa = |p: &dyn Patcher| -> f64 {
            let (mut bytes, mut patches) = (0usize, 0usize);
            let mut fim = Vec::new();
            for l in &linhas {
                p.fronteiras(l, &mut fim);
                bytes += l.len();
                patches += fim.len();
            }
            bytes as f64 / patches.max(1) as f64
        };
        let base = taxa(&PorPalavra::default());
        println!("\n  calibracao em {} linhas do corpus", linhas.len());
        println!("  por_palavra .......... {base:.3} bytes/patch");
        let mut melhor = (f32::NAN, f64::INFINITY);
        for passo in 1..=15 {
            let limiar = passo as f32 * 0.5;
            let t = taxa(&PorEntropia { fonte: &ng, max: 8, min: 2, limiar });
            let erro = (t - base).abs();
            if erro < melhor.1 {
                melhor = (limiar, erro);
            }
            println!("  limiar {limiar:>4.1} bits ..... {t:.3} bytes/patch");
        }
        println!(
            "\n  limiar que mais se aproxima do por_palavra: {:.1} bits",
            melhor.0
        );
        println!("  e o que deve ser usado na comparacao de acuracia.");
    }

    if let Some(caminho) = &args.saida {
        match ng.salvar(caminho) {
            Ok(n) => println!("\n  gravado em {} ({} KB)", caminho.display(), n / 1024),
            Err(e) => eprintln!("\n  falha ao gravar: {e}"),
        }
    } else {
        println!("\n  (sem --saida: nada foi gravado)");
    }
}

/// Mede bits/byte de um cerebro num texto que ficou FORA do treino.
///
/// Existe porque comparar corpora exige uma regua independente dos dois. Medir no
/// proprio corpus de treino responde "ela decorou?"; medir no benchmark de
/// ferramentas mistura dois efeitos e nao diz qual foi.
fn rodar_medir_lm(args: &Args) {
    let Some(caminho_modelo) = &args.carregar else {
        eprintln!("  use --carregar <cerebro.bin> --texto <arquivo>");
        std::process::exit(1);
    };
    let caminho_texto = args
        .texto_val
        .clone()
        .unwrap_or_else(|| PathBuf::from("dados/val_tec.txt"));

    let modelo = match Teka::<f32>::carregar(caminho_modelo) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("  nao consegui carregar {}: {e}", caminho_modelo.display());
            std::process::exit(1);
        }
    };
    let texto = match std::fs::read(&caminho_texto) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  nao consegui ler {}: {e}", caminho_texto.display());
            std::process::exit(1);
        }
    };
    // As linhas `# fonte:` sao procedencia, nao portugues: contariam como texto
    // que o modelo nunca vai ver em uso e sujariam a metrica.
    let limpo: Vec<u8> = String::from_utf8_lossy(&texto)
        .lines()
        .filter(|l| !l.starts_with("# fonte:"))
        .collect::<Vec<_>>()
        .join("
")
        .into_bytes();

    let ops = Paralelo::new(args.threads);
    let patcher_dono = montar_patcher(args);
    let patcher: &dyn Patcher = &*patcher_dono;
    let mut cache = TekaCache::new();
    let bpb = teka::learn::train::avaliar(&modelo, &ops, &patcher, &limpo, args.sup.seq, &mut cache);

    println!("
  modelo ... {}", caminho_modelo.display());
    println!("  texto .... {} ({:.2} MB)", caminho_texto.display(), limpo.len() as f64 / 1e6);
    println!("  params ... {}", modelo.n_params());
    println!("
  bits/byte: {bpb:.4}   (menor e melhor)");
}

/// Converte um agente para int8 por bloco e MEDE o que se perdeu.
///
/// Medir e a parte que importa. Quantizacao que nao muda o benchmark e ganho puro;
/// quantizacao que muda e uma troca, e a troca tem de estar na tela em vez de virar
/// uma surpresa tres semanas depois.
fn rodar_quantizar(args: &Args) {
    let Some(entrada) = &args.carregar else {
        eprintln!("  use: teka quantizar --carregar <agente.bin> --saida <agente_q8.bin>");
        std::process::exit(1);
    };
    let saida = args
        .saida
        .clone()
        .unwrap_or_else(|| PathBuf::from("agente_q8.bin"));

    let mut ag = match Agente::<f32>::carregar(entrada, Registro::padrao()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("  nao consegui carregar {}: {e}", entrada.display());
            std::process::exit(1);
        }
    };
    let originais: Vec<Vec<f32>> = ag.params_mut().iter().map(|t| t.to_vec()).collect();

    let antes = std::fs::metadata(entrada).map(|m| m.len()).unwrap_or(0);
    let n = match ag.salvar_q8(&saida) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("  falha ao salvar: {e}");
            std::process::exit(1);
        }
    };
    println!("\n  {} -> {}", entrada.display(), saida.display());
    println!(
        "  {:.2} MB -> {:.2} MB   ({:.2}x menor)",
        antes as f64 / 1e6,
        n as f64 / 1e6,
        antes as f64 / n.max(1) as f64
    );

    // O arquivo tem de recarregar, e o erro tem de ser medido tensor a tensor.
    let mut recarregado = match Agente::<f32>::carregar(&saida, Registro::padrao()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("  o arquivo salvo NAO recarrega: {e}");
            std::process::exit(1);
        }
    };
    let (mut pior, mut soma) = (0.0f64, 0.0f64);
    let novos = recarregado.params_mut();
    for (o, x) in originais.iter().zip(novos.iter()) {
        let e = teka::nn::quant::erro_relativo(o, x);
        if e > pior {
            pior = e;
        }
        soma += e;
    }
    println!(
        "  erro relativo: medio {:.4}, pior tensor {:.4}",
        soma / originais.len().max(1) as f64,
        pior
    );
    println!("\n  agora compare de verdade:");
    println!("    teka agente --carregar {} --benchmark", saida.display());
}

/// Pratica num mundo isolado e aprende do resultado.
///
/// O benchmark e medido ANTES e DEPOIS, e nao entra na recompensa. Um modelo que
/// sobe no ambiente e cai no benchmark decorou o ambiente — e essa suspeita tem de
/// ser visivel, nao silenciosa.
fn rodar_pratica(args: &Args) {
    let Some(entrada) = &args.carregar else {
        eprintln!("  use: teka praticar --carregar <agente.bin> --saida <treinado.bin>");
        std::process::exit(1);
    };
    let mut ag = match Agente::<f32>::carregar(entrada, Registro::padrao()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("  nao consegui carregar {}: {e}", entrada.display());
            std::process::exit(1);
        }
    };
    let ops = Paralelo::new(args.threads);
    let patcher_dono = montar_patcher(args);
    let patcher: &dyn Patcher = &*patcher_dono;

    // Base do replay supervisionado e validacao: as mesmas do treino normal.
    let mut r = Rng::new(4242);
    let exs = gerar(&ag.registro, &patcher, args.exemplos.min(8000), &mut r);
    let (base, validacao) = dividir_por_frase(exs, 4);

    let caminho_mem = PathBuf::from("memoria_pratica.bin");
    let mut mem = MemoriaEpisodica::carregar(&caminho_mem).unwrap_or_else(|_| MemoriaEpisodica::nova());

    let cfg = teka::ambiente::laco::CfgLaco {
        tentativas_por_rodada: args.tentativas,
        rodadas: args.rodadas,
        // A semente do sorteio de tarefas. Varia-la e o que permite medir se o
        // ganho e do treino ou do acaso — a licao mais cara desta sessao.
        semente: args.semente.wrapping_mul(2_654_435_761),
        ..Default::default()
    };
    let raiz = std::env::temp_dir().join("teka-mundo");

    println!("\n  PRATICA — mundo isolado em {}", raiz.display());
    println!("  {} rodadas x {} tentativas", cfg.rodadas, cfg.tentativas_por_rodada);
    let antes = medir_benchmark(&ag, &ops, &patcher);
    println!("  benchmark ANTES: {}/{} ({:.0}%)\n", antes.0, antes.1, 100.0 * antes.0 as f64 / antes.1 as f64);

    let rodadas = match teka::ambiente::laco::rodar(
        &mut ag, &ops, &patcher, &raiz, &mut mem, &base, &validacao, &cfg,
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  a pratica falhou: {e}");
            std::process::exit(1);
        }
    };

    let depois = medir_benchmark(&ag, &ops, &patcher);
    println!("\n  ambiente: {:.0}% -> {:.0}%",
        rodadas.first().map(|r| r.taxa() * 100.0).unwrap_or(f64::NAN),
        rodadas.last().map(|r| r.taxa() * 100.0).unwrap_or(f64::NAN));
    println!("  benchmark: {}/{} -> {}/{}", antes.0, antes.1, depois.0, depois.1);
    if depois.0 < antes.0 {
        println!("\n  ATENCAO: o benchmark CAIU. Ela pode ter decorado o ambiente.");
        println!("  O modelo so sera salvo se voce passar --saida explicitamente.");
    }

    let _ = mem.salvar(&caminho_mem);
    if let Some(saida) = &args.saida {
        match ag.salvar(saida) {
            Ok(n) => println!("\n  salvo em {} ({:.1} MB)", saida.display(), n as f64 / 1e6),
            Err(e) => eprintln!("  falha ao salvar: {e}"),
        }
    } else {
        println!("\n  (nao salvo: passe --saida para guardar)");
    }
}

/// Acertos de ferramenta no benchmark. Separado para poder medir antes e depois.
fn medir_benchmark(ag: &Agente<f32>, ops: &Paralelo, patcher: &dyn Patcher) -> (usize, usize) {
    let casos = teka::learn::dados::ler_casos_teste(include_str!("../dados/frases_teste.txt"));
    let mut c = AgenteCache::new();
    let mut ok = 0usize;
    for caso in &casos {
        if let Ok(ch) = ag.responder(ops, patcher, &caso.pedido, &mut c) {
            if ag.registro.ferramentas[ch.ferramenta].nome == caso.ferramenta {
                ok += 1;
            }
        }
    }
    (ok, casos.len())
}

/// O pulso: ela continua rodando quando ninguem esta pedindo nada.
///
/// Ver `teka::pulso` para o desenho. O resumo do que importa aqui: PENSAR nao muda
/// peso, CONSOLIDAR muda e so acontece sobre memoria REAL — episodio que aconteceu,
/// correcao que voce deu.
///
/// Grava os pesos **logo depois de cada consolidacao**, e nao no fim. Um laco que so
/// salva ao sair perde tudo no primeiro Ctrl-C, e este e feito para ficar de pe.
fn rodar_pulso(args: &Args) {
    use teka::pulso::{escrever_pulso, CfgPulso, Pulso};

    let ops = Paralelo::new(args.threads);
    let patcher_dono = montar_patcher(args);
    let patcher: &dyn Patcher = &*patcher_dono;
    let registro = Registro::padrao();

    let Some(caminho) = args.carregar.clone().or_else(|| args.saida.clone()) else {
        eprintln!("  use: teka pulsar --carregar <agente.bin>");
        eprintln!("  o pulso consolida a memoria NOS PESOS, entao precisa saber onde grava-los");
        std::process::exit(1);
    };
    let mut ag = match Agente::<f32>::carregar(&caminho, registro.clone()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("  falha ao carregar {}: {e}", caminho.display());
            std::process::exit(1);
        }
    };
    println!("  agente carregado de {} ({} params)", caminho.display(), ag.n_params());

    let caminho_memoria = PathBuf::from("memoria.bin");
    let memoria =
        MemoriaEpisodica::carregar(&caminho_memoria).unwrap_or_else(|_| MemoriaEpisodica::nova());

    // A memoria semantica ainda nao e populada por nada no CLI. Entra vazia de
    // proposito, e o contador "esquecendo" vai ler 0 — e honesto que leia, em vez de
    // inventar numero.
    let semantica = teka::memory::semantica::MemoriaSemantica::nova(0);

    // A base de replay: e o que segura o esquecimento quando a consolidacao puxa os
    // pesos na direcao dos episodios. Sem ela, consolidar 8 episodios sobrescreve o
    // que ela sabia.
    let mut r = Rng::new(7);
    let exs = gerar(&ag.registro, &patcher, args.exemplos, &mut r);
    let (base, validacao) = dividir_por_frase(exs, 4);

    let cfg = CfgPulso {
        arquivo: PathBuf::from("pulso.json"),
        ..args.pulso.clone()
    };
    println!(
        "
  PULSO — tick a cada {}s, consolida a cada {} ticks ou com {}+ episodios novos",
        cfg.intervalo_s, cfg.consolidar_a_cada, cfg.gatilho_episodios
    );
    println!("  memoria: {} episodios ({} ensinaveis)", memoria.len(), memoria.n_ensinaveis());
    if memoria.n_ensinaveis() == 0 {
        println!("  (nada corrigido ainda — o laco vai pensar e nao vai ter o que aprender)");
    }
    println!("  estado a cada tick em {}", cfg.arquivo.display());
    println!("  Ctrl-C para parar. Os pesos sao gravados apos cada consolidacao.
");

    let mut pulso = Pulso::novo(args.semente);
    let t0 = std::time::SystemTime::now();
    loop {
        // Reler a cada tick: quem esta conversando com ela num outro terminal grava
        // episodio novo, e o pulso tem de enxergar isso. E o "perceber" do laco.
        let memoria = MemoriaEpisodica::carregar(&caminho_memoria).unwrap_or_else(|_| memoria.clone());
        let dias = t0
            .elapsed()
            .map(|d| d.as_secs_f64() / 86_400.0)
            .unwrap_or(0.0);

        let rel = pulso.tick(
            &mut ag, &ops, &patcher, &memoria, &semantica, dias, &base, &validacao, &cfg,
        );
        if let Err(e) = escrever_pulso(&cfg.arquivo, &rel) {
            eprintln!("  (nao consegui escrever {}: {e})", cfg.arquivo.display());
        }

        let marca = chrono_simples();
        if rel.consolidou {
            match ag.salvar(&caminho) {
                Ok(_) => println!(
                    "  [{marca}] tick {} — consolidou {} passos, intencao {:.1}% -> {:.1}%, pesos salvos",
                    rel.tick,
                    rel.passos,
                    rel.antes * 100.0,
                    rel.depois * 100.0
                ),
                Err(e) => eprintln!("  [{marca}] tick {} — consolidou mas NAO SALVOU: {e}", rel.tick),
            }
        } else if rel.tentou {
            // Era hora de consolidar e nao havia o que aprender. Nao e o mesmo que
            // um tick tranquilo, e o log nao pode fazer parecer que e.
            println!(
                "  [{marca}] tick {} — era hora de consolidar e nao ha nada ensinavel \
                 ({} episodios, 0 com /certo ou /errado)",
                rel.tick,
                rel.ensinaveis + rel.episodios_novos.min(0),
            );
        } else {
            println!(
                "  [{marca}] tick {} — pensou {} bytes, {} episodio(s) novo(s), {:.1}s",
                rel.tick,
                rel.pensamento.len(),
                rel.episodios_novos,
                rel.segundos
            );
        }

        if args.ticks > 0 && pulso.ticks() >= args.ticks {
            println!("
  {} ticks, como pedido. Fim.", args.ticks);
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(cfg.intervalo_s));
    }
}

/// HH:MM:SS sem dependencia. So para o log do pulso ter hora.
fn chrono_simples() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let d = s % 86_400;
    format!("{:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60)
}

/// Exporta os pesos em safetensors, para inspecionar fora do Rust.
///
/// O `.bin` nativo guarda comprimento e mais nada; safetensors carrega nome, forma e
/// tipo, que e o que qualquer outra ferramenta precisa para abrir.
///
/// Vale dizer o que isto NAO da: safetensors e um conteiner, nao uma definicao de
/// modelo. Quem abrir ve os tensores e nao consegue EXECUTAR a Teka a partir dele —
/// nao existe do outro lado um RG-LRU hierarquico byte a byte esperando esses pesos.
/// Serve para inspecionar, comparar e plotar. Nao serve para rodar.
fn rodar_exportacao(args: &Args) {
    let registro = Registro::padrao();
    let Some(entrada) = args.carregar.clone() else {
        eprintln!("  use: teka exportar --carregar <agente.bin> [--saida <arquivo.safetensors>]");
        std::process::exit(1);
    };
    let mut ag = match Agente::<f32>::carregar(&entrada, registro) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("  falha ao carregar {}: {e}", entrada.display());
            std::process::exit(1);
        }
    };
    let saida = args
        .saida
        .clone()
        .unwrap_or_else(|| entrada.with_extension("safetensors"));

    let desc = ag.descritores();
    let pesos = ag.params_mut();
    // A mesma invariante do teste `os_descritores_batem_com_os_pesos`, cobrada aqui
    // tambem: exportar com as listas fora de sincronia produziria um arquivo em que
    // TODO tensor carrega o nome do vizinho, e nada avisaria.
    if desc.len() != pesos.len() {
        eprintln!(
            "  descritores ({}) e pesos ({}) nao batem — nao vou escrever arquivo torto",
            desc.len(),
            pesos.len()
        );
        std::process::exit(1);
    }

    let mut tensores: Vec<(String, Vec<usize>, Vec<f32>)> = Vec::with_capacity(desc.len());
    let mut total = 0usize;
    for ((nome, forma), peso) in desc.into_iter().zip(pesos.iter()) {
        let n: usize = forma.iter().product();
        if n != peso.len() {
            eprintln!("  {nome}: forma {forma:?} da {n}, peso tem {}", peso.len());
            std::process::exit(1);
        }
        total += n;
        tensores.push((nome, forma, peso.to_vec()));
    }

    let meta = vec![
        ("formato", "teka".to_string()),
        ("arquitetura", "SSM hierarquico byte a byte (RG-LRU), patcher por palavra".to_string()),
        ("parametros", total.to_string()),
        ("tensores", tensores.len().to_string()),
        ("origem", entrada.display().to_string()),
        (
            "aviso",
            "conteiner de pesos: nao ha definicao de modelo aqui, nao da para executar".to_string(),
        ),
    ];
    match teka::model::safetensors::escrever(&saida, &tensores, &meta) {
        Ok(bytes) => {
            println!("  {} tensores, {total} parametros", tensores.len());
            println!("  {} ({:.1} MB)", saida.display(), bytes as f64 / 1_048_576.0);
            println!("
  os cinco primeiros:");
            for (nome, forma, _) in tensores.iter().take(5) {
                println!("    {nome:<28} {forma:?}");
            }
        }
        Err(e) => {
            eprintln!("  falha ao escrever {}: {e}", saida.display());
            std::process::exit(1);
        }
    }
}

fn main() {
    let args = parse_args();

    if args.modo == "gerar" {
        rodar_gerador(&args);
        return;
    }

    if args.modo == "coletar" {
        rodar_coletor(&args);
        return;
    }

    if args.modo == "medir-lm" {
        rodar_medir_lm(&args);
        return;
    }

    if args.modo == "ngrama" {
        rodar_ngrama(&args);
        return;
    }

    if args.modo == "quantizar" {
        rodar_quantizar(&args);
        return;
    }

    if args.modo == "exportar" {
        rodar_exportacao(&args);
        return;
    }
    if args.modo == "pulsar" {
        rodar_pulso(&args);
        return;
    }
    if args.modo == "praticar" {
        rodar_pratica(&args);
        return;
    }

    if args.modo == "agente" {
        rodar_agente(&args);
        return;
    }

    if args.modo == "bench" {
        println!("┌───────────────────────────────────────────────────────────────┐");
        println!("│  TEKA — fase 1: modelo hierarquico byte a byte                 │");
        println!("└───────────────────────────────────────────────────────────────┘");
        println!("  {} threads", args.threads);
        bench_gemm(args.threads);
        bench_rglru(args.threads);
        bench_patcher();
        bench_modelo(args.threads);
        println!("\n  Treinar:   cargo run --release -- treinar --minutos 30");
        println!("  Corretude: cargo test --release -- --nocapture\n");
        return;
    }

    let corpus = match std::fs::read(&args.corpus) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("nao consegui ler {}: {e}", args.corpus.display());
            std::process::exit(1);
        }
    };
    // Últimos 100 KB ficam de fora do treino, para validação honesta.
    let corte = corpus.len().saturating_sub(100_000);
    let (treino, validacao) = corpus.split_at(corte);

    let cfg_modelo = resolver_preset(&args.preset);
    let mut rng = Rng::new(7);
    let mut modelo = Teka::<f32>::new(cfg_modelo, &mut rng);
    let ops = Paralelo::new(args.threads);
    let patcher_dono = montar_patcher(&args);
    let patcher: &dyn Patcher = &*patcher_dono;

    println!("\n  TEKA — treino (preset {}, {} threads)\n", args.preset, args.threads);
    let rel = treinar(
        &mut modelo,
        &ops,
        &patcher,
        treino.to_vec(),
        &args.cfg,
        validacao,
        args.saida.as_deref(),
    );
    println!(
        "\n  {} passos em {:.0}s — {:.4} bits/byte, {:.2} bytes/patch, {:.0} bytes/s",
        rel.passos, rel.segundos, rel.bits_por_byte, rel.bytes_por_patch, rel.bytes_por_s
    );
}

#[cfg(test)]
mod testes_flags {
    /// Nenhuma flag pode aparecer duas vezes no `match` de argumentos.
    ///
    /// Ja aconteceu: `--rodadas` existia para o gerador e para a pratica. O
    /// primeiro braco vencia, entao `--rodadas 8` virava 10 em silencio. Erro que
    /// nao da erro e ainda mente no relatorio.
    #[test]
    fn nao_ha_flag_duplicada() {
        let fonte = include_str!("main.rs");
        let mut vistas: Vec<&str> = Vec::new();
        let mut repetidas: Vec<&str> = Vec::new();
        for linha in fonte.lines() {
            let t = linha.trim();
            if !t.starts_with("\"--") {
                continue;
            }
            let Some(fim) = t[1..].find('"') else { continue };
            let flag = &t[1..1 + fim];
            if vistas.contains(&flag) {
                repetidas.push(flag);
            } else {
                vistas.push(flag);
            }
        }
        assert!(repetidas.is_empty(), "flags duplicadas: {repetidas:?}");
        assert!(vistas.len() > 15, "so achei {} flags", vistas.len());
    }
}
