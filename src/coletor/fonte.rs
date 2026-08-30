//! Onde o texto é buscado. **A única parte que toca a rede.**
//!
//! Delega o download ao cliente HTTP do sistema (`curl`, e no Windows o
//! `Invoke-WebRequest` do PowerShell). A razão é simples: a Wikipédia só serve por
//! HTTPS, não há TLS neste projeto, e não vai haver — implementar TLS à mão para
//! alimentar um coletor de desenvolvimento seria trocar um problema resolvido por um
//! problema perigoso.
//!
//! O runtime da Teka continua sem rede e sem dependência. Isto aqui roda no
//! desenvolvimento, produz um arquivo de texto, e some.
//!
//! ## Boas maneiras com quem serve o texto
//!
//! Há um intervalo entre pedidos e um `User-Agent` que identifica o projeto. Não é
//! educação abstrata: um coletor que martela um servidor é bloqueado, e aí não
//! coleta nada. Wikipédia pede identificação explicitamente na política de API dela.

use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use super::{Pedaco, DOMINIOS};

pub const AGENTE: &str = "Teka-coletor/0.1 (projeto pessoal de pesquisa; contato via repositorio)";
/// Intervalo entre pedidos ao mesmo host.
pub const PAUSA_MS: u64 = 1_000;

/// O host de uma URL está na lista permitida?
///
/// Compara o host **inteiro**, não por `contains`: `pt.wikipedia.org.evil.com`
/// passaria num teste de substring, e é exatamente assim que uma allowlist frouxa
/// deixa de ser allowlist.
pub fn permitido(url: &str) -> bool {
    let sem_esquema = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = sem_esquema
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    DOMINIOS.iter().any(|d| *d == host)
}

