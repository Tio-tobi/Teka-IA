//! O destino de pasta sai bem-formado, ou sai "para a pasta notas.md"?
use teka::learn::dados::gerar;
use teka::model::patcher::PorPalavra;
use teka::rng::Rng;
use teka::tools::Registro;
fn main() {
    let reg = Registro::padrao();
    let exs = gerar(&reg, &PorPalavra::default(), 6000, &mut Rng::new(7));
    for alvo in ["copiar_arquivo", "mover_arquivo"] {
        let i = reg.indice(alvo).unwrap();
        let meus: Vec<_> = exs.iter().filter(|e| e.ferramenta == i).collect();
        let com = meus.iter().filter(|e| e.pedido.contains("pasta") || e.pedido.contains("diretorio")).count();
        println!("\n=== {alvo}: {} exemplos, {com} com pasta/diretorio ({:.0}%)",
                 meus.len(), 100.0 * com as f64 / meus.len().max(1) as f64);
        let mut n = 0;
        for e in &meus {
            if (e.pedido.contains("pasta") || e.pedido.contains("diretorio")) && n < 6 {
                let args: Vec<String> = e.args.iter()
                    .map(|(s, (a, b))| format!("{s}={:?}", &e.pedido[*a..*b])).collect();
                println!("   {:<52} {}", e.pedido, args.join(" "));
                n += 1;
            }
        }
    }
}
