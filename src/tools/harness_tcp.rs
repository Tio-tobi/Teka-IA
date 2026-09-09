//! A Teka falando com a ponte do Harness por socket local.
//!
//! ## Por que socket, e não os canos do processo
//!
//! A doc do servidor JSON-RPC do Harness diz que *"stdout is the protocol"*. Mas num
//! perfil montado o stdout já pertence ao aplicativo — o `headless` imprime a
//! resposta da tarefa ali. Dividir aquele cano quebraria o parser dos dois lados.
//!
//! Socket em `127.0.0.1` não disputa com ninguém. E não custa nada deste lado: o
//! [`Protocolo`](super::harness::Protocolo) é genérico sobre `BufRead`/`Write`, então
//! ele fala TCP sem uma linha de mudança. A peça de ontem serviu tal como estava.
//!
//! ## O que roda do outro lado
//!
//! Um plugin de ~180 linhas (`teka-ponte.mjs`) montado num perfil `dsh` que tem só o
//! `dsh-base` — as ferramentas, e nenhum aplicativo. Ele expõe:
//!
//! ```text
//! hello        autentica; TEM de ser o primeiro pedido
//! ping         { ok, tools }
//! tools/list   { tools: [ {name, description, parameters} ] }
//! tools/call   { isError, content, meta, value }
//! ```
//!
//! Medido em 2026-09-09: 25 ferramentas, e `glob` devolvendo arquivos de verdade —
//! **sem agente e sem LLM no caminho**. É o que torna a fusão possível sem
//! terceirizar a decisão, que era o preço da porta do SDK deles.
//!
//! ## Segurança
//!
//! O segredo vai no `hello` e o servidor derruba a conexão se o primeiro pedido for
//! outra coisa. Ele também **não sobe** sem o segredo no ambiente: um socket que
//! executa `pwsh` sem autenticação é um buraco, não um atalho.

use super::harness::{metodos, Protocolo};
use crate::json::{self, Json};
use std::io::BufReader;
use std::net::TcpStream;
use std::time::Duration;

/// Onde a ponte escuta por padrão. `8767` é a ponte do Spotify.
pub const ENDERECO_PADRAO: &str = "127.0.0.1:8768";

/// Quanto esperar por uma resposta antes de chamar de silêncio.
///
/// Generoso porque `tools/call` pode rodar `pwsh` ou uma busca na web, e um prazo
/// curto transformaria trabalho lento em falha.
pub const PRAZO_PADRAO: Duration = Duration::from_secs(120);

/// Uma conversa aberta com a ponte.
pub type Conversa = Protocolo<BufReader<TcpStream>, TcpStream>;

/// Conecta, autentica, e devolve a conversa pronta.
///
/// A autenticação acontece **aqui**, e não fica a cargo de quem chama: o servidor
/// derruba a conexão se o primeiro pedido não for `hello`, então uma conexão sem
/// autenticar não é uma conexão a meio caminho — é uma conexão morta.
pub fn conectar(endereco: &str, segredo: &str, prazo: Duration) -> Result<Conversa, String> {
    let destino = endereco
        .parse()
        .map_err(|e| format!("endereco invalido {endereco:?}: {e}"))?;

    let fluxo = TcpStream::connect_timeout(&destino, prazo)
        .map_err(|e| format!("nao conectei em {endereco}: {e} (a ponte esta de pe?)"))?;

    // Prazo nos DOIS sentidos. Sem ele, uma ponte que aceita a conexao e emudece
    // deixa a Teka esperando para sempre.
    fluxo
        .set_read_timeout(Some(prazo))
        .and_then(|_| fluxo.set_write_timeout(Some(prazo)))
        .map_err(|e| format!("nao consegui pôr prazo no socket: {e}"))?;

    let leitor = BufReader::new(
        fluxo
            .try_clone()
            .map_err(|e| format!("nao consegui duplicar o socket: {e}"))?,
    );
    let mut conversa = Protocolo::novo(leitor, fluxo);
    apresentar(&mut conversa, segredo)?;
    Ok(conversa)
}

/// O `hello`, que tem de ser o primeiro pedido da conexão.
fn apresentar(c: &mut Conversa, segredo: &str) -> Result<(), String> {
    c.pedir("hello", json::obj(vec![("token", json::txt(segredo))]))
        .map(|_| ())
        .map_err(|e| format!("a ponte recusou o hello: {e}"))
}

/// As ferramentas que a ponte oferece agora.
///
/// Lida a cada chamada, e não guardada: o registro do Harness se enche ao longo da
/// montagem — medido, 0 ferramentas no `apply` e 25 um quarto de segundo depois.
/// Uma lista guardada cedo demais seria uma lista vazia para sempre.
pub fn listar(c: &mut Conversa) -> Result<Vec<Ferramenta>, String> {
    let r = c.pedir("tools/list", json::obj(vec![]))?;
    let lista = r
        .get("tools")
        .and_then(Json::lista)
        .ok_or("tools/list sem `tools`")?;
    Ok(lista
        .iter()
        .filter_map(|t| {
            Some(Ferramenta {
                nome: t.get("name")?.texto()?.to_string(),
                descricao: t
                    .get("description")
                    .and_then(Json::texto)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect())
}

/// Uma ferramenta do Harness, como a Teka a vê.
#[derive(Debug, Clone, PartialEq)]
pub struct Ferramenta {
    pub nome: String,
    pub descricao: String,
}

/// O que voltou de uma chamada.
#[derive(Debug, Clone)]
pub struct Resposta {
    /// A ferramenta em si falhou? **Não é o mesmo que a chamada ter falhado** — um
    /// `grep` que não acha nada devolve `isError: false` com texto dizendo isso.
    pub erro: bool,
    /// O texto que a ferramenta devolveu, já concatenado.
    pub texto: String,
}

/// Chama uma ferramenta do Harness.
pub fn chamar(c: &mut Conversa, nome: &str, argumentos: Json) -> Result<Resposta, String> {
    let r = c.pedir(
        "tools/call",
        json::obj(vec![("name", json::txt(nome)), ("arguments", argumentos)]),
    )?;
    let erro = r.get("isError").and_then(Json::booleano).unwrap_or(false);
    let texto = r
        .get("content")
        .and_then(Json::lista)
        .map(|blocos| {
            blocos
                .iter()
                .filter_map(|b| b.get("text").and_then(Json::texto))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    Ok(Resposta { erro, texto })
}

/// Encerra a conversa.
///
/// Existe para simetria e para o dia em que a ponte quiser saber que acabou; hoje
/// fechar o socket já basta do lado dela.
pub fn encerrar(c: &mut Conversa) -> Result<(), String> {
    metodos::shutdown(c).map(|_| ()).or(Ok(()))
}
