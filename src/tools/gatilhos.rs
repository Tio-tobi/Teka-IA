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

/// Palavras que não distinguem nada, e por isso não contam na cobertura.
///
/// A cobertura de 0,66 pressupõe que as palavras do gatilho carregam sentido. Não
/// carregam: "mudo no sistema" casava com *"quero conferir a data no sistema"* —
/// dois de três, 0,67 — porque "no" e "sistema" bastavam e `mudo`, a palavra que
/// decide, era exatamente a que faltava. Três frases do benchmark caíam assim.
///
/// É o mesmo erro dos moldes genéricos ("manda = fora"), agora na tabela: superfície
/// larga demais rouba de outra ferramenta. A lista é curta de propósito e cada
/// entrada é gramatical, não temática — nada de tirar "musica" daqui.
///
/// `para` fica DE FORA: em "para tudo" ela é o verbo parar, não a preposição. Em
/// português a distinção não cabe numa lista de palavras. `nao` também fica de fora,
/// e por um motivo mais forte: é ela que separa "nao retoma mais" de "retoma sempre".
const VAZIAS: &[&str] = &[
    "os", "as", "no", "na", "nos", "nas", "em", "de", "do", "da", "ai", "um",
    "uma", "que", "esta", "isso", "aqui", "pra", "mais",
    // Cortesia e enchimento: nao mudam o pedido, e sem elas "pausa a musica por
    // favor" perderia o gatilho pela regra da sobra.
    "por", "favor", "pfv", "agora", "ja", "entao",
];

/// As palavras do gatilho que de fato o distinguem.
fn palavras_fortes(texto: &str) -> Vec<String> {
    palavras(texto).into_iter().filter(|p| !VAZIAS.contains(&p.as_str())).collect()
}

