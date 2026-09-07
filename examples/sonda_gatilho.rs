//! O que a tabela de gatilhos faz com uma frase. Diagnostico, sem efeito nenhum.
//!
//!   cargo run --release --example sonda_gatilho -- "me muta" "muta o stitch"
fn main() {
    let frases: Vec<String> = std::env::args().skip(1).collect();
    let tab = teka::tools::gatilhos::tabela();
    for f in &frases {
        match teka::tools::gatilhos::casar_em(&tab, f) {
            Some((a, alvo)) => println!("  {f:38} -> {a:20} alvo={alvo:?}"),
            None => println!("  {f:38} -> (nenhum atalho)"),
        }
    }
}
