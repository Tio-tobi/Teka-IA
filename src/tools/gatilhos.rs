//! Frase que o John diz → atalho que ela executa.
//!
//! ## O problema que isto resolve
//!
//! A cabeça de ponteiro **copia** um trecho do pedido; ela não inventa texto. Então
//! para emitir `atalho(nome="proxima_musica")` a palavra `proxima_musica` teria de
//! estar no pedido — e ninguém fala assim. O John diz *"pula essa música"*.
//!
//! Esta tabela faz a tradução **fora do modelo**. O trabalho dele passa a ser
//! reconhecer que é um atalho e copiar o alvo quando houver; qual atalho é, decide
//! aqui.
//!
//! A ideia é da Nyxara — `tools/shortcuts.py`, com `triggers` no `conf.yaml`. O que
//! importa não é o algoritmo, é a tabela ser **dado e não código**: acrescentar
//! "manda a próxima" não deveria exigir recompilar nada.
//!
//! ## O casamento, em três níveis
//!
//! Portado de `trigger_match_score` da Nyxara, com os mesmos números:
//!
//! ```text
//! 1.00   o gatilho aparece literal no pedido
//! 0.95   aparece depois de normalizar (sem acento, so letra e numero)
//! 0.66+  cobertura de palavras do gatilho dentro do pedido
//! ```
//!
//! O desempate por **gatilho mais longo** é acréscimo meu, e tem motivo: "toca a
//! playlist" e "toca" casam os dois em *"toca a playlist animada"*, e o segundo
//! mandaria procurar uma faixa chamada "a playlist animada".

/// Acentos do português, para normalizar sem depender de tabela Unicode inteira.
const ACENTOS: &[(char, char)] = &[
    ('á', 'a'), ('à', 'a'), ('ã', 'a'), ('â', 'a'), ('ä', 'a'),
    ('é', 'e'), ('ê', 'e'), ('è', 'e'), ('ë', 'e'),
    ('í', 'i'), ('î', 'i'), ('ì', 'i'), ('ï', 'i'),
    ('ó', 'o'), ('õ', 'o'), ('ô', 'o'), ('ò', 'o'), ('ö', 'o'),
    ('ú', 'u'), ('û', 'u'), ('ù', 'u'), ('ü', 'u'),
    ('ç', 'c'), ('ñ', 'n'),
];

/// Minúsculas, sem acento, só letra e número, espaço colapsado.
pub fn normalizar(texto: &str) -> String {
    let mut fora = String::with_capacity(texto.len());
    for c in texto.chars() {
        let c = c.to_lowercase().next().unwrap_or(c);
        let c = ACENTOS
            .iter()
            .find(|(a, _)| *a == c)
            .map(|(_, b)| *b)
            .unwrap_or(c);
        if c.is_ascii_alphanumeric() {
            fora.push(c);
        } else if !fora.ends_with(' ') {
            fora.push(' ');
        }
    }
    fora.trim().to_string()
}

