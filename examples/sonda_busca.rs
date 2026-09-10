//! As duas fontes do DuckDuckGo, lado a lado.
//!
//! O teto do `buscar_web` estava documentado como estrutural: a Instant Answer API
//! so responde TERMO UNICO de enciclopedia, e frase composta volta vazia. A pagina
//! de resultados responde.
//!
//! Roda a mesma consulta nas duas e o par e a resposta.
//!
//!   cargo run --release --example sonda_busca
fn main() {
    let consultas = [
        "capital do Brasil",
        "qual a melhor configuracao de RAM para Ryzen 5 5500",
    ];
    for q in consultas {
        println!("\n=============== {q:?}");

        // PAGINA DE RESULTADOS
        match teka::coletor::fonte::buscar(&teka::tools::busca_ddg::url(q), 15) {
            Ok(html) => {
                let rs = teka::tools::busca_ddg::extrair(&html);
                println!("  [pagina]  {} resultados", rs.len());
                for r in rs.iter().take(2) {
                    println!("            • {}", r.titulo.chars().take(70).collect::<String>());
                }
            }
            Err(e) => println!("  [pagina]  falhou: {e}"),
        }

        // O DuckDuckGo bloqueia pedido rapido demais do mesmo cliente. Sem esta
        // pausa, a segunda consulta volta vazia e parece defeito da consulta --
        // foi exatamente o que me confundiu na primeira medicao.
        std::thread::sleep(std::time::Duration::from_millis(1500));

        // INSTANT ANSWER, o teto antigo
        let u = format!(
            "https://api.duckduckgo.com/?q={}&format=json&kl=br-pt&no_html=1",
            teka::coletor::fonte::codificar(q)
        );
        match teka::coletor::fonte::buscar(&u, 15) {
            Ok(j) => {
                let tem = j.contains("\"AbstractText\":\"") && !j.contains("\"AbstractText\":\"\"");
                println!("  [api]     {}", if tem { "respondeu" } else { "VAZIO — o teto" });
            }
            Err(e) => println!("  [api]     falhou: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
}
