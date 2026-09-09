//! JSON mínimo, `std` puro — para falar JSON-RPC com o DeepSeek-Harness.
//!
//! ## Por que isto existe
//!
//! O que havia era `teclado::campo_json`: acha `"chave":"valor"` e devolve a string.
//! Serve para a ponte do Spotify, cujas respostas são planas. Não serve para o
//! Harness, cujo protocolo é aninhado:
//!
//! ```text
//! {"jsonrpc":"2.0","id":1,"result":{"messageId":"m-7"}}
//! {"jsonrpc":"2.0","method":"session.event","params":{"event":{"kind":"tool"}}}
//! ```
//!
//! Extrair `result.messageId` por busca de substring funciona até o dia em que outra
//! chave se chamar `messageId` mais acima. Aí falha em silêncio, com o valor errado —
//! que é o pior tipo de falha, e o que este arquivo existe para evitar.
//!
//! ## O que ele NÃO é
//!
//! Não é uma biblioteca de JSON. O número é sempre `f64`, o objeto é um `Vec`, e não
//! há derive nem reflexão. É o suficiente para um protocolo, e o suficiente é a meta.
//!
//! ## Duas coisas que já morderam este projeto
//!
//! - **Entrada truncada não entra em pânico.** `gerador/http.rs` tem um teste com
//!   esse nome exato: a rede corta a resposta no meio e o processo não pode cair.
//! - **Profundidade tem teto.** Um JSON com dez mil colchetes abertos estoura a pilha
//!   num parser recursivo, e o processo morre sem log. O teto transforma isso num
//!   erro comum, que o chamador trata.

/// Um valor JSON.
///
/// O objeto é um `Vec` e não um mapa: a ordem das chaves é preservada, e busca linear
/// é mais rápida que hash para os punhados de chaves de um frame de protocolo.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Nulo,
    Bool(bool),
    Num(f64),
    Str(String),
    Lista(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// Quantos níveis de aninhamento antes de desistir.
///
/// O parser é recursivo, então profundidade vira pilha. Protocolo real não passa de
/// meia dúzia de níveis; 64 é folga generosa e ainda longe de estourar.
const FUNDO_MAXIMO: usize = 64;

impl Json {
    /// Segue um caminho separado por ponto: `"result.messageId"`.
    ///
    /// É o que torna a busca por substring desnecessária — `get` **desce** pela
    /// estrutura, então uma chave homônima em outro nível não confunde.
    ///
    /// Índice numérico entra em lista: `"params.blocks.0.text"`.
    pub fn get(&self, caminho: &str) -> Option<&Json> {
        let mut atual = self;
        for parte in caminho.split('.').filter(|p| !p.is_empty()) {
            match atual {
                Json::Obj(campos) => {
                    atual = campos.iter().find(|(k, _)| k == parte).map(|(_, v)| v)?;
                }
                Json::Lista(itens) => {
                    atual = itens.get(parte.parse::<usize>().ok()?)?;
                }
                _ => return None,
            }
        }
        Some(atual)
    }

    /// O texto, se for texto.
    ///
    /// `None` para qualquer outro tipo, de propósito: número onde se esperava string
    /// é erro de protocolo, não coisa a converter em silêncio.
    pub fn texto(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn numero(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn booleano(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn lista(&self) -> Option<&[Json]> {
        match self {
            Json::Lista(v) => Some(v),
            _ => None,
        }
    }

    /// Serializa. Escapa o que o RFC 8259 exige, e nada além.
    pub fn escrever(&self) -> String {
        let mut s = String::new();
        self.escrever_em(&mut s);
        s
    }

    fn escrever_em(&self, fora: &mut String) {
        match self {
            Json::Nulo => fora.push_str("null"),
            Json::Bool(true) => fora.push_str("true"),
            Json::Bool(false) => fora.push_str("false"),
            Json::Num(n) => {
                // NaN e infinito não existem em JSON. Viram `null` — melhor que emitir
                // texto inválido que o outro lado não consegue ler.
                if !n.is_finite() {
                    fora.push_str("null");
                } else if *n == n.trunc() && n.abs() < 1e15 {
                    // Inteiro sai sem ".0": `id` de JSON-RPC é inteiro, e alguns
                    // servidores recusam `1.0` onde esperam `1`.
                    fora.push_str(&format!("{}", *n as i64));
                } else {
                    fora.push_str(&format!("{n}"));
                }
            }
            Json::Str(s) => escrever_texto(s, fora),
            Json::Lista(v) => {
                fora.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        fora.push(',');
                    }
                    x.escrever_em(fora);
                }
                fora.push(']');
            }
            Json::Obj(c) => {
                fora.push('{');
                for (i, (k, v)) in c.iter().enumerate() {
                    if i > 0 {
                        fora.push(',');
                    }
                    escrever_texto(k, fora);
                    fora.push(':');
                    v.escrever_em(fora);
                }
                fora.push('}');
            }
        }
    }
}

fn escrever_texto(s: &str, fora: &mut String) {
    fora.push('"');
    for c in s.chars() {
        match c {
            '"' => fora.push_str("\\\""),
            '\\' => fora.push_str("\\\\"),
            '\n' => fora.push_str("\\n"),
            '\r' => fora.push_str("\\r"),
            '\t' => fora.push_str("\\t"),
            // De controle: o RFC exige escapar tudo abaixo de 0x20.
            c if (c as u32) < 0x20 => fora.push_str(&format!("\\u{:04x}", c as u32)),
            c => fora.push(c),
        }
    }
    fora.push('"');
}

/// Lê um valor JSON inteiro. Sobra depois do valor é erro.
pub fn ler(texto: &str) -> Result<Json, String> {
    let b: Vec<char> = texto.chars().collect();
    let mut i = 0usize;
    let v = valor(&b, &mut i, 0)?;
    pular_branco(&b, &mut i);
    if i < b.len() {
        return Err(format!("sobrou texto depois do valor, na posicao {i}"));
    }
    Ok(v)
}

fn pular_branco(b: &[char], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], ' ' | '\t' | '\n' | '\r') {
        *i += 1;
    }
}

