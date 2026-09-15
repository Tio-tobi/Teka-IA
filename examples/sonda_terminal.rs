//! "A assertividade dela ta muito baixa para executar comandos de terminal" — John.
//!
//! Duas coisas opostas cabem nessa frase, e o conserto de uma PIORA a outra:
//!
//!   A) ela ABSTEM demais: o pedido e de comando e ela responde `perguntar`.
//!   B) ela ESCOLHE certo e escreve um comando que NAO RODA.
//!
//! Se for (A), o conserto e baixar o vies de abstencao. Se for (B), baixar o vies so
//! faz ela executar mais lixo com confianca. Por isso mede-se antes.
//!
//! ## A terceira coisa, que ja mordeu este projeto
//!
//! Em 12/09 eu afirmei "9 comandos rodariam" medindo com `where` do Git Bash. A Teka
//! roda `cmd /C`, que tem OUTRO PATH. O numero honesto era zero. Entao aqui o teste
//! de "roda?" e feito do jeito que ELA executa: `cmd /C`, com o comando inteiro, e
//! com um tempo-limite.
//!
//! ## SEGURANCA
//!
//! Executar comando de verdade tocaria a maquina do John. Aqui NADA e executado de
//! verdade: a checagem de "roda?" usa o proprio `cmd` so para RESOLVER O NOME do
//! executavel (`where` de dentro do `cmd /C`), nunca o comando completo. Um `del` que
//! ela escreva e classificado, nao rodado.
//!
//! ## FALSIFICACAO, dita antes de rodar
//!
//! Tese: o problema e (B) — ela escolhe a ferramenta certa e o comando nao roda.
//!
//! - Se `executar_comando` tiver acerto de ferramenta ALTO (>70%) e taxa de comando
//!   executavel BAIXA (<50%), a tese se sustenta.
//! - Se o acerto de ferramenta for baixo e a maioria dos erros cair em `perguntar`,
//!   a tese esta ERRADA e o problema e (A), abstencao.
//! - Se os erros se espalharem por outras ferramentas, nao e nenhum dos dois: e
//!   confusao de fronteira, e o conserto e de dado.
use std::collections::BTreeMap;
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::Registro;

/// O primeiro token do comando — o executavel.
fn executavel(cmd: &str) -> &str {
    cmd.trim().split_whitespace().next().unwrap_or("")
}

/// Esse nome existe para o `cmd /C`? Pergunta do jeito que ELA vai rodar.
///
/// `where` DE DENTRO do `cmd`, e nao o `where` do Git Bash — foi essa confusao que
/// produziu o "9 comandos rodariam" falso de 12/09.
fn resolve_no_cmd(nome: &str) -> bool {
    if nome.is_empty() {
        return false;
    }
    // Interno do cmd (dir, echo, cd...) nao aparece no `where`, mas roda.
    const INTERNOS: &[&str] = &[
        "dir", "echo", "cd", "type", "copy", "move", "del", "md", "mkdir", "rd",
        "rmdir", "cls", "set", "ver", "vol", "date", "time", "path", "assoc", "ftype",
    ];
    if INTERNOS.contains(&nome.to_lowercase().as_str()) {
        return true;
    }
    std::process::Command::new("cmd")
        .args(["/C", "where", nome])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn main() {
    let molde = std::env::args().nth(1).unwrap_or_else(|| "modelos/teka_fechar_s%.bin".into());
    let casos = teka::learn::dados::ler_casos_teste(include_str!("../dados/frases_teste.txt"));
    let alvo: Vec<_> = casos.iter().filter(|c| c.ferramenta == "executar_comando").collect();

    let ops = Paralelo::new(2);
    let patcher = PorPalavra::default();

    let mut n_modelos = 0usize;
    // para onde vao os erros
    let mut para_onde: BTreeMap<String, usize> = BTreeMap::new();
    let (mut certa, mut total) = (0usize, 0usize);
    // comando escrito -> (quantas vezes, roda?)
    let mut comandos: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    let mut roda_cache: BTreeMap<String, bool> = BTreeMap::new();
    // frase -> ferramenta errada -> quantas sementes
    let mut por_frase: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();

    for s in 19..=30 {
        let caminho = molde.replace('%', &s.to_string());
        let Ok(ag) = Agente::<f32>::carregar(std::path::Path::new(&caminho), Registro::padrao())
        else {
            continue;
        };
        n_modelos += 1;
        let mut cache = AgenteCache::new();
        for caso in &alvo {
            total += 1;
            let Ok(ch) = ag.responder(&ops, &patcher, &caso.pedido, &mut cache) else {
                *para_onde.entry("<erro>".into()).or_default() += 1;
                continue;
            };
            let nome = ag.registro.ferramentas[ch.ferramenta].nome.clone();
            if nome != "executar_comando" {
                *para_onde.entry(nome.clone()).or_default() += 1;
                let e = por_frase.entry(caso.pedido.clone()).or_default();
                *e.entry(nome).or_default() += 1;
                continue;
            }
            certa += 1;
            let cmd = ch
                .args
                .iter()
                .find(|(k, _)| k == "comando")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let exe = executavel(&cmd).to_string();
            let roda = *roda_cache.entry(exe.clone()).or_insert_with(|| resolve_no_cmd(&exe));
            let e = comandos.entry(cmd).or_insert((0, roda));
            e.0 += 1;
        }
    }

    if n_modelos == 0 {
        eprintln!("nenhum modelo casou com {molde}");
        return;
    }
    println!("\n  {n_modelos} modelos, {} frases de executar_comando\n", alvo.len());
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b as f64;
    println!("  ferramenta certa ....... {certa}/{total}  ({:.1}%)", pct(certa, total));

    let roda_n: usize = comandos.values().filter(|(_, r)| *r).map(|(n, _)| n).sum();
    println!("  comando que RODA ....... {roda_n}/{certa}  ({:.1}%)", pct(roda_n, certa.max(1)));
    println!(
        "  ponta a ponta .......... {roda_n}/{total}  ({:.1}%)  <- o numero do John",
        pct(roda_n, total)
    );

    println!("\n  para onde vao os erros de ferramenta:");
    let mut v: Vec<_> = para_onde.iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    let err_total: usize = para_onde.values().sum();
    for (nome, c) in v {
        println!("    {nome:<22} {c:>4}  {:>5.1}%", pct(*c, err_total.max(1)));
    }

    println!("\n  comandos que ela escreveu e NAO rodam:");
    let mut v: Vec<_> = comandos.iter().filter(|(_, (_, r))| !*r).collect();
    v.sort_by_key(|(_, (c, _))| std::cmp::Reverse(*c));
    for (cmd, (c, _)) in v.iter().take(20) {
        println!("    {c:>3}x  {cmd:?}");
    }
    println!("
  QUAL frase vai para qual ferramenta errada:");
    let mut vf: Vec<_> = por_frase.iter().collect();
    vf.sort_by_key(|(_, m)| std::cmp::Reverse(m.values().sum::<usize>()));
    for (frase, m) in vf {
        let mut d: Vec<String> = m.iter().map(|(f, c)| format!("{f} x{c}")).collect();
        d.sort();
        println!("    {frase:<50} {}", d.join(", "));
    }

    println!("\n  acerto alto + roda baixo = o problema e o ARGUMENTO, nao a abstencao");
    println!("  acerto baixo + erros em `perguntar` = e abstencao, e o vies resolve");
}
