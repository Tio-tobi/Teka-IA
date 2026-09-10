//! O teto do `buscar_web` caiu?
//!
//! O limite estava documentado como estrutural: a Instant Answer da DuckDuckGo so
//! responde TERMO UNICO de enciclopedia, e frase composta volta vazia. Quem
//! pergunta "que configuracao usar" recebe verbete.
//!
//! Roda a mesma consulta com e sem a ponte, e o par e a resposta.
//!
//!   TEKA_PONTE_TOKEN=... cargo run --release --example sonda_busca
fn main() {
    let reg = teka::tools::Registro::padrao();
    let pol = teka::tools::seguranca::Politica::default();
    let consultas = [
        "capital do Brasil",
        "qual a melhor configuracao de RAM para Ryzen 5 5500",
    ];
    for q in consultas {
        println!("\n=================== {q:?}");
        for (rotulo, com_ponte) in [("SEM a ponte", false), ("COM a ponte", true)] {
            let guarda = std::env::var("TEKA_PONTE_TOKEN").ok();
            if !com_ponte {
                unsafe { std::env::remove_var("TEKA_PONTE_TOKEN") };
            } else if let Some(g) = &guarda {
                unsafe { std::env::set_var("TEKA_PONTE_TOKEN", g) };
            }
            let ch = teka::tools::Chamada {
                ferramenta: reg.indice("buscar_web").expect("buscar_web existe"),
                args: vec![("consulta".into(), q.to_string())],
            };
            let r = reg.executar(&ch, &pol);
            let texto = match r {
                Ok(t) => t,
                Err(e) => format!("ERRO: {e}"),
            };
            let recorte: String = texto.chars().take(260).collect();
            println!("  [{rotulo}] {recorte}");
            if let Some(g) = &guarda {
                unsafe { std::env::set_var("TEKA_PONTE_TOKEN", g) };
            }
        }
    }
}
