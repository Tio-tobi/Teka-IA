//! Sobe o Harness como processo filho e liga os canos.
//!
//! Terceira peça da ponte. A primeira foi o JSON (`crate::json`), a segunda o
//! protocolo em memória (`super::harness`), e esta é a que encosta no sistema
//! operacional.
//!
//! ## Três armadilhas, e nenhuma é hipotética
//!
//! **1. stderr entupido trava o filho.** Se stderr é um cano e ninguém lê, o buffer
//! do sistema enche (~64 KB) e o filho **bloqueia na escrita** — para sempre, em
//! silêncio, parecendo lentidão. Por isso stderr é drenado numa thread desde o
//! primeiro instante. O `falso_harness barulhento` existe para provar isto.
//!
//! **2. Sem prazo, um servidor mudo trava a Teka.** `read_line` num cano espera
//! indefinidamente. `LinhasComPrazo` transforma silêncio em erro, que o chamador
//! trata. `falso_harness mudo` prova.
//!
//! **3. Filho não morre sozinho.** Se a Teka cair sem encerrar, sobra um Node vivo
//! segurando porta e memória. O `Drop` tenta `shutdown`, e mata se não colaborar.
//!
//! ## O que fica guardado do stderr, e por quê
//!
//! As últimas linhas, não todas. Quando o Harness não sobe, o motivo está lá — a doc
//! deles diz que a falta do `cordis.yml` imprime uso no stderr e sai 1. Sem guardar,
//! a Teka diria só "EOF" e a pessoa ficaria sem saber por quê.

use super::harness::Protocolo;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Quanto esperar por uma linha antes de chamar de silêncio.
///
/// Generoso de propósito: `initialize` espera a árvore de plugins do Harness
/// assentar, e descoberta de MCP pode demorar. Um prazo curto transformaria partida
/// lenta em falha.
pub const PRAZO_PADRAO: Duration = Duration::from_secs(30);

/// Quantas linhas de stderr guardar para diagnóstico.
pub const LINHAS_DE_ERRO: usize = 50;

/// Um `BufRead` que desiste em vez de esperar para sempre.
///
/// A leitura de verdade acontece numa thread, que empurra linha por linha para um
/// canal. Aqui só se espera com prazo. Estouro de prazo vira `TimedOut`, e canal
/// desconectado vira fim de arquivo — que é como `read_line` sinaliza EOF.
pub struct LinhasComPrazo {
    rx: Receiver<std::io::Result<String>>,
    prazo: Duration,
    buf: Vec<u8>,
    pos: usize,
    acabou: bool,
}

impl LinhasComPrazo {
    fn novo(rx: Receiver<std::io::Result<String>>, prazo: Duration) -> Self {
        Self { rx, prazo, buf: Vec::new(), pos: 0, acabou: false }
    }
}

impl Read for LinhasComPrazo {
    fn read(&mut self, alvo: &mut [u8]) -> std::io::Result<usize> {
        let disponivel = self.fill_buf()?;
        let n = disponivel.len().min(alvo.len());
        alvo[..n].copy_from_slice(&disponivel[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for LinhasComPrazo {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.pos >= self.buf.len() && !self.acabou {
            match self.rx.recv_timeout(self.prazo) {
                Ok(Ok(linha)) => {
                    self.buf = linha.into_bytes();
                    self.pos = 0;
                }
                Ok(Err(e)) => return Err(e),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("o harness ficou {:?} sem falar", self.prazo),
                    ));
                }
                // Canal fechado: a thread leitora viu o fim. Buffer vazio e EOF.
                Err(RecvTimeoutError::Disconnected) => {
                    self.acabou = true;
                    self.buf.clear();
                    self.pos = 0;
                }
            }
        }
        Ok(&self.buf[self.pos.min(self.buf.len())..])
    }

    fn consume(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.buf.len());
    }
}

/// Um Harness vivo, com os canos ligados.
pub struct Processo {
    filho: Child,
    pub protocolo: Protocolo<LinhasComPrazo, ChildStdin>,
    erros: Arc<Mutex<Vec<String>>>,
}

impl Processo {
    /// Sobe o binário e liga os canos. Não fala nada ainda.
    ///
    /// `config` é o `cordis.yml`; o bin deles aceita por `argv[2]` ou pela variável
    /// `DSH_CORDIS_CONFIG`, e **sai 1 se o arquivo não existir** — sem procurar em
    /// lugar nenhum. Passar aqui e deixar o erro aparecer é melhor que adivinhar.
    pub fn abrir(bin: &str, config: Option<&str>, prazo: Duration) -> Result<Self, String> {
        let mut cmd = Command::new(bin);
        if let Some(c) = config {
            cmd.arg(c);
        }
        let mut filho = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("nao subi {bin:?}: {e}"))?;

        let entrada = filho.stdin.take().ok_or("sem stdin no filho")?;
        let saida = filho.stdout.take().ok_or("sem stdout no filho")?;
        let erro = filho.stderr.take().ok_or("sem stderr no filho")?;

        // DRENAR STDERR SEMPRE, desde ja. Ver a nota do modulo: cano cheio trava o
        // filho, e o sintoma parece lentidao em vez de impasse.
        let erros = Arc::new(Mutex::new(Vec::new()));
        {
            let erros = Arc::clone(&erros);
            std::thread::spawn(move || {
                for l in BufReader::new(erro).lines().map_while(Result::ok) {
                    if let Ok(mut v) = erros.lock() {
                        if v.len() >= LINHAS_DE_ERRO {
                            v.remove(0);
                        }
                        v.push(l);
                    }
                }
            });
        }

        // E stdout numa thread tambem, para o prazo poder existir.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut leitor = BufReader::new(saida);
            loop {
                let mut linha = String::new();
                match leitor.read_line(&mut linha) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(Ok(linha)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        Ok(Self {
            filho,
            protocolo: Protocolo::novo(LinhasComPrazo::novo(rx, prazo), entrada),
            erros,
        })
    }

    /// O que o Harness escreveu no stderr até agora.
    ///
    /// É onde mora o motivo quando ele não sobe: falta de `cordis.yml`, plugin que
    /// não carrega, porta ocupada.
    pub fn diagnostico(&self) -> Vec<String> {
        self.erros.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Aperta a mão e confere com quem está falando.
    pub fn iniciar(&mut self) -> Result<String, String> {
        super::harness::metodos::initialize(&mut self.protocolo).map_err(|e| {
            let d = self.diagnostico();
            if d.is_empty() {
                e
            } else {
                format!("{e}\n  stderr do harness:\n    {}", d.join("\n    "))
            }
        })
    }

    /// O identificador do processo, para quem precisar conferir que ele morreu.
    pub fn pid(&self) -> u32 {
        self.filho.id()
    }

    /// Encerra com jeito: pede `shutdown` e espera. Mata se não colaborar.
    pub fn encerrar(&mut self) -> Result<(), String> {
        let pedido = super::harness::metodos::shutdown(&mut self.protocolo);
        // Independente do que ele respondeu, o processo tem de acabar.
        match self.filho.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.filho.kill();
                let _ = self.filho.wait();
            }
        }
        pedido
    }
}

impl Drop for Processo {
    /// Nunca deixar zumbi.
    ///
    /// Se a Teka cair no meio, sobra um Node vivo segurando memoria e porta. `Drop`
    /// e a unica garantia que existe aqui — encerrar na mao e o caminho feliz, e o
    /// caminho feliz nao e o unico.
    fn drop(&mut self) {
        if matches!(self.filho.try_wait(), Ok(None)) {
            let _ = self.filho.kill();
            let _ = self.filho.wait();
        }
    }
}
