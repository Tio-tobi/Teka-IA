//! A busca da Teka pela **página de resultados** do DuckDuckGo.
//!
//! ## O teto que isto derruba
//!
//! O `buscar_web` usava a *Instant Answer API*, e o roteiro registrava o limite dela
//! como estrutural: **só responde termo único de enciclopédia**. Frase composta volta
//! vazia. Quem pergunta *"que configuração usar"* recebe verbete, ou nada.
//!
//! Estava escrito como *"nenhum treino resolve — só trocar a fonte"*.
//!
//! ## Por que esta fonte, e não a paga
//!
//! O `web_search` do Harness resolveria, mas exige `DEEPSEEK_API_KEY` — é um provedor
//! pago. Decisão do John em 2026-09-09: *"não vale a pena usar API do DeepSeek para
//! pesquisa sendo que tem como arrumar de graça com o DuckDuckGo"*.
//!
//! E ele está certo: `html.duckduckgo.com/html/` devolve a lista de resultados de
//! verdade, sem chave e sem cota. Medido em 10/09: HTTP 200, 9 resultados para a
//! mesma consulta que a Instant Answer devolvia vazia.
//!
//! ## O preço, dito de frente
//!
//! Isto é **raspagem**, e raspagem é frágil por natureza: o HTML deles muda sem
//! aviso e sem versão. O que dá para fazer não é evitar a quebra — é torná-la
//! **visível**:
//!
//! - o extrator é testado contra um recorte REAL da página
//!   (`dados/fixtures/ddg_resultados.html`), então mudança de layout vira teste
//!   vermelho em vez de busca que silenciosamente para de achar;
//! - e quando a extração não rende nada, o `buscar_web` cai na Instant Answer, que
//!   continua lá. Perde-se o teto melhor, não a ferramenta.

use crate::coletor;

/// Um resultado da página.
#[derive(Debug, Clone, PartialEq)]
pub struct Resultado {
    pub titulo: String,
    pub trecho: String,
    pub url: String,
}

/// O host da página de resultados. Entra na **mesma** allowlist do coletor.
pub const HOST: &str = "html.duckduckgo.com";

/// Monta a URL da busca.
///
/// `kl=br-pt` pede resultado em português do Brasil — a mesma região que a Instant
/// Answer já usava.
pub fn url(consulta: &str) -> String {
    format!("https://{HOST}/html/?q={}&kl=br-pt", escapar_consulta(consulta))
}

/// Escapa uma CONSULTA para a query string.
///
/// O `coletor::fonte::codificar` nao serve aqui, e nao por descuido dele: ele troca
/// espaco por `_` porque foi escrito para **titulo de Wikipedia**, onde sublinhado
/// **e** a convencao (`Rio_de_Janeiro`). Numa busca, `_` nao e espaco — e o
/// DuckDuckGo so tolerou por gentileza.
///
/// Reusar aquela funcao aqui teria funcionado o suficiente para eu nao notar, que e
/// o pior tipo de quase-certo.
fn escapar_consulta(s: &str) -> String {
    let mut fora = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                fora.push(*b as char)
            }
            b' ' => fora.push('+'),
            outro => fora.push_str(&format!("%{outro:02X}")),
        }
    }
    fora
}

/// Extrai os resultados do HTML.
///
/// Não é parser de HTML e não precisa ser: procura as duas classes que carregam o
/// que interessa e recorta entre `>` e `</a>`. Um parser completo seria mais código
/// para a mesma fragilidade — o que quebra aqui é o **nome da classe** mudar, e
/// nenhum parser protege disso.
pub fn extrair(html: &str) -> Vec<Resultado> {
    let titulos = fatiar(html, "class=\"result__a\"");
    let trechos = fatiar(html, "class=\"result__snippet\"");

    titulos
        .into_iter()
        .zip(trechos)
        .filter_map(|((titulo, href), (trecho, _))| {
            let titulo = coletor::limpar(&titulo);
            let trecho = coletor::limpar(&trecho);
            if titulo.is_empty() && trecho.is_empty() {
                return None;
            }
            Some(Resultado { titulo, trecho, url: url_real(&href) })
        })
        .collect()
}

