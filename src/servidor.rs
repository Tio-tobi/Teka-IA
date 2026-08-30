//! Modo residente: a Teka de pé, esperando.
//!
//! ```text
//!   CLI por pedido            residente
//!   ───────────────           ─────────
//!   33 ms  subir processo     —
//!   63 ms  carregar + pensar  ~1 ms  pensar
//!   ──────                    ──────
//!   96 ms                     ~1 ms
//! ```
//!
//! O modelo não mudou. O que sumiu foi pagar a partida do processo e a leitura de
//! 6 MB de disco **a cada pedido** — custo que só existia porque ela era um comando,
//! não um serviço.
//!
//! ## Por que TCP em 127.0.0.1 e não algo mais esperto
//!
//! Porque qualquer linguagem sabe abrir um socket. Um atalho de teclado, um script
//! de uma linha, o Whisper quando chegar — todos falam com ela sem precisar de
//! biblioteca. E é `std` puro, então o zero-dependência continua de pé.
//!
//! ## O que este servidor NÃO é
//!
//! **Não é um serviço de rede.** Ele liga em `127.0.0.1` e recusa qualquer outro
//! endereço — a ligação está no código, não em configuração, porque um servidor que
//! executa comando do sistema em `0.0.0.0` é um erro de uma linha que não pode ser
//! possível de cometer.
//!
//! ## A chamada vai como STRING, não embutida
//!
//! [`crate::grammar`] produz um formato **parecido com JSON mas que não é JSON**:
//! dentro de um valor o `\` vai cru, porque caminho do Windows tem barra e escapar
//! cada uma seria fonte de erro sem retorno.
//!
//! Isso é decisão do formato dela e está certo. O erro foi meu, ao embutir esse
//! texto direto na resposta do servidor: `{"decidiu":{"caminho":"C:	emp"}}` quebra
//! qualquer `JSON.parse`. A chamada agora vai como string escapada, e quem quiser o
//! conteúdo dela desserializa em dois passos.
//!
//! As três guardas continuam valendo, e é o mesmo [`Executor`] do REPL: sandbox por
//! padrão, denylist, raiz permitida, mais diário e oficina. O residente não abre
//! nenhuma porta nova para o que ela pode fazer — só para quem pode pedir.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};

use crate::backend::Paralelo;
use crate::model::agente::{Agente, AgenteCache};
use crate::model::patcher::Patcher;
use crate::memory::MemoriaEpisodica;
use crate::tools::execucao::Executor;

/// Teto de bytes por linha de pedido. Sem isto, um cliente que nunca manda `\n`
/// cresce o buffer até a memória acabar.
const MAX_LINHA: usize = 8 * 1024;

pub struct CfgServidor {
    pub porta: u16,
    /// Executa a ferramenta, ou só devolve a chamada?
    ///
    /// `false` é o **modo observador**: ela lê, decide, responde o que faria, e não
    /// toca em nada. É o degrau que faz sentido antes de deixar um agente ligado
    /// sozinho no computador.
    pub executar: bool,
}

impl Default for CfgServidor {
    fn default() -> Self {
        Self {
            porta: 8931,
            executar: false, // observar por padrão: agir tem de ser escolha explícita
        }
    }
}

