//! O lado da Teka da ponte com o DeepSeek-Harness: JSON-RPC por linha.
//!
//! ## O contrato, que é deles e não meu
//!
//! O Harness já publica o que a fusão precisa (`packages/sdk/server/`): *"serves
//! newline-delimited JSON-RPC over stdio so out-of-process SDK clients can drive
//! harness agents"*. Então o trabalho aqui não é inventar protocolo — é falar o que
//! existe.
//!
//! ```text
//! initialize      fronteira de prontidão: espera a árvore de plugins assentar
//! session/prompt  enfileira UMA mensagem e devolve `{ messageId }` NA HORA
//! notificações    `session.event` por fato durável, `session.status` por transição
//! shutdown        responde, descarrega, sai 0
//! ```
//!
//! ## Duas regras deles que mandam no desenho daqui
//!
//! **stdout é só protocolo.** Diagnóstico deles vai para stderr. Se este lado ler as
//! duas coisas na mesma tubulação, o parser quebra em texto que não é frame — e
//! quebra num dia qualquer, não no primeiro.
//!
//! **`session/prompt` é assíncrono.** Ele devolve `messageId` na hora e os fatos
//! chegam depois, em notificação. Não existe "resposta da chamada"; existe um fluxo.
//! Quem escrever isto esperando resposta síncrona vai esperar para sempre.
//!
//! ## Por que o protocolo está separado do processo
//!
//! `Protocolo` fala com qualquer `BufRead`/`Write`. É o que permite testar o
//! enquadramento, o casamento de `id`, a fila de notificação e os erros **sem subir
//! um Harness** — em memória, em milissegundos, na suíte comum.
//!
//! Subir processo e cuidar de tempo-limite é outra peça, e vem depois: misturar as
//! duas faria a lógica só testável com Node instalado, e teste que precisa de
//! ambiente é teste que não roda.

use crate::json::{self, Json};
use std::io::{BufRead, Write};

/// Uma sessão de JSON-RPC por linha.
pub struct Protocolo<R: BufRead, W: Write> {
    entrada: R,
    saida: W,
    proximo_id: u64,
    /// Notificações que chegaram enquanto eu esperava a resposta de um pedido.
    ///
    /// Elas **não podem ser descartadas**: `session/prompt` devolve só um
    /// `messageId`, e tudo que a Teka quer saber vem depois, aqui.
    pendentes: Vec<Json>,
}

/// Quantas linhas ler à espera de uma resposta antes de desistir.
///
/// O Harness pode emitir muita notificação entre o pedido e a resposta. Mas sem teto
/// um servidor que só notifica e nunca responde trava a Teka para sempre, e travar em
/// silêncio é o pior fim possível.
const LINHAS_MAXIMAS: usize = 10_000;

impl<R: BufRead, W: Write> Protocolo<R, W> {
    pub fn novo(entrada: R, saida: W) -> Self {
        Self { entrada, saida, proximo_id: 1, pendentes: Vec::new() }
    }

