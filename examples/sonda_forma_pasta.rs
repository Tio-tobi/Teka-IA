//! Por que a forma "arquivo -> pasta" nao pegou?
//!
//! 16 moldes novos, 23% dos exemplos gerados com a forma, primitiva consertada e
//! binario conferido. E `copiar_arquivo`/`mover_arquivo` continuaram em 0,0%.
//!
//! HIPOTESE, dita antes de medir: eu consertei o DESTINO e a origem continuou
//! concreta. O benchmark pede origem DEITICA.
//!
//!     molde      "copia notas.md para a pasta backup"
//!     benchmark  "joga uma copia desse arquivo na pasta backup"
//!
//! Se for isso, ela acerta a forma ENSINADA e erra a COBRADA. Se errar as duas, a
//! hipotese esta errada e ela nao aprendeu a forma de jeito nenhum -- e o problema
//! seria outro.
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

/// (frase, de onde ela vem)
const CASOS: &[(&str, &str)] = &[
    // A forma ENSINADA, com origem concreta. Valores dos pocos, nao do benchmark.
    ("copia notas.md para a pasta backup", "ensinada"),
    ("move config.json para a pasta dados", "ensinada"),
    ("arquiva relatorio.txt na pasta documentos", "ensinada"),
    ("transfere agenda.md para a pasta build", "ensinada"),
    // A forma COBRADA: origem deitica, destino de pasta.
    ("joga uma copia desse arquivo na pasta backup", "cobrada"),
    ("pega esse arquivo e joga na pasta documentos", "cobrada"),
    ("tira essa imagem daqui e coloca na pasta fotos", "cobrada"),
    ("move esse arquivo pra pasta documentos", "cobrada"),
    // Meio-termo: origem concreta, frase do jeito do John.
    ("joga uma copia de notas.md na pasta backup", "meio"),
    ("pega notas.md e joga na pasta documentos", "meio"),
];

fn main() {
    let caminho = std::env::args().nth(1).unwrap_or("modelos/teka_pasta_s19.bin".into());
    let ag = match Agente::<f32>::carregar(std::path::Path::new(&caminho), Registro::padrao()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nao carreguei {caminho}: {e}");
            return;
        }
    };
    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();
    let mut cache = AgenteCache::new();
    println!("\n  modelo: {caminho}\n");
    let mut por: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for (frase, grupo) in CASOS {
        let r = ag.responder(&ops, &patcher, frase, &mut cache);
        let (nome, texto) = match &r {
            Ok(c) => (
                ag.registro.ferramentas[c.ferramenta].nome.as_str(),
                c.texto(&ag.registro),
            ),
            Err(e) => ("<erro>", e.clone()),
        };
        let certo = nome == "copiar_arquivo" || nome == "mover_arquivo";
        let e = por.entry(grupo).or_default();
        e.1 += 1;
        if certo {
            e.0 += 1;
        }
        println!(
            "  [{grupo:<8}] {} {:<48} {}",
            if certo { "ok " } else { "ERR" },
            frase,
            texto.chars().take(52).collect::<String>()
        );
    }
    println!();
    for (g, (ok, n)) in &por {
        println!("  {g:<10} {ok}/{n}");
    }
    println!("\n  ensinada alta + cobrada baixa = ela aprendeu a forma, o benchmark pede outra");
    println!("  as duas baixas             = ela nao aprendeu a forma; o problema e outro");
}
