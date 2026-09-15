//! Quanto ela aguenta? Vazamento, vazao, tamanho do pedido, threads e limites.
//!
//! A Teka e feita para ficar ligada o dia inteiro. Uma medida anterior apontou
//! **RAM subindo sem teto: 35 MB -> 120 MB em 250 s, ~966 bytes por pedido**. Esta
//! sonda existe para dizer se isso e real, de onde vem, e o que mais quebra quando
//! o uso e longo.
//!
//! ## FALSIFICACAO, dita antes de rodar
//!
//! **Tese 1 — o vazamento e a memoria episodica, e nada mais.**
//! Cada turno faz um `mem.gravar(...)` com a assinatura do backbone (`d_bb` f32) e
//! NADA poda a lista fora do `/consolidar` manual.
//! - Falsifica se o braco SEM memoria (so `Agente::responder`) tambem crescer
//!   linearmente depois das primeiras centenas de pedidos: ai o vazamento e do
//!   modelo/cache e a episodica e so um agravante.
//! - Falsifica se a contabilidade exata da episodica ficar MUITO abaixo do RSS
//!   medido (digamos, menos de metade): ai ha um segundo vazador nao identificado.
//! - Confirma se contabilidade exata ~ RSS medido e o braco sem memoria estabilizar.
//!
//! **Tese 2 — a vazao NAO degrada com o tempo.**
//! O caminho quente e O(1) na memoria: `gravar` e um `push`, e nada no turno
//! normal varre `episodios`. `destilar` (O(n^2)) so roda em `/consolidar`.
//! - Falsifica se a ultima janela de uma corrida longa ficar visivelmente mais
//!   lenta que a primeira COM memoria e nao sem ela.
//!
//! **Tese 3 — a latencia e ~linear no tamanho do pedido.**
//! O backbone e um SSM recorrente por patch, sem atencao quadratica. Entao dobrar o
//! pedido deve dobrar o custo, nao quadruplicar.
//! - Falsifica se ms/byte subir com o tamanho (indicio de custo quadratico em algum
//!   lugar: `Plano::novo`, o recorte, a gramatica).
//!
//! **Tese 4 — nao ha limite de entrada declarado.**
//! `decidir` so rejeita `seq == 0`. Um pedido de 1 MB deve ser ACEITO e caro, nao
//! recusado. Falsifica se der erro explicito de tamanho.
//!
//! ## ATENCAO — contaminacao
//!
//! Os modos `tamanho`, `threads` e `folego` medem TEMPO. Se houver treino rodando,
//! todo numero sai contaminado. Os modos `contabilidade` e `limites` NAO dependem de
//! tempo: contabilidade conta bytes de estrutura, limites olha aceita/recusa.
//!
//! ## Uso
//!
//! ```text
//! sonda_carga contabilidade <modelo> [n_pedidos]   bytes exatos + RSS, com e sem memoria
//! sonda_carga limites       <modelo>               vazio, so espaco, 1 MB, bytes crus
//! sonda_carga tamanho       <modelo>               latencia x tamanho (TEMPO)
//! sonda_carga threads       <modelo>               1,2,4,8 threads (TEMPO)
//! sonda_carga folego        <modelo> [segundos]    vazao por janela (TEMPO)
//! ```
use std::time::Instant;
use teka::backend::Paralelo;
use teka::memory::{Feedback, MemoriaEpisodica, Resultado};
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

// ---------------------------------------------------------------------------
// RSS de verdade, sem dependencia.
//
// `K32GetProcessMemoryInfo` mora na kernel32, que o alvo MSVC ja linka. Usar a
// psapi.dll exigiria `#[link]`, e o projeto nao quer dependencia nenhuma.
//
// Working set e ruidoso (o Windows apara paginas quando quer), por isso ele NAO e a
// medida principal — e a testemunha. A medida principal e a contabilidade exata.
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod rss {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Contadores {
        cb: u32,
        faltas_de_pagina: u32,
        pico_working_set: usize,
        working_set: usize,
        pico_paged_pool: usize,
        paged_pool: usize,
        pico_nonpaged_pool: usize,
        nonpaged_pool: usize,
        pagefile: usize,
        pico_pagefile: usize,
    }

    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(processo: isize, c: *mut Contadores, cb: u32) -> i32;
    }

    /// Bytes do working set agora, ou 0 se a chamada falhar.
    pub fn bytes() -> usize {
        let mut c = Contadores {
            cb: std::mem::size_of::<Contadores>() as u32,
            ..Default::default()
        };
        unsafe {
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
                c.working_set
            } else {
                0
            }
        }
    }
}

