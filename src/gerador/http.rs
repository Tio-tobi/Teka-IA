//! Cliente HTTP mínimo para `127.0.0.1` — **ferramenta offline**.
//!
//! Este módulo existe para o gerador de dados falar com o LM Studio. A Teka em si
//! **nunca** o usa: o LLM produz um arquivo de texto, e o runtime dela continua sem
//! falar com ninguém. Zero dependências preservado.
//!
//! É só `std`: um POST HTTP/1.1 num socket TCP. Sem TLS, porque é localhost — e é
//! justamente isso que torna viável escrever à mão em 100 linhas em vez de arrastar
//! uma pilha de crates para dentro do projeto.
//!
//! ## Sobre a extração de JSON
//!
//! Não há parser de JSON aqui, e isso é deliberado. O que se precisa são dois
//! campos de formato conhecido (`content` e `embedding`), e escrever um parser
//! completo seria mais código para manter do que o problema justifica. As funções
//! abaixo são **extratores dirigidos**: sabem o que procuram e falham devolvendo
//! `None` quando o formato não é o esperado.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// POST em `http://host:porta/caminho` com corpo JSON. Devolve o corpo da resposta.
pub fn post_json(
    host: &str,
    porta: u16,
    caminho: &str,
    corpo: &str,
    timeout_s: u64,
) -> Result<String, String> {
    let mut fluxo = TcpStream::connect((host, porta))
        .map_err(|e| format!("nao consegui conectar em {host}:{porta} — {e}"))?;
    fluxo
        .set_read_timeout(Some(Duration::from_secs(timeout_s)))
        .map_err(|e| e.to_string())?;
    fluxo
        .set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;

    let pedido = format!(
        "POST {caminho} HTTP/1.1\r\n\
         Host: {host}:{porta}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{corpo}",
        corpo.len()
    );
    fluxo
        .write_all(pedido.as_bytes())
        .map_err(|e| format!("falha ao enviar: {e}"))?;

    let mut bruto = Vec::new();
    fluxo
        .read_to_end(&mut bruto)
        .map_err(|e| format!("falha ao ler resposta: {e}"))?;
    let texto = String::from_utf8_lossy(&bruto).into_owned();

    // `Connection: close` evita ter de interpretar chunked encoding: o servidor
    // fecha o socket no fim do corpo, e ler até EOF basta.
    match texto.find("\r\n\r\n") {
        Some(i) => Ok(texto[i + 4..].to_string()),
        None => Err(format!("resposta sem cabeçalho HTTP válido: {}", &texto[..texto.len().min(200)])),
    }
}

/// Escapa uma string para caber dentro de JSON.
pub fn escapar(s: &str) -> String {
    let mut saida = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '"' => saida.push_str("\\\""),
            '\\' => saida.push_str("\\\\"),
            '\n' => saida.push_str("\\n"),
            '\r' => saida.push_str("\\r"),
            '\t' => saida.push_str("\\t"),
            c if (c as u32) < 0x20 => saida.push_str(&format!("\\u{:04x}", c as u32)),
            c => saida.push(c),
        }
    }
    saida
}

/// Lê uma string JSON a partir de `i`, que deve apontar para a aspa de abertura.
fn ler_string(b: &[u8], mut i: usize) -> Option<(String, usize)> {
    if b.get(i)? != &b'"' {
        return None;
    }
    i += 1;
    let mut saida = String::new();
    while i < b.len() {
        match b[i] {
            b'"' => return Some((saida, i + 1)),
            b'\\' => {
                i += 1;
                match *b.get(i)? {
                    b'n' => saida.push('\n'),
                    b'r' => saida.push('\r'),
                    b't' => saida.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(b.get(i + 1..i + 5)?).ok()?;
                        let n = u32::from_str_radix(hex, 16).ok()?;
                        saida.push(char::from_u32(n).unwrap_or('?'));
                        i += 4;
                    }
                    outro => saida.push(outro as char),
                }
                i += 1;
            }
            _ => {
                // Copia bytes crus e resolve o UTF-8 no fim — caractere acentuado
                // ocupa mais de um byte, e tratar byte a byte como `char` o quebraria.
                let ini = i;
                while i < b.len() && b[i] != b'"' && b[i] != b'\\' {
                    i += 1;
                }
                saida.push_str(&String::from_utf8_lossy(&b[ini..i]));
            }
        }
    }
    None
}

/// Extrai o primeiro `"campo":"..."` do JSON.
pub fn extrair_texto(json: &str, campo: &str) -> Option<String> {
    let b = json.as_bytes();
    let alvo = format!("\"{campo}\":");
    let i = json.find(&alvo)? + alvo.len();
    let i = i + b[i..].iter().take_while(|c| c.is_ascii_whitespace()).count();
    ler_string(b, i).map(|(s, _)| s)
}

/// Extrai todos os `"embedding":[...]` do JSON, na ordem.
pub fn extrair_embeddings(json: &str) -> Vec<Vec<f32>> {
    let mut saida = Vec::new();
    let mut resto = json;
    while let Some(i) = resto.find("\"embedding\":") {
        let apos = &resto[i + "\"embedding\":".len()..];
        let Some(abre) = apos.find('[') else { break };
        let Some(fecha) = apos[abre..].find(']') else { break };
        let corpo = &apos[abre + 1..abre + fecha];
        let v: Vec<f32> = corpo
            .split(',')
            .filter_map(|x| x.trim().parse::<f32>().ok())
            .collect();
        if !v.is_empty() {
            saida.push(v);
        }
        resto = &apos[abre + fecha..];
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapa_o_que_quebraria_o_json() {
        assert_eq!(escapar("a\"b"), "a\\\"b");
        assert_eq!(escapar("c:\\temp"), "c:\\\\temp");
        assert_eq!(escapar("linha\nnova"), "linha\\nnova");
        // Acentuado passa direto: JSON aceita UTF-8.
        assert_eq!(escapar("memória"), "memória");
    }

    #[test]
    fn extrai_conteudo_com_escapes_e_acento() {
        let j = r#"{"choices":[{"message":{"role":"assistant","content":"linha um\nlinha \"dois\"\nmemória em uso"}}]}"#;
        let c = extrair_texto(j, "content").expect("devia achar content");
        assert_eq!(c, "linha um\nlinha \"dois\"\nmemória em uso");
    }

    #[test]
    fn conteudo_vazio_e_conteudo_ausente_se_distinguem() {
        assert_eq!(extrair_texto(r#"{"content":""}"#, "content"), Some(String::new()));
        assert_eq!(extrair_texto(r#"{"outro":"x"}"#, "content"), None);
    }

    #[test]
    fn extrai_embeddings_na_ordem() {
        let j = r#"{"data":[{"embedding":[1.0,-0.5,0.25]},{"embedding":[0.1,0.2,0.3]}]}"#;
        let v = extrair_embeddings(j);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], vec![1.0, -0.5, 0.25]);
        assert_eq!(v[1], vec![0.1, 0.2, 0.3]);
        assert!(extrair_embeddings(r#"{"data":[]}"#).is_empty());
    }

    #[test]
    fn json_truncado_nao_entra_em_panico() {
        assert_eq!(extrair_texto(r#"{"content":"sem fim"#, "content"), None);
        assert!(extrair_embeddings(r#"{"embedding":[1.0,2.0"#).is_empty());
    }
}