fn palavras(texto: &str) -> Vec<String> {
    normalizar(texto)
        .split_whitespace()
        .filter(|p| p.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

/// Quanto este gatilho casa com o pedido. `0.0` = não casa.
///
/// Os três níveis são os da Nyxara. A cobertura de 0,66 também: exigir todas as
/// palavras rejeitaria *"pula essa música aí"* contra o gatilho "pula essa musica",
/// e exigir uma só faria "musica" casar com qualquer coisa que mencione música.
pub fn pontuacao(gatilho: &str, pedido: &str, pedido_norm: &str) -> f64 {
    let g = gatilho.trim();
    if g.is_empty() {
        return 0.0;
    }
    if pedido.to_lowercase().contains(&g.to_lowercase()) {
        return 1.0;
    }
    let gn = normalizar(g);
    if !gn.is_empty() && pedido_norm.contains(&gn) {
        return 0.95;
    }
    let gp = palavras(g);
    if gp.is_empty() {
        return 0.0;
    }
    let pp = palavras(pedido);
    let comuns = gp.iter().filter(|p| pp.contains(p)).count();
    let preciso = ((gp.len() as f64) * 0.66).ceil().max(1.0) as usize;
    if comuns < preciso {
        return 0.0;
    }
    comuns as f64 / gp.len() as f64
}

/// A tabela, embutida no binário. Zero dependência continua valendo.
const TABELA: &str = include_str!("../../dados/gatilhos.txt");

/// Uma linha da tabela já separada.
pub struct Entrada {
    pub atalho: String,
    pub gatilhos: Vec<String>,
}

pub fn ler(texto: &str) -> Vec<Entrada> {
    let mut fora = Vec::new();
    for l in texto.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let Some((atalho, resto)) = l.split_once('|') else { continue };
        let gatilhos: Vec<String> = resto
            .split(',')
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect();
        if !gatilhos.is_empty() {
            fora.push(Entrada { atalho: atalho.trim().to_string(), gatilhos });
        }
    }
    fora
}

pub fn tabela() -> Vec<Entrada> {
    ler(TABELA)
}

/// Traduz um pedido em `(atalho, alvo)`.
///
/// O alvo é o que sobra do pedido depois de tirar o gatilho — é assim que
/// *"toca deslocado do napa"* vira `tocar_faixa` com alvo `"deslocado do napa"`.
/// Sobra vazia vira `None`, que é o caso de "pula essa musica".
pub fn casar(pedido: &str) -> Option<(String, Option<String>)> {
    casar_em(&tabela(), pedido)
}

pub fn casar_em(tab: &[Entrada], pedido: &str) -> Option<(String, Option<String>)> {
    let norm = normalizar(pedido);
    let mut melhor: Option<(f64, usize, &str, &str)> = None; // (pontos, tamanho, atalho, gatilho)
    for e in tab {
        for g in &e.gatilhos {
            let p = pontuacao(g, pedido, &norm);
            if p <= 0.0 {
                continue;
            }
            // Empate vai para o gatilho MAIS LONGO: "toca a playlist" e "toca"
            // casam os dois em "toca a playlist animada", e o curto mandaria
            // procurar uma FAIXA chamada "a playlist animada".
            let cand = (p, g.chars().count(), e.atalho.as_str(), g.as_str());
            if melhor.is_none_or(|m| (cand.0, cand.1) > (m.0, m.1)) {
                melhor = Some(cand);
            }
        }
    }
    let (_, _, atalho, gatilho) = melhor?;
    Some((atalho.to_string(), sobra(pedido, gatilho)))
}

/// O que resta do pedido depois de tirar o gatilho.
fn sobra(pedido: &str, gatilho: &str) -> Option<String> {
    let pn = normalizar(pedido);
    let gn = normalizar(gatilho);
    let resto = match pn.find(&gn) {
        Some(i) => format!("{} {}", &pn[..i], &pn[i + gn.len()..]),
        // Casou por cobertura de palavras: tira as palavras do gatilho uma a uma.
        None => {
            let gp = palavras(gatilho);
            pn.split_whitespace()
                .filter(|p| !gp.iter().any(|g| g == p))
                .collect::<Vec<_>>()
                .join(" ")
        }
    };
    let resto = resto.split_whitespace().collect::<Vec<_>>().join(" ");
    (!resto.is_empty()).then_some(resto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_como_a_nyxara() {
        assert_eq!(normalizar("Pula ESSA Música!"), "pula essa musica");
        assert_eq!(normalizar("Cansaço  do   Alec"), "cansaco do alec");
        assert_eq!(normalizar("  ção, ão; ê  "), "cao ao e");
    }

    #[test]
    fn os_tres_niveis_pontuam_como_esperado() {
        let p = |g: &str, t: &str| pontuacao(g, t, &normalizar(t));
        assert_eq!(p("pula essa musica", "pula essa musica"), 1.0, "literal");
        assert_eq!(p("pula essa musica", "Pula essa MÚSICA"), 0.95, "normalizado");
        // Cobertura: 2 de 3 palavras = 0,67, acima do piso de 0,66.
        assert!(p("pula essa musica", "pula a musica ai") > 0.6);
        assert_eq!(p("pula essa musica", "que horas sao"), 0.0);
    }

    /// O caso que motivou a tabela inteira.
    #[test]
    fn a_frase_natural_vira_o_atalho_certo() {
        let t = tabela();
        for (pedido, esperado) in [
            ("pula essa musica", "proxima_musica"),
            ("pula essa música aí", "proxima_musica"),
            ("volta a musica", "musica_anterior"),
            ("aumenta o som", "aumentar_volume"),
            ("abaixa o volume", "diminuir_volume"),
            ("me muta no discord", "mutar_discord"),
            ("me muta", "mutar_discord"),
        ] {
            let r = casar_em(&t, pedido);
            assert_eq!(
                r.as_ref().map(|(a, _)| a.as_str()),
                Some(esperado),
                "{pedido:?} deu {r:?}"
            );
        }
    }

    /// O alvo sai do que sobra — e o desempate por gatilho longo importa aqui.
    #[test]
    fn o_alvo_e_o_que_sobra_do_pedido() {
        let t = tabela();
        let (a, alvo) = casar_em(&t, "toca deslocado do napa").unwrap();
        assert_eq!(a, "tocar_faixa");
        assert_eq!(alvo.as_deref(), Some("deslocado do napa"));

        let (a, alvo) = casar_em(&t, "toca a playlist animada").unwrap();
        assert_eq!(a, "tocar_playlist", "o gatilho longo tem de vencer o curto");
        assert_eq!(alvo.as_deref(), Some("animada"));

        // Sem sobra: o atalho nao precisa de alvo.
        let (a, alvo) = casar_em(&t, "pula essa musica").unwrap();
        assert_eq!(a, "proxima_musica");
        assert_eq!(alvo, None);
    }

    /// Conversa nao pode virar atalho. A tabela e um filtro, nao um ima.
    #[test]
    fn conversa_nao_dispara_atalho() {
        let t = tabela();
        for fora in [
            "que horas sao",
            "oi tudo bem",
            "me explica como funciona uma rede neural",
            "lista os arquivos da pasta dados",
            "quanto de memoria sobrou",
        ] {
            assert_eq!(casar_em(&t, fora), None, "{fora:?} virou atalho");
        }
    }

    /// Todo atalho da tabela tem de existir na ferramenta — senao o gatilho
    /// dispara e ela responde "nao conheco o atalho".
    #[test]
    fn todo_gatilho_aponta_para_atalho_que_existe() {
        for e in tabela() {
            assert!(
                super::super::teclado::como_de(&e.atalho).is_some(),
                "a tabela mapeia para {:?}, que a ferramenta nao conhece",
                e.atalho
            );
        }
    }
}