/// Todos os `(texto, href)` de uma classe.
fn fatiar(html: &str, marca: &str) -> Vec<(String, String)> {
    let mut fora = Vec::new();
    let mut resto = html;
    while let Some(i) = resto.find(marca) {
        resto = &resto[i + marca.len()..];
        // O `href` vem DEPOIS da classe nesta pagina; se um dia vier antes, o teste
        // com o recorte real acusa.
        let href = entre(resto, "href=\"", "\"").unwrap_or_default();
        let Some(texto) = entre(resto, ">", "</a>") else { continue };
        fora.push((texto, href));
    }
    fora
}

fn entre(s: &str, de: &str, ate: &str) -> Option<String> {
    let i = s.find(de)? + de.len();
    let resto = &s[i..];
    let fim = resto.find(ate)?;
    Some(resto[..fim].to_string())
}

/// O link real, tirado de dentro do redirecionador deles.
///
/// A pagina nao entrega a URL direta: entrega
/// `//duckduckgo.com/l/?uddg=<url escapada>&rut=...`. Sem desembrulhar, a fonte que
/// aparece para o John seria sempre "duckduckgo.com", o que nao informa nada.
fn url_real(href: &str) -> String {
    let Some(i) = href.find("uddg=") else {
        return href.trim_start_matches("//").to_string();
    };
    let bruto = &href[i + 5..];
    let fim = bruto.find('&').unwrap_or(bruto.len());
    despercentar(&bruto[..fim])
}

/// Desfaz `%XX`. O `&amp;` da pagina ja saiu no `limpar`.
fn despercentar(s: &str) -> String {
    let b = s.as_bytes();
    let mut fora: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(a), Some(c)) = (hex(b[i + 1]), hex(b[i + 2])) {
                fora.push((a * 16 + c) as u8);
                i += 3;
                continue;
            }
        }
        fora.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&fora).into_owned()
}

/// A resposta veio, mas veio SEM resultado nenhum. Por quê?
///
/// A distinção importa e é a mesma de sempre: **"não consegui buscar" não é "não
/// achei nada"**, e juntar as duas esconde a primeira. Quem lê "nada encontrado"
/// procura outra consulta; quem lê "fui bloqueado" espera e tenta de novo.
///
/// O DuckDuckGo bloqueia pedido rápido demais com **HTTP 200 e uma página sem
/// resultados** — não com 429. Então `buscar_teimoso`, que já espera em 429, não
/// pega este caso. Medido em 10/09, a mesma consulta duas vezes seguidas:
///
/// ```text
/// 1a isolada    34 KB, 10 resultados
/// 2a imediata   14 KB,  0 resultados
/// ```
///
/// O sinal que separa: uma página de resultados de verdade **sempre** traz a classe
/// `result__a`, mesmo quando a busca não achou nada — aí ela vem com a caixa de "no
/// results". Página sem a classe é bloqueio ou layout mudado, e nos dois casos a
/// resposta honesta é "não consegui", nunca "não achei".
pub fn parece_bloqueio(html: &str) -> bool {
    !html.contains("result__a")
}

