//! As tres que a sonda anterior pulou.
//!
//! `buscar_web` roda de verdade -- e so leitura de rede. As outras duas rodam em
//! SANDBOX: verificam que a chamada e aceita e o que ela FARIA, sem abrir janela
//! nem mexer no volume do John.
use teka::tools::{Chamada, Politica, Registro};
fn main() {
    let reg = Registro::padrao();
    println!("\n  -- de verdade --");
    let pol = Politica::real_em(std::env::temp_dir());
    for (nome, args) in [("buscar_web", vec![("consulta", "capital do Brasil")])] {
        let i = reg.indice(nome).unwrap();
        let c = Chamada { ferramenta: i, args: args.iter().map(|(k,v)| (k.to_string(), v.to_string())).collect() };
        match reg.executar(&c, &pol) {
            Ok(s) => println!("  {nome:<18} OK    {}", s.lines().next().unwrap_or("").chars().take(60).collect::<String>()),
            Err(e) => println!("  {nome:<18} ERRO  {e}"),
        }
    }
    println!("\n  -- em sandbox (nao mexe no mundo) --");
    let sb = Politica::default();
    for (nome, args) in [
        ("abrir_programa", vec![("programa", "notepad")]),
        ("atalho", vec![("nome", "aumenta o volume")]),
    ] {
        let i = reg.indice(nome).unwrap();
        let c = Chamada { ferramenta: i, args: args.iter().map(|(k,v)| (k.to_string(), v.to_string())).collect() };
        match reg.executar(&c, &sb) {
            Ok(s) => println!("  {nome:<18} OK    {}", s.lines().next().unwrap_or("").chars().take(60).collect::<String>()),
            Err(e) => println!("  {nome:<18} ERRO  {e}"),
        }
    }
}
