//! Ponte WebSocket para a extensão do Spotify — o caminho que **não rouba o foco**.
//!
//! ## Por que WebSocket, num projeto sem dependência
//!
//! A extensão Spicetify que o John escreveu vive **dentro** do cliente do Spotify e
//! fala WebSocket com quem quiser controlá-la. Ela já está instalada e conectando a
//! cada 4 segundos. Falar o protocolo dela custa umas 300 linhas de `std`; mudar a
//! extensão para HTTP quebraria a Nyxara, que usa a mesma.
//!
//! ## O que isto compra sobre os outros caminhos
//!
//! Medido em 2026-09-06, com o navegador do John em primeiro plano e o handle da
//! janela comparado antes e depois de cada troca de faixa:
//!
//! ```text
//! Spicetify (socket)   toca por nome   foco INTACTO nas 5 musicas
//! UIAutomation         toca por nome   o Spotify SOBE na frente
//! tecla de midia       so pula/pausa   foco intacto
//! ```
//!
//! A ponte fala com o cliente por socket: não existe janela envolvida, então não há
//! foco para roubar. É a diferença entre pedir ao Spotify e mexer na tela dele.
//!
//! ## O handshake, e por que SHA-1 e base64 estão aqui
//!
//! O protocolo exige devolver `base64(sha1(chave + GUID))`. São as duas únicas
//! funções que faltavam, e as duas têm teste contra vetor conhecido — errar o
//! handshake não dá erro visível, o navegador só fecha a conexão sem dizer por quê.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

/// Porta da extensão da Teka. A da Nyxara fica na 8766 e não é tocada.
pub const PORTA: u16 = 8767;

/// Quanto esperar por uma resposta da extensão.
const PRAZO: Duration = Duration::from_secs(8);

/// O GUID que o protocolo manda concatenar. Não é segredo, é constante da RFC 6455.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// ---------------------------------------------------------------------------
// SHA-1 e base64 — o mínimo para o handshake
// ---------------------------------------------------------------------------