    /// Manda um pedido e espera a resposta **daquele id**.
    ///
    /// Notificações que chegarem no meio ficam guardadas em vez de descartadas.
    pub fn pedir(&mut self, metodo: &str, params: Json) -> Result<Json, String> {
        let id = self.proximo_id;
        self.proximo_id += 1;

        let quadro = json::obj(vec![
            ("jsonrpc", json::txt("2.0")),
            ("id", Json::Num(id as f64)),
            ("method", json::txt(metodo)),
            ("params", params),
        ]);
        self.escrever(&quadro)?;

        for _ in 0..LINHAS_MAXIMAS {
            let v = self.ler_quadro()?;
            match v.get("id").and_then(Json::numero) {
                // A resposta que eu esperava.
                Some(n) if n as u64 == id => {
                    if let Some(erro) = v.get("error") {
                        let msg = erro
                            .get("message")
                            .and_then(Json::texto)
                            .unwrap_or("sem mensagem");
                        let cod = erro.get("code").and_then(Json::numero).unwrap_or(0.0);
                        return Err(format!("{metodo} falhou ({cod:.0}): {msg}"));
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Json::Nulo));
                }
                // Resposta de OUTRO pedido: guardo. Pedidos independentes podem
                // enfileirar trabalho na mesma sessao, entao isto acontece.
                Some(_) => self.pendentes.push(v),
                // Sem `id` e notificacao.
                None => self.pendentes.push(v),
            }
        }
        Err(format!("{metodo}: {LINHAS_MAXIMAS} linhas sem a resposta do id {id}"))
    }

    /// Manda uma notificação — sem `id`, e portanto sem resposta.
    pub fn notificar(&mut self, metodo: &str, params: Json) -> Result<(), String> {
        let quadro = json::obj(vec![
            ("jsonrpc", json::txt("2.0")),
            ("method", json::txt(metodo)),
            ("params", params),
        ]);
        self.escrever(&quadro)
    }

    /// Tira da fila o que chegou sem eu ter pedido.
    pub fn colher(&mut self) -> Vec<Json> {
        std::mem::take(&mut self.pendentes)
    }

    /// Lê a próxima linha, mesmo que não seja resposta de nada.
    ///
    /// É como se consome o fluxo de `session.event` depois de um `session/prompt`.
    pub fn ler_quadro(&mut self) -> Result<Json, String> {
        let mut linha = String::new();
        loop {
            linha.clear();
            let n = self
                .entrada
                .read_line(&mut linha)
                .map_err(|e| format!("nao consegui ler do harness: {e}"))?;
            if n == 0 {
                return Err("o harness fechou a saida (EOF)".into());
            }
            // Linha em branco não é frame; o RFC não a proíbe entre frames.
            if linha.trim().is_empty() {
                continue;
            }
            return json::ler(linha.trim())
                .map_err(|e| format!("quadro invalido: {e} — linha: {:?}", corte(&linha)));
        }
    }

    fn escrever(&mut self, v: &Json) -> Result<(), String> {
        let linha = v.escrever();
        // O quadro NAO pode ter nova linha dentro: ela e o delimitador. O serializador
        // escapa `\n` dentro de texto, entao isto so falharia por bug — e e barato o
        // bastante para conferir em vez de confiar.
        debug_assert!(!linha.contains('\n'));
        self.saida
            .write_all(linha.as_bytes())
            .and_then(|_| self.saida.write_all(b"\n"))
            .and_then(|_| self.saida.flush())
            .map_err(|e| format!("nao consegui escrever para o harness: {e}"))
    }
}

/// Corta a linha para a mensagem de erro não despejar um quadro inteiro no log.
fn corte(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 120 {
        return s.to_string();
    }
    let ini: String = s.chars().take(117).collect();
    format!("{ini}...")
}

/// Os métodos do Harness, com os nomes que ele publica.
///
/// Separados de `Protocolo` porque `Protocolo` é JSON-RPC genérico e isto é o
/// vocabulário DELES. Se o vocabulário mudar numa versão, muda só aqui.
pub mod metodos {
    use super::*;

    /// `initialize` — a fronteira de prontidão.
    ///
    /// Quando o servidor é montado por uma composição de Loader, ele **espera a
    /// árvore de plugins assentar** antes de responder. Então descoberta assíncrona
    /// (ferramentas de MCP, por exemplo) já está visível no primeiro prompt.
    ///
    /// `serverInfo.name` estável no fio é `deepseek-harness-sdk-runtime`. Conferir
    /// isso é como a Teka sabe que falou com o Harness e não com outra coisa que
    /// aceitou a conexão.
    pub const NOME_ESPERADO: &str = "deepseek-harness-sdk-runtime";

    pub fn initialize<R: BufRead, W: Write>(p: &mut Protocolo<R, W>) -> Result<String, String> {
        let r = p.pedir("initialize", json::obj(vec![]))?;
        let nome = r
            .get("serverInfo.name")
            .and_then(Json::texto)
            .ok_or("initialize sem serverInfo.name")?;
        if nome != NOME_ESPERADO {
            return Err(format!("do outro lado nao e o harness: {nome:?}"));
        }
        Ok(nome.to_string())
    }