#[cfg(not(windows))]
mod rss {
    pub fn bytes() -> usize {
        0
    }
}

// ---------------------------------------------------------------------------
// Contabilidade exata da memoria episodica.
//
// Nao estima: soma o que a estrutura de fato segura. Duas leituras de proposito:
//
//   por len         o que os dados valem
//   por capacity    o que o `Vec` reservou (ele dobra, entao chega a 2x o de cima)
//
// O que ela NAO cobre e o cabecalho que o alocador poe em cada bloco (~16-32 B por
// alocacao no heap do Windows, e sao 2 alocacoes por episodio). Se o RSS ficar
// ACIMA da contabilidade por uma margem dessa ordem, esta explicado; se ficar muito
// acima, ha outro vazador.
// ---------------------------------------------------------------------------
fn bytes_da_episodica(m: &MemoriaEpisodica) -> (usize, usize) {
    let tam_ep = std::mem::size_of::<teka::memory::Episodio>();
    let mut heap_len = 0usize;
    let mut heap_cap = 0usize;
    for e in &m.episodios {
        heap_len += e.pedido.len() + e.assinatura.len() * 4 + e.args.len() * 24;
        heap_cap += e.pedido.capacity() + e.assinatura.capacity() * 4 + e.args.capacity() * 24;
    }
    let por_len = tam_ep * m.episodios.len() + heap_len;
    let por_cap = tam_ep * m.episodios.capacity() + heap_cap;
    (por_len, por_cap)
}

/// Pedidos variados e realistas. O tamanho do pedido entra na conta do vazamento
/// (a `String` fica guardada inteira), entao usar so "oi" subestimaria.
const PEDIDOS: &[&str] = &[
    "que horas sao",
    "abre o discord",
    "fecha o spotify pra mim",
    "lista os arquivos da pasta downloads",
    "le o arquivo notas.txt",
    "quanto espaco tem no disco",
    "escreve um lembrete no arquivo lembretes.txt dizendo comprar pao",
    "roda o comando dir na pasta de projetos",
    "procura na web quem ganhou o jogo de ontem",
    "aperta control c",
    "tira um print da tela",
    "cria uma pasta chamada teste",
    "me fala o que tem dentro de C:\\Projetos",
    "apaga o arquivo temporario velho.log",
    "qual a data de hoje",
    "poe o obs pra rodar",
];

fn carregar(caminho: &str) -> Agente<f32> {
    match Agente::<f32>::carregar(std::path::Path::new(caminho), Registro::padrao()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("  nao carreguei {caminho}: {e}");
            eprintln!("  (teka_tres_* e teka_b329_* nao carregam mais: o registro foi de 21 para 22 ferramentas)");
            std::process::exit(1);
        }
    }
}

fn banner(ag: &Agente<f32>, ops: &Paralelo) {
    println!(
        "  modelo {} params | {} ferramentas | ops com {} threads | {} CPUs logicas",
        ag.n_params(),
        ag.registro.n(),
        ops.threads,
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
    );
}

