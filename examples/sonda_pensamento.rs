//! O que ela "pensa" num tick? Diagnostico, sem efeito nenhum.
//!
//! O passo PENSAR do `pulso` gera bytes a partir do fio (as ultimas coisas que
//! ela viu) com a temperatura vinda do afeto. O texto gerado nao e lido por
//! ninguem: fora do `pulso.rs` o unico consumidor e um `.len()`.
//!
//!   cargo run --release --example sonda_pensamento -- teka_ct_20_s19.bin
fn main() {
    let caminho = std::env::args().nth(1).unwrap_or_else(|| "teka.bin".into());
    let ops = teka::backend::parallel::Paralelo::new(4);
    let ag: teka::model::agente::Agente<f32> = match teka::model::agente::Agente::carregar(std::path::Path::new(&caminho), teka::tools::Registro::padrao()) {
        Ok(a) => a,
        Err(e) => { eprintln!("nao carreguei {caminho:?}: {e}"); return; }
    };
    let patcher = teka::model::patcher::PorPalavra::default();
    let mut rng = teka::rng::Rng::new(7);
    for (semente, temp) in [
        ("pula essa musica", 0.7), ("que horas sao", 0.7),
        ("lista os arquivos da pasta", 1.0), ("teka", 1.2),
    ] {
        let p = teka::pulso::pensar(&ag, &ops, &patcher, semente, 120, temp, &mut rng);
        println!("\n  fio: {semente:?}  (temperatura {temp})\n  pensou: {p:?}");
    }
}