    /// `session/prompt` — enfileira uma mensagem e devolve o `messageId` NA HORA.
    ///
    /// O que a Teka pediu chega depois, em `session.event`. Quem tratar isto como
    /// chamada síncrona vai esperar uma resposta que não vem.
    pub fn prompt<R: BufRead, W: Write>(
        p: &mut Protocolo<R, W>,
        sessao: &str,
        texto: &str,
    ) -> Result<String, String> {
        let params = json::obj(vec![
            ("sessionId", json::txt(sessao)),
            (
                "contentBlocks",
                Json::Lista(vec![json::obj(vec![
                    ("type", json::txt("text")),
                    ("text", json::txt(texto)),
                ])]),
            ),
        ]);
        let r = p.pedir("session/prompt", params)?;
        r.get("messageId")
            .and_then(Json::texto)
            .map(str::to_string)
            .ok_or_else(|| "session/prompt sem messageId".into())
    }

    pub fn shutdown<R: BufRead, W: Write>(p: &mut Protocolo<R, W>) -> Result<(), String> {
        p.pedir("shutdown", json::obj(vec![])).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Monta um protocolo sobre linhas de mentira, e devolve o que foi escrito.
    fn com(linhas: &str) -> Protocolo<Cursor<Vec<u8>>, Vec<u8>> {
        Protocolo::novo(Cursor::new(linhas.as_bytes().to_vec()), Vec::new())
    }

    #[test]
    fn o_pedido_sai_no_formato_do_jsonrpc() {
        let mut p = com("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n");
        let r = p.pedir("session/prompt", json::obj(vec![("a", json::txt("b"))])).unwrap();
        assert_eq!(r.get("ok").and_then(Json::booleano), Some(true));

        let escrito = String::from_utf8(p.saida.clone()).unwrap();
        assert!(escrito.ends_with('\n'), "o quadro tem de terminar em nova linha");
        let q = json::ler(escrito.trim()).unwrap();
        assert_eq!(q.get("jsonrpc").and_then(Json::texto), Some("2.0"));
        assert_eq!(q.get("id").and_then(Json::numero), Some(1.0));
        assert_eq!(q.get("method").and_then(Json::texto), Some("session/prompt"));
        assert_eq!(q.get("params.a").and_then(Json::texto), Some("b"));
    }

    /// O caso que decide o desenho: notificação chegando ANTES da resposta.
    ///
    /// Descartá-las perderia tudo que interessa — `session/prompt` devolve só um
    /// `messageId`, e os fatos vêm em `session.event`.
    #[test]
    fn notificacao_no_meio_e_guardada_e_nao_descartada() {
        let mut p = com(concat!(
            "{\"jsonrpc\":\"2.0\",\"method\":\"session.status\",\"params\":{\"s\":\"pensando\"}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"session.event\",\"params\":{\"kind\":\"tool\"}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"messageId\":\"m-7\"}}\n"
        ));
        let id = metodos::prompt(&mut p, "s-1", "lista os arquivos").unwrap();
        assert_eq!(id, "m-7");

        let colhidas = p.colher();
        assert_eq!(colhidas.len(), 2, "as duas notificacoes tinham de sobreviver");
        assert_eq!(colhidas[0].get("params.s").and_then(Json::texto), Some("pensando"));
        assert_eq!(colhidas[1].get("params.kind").and_then(Json::texto), Some("tool"));
        assert!(p.colher().is_empty(), "colher esvazia a fila");
    }

    #[test]
    fn erro_do_harness_vira_erro_daqui_com_o_motivo() {
        let mut p = com(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601,\"message\":\"metodo desconhecido\"}}\n",
        );
        let e = p.pedir("nao_existe", json::obj(vec![])).unwrap_err();
        assert!(e.contains("metodo desconhecido"), "erro sem o motivo: {e}");
        assert!(e.contains("-32601"), "erro sem o codigo: {e}");
    }

    /// `initialize` confere COM QUEM falou.
    #[test]
    fn initialize_recusa_quem_nao_e_o_harness() {
        let mut bom = com(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"serverInfo\":{\"name\":\"deepseek-harness-sdk-runtime\"}}}\n",
        );
        assert!(metodos::initialize(&mut bom).is_ok());

        let mut outro = com(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"serverInfo\":{\"name\":\"outra-coisa\"}}}\n",
        );
        let e = metodos::initialize(&mut outro).unwrap_err();
        assert!(e.contains("nao e o harness"), "{e}");
    }

    /// EOF é fim, e tem de dizer isso — não travar nem entrar em pânico.
    #[test]
    fn eof_vira_erro_legivel() {
        let mut p = com("");
        let e = p.pedir("initialize", json::obj(vec![])).unwrap_err();
        assert!(e.contains("EOF"), "{e}");
    }

    /// Lixo no stdout deles é erro daqui, com a linha recortada no log.
    ///
    /// Não deveria acontecer — a doc deles diz que stdout é só protocolo. Mas
    /// "não deveria" não é garantia, e um parser que entra em pânico com texto
    /// inesperado derruba a Teka por causa de um `console.log` alheio.
    #[test]
    fn texto_que_nao_e_quadro_vira_erro_e_nao_panico() {
        let mut p = com("isto nao e json\n");
        let e = p.pedir("initialize", json::obj(vec![])).unwrap_err();
        assert!(e.contains("quadro invalido"), "{e}");
        assert!(e.contains("isto nao e json"), "o erro tem de mostrar a linha: {e}");
    }

    #[test]
    fn linha_em_branco_entre_quadros_nao_atrapalha() {
        let mut p = com("\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n");
        assert!(p.pedir("initialize", json::obj(vec![])).is_ok());
    }

    /// Servidor que só notifica e nunca responde não pode travar a Teka.
    #[test]
    fn servidor_que_nunca_responde_desiste_em_vez_de_travar() {
        let ruido = "{\"jsonrpc\":\"2.0\",\"method\":\"session.event\",\"params\":{}}\n"
            .repeat(LINHAS_MAXIMAS + 5);
        let mut p = com(&ruido);
        let e = p.pedir("initialize", json::obj(vec![])).unwrap_err();
        assert!(e.contains("sem a resposta"), "{e}");
    }

    /// O `id` cresce, e a resposta de um pedido não vale para o seguinte.
    #[test]
    fn cada_pedido_espera_o_proprio_id() {
        let mut p = com(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"n\":1}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"n\":2}}\n"
        ));
        assert_eq!(p.pedir("a", json::obj(vec![])).unwrap().get("n").and_then(Json::numero), Some(1.0));
        assert_eq!(p.pedir("b", json::obj(vec![])).unwrap().get("n").and_then(Json::numero), Some(2.0));

        let escrito = String::from_utf8(p.saida.clone()).unwrap();
        let ids: Vec<f64> = escrito
            .lines()
            .filter_map(|l| json::ler(l).ok()?.get("id").and_then(Json::numero))
            .collect();
        assert_eq!(ids, vec![1.0, 2.0]);
    }

    /// Texto com nova linha dentro não pode quebrar o enquadramento.
    #[test]
    fn nova_linha_no_texto_e_escapada_e_nao_vira_quadro_novo() {
        let mut p = com("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
        metodos::prompt(&mut p, "s-1", "linha um\nlinha dois").ok();
        let escrito = String::from_utf8(p.saida.clone()).unwrap();
        assert_eq!(escrito.lines().count(), 1, "virou dois quadros: {escrito:?}");
        let q = json::ler(escrito.trim()).unwrap();
        assert_eq!(
            q.get("params.contentBlocks.0.text").and_then(Json::texto),
            Some("linha um\nlinha dois")
        );
    }
}