// ---------------------------------------------------------------------------
// MODO contabilidade — NAO depende de tempo.
// ---------------------------------------------------------------------------
fn modo_contabilidade(caminho: &str, n: usize) {
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let ag = carregar(caminho);
    banner(&ag, &ops);
    println!("  size_of::<Episodio>() = {} B", std::mem::size_of::<teka::memory::Episodio>());
    println!("  assinatura = d_bb f32 (o tamanho sai do modelo carregado)\n");

    // ---- BRACO A: como o REPL e o servidor fazem — grava cada turno ----
    let mut cache = AgenteCache::new();
    let mut mem = MemoriaEpisodica::nova();
    let base = rss::bytes();
    println!("  BRACO A — COM memoria episodica (o caminho real do REPL/servidor)");
    println!("  {:>9} {:>12} {:>12} {:>12} {:>12}", "pedidos", "RSS MB", "d-RSS B/ped", "exata len B", "exata/ped");
    let mut marcos: Vec<(usize, usize, usize)> = Vec::new();
    for i in 0..n {
        let pedido = PEDIDOS[i % PEDIDOS.len()];
        let ferramenta = match ag.responder(&ops, &patcher, pedido, &mut cache) {
            Ok(c) => c.ferramenta,
            Err(_) => 0,
        };
        // Exatamente o que `main.rs:690` faz a cada turno.
        let assinatura = ag.assinatura(&cache);
        mem.gravar(pedido, ferramenta, vec![], Resultado::Executou, Feedback::Nenhum, assinatura);

        let passo = (n / 10).max(1);
        if (i + 1) % passo == 0 || i + 1 == n {
            let r = rss::bytes();
            let (exata, _cap) = bytes_da_episodica(&mem);
            let feitos = i + 1;
            println!(
                "  {:>9} {:>12.1} {:>12.0} {:>12} {:>12.0}",
                feitos,
                r as f64 / 1e6,
                (r.saturating_sub(base)) as f64 / feitos as f64,
                exata,
                exata as f64 / feitos as f64,
            );
            marcos.push((feitos, r, exata));
        }
    }
    let (exata, cap) = bytes_da_episodica(&mem);
    println!("\n  final: {} episodios | exata por len {} B | por capacity {} B", mem.len(), exata, cap);
    // Inclinacao entre o primeiro e o ultimo marco: mais honesta que (fim-inicio)/n,
    // porque descarta o custo fixo de carregar o modelo.
    if marcos.len() >= 2 {
        let (n0, r0, e0) = marcos[0];
        let (n1, r1, e1) = *marcos.last().unwrap();
        if n1 > n0 {
            println!(
                "  inclinacao entre marcos: RSS {:.0} B/pedido | exata {:.0} B/pedido",
                (r1 as f64 - r0 as f64) / (n1 - n0) as f64,
                (e1 as f64 - e0 as f64) / (n1 - n0) as f64,
            );
        }
    }

    // ---- BRACO B: o controle — mesmo forward, sem gravar nada ----
    //
    // Este e o braco que FALSIFICA a tese 1. Se ele crescer igual, o culpado nao e
    // a episodica.
    println!("\n  BRACO B — SEM memoria (controle: so `responder`)");
    let mut cache_b = AgenteCache::new();
    // Aquece: a primeira dezena aloca os buffers do cache e isso nao e vazamento.
    for i in 0..50 {
        let _ = ag.responder(&ops, &patcher, PEDIDOS[i % PEDIDOS.len()], &mut cache_b);
    }
    let base_b = rss::bytes();
    println!("  {:>9} {:>12} {:>12}", "pedidos", "RSS MB", "d-RSS B/ped");
    for i in 0..n {
        let _ = ag.responder(&ops, &patcher, PEDIDOS[i % PEDIDOS.len()], &mut cache_b);
        let passo = (n / 10).max(1);
        if (i + 1) % passo == 0 || i + 1 == n {
            let r = rss::bytes();
            println!(
                "  {:>9} {:>12.1} {:>12.0}",
                i + 1,
                r as f64 / 1e6,
                (r as f64 - base_b as f64) / (i + 1) as f64,
            );
        }
    }

    // ---- O disco: `memoria.bin` e recarregado a cada abertura ----
    //
    // A RAM se resolve fechando o programa; o arquivo NAO. Ele e salvo na saida e
    // relido na entrada, entao o custo dele e permanente e volta maior a cada dia.
    let tmp = std::env::temp_dir().join("teka_sonda_carga_memoria.bin");
    match mem.salvar(&tmp) {
        Ok(_) => {
            let bytes = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
            println!(
                "\n  disco: {} episodios viram {} B = {:.0} B/pedido em memoria.bin",
                mem.len(),
                bytes,
                bytes as f64 / mem.len() as f64
            );
            // Recarregar e o que acontece toda vez que a Teka abre.
            let t = Instant::now();
            let volta = MemoriaEpisodica::carregar(&tmp);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            println!(
                "  recarregar esses {} episodios: {ms:.0} ms (TEMPO — ver aviso de contencao)",
                volta.map(|v| v.len()).unwrap_or(0)
            );
            let _ = std::fs::remove_file(&tmp);
        }
        Err(e) => println!("\n  disco: falhei ao salvar: {e}"),
    }

    // ---- `destilar` e O(n^2): o unico freio da memoria nao escala ----
    //
    // E o que `/consolidar` chama antes de treinar. Se ele fica caro demais, o
    // usuario perde a unica ferramenta que poda a memoria.
    //
    // Nao da pra medir isso na `mem` acima: os 16 pedidos repetidos dao 16
    // assinaturas, colapsam para 16 e o custo vira O(n*16) — o caso FACIL. O caso
    // que interessa e o oposto: uso real traz pedidos distintos, nada colapsa, e ai
    // e O(n^2) de verdade. Aqui se constroi esse pior caso de proposito.
    let d = ag.assinatura(&cache).len();
    println!("\n  destilar(0.995) — PIOR CASO (todo episodio distinto, nada colapsa) (TEMPO)");
    println!("  {:>10} {:>12} {:>14}", "episodios", "ms", "ms por 1k");
    for quantos in [500usize, 1000, 2000, 4000] {
        let mut pior = MemoriaEpisodica::nova();
        for i in 0..quantos {
            // Assinaturas ortogonais duas a duas: cosseno ~0, nunca passa de 0,995.
            let mut a = vec![0.0f32; d];
            a[i % d] = 1.0;
            a[(i / d) % d] += 0.5;
            pior.gravar(
                &format!("pedido numero {i}"),
                i % 22,
                vec![],
                Resultado::Executou,
                Feedback::Aprovado,
                a,
            );
        }
        let t = Instant::now();
        let colapsados = pior.destilar(0.995);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "  {quantos:>10} {ms:>12.0} {:>14.1}   (colapsou {colapsados})",
            ms / quantos as f64 * 1000.0
        );
    }

    // ---- Quanto tempo ate 1 GB ----
    //
    // O numero que interessa ao usuario nao e "bytes por pedido", e "quantas horas
    // ate incomodar". Precisa de uma taxa de uso: um pedido a cada X segundos.
    let por_ped = exata as f64 / mem.len() as f64;
    println!("\n  quanto tempo ate 1 GB so de episodica (por ritmo de uso):");
    println!("  {:>16} {:>14} {:>14}", "ritmo", "pedidos/1GB", "tempo");
    for (nome, seg_por_pedido) in [
        ("1 por segundo", 1.0f64),
        ("1 por 10 s", 10.0),
        ("1 por minuto", 60.0),
        ("1 por 5 min", 300.0),
    ] {
        let peds = 1.0e9 / por_ped;
        let horas = peds * seg_por_pedido / 3600.0;
        println!("  {nome:>16} {peds:>14.0} {:>11.1} h", horas);
    }
    println!("  (usando {por_ped:.0} B/pedido da contabilidade exata, sem o cabecalho do alocador)");
}