/// Busca uma URL. Recusa qualquer coisa fora da lista, antes de tocar na rede.
pub fn buscar(url: &str, timeout_s: u64) -> Result<String, String> {
    if !permitido(url) {
        return Err(format!(
            "recusado: {url} nao esta na lista de dominios permitidos"
        ));
    }
    // `curl` primeiro; PowerShell como reserva no Windows sem curl.
    let saida = Command::new("curl")
        .args([
            "-sS",
            "--fail",
            "--max-time",
            &timeout_s.to_string(),
            "-A",
            AGENTE,
            url,
        ])
        .output();

    match saida {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(format!(
            "curl falhou: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(_) => powershell(url, timeout_s),
    }
}

fn powershell(url: &str, timeout_s: u64) -> Result<String, String> {
    let script = format!(
        "$ProgressPreference='SilentlyContinue'; \
         (Invoke-WebRequest -Uri '{}' -UseBasicParsing -TimeoutSec {} \
          -Headers @{{'User-Agent'='{}'}}).Content",
        url.replace('\'', "''"),
        timeout_s,
        AGENTE
    );
    let o = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| format!("nem curl nem powershell disponiveis: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(format!(
            "powershell falhou: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

/// Busca com recuo exponencial diante de 429.
///
/// Existe porque a fase de descobrir títulos não tinha recuo nenhum, só [`buscar`]
/// direto, e isso custou caro numa medição real: dos 560 pedidos de sorteio de uma
/// coleta, **123 passaram e 437 levaram 429** — franquia inicial e depois
/// estrangulamento. Os títulos perdidos não voltavam, e o trabalho seguiu com 60 mil
/// em vez dos 280 mil pedidos.
///
/// [`colher`] já tinha esta lógica embutida; o erro foi não tê-la nos dois lugares.
pub fn buscar_teimoso(url: &str, timeout_s: u64, tentativas: usize) -> Result<String, String> {
    let mut espera = 2_000u64;
    let mut ultimo = String::new();
    for n in 0..tentativas.max(1) {
        match buscar(url, timeout_s) {
            Ok(corpo) => return Ok(corpo),
            Err(e) => {
                ultimo = e;
                // Só vale insistir em estrangulamento. Recusa por domínio, URL
                // errada ou rede fora não melhoram com espera.
                if !ultimo.contains("429") || n + 1 == tentativas.max(1) {
                    return Err(ultimo);
                }
                eprintln!("  429: esperando {espera}ms");
                sleep(Duration::from_millis(espera));
                espera = (espera * 2).min(60_000);
            }
        }
    }
    Err(ultimo)
}

/// Quantos títulos por pedido.
///
/// A API aceita até 50. **É isto que resolve o 429**, não pausa maior: a primeira
/// versão pedia uma página por vez e levou HTTP 429 depois de ~42 páginas, mesmo
/// com 1s de intervalo. 500 páginas em 25 pedidos em vez de 500 é 20x menos
/// carga — e a única forma honesta de pedir muito texto a um servidor alheio.
pub const POR_PEDIDO: usize = 20;

/// Monta a URL da API da Wikipédia que devolve **texto puro** de vários títulos.
///
/// `explaintext` evita ter de interpretar HTML ou wikitexto — a maior fonte de
/// entulho num coletor é justamente a marcação mal removida.
pub fn url_wikipedia(titulos: &[String], so_introducao: bool) -> String {
    let lista: Vec<String> = titulos.iter().map(|t| codificar(t)).collect();
    // A API disse a regra em voz alta e eu levei duas rodadas para ler:
    //
    //   "exlimit" was too large for a whole article extracts request, lowered to 1.
    //
    // **Artigo inteiro e sempre 1 por pedido.** `exlimit=max` so vale com
    // `exintro`. Sem isso, um lote de 20 titulos devolve 20 paginas e o extrato de
    // uma — que foi como 15 lotes renderam 14 extratos, sem erro nenhum aparecer.
    let extra = if so_introducao {
        "&exintro=1&exlimit=max"
    } else {
        "&exlimit=1"
    };
    format!(
        "https://pt.wikipedia.org/w/api.php?action=query&prop=extracts&explaintext=1\
         {extra}&format=json&maxlag=5&redirects=1&titles={}",
        lista.join("%7C") // '|' codificado
    )
}

/// URL da API que lista páginas de uma categoria.
///
/// `cmtype=page` pede **só artigo**, não subcategoria. Sem isso boa parte do que
/// volta de uma categoria ampla são outras categorias, que [`extrair_titulos`] depois
/// descarta pelo prefixo — gastando pedido para trazer o que vai ser jogado fora.
///
/// Categoria rende muito mais byte por pedido que sorteio, e isso foi medido: 40
/// títulos de "Algoritmos" em modo introdução deram 17.852 bytes por pedido, contra
/// ~5.600 de títulos sorteados, porque artigo sorteado é stub em 60% das vezes.
/// Quando o limite do servidor é por *pedido*, é essa razão que manda.
pub fn url_categoria(categoria: &str, limite: usize) -> String {
    format!(
        "https://pt.wikipedia.org/w/api.php?action=query&list=categorymembers\
         &cmtitle=Categoria:{}&cmtype=page&cmlimit={}&format=json&maxlag=5",
        codificar(categoria),
        limite.min(500)
    )
}

/// URL da API que **sorteia** páginas do espaço principal.
///
/// Existe porque categoria e sorteio respondem a perguntas diferentes. Categoria dá
/// vocabulário dirigido — foi assim que "arquivo" entrou num corpus de literatura de
/// 1898 onde aparecia zero vezes. Sorteio dá *largura*: um corpus colhido só de
/// categorias de informática ensina um registro só, e um modelo de entropia treinado
/// nele fica confiante demais num padrão estreito — exatamente o defeito que o
/// patcher por entropia não pode ter.
///
/// `rnfilterredir=nonredirects` evita gastar pedido com redirecionamento, que
/// devolveria o mesmo texto duas vezes sob títulos diferentes.
pub fn url_aleatorio(limite: usize) -> String {
    format!(
        "https://pt.wikipedia.org/w/api.php?action=query&list=random\
         &rnnamespace=0&rnfilterredir=nonredirects&rnlimit={}&format=json&maxlag=5",
        limite.clamp(1, 500)
    )
}

/// Teto real de `aplimit` para cliente anônimo.
///
/// A documentação diz 500, mas 500 é para quem tem `apihighlimits` (conta de robô).
/// Anônimo recebe 50. Pedir 500 não dá erro — devolve 50 calado, e foi assim que um
/// laço que descontava 500 do orçamento por pedido colheu 1.400 títulos onde eu
/// achava que colhia 20.000. **Descontar o pedido em vez do recebido é a armadilha.**
pub const POR_ENUMERACAO: usize = 50;

/// Onde cada faixa da enumeração começa.
///
/// A enumeração alfabética crua é inútil aqui, e isso custou uma noite: ela começa nos
/// títulos numéricos, e a Wikipédia em português nessa faixa é dominada por stub de
/// asteroide — `(38620) 2000 AQ186` e dezenas de milhares de irmãos, todos abaixo dos
/// 400 bytes do filtro. O cursor passou horas moendo o que ia ser descartado.
///
/// Vinte e seis faixas, uma por letra, cada uma com seu cursor e limitada pela letra
/// seguinte: o acervo é varrido em paralelo lógico, sem repetir e sem afundar num
/// bolsão de lixo. Números ficam de fora de propósito.
pub const FAIXAS: &[(&str, &str)] = &[
    ("A", "B"), ("B", "C"), ("C", "D"), ("D", "E"), ("E", "F"), ("F", "G"),
    ("G", "H"), ("H", "I"), ("I", "J"), ("J", "K"), ("K", "L"), ("L", "M"),
    ("M", "N"), ("N", "O"), ("O", "P"), ("P", "Q"), ("Q", "R"), ("R", "S"),
    ("S", "T"), ("T", "U"), ("U", "V"), ("V", "W"), ("W", "X"), ("X", "Y"),
    ("Y", "Z"), ("Z", "ZZZZ"),
];

/// URL que **enumera** o espaço principal, de `de` até `ate`.
///
/// Existe porque as outras duas fontes de título se esgotam ou se repetem. Categoria
/// devolve os mesmos títulos toda rodada — e com retomada ligada, a segunda rodada
/// colhe zero deles. Sorteio não repete, mas rende stub em 60% das vezes.
///
/// `apfilterredir=nonredirects` evita gastar pedido com redirecionamento, que traria
/// o mesmo texto sob outro título.
pub fn url_todas_paginas(de: &str, ate: &str, limite: usize) -> String {
    let mut faixa = String::new();
    if !de.is_empty() {
        faixa.push_str(&format!("&apfrom={}", codificar(de)));
    }
    if !ate.is_empty() {
        faixa.push_str(&format!("&apto={}", codificar(ate)));
    }
    format!(
        "https://pt.wikipedia.org/w/api.php?action=query&list=allpages\
         &apnamespace=0&apfilterredir=nonredirects&aplimit={}{}&format=json&maxlag=5",
        limite.clamp(1, POR_ENUMERACAO),
        faixa
    )
}

/// Onde a enumeração parou, para a próxima rodada continuar dali.
///
/// Sem isto cada rodada recomeçaria do "A" e colheria o que já está em disco — a
/// retomada pularia tudo e a rodada renderia zero.
pub fn extrair_continuacao(json: &str) -> Option<String> {
    let i = json.find("\"apcontinue\":\"")?;
    let apos = &json[i + "\"apcontinue\":\"".len()..];
    let fim = apos.find('"')?;
    Some(desescapar_unicode(&apos[..fim]))
}

/// Percent-encoding do que não é seguro em URL.
pub fn codificar(s: &str) -> String {
    let mut saida = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                saida.push(*b as char)
            }
            b' ' => saida.push('_'),
            outro => saida.push_str(&format!("%{outro:02X}")),
        }
    }
    saida
}

/// Extrai os `extract` de uma resposta da API. Sem parser de JSON — o formato é
/// conhecido e o extrator falha devolvendo vazio, igual ao do gerador.
pub fn extrair_extratos(json: &str) -> Vec<String> {
    let mut saida = Vec::new();
    let mut resto = json;
    while let Some(i) = resto.find("\"extract\":") {
        let apos = &resto[i + "\"extract\":".len()..];
        let Some(abre) = apos.find('"') else { break };
        let bytes = apos.as_bytes();
        let mut j = abre + 1;
        let mut texto = String::new();
        while j < bytes.len() {
            match bytes[j] {
                b'"' => break,
                b'\\' => {
                    j += 1;
                    match bytes.get(j) {
                        Some(b'n') => texto.push('\n'),
                        Some(b't') => texto.push('\t'),
                        Some(b'u') => {
                            if let Some(hex) = apos.get(j + 1..j + 5) {
                                if let Ok(n) = u32::from_str_radix(hex, 16) {
                                    texto.push(char::from_u32(n).unwrap_or(' '));
                                }
                            }
                            j += 4;
                        }
                        Some(outro) => texto.push(*outro as char),
                        None => break,
                    }
                    j += 1;
                }
                _ => {
                    let ini = j;
                    while j < bytes.len() && bytes[j] != b'"' && bytes[j] != b'\\' {
                        j += 1;
                    }
                    texto.push_str(&String::from_utf8_lossy(&bytes[ini..j]));
                }
            }
        }
        if !texto.trim().is_empty() {
            saida.push(texto);
        }
        resto = &apos[j.min(apos.len())..];
    }
    saida
}

/// Decodifica `\uXXXX` para o caractere real.
///
/// A primeira versão trocava `ç` por `ç` e mais nada, o que produziu títulos
/// como `Unidade de alocação` — que a API depois não encontra, porque o título
/// não existe com aquela sequência literal. Meia decodificação é pior que nenhuma:
/// falha silenciosa em vez de erro.
/// Decodifica `\uXXXX` para o caractere de verdade.
///
/// Publica porque a busca web (`tools::prim`) precisa da mesma decodificacao: a
/// API da DuckDuckGo devolve JSON escapado, e sem isto todo acento em portugues
/// chegava ao usuario como `\u00e9` cru — o que e quase todo o texto.
pub fn desescapar_unicode(s: &str) -> String {
    let b = s.as_bytes();
    let mut saida = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'u') {
            if let Some(hex) = s.get(i + 2..i + 6) {
                if let Ok(n) = u32::from_str_radix(hex, 16) {
                    if let Some(c) = char::from_u32(n) {
                        saida.push(c);
                        i += 6;
                        continue;
                    }
                }
            }
        }
        // Copia byte a byte respeitando UTF-8 multibyte.
        let ini = i;
        i += 1;
        while i < b.len() && (b[i] & 0xC0) == 0x80 {
            i += 1;
        }
        saida.push_str(&String::from_utf8_lossy(&b[ini..i]));
    }
    saida
}

