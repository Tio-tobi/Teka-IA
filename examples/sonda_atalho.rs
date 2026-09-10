//! De onde vem a maiuscula (ou a falta dela) nos exemplos de `atalho`?
fn main() {
    let reg = teka::tools::Registro::padrao();
    let patcher = teka::model::patcher::PorPalavra::default();
    let mut rng = teka::rng::Rng::new(3);
    let exs = teka::learn::dados::gerar(&reg, &patcher, 20000, &mut rng);
    let i = reg.indice("atalho").unwrap();
    let meus: Vec<_> = exs.iter().filter(|e| e.ferramenta == i).collect();
    let comeca_no_arg = meus.iter().filter(|e| e.args.iter().any(|&(_, (a, _))| a == 0)).count();
    let maiuscula = meus.iter().filter(|e| e.pedido.chars().next().is_some_and(char::is_uppercase)).count();
    println!("  atalho: {} exemplos", meus.len());
    println!("  comecam NO argumento: {comeca_no_arg} ({:.1}%)", 100.0 * comeca_no_arg as f64 / meus.len() as f64);
    println!("  com maiuscula inicial: {maiuscula} ({:.1}%)", 100.0 * maiuscula as f64 / meus.len() as f64);
    println!("\n  amostra:");
    for e in meus.iter().take(8) {
        let no_zero = e.args.iter().any(|&(_, (a, _))| a == 0);
        println!("    [{}] {:?}", if no_zero { "arg em 0" } else { "livre  " }, e.pedido);
    }
}
