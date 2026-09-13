//! AS 22 FERRAMENTAS FUNCIONAM? Executa cada uma de verdade e diz.
//!
//! Pergunta do John em 12/09: "vc testou totalmente todas as ferramentas?".
//! A resposta honesta era nao -- e a prova apareceu no mesmo dia: `copiar_arquivo`
//! e `mover_arquivo` nao conseguiam copiar para uma PASTA, e ninguem sabia.
//!
//! Contar mencao do nome em teste NAO responde: mistura "o teste executa" com "o
//! nome aparece num teste de classificacao". Isto aqui EXECUTA.
use teka::tools::{Chamada, Politica, Registro};

fn main() {
    let reg = Registro::padrao();
    let raiz = std::env::temp_dir().join(format!("teka_ferr_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(raiz.join("sub")).expect("raiz");
    std::fs::write(raiz.join("nota.txt"), b"alfa\nsenha secreta\nbeta").expect("w");
    std::fs::write(raiz.join("outro.txt"), b"nada aqui").expect("w");
    // Um PNG 1x1 de verdade. A primeira versao desta sonda mandava `nota.txt` para
    // o `ler_imagem` e levava "only accepts PNG/JPEG/WebP/GIF" -- erro do TESTE, que
    // eu quase li como erro da ferramenta.
    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
        0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
        0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(raiz.join("ponto.png"), PNG_1X1).expect("png");
    let pol = Politica::real_em(&raiz);
    let r = |p: &str| raiz.join(p).to_string_lossy().to_string();

    // (ferramenta, argumentos, roda de verdade?)  — as que mexem no mundo do John
    // ficam de fora: abrir_programa abriria janela, atalho mexeria no volume dele.
    let casos: Vec<(&str, Vec<(&str, String)>, bool)> = vec![
        ("hora", vec![], true),
        ("calcular", vec![("expressao", "2+3*4".into())], true),
        ("memoria", vec![], true),
        ("disco", vec![], true),
        ("processos", vec![], true),
        ("rede", vec![], true),
        ("listar_pasta", vec![("caminho", r(""))], true),
        ("ler_arquivo", vec![("caminho", r("nota.txt"))], true),
        ("info_arquivo", vec![("caminho", r("nota.txt"))], true),
        ("procurar_arquivo", vec![("nome", "nota".into()), ("raiz", r(""))], true),
        ("buscar_no_conteudo", vec![("padrao", "senha".into()), ("raiz", r(""))], true),
        ("escrever_arquivo", vec![("caminho", r("novo.txt")), ("texto", "oi".into())], true),
        ("criar_pasta", vec![("caminho", r("pasta_nova"))], true),
        ("copiar_arquivo", vec![("origem", r("nota.txt")), ("destino", r("sub"))], true),
        ("mover_arquivo", vec![("origem", r("outro.txt")), ("destino", r("sub"))], true),
        ("apagar_arquivo", vec![("caminho", r("novo.txt"))], true),
        ("ler_imagem", vec![("caminho", r("ponto.png"))], true),
        ("executar_comando", vec![("comando", "hostname".into())], true),
        ("perguntar", vec![], true),
        ("buscar_web", vec![("consulta", "capital do Brasil".into())], false),
        ("abrir_programa", vec![("programa", "notepad".into())], false),
        ("atalho", vec![("nome", "aumenta o volume".into())], false),
    ];

    let (mut ok, mut err, mut pulou) = (0, 0, 0);
    println!();
    for (nome, args, roda) in casos {
        let i = match reg.indice(nome) { Some(i) => i, None => { println!("  {nome:<20} SEM REGISTRO"); continue } };
        if !roda {
            println!("  {nome:<20} pulado   (mexe no mundo do John ou precisa de rede)");
            pulou += 1;
            continue;
        }
        let c = Chamada { ferramenta: i, args: args.into_iter().map(|(k, v)| (k.to_string(), v)).collect() };
        match reg.executar(&c, &pol) {
            Ok(s) => {
                let l = s.lines().next().unwrap_or("").chars().take(58).collect::<String>();
                println!("  {nome:<20} OK       {l}");
                ok += 1;
            }
            Err(e) => { println!("  {nome:<20} ERRO     {e}"); err += 1; }
        }
    }
    println!("\n  {ok} funcionam, {err} com erro, {pulou} pulados");
    let _ = std::fs::remove_dir_all(&raiz);
}
