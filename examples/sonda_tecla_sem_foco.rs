//! Da para mandar tecla para um JOGO sem roubar o foco?
//!
//! ## A pergunta
//!
//! Discord e Spotify aceitam tecla em segundo plano porque sao Chromium: eles leem
//! a FILA DE MENSAGENS da janela, e `PostMessage` entrega ali. Jogo costuma ler
//! raw input / polling direto do dispositivo, e ignorar a fila.
//!
//! Isso e HIPOTESE minha, nao fato medido. Esta semana eu afirmei duas vezes coisa
//! parecida sobre o Spotify e errei nas duas.
//!
//! ## O desenho, e por que tem duas partes
//!
//! Negativo sem controle positivo nao vale nada: se nada acontecer, pode ser que
//! `PostMessage` nao funcione — ou que meu codigo de tecla esteja errado.
//!
//! ```text
//! controle   janela EM FOCO, por SendInput      tem de funcionar
//! teste      janela SEM FOCO, por PostMessage   e o que eu quero saber
//! ```
//!
//! ## MEDIDO EM 2026-09-09 — Minecraft 1.21.5 modado, `javaw.exe`
//!
//! ```text
//! CONTROLE   F3 por SendInput, EM foco      -> a tela de debug apareceu     OK
//! TESTE      F3 por PostMessage, SEM foco   -> nao sumiu                    NAO
//! ```
//!
//! O primeiro `teste` caiu no confundimento previsto: single-player PAUSA ao perder
//! o foco, e ai "nada aconteceu" tinha tres explicacoes (a pausa, a tela de menu, ou
//! o `PostMessage`). Resolvido abrindo o mundo em LAN — vira servidor integrado e o
//! jogo para de pausar. O titulo confirmou: "Multijogador (LAN)".
//!
//! `PostMessage` devolveu 1 nas duas mensagens: elas ENTRARAM NA FILA. O jogo
//! simplesmente nao le a fila. LWJGL pega entrada bruta do dispositivo.
//!
//! **A fronteira que isto define:**
//!
//! ```text
//! le a fila de mensagens   Discord, Spotify, Chromium   2o plano FUNCIONA
//! le entrada bruta         Minecraft, jogos em geral    precisa do FOCO
//! ```
//!
//! Automatizar jogo por injecao de tecla toma o PC. Nao ha jeito de contornar por
//! aqui — o caminho certo para jogo e um mod, que roda dentro dele e nao disputa
//! foco com ninguem.
//!
//! ## Uso
//!
//!   cargo run --release --example sonda_tecla_sem_foco -- controle javaw.exe
//!   cargo run --release --example sonda_tecla_sem_foco -- teste    javaw.exe
//!
//! A tecla e F3 (tela de debug): alternavel, nao muda nada no mundo, nao manda
//! mensagem no chat, e impossivel nao ver.
#[cfg(windows)]
fn main() {
    use std::ffi::c_void;

    const VK_F3: u16 = 0x72;
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_KEYUP: u32 = 0x0101;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PostMessageW(h: isize, msg: u32, w: usize, l: isize) -> i32;
        fn GetForegroundWindow() -> isize;
        fn GetWindowTextW(h: isize, b: *mut u16, n: i32) -> i32;
        fn MapVirtualKeyW(codigo: u32, tipo: u32) -> u32;
    }

    let modo = std::env::args().nth(1).unwrap_or_else(|| "teste".into());
    let alvo = std::env::args().nth(2).unwrap_or_else(|| "javaw.exe".into());

    let Some(h) = teka::tools::uia::janela_de(&alvo) else {
        println!("  nao achei janela visivel de {alvo:?}");
        return;
    };
    let titulo = {
        let mut b = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, b.as_mut_ptr(), 256) };
        String::from_utf16_lossy(&b[..n.max(0) as usize])
    };
    let em_foco = unsafe { GetForegroundWindow() } == h;
    println!("  janela: {titulo:?}  (hwnd {h:#x})");
    println!("  em foco agora: {em_foco}");

    match modo.as_str() {
        "controle" => {
            if !em_foco {
                println!("\n  O CONTROLE PRECISA DA JANELA EM FOCO. Clica no jogo e roda de novo.");
                return;
            }
            println!("\n  mandando F3 por SendInput (com foco) em 3s...");
            std::thread::sleep(std::time::Duration::from_secs(3));
            match teka::tools::teclado::mandar_teclas("F3", &[VK_F3]) { Ok(_) => {}, Err(e) => println!("  erro: {e}") }
            println!("  mandei. A tela de debug apareceu/sumiu?");
        }
        _ => {
            if em_foco {
                println!("\n  O TESTE PRECISA DA JANELA SEM FOCO — e o ponto todo dele.");
                println!("  Clica em outra janela (deixando o Minecraft visivel) e roda de novo.");
                return;
            }
            // O `lParam` de WM_KEYDOWN carrega o scan code nos bits 16..23. Alguns
            // aplicativos so olham o virtual-key; outros exigem o scan. Mando os
            // dois preenchidos para o negativo nao ser por detalhe de formato.
            let scan = unsafe { MapVirtualKeyW(VK_F3 as u32, 0) } as isize;
            let l_down: isize = 1 | (scan << 16);
            let l_up: isize = 1 | (scan << 16) | (1 << 30) | (1 << 31);
            println!("\n  mandando F3 por PostMessage (SEM foco), scan {scan:#x}...");
            let a = unsafe { PostMessageW(h, WM_KEYDOWN, VK_F3 as usize, l_down) };
            std::thread::sleep(std::time::Duration::from_millis(60));
            let b = unsafe { PostMessageW(h, WM_KEYUP, VK_F3 as usize, l_up) };
            println!("  PostMessage devolveu {a} e {b} (1 = a mensagem entrou na fila)");
            println!("\n  ATENCAO: `1` diz que a mensagem foi ENFILEIRADA, nao que o jogo");
            println!("  reagiu. So o que voce ve na tela responde isso.");
            let _ = std::ptr::null::<c_void>();
        }
    }
}

#[cfg(not(windows))]
fn main() {
    println!("so faz sentido no Windows");
}
