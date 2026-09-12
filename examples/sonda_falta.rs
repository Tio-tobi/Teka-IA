//! Nas frases deiticas do John, o que ela faz HOJE?
//!
//! Tres saidas possiveis, e so uma delas o mecanismo novo atende:
//!
//!   perguntar          o caminho de contexto que ja existe em `main.rs` cobre
//!   ferramenta + Err   E ISTO que `responder_ou_falta` transforma em pergunta
//!   ferramenta certa   ela achou um argumento onde eu achei que nao havia
use std::collections::BTreeMap;
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache, Falta};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

fn main() {
    let caminho = std::path::Path::new("modelos/teka_tr33_s19.bin");
    let ag = match Agente::<f32>::carregar(caminho, Registro::padrao()) {
        Ok(a) => a,
        Err(e) => { eprintln!("nao carreguei o modelo: {e}"); return; }
    };
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let mut cache = AgenteCache::new();

    // As deiticas que viraram `perguntar` no benchmark, do arquivo do John.
    let texto = std::fs::read_to_string("dados/frases_john_rotuladas.txt").unwrap_or_default();
    let mut n = 0usize;
    let mut conta: BTreeMap<&str, usize> = BTreeMap::new();
    let mut exemplos: Vec<String> = Vec::new();

    for l in texto.lines() {
        if l.starts_with('#') || !l.contains('|') { continue; }
        let p: Vec<&str> = l.split('|').map(str::trim).collect();
        if p.len() < 2 || p[0] != "perguntar" { continue; }
        n += 1;
        match ag.responder_ou_falta(&ops, &patcher, p[1], &mut cache) {
            Ok((c, _)) => {
                let nome = &ag.registro.ferramentas[c.ferramenta].nome;
                if nome == "perguntar" { *conta.entry("abstem (contexto ja cobre)").or_default() += 1; }
                else {
                    *conta.entry("age com argumento que achou").or_default() += 1;
                    if exemplos.len() < 10 {
                        exemplos.push(format!("    {:<42} -> {}", p[1], c.texto(&ag.registro)));
                    }
                }
            }
            Err(Falta::Argumento { ferramenta, ref param }) => {
                *conta.entry("FALTA ARGUMENTO (o mecanismo novo)").or_default() += 1;
                if exemplos.len() < 6 {
                    let f = &ag.registro.ferramentas[ferramenta];
                    let q = Falta::Argumento { ferramenta, param: param.clone() }
                        .pergunta(&ag.registro).unwrap_or_default();
                    exemplos.push(format!("    {:<44} -> {} | {q}", p[1], f.nome));
                }
            }
            Err(Falta::Outra(_)) => { *conta.entry("outra falha").or_default() += 1; }
        }
    }
    println!("\n  {n} frases rotuladas `perguntar`:\n");
    for (k, v) in &conta {
        println!("  {:<36} {v:>3}  ({:.0}%)", k, 100.0 * *v as f64 / n as f64);
    }
    if !exemplos.is_empty() {
        println!("\n  o que ela EMITE nessas frases:");
        for e in &exemplos { println!("{e}"); }
    }
}