// ---------------------------------------------------------------------------
// MODO limites — NAO depende de tempo (o que importa e aceita/recusa/quebra).
// ---------------------------------------------------------------------------
fn modo_limites(caminho: &str) {
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let ag = carregar(caminho);
    banner(&ag, &ops);
    let mut cache = AgenteCache::new();

    let gigante = "abre o discord ".repeat(70_000); // ~1,05 MB
    let mut casos: Vec<(String, String)> = vec![
        ("vazio".into(), String::new()),
        ("so um espaco".into(), " ".into()),
        ("so espacos (80)".into(), " ".repeat(80)),
        ("so quebra de linha".into(), "\n".into()),
        ("so pontuacao".into(), "...".into()),
        ("um byte".into(), "a".into()),
        ("bytes invalidos-ish (acento solto)".into(), "\u{fffd}".into()),
        ("nulo no meio".into(), "abre o\0discord".into()),
        ("normal (referencia)".into(), "abre o discord".into()),
        ("10 KB".into(), "abre o discord ".repeat(700)),
        ("100 KB".into(), "abre o discord ".repeat(7_000)),
        ("1 MB".into(), gigante),
    ];
    casos.push(("1 MB de espaco".into(), " ".repeat(1_048_576)));

    println!("\n  {:<36} {:>10} {:>10} {:>22} {}", "caso", "bytes", "ms", "resultado", "obs");
    for (nome, texto) in &casos {
        let t = Instant::now();
        let r = ag.responder(&ops, &patcher, texto, &mut cache);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let (resultado, obs) = match &r {
            Ok(c) => (
                ag.registro.ferramentas.get(c.ferramenta).map(|f| f.nome.clone()).unwrap_or("?".into()),
                String::new(),
            ),
            Err(e) => ("<erro>".to_string(), e.clone()),
        };
        println!("  {nome:<36} {:>10} {ms:>10.1} {resultado:>22} {obs}", texto.len());
    }
    println!("\n  (os ms aqui sao so ordem de grandeza; ver aviso de contencao no cabecalho)");
}

