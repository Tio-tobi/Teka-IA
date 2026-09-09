//! Um Harness de mentira, só para os testes da ponte terem contra quem falar.
//!
//! ## Por que existe um binário só para teste
//!
//! O `Protocolo` já é testado em memória. Mas a camada de **processo** — canos
//! ligados, stderr drenado, prazo, `Drop` que não deixa zumbi — só se prova
//! subindo um processo de verdade.
//!
//! Testar contra o Harness real exigiria Node, `pnpm install` e um `cordis.yml`
//! montado. Teste que precisa de ambiente é teste que não roda, e teste que não roda
//! é o mesmo que não existir. Este binário é a alternativa: `cargo` o compila
//! sozinho, e o teste acha o caminho dele por `CARGO_BIN_EXE_falso_harness`.
//!
//! Ele imita só o que o contrato promete, e nada mais:
//!
//! ```text
//! initialize      responde com serverInfo.name = deepseek-harness-sdk-runtime
//! session/prompt  MANDA DUAS NOTIFICACOES E DEPOIS a resposta com messageId
//! shutdown        responde e sai 0
//! ```
//!
//! A ordem em `session/prompt` é de propósito: é o comportamento que quebraria um
//! cliente escrito supondo resposta síncrona.
//!
//! ## Modos de sabotagem
//!
//! `argv[1]` escolhe como ele se comporta mal, porque o caminho feliz é o menos
//! interessante de testar:
//!
//! ```text
//! (nada)      bem comportado
//! mudo        aceita tudo e nunca responde  -> prova o prazo
//! lixo        escreve texto que nao e quadro
//! barulhento  despeja MUITO no stderr       -> prova que stderr e drenado
//! morre       sai na hora                   -> prova o EOF
//! ```
use std::io::{BufRead, Write};

fn main() {
    let modo = std::env::args().nth(1).unwrap_or_default();
    if modo == "morre" {
        return;
    }
    if modo == "barulhento" {
        // Sem alguem lendo, o cano de stderr enche e o FILHO TRAVA na escrita. E o
        // impasse classico de processo com cano, e a razao de a ponte drenar stderr
        // numa thread. 64 KB costuma ser o tamanho do cano no Windows; passo bem
        // disso de proposito.
        for i in 0..4000 {
            eprintln!("[falso_harness] diagnostico numero {i} — isto vai para stderr");
        }
    }

    let entrada = std::io::stdin();
    let mut saida = std::io::stdout();
    for linha in entrada.lock().lines() {
        let Ok(linha) = linha else { break };
        let linha = linha.trim().to_string();
        if linha.is_empty() {
            continue;
        }
        if modo == "mudo" {
            continue;
        }
        if modo == "lixo" {
            let _ = writeln!(saida, "isto definitivamente nao e um quadro json");
            let _ = saida.flush();
            continue;
        }

        let id = campo_num(&linha, "id").unwrap_or(0);
        let metodo = campo_txt(&linha, "method").unwrap_or_default();

        match metodo.as_str() {
            "initialize" => {
                responder(
                    &mut saida,
                    id,
                    "{\"serverInfo\":{\"name\":\"deepseek-harness-sdk-runtime\"}}",
                );
            }
            "session/prompt" => {
                // As notificacoes vem ANTES da resposta, como no de verdade.
                let _ = writeln!(
                    saida,
                    "{{\"jsonrpc\":\"2.0\",\"method\":\"session.status\",\"params\":{{\"status\":\"pensando\"}}}}"
                );
                let _ = writeln!(
                    saida,
                    "{{\"jsonrpc\":\"2.0\",\"method\":\"session.event\",\"params\":{{\"kind\":\"tool\"}}}}"
                );
                responder(&mut saida, id, "{\"messageId\":\"m-1\"}");
            }
            "shutdown" => {
                responder(&mut saida, id, "{}");
                let _ = saida.flush();
                return;
            }
            outro => {
                let _ = writeln!(
                    saida,
                    "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32601,\"message\":\"metodo desconhecido: {outro}\"}}}}"
                );
            }
        }
        let _ = saida.flush();
    }
}

fn responder(saida: &mut impl Write, id: u64, resultado: &str) {
    let _ = writeln!(saida, "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{resultado}}}");
    let _ = saida.flush();
}

/// Extração rasa, de propósito: este binário não deve depender do `json` da Teka.
///
/// Se dependesse, um defeito no parser dela passaria despercebido — o teste estaria
/// medindo o parser contra ele mesmo.
fn campo_txt(linha: &str, chave: &str) -> Option<String> {
    let marca = format!("\"{chave}\":\"");
    let i = linha.find(&marca)? + marca.len();
    let resto = &linha[i..];
    let fim = resto.find('"')?;
    Some(resto[..fim].to_string())
}

fn campo_num(linha: &str, chave: &str) -> Option<u64> {
    let marca = format!("\"{chave}\":");
    let i = linha.find(&marca)? + marca.len();
    let resto = &linha[i..];
    let fim = resto.find(|c: char| !c.is_ascii_digit()).unwrap_or(resto.len());
    resto[..fim].parse().ok()
}
