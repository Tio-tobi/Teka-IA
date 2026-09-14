//! Quanto custa a voz? A frase digitada contra a mesma frase falada e transcrita.
//!
//! Proposta do John em 13/09: usar o `omnivoice-pt`, que tem a voz clonada dele, para
//! gerar a fala, passar pelo Whisper, e usar a TRANSCRICAO como entrada. O motivo e
//! metodologico e vale mais que o resultado: erro de transcricao e ruido REAL com
//! estrutura real, e nenhum deles eu inventaria sozinho.
//!
//! Foi assim que a inversao "fecha" -> `abrir_programa` apareceu, por acaso, numa
//! brincadeira com "discordia".
//!
//! ## A tubulacao
//!
//! ```text
//!   frase do benchmark
//!     -> omnivoice-pt -p VozJohn.ovprompt   (RX 580, Vulkan, ~15s por frase)
//!     -> faster-whisper base E medium       (os dois que o John tem em cache)
//!     -> Teka
//! ```
//!
//! ## RESSALVA que nao pode sumir do resultado
//!
//! E voz SINTETIZADA, nao gravada. Pode ser mais dificil para o Whisper que a fala
//! real. O numero daqui e um TETO de dificuldade, nao a taxa de erro do John.
//!
//! O que sustenta a fidelidade: o `base` devolveu "Ligo discordia e rapidao" para
//! "liga o discord ai rapidao" -- exatamente o erro que ele descreveu de memoria,
//! antes de qualquer medicao.
//!
//! ## Como ler
//!
//! `digitado` e o teto: o que ela acerta com a frase limpa. A diferenca para
//! `base`/`medium` e o que a voz custa. Se `base` for muito pior que `medium`, a
//! recomendacao e trocar de modelo, e nao mexer na Teka.
use std::collections::BTreeMap;
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

fn main() {
    let mut a = std::env::args().skip(1);
    let tsv = a.next().unwrap_or_else(|| "dados/transcricoes_voz.tsv".into());
    let molde = a.next().unwrap_or_else(|| "modelos/teka_tres_s%.bin".into());

    let bruto = match std::fs::read_to_string(&tsv) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("nao li {tsv}: {e}");
            return;
        }
    };
    // (modelo_whisper, ferramenta_certa, frase_digitada, transcricao)
    let linhas: Vec<(String, String, String, String)> = bruto
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 4)
                .then(|| (c[0].into(), c[1].into(), c[2].into(), c[3].trim().into()))
        })
        .collect();
    if linhas.is_empty() {
        eprintln!("{tsv} vazio ou fora do formato");
        return;
    }
    let canais: Vec<String> = {
        let mut v: Vec<String> = linhas.iter().map(|l| l.0.clone()).collect();
        v.sort();
        v.dedup();
        v
    };

    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    // canal -> acertos somados; "digitado" e o teto
    let mut soma: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    // frase digitada -> em quantas sementes a voz QUEBROU (acertava digitada, errou falada)
    let mut quebrou: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut n_modelos = 0usize;

    for s in 19..=30 {
        let caminho = molde.replace('%', &s.to_string());
        let Ok(ag) = Agente::<f32>::carregar(std::path::Path::new(&caminho), Registro::padrao())
        else {
            continue;
        };
        n_modelos += 1;
        let mut cache = AgenteCache::new();
        let acerta = |ag: &Agente<f32>, c: &mut AgenteCache<f32>, t: &str, alvo: &str| -> bool {
            match ag.responder(&ops, &patcher, t, c) {
                Ok(ch) => ag.registro.ferramentas[ch.ferramenta].nome == alvo,
                Err(_) => false,
            }
        };
        // O teto mede so uma vez por frase distinta, senao conta cada uma duas vezes
        // (uma por modelo de whisper) e o denominador mente.
        let primeiro = &canais[0];
        for (canal, alvo, frase, transcrito) in &linhas {
            if canal == primeiro {
                let e = soma.entry("digitado".into()).or_default();
                e.1 += 1;
                if acerta(&ag, &mut cache, frase, alvo) {
                    e.0 += 1;
                }
            }
            let e = soma.entry(canal.clone()).or_default();
            e.1 += 1;
            let ok = acerta(&ag, &mut cache, transcrito, alvo);
            if ok {
                e.0 += 1;
            } else if canal == primeiro && acerta(&ag, &mut cache, frase, alvo) {
                let q = quebrou.entry(frase.clone()).or_default();
                q.0 += 1;
                q.1 = transcrito.clone();
            }
        }
    }

    if n_modelos == 0 {
        eprintln!("nenhum modelo casou com {molde}");
        return;
    }
    let n_frases = linhas.len() / canais.len().max(1);
    println!("\n  {n_modelos} modelos, {n_frases} frases, voz clonada do John\n");
    println!("  {:<10} {:>10} {:>10}", "canal", "acerto", "custo");
    let teto = soma.get("digitado").map(|(o, n)| 100.0 * *o as f64 / *n as f64).unwrap_or(0.0);
    for (canal, (ok, n)) in &soma {
        let p = 100.0 * *ok as f64 / *n as f64;
        if canal == "digitado" {
            println!("  {canal:<10} {p:>9.1}% {:>10}", "(teto)");
        } else {
            println!("  {canal:<10} {p:>9.1}% {:>+9.1}", p - teto);
        }
    }

    println!("\n  o que a voz quebrou (acertava digitada, errou transcrita):");
    let mut v: Vec<_> = quebrou.iter().collect();
    v.sort_by_key(|(_, (c, _))| std::cmp::Reverse(*c));
    for (frase, (c, transcrito)) in v.iter().take(20) {
        println!("    {c:>3}/{n_modelos}  {frase}");
        println!("             -> {transcrito}");
    }
    println!("\n  RESSALVA: voz sintetizada, nao gravada. Teto de dificuldade, nao a taxa do John.");
}