// ---------------------------------------------------------------------------
// MODO tamanho — TEMPO. Contaminado se houver treino rodando.
// ---------------------------------------------------------------------------
fn modo_tamanho(caminho: &str) {
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let ag = carregar(caminho);
    banner(&ag, &ops);
    let mut cache = AgenteCache::new();

    println!("\n  latencia x tamanho do pedido (mediana de repeticoes)");
    println!("  {:>10} {:>10} {:>10} {:>10} {:>12} {}", "alvo B", "real B", "mediana ms", "p90 ms", "us/byte", "interativo?");
    // Passa de 20 KB de proposito: e acima de ~100 KB que a curva sai do linear,
    // e esse joelho e parte da resposta sobre "pedido gigante".
    for alvo in [10usize, 50, 200, 1000, 5000, 20000, 50000, 100_000, 200_000] {
        // Texto real, nao lixo: o patcher por_palavra corta em espaco, e um bloco
        // sem espaco daria um patch gigante que nao representa uso.
        let mut texto = String::new();
        while texto.len() < alvo {
            texto.push_str("abre o discord ");
        }
        texto.truncate(alvo);
        // Nao cortar no meio de um caractere multibyte.
        while !texto.is_char_boundary(texto.len()) {
            texto.pop();
        }

        // Aquece e depois mede: a primeira chamada de cada tamanho paga o
        // redimensionamento dos buffers do cache, e isso nao e latencia de regime.
        for _ in 0..3 {
            let _ = ag.responder(&ops, &patcher, &texto, &mut cache);
        }
        let reps = if alvo <= 1000 { 60 } else if alvo <= 20000 { 12 } else { 5 };
        let mut amostras: Vec<f64> = Vec::with_capacity(reps);
        for _ in 0..reps {
            let t = Instant::now();
            let _ = ag.responder(&ops, &patcher, &texto, &mut cache);
            amostras.push(t.elapsed().as_secs_f64() * 1e3);
        }
        amostras.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = amostras[amostras.len() / 2];
        let p90 = amostras[(amostras.len() * 9 / 10).min(amostras.len() - 1)];
        println!(
            "  {alvo:>10} {:>10} {med:>10.2} {p90:>10.2} {:>12.3} {}",
            texto.len(),
            med * 1e3 / texto.len() as f64,
            if med > 100.0 { "NAO (>100ms)" } else { "sim" },
        );
    }
}

// ---------------------------------------------------------------------------
// MODO threads — TEMPO. Contaminado se houver treino rodando.
// ---------------------------------------------------------------------------
fn modo_threads(caminho: &str) {
    let patcher = PorPalavra::default();
    let ag = carregar(caminho);
    println!(
        "  {} CPUs logicas",
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)
    );

    // Dois tamanhos: o paralelismo do `Paralelo` so liga acima de 64 linhas
    // (`fatias`), entao um pedido curto pode nao usar thread nenhuma por desenho.
    for (rotulo, texto) in [
        ("pedido curto (14 B)", "abre o discord".to_string()),
        ("pedido longo (~5 KB)", "abre o discord ".repeat(340)),
    ] {
        println!("\n  {rotulo}");
        println!("  {:>8} {:>12} {:>10} {:>10}", "threads", "mediana ms", "speedup", "eficiencia");
        let mut base: Option<f64> = None;
        for t in [1usize, 2, 4, 8] {
            let ops = Paralelo::new(t);
            let mut cache = AgenteCache::new();
            for _ in 0..5 {
                let _ = ag.responder(&ops, &patcher, &texto, &mut cache);
            }
            let reps = 40;
            let mut amostras: Vec<f64> = Vec::with_capacity(reps);
            for _ in 0..reps {
                let ini = Instant::now();
                let _ = ag.responder(&ops, &patcher, &texto, &mut cache);
                amostras.push(ini.elapsed().as_secs_f64() * 1e3);
            }
            amostras.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let med = amostras[amostras.len() / 2];
            let b = *base.get_or_insert(med);
            println!("  {t:>8} {med:>12.2} {:>10.2}x {:>9.0}%", b / med, 100.0 * (b / med) / t as f64);
        }
    }
}