/// Junta os resultados num texto para a Teka devolver.
///
/// Poucos, e curtos: a saída dela vai para um terminal e, um dia, para a janela de
/// contexto de outro turno. Despejar dez resultados inteiros seria trocar um teto
/// baixo por um despejo.
pub fn resumir(rs: &[Resultado], quantos: usize) -> String {
    rs.iter()
        .take(quantos)
        .map(|r| {
            let t: String = r.trecho.chars().take(280).collect();
            format!("• {}\n  {}\n  {}", r.titulo, t, r.url)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O recorte é HTML REAL da página, capturado em 2026-09-10.
    ///
    /// É isto que torna a quebra visível: no dia em que o DuckDuckGo mudar o nome
    /// das classes, este teste fica vermelho — em vez de a busca simplesmente parar
    /// de achar e ninguém notar.
    const REAL: &str = include_str!("../../dados/fixtures/ddg_resultados.html");

    #[test]
    fn extrai_do_html_de_verdade() {
        let rs = extrair(REAL);
        assert!(!rs.is_empty(), "nao extraiu nada do recorte real");
        let r = &rs[0];
        assert!(!r.titulo.is_empty(), "titulo vazio");
        assert!(!r.trecho.is_empty(), "trecho vazio");
        assert!(
            r.url.starts_with("http"),
            "a url tinha de vir desembrulhada, veio {:?}",
            r.url
        );
        assert!(
            !r.url.contains("duckduckgo.com/l/"),
            "a url ficou dentro do redirecionador: {:?}",
            r.url
        );
    }

    /// A marcação `<b>` do destaque não pode vazar para o texto.
    #[test]
    fn o_trecho_vem_sem_marcacao_nem_entidade() {
        let rs = extrair(REAL);
        for r in &rs {
            assert!(!r.trecho.contains('<'), "sobrou tag: {:?}", r.trecho);
            assert!(!r.trecho.contains("&amp;"), "sobrou entidade: {:?}", r.trecho);
            assert!(!r.titulo.contains('<'), "sobrou tag no titulo: {:?}", r.titulo);
        }
    }

    #[test]
    fn desembrulha_o_redirecionador() {
        let h = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexemplo.com%2Fa%20b&amp;rut=x";
        assert_eq!(url_real(h), "https://exemplo.com/a b");
        // Sem `uddg`, devolve o que veio, sem as barras da frente.
        assert_eq!(url_real("//exemplo.com/direto"), "exemplo.com/direto");
    }

    /// HTML vazio ou estranho devolve lista vazia, nunca pânico.
    ///
    /// Raspagem recebe o que o servidor mandar — inclusive uma pagina de erro, um
    /// captcha, ou meio arquivo. Nenhum desses pode derrubar a Teka.
    #[test]
    fn html_estranho_nao_entra_em_panico() {
        for ruim in ["", "<html></html>", "class=\"result__a\"", "<a href=", "%%%"] {
            let _ = extrair(ruim);
        }
        assert!(extrair("").is_empty());
    }

    /// Espaco vira `+`, e NAO `_`.
    ///
    /// O `codificar` do coletor troca por sublinhado porque serve a titulo de
    /// Wikipedia. Numa busca isso muda a consulta -- "melhor configuracao" viraria
    /// "melhor_configuracao", que e outra string. Funcionou nos testes por tolerancia
    /// do DuckDuckGo, e quase-certo assim e o que nao se descobre.
    #[test]
    fn a_url_da_busca_escapa_a_consulta() {
        let u = url("ryzen 5 5500 & ram");
        assert!(u.starts_with("https://html.duckduckgo.com/html/?q="));
        assert!(!u.contains(' '), "espaco cru na url: {u}");
        assert!(u.ends_with("&kl=br-pt"));
        assert!(u.contains("ryzen+5+5500"), "espaco tinha de virar +: {u}");
        assert!(!u.contains('_'), "sublinhado nao e espaco numa busca: {u}");
        assert!(u.contains("%26"), "o & da consulta tinha de ser escapado: {u}");

        // E acento nao pode virar lixo.
        let a = url("configuracao de memoria");
        assert!(a.contains("configuracao+de+memoria"), "{a}");
    }

    /// Pagina de bloqueio nao pode ser lida como "nao achei nada".
    #[test]
    fn distingue_bloqueio_de_busca_sem_resultado() {
        // A pagina real tem a classe, entao NAO e bloqueio -- mesmo que um dia venha
        // com zero resultados dentro.
        assert!(!parece_bloqueio(REAL));
        // A de bloqueio nao tem.
        assert!(parece_bloqueio("<html><body>too many requests</body></html>"));
        assert!(parece_bloqueio(""));
    }

    #[test]
    fn o_resumo_corta_em_vez_de_despejar() {
        let rs = extrair(REAL);
        let texto = resumir(&rs, 3);
        assert!(texto.lines().count() <= 9, "resumo comprido demais");
        assert!(!texto.is_empty());
    }
}
