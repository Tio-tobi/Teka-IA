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

/// Como um atalho age no mundo.
///
/// Dois mecanismos, e a escolha nao e de gosto — foi medida. Ver
/// [`crate::tools::uia`] para a tabela de quanto cada aplicativo publica.
pub enum Como {
    /// Tecla do SISTEMA. Funciona sem foco, com o jogo em tela cheia na frente, e
    /// sem API nenhuma — e o que resolve o Spotify numa conta Free.
    ///
    /// Age as cegas: manda e nao sabe o que aconteceu.
    Teclas(&'static [u16]),
    /// Controle NOMEADO dentro de um aplicativo, via UIAutomation.
    ///
    /// Melhor que tecla quando o app publica a arvore: **le o estado, age, e
    /// confirma que mudou**. Pior quando nao publica — o Spotify expoe 2 controles
    /// com nome e nenhum serve, entao ele fica nas teclas.
    ///
    /// `janela` e casada por TRECHO do titulo, porque o titulo do Discord carrega o
    /// canal atual ("anny_lay | R.E.P.O. Brasil - Discord").
    Controle { janela: &'static str, nome: &'static str },
    /// Botao NOMEADO que so aciona (nao alterna). O "Tocar <faixa>" do Spotify.
    Botao { janela: &'static str, nome: &'static str },
    /// Pela PONTE — socket para a extensao dentro do Spotify.
    ///
    /// **O caminho preferido**, e a razao foi medida: com o navegador do John em
    /// primeiro plano, as cinco trocas de faixa deixaram o foco INTACTO. O caminho
    /// por UIAutomation faz o Spotify subir na frente, o que no meio de uma partida
    /// e exatamente o que se quer evitar.
    Ponte { cmd: &'static str },
    /// Busca e toca pelo nome: escreve no campo de busca e aciona o resultado.
    ///
    /// Duas etapas porque e assim que uma pessoa faz — nao existe atalho para
    /// "tocar musica X" no Spotify, existe "buscar" e depois "tocar o primeiro".
    TocarPorNome { janela: &'static str, campo: &'static str },
}

/// Os atalhos que ela sabe mandar, e nada além disso.
///
/// Toda entrada tem de ser reversível apertando de novo ou apertando o contrário —
/// é o que sustenta o `Efeito::Reversivel`. Um atalho destrutivo aqui dentro furaria
/// a confirmação por uma porta lateral.
///
/// **Deliberadamente de fora:** `Desconectar` (sai da chamada, e voltar exige achar
/// o canal de novo) e `Compartilhar a tela` (manda imagem para outras pessoas — isso
/// é `Efeito::ParaFora`, não `Reversivel`). Os dois existem na árvore do Discord e
/// seriam triviais de adicionar. Não estão aqui porque a isenção de confirmação vale
/// para o que se desfaz num clique, e nenhum dos dois se desfaz.
pub const ATALHOS: &[(&str, Como)] = &[
    // Midia — teclas do SISTEMA. Funcionam sem foco, com o jogo na frente.
    ("proxima_musica", Como::Teclas(&[VK_MEDIA_NEXT])),
    ("musica_anterior", Como::Teclas(&[VK_MEDIA_PREV])),
    ("pausar_musica", Como::Teclas(&[VK_MEDIA_PLAY_PAUSE])),
    ("tocar_musica", Como::Teclas(&[VK_MEDIA_PLAY_PAUSE])),
    ("parar_musica", Como::Teclas(&[VK_MEDIA_STOP])),
    // Volume do sistema.
    ("aumentar_volume", Como::Teclas(&[VK_VOLUME_UP])),
    ("diminuir_volume", Como::Teclas(&[VK_VOLUME_DOWN])),
    ("mudo", Como::Teclas(&[VK_VOLUME_MUTE])),
    // Discord — por CONTROLE e nao por tecla, porque ele publica a arvore inteira
    // (898 controles, medido) e assim ela confirma o estado em vez de alternar as
    // cegas. Os nomes sao os que o proprio Discord expoe em portugues.
    ("mutar_discord", Como::Controle { janela: "Discord.exe", nome: "Silenciar" }),
    ("ensurdecer_discord", Como::Controle { janela: "Discord.exe", nome: "Desativar áudio" }),
    // Spotify — casado por EXECUTAVEL e nao por titulo, porque o titulo dele E a
    // musica tocando ("Gibran Alcocer - Idea 22") e muda a cada faixa. Procurar
    // "Spotify" no titulo falha exatamente quando ele esta tocando.
    // Pela ponte: toca por nome SEM subir a janela. Se a extensao nao estiver no
    // ar, o erro diz isso — e ai o `tocar_faixa_na_tela` e a reserva.
    ("tocar_faixa", Como::Ponte { cmd: "play" }),
    ("proxima_faixa", Como::Ponte { cmd: "next" }),
    ("faixa_anterior", Como::Ponte { cmd: "prev" }),
    ("alternar_musica", Como::Ponte { cmd: "play_pause" }),
    ("embaralhar", Como::Ponte { cmd: "shuffle" }),
    ("que_musica_e_essa", Como::Ponte { cmd: "getdata" }),
    ("tocar_faixa_na_tela", Como::TocarPorNome { janela: "Spotify.exe", campo: "O que você quer ouvir" }),
    ("tocar_playlist", Como::Botao { janela: "Spotify.exe", nome: "Playlist" }),
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
pub fn como_de(nome: &str) -> Option<&'static Como> {
    let n = nome.trim().to_lowercase();
    ATALHOS.iter().find(|(k, _)| *k == n).map(|(_, c)| c)
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
/// Puxa um campo de texto de um JSON simples, sem trazer um parser inteiro.
///
/// A resposta da extensao e sempre plana (`{"ok":true,"track":"...","id":1}`), entao
/// isto basta — e um parser de JSON completo por causa de tres campos seria peso sem
/// retorno num projeto que se orgulha de nao ter dependencia.
fn campo_json(json: &str, chave: &str) -> Option<String> {
    let marca = format!("\"{chave}\":\"");
    let i = json.find(&marca)? + marca.len();
    let resto = &json[i..];
    let mut fora = String::new();
    let mut chars = resto.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(fora),
            c if c == 92 as char => fora.push(chars.next().unwrap_or(c)),
            c => fora.push(c),
        }
    }
    None
}

/// O botao "Tocar X" para um alvo, se existir na arvore agora.
fn achar_botao_tocar(janela: &str, alvo: &str) -> Option<String> {
    let baixo = alvo.to_lowercase();
    super::uia::listar(janela, super::uia::TIPO_BOTAO, "Tocar ")
        .ok()?
        .into_iter()
        .find(|n| n.to_lowercase().contains(&baixo))
}

/// Busca e toca. Duas etapas, porque e assim que uma pessoa faz.
///
/// O botao alvo precisa casar com "Tocar" **e com a busca**. So "Tocar" nao serve:
/// os resultados anteriores continuam na arvore enquanto a busca nova nao chega, e
/// a primeira versao disto pediu "Deslocado NAPA" e tocou "SHADOW de ONIMXRU" —
/// clicou no botao que ja estava la. Tocar a musica errada e pior que falhar, porque
/// parece ter funcionado.
fn tocar_por_nome(janela: &str, campo: &str, alvo: Option<&str>) -> Result<String, String> {
    let Some(musica) = alvo.map(str::trim).filter(|s| !s.is_empty()) else {
        return Err("tocar_faixa precisa do nome da musica".into());
    };
    // A palavra mais longa da busca e a mais distintiva: em "Deslocado NAPA" e
    // "Deslocado", e e ela que tem de aparecer no nome do botao.
    let chave = musica
        .split_whitespace()
        .max_by_key(|p| p.chars().count())
        .unwrap_or(musica);

    super::uia::escrever(janela, campo, musica)?;

    // A busca do Spotify e assincrona. Espera o botao CERTO aparecer, e nao um
    // botao qualquer.
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if let Some(bom) = achar_botao_tocar(janela, chave) {
            let n = super::uia::acionar_botao(janela, &bom)?;
            return Ok(format!("tocando {n:?}"));
        }
    }
    Err(format!(
        "busquei {musica:?} mas nenhum resultado com {chave:?} apareceu em 7s"
    ))
}

