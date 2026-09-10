//! A assinatura pesa o FIM da frase. Isso e o defeito?
//!
//! `a_memoria_recupera_o_episodio_certo_pela_leitura_do_pedido` passa com 20
//! ferramentas (cos 0,382 no episodio certo) e falha com 22 (cos 0,158, trazendo
//! "que horas sao"). A acuracia e a MESMA dos dois lados -- 53,9% contra 54,0% --
//! entao nao e falta de treino, e treinar mais nao consertaria.
//!
//! A suspeita, dita antes de medir: `assinatura()` devolve o estado do ULTIMO
//! patch. Num backbone recorrente com porta o estado decai, entao ela pesa o fim
//! da frase -- e o assunto esta no MEIO:
//!
//!     "como esta a ram do computador"     termina em "computador", assunto "ram"
//!     "quanto de memoria esta em uso"     termina em "uso",        assunto "memoria"
//!     "que horas sao"                     termina em "sao"
//!
//! Com 20 ferramentas isso dava certo. Se der certo por acidente, um esquema que
//! olha a frase INTEIRA tem de separar melhor -- e nao so no caso que falha.
//!
//! UM treino, varios esquemas: o modelo e o mesmo para todos, entao a comparacao
//! nao carrega diferenca de treino junto.
//!
//! FALSEAMENTO: se o agrupamento pela frase inteira NAO recuperar o episodio certo,
//! ou recuperar com margem pior, a minha explicacao esta errada e o problema nao e
//! onde a assinatura le.

use teka::backend::Paralelo;
use teka::learn::dados::{dividir_por_frase, gerar};
use teka::learn::supervisionado::{treinar_agente, CfgSup};
use teka::model::agente::{Agente, AgenteCache};
use teka::model::hierarchy::Config;
use teka::model::patcher::{Plano, PorPalavra};
use teka::rng::Rng;
use teka::tools::Registro;

/// Os quatro episodios guardados e a consulta -- iguais aos do teste.
const EPISODIOS: [&str; 4] = [
    "quanto de memoria esta em uso",
    "lista os arquivos de src",
    "quanto e 2+2",
    "que horas sao",
];
const CONSULTA: &str = "como esta a ram do computador";
const CERTO: usize = 0;

fn cosseno(a: &[f32], b: &[f32]) -> f32 {
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len().min(b.len()) {
        ab += a[i] * b[i];
        aa += a[i] * a[i];
        bb += b[i] * b[i];
    }
    if aa == 0.0 || bb == 0.0 { 0.0 } else { ab / (aa.sqrt() * bb.sqrt()) }
}

/// Os estados de TODOS os patches, `[n_patches][d]`, para batch = 1.
fn estados(ag: &Agente<f32>, patcher: &PorPalavra, cache: &AgenteCache<f32>, frase: &str) -> Vec<Vec<f32>> {
    let d = ag.cabecas.d;
    let bytes = frase.as_bytes().to_vec();
    let np = Plano::novo(patcher, &bytes, bytes.len(), 1).n_patches[0];
    let z = cache.modelo.z();
    // `z` e [p_max, batch, d]; com batch = 1 o patch p comeca em p*d.
    (0..np).map(|p| z[p * d..(p + 1) * d].to_vec()).collect()
}

fn media(v: &[Vec<f32>]) -> Vec<f32> {
    let d = v[0].len();
    let mut m = vec![0.0; d];
    for e in v {
        for i in 0..d { m[i] += e[i]; }
    }
    for x in m.iter_mut() { *x /= v.len() as f32; }
    m
}

/// Media com as ULTIMAS `k` fora: o fim e o que o estado ja pesa demais.
fn media_sem_cauda(v: &[Vec<f32>], k: usize) -> Vec<f32> {
    let corte = v.len().saturating_sub(k).max(1);
    media(&v[..corte])
}

fn main() {
    let patcher = PorPalavra::default();
    let mut rng = Rng::new(9);
    let mut ag = Agente::<f32>::novo(Config::pequeno(), Registro::padrao(), &mut rng);
    let ops = Paralelo::auto();

    let mut r = Rng::new(5);
    let exs = gerar(&ag.registro, &patcher, 4000, &mut r);
    let (base, val) = dividir_por_frase(exs, 4);
    println!("  {} ferramentas no registro", ag.registro.n());
    treinar_agente(&mut ag, &ops, &patcher, &base, &val,
        &CfgSup { epocas: 10, log_cada: 10, ..Default::default() });

    let mut cache = AgenteCache::new();
    // Para cada frase: a assinatura de hoje, e os estados de todos os patches.
    let mut colhido = Vec::new();
    for frase in EPISODIOS.iter().chain(std::iter::once(&CONSULTA)) {
        let _ = ag.responder(&ops, &patcher, frase, &mut cache);
        colhido.push((ag.assinatura(&cache), estados(&ag, &patcher, &cache, frase)));
    }

    let esquemas: Vec<(&str, Box<dyn Fn(&(Vec<f32>, Vec<Vec<f32>>)) -> Vec<f32>>)> = vec![
        ("ultimo patch (hoje)", Box::new(|c: &(Vec<f32>, Vec<Vec<f32>>)| c.0.clone())),
        ("media da frase",      Box::new(|c: &(Vec<f32>, Vec<Vec<f32>>)| media(&c.1))),
        ("media sem a ultima",  Box::new(|c: &(Vec<f32>, Vec<Vec<f32>>)| media_sem_cauda(&c.1, 1))),
        ("media sem as 2 ult.", Box::new(|c: &(Vec<f32>, Vec<Vec<f32>>)| media_sem_cauda(&c.1, 2))),
    ];

    println!("\n  consulta: {CONSULTA:?}\n");
    println!("  {:<22} {:>8} {:>8} {:>8} {:>8}   vencedor          margem",
        "esquema", "memoria", "listar", "2+2", "hora");
    println!("  {}", "-".repeat(92));
    for (nome, f) in &esquemas {
        let q = f(&colhido[4]);
        let cos: Vec<f32> = (0..4).map(|i| cosseno(&f(&colhido[i]), &q)).collect();
        let venc = (0..4).max_by(|a, b| cos[*a].total_cmp(&cos[*b])).unwrap();
        let mut ord = cos.clone();
        ord.sort_by(|a, b| b.total_cmp(a));
        let margem = ord[0] - ord[1];
        println!("  {:<22} {:>8.3} {:>8.3} {:>8.3} {:>8.3}   {:<16} {:+.3} {}",
            nome, cos[0], cos[1], cos[2], cos[3],
            EPISODIOS[venc].split_whitespace().take(2).collect::<Vec<_>>().join(" "),
            margem,
            if venc == CERTO { "OK" } else { "ERRADO" });
    }
    println!("\n  margem = quanto o 1o ganha do 2o. Recuperar certo com margem de");
    println!("  0,005 nao e recuperar certo -- e sorte que vai virar vermelho depois.");
}
