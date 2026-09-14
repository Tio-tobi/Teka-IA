//! "fecha o discord" -> `abrir_programa(discord)`. Qual o tamanho disso?
//!
//! Achado em 13/09 por acaso, numa brincadeira do John com erro de Whisper. O
//! benchmark de 329 frases tem ZERO pedidos de fechar -- entao a inversao nunca
//! apareceu em medida nenhuma, e sobreviveu a semana inteira invisivel.
//!
//! Aqui nao entra no `frases_teste.txt`: isso mudaria o denominador e quebraria a
//! comparacao com todos os bracos ja medidos. Mede-se de fora primeiro.
//!
//! ## O que cada grupo responde
//!
//! - `abrir`   CONTROLE. Se ela errar aqui, o problema nao e o verbo de fechar.
//! - `fechar`  O ALVO. Nao existe ferramenta de fechar, entao o certo e `perguntar`.
//! - `matar`   O MESMO PEDIDO pelo caminho do comando. Ontem ela acertou
//!             `taskkill /F /IM Mir4G.exe` na mao -- a capacidade existe, falta nome.
//!
//! ## FALSIFICACAO, dita antes de rodar
//!
//! A tese e "o verbo nao e sinal nenhum no espaco de programa, entao qualquer verbo
//! com nome de programa cai em `abrir_programa`".
//!
//! - Se em `fechar` a maioria cair em `perguntar`, a tese esta ERRADA: ela ja
//!   distingue, e os poucos erros sao de vocabulario, nao de estrutura. Nao vale
//!   ferramenta nova.
//! - Se a maioria cair em `abrir_programa`, a tese se sustenta.
//! - Se espalhar por varias ferramentas sem dominante, a tese tambem esta errada --
//!   ai e ruido, nao inversao sistematica.
//!
//! O numero que importa e INVERSAO: fechar que virou `abrir_programa`. Esse e o
//! unico erro que faz o CONTRARIO do pedido com uma ferramenta que age.
use std::collections::BTreeMap;
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

/// (frase, grupo). Programas dos POCOS de treino, para nao medir vocabulario novo
/// junto com o fenomeno -- so o verbo muda entre `abrir` e `fechar`.
const CASOS: &[(&str, &str)] = &[
    // CONTROLE: ela sabe abrir?
    ("abre o discord", "abrir"),
    ("abre o spotify", "abrir"),
    ("abre o notepad", "abrir"),
    ("abre o steam", "abrir"),
    ("liga o chrome", "abrir"),
    ("inicia o vscode", "abrir"),
    ("poe o obs pra rodar", "abrir"),
    ("quero abrir o paint", "abrir"),
    // ALVO: o mesmo espaco, verbo invertido. Certo hoje = `perguntar`.
    ("fecha o discord", "fechar"),
    ("fecha o spotify", "fechar"),
    ("fecha o notepad", "fechar"),
    ("fecha o steam", "fechar"),
    ("fecha o chrome", "fechar"),
    ("encerra o vscode", "fechar"),
    ("sai do obs", "fechar"),
    ("desliga o paint", "fechar"),
    ("fecha o discord pra mim", "fechar"),
    ("pode fechar o spotify", "fechar"),
    ("quero fechar o chrome", "fechar"),
    ("fecha essa janela do notepad", "fechar"),
    ("tira o steam da tela", "fechar"),
    ("para o obs ai", "fechar"),
    ("finaliza o vscode", "fechar"),
    ("encerra o programa discord", "fechar"),
    // O MESMO PEDIDO pelo caminho do comando.
    ("mata o processo do discord", "matar"),
    ("roda taskkill no spotify", "matar"),
    ("usa o taskkill pra derrubar o notepad", "matar"),
    ("executa taskkill /IM chrome.exe", "matar"),
];

fn main() {
    let padrao = "modelos/teka_b329_s%.bin";
    let molde = std::env::args().nth(1).unwrap_or_else(|| padrao.into());
    let sementes: Vec<u32> = (19..=30).collect();

    let ops = Paralelo::auto();
    let patcher = PorPalavra::default();

    // grupo -> (certo, inversao, total)
    let mut soma: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    // frase -> quantas sementes a mandaram para `abrir_programa`
    let mut inverteu: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ferr: BTreeMap<String, usize> = BTreeMap::new();
    let mut n_modelos = 0usize;

    for s in &sementes {
        let caminho = molde.replace('%', &s.to_string());
        let Ok(ag) = Agente::<f32>::carregar(std::path::Path::new(&caminho), Registro::padrao())
        else {
            continue;
        };
        n_modelos += 1;
        let mut cache = AgenteCache::new();
        for (frase, grupo) in CASOS {
            let nome = match ag.responder(&ops, &patcher, frase, &mut cache) {
                Ok(c) => ag.registro.ferramentas[c.ferramenta].nome.clone(),
                Err(_) => "<erro>".to_string(),
            };
            let certo = match *grupo {
                "abrir" => nome == "abrir_programa",
                // Sem ferramenta de fechar, a resposta honesta e perguntar.
                "fechar" => nome == "perguntar",
                // Aqui o comando e o caminho legitimo; perguntar tambem serve.
                _ => nome == "executar_comando" || nome == "perguntar",
            };
            let inversao = *grupo != "abrir" && nome == "abrir_programa";
            let e = soma.entry(grupo).or_default();
            e.2 += 1;
            if certo {
                e.0 += 1;
            }
            if inversao {
                e.1 += 1;
                *inverteu.entry(frase).or_default() += 1;
            }
            if *grupo == "fechar" {
                *ferr.entry(nome).or_default() += 1;
            }
        }
    }

    if n_modelos == 0 {
        eprintln!("nenhum modelo casou com {molde}");
        return;
    }
    println!("\n  {n_modelos} modelos, {} frases cada\n", CASOS.len());
    println!("  {:<8} {:>12} {:>12}", "grupo", "certo", "INVERSAO");
    for (g, (ok, inv, n)) in &soma {
        let pct = 100.0 * *ok as f64 / *n as f64;
        let pinv = 100.0 * *inv as f64 / *n as f64;
        println!("  {g:<8} {ok:>5}/{n:<5} {pct:>4.0}% {inv:>5}/{n:<5} {pinv:>4.0}%");
    }

    println!("\n  onde caem os pedidos de FECHAR:");
    let mut v: Vec<_> = ferr.iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    let tot: usize = ferr.values().sum();
    for (nome, c) in v {
        println!("    {nome:<20} {c:>4}  {:>4.0}%", 100.0 * *c as f64 / tot as f64);
    }

    println!("\n  frases que inverteram (fechar -> abrir_programa), de {n_modelos} modelos:");
    let mut v: Vec<_> = inverteu.iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (f, c) in v {
        println!("    {c:>3}/{n_modelos}  {f}");
    }
    println!("\n  maioria em perguntar = tese ERRADA, nao vale ferramenta nova");
    println!("  maioria em abrir_programa = tese sustentada, o verbo nao e sinal");
}
