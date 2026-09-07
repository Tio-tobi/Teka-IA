//! Arvore fria do Chromium: a primeira pergunta volta antes de a arvore nascer.
//!
//! Discord e Spotify sao Electron. Medido em PowerShell, a MESMA consulta duas
//! vezes seguidas no mesmo Discord: a primeira nao acha 'Silenciar', a segunda
//! acha. `uia::listar` ganhou teimosia por causa disto.
//!
//! O pareado tem de sair da MESMA arvore fria, senao os dois numeros vem de
//! estados diferentes e nao comparam nada. Entao: a consulta crua primeiro (que
//! e o que acorda a arvore), a teimosa logo depois.
//!
//!   cargo run --release --example sonda_uia -- Discord.exe
fn main() {
    let spec = std::env::args().nth(1).unwrap_or_else(|| "Discord.exe".into());
    println!("\n  alvo: {spec}\n");

    let t = std::time::Instant::now();
    match teka::tools::uia::listar_uma_vez(&spec, 0, "") {
        Ok((v, n)) => println!("  CRUA     {:5} controles  (arvore ofereceu {n})  {:?}", v.len(), t.elapsed()),
        Err(e) => println!("  CRUA     erro: {e}"),
    }
    let t = std::time::Instant::now();
    match teka::tools::uia::listar(&spec, 0, "") {
        Ok(v) => println!("  TEIMOSA  {:5} controles  {:?}", v.len(), t.elapsed()),
        Err(e) => println!("  TEIMOSA  erro: {e}"),
    }
    // E o que de fato importa: o botao esta la?
    match teka::tools::uia::listar(&spec, 0, "Silenciar") {
        Ok(v) => println!("\n  'Silenciar': {:?}", v),
        Err(e) => println!("\n  'Silenciar': erro: {e}"),
    }
}