fn valor(b: &[char], i: &mut usize, fundo: usize) -> Result<Json, String> {
    if fundo > FUNDO_MAXIMO {
        return Err(format!("aninhamento passou de {FUNDO_MAXIMO} niveis"));
    }
    pular_branco(b, i);
    let Some(&c) = b.get(*i) else {
        return Err("acabou antes de um valor".into());
    };
    match c {
        '{' => objeto(b, i, fundo),
        '[' => lista(b, i, fundo),
        '"' => texto(b, i).map(Json::Str),
        't' => literal(b, i, "true", Json::Bool(true)),
        'f' => literal(b, i, "false", Json::Bool(false)),
        'n' => literal(b, i, "null", Json::Nulo),
        _ => numero(b, i),
    }
}

fn literal(b: &[char], i: &mut usize, alvo: &str, v: Json) -> Result<Json, String> {
    for c in alvo.chars() {
        if b.get(*i) != Some(&c) {
            return Err(format!("esperava {alvo:?} na posicao {i}"));
        }
        *i += 1;
    }
    Ok(v)
}

fn objeto(b: &[char], i: &mut usize, fundo: usize) -> Result<Json, String> {
    *i += 1; // consome '{'
    let mut campos = Vec::new();
    pular_branco(b, i);
    if b.get(*i) == Some(&'}') {
        *i += 1;
        return Ok(Json::Obj(campos));
    }
    loop {
        pular_branco(b, i);
        if b.get(*i) != Some(&'"') {
            return Err(format!("esperava chave na posicao {i}"));
        }
        let k = texto(b, i)?;
        pular_branco(b, i);
        if b.get(*i) != Some(&':') {
            return Err(format!("esperava dois-pontos na posicao {i}"));
        }
        *i += 1;
        let v = valor(b, i, fundo + 1)?;
        campos.push((k, v));
        pular_branco(b, i);
        match b.get(*i) {
            Some(&',') => *i += 1,
            Some(&'}') => {
                *i += 1;
                return Ok(Json::Obj(campos));
            }
            _ => return Err(format!("esperava virgula ou fecha-chaves na posicao {i}")),
        }
    }
}

fn lista(b: &[char], i: &mut usize, fundo: usize) -> Result<Json, String> {
    *i += 1; // consome '['
    let mut itens = Vec::new();
    pular_branco(b, i);
    if b.get(*i) == Some(&']') {
        *i += 1;
        return Ok(Json::Lista(itens));
    }
    loop {
        itens.push(valor(b, i, fundo + 1)?);
        pular_branco(b, i);
        match b.get(*i) {
            Some(&',') => *i += 1,
            Some(&']') => {
                *i += 1;
                return Ok(Json::Lista(itens));
            }
            _ => return Err(format!("esperava virgula ou fecha-colchetes na posicao {i}")),
        }
    }
}

