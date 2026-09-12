//! O argumento que ela extrai PARECE do tipo do parametro?
//!
//! Isto NAO constroi nada -- dimensiona. Medido em 12/09 que o modo de falha real
//! nao e "nao consegui recortar" e sim "recortei lixo com confianca":
//!
//!     "pega esse txt e faz uma copia dele"  ->  executar_comando(comando="dele")
//!
//! A pergunta que decide se vale construir a checagem de tipo na INFERENCIA:
//! quantos dos acertos de hoje ela perderia? Se marcar muito acerto como suspeito,
//! a checagem custa mais do que paga.
use std::collections::BTreeMap;
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::{Registro, TipoParam};

/// Frouxa de proposito: o alvo e pegar "dele" como comando, nao policiar estilo.
fn plausivel(v: &str, t: TipoParam) -> bool {
    match t {
        TipoParam::Numero => v.chars().any(|c| c.is_ascii_digit()),
        TipoParam::Caminho => {
            v.contains(r"\") || v.contains('/') || v.contains(':')
                || v.rsplit_once('.').is_some_and(|(a, e)| {
                    !a.is_empty() && (2..=5).contains(&e.len())
                        && e.chars().all(|c| c.is_ascii_alphanumeric())
                })
                // pasta sem extensao e caso real ("src", "documentos"): so recusa
                // quando e palavra funcional, que e o sintoma do recorte errado.
                || !matches!(v, "dele" | "dela" | "isso" | "aquilo" | "esse" | "essa"
                                | "aqui" | "ali" | "la" | "outro" | "outra")
        }
        TipoParam::Texto => !matches!(v, "dele" | "dela" | "isso" | "aquilo" | "esse"
                                        | "essa" | "aqui" | "ali" | "la"),
    }
}

fn main() {
    let ag = match Agente::<f32>::carregar(
        std::path::Path::new("modelos/teka_tr33_s19.bin"), Registro::padrao()) {
        Ok(a) => a, Err(e) => { eprintln!("{e}"); return; } };
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let mut cache = AgenteCache::new();

    let casos = teka::learn::dados::ler_casos_teste(
        &std::fs::read_to_string("dados/frases_teste.txt").unwrap());
    let mut c: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ex: Vec<String> = Vec::new();
    for caso in &casos {
        let Ok((ch, _)) = ag.responder_com_confianca(&ops, &patcher, &caso.pedido, &mut cache)
            else { *c.entry("nao respondeu").or_default() += 1; continue };
        let f = &ag.registro.ferramentas[ch.ferramenta];
        let certo = f.nome == caso.ferramenta;
        let mut suspeito = false;
        for (nome, v) in &ch.args {
            if let Some(p) = f.params.iter().find(|p| &p.nome == nome) {
                if !plausivel(v, p.tipo) {
                    suspeito = true;
                    if ex.len() < 8 && !certo {
                        ex.push(format!("    {:<40} -> {}", caso.pedido, ch.texto(&ag.registro)));
                    }
                }
            }
        }
        *c.entry(match (certo, suspeito) {
            (true, false) => "acerto, argumento plausivel",
            (true, true)  => "ACERTO marcado suspeito  <- o custo",
            (false, true) => "erro pego como suspeito  <- o ganho",
            (false, false)=> "erro que passa batido",
        }).or_default() += 1;
    }
    println!("\n  {} frases do benchmark:\n", casos.len());
    for (k, v) in &c { println!("  {k:<38} {v:>3}"); }
    println!("\n  erros pegos como suspeitos:");
    for e in &ex { println!("{e}"); }
}
