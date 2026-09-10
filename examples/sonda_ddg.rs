//! A pagina de resultados responde a frase composta?
//!
//! Uma consulta so, sem repeticao, para separar "a consulta falha" de "levei
//! bloqueio por pedir rapido demais".
fn main() {
    let q = std::env::args().nth(1).unwrap_or_else(|| "melhor configuracao de RAM para Ryzen 5 5500".into());
    let u = teka::tools::busca_ddg::url(&q);
    println!("  url: {u}\n");
    match teka::coletor::fonte::buscar(&u, 15) {
        Ok(html) => {
            println!("  html: {} bytes", html.len());
            let rs = teka::tools::busca_ddg::extrair(&html);
            println!("  extraidos: {}\n", rs.len());
            for r in rs.iter().take(3) {
                println!("  • {}", r.titulo);
                println!("    {}", r.trecho.chars().take(120).collect::<String>());
            }
            if rs.is_empty() {
                let amostra: String = html.chars().take(400).collect();
                println!("  --- inicio do html ---\n{amostra}");
            }
        }
        Err(e) => println!("  buscar falhou: {e}"),
    }
}