fn texto(b: &[char], i: &mut usize) -> Result<String, String> {
    *i += 1; // consome a aspa de abertura
    let mut fora = String::new();
    loop {
        let Some(&c) = b.get(*i) else {
            return Err("texto sem aspas de fechamento".into());
        };
        *i += 1;
        match c {
            '"' => return Ok(fora),
            '\\' => {
                let Some(&e) = b.get(*i) else {
                    return Err("escape sem o que escapar".into());
                };
                *i += 1;
                match e {
                    '"' => fora.push('"'),
                    '\\' => fora.push('\\'),
                    '/' => fora.push('/'),
                    'b' => fora.push('\u{8}'),
                    'f' => fora.push('\u{c}'),
                    'n' => fora.push('\n'),
                    'r' => fora.push('\r'),
                    't' => fora.push('\t'),
                    'u' => fora.push(escape_unicode(b, i)?),
                    _ => return Err(format!("escape desconhecido: {e:?}")),
                }
            }
            c => fora.push(c),
        }
    }
}

/// `\uXXXX`, incluindo o **par substituto** que o JSON usa para fora do BMP.
///
/// Emoji chega como DOIS escapes, e juntar os dois é obrigatório: tratar cada metade
/// isolada produz caractere inválido. Isto não é hipotético aqui — o John usa emoji em
/// nome de canal do Discord, e a árvore de acessibilidade devolve aquilo.
fn escape_unicode(b: &[char], i: &mut usize) -> Result<char, String> {
    let alto = hex4(b, i)?;
    // Metade ALTA de um par substituto: exige a baixa logo em seguida.
    if (0xD800..0xDC00).contains(&alto) {
        if b.get(*i) != Some(&'\\') || b.get(*i + 1) != Some(&'u') {
            return Err("metade alta de par substituto sem a baixa".into());
        }
        *i += 2;
        let baixo = hex4(b, i)?;
        if !(0xDC00..0xE000).contains(&baixo) {
            return Err("a segunda metade nao e substituto baixo".into());
        }
        let cp = 0x10000 + ((alto - 0xD800) << 10) + (baixo - 0xDC00);
        return char::from_u32(cp).ok_or_else(|| "ponto de codigo invalido".to_string());
    }
    char::from_u32(alto).ok_or_else(|| format!("ponto de codigo invalido: {alto:#x}"))
}

fn hex4(b: &[char], i: &mut usize) -> Result<u32, String> {
    let mut v = 0u32;
    for _ in 0..4 {
        let Some(&c) = b.get(*i) else {
            return Err("escape unicode truncado".into());
        };
        let d = c.to_digit(16).ok_or_else(|| format!("hex invalido: {c:?}"))?;
        v = v * 16 + d;
        *i += 1;
    }
    Ok(v)
}

fn numero(b: &[char], i: &mut usize) -> Result<Json, String> {
    let ini = *i;
    if b.get(*i) == Some(&'-') {
        *i += 1;
    }
    while matches!(b.get(*i), Some(c) if c.is_ascii_digit()
        || *c == '.' || *c == 'e' || *c == 'E' || *c == '+' || *c == '-')
    {
        *i += 1;
    }
    if *i == ini {
        return Err(format!("nao e um valor na posicao {ini}"));
    }
    let s: String = b[ini..*i].iter().collect();
    s.parse::<f64>()
        .map(Json::Num)
        .map_err(|_| format!("numero invalido: {s:?}"))
}

