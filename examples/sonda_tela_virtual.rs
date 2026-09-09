//! Da para MANDAR TECLA para um desktop escondido?
//!
//! No Windows o foco e por DESKTOP: cada um tem a propria fila de entrada e o
//! proprio conceito de "janela em foco". `SendInput` cai no foco do desktop AO QUAL
//! A THREAD ESTA ATACHADA.
//!
//! A `tela.rs` ja lanca processo num desktop escondido, mas nunca chamou
//! `SetThreadDesktop` — ela sabe esconder e nao sabe dirigir. Se isto funcionar,
//! abre a porta para rodar um jogo num desktop invisivel e controla-lo sem tocar na
//! tela do John. Se nao funcionar, aquela ideia morre aqui, barato.
//!
//! ## Por que Bloco de Notas e nao o jogo
//!
//! Eu nao enxergo a tela, e desktop escondido ninguem ve. O Bloco de Notas resolve:
//! mando texto e LEIO DE VOLTA pela arvore de acessibilidade — a verificacao nao
//! depende de ninguem olhar.
//!
//! E separa as duas perguntas. Esta sonda responde "o mecanismo de ENTRADA
//! funciona?". Se um jogo pesado RENDERIZA num desktop invisivel e outra pergunta, e
//! so vale gastar tempo nela se esta der certo.
//!
//! ## MEDIDO EM 2026-09-09 — A IDEIA MORREU AQUI
//!
//! ```text
//! 0. tela criada                    OK
//! 1. Bloco de Notas lancado nela    OK
//! 2. thread atachada no desktop     OK
//! 3. SendInput                      0 de 2, GetLastError = 5 (ACCESS_DENIED)
//! ```
//!
//! `SendInput` so e aceito quando a thread esta no **desktop de ENTRADA** — o que
//! recebe o teclado fisico. Desktop escondido nao e, entao a injecao e recusada.
//!
//! Para ele virar o desktop de entrada seria preciso `SwitchDesktop`, que TOMA A
//! TELA do usuario — exatamente o que a ideia tentava evitar. Nao ha meio-termo:
//! ou o desktop recebe entrada e esta visivel, ou esta escondido e nao recebe.
//!
//! **Consequencia:** tela virtual serve para ESCONDER janela (o uso atual da Teka)
//! e nao serve para DIRIGIR nada escondido. A ideia de rodar o jogo num desktop
//! invisivel e controla-lo esta encerrada.
//!
//! Erro meu no caminho, registrado porque custou duas rodadas: `lancar` so USA o
//! nome do desktop; quem CRIA e `TelaVirtual::nova`. Sem criar, o processo sobe no
//! desktop normal e a sonda mede a coisa errada.
//!
//!   cargo run --release --example sonda_tela_virtual
#[cfg(not(windows))]
fn main() {
    println!("so faz sentido no Windows");
}

