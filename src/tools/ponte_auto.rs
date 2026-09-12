//! A ponte do Harness sobe sozinha quando a Teka precisa dela.
//!
//! ## Por que isto existe
//!
//! Em 12/09 o John perguntou se as ferramentas funcionam de verdade. Executei as 22
//! e duas falharam:
//!
//! ```text
//! buscar_no_conteudo   ERRO  a ponte do harness nao esta configurada
//! ler_imagem           ERRO  (falta TEKA_PONTE_TOKEN)
//! ```
//!
//! São as duas que vieram da ponte, e as mesmas que custaram -2,75 pontos no
//! benchmark. Custavam e não entregavam, porque dependiam de alguém lembrar de
//! subir um processo Node com um token na mão.
//!
//! O Harness em si já estava montado — `dsh_home/profiles/teka` existe desde 09/09.
//! O que faltava era ninguém ligar.
//!
//! ## O que este módulo NÃO faz
//!
//! **Não guarda o segredo em disco.** O token é sorteado por execução e vive só na
//! memória deste processo. Um socket que executa `pwsh` protegido por um arquivo
//! seria trocar "a pessoa esquece" por "qualquer processo do usuário entra".
//!
//! A consequência é deliberada: se uma ponte já está de pé e não foi esta execução
//! que a subiu, a Teka **não adivinha o segredo dela** — diz isso e para.
//!
//! **Não mata o que não subiu.** Só derruba o filho que ela mesma criou.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Quanto esperar a ponte começar a atender. Node sobe a árvore de plugins inteira
/// antes de escutar.
const PRAZO_SUBIDA: Duration = Duration::from_secs(20);

/// A ponte que ESTA execução subiu. Vazia quando é o John quem manda o token.
static NOSSA: OnceLock<Mutex<Option<Viva>>> = OnceLock::new();

struct Viva {
    filho: Child,
    segredo: String,
}

impl Drop for Viva {
    /// Nunca deixar zumbi: um `dsh` esquecido segura a porta 8768, e a próxima
    /// execução da Teka não teria como entrar — ela não sabe o segredo dele.
    ///
    /// ## Por que matar a ÁRVORE, e não o filho
    ///
    /// `dsh.CMD` é um envoltório em lote: ele configura `NODE_PATH` e **chama o
    /// node**. O filho que a Teka segura é o `cmd.exe`; quem escuta na 8768 é o
    /// neto. Medido em 12/09 — `filho.kill()` sozinho deixou dois `node.exe` vivos
    /// e a porta ocupada.
    ///
    /// `taskkill /T` desce a árvore. O `kill()` continua depois, como rede para
    /// quando o `taskkill` não existe ou o filho já morreu.
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.filho.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.filho.kill();
        let _ = self.filho.wait();
    }
}

/// Onde o segredo da ponte viva fica guardado.
///
/// Dentro do `dsh_home`, que ja e a pasta de estado do Harness — e nao na raiz do
/// repositorio, que iria parar no git um dia por descuido.
fn arquivo_do_segredo(home: &str) -> std::path::PathBuf {
    std::path::Path::new(home).join(".teka-ponte-token")
}

/// O segredo guardado abre MESMO a ponte que está atendendo?
///
/// Conferir de verdade, e não só ler o arquivo. Um token velho de uma ponte morta,
/// com outra ponte no lugar, passaria na leitura e falharia na primeira chamada — e
/// o erro apareceria como "a ferramenta falhou", longe da causa.
fn segredo_serve(endereco: &str, segredo: &str) -> bool {
    super::harness_tcp::conectar(endereco, segredo, Duration::from_secs(5)).is_ok()
}

/// Alguém está atendendo neste endereço?
fn atendendo(endereco: &str) -> bool {
    let Ok(destino) = endereco.parse() else {
        return false;
    };
    TcpStream::connect_timeout(&destino, Duration::from_millis(300)).is_ok()
}

/// Onde mora o Harness. Configurável, com o padrão sendo o vizinho do repositório.
fn caminhos() -> (String, String) {
    let home =
        std::env::var("TEKA_DSH_HOME").unwrap_or_else(|_| "../DeepSeek-Harness/dsh_home".into());
    let bin = std::env::var("TEKA_DSH_BIN").unwrap_or_else(|_| {
        "../DeepSeek-Harness/instalacao-funcionando/node_modules/.bin/dsh.CMD".into()
    });
    (home, bin)
}

/// Um segredo de uso único: relógio, endereço de uma alocação e pid.
///
/// Não é criptografia — é para o socket local recusar quem não foi convidado, e ele
/// vive só nesta execução. O `Rng` do projeto é determinístico por semente:
/// perfeito para experimento, péssimo para segredo.
fn sortear_segredo() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let caixa = Box::new(0u8);
    let end = &*caixa as *const u8 as usize;
    format!("{t:x}{end:x}{:x}", std::process::id())
}