/// Atalho para montar objeto sem cerimônia.
pub fn obj(campos: Vec<(&str, Json)>) -> Json {
    Json::Obj(campos.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Atalho para string.
pub fn txt(s: &str) -> Json {
    Json::Str(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_os_frames_que_o_harness_manda() {
        let r = ler(r#"{"jsonrpc":"2.0","id":1,"result":{"messageId":"m-7"}}"#).unwrap();
        assert_eq!(r.get("result.messageId").and_then(Json::texto), Some("m-7"));
        assert_eq!(r.get("id").and_then(Json::numero), Some(1.0));

        let n = ler(r#"{"method":"session.event","params":{"event":{"kind":"tool"}}}"#).unwrap();
        assert_eq!(n.get("method").and_then(Json::texto), Some("session.event"));
        assert_eq!(n.get("params.event.kind").and_then(Json::texto), Some("tool"));
    }

    /// O motivo de existir: chave homônima em outro nível não pode confundir.
    ///
    /// A busca por substring que a `campo_json` faz devolveria `"errado"` aqui — o
    /// primeiro `messageId` do texto — e devolveria em silêncio.
    #[test]
    fn caminho_desce_e_nao_casa_homonimo_de_outro_nivel() {
        let j = ler(r#"{"echo":{"messageId":"errado"},"result":{"messageId":"certo"}}"#).unwrap();
        assert_eq!(j.get("result.messageId").and_then(Json::texto), Some("certo"));
        assert_eq!(j.get("messageId"), None, "nao existe messageId na raiz");
    }

    #[test]
    fn indice_entra_em_lista() {
        let j = ler(r#"{"blocks":[{"text":"a"},{"text":"b"}]}"#).unwrap();
        assert_eq!(j.get("blocks.1.text").and_then(Json::texto), Some("b"));
        assert_eq!(j.get("blocks").and_then(Json::lista).map(<[_]>::len), Some(2));
        assert_eq!(j.get("blocks.9.text"), None);
    }

    /// Entrada truncada devolve erro, nunca pânico. Ver a nota do módulo.
    #[test]
    fn truncado_nao_entra_em_panico() {
        for ruim in [
            "",
            "{",
            "[",
            r#"{"a""#,
            r#"{"a":"#,
            r#"{"a":1,"#,
            r#""sem fim"#,
            r#"{"a":"\"#,
            r#"{"a":"\u12"#,
            "tru",
            "[1,2",
            "{}extra",
        ] {
            assert!(ler(ruim).is_err(), "{ruim:?} devia dar erro e nao deu");
        }
    }

    /// Aninhamento fundo vira erro comum, e não pilha estourada.
    #[test]
    fn fundo_demais_e_erro_e_nao_queda() {
        let fundo = "[".repeat(FUNDO_MAXIMO + 10) + &"]".repeat(FUNDO_MAXIMO + 10);
        let e = ler(&fundo).unwrap_err();
        assert!(e.contains("aninhamento"), "erro inesperado: {e}");

        // E o que cabe no teto continua funcionando.
        let ok = "[".repeat(10) + &"]".repeat(10);
        assert!(ler(&ok).is_ok());
    }

    /// Emoji chega como par substituto. Nome de canal do Discord tem disso.
    #[test]
    fn par_substituto_vira_um_caractere_so() {
        let j = ler(r#"{"canal":"🔊 Sem Mod"}"#).unwrap();
        assert_eq!(j.get("canal").and_then(Json::texto), Some("🔊 Sem Mod"));

        // Metade solta é erro, e não caractere inválido em silêncio.
        assert!(ler(r#"{"a":"\uD83D"}"#).is_err());
    }

    #[test]
    fn escapes_comuns_vao_e_voltam() {
        let original = "aspas \" barra \\ linha \n tab \t fim";
        let escrito = txt(original).escrever();
        assert_eq!(ler(&escrito).unwrap().texto(), Some(original));
    }

    /// `id` de JSON-RPC é inteiro. Alguns servidores recusam `1.0` onde esperam `1`.
    #[test]
    fn inteiro_sai_sem_casa_decimal() {
        assert_eq!(Json::Num(1.0).escrever(), "1");
        assert_eq!(Json::Num(-42.0).escrever(), "-42");
        assert_eq!(Json::Num(1.5).escrever(), "1.5");
        // NaN e infinito não existem em JSON.
        assert_eq!(Json::Num(f64::NAN).escrever(), "null");
        assert_eq!(Json::Num(f64::INFINITY).escrever(), "null");
    }

    #[test]
    fn monta_a_chamada_que_a_ponte_vai_mandar() {
        let pedido = obj(vec![
            ("jsonrpc", txt("2.0")),
            ("id", Json::Num(1.0)),
            ("method", txt("session/prompt")),
            (
                "params",
                obj(vec![
                    ("sessionId", txt("s-1")),
                    ("contentBlocks", Json::Lista(vec![obj(vec![
                        ("type", txt("text")),
                        ("text", txt("lista os arquivos")),
                    ])])),
                ]),
            ),
        ]);
        let linha = pedido.escrever();
        assert!(!linha.contains('\n'), "o frame nao pode ter nova linha dentro");
        // E o que sai tem de voltar igual.
        assert_eq!(ler(&linha).unwrap(), pedido);
        assert_eq!(
            ler(&linha).unwrap().get("params.contentBlocks.0.text").and_then(Json::texto),
            Some("lista os arquivos")
        );
    }
}