fn palavras(texto: &str) -> Vec<String> {
    normalizar(texto)
        .split_whitespace()
        .filter(|p| p.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

/// O texto contém o trecho como PALAVRA INTEIRA?
///
/// Sem isto, o gatilho "toca" casa com *"seria bom uma radio tocando"* e com
/// *"poe um som pra tocar"* — e casa no nível literal, 1,00, que vence tudo.
/// Descoberto quando um teste acusou aquelas duas frases de serem coisa que ela
/// sabe fazer: não eram, o medidor é que estava largo.
///
/// O mesmo defeito estragava o alvo: *"poe um som pra tocar"* devolvia
/// `"poe um som pra r"`, porque `sobra` recortava "toca" de dentro de "tocar".
///
/// Trabalha sobre a forma normalizada, onde toda fronteira já virou espaço — então
/// basta procurar o trecho cercado de espaços.
fn contem_inteiro(texto_norm: &str, trecho_norm: &str) -> Option<usize> {
    if trecho_norm.is_empty() {
        return None;
    }
    let h = format!(" {texto_norm} ");
    let n = format!(" {trecho_norm} ");
    // O índice devolvido é o do texto original normalizado, sem o espaço que eu pus.
    h.find(&n).map(|i| i)
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
    // Palavra inteira nos dois niveis: ver `contem_inteiro`.
    let gn = normalizar(g);
    if contem_inteiro(pedido_norm, &gn).is_some() {
        // Literal (1,00) se aparece tal e qual no texto cru; normalizado (0,95) se
        // so aparece depois de tirar acento e pontuacao.
        return if pedido.to_lowercase().contains(&g.trim().to_lowercase()) { 1.0 } else { 0.95 };
    }
    // A cobertura olha so as palavras FORTES: ver `VAZIAS`.
    //
    // E exige DUAS delas para valer. Com uma so, cobrir e o mesmo que conter a
    // palavra, e o gatilho vira ima: "mais alto" tem [alto] de forte, e ai
    // *"o consumo de ram esta alto"* virava `aumentar_volume`. Casar difuso uma
    // palavra unica e busca por substring com passos a mais.
    //
    // Gatilho de uma palavra forte nao fica sem casamento — ele ainda casa literal
    // e normalizado, que e como "toca" e "pausa ai" sempre funcionaram.
    let gp = palavras_fortes(g);
    if gp.len() < 2 {
        return 0.0;
    }
    let pp = palavras(pedido);
    let comuns = gp.iter().filter(|p| pp.contains(p)).count();
    let preciso = ((gp.len() as f64) * 0.66).ceil().max(1.0) as usize;
    if comuns < preciso {
        return 0.0;
    }
    // TETO DE 0,90, abaixo do 0,95 do nivel normalizado.
    //
    // Sem ele a cobertura EMPATA com o casamento literal: tirar as vazias encolhe o
    // denominador, "que musica e essa" vira [musica, essa], e *"pula essa musica"*
    // cobre as duas — 2 de 2, pontuacao 1,00. Empatava com o proprio gatilho literal
    // e vencia no desempate por tamanho. Cobertura e o nivel mais fraco dos tres e
    // tem de pontuar como tal, sempre.
    (comuns as f64 / gp.len() as f64).min(0.90)
}

/// A tabela, embutida no binário. Zero dependência continua valendo.
const TABELA: &str = include_str!("../../dados/gatilhos.txt");

/// Uma linha da tabela já separada.
pub struct Entrada {
    pub atalho: String,
    pub gatilhos: Vec<String>,
    /// Marcado com `*` na tabela: este atalho recebe um alvo.
    pub com_alvo: bool,
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
            let nome = atalho.trim();
            let com_alvo = nome.ends_with('*');
            fora.push(Entrada {
                atalho: nome.trim_end_matches('*').to_string(),
                gatilhos,
                com_alvo,
            });
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
            // ATALHO SEM ALVO NAO CASA COM FRASE QUE SOBRA CONTEUDO.
            //
            // "muta o theo no discord" casava com o gatilho "muta o discord" por
            // cobertura, virava `mutar_discord` — que nao tem alvo — e "theo" ia
            // fora calado: voce pedia para mutar o Theo e ela mutava VOCE.
            //
            // Sobra de palavra vazia segue valendo, senao "pula essa musica ai"
            // morreria por causa do "ai".
            if !e.com_alvo {
                if let Some(resto) = sobra(pedido, g) {
                    // A sobra so vale se for vocabulario DESTE atalho. "para de
                    // retomar a musica" sobra "a musica", e "musica" esta nos
                    // gatilhos dele — e o mesmo assunto, dito com mais palavra.
                    // "muta o theo no discord" sobra "theo", que nao esta em lugar
                    // nenhum da entrada: e OUTRO alguem, e ai nao e este atalho.
                    let vocab: Vec<String> =
                        e.gatilhos.iter().flat_map(|x| palavras_fortes(x)).collect();
                    if palavras_fortes(&resto).iter().any(|w| !vocab.contains(w)) {
                        continue;
                    }
                }
            }
            let cand = (p, g.chars().count(), e.atalho.as_str(), g.as_str());
            if melhor.is_none_or(|m| (cand.0, cand.1) > (m.0, m.1)) {
                melhor = Some(cand);
            }
        }
    }
    let (_, _, atalho, gatilho) = melhor?;
    // Atalho sem alvo devolve alvo NENHUM. O que sobrou dele ja passou pelo filtro
    // acima, entao e cortesia ou enchimento ("por favor", "ai") — nao e argumento,
    // e deixar passar convidaria alguem a ler isso um dia como se fosse.
    let com_alvo = tab.iter().any(|e| e.atalho == atalho && e.com_alvo);
    Some((atalho.to_string(), com_alvo.then(|| sobra(pedido, gatilho)).flatten()))
}

