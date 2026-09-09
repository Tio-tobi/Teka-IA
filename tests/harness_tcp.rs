//! A conversa da Teka com a ponte do Harness, testada contra uma ponte de mentira.
//!
//! A ponte de verdade é um plugin Node dentro de um perfil `dsh`. Depender dela aqui
//! faria o teste precisar de Node, do `dsh_home` e de um token — e teste que precisa
//! de ambiente é teste que não roda.
//!
//! Então a ponte falsa é um `TcpListener` numa thread, falando o mesmo protocolo. O
//! que ela prova é o que este lado precisa acertar: o `hello` vindo primeiro, o
//! `tools/list` sendo lido a cada vez, e o `content` virando texto.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use teka::json;
use teka::tools::harness_tcp;

const SEGREDO: &str = "segredo-de-teste";

/// Sobe uma ponte falsa numa porta livre e devolve o endereço.
///
/// `porta 0` deixa o sistema escolher: porta fixa em teste é conflito esperando
/// acontecer, e dois testes em paralelo brigariam pela mesma.
fn ponte_falsa(exige_hello: bool) -> String {
    let ouvinte = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endereco = ouvinte.local_addr().expect("addr").to_string();

    std::thread::spawn(move || {
        let Ok((fluxo, _)) = ouvinte.accept() else { return };
        atender(fluxo, exige_hello);
    });

    endereco
}

fn atender(fluxo: TcpStream, exige_hello: bool) {
    let mut escrita = fluxo.try_clone().expect("clone");
    let leitor = BufReader::new(fluxo);
    let mut apresentou = false;

    for linha in leitor.lines().map_while(Result::ok) {
        let linha = linha.trim().to_string();
        if linha.is_empty() {
            continue;
        }
        let v = json::ler(&linha).expect("quadro valido");
        let id = v.get("id").and_then(json::Json::numero).unwrap_or(0.0);
        let metodo = v.get("method").and_then(json::Json::texto).unwrap_or("");

        // O PRIMEIRO pedido tem de ser `hello`, como na ponte de verdade.
        if exige_hello && !apresentou {
            if metodo != "hello" || v.get("params.token").and_then(json::Json::texto) != Some(SEGREDO)
            {
                let _ = writeln!(
                    escrita,
                    r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32000,"message":"nao autenticado"}}}}"#
                );
                return;
            }
            apresentou = true;
            let _ = writeln!(escrita, r#"{{"jsonrpc":"2.0","id":{id},"result":{{"ok":true}}}}"#);
            continue;
        }

        let resposta = match metodo {
            "tools/list" => concat!(
                r#"{"tools":[{"name":"glob","description":"acha arquivo","parameters":{}},"#,
                r#"{"name":"grep","description":"procura texto","parameters":{}}]}"#
            )
            .to_string(),
            "tools/call" => {
                let nome = v.get("params.name").and_then(json::Json::texto).unwrap_or("");
                if nome == "nao_existe" {
                    r#"{"isError":true,"content":[{"type":"text","text":"unknown tool"}]}"#.into()
                } else {
                    // Dois blocos de proposito: o lado da Teka tem de juntar os dois.
                    r#"{"isError":false,"content":[{"type":"text","text":"linha um"},{"type":"text","text":"linha dois"}]}"#.into()
                }
            }
            _ => r#"{}"#.to_string(),
        };
        let _ = writeln!(escrita, r#"{{"jsonrpc":"2.0","id":{id},"result":{resposta}}}"#);
    }
}

fn prazo() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn conecta_autentica_e_lista() {
    let endereco = ponte_falsa(true);
    let mut c = harness_tcp::conectar(&endereco, SEGREDO, prazo()).expect("conectar");

    let v = harness_tcp::listar(&mut c).expect("listar");
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].nome, "glob");
    assert_eq!(v[0].descricao, "acha arquivo");
    assert_eq!(v[1].nome, "grep");
}

/// O `content` vem em blocos, e a Teka tem de juntar todos.
///
/// Um `grep` com muitos resultados volta partido; ficar só com o primeiro bloco
/// perderia o resto em silêncio, que é o pior jeito de perder dado.
#[test]
fn a_resposta_junta_todos_os_blocos_de_texto() {
    let endereco = ponte_falsa(true);
    let mut c = harness_tcp::conectar(&endereco, SEGREDO, prazo()).expect("conectar");

    let r = harness_tcp::chamar(&mut c, "glob", json::obj(vec![("pattern", json::txt("*.md"))]))
        .expect("chamar");
    assert!(!r.erro);
    assert_eq!(r.texto, "linha um\nlinha dois");
}

/// `isError` da ferramenta NÃO é erro da chamada.
///
/// A distinção importa: um `grep` que não acha nada e uma ponte que caiu são coisas
/// diferentes, e tratá-las igual esconde a segunda.
#[test]
fn ferramenta_que_falha_nao_e_chamada_que_falha() {
    let endereco = ponte_falsa(true);
    let mut c = harness_tcp::conectar(&endereco, SEGREDO, prazo()).expect("conectar");

    let r = harness_tcp::chamar(&mut c, "nao_existe", json::obj(vec![])).expect("a chamada em si vai bem");
    assert!(r.erro, "a FERRAMENTA falhou");
    assert!(r.texto.contains("unknown tool"));
}

/// Segredo errado é recusado, e a Teka diz o que houve.
#[test]
fn segredo_errado_nao_conecta() {
    let endereco = ponte_falsa(true);
    // `unwrap_err` exigiria `Debug` no lado Ok, e `Protocolo` guarda um socket —
    // casar o `Result` e mais claro que arranjar `Debug` para um cano.
    let Err(e) = harness_tcp::conectar(&endereco, "segredo-errado", prazo()) else {
        panic!("o segredo errado tinha de ser recusado");
    };
    assert!(e.contains("recusou o hello"), "erro inesperado: {e}");
}

/// Ponte que não existe vira erro legível, e não espera.
#[test]
fn ponte_fora_do_ar_vira_erro_legivel() {
    // Porta que ninguem escuta: peço uma, fecho, e uso o número.
    let porta = {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    let Err(e) =
        harness_tcp::conectar(&format!("127.0.0.1:{porta}"), SEGREDO, Duration::from_millis(400))
    else {
        panic!("nao devia conectar em porta que ninguem escuta");
    };
    assert!(e.contains("nao conectei"), "erro inesperado: {e}");
    assert!(e.contains("esta de pe"), "o erro tem de sugerir o motivo: {e}");
}