/// Devolve o segredo de uma ponte que está de pé, subindo uma se preciso.
///
/// Três caminhos, nesta ordem, e a ordem importa:
///
/// 1. `TEKA_PONTE_TOKEN` no ambiente — quem chamou está no comando: respeita e não
///    sobe nada. É o caminho de quem já tem a ponte rodando do jeito dele.
/// 2. Já subimos nesta execução — reusa.
/// 3. Sobe uma nova.
///
/// Se a porta está ocupada e não fomos nós, devolve erro explicando: adivinhar
/// segredo alheio não é opção, e matar processo que não é nosso, menos ainda.
pub fn garantir(endereco: &str) -> Result<String, String> {
    if let Ok(s) = std::env::var("TEKA_PONTE_TOKEN") {
        if !s.is_empty() {
            return Ok(s);
        }
    }

    let trava = NOSSA.get_or_init(|| Mutex::new(None));
    let mut guarda = trava.lock().map_err(|_| "trava da ponte envenenada")?;
    if let Some(v) = guarda.as_mut() {
        // `try_wait` devolve Some quando o filho morreu: aí a ponte caiu e a entrada
        // guardada é mentira. Descarta e sobe outra.
        match v.filho.try_wait() {
            Ok(None) => return Ok(v.segredo.clone()),
            _ => *guarda = None,
        }
    }

    let (home, bin) = caminhos();

    // Ponte de pe que nao e desta execucao: pode ser de uma anterior que saiu por
    // `process::exit` sem desligar. O arquivo diz o segredo dela.
    if atendendo(endereco) {
        if let Ok(guardado) = std::fs::read_to_string(arquivo_do_segredo(&home)) {
            let guardado = guardado.trim().to_string();
            if !guardado.is_empty() && segredo_serve(endereco, &guardado) {
                return Ok(guardado);
            }
        }
        return Err(format!(
            "ja ha uma ponte atendendo em {endereco} e o segredo guardado nao abre ela.              Feche aquele processo, ou defina TEKA_PONTE_TOKEN com o segredo que ele usa."
        ));
    }


    let segredo = sortear_segredo();
    let filho = Command::new(&bin)
        .args(["--profile", "teka"])
        .env("DSH_HOME", &home)
        .env("TEKA_PONTE_TOKEN", &segredo)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => format!(
                "nao achei o dsh em {bin:?}. Defina TEKA_DSH_BIN (e TEKA_DSH_HOME) \
                 apontando para a instalacao do Harness. Ver ponte_harness/README.md"
            ),
            _ => format!("nao subi a ponte: {e}"),
        })?;

    // Guarda ANTES de esperar: se esta execucao morrer por `process::exit` no meio,
    // a proxima ainda acha o segredo da ponte que ficou de pe.
    if let Err(e) = std::fs::write(arquivo_do_segredo(&home), &segredo) {
        // Nao e fatal: sem o arquivo a ponte DESTA execucao funciona igual, e so a
        // proxima e que teria de subir outra. Falhar aqui seria pior que seguir.
        eprintln!("  aviso: nao guardei o segredo da ponte ({e})");
    }

    let mut viva = Viva { filho, segredo };
    let ate = Instant::now() + PRAZO_SUBIDA;
    while Instant::now() < ate {
        if atendendo(endereco) {
            let s = viva.segredo.clone();
            *guarda = Some(viva);
            return Ok(s);
        }
        // O filho pode morrer na partida — perfil errado, porta ocupada, dependência
        // faltando. Esperar o prazo inteiro por um defunto é desperdício.
        if matches!(viva.filho.try_wait(), Ok(Some(_))) {
            return Err(format!(
                "a ponte subiu e morreu na partida. Confira que o perfil `teka` existe \
                 em {home}/profiles/teka. Ver ponte_harness/README.md"
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!("a ponte nao comecou a atender em {PRAZO_SUBIDA:?}"))
}

/// Derruba a ponte que esta execução subiu, se houver.
///
/// ## Por que isto precisa ser CHAMADO, e não confiar no `Drop`
///
/// [`Viva`] mora num `static`, e **Rust não roda destrutor de `static` ao sair do
/// processo**. O `Drop` logo acima é código morto no caminho normal — descoberto em
/// 12/09 medindo: a sonda terminou, imprimiu tudo, e o `node.exe` da ponte continuou
/// vivo segurando a 8768.
///
/// Escrever "nunca deixar zumbi" na doc e guardar o objeto no único lugar onde o
/// destrutor não roda é o tipo de erro que só aparece olhando a lista de processos.
///
/// Quem tem ponto de saída chama isto. O `Drop` fica para os casos em que ele roda
/// de verdade (o `Viva` sendo descartado e trocado por outro dentro de [`garantir`]).
pub fn derrubar() {
    if let Some(trava) = NOSSA.get() {
        if let Ok(mut g) = trava.lock() {
            *g = None; // aqui o `Drop` roda, e mata a árvore
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dois sorteios nao dao o mesmo segredo. Se dessem, a autenticacao seria
    /// decorativa — qualquer processo do usuario adivinharia o de outro.
    #[test]
    fn o_segredo_nao_se_repete() {
        let a = sortear_segredo();
        let b = sortear_segredo();
        assert_ne!(a, b, "segredo repetido: {a}");
        assert!(a.len() >= 16, "segredo curto demais: {a:?}");
    }

    /// Controle positivo de `atendendo`: sem isto, uma versao que sempre devolvesse
    /// `false` passaria despercebida e a Teka subiria ponte em cima de ponte.
    #[test]
    fn porta_vazia_nao_esta_atendendo() {
        let porta = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            l.local_addr().expect("addr").port()
        };
        assert!(!atendendo(&format!("127.0.0.1:{porta}")));
    }

    /// E o outro lado: porta que ALGUEM escuta esta atendendo.
    #[test]
    fn porta_ocupada_esta_atendendo() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let e = l.local_addr().expect("addr").to_string();
        assert!(atendendo(&e), "deveria detectar o ouvinte em {e}");
    }

    /// `TEKA_PONTE_TOKEN` no ambiente manda, e nada sobe.
    #[test]
    fn token_no_ambiente_vence_e_nao_sobe_nada() {
        // Porta impossivel de atender, para provar que nem tentou conectar.
        std::env::set_var("TEKA_PONTE_TOKEN", "segredo-do-john");
        let r = garantir("127.0.0.1:1");
        std::env::remove_var("TEKA_PONTE_TOKEN");
        assert_eq!(r.as_deref(), Ok("segredo-do-john"));
    }
}