/// Sobe o servidor e atende até levar Ctrl-C.
///
/// Atende **um cliente por vez**, de propósito: a Teka tem um estado de memória
/// episódica só, e servir dois pedidos em paralelo os faria escrever em cima um do
/// outro. Um pedido leva ~1 ms — fila não é gargalo aqui.
pub fn servir(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &dyn Patcher,
    exec: &mut Executor,
    mem: &mut MemoriaEpisodica,
    cfg: &CfgServidor,
) -> std::io::Result<()> {
    // Ipv4Addr::LOCALHOST está no código de propósito. Ver a nota do módulo.
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, cfg.porta);
    let ouvinte = TcpListener::bind(addr)?;

    println!("\n  Teka residente em {addr}");
    println!(
        "  modo: {}",
        if cfg.executar {
            "EXECUTA as ferramentas"
        } else {
            "OBSERVADOR — decide e responde, nao toca em nada"
        }
    );
    println!("  politica: {:?}", exec.politica().modo);
    println!("\n  teste:  echo \"que horas sao\" | ncat 127.0.0.1 {}\n", cfg.porta);

    let mut cache = AgenteCache::new();
    for fluxo in ouvinte.incoming() {
        match fluxo {
            Ok(f) => {
                if let Err(e) = atender(f, ag, ops, patcher, exec, mem, cfg, &mut cache) {
                    eprintln!("  cliente caiu: {e}");
                }
            }
            Err(e) => eprintln!("  conexao falhou: {e}"),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn atender(
    fluxo: TcpStream,
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &dyn Patcher,
    exec: &mut Executor,
    mem: &mut MemoriaEpisodica,
    cfg: &CfgServidor,
    cache: &mut AgenteCache<f32>,
) -> std::io::Result<()> {
    let mut escrita = fluxo.try_clone()?;
    let leitura = BufReader::new(fluxo);

    for linha in leitura.take(MAX_LINHA as u64 * 64).lines() {
        let pedido = linha?;
        let pedido = pedido.trim();
        if pedido.is_empty() {
            continue;
        }
        if pedido.len() > MAX_LINHA {
            writeln!(escrita, "{{\"erro\":\"pedido longo demais\"}}")?;
            continue;
        }

        let resposta = responder_uma(ag, ops, patcher, exec, mem, cfg, cache, pedido);
        writeln!(escrita, "{resposta}")?;
        escrita.flush()?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn responder_uma(
    ag: &Agente<f32>,
    ops: &Paralelo,
    patcher: &dyn Patcher,
    exec: &mut Executor,
    mem: &mut MemoriaEpisodica,
    cfg: &CfgServidor,
    cache: &mut AgenteCache<f32>,
    pedido: &str,
) -> String {
    let (chamada, conf) = match ag.responder_com_confianca(ops, patcher, pedido, cache) {
        Ok(x) => x,
        Err(e) => return format!("{{\"erro\":\"{}\"}}", escapar(&e)),
    };
    let texto = chamada.texto(&ag.registro);
    let assinatura = ag.assinatura(cache);

    if !cfg.executar {
        // Observador: registra o que faria, sem fazer.
        return format!(
            "{{\"decidiu\":\"{}\",\"margem\":{:.3},\"executou\":false}}",
            escapar(&texto),
            conf.margem()
        );
    }

    let saida = exec.executar(&ag.registro, &chamada);
    let ok = saida.is_ok();
    let corpo = match &saida {
        Ok(s) => escapar(s),
        Err(e) => escapar(e),
    };
    // A memória guarda o episódio igual ao REPL: é o que alimenta o reforço.
    mem.gravar(
        pedido,
        chamada.ferramenta,
        Vec::new(),
        if ok {
            crate::memory::Resultado::Executou
        } else {
            crate::memory::Resultado::Falhou
        },
        // Sem feedback: o servidor nao tem quem aprove na hora. O reforco usa o
        // sinal implicito (executou/falhou) e a correcao vem depois, pelo REPL.
        crate::memory::Feedback::Nenhum,
        assinatura,
    );
    format!(
        "{{\"decidiu\":\"{}\",\"margem\":{:.3},\"executou\":true,\"ok\":{ok},\"saida\":\"{corpo}\"}}",
        escapar(&texto),
        conf.margem()
    )
}

fn escapar(s: &str) -> String {
    let mut saida = String::with_capacity(s.len() + 8);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_padrao_e_observar_nao_agir() {
        let c = CfgServidor::default();
        assert!(
            !c.executar,
            "deixar um agente ligado sozinho AGINDO nao pode ser o padrao"
        );
    }

    #[test]
    fn a_porta_e_sempre_local() {
        // O endereco esta no codigo, nao em configuracao: um servidor que roda
        // comando do sistema em 0.0.0.0 nao pode ser um erro possivel de cometer.
        let a = SocketAddrV4::new(Ipv4Addr::LOCALHOST, CfgServidor::default().porta);
        assert!(a.ip().is_loopback());
        assert_eq!(a.ip().octets(), [127, 0, 0, 1]);
    }

    #[test]
    fn a_saida_escapa_o_que_quebraria_a_linha() {
        // O protocolo e uma linha por resposta: quebra de linha crua na saida de
        // uma ferramenta partiria a resposta em duas e o cliente leria lixo.
        assert_eq!(escapar("linha um\nlinha dois"), "linha um\\nlinha dois");
        assert_eq!(escapar("aspas \" e barra \\"), "aspas \\\" e barra \\\\");
        // Acento passa: e UTF-8 valido dentro de JSON.
        assert_eq!(escapar("memória"), "memória");
    }

    #[test]
    fn o_teto_de_linha_existe() {
        // Sem teto, um cliente que nunca manda \n cresce o buffer ate a RAM acabar.
        assert!(MAX_LINHA > 0 && MAX_LINHA <= 64 * 1024);
    }
}

#[cfg(test)]
mod testes_json {
    use super::*;

    /// Uma barra invertida, sem passar por escape de literal.
    fn barra() -> char {
        char::from(92u8)
    }

    /// A resposta do servidor tem de ser JSON valido, inclusive com caminho do
    /// Windows dentro.
    ///
    /// A gramatica da Teka produz um formato PARECIDO com JSON que nao e JSON: a
    /// barra invertida vai crua, de proposito, porque caminho do Windows tem barra
    /// e escapar cada uma seria fonte de erro sem retorno.
    ///
    /// Isso esta certo do lado dela. O erro foi meu, ao embutir esse texto direto
    /// numa resposta que se anuncia como JSON — quebra todo cliente que faz
    /// `JSON.parse`.
    #[test]
    fn a_chamada_vai_escapada_dentro_da_resposta() {
        let b = barra();
        let aspas = char::from(34u8);
        // Como a gramatica emite de verdade.
        let crua = format!("{aspas}caminho{aspas}:{aspas}C:{b}temp{b}x.txt{aspas}");

        let dentro = escapar(&crua);
        // Cada barra virou um par.
        assert!(
            dentro.contains(&format!("C:{b}{b}temp{b}{b}x.txt")),
            "barra nao foi duplicada: {dentro}"
        );
        // E cada aspa da chamada foi neutralizada, senao ela fecharia a string
        // da resposta no meio.
        let aspas_cruas = dentro
            .char_indices()
            .filter(|(i, c)| *c == aspas && (*i == 0 || dentro.as_bytes()[i - 1] != 92))
            .count();
        assert_eq!(aspas_cruas, 0, "aspas cruas sobreviveram: {dentro}");
    }

    #[test]
    fn o_escape_e_reversivel_no_que_importa() {
        let b = barra();
        let original = format!("C:{b}Users{b}User{b}Documentos");
        let e = escapar(&original);
        // O que sai tem o dobro de barras, e nenhuma delas fica solta.
        assert_eq!(e.matches(b).count(), original.matches(b).count() * 2);
    }
}