#[cfg(windows)]
fn main() {
    const TELA: &str = "teka_sonda";
    const TEXTO: &str = "teka esteve aqui";

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenDesktopW(nome: *const u16, flags: u32, herdar: i32, acesso: u32) -> isize;
        fn SetThreadDesktop(h: isize) -> i32;
        fn GetThreadDesktop(tid: u32) -> isize;
        fn CloseDesktop(h: isize) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetLastError() -> u32;
    }
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };

    println!();
    println!("  0. criando a tela {TELA:?}");
    // `lancar` so USA o nome; quem CRIA e `TelaVirtual::nova`. Sem isto o processo
    // sobe no desktop NORMAL e a sonda mede a coisa errada — foi o que aconteceu na
    // primeira tentativa, e o sintoma foi `OpenDesktopW` devolvendo 0 depois.
    //
    // E a tela tem de ficar viva ate o fim: o `Drop` dela fecha o handle.
    let _tela = match teka::tools::tela::TelaVirtual::nova(TELA) {
        Ok(t) => t,
        Err(e) => {
            println!("     FALHOU ao criar: {e}");
            return;
        }
    };
    println!("     criada");

    println!("  1. lancando o Bloco de Notas nela");
    let lancado = match teka::tools::tela::lancar("notepad.exe", &[], Some(TELA), None) {
        Ok(l) => l,
        Err(e) => {
            println!("     FALHOU: {e}");
            return;
        }
    };
    println!("     pid {}", lancado.pid);
    std::thread::sleep(std::time::Duration::from_millis(2500));

    println!("  2. atachando ESTA thread naquele desktop");
    // DESKTOP_READOBJECTS | DESKTOP_CREATEWINDOW | DESKTOP_WRITEOBJECTS |
    // DESKTOP_ENUMERATE | DESKTOP_SWITCHDESKTOP
    const ACESSO: u32 = 0x0001 | 0x0002 | 0x0080 | 0x0040 | 0x0100;
    let anterior = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    let h = unsafe { OpenDesktopW(w(TELA).as_ptr(), 0, 0, ACESSO) };
    if h == 0 {
        println!("     FALHOU: OpenDesktopW erro {}", unsafe { GetLastError() });
        return;
    }
    if unsafe { SetThreadDesktop(h) } == 0 {
        println!("     FALHOU: SetThreadDesktop erro {}", unsafe { GetLastError() });
        unsafe { CloseDesktop(h) };
        return;
    }
    println!("     atachada");

    println!("  3. digitando {TEXTO:?} por SendInput");
    // Codigo virtual direto: letra minuscula sem shift vira o VK da maiuscula, e
    // espaco e 0x20. Nao vale API publica nova para uma sonda.
    // Chamada DIRETA para capturar o `GetLastError`: o envoltorio da Teka reporta
    // "aceitou 0 de N" e engole o motivo, e o motivo e a coisa toda aqui.
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Ent { tipo: u32, _pad: u32, vk: u16, scan: u16, flags: u32, tempo: u32, extra: usize, _resto: [u8; 8] }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SendInput(n: u32, e: *const Ent, tam: i32) -> u32;
    }
    for c in TEXTO.chars().take(1) {
        let vk: u16 = if c == ' ' { 0x20 } else { c.to_ascii_uppercase() as u16 };
        let ents = [
            Ent { tipo: 1, vk, ..Default::default() },
            Ent { tipo: 1, vk, flags: 0x0002, ..Default::default() },
        ];
        let n = unsafe { SendInput(2, ents.as_ptr(), std::mem::size_of::<Ent>() as i32) };
        let erro = unsafe { GetLastError() };
        println!("     SendInput aceitou {n} de 2, GetLastError = {erro}");
        if erro == 5 {
            println!("     (5 = ACCESS_DENIED — o desktop nao e o de ENTRADA)");
        }
    }
    let _ = teka::tools::teclado::mandar_teclas("nada", &[]);
    std::thread::sleep(std::time::Duration::from_millis(500));

    println!("  4. lendo de volta pela arvore de acessibilidade");
    match teka::tools::uia::listar("notepad.exe", 0, "") {
        Ok(v) => {
            let achou = v.iter().any(|n| n.contains(TEXTO));
            println!("     {} controles na arvore daquele desktop", v.len());
            println!();
            if achou {
                println!("  RESULTADO: FUNCIONA — a tecla chegou no desktop escondido");
            } else {
                println!("  RESULTADO: NAO — a tecla nao chegou, ou a leitura nao alcanca");
                for n in v.iter().filter(|n| !n.trim().is_empty()).take(6) {
                    println!("       viu: {n:?}");
                }
            }
        }
        Err(e) => println!("     erro ao ler: {e}"),
    }

    // Volta a thread e derruba o processo, sempre.
    if anterior != 0 {
        unsafe { SetThreadDesktop(anterior) };
    }
    unsafe { CloseDesktop(h) };
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &lancado.pid.to_string(), "/F"])
        .output();
    println!("  5. bloco de notas encerrado, thread de volta ao desktop normal");
}
