//! Com que valores as ferramentas novas aprendem os argumentos delas?
//!
//! `valor_para` casa por (ferramenta, parametro) e cai em `TEXTOS` quando nada bate.
//! `buscar_no_conteudo` e `ler_imagem` entraram em 09/09 sem braco proprio.
use teka::learn::dados::gerar;
use teka::model::patcher::PorPalavra;
use teka::rng::Rng;
use teka::tools::Registro;

fn main() {
    let reg = Registro::padrao();
    let patcher = PorPalavra::default();
    let mut rng = Rng::new(7);
    let exs = gerar(&reg, &patcher, 4000, &mut rng);
    for alvo in ["ler_imagem", "buscar_no_conteudo", "ler_arquivo", "listar_pasta"] {
        let i = reg.indice(alvo).unwrap();
        let mut vistos: Vec<String> = Vec::new();
        for e in exs.iter().filter(|e| e.ferramenta == i) {
            for &(_, (a, b)) in &e.args {
                let v = e.pedido[a..b].to_string();
                if !vistos.contains(&v) {
                    vistos.push(v);
                }
            }
        }
        vistos.sort();
        println!("\n=== {alvo}  ({} valores distintos)", vistos.len());
        println!("  {}", vistos.iter().take(14).cloned().collect::<Vec<_>>().join(" | "));
    }
}
