//! Maiuscula e ponto final custam quanto? A metade barata do teste de voz.
//!
//! O Whisper devolve "Feche o Discord pra mim." -- maiuscula na frente, ponto no
//! fim. Os moldes de treino sao TODOS minusculos e sem pontuacao, e a Teka le byte a
//! byte: `F` e `f` nao compartilham nada, e o `.` colado na ultima palavra muda o
//! patch inteiro.
//!
//! Isto nao precisa de audio nenhum. Pega as 329 frases da regua e aplica a
//! transformacao no texto. Se derrubar sozinho, o problema e de NORMALIZACAO --
//! conserto de dez linhas na entrada -- e nao da voz.
//!
//! ## FALSIFICACAO, dita antes de rodar
//!
//! A tese e "maiuscula e ponto custam acerto de verdade".
//!
//! - Queda < 1 ponto: tese ERRADA. O modelo ja e robusto a isso, e a normalizacao
//!   nao e o caminho. O que sobrar no teste de voz sera troca de palavra, nao forma.
//! - Queda >= 3 pontos: tese sustentada, e o conserto e barato.
//! - Entre 1 e 3: fraco. Registra e nao mexe.
//!
//! A separacao entre `MAIUSCULA` e `PONTO` sozinhas diz QUAL das duas paga a conta.
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

/// Primeira letra maiuscula, como todo transcritor devolve.
fn maiuscula(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn ponto(s: &str) -> String {
    let t = s.trim_end();
    if t.ends_with('?') || t.ends_with('.') || t.ends_with('!') {
        t.to_string()
    } else {
        format!("{t}.")
    }
}

/// A normalizacao candidata. E a funcao DE VERDADE da biblioteca, nao uma copia --
/// se eu reescrevesse a regra aqui, mediria o proxy em vez da propriedade.
fn norm(s: &str) -> String {
    teka::model::agente::normalizar_pedido(s).into_owned()
}

const FORMAS: &[(&str, fn(&str) -> String)] = &[
    ("original", |s| s.to_string()),
    ("MAIUSCULA", maiuscula),
    ("PONTO", ponto),
    ("whisper", |s| ponto(&maiuscula(s))),
    // A prova dos dois lados: recupera o falado, e NAO estraga o digitado.
    ("whisper+norm", |s| norm(&ponto(&maiuscula(s)))),
    ("original+norm", norm),
];

fn main() {
    let molde = std::env::args().nth(1).unwrap_or_else(|| "modelos/teka_tres_s%.bin".into());
    let casos = teka::learn::dados::ler_casos_teste(include_str!("../dados/frases_teste.txt"));
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();

    // forma -> acertos por semente
    let mut serie: Vec<(&str, Vec<f64>)> = FORMAS.iter().map(|(n, _)| (*n, Vec::new())).collect();
    let mut quebrou: std::collections::BTreeMap<String, usize> = Default::default();
    let mut n_modelos = 0usize;

    for s in 19..=30 {
        let caminho = molde.replace('%', &s.to_string());
        let Ok(ag) = Agente::<f32>::carregar(std::path::Path::new(&caminho), Registro::padrao())
        else {
            continue;
        };
        n_modelos += 1;
        let mut cache = AgenteCache::new();
        // Guarda o que a forma ORIGINAL acertou, para achar quem a transformacao quebrou.
        let mut certo_antes = vec![false; casos.len()];
        for (i, (nome, f)) in FORMAS.iter().enumerate() {
            let mut ok = 0usize;
            for (j, caso) in casos.iter().enumerate() {
                let texto = f(&caso.pedido);
                let acertou = match ag.responder(&ops, &patcher, &texto, &mut cache) {
                    Ok(ch) => ag.registro.ferramentas[ch.ferramenta].nome == caso.ferramenta,
                    Err(_) => false,
                };
                if acertou {
                    ok += 1;
                }
                if i == 0 {
                    certo_antes[j] = acertou;
                } else if *nome == "whisper" && certo_antes[j] && !acertou {
                    *quebrou.entry(caso.pedido.clone()).or_default() += 1;
                }
            }
            serie[i].1.push(100.0 * ok as f64 / casos.len() as f64);
        }
    }

    if n_modelos == 0 {
        eprintln!("nenhum modelo casou com {molde}");
        return;
    }
    let media = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let base = media(&serie[0].1);
    println!("\n  {n_modelos} modelos, {} frases\n", casos.len());
    println!("  {:<12} {:>8} {:>10}", "forma", "acerto", "delta");
    for (nome, v) in &serie {
        let m = media(v);
        println!("  {nome:<12} {m:>7.1}% {:>+9.2}", m - base);
    }

    println!("\n  frases que a forma whisper QUEBROU (acertava e parou de acertar):");
    let mut v: Vec<_> = quebrou.iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (f, c) in v.iter().take(25) {
        println!("    {c:>3}/{n_modelos}  {f}");
    }
    println!("    ... {} frases distintas quebraram em ao menos 1 semente", v.len());
    println!("\n  queda < 1 ponto = tese ERRADA, normalizar nao e o caminho");
    println!("  queda >= 3 = tese sustentada, e o conserto e barato");
}