// ---------------------------------------------------------------------------
// MODO folego — TEMPO. Vazao por janela, com e sem memoria.
// ---------------------------------------------------------------------------
fn modo_folego(caminho: &str, segundos: u64) {
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let carga = Instant::now();
    let ag = carregar(caminho);
    let ms_carga = carga.elapsed().as_secs_f64() * 1e3;
    banner(&ag, &ops);
    println!("  carregar o modelo: {ms_carga:.0} ms");

    let mut cache = AgenteCache::new();
    let mut mem = MemoriaEpisodica::nova();
    let base_rss = rss::bytes();
    let inicio = Instant::now();
    let janela = 10.0f64;
    let mut prox = janela;
    let mut na_janela = 0usize;
    let mut total = 0usize;
    let mut linhas: Vec<(f64, f64, f64, usize)> = Vec::new();

    println!("\n  {:>8} {:>12} {:>12} {:>10}", "s", "pedidos/s", "RSS MB", "episodios");
    loop {
        let p = PEDIDOS[total % PEDIDOS.len()];
        let f = ag.responder(&ops, &patcher, p, &mut cache).map(|c| c.ferramenta).unwrap_or(0);
        let a = ag.assinatura(&cache);
        mem.gravar(p, f, vec![], Resultado::Executou, Feedback::Nenhum, a);
        total += 1;
        na_janela += 1;

        let t = inicio.elapsed().as_secs_f64();
        if t >= prox {
            let r = rss::bytes();
            let vazao = na_janela as f64 / janela;
            println!("  {t:>8.0} {vazao:>12.0} {:>12.1} {:>10}", r as f64 / 1e6, mem.len());
            linhas.push((t, vazao, r as f64, mem.len()));
            na_janela = 0;
            prox += janela;
            if t >= segundos as f64 {
                break;
            }
        }
    }
    let dur = inicio.elapsed().as_secs_f64();
    println!("\n  {total} pedidos em {dur:.0} s = {:.0}/s medio | {:.3} ms/pedido", total as f64 / dur, dur * 1e3 / total as f64);
    if linhas.len() >= 2 {
        let (_, v0, r0, _) = linhas[0];
        let (_, v1, r1, n1) = *linhas.last().unwrap();
        println!("  vazao primeira janela {v0:.0}/s -> ultima {v1:.0}/s  ({:+.1}%)", 100.0 * (v1 - v0) / v0);
        println!("  RSS {:.1} MB -> {:.1} MB | {:.0} B/pedido (delta RSS / pedidos)", r0 / 1e6, r1 / 1e6, (r1 - base_rss as f64) / n1 as f64);
        let (exata, cap) = bytes_da_episodica(&mem);
        println!("  episodica exata: {} B por len, {} B por capacity | {:.0} B/pedido", exata, cap, exata as f64 / n1 as f64);
    }
}

fn main() {
    let modo = std::env::args().nth(1).unwrap_or_else(|| "contabilidade".into());
    let modelo = std::env::args().nth(2).unwrap_or_else(|| "modelos/teka_fechar_s19.bin".into());
    let n: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(5000);

    println!("\n  sonda_carga — modo {modo}");
    match modo.as_str() {
        "contabilidade" => modo_contabilidade(&modelo, n),
        "limites" => modo_limites(&modelo),
        "tamanho" => modo_tamanho(&modelo),
        "threads" => modo_threads(&modelo),
        "folego" => modo_folego(&modelo, n as u64),
        outro => {
            eprintln!("  modo desconhecido: {outro}");
            eprintln!("  use: contabilidade | limites | tamanho | threads | folego");
            std::process::exit(2);
        }
    }
}
