//! Teclas de mídia e atalhos nomeados, para pedir coisas **sem sair do jogo**.
//!
//! ## O problema que isto resolve
//!
//! Trocar de música, mutar o microfone, baixar o volume. Coisas de dois segundos
//! que hoje custam um alt-tab — e um alt-tab no meio de uma partida custa mais que
//! os dois segundos.
//!
//! ## Por que tecla e não mouse
//!
//! A ideia original era abrir o Discord e **clicar** no botão de mudo. Três
//! problemas, e o terceiro mata:
//!
//! 1. A Teka não enxerga. Ela devolve `(ferramenta, trecho copiado do pedido)`, sem
//!    screenshot e sem visão — qualquer clique seria em coordenada fixa.
//! 2. Coordenada fixa quebra com tamanho de janela, tema e a próxima atualização.
//! 3. Clique errado aperta **o que estiver ali**, e o que está ali pode ser
//!    "Sim, apagar". Tecla errada faz uma coisa conhecida.
//!
//! As teclas de mídia do Windows (`VK_MEDIA_*`) são do sistema operacional, não do
//! aplicativo. O Spotify obedece a elas como qualquer player: **sem API, sem
//! Premium, sem foco na janela, com o jogo em tela cheia na frente.**
//!
//! ## Por que não pede confirmação
//!
//! Ver [`crate::tools::prim::Efeito::Reversivel`]. Resumo: confirmação existe por
//! dano irreversível e por surpresa, e pular faixa não tem nenhum dos dois. Uma
//! confirmação que exige alt-tab para clicar "sim" destruiria a única razão de esta
//! ferramenta existir.
//!
//! A guarda aqui não é perguntar — é a **lista fechada**. Ela não manda tecla
//! arbitrária; manda uma das que estão em [`ATALHOS`].

/// Os atalhos que ela sabe mandar, e nada além disso.
///
/// Cada linha é `(nome, teclas)`. Toda entrada tem de ser reversível apertando de
/// novo ou apertando o contrário — é o que sustenta o `Efeito::Reversivel`. Um
/// atalho destrutivo aqui dentro furaria a confirmação por uma porta lateral.
pub const ATALHOS: &[(&str, &[u16])] = &[
    // Mídia — teclas do SISTEMA. Funcionam sem foco, com o jogo na frente.
    ("proxima_musica", &[VK_MEDIA_NEXT]),
    ("musica_anterior", &[VK_MEDIA_PREV]),
    ("pausar_musica", &[VK_MEDIA_PLAY_PAUSE]),
    ("tocar_musica", &[VK_MEDIA_PLAY_PAUSE]),
    ("parar_musica", &[VK_MEDIA_STOP]),
    // Volume do sistema.
    ("aumentar_volume", &[VK_VOLUME_UP]),
    ("diminuir_volume", &[VK_VOLUME_DOWN]),
    ("mudo", &[VK_VOLUME_MUTE]),
    // Discord. Ctrl+Shift+M é o padrão dele para alternar o mudo; para funcionar
    // com o jogo em primeiro plano, o atalho tem de estar marcado como GLOBAL nas
    // preferências do Discord (Configurações -> Teclas de atalho).
    ("mutar_discord", &[VK_CONTROL, VK_SHIFT, b'M' as u16]),
    ("ensurdecer_discord", &[VK_CONTROL, VK_SHIFT, b'D' as u16]),
];

pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_VOLUME_MUTE: u16 = 0xAD;
pub const VK_VOLUME_DOWN: u16 = 0xAE;
pub const VK_VOLUME_UP: u16 = 0xAF;
pub const VK_MEDIA_NEXT: u16 = 0xB0;
pub const VK_MEDIA_PREV: u16 = 0xB1;
pub const VK_MEDIA_STOP: u16 = 0xB2;
pub const VK_MEDIA_PLAY_PAUSE: u16 = 0xB3;

/// O nome existe na lista?
pub fn teclas_de(nome: &str) -> Option<&'static [u16]> {
    let n = nome.trim().to_lowercase();
    ATALHOS
        .iter()
        .find(|(k, _)| *k == n)
        .map(|(_, teclas)| *teclas)
}

/// Os nomes, para a mensagem de erro dizer o que ela aceita.
pub fn nomes() -> String {
    ATALHOS
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(windows)]
mod win {
    pub const INPUT_KEYBOARD: u32 = 1;
    pub const KEYEVENTF_KEYUP: u32 = 0x0002;

