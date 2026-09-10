//! O `buscar_web` de verdade, pela ferramenta, com a repeticao e a reserva.
fn main() {
    let reg = teka::tools::Registro::padrao();
    let pol = teka::tools::seguranca::Politica::default();
    for q in [
        "capital do Brasil",
        "qual a melhor configuracao de RAM para Ryzen 5 5500",
        "como funciona memoria em dual channel",
    ] {
        let ch = teka::tools::Chamada {
            ferramenta: reg.indice("buscar_web").expect("existe"),
            args: vec![("consulta".into(), q.to_string())],
        };
        println!("\n=============== {q:?}");
        match reg.executar(&ch, &pol) {
            Ok(t) => {
                for l in t.lines().take(4) {
                    println!("  {}", l.chars().take(96).collect::<String>());
                }
            }
            Err(e) => println!("  ERRO: {e}"),
        }
    }
}