/// O que resta do pedido depois de tirar o gatilho.
fn sobra(pedido: &str, gatilho: &str) -> Option<String> {
    let pn = normalizar(pedido);
    let gn = normalizar(gatilho);
    // `contem_inteiro` devolve o indice dentro de " {pn} ", entao desconto o espaco.
    let resto = match contem_inteiro(&pn, &gn).map(|i| i.saturating_sub(1)) {
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

    /// Gatilho e palavra inteira, nao pedaco de palavra.
    #[test]
    fn gatilho_nao_casa_pedaco_de_palavra() {
        let t = tabela();
        // "toca" dentro de "tocando" nao pode disparar nada.
        for fora in ["seria bom uma radio tocando", "quem e o tocador de violao"] {
            let r = casar_em(&t, fora);
            assert!(r.is_none(), "{fora:?} casou pedaco de palavra: {r:?}");
        }
        // Mas "poe um som pra tocar" NAO e pedaco de palavra: e o gatilho inteiro
        // "poe pra tocar" casando por cobertura. Aquilo e pedido de musica mesmo, e
        // tem de continuar casando — foi por confundir os dois que eu quase
        // "consertei" o codigo certo.
        assert_eq!(
            casar_em(&t, "poe um som pra tocar").map(|(a, _)| a),
            Some("tocar_faixa".to_string())
        );
        // E o de verdade continua casando.
        let (a, alvo) = casar_em(&t, "toca deslocado do napa").unwrap();
        assert_eq!(a, "tocar_faixa");
        assert_eq!(alvo.as_deref(), Some("deslocado do napa"));
    }

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

    /// O pedido que originou a tabela de regras, dito como o John diria.
    #[test]
    fn pedir_para_retomar_sempre_liga_a_regra() {
        let t = tabela();
        for pedido in [
            "toda vez que a musica parar voce retoma",
            "sempre que a musica parar retoma",
            "retoma sempre a musica",
        ] {
            assert_eq!(
                casar_em(&t, pedido).map(|(a, _)| a),
                Some("retomar_sempre".to_string()),
                "{pedido:?}"
            );
        }
        for pedido in ["para de retomar a musica", "pode parar de retomar"] {
            assert_eq!(
                casar_em(&t, pedido).map(|(a, _)| a),
                Some("parar_de_retomar".to_string()),
                "{pedido:?}"
            );
        }
    }

    /// Palavra vazia nao carrega cobertura — os tres casos que o benchmark pegou.
    #[test]
    fn palavra_vazia_nao_casa_gatilho() {
        let t = tabela();
        for (frase, ladrao) in [
            ("quero conferir a data no sistema", "mudo"),
            ("o que esta arquivado em target", "que_musica_e_essa"),
            ("poe no diario.md a anotacao reuniao", "embaralhar"),
        ] {
            let r = casar_em(&t, frase).map(|(a, _)| a);
            assert_ne!(r.as_deref(), Some(ladrao), "{frase:?} voltou a cair em {ladrao}");
        }
        // E nenhum gatilho pode ser SO palavra vazia: viraria um ima.
        for e in &t {
            for g in &e.gatilhos {
                assert!(
                    !palavras_fortes(g).is_empty(),
                    "o gatilho {g:?} de {} nao tem palavra forte nenhuma",
                    e.atalho
                );
            }
        }
    }

    /// Pedir para mutar OUTRA PESSOA nao pode virar "muta voce".
    #[test]
    fn atalho_sem_alvo_nao_engole_conteudo() {
        let t = tabela();
        for pedido in [
            "muta o theo no discord",
            "muta o stitch no discord",
            "silencia o filipe no discord",
        ] {
            assert_eq!(
                casar_em(&t, pedido),
                None,
                "{pedido:?} virou atalho, e o alvo sumiria"
            );
        }
        // O de voce mesmo continua funcionando.
        for pedido in ["me muta", "muta meu microfone", "muta o discord"] {
            assert_eq!(
                casar_em(&t, pedido).map(|(a, _)| a),
                Some("mutar_discord".to_string()),
                "{pedido:?}"
            );
        }
        // E sobra de palavra vazia nao derruba nada.
        assert_eq!(
            casar_em(&t, "pula essa musica ai").map(|(a, _)| a),
            Some("proxima_musica".to_string())
        );
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