pub fn mandar(nome: &str) -> Result<String, String> {
    mandar_com(nome, None)
}

/// A mesma coisa, com um alvo — o nome da musica, por exemplo.
pub fn mandar_com(nome: &str, alvo: Option<&str>) -> Result<String, String> {
    // Uma guarda para a OPERACAO inteira. Sem isto, `tocar_faixa` — que abre tres
    // sessoes de UIA — devolvia o foco para o estado intermediario e deixava o
    // Spotify na frente do jogo do John.
    let _foco = super::uia::Foco::guardar();
    match como_de(nome) {
        None => Err(format!("nao conheco o atalho {nome:?}. conheco: {}", nomes())),
        Some(Como::Teclas(t)) => mandar_teclas(nome, t),
        Some(Como::Botao { janela, nome: ctl }) => {
            // Prefere o botao "Tocar X" ao item de navegacao "X".
            //
            // Clicar no item da barra lateral so ABRE a playlist; quem toca e o
            // botao de play dela. Os dois casam com o nome da playlist, e o de
            // navegacao vem primeiro na arvore — entao pedir "toca a playlist" sem
            // esta preferencia navegava e ficava por isso mesmo.
            let alvo = alvo.unwrap_or(ctl);
            match achar_botao_tocar(janela, alvo) {
                Some(n) => super::uia::acionar_botao(janela, &n)
                    .map(|x| format!("{nome}: tocando {x:?}")),
                None => super::uia::acionar_botao(janela, alvo)
                    .map(|n| format!("{nome}: abri {n:?} (sem botao de tocar visivel)")),
            }
        }
        Some(Como::Ponte { cmd }) => {
            let r = super::ponte::global()?.comando(cmd, alvo)?;
            // A extensao devolve JSON; extrai a faixa se houver, senao devolve cru.
            Ok(match campo_json(&r, "track") {
                Some(t) if !t.is_empty() => format!("{nome}: {t}"),
                _ => match campo_json(&r, "error") {
                    Some(e) if !e.is_empty() => return Err(e),
                    _ => format!("{nome}: {r}"),
                },
            })
        }
        Some(Como::TocarPorNome { janela, campo }) => tocar_por_nome(janela, campo, alvo),
        Some(Como::Controle { janela, nome: ctl }) => {
            let r = super::uia::alternar(janela, ctl)?;
            // O estado ANTES e DEPOIS e o que esta ferramenta tem e a tecla nao tem:
            // ela sabe o que aconteceu, em vez de torcer.
            Ok(format!(
                "{nome}: {} -> {}",
                super::uia::rotulo(r.antes),
                super::uia::rotulo(r.depois)
            ))
        }
    }
}

