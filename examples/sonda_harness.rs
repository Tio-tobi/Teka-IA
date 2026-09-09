//! Fala com a ponte do Harness e mostra o que ela oferece.
//!
//!   cargo run --release --example sonda_harness
//!
//! Precisa da ponte de pe:
//!   DSH_HOME=... TEKA_PONTE_TOKEN=... dsh --profile teka
fn main() {
    let endereco = std::env::args()
        .nth(1)
        .unwrap_or_else(|| teka::tools::harness_tcp::ENDERECO_PADRAO.to_string());
    let segredo = std::env::var("TEKA_PONTE_TOKEN").unwrap_or_default();
    if segredo.is_empty() {
        println!("  TEKA_PONTE_TOKEN nao definido — a ponte exige segredo");
        return;
    }

    let prazo = teka::tools::harness_tcp::PRAZO_PADRAO;
    let mut c = match teka::tools::harness_tcp::conectar(&endereco, &segredo, prazo) {
        Ok(c) => c,
        Err(e) => { println!("  {e}"); return; }
    };
    println!("\n  conectado e autenticado em {endereco}");

    match teka::tools::harness_tcp::listar(&mut c) {
        Ok(v) => {
            println!("  {} ferramentas do Harness:\n", v.len());
            for f in &v {
                println!("    {:<22} {}", f.nome, f.descricao.chars().take(60).collect::<String>());
            }
        }
        Err(e) => println!("  listar falhou: {e}"),
    }

    // Uma chamada de verdade, so leitura.
    let args = teka::json::obj(vec![
        ("pattern", teka::json::txt("*.md")),
        ("path", teka::json::txt("C:/Users/User/Projetos/Assistente/Teka-IA")),
    ]);
    match teka::tools::harness_tcp::chamar(&mut c, "glob", args) {
        Ok(r) => {
            println!("\n  glob *.md  ->  erro={}", r.erro);
            for l in r.texto.lines().take(6) {
                println!("    {l}");
            }
        }
        Err(e) => println!("\n  chamar falhou: {e}"),
    }
}
