//! Quantas das ferramentas do Harness servem pela ponte?
//!
//! Em 12/09 descobrimos que `read_image` NAO serve: ela le provider e modelo do
//! agente que chamou, e depois exige sessao viva no store. A ponte omite agente de
//! proposito -- e o que permite executar sem LLM.
//!
//! A linha divisoria tem nome: ferramenta SEM ESTADO passa, ferramenta acoplada a
//! SESSAO nao. Isto mede de que lado cada uma cai.
//!
//! METODO: chama cada uma com argumentos VAZIOS e classifica pelo erro. Nada
//! executa -- quem reclama de "missing required property" parou na validacao, que ja
//! e prova de que chegou la sem agente.
use std::collections::BTreeMap;
use teka::json;
use teka::tools::harness_tcp;

fn main() {
    let endereco = std::env::var("TEKA_PONTE_ENDERECO")
        .unwrap_or_else(|_| harness_tcp::ENDERECO_PADRAO.to_string());
    let segredo = match teka::tools::ponte_auto::garantir(&endereco) {
        Ok(s) => s,
        Err(e) => { eprintln!("{e}"); return; }
    };
    let prazo = std::time::Duration::from_secs(30);
    let mut c = match harness_tcp::conectar(&endereco, &segredo, prazo) {
        Ok(c) => c, Err(e) => { eprintln!("{e}"); return; } };
    let lista = match harness_tcp::listar(&mut c) {
        Ok(l) => l, Err(e) => { eprintln!("{e}"); return; } };
    println!("\n  {} ferramentas na ponte\n", lista.len());

    let mut grupos: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for f in &lista {
        let mut c2 = match harness_tcp::conectar(&endereco, &segredo, prazo) {
            Ok(c) => c, Err(_) => continue };
        let r = harness_tcp::chamar(&mut c2, &f.nome, json::obj(vec![]));
        let (grupo, detalhe) = match r {
            Ok(resp) => {
                let t = resp.texto.to_lowercase();
                if !resp.erro {
                    ("SEM ESTADO (executou)", resp.texto.chars().take(40).collect::<String>())
                } else if t.contains("session") || t.contains("agent") || t.contains("model route") {
                    ("precisa de SESSAO/AGENTE", resp.texto.chars().take(60).collect())
                } else if t.contains("missing") || t.contains("required") || t.contains("invalid arguments") {
                    ("SEM ESTADO (validou argumento)", resp.texto.chars().take(40).collect())
                } else {
                    ("outro erro", resp.texto.chars().take(60).collect())
                }
            }
            Err(e) => {
                let t = e.to_lowercase();
                if t.contains("session") || t.contains("agent") || t.contains("model route") {
                    ("precisa de SESSAO/AGENTE", e.chars().take(60).collect())
                } else {
                    ("falhou na chamada", e.chars().take(60).collect())
                }
            }
        };
        grupos.entry(grupo).or_default().push(format!("{:<24} {detalhe}", f.nome));
    }
    let total: usize = grupos.values().map(|v| v.len()).sum();
    for (g, v) in &grupos {
        println!("  === {g}: {} de {total}", v.len());
        for l in v { println!("      {l}"); }
        println!();
    }
}