#[cfg(windows)]
fn mandar_teclas(nome: &str, teclas: &[u16]) -> Result<String, String> {
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
fn mandar_teclas(_nome: &str, _teclas: &[u16]) -> Result<String, String> {
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
        assert!(como_de("proxima_musica").is_some());
        assert!(como_de("PROXIMA_MUSICA").is_some(), "deve ignorar caixa");
        assert!(como_de("  mudo  ").is_some(), "deve ignorar espaco");
        for fora in ["alt+f4", "ctrl+w", "delete", "", "formatar"] {
            assert!(como_de(fora).is_none(), "aceitou {fora:?}");
        }
    }

    /// A isencao de confirmacao vale para o que se desfaz. Um controle que
    /// desconecta da chamada ou compartilha a tela nao se desfaz num clique, e
    /// entrar aqui furaria a confirmacao por uma porta lateral.
    #[test]
    fn nenhum_controle_e_de_via_unica() {
        const FORA: [&str; 4] = ["Desconectar", "Compartilhar a tela", "Sair", "Encerrar"];
        for (nome, como) in ATALHOS {
            if let Como::Controle { nome: ctl, .. } = como {
                assert!(
                    !FORA.contains(ctl),
                    "{nome} aciona {ctl:?}, que nao se desfaz num clique"
                );
            }
        }
    }

    /// A lista fechada e a guarda que substitui a confirmacao. Se entrar aqui um
    /// atalho que fecha janela, apaga ou envia, a isencao deixa de se justificar.
    #[test]
    fn nenhum_atalho_e_destrutivo() {
        const PROIBIDAS: [u16; 4] = [0x2E, 0x73, 0x7B, 0x5B]; // Delete, F4, F12, Win
        for (nome, como) in ATALHOS {
            let Como::Teclas(teclas) = como else { continue };
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
                como_de(nome).is_some(),
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