    /// `INPUT` do Win32, achatado para o caso de teclado.
    ///
    /// A união do `INPUT` real é do tamanho do `MOUSEINPUT`, que é maior que o
    /// `KEYBDINPUT` — daí o `_resto`. Errar isso não dá erro de compilação: o
    /// `SendInput` simplesmente recusa em silêncio, devolvendo 0. O teste
    /// `o_tamanho_do_input_bate_com_o_win32` prende os 40 bytes.
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Input {
        pub tipo: u32,
        pub _pad: u32,
        pub vk: u16,
        pub scan: u16,
        pub flags: u32,
        pub tempo: u32,
        pub extra: usize,
        pub _resto: [u8; 8],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn SendInput(n: u32, entradas: *const Input, tamanho: i32) -> u32;
    }
}

/// Manda o atalho. Pressiona na ordem, solta na ordem inversa.
///
/// A ordem inversa não é detalhe: soltar `ctrl` antes do `m` faria o `m` chegar
/// sozinho ao aplicativo, que é uma tecla completamente diferente do que foi pedido.
#[cfg(windows)]
pub fn mandar(nome: &str) -> Result<String, String> {
    let Some(teclas) = teclas_de(nome) else {
        return Err(format!("nao conheco o atalho {nome:?}. conheco: {}", nomes()));
    };
    let mut entradas: Vec<win::Input> = Vec::with_capacity(teclas.len() * 2);
    for &vk in teclas {
        entradas.push(win::Input { tipo: win::INPUT_KEYBOARD, vk, ..Default::default() });
    }
    for &vk in teclas.iter().rev() {
        entradas.push(win::Input {
            tipo: win::INPUT_KEYBOARD,
            vk,
            flags: win::KEYEVENTF_KEYUP,
            ..Default::default()
        });
    }
    let n = unsafe {
        win::SendInput(
            entradas.len() as u32,
            entradas.as_ptr(),
            std::mem::size_of::<win::Input>() as i32,
        )
    };
    if n as usize != entradas.len() {
        return Err(format!(
            "o Windows aceitou {n} de {} teclas (SendInput bloqueado?)",
            entradas.len()
        ));
    }
    Ok(format!("mandei {nome}"))
}

#[cfg(not(windows))]
pub fn mandar(_nome: &str) -> Result<String, String> {
    Err("atalho de teclado so existe no Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Errar o tamanho do `INPUT` não quebra a compilação — o `SendInput` recusa em
    /// silêncio e devolve 0. Este teste é a única coisa entre isso e um bug mudo.
    #[cfg(windows)]
    #[test]
    fn o_tamanho_do_input_bate_com_o_win32() {
        assert_eq!(
            std::mem::size_of::<win::Input>(),
            40,
            "INPUT do Win32 tem 40 bytes em x64; fora disso o SendInput recusa calado"
        );
    }

    #[test]
    fn so_aceita_nome_da_lista() {
        assert!(teclas_de("proxima_musica").is_some());
        assert!(teclas_de("PROXIMA_MUSICA").is_some(), "deve ignorar caixa");
        assert!(teclas_de("  mudo  ").is_some(), "deve ignorar espaco");
        for fora in ["alt+f4", "ctrl+w", "delete", "", "formatar"] {
            assert!(teclas_de(fora).is_none(), "aceitou {fora:?}");
        }
    }

    /// A lista fechada e a guarda que substitui a confirmacao. Se entrar aqui um
    /// atalho que fecha janela, apaga ou envia, a isencao deixa de se justificar.
    #[test]
    fn nenhum_atalho_e_destrutivo() {
        const PROIBIDAS: [u16; 4] = [0x2E, 0x73, 0x7B, 0x5B]; // Delete, F4, F12, Win
        for (nome, teclas) in ATALHOS {
            for t in *teclas {
                assert!(
                    !PROIBIDAS.contains(t),
                    "{nome} usa uma tecla que nao e reversivel"
                );
            }
            assert!(
                teclas.len() <= 3,
                "{nome}: atalho longo demais para ser obviamente reversivel"
            );
        }
    }

    #[test]
    fn o_erro_diz_o_que_ela_aceita() {
        let e = mandar("inventado").unwrap_err();
        assert!(e.contains("proxima_musica"), "{e}");
    }

    /// O poço do gerador e a lista real têm de ser a MESMA coisa.
    ///
    /// Se divergirem, o treino ensina nome que a ferramenta recusa — ela aprende a
    /// pedir `pular_faixa` e leva "nao conheco o atalho" na cara toda vez. Erro que
    /// não aparece em teste nenhum de execução, só na frustração do John.
    #[test]
    fn o_poco_do_gerador_e_a_lista_real_batem() {
        let poco = crate::learn::dados::NOMES_DE_ATALHO;
        for nome in poco {
            assert!(
                teclas_de(nome).is_some(),
                "o gerador ensina {nome:?}, que a ferramenta nao conhece"
            );
        }
        for (nome, _) in ATALHOS {
            assert!(
                poco.contains(nome),
                "{nome:?} existe na ferramenta mas o gerador nunca ensina a pedir"
            );
        }
    }
}
