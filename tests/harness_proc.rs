//! A camada de PROCESSO da ponte com o Harness, testada contra um Harness de mentira.
//!
//! Estes testes vivem aqui e não dentro do módulo por uma razão de ferramenta:
//! `CARGO_BIN_EXE_falso_harness` só existe em teste de integração. E acabou sendo o
//! lugar certo mesmo — eles sobem processo, ligam cano e conferem que ninguém ficou
//! zumbi. Isso não é teste de unidade.
//!
//! O `falso_harness` (`src/bin/`) existe para isto ser verificável **sem Node, sem
//! `pnpm install` e sem um `cordis.yml` montado**. Teste que precisa de ambiente é
//! teste que não roda, e teste que não roda é o mesmo que não existir.

use std::time::Duration;
use teka::json::Json;
use teka::tools::harness::metodos;
use teka::tools::harness_proc::{Processo, LINHAS_DE_ERRO, PRAZO_PADRAO};

fn falso() -> &'static str {
    env!("CARGO_BIN_EXE_falso_harness")
}

fn subir(modo: Option<&str>, prazo: Duration) -> Processo {
    Processo::abrir(falso(), modo, prazo).expect("o falso harness tinha de subir")
}

#[test]
fn aperta_a_mao_e_confere_com_quem_falou() {
    let mut p = subir(None, PRAZO_PADRAO);
    assert_eq!(p.iniciar().unwrap(), "deepseek-harness-sdk-runtime");
    p.encerrar().ok();
}

/// O que motivou a ponte inteira: `session/prompt` devolve o id NA HORA, e os fatos
/// chegam depois, em notificação.
///
/// Um cliente escrito supondo resposta síncrona esperaria para sempre. O falso
/// harness manda as duas notificações ANTES da resposta justamente para provar isso.
#[test]
fn o_prompt_devolve_id_na_hora_e_os_fatos_vem_em_notificacao() {
    let mut p = subir(None, PRAZO_PADRAO);
    p.iniciar().unwrap();

    let id = metodos::prompt(&mut p.protocolo, "s-1", "lista os arquivos").unwrap();
    assert_eq!(id, "m-1");

    let vindas = p.protocolo.colher();
    assert_eq!(vindas.len(), 2, "as duas notificacoes tinham de chegar");
    assert_eq!(vindas[0].get("method").and_then(Json::texto), Some("session.status"));
    assert_eq!(vindas[1].get("params.kind").and_then(Json::texto), Some("tool"));
    p.encerrar().ok();
}

/// Servidor mudo vira erro de prazo, e não Teka travada para sempre.
#[test]
fn servidor_mudo_estoura_o_prazo_em_vez_de_travar() {
    let mut p = subir(Some("mudo"), Duration::from_millis(300));
    let e = p.iniciar().unwrap_err();
    assert!(e.contains("sem falar"), "erro inesperado: {e}");
}

/// stderr é drenado, então cano cheio não trava o filho.
///
/// **Esta é a armadilha número 1 do módulo, com prova.** Sem a thread de drenagem, o
/// `falso_harness barulhento` bloqueia na escrita depois de encher o cano (~64 KB) e
/// este teste ficaria pendurado até o prazo — o sintoma pareceria lentidão, não
/// impasse.
#[test]
fn stderr_cheio_nao_trava_o_filho() {
    let mut p = subir(Some("barulhento"), Duration::from_secs(20));
    assert_eq!(p.iniciar().unwrap(), "deepseek-harness-sdk-runtime");
    let d = p.diagnostico();
    assert!(!d.is_empty(), "o diagnostico tinha de ter sido capturado");
    assert!(d.len() <= LINHAS_DE_ERRO, "guardou {} linhas, o teto e {LINHAS_DE_ERRO}", d.len());
    p.encerrar().ok();
}

/// Processo que morre na partida vira erro legível, e não espera.
#[test]
fn morte_na_partida_vira_erro_e_nao_espera() {
    let mut p = subir(Some("morre"), Duration::from_secs(5));
    let e = p.iniciar().unwrap_err();
    assert!(e.contains("EOF"), "erro inesperado: {e}");
}

/// Lixo no stdout deles não derruba a Teka.
///
/// A doc do Harness diz que stdout é só protocolo. Mas "não deveria acontecer" não é
/// garantia, e um `console.log` alheio não pode virar pânico aqui.
#[test]
fn lixo_no_stdout_vira_erro() {
    let mut p = subir(Some("lixo"), Duration::from_secs(5));
    let e = p.iniciar().unwrap_err();
    assert!(e.contains("quadro invalido"), "erro inesperado: {e}");
}

/// `Drop` não deixa zumbi, mesmo quando ninguém chamou `encerrar`.
///
/// Se a Teka cair no meio de uma sessão, sobra um Node vivo segurando memória e
/// porta. O `Drop` é a única garantia que existe — o caminho feliz não é o único.
#[test]
fn drop_mata_o_filho_que_ficou() {
    let pid = {
        let mut p = subir(Some("mudo"), Duration::from_millis(200));
        let pid = p.pid();
        let _ = p.iniciar();
        pid
    };
    std::thread::sleep(Duration::from_millis(500));

    #[cfg(windows)]
    {
        let saida = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .expect("tasklist");
        let texto = String::from_utf8_lossy(&saida.stdout);
        assert!(
            !texto.contains(&pid.to_string()),
            "o pid {pid} continuou vivo depois do Drop:\n{texto}"
        );
    }
    #[cfg(not(windows))]
    let _ = pid;
}
