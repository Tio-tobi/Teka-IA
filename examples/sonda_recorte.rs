//! `recortar` devolve vazio alguma vez, com pedido nao vazio?
use teka::model::agente::recortar;
fn main() {
    for (p, a, b) in [
        ("apaga esse arquivo aqui", 5, 5),
        ("apaga esse arquivo aqui", 0, 0),
        ("apaga , aqui", 6, 6),
        ("apaga ... aqui", 7, 8),
        ("a", 0, 0),
    ] {
        println!("  {p:?} ({a},{b}) -> {:?}", recortar(p.as_bytes(), a, b));
    }
}