/// Extrai os títulos de uma resposta de `categorymembers`.
pub fn extrair_titulos(json: &str) -> Vec<String> {
    let mut saida = Vec::new();
    let mut resto = json;
    while let Some(i) = resto.find("\"title\":\"") {
        let apos = &resto[i + "\"title\":\"".len()..];
        let Some(fim) = apos.find('"') else { break };
        let t = desescapar_unicode(&apos[..fim]);
        // Páginas de serviço não são texto.
        if !t.starts_with("Categoria:")
            && !t.starts_with("Predefinição:")
            && !t.starts_with("Ficheiro:")
            && !t.starts_with("Wikipédia:")
            && !t.starts_with("Anexo:")
        {
            saida.push(t);
        }
        resto = &apos[fim..];
    }
    saida
}

/// Busca os títulos em lotes, com pausa entre lotes.
///
/// Em caso de 429 a espera **dobra** antes de tentar de novo. Insistir no mesmo
/// ritmo depois de levar 429 é como o coletor anterior perdeu 458 das 500 páginas:
/// o servidor já disse para diminuir, e continuar batendo só prolonga o bloqueio.
pub fn colher(
    titulos: &[String],
    timeout_s: u64,
    so_introducao: bool,
    pausa_ms: u64,
) -> Vec<Pedaco> {
    let mut saida = Vec::new();
    // Artigo inteiro vai 1 por pedido, então o intervalo tem de ser bem maior: são
    // 500 pedidos em vez de 25, e foi exatamente essa rajada que rendeu os 429.
    let base = pausa_ms.max(PAUSA_MS);
    let mut espera = if so_introducao { base } else { base.max(2_500) };
    let por_pedido = if so_introducao { POR_PEDIDO } else { 1 };

    for (n, lote) in titulos.chunks(por_pedido).enumerate() {
        if n > 0 {
            sleep(Duration::from_millis(espera));
        }
        let url = url_wikipedia(lote, so_introducao);
        let mut tentativa = 0;
        loop {
            match buscar(&url, timeout_s) {
                Ok(corpo) => {
                    let extratos = extrair_extratos(&corpo);
                    let titulos_resp = extrair_titulos(&corpo);
                    if so_introducao || (n + 1) % 25 == 0 {
                        println!(
                            "  {} {}: {} de {} paginas",
                            if so_introducao { "lote" } else { "pagina" },
                            n + 1,
                            extratos.len(),
                            lote.len()
                        );
                    }
                    for (k, texto) in extratos.into_iter().enumerate() {
                        // A origem sai do titulo que a propria resposta traz —
                        // `redirects=1` faz o titulo pedido diferir do entregue.
                        let titulo = titulos_resp
                            .get(k)
                            .cloned()
                            .unwrap_or_else(|| lote.get(k).cloned().unwrap_or_default());
                        saida.push(Pedaco {
                            texto,
                            origem: format!("pt.wikipedia.org/wiki/{}", codificar(&titulo)),
                        });
                    }
                    // Deu certo: relaxa, mas MUITO devagar.
                    //
                    // A versao anterior tirava 200ms fixos por acerto. Depois de um
                    // 429 levar a espera a 8s, ela voltava perto do piso em poucos
                    // pedidos e tomava 429 de novo — o log da coleta grande mostrou
                    // essa oscilacao repetida, e nao era limite inevitavel do
                    // servidor, era eu descendo rapido demais. Corte geometrico de 5%
                    // leva ~14 acertos para desfazer uma dobra, que e a proporcao
                    // certa entre subir na recusa e descer na permissao.
                    espera = (espera * 95 / 100).max(base);
                    break;
                }
                Err(e) if e.contains("429") && tentativa < 8 => {
                    tentativa += 1;
                    espera = (espera * 2).min(120_000);
                    eprintln!("  429 no lote {}: esperando {espera}ms", n + 1);
                    sleep(Duration::from_millis(espera));
                }
                Err(e) => {
                    eprintln!("  lote {}: {e}", n + 1);
                    break;
                }
            }
        }
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_allowlist_compara_o_host_inteiro() {
        assert!(permitido("https://pt.wikipedia.org/w/api.php?x=1"));
        assert!(permitido("https://docs.python.org/pt-br/3/"));
        // O caso que um `contains` deixaria passar.
        assert!(!permitido("https://pt.wikipedia.org.evil.com/roubar"));
        assert!(!permitido("https://evil.com/?pt.wikipedia.org"));
        // Userinfo na URL e o outro truque classico.
        assert!(!permitido("https://pt.wikipedia.org@evil.com/x"));
        assert!(!permitido("https://en.wikipedia.org/wiki/File"));
    }

    #[test]
    fn codifica_o_que_quebraria_a_url() {
        assert_eq!(codificar("Sistema de arquivos"), "Sistema_de_arquivos");
        assert_eq!(codificar("C++"), "C%2B%2B");
        // Acento vira UTF-8 percent-encoded.
        assert_eq!(codificar("memória"), "mem%C3%B3ria");
        assert_eq!(codificar("a/b?c=d"), "a%2Fb%3Fc%3Dd");
    }

    #[test]
    fn extrai_os_extratos_da_api() {
        let j = r#"{"query":{"pages":{"1":{"title":"A","extract":"primeiro texto\ncom quebra"},
                    "2":{"title":"B","extract":"segundo com acento: memória"}}}}"#;
        let v = extrair_extratos(j);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], "primeiro texto\ncom quebra");
        assert!(v[1].contains("memória"), "veio: {}", v[1]);
    }

    #[test]
    fn extrato_vazio_e_json_torto_nao_entram_em_panico() {
        assert!(extrair_extratos(r#"{"query":{"pages":{}}}"#).is_empty());
        assert!(extrair_extratos(r#"{"extract":"#).is_empty());
        assert!(extrair_extratos(r#"{"extract":"   "}"#).is_empty());
    }

    #[test]
    fn titulos_de_servico_nao_viram_pagina() {
        let j = r#"{"query":{"categorymembers":[
            {"title":"Sistema de arquivos"},
            {"title":"Categoria:Informática"},
            {"title":"Predefinição:Info"},
            {"title":"Memória RAM"}]}}"#;
        let v = extrair_titulos(j);
        assert_eq!(v, vec!["Sistema de arquivos", "Memória RAM"]);
    }

    #[test]
    fn a_url_da_api_pede_texto_puro_e_varios_titulos() {
        let u = url_wikipedia(&["Sistema de arquivos".into(), "Memoria RAM".into()], true);
        assert!(u.contains("explaintext=1"), "sem isso vem wikitexto: {u}");
        assert!(
            u.contains("exlimit=max") && u.contains("exintro=1"),
            "lote so funciona com exintro; a API forca exlimit=1 em artigo inteiro: {u}"
        );
        // Artigo inteiro NAO pode pedir lote: a API recusa e devolve um so.
        let inteiro = url_wikipedia(&["A".into()], false);
        assert!(inteiro.contains("exlimit=1"), "{inteiro}");
        assert!(!inteiro.contains("exintro"), "{inteiro}");
        assert!(u.contains("titles=Sistema_de_arquivos%7CMemoria_RAM"));
        assert!(permitido(&u));
    }

    #[test]
    fn decodifica_unicode_de_verdade_e_nao_so_o_c_cedilha() {
        // O bug real: "Unidade de aloca\u00e7\u00e3o" virava "alocação",
        // com o ã intacto — titulo que a API depois nao encontra.
        let j = r#"{"query":{"categorymembers":[
            {"title":"Unidade de alocação"},
            {"title":"Memória RAM"},
            {"title":"Anexo:Lista de coisas"}]}}"#;
        let v = extrair_titulos(j);
        assert_eq!(v, vec!["Unidade de alocação", "Memória RAM"]);
    }

    #[test]
    fn o_lote_e_o_que_evita_o_429() {
        // 500 paginas em lotes de 20 sao 25 pedidos, nao 500.
        assert!(POR_PEDIDO >= 10 && POR_PEDIDO <= 50);
        let muitos: Vec<String> = (0..500).map(|i| format!("P{i}")).collect();
        assert_eq!(muitos.chunks(POR_PEDIDO).count(), 25);
    }
}