/// SHA-1 de um pedaço de bytes.
///
/// Aqui **não é criptografia**: é o que a RFC 6455 manda calcular para provar ao
/// navegador que o servidor entendeu o protocolo. Um SHA-1 errado não dá erro — a
/// conexão simplesmente não abre, e sem mensagem.
pub fn sha1(dados: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = dados.to_vec();
    let bits = (dados.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());

    for bloco in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([bloco[i * 4], bloco[i * 4 + 1], bloco[i * 4 + 2], bloco[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut fora = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        fora[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    fora
}

pub fn base64(dados: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(dados.len().div_ceil(3) * 4);
    for p in dados.chunks(3) {
        let b = [p[0], *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        s.push(A[(n >> 18) as usize & 63] as char);
        s.push(A[(n >> 12) as usize & 63] as char);
        s.push(if p.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if p.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    s
}

// ---------------------------------------------------------------------------
// Quadros
// ---------------------------------------------------------------------------

/// Escreve um quadro de texto. **Servidor não mascara** — é regra do protocolo, e
/// o contrário quebra o cliente sem mensagem de erro.
fn escrever_texto(f: &mut TcpStream, texto: &str) -> std::io::Result<()> {
    let b = texto.as_bytes();
    let mut q = vec![0x81u8]; // FIN + opcode texto
    if b.len() < 126 {
        q.push(b.len() as u8);
    } else if b.len() < 65536 {
        q.push(126);
        q.extend_from_slice(&(b.len() as u16).to_be_bytes());
    } else {
        q.push(127);
        q.extend_from_slice(&(b.len() as u64).to_be_bytes());
    }
    q.extend_from_slice(b);
    f.write_all(&q)?;
    f.flush()
}

/// Lê um quadro. **Cliente sempre mascara** — também regra do protocolo.
///
/// Devolve `None` para quadro de controle (ping, close) que não interessa aqui.
fn ler_texto(f: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut cab = [0u8; 2];
    f.read_exact(&mut cab)?;
    let opcode = cab[0] & 0x0F;
    let mascarado = cab[1] & 0x80 != 0;
    let mut tam = (cab[1] & 0x7F) as usize;
    if tam == 126 {
        let mut e = [0u8; 2];
        f.read_exact(&mut e)?;
        tam = u16::from_be_bytes(e) as usize;
    } else if tam == 127 {
        let mut e = [0u8; 8];
        f.read_exact(&mut e)?;
        tam = u64::from_be_bytes(e) as usize;
    }
    let mut mask = [0u8; 4];
    if mascarado {
        f.read_exact(&mut mask)?;
    }
    let mut dados = vec![0u8; tam];
    f.read_exact(&mut dados)?;
    if mascarado {
        for (i, b) in dados.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    // 0x1 texto. 0x8 close, 0x9 ping, 0xA pong: nao sao resposta a comando.
    if opcode == 0x1 {
        Ok(Some(String::from_utf8_lossy(&dados).into_owned()))
    } else {
        Ok(None)
    }
}

/// Responde ao `GET ... Upgrade: websocket` e devolve `true` se abriu o canal.
fn handshake(f: &mut TcpStream) -> std::io::Result<bool> {
    let mut cru = Vec::new();
    let mut buf = [0u8; 1024];
    // Le ate a linha em branco que fecha os cabecalhos.
    while !cru.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(false);
        }
        cru.extend_from_slice(&buf[..n]);
        if cru.len() > 8192 {
            return Ok(false);
        }
    }
    let texto = String::from_utf8_lossy(&cru);
    let chave = texto
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("sec-websocket-key").then(|| v.trim().to_string())
        });
    let Some(chave) = chave else { return Ok(false) };
    let aceite = base64(&sha1(format!("{chave}{GUID}").as_bytes()));
    write!(
        f,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Accept: {aceite}\r\n\r\n"
    )?;
    f.flush()?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// A ponte
// ---------------------------------------------------------------------------

type Pedido = (String, Option<String>, mpsc::Sender<Result<String, String>>);

pub struct Ponte {
    envio: mpsc::Sender<Pedido>,
}

impl Ponte {
    /// Sobe o ouvinte em `127.0.0.1:porta` e devolve a ponte.
    ///
    /// O endereço está no código, como no [`crate::servidor`]: um socket que
    /// comanda o computador do dono não pode ser exposto por engano de configuração.
    pub fn abrir(porta: u16) -> std::io::Result<Ponte> {
        let ouvinte = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, porta))?;
        let (envio, recebe) = mpsc::channel::<Pedido>();

        std::thread::spawn(move || {
            // Aceita para sempre: a extensao reconecta a cada 4s, e o Spotify pode
            // reiniciar. Uma ponte que morre na primeira queda seria pior que nao
            // existir, porque falharia depois de ja ter funcionado.
            for fluxo in ouvinte.incoming() {
                let Ok(mut f) = fluxo else { continue };
                if handshake(&mut f).unwrap_or(false) {
                    let _ = f.set_read_timeout(Some(PRAZO));
                    // A extensao manda um `hello` ao conectar. Sem consumir, TODA
                    // resposta fica deslocada em uma — foi o primeiro erro que eu
                    // cometi falando com ela.
                    let _ = ler_texto(&mut f);
                    servir(&mut f, &recebe);
                }
            }
        });
        Ok(Ponte { envio })
    }

    /// Manda um comando e espera a resposta.
    pub fn comando(&self, cmd: &str, query: Option<&str>) -> Result<String, String> {
        let (tx, rx) = mpsc::channel();
        self.envio
            .send((cmd.to_string(), query.map(str::to_string), tx))
            .map_err(|_| "a ponte caiu".to_string())?;
        rx.recv_timeout(PRAZO + Duration::from_secs(1))
            .map_err(|_| "a extensao do Spotify nao respondeu (ela esta rodando?)".to_string())?
    }
}

/// Serve pedidos enquanto a conexão viver.
fn servir(f: &mut TcpStream, recebe: &mpsc::Receiver<Pedido>) {
    let mut id = 0u64;
    while let Ok((cmd, query, responder)) = recebe.recv() {
        id += 1;
        let msg = match &query {
            Some(q) => format!(
                "{{\"cmd\":\"{}\",\"query\":\"{}\",\"id\":{id}}}",
                escapar(&cmd),
                escapar(q)
            ),
            None => format!("{{\"cmd\":\"{}\",\"id\":{id}}}", escapar(&cmd)),
        };
        if escrever_texto(f, &msg).is_err() {
            let _ = responder.send(Err("a conexao com o Spotify caiu".into()));
            return; // volta a aceitar: a extensao reconecta
        }
        // Pula quadro de controle ate achar a resposta de texto.
        let mut resposta = None;
        for _ in 0..5 {
            match ler_texto(f) {
                Ok(Some(t)) => {
                    resposta = Some(t);
                    break;
                }
                Ok(None) => continue,
                Err(_) => break,
            }
        }
        match resposta {
            Some(t) => {
                let _ = responder.send(Ok(t));
            }
            None => {
                let _ = responder.send(Err("o Spotify nao respondeu".into()));
                return;
            }
        }
    }
}

fn escapar(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' | '\r' | '\t' => vec![' '],
            c => vec![c],
        })
        .collect()
}

/// A ponte do processo, aberta na primeira vez que alguém precisa.
pub fn global() -> Result<&'static Ponte, String> {
    static PONTE: OnceLock<Option<Ponte>> = OnceLock::new();
    PONTE
        .get_or_init(|| Ponte::abrir(PORTA).ok())
        .as_ref()
        .ok_or_else(|| format!("nao consegui abrir a ponte em 127.0.0.1:{PORTA}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vetores da FIPS 180-1. Um SHA-1 errado não dá erro visível: o navegador só
    /// fecha a conexão, sem dizer por quê.
    #[test]
    fn sha1_bate_com_os_vetores_conhecidos() {
        let hex = |b: [u8; 20]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn base64_bate_com_os_vetores_conhecidos() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// O exemplo da própria RFC 6455: chave `dGhlIHNhbXBsZSBub25jZQ==` tem de
    /// produzir `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=`.
    #[test]
    fn o_handshake_produz_o_aceite_da_rfc() {
        let chave = "dGhlIHNhbXBsZSBub25jZQ==";
        let aceite = base64(&sha1(format!("{chave}{GUID}").as_bytes()));
        assert_eq!(aceite, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn escapa_aspas_e_barras_do_json() {
        assert_eq!(escapar(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escapar(r"a\b"), r"a\\b");
        assert_eq!(escapar("a\nb"), "a b");
    }

    /// A ponte tem de subir em 127.0.0.1 e em nenhum outro lugar.
    #[test]
    fn a_ponte_so_escuta_em_localhost() {
        let p = Ponte::abrir(0);
        assert!(p.is_ok(), "nao consegui subir a ponte: {:?}", p.err());
    }
}
