//! Coletor de corpus — **ferramenta offline**, como o gerador.
//!
//! A Teka não baixa nada. Este módulo monta um arquivo de texto; o runtime dela
//! continua sem rede e sem dependência.
//!
//! ## O problema que ele resolve
//!
//! O `dados/corpus_pt.txt` que ensina português para a Teka é literatura portuguesa
//! de 1898 (Projeto Gutenberg, Eça de Queirós). Medido:
//!
//! ```text
//! "elle" ..... 1.984x     "sciencia" ... 258x     "theatro" ... 88x
//! computador ..... 0      arquivo ......... 0     internet ...... 0
//! ```
//!
//! **A palavra `arquivo` aparece zero vezes** no corpus que deveria ensinar
//! português a uma agente de arquivos. Isso explica um resultado que estava sem
//! explicação: a medição de que o pré-treino dava ≈ 0 de ganho. O domínio do
//! pré-treino não toca o domínio da tarefa em ponto nenhum.
//!
//! Portanto o problema **não é corpus pequeno, é corpus errado**. Mais 500 MB de
//! Eça não mudaria nada.
//!
//! ## Por que isto delega o download
//!
//! A Wikipédia só serve por HTTPS, e não há TLS neste projeto — nem vai haver, por
//! causa do zero-dependência. O coletor chama o cliente HTTP do sistema
//! (`curl`/PowerShell) e cuida do que importa aqui: **limpar, filtrar e registrar de
//! onde veio**. Baixar é a parte fácil; decidir o que entra no corpus é a difícil.
//!
//! ## Procedência não é burocracia
//!
//! Texto baixado vira dado de treino, e dado de treino vira comportamento. Cada
//! pedaço guarda a URL de origem no manifesto, pelo mesmo motivo que
//! [`crate::memory::semantica::Fonte`] existe: quando algo aparecer errado no
//! comportamento dela, a única forma de achar a causa é saber de onde o texto veio.
//! E a lista de domínios é **fixa no código**, não configurável por argumento —
//! raspagem aberta é o caminho mais curto para envenenar um modelo sem perceber.

pub mod fonte;

use std::collections::HashSet;

/// Domínios de onde é permitido coletar.
///
/// Fixa de propósito. Todos servem português moderno e são auditáveis: dá para
/// abrir e ler o que entrou. Fórum aberto e conteúdo raspado sem curadoria ficam de
/// fora — não por serem inúteis, mas por não haver como revisar o que se pegou.
pub const DOMINIOS: &[&str] = &[
    // A busca da Teka (`Primitiva::BuscarWeb`) usa a MESMA lista. Duas allowlists
    // seriam duas chances de esquecer uma — e a diferenca entre "busca" e "manda o
    // que voce digitou para qualquer lugar" e exatamente esta lista.
    "api.duckduckgo.com",
    // A PAGINA de resultados, que a Instant Answer nao substitui: aquela so
    // responde termo unico de enciclopedia. Ver `tools::busca_ddg`.
    "html.duckduckgo.com",
    "pt.wikipedia.org",
    "pt.wikibooks.org",
    "docs.python.org",
    "developer.mozilla.org",
];

/// Um pedaço de texto coletado, com a origem grudada.
#[derive(Clone, Debug)]
pub struct Pedaco {
    pub texto: String,
    pub origem: String,
}

/// Por que um pedaço foi recusado. Serve ao relatório: um coletor que só diz
/// "aceitos: 412" esconde se o filtro está fazendo sentido.
#[derive(Default, Debug)]
pub struct Contagem {
    pub vistos: usize,
    pub curto: usize,
    pub nao_portugues: usize,
    pub entulho: usize,
    pub duplicado: usize,
    pub aceitos: usize,
    pub bytes: usize,
}

/// Palavras funcionais do português. A presença delas é o teste de idioma.
///
/// Funciona porque palavra funcional é o que **não** muda com o assunto: um texto
/// sobre kernel e um sobre culinária têm ambos "de", "que", "para". Contar palavra
/// de conteúdo mediria o tema, não o idioma.
const FUNCIONAIS: &[&str] = &[
    "de", "que", "e", "o", "a", "do", "da", "em", "um", "uma", "para", "com", "nao",
    "os", "as", "no", "na", "por", "se", "mais", "como", "mas", "ao", "dos", "das",
    "ou", "quando", "muito", "sem", "pelo", "isso", "ele", "ela", "voce", "foi",
];

/// Fração mínima de palavras funcionais para o texto contar como português.
const MIN_FUNCIONAIS: f32 = 0.12;

/// Linhas que são estrutura de página, não texto.
const ENTULHO: &[&str] = &[
    "editar código-fonte",
    "editar codigo-fonte",
    "ver histórico",
    "ligações externas",
    "ver também",
    "referências",
    "predefinição:",
    "categoria:",
    "ficheiro:",
    "wikipédia:",
    "obtida de \"",
    "esta página foi editada",
    "política de privacidade",
    "todos os direitos reservados",
    "aceitar cookies",
    "javascript",
    "\u{00a9}",
];

/// Tira marcação e normaliza espaço. Não é parser de HTML, e não precisa ser: a API
/// da Wikipédia devolve `explaintext`, então o que sobra é resíduo.
pub fn limpar(bruto: &str) -> String {
    let mut saida = String::with_capacity(bruto.len());
    let mut dentro_de_tag = false;
    for c in bruto.chars() {
        match c {
            '<' => dentro_de_tag = true,
            '>' => dentro_de_tag = false,
            _ if dentro_de_tag => {}
            _ => saida.push(c),
        }
    }
    let saida = saida
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    // Espaço colapsado, linha a linha, preservando parágrafo.
    saida
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\n\n\n", "\n\n")
}

/// Sem acento, minúsculas — só para os testes de idioma e de duplicata.
fn dobrar(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    for c in s.chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ô' | 'õ' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            outro => outro,
        };
        for m in c.to_lowercase() {
            saida.push(m);
        }
    }
    saida
}

/// Isto parece português?
pub fn parece_portugues(texto: &str) -> bool {
    let dobrado = dobrar(texto);
    let palavras: Vec<&str> = dobrado
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    if palavras.len() < 20 {
        return false;
    }
    let funcionais = palavras
        .iter()
        .filter(|p| FUNCIONAIS.contains(&&***p))
        .count();
    funcionais as f32 / palavras.len() as f32 >= MIN_FUNCIONAIS
}

/// Linha que é estrutura de página, não conteúdo.
pub fn e_entulho(linha: &str) -> bool {
    let d = dobrar(linha);
    let d = d.trim();
    if d.is_empty() {
        return true;
    }
    // Linha curtíssima quase nunca é prosa: é menu, legenda ou cabeçalho.
    if d.split_whitespace().count() < 4 {
        return true;
    }
    ENTULHO.iter().any(|e| d.contains(&dobrar(e)))
}

/// Hash de conteúdo para deduplicar. FNV-1a, mesmo do diário.
fn impressao(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in dobrar(s).split_whitespace().collect::<Vec<_>>().join(" ").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Filtra um lote e devolve o que entra no corpus.
///
/// A ordem dos testes é por custo: forma antes de idioma, idioma antes de duplicata.
pub fn filtrar(pedacos: Vec<Pedaco>, min_bytes: usize) -> (Vec<Pedaco>, Contagem) {
    let mut vistos: HashSet<u64> = HashSet::new();
    let mut saida = Vec::new();
    let mut c = Contagem::default();

    for p in pedacos {
        c.vistos += 1;

        // Entulho fora, linha a linha — uma página boa quase sempre traz menu junto.
        let corpo: String = limpar(&p.texto)
            .lines()
            .filter(|l| !e_entulho(l))
            .collect::<Vec<_>>()
            .join("\n");

        if corpo.len() < min_bytes {
            c.curto += 1;
            continue;
        }
        if corpo.lines().count() == 0 {
            c.entulho += 1;
            continue;
        }
        if !parece_portugues(&corpo) {
            c.nao_portugues += 1;
            continue;
        }
        if !vistos.insert(impressao(&corpo)) {
            c.duplicado += 1;
            continue;
        }
        c.aceitos += 1;
        c.bytes += corpo.len();
        saida.push(Pedaco {
            texto: corpo,
            origem: p.origem,
        });
    }
    (saida, c)
}

/// Serializa o corpus com o manifesto de procedência intercalado.
///
/// A origem vai como comentário `#` antes de cada bloco. O treino ignora linha `#`,
/// e quem for auditar consegue rastrear qualquer trecho até a URL sem precisar de um
/// segundo arquivo que pode dessincronizar.
pub fn formatar(pedacos: &[Pedaco]) -> String {
    let mut s = String::new();
    for p in pedacos {
        s.push_str("# fonte: ");
        s.push_str(&p.origem);
        s.push('\n');
        s.push_str(&p.texto);
        s.push_str("\n\n");
    }
    s
}

impl Contagem {
    pub fn relatorio(&self) -> String {
        format!(
            "  vistos ............ {}\n  \
               curtos demais ..... {}\n  \
               nao-portugues ..... {}\n  \
               so entulho ........ {}\n  \
               duplicados ........ {}\n  \
               ACEITOS ........... {} ({:.2} MB)\n",
            self.vistos,
            self.curto,
            self.nao_portugues,
            self.entulho,
            self.duplicado,
            self.aceitos,
            self.bytes as f64 / 1_048_576.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: &str) -> Pedaco {
        Pedaco {
            texto: t.into(),
            origem: "pt.wikipedia.org/teste".into(),
        }
    }

    /// Prosa moderna longa o bastante para passar nos filtros.
    fn prosa(assunto: &str) -> String {
        format!(
            "Um arquivo de computador e um conjunto de dados que fica guardado no \
             disco do sistema, e que pode ser lido ou escrito por um programa. \
             Quando voce salva um documento, o sistema operacional grava esse \
             conteudo em uma pasta especifica. O {assunto} funciona de maneira \
             parecida com isso, mas com algumas diferencas que sao importantes \
             para quem precisa entender como a memoria do computador e usada."
        )
    }

    #[test]
    fn limpar_tira_marcacao_e_entidade() {
        assert_eq!(limpar("<p>ola <b>mundo</b></p>"), "ola mundo");
        assert_eq!(limpar("a &amp; b &nbsp; c"), "a & b c");
        assert_eq!(limpar("muito     espaco"), "muito espaco");
        // Acento nao pode ser tocado: e byte que importa para ela.
        assert_eq!(limpar("<i>memória</i>"), "memória");
    }

    #[test]
    fn reconhece_portugues_e_recusa_o_resto() {
        assert!(parece_portugues(&prosa("cache")));
        // Ingles tem estrutura parecida mas outras palavras funcionais.
        assert!(!parece_portugues(
            "A computer file is a collection of data stored on a disk that can be \
             read or written by a program running on the operating system of the \
             machine that the user happens to be working with at the time."
        ));
        // Texto curto demais nao da para julgar: recusa por seguranca.
        assert!(!parece_portugues("um arquivo de computador"));
    }

    #[test]
    fn entulho_de_pagina_nao_entra() {
        assert!(e_entulho("Editar código-fonte"));
        assert!(e_entulho("Ver também"));
        assert!(e_entulho("Categoria: Informática"));
        assert!(e_entulho("Esta página foi editada pela ultima vez em maio"));
        assert!(e_entulho(""));
        // Linha curta e menu, nao prosa.
        assert!(e_entulho("Ligações externas"));
        // Prosa de verdade passa.
        assert!(!e_entulho(
            "O sistema de arquivos organiza os dados em pastas e subpastas."
        ));
    }

    #[test]
    fn duplicata_cai_mesmo_com_espaco_e_caixa_diferentes() {
        let a = prosa("cache");
        let b = a.to_uppercase().replace(' ', "  ");
        let (aceitos, c) = filtrar(vec![p(&a), p(&b)], 100);
        assert_eq!(aceitos.len(), 1, "eram o mesmo texto");
        assert_eq!(c.duplicado, 1);
    }

    #[test]
    fn a_procedencia_sobrevive_ate_o_arquivo() {
        let (aceitos, _) = filtrar(vec![p(&prosa("cache"))], 100);
        let texto = formatar(&aceitos);
        assert!(
            texto.starts_with("# fonte: pt.wikipedia.org/teste"),
            "sem a origem nao ha como auditar depois: {}",
            &texto[..60.min(texto.len())]
        );
        // O corpo tem de vir logo abaixo, e o treino ignora a linha `#`.
        assert!(texto.contains("arquivo de computador"));
    }

    #[test]
    fn o_filtro_conta_cada_motivo_separado() {
        let entrada = vec![
            p(&prosa("cache")),
            p("curto"),
            p("The quick brown fox jumps over the lazy dog and then runs away from \
               the house because it is afraid of what might happen next in the story \
               that we are telling here today for the purposes of this test case."),
            p(&prosa("cache")), // duplicado
        ];
        let (aceitos, c) = filtrar(entrada, 100);
        assert_eq!(aceitos.len(), 1);
        assert_eq!(c.vistos, 4);
        assert_eq!(c.curto, 1);
        assert_eq!(c.nao_portugues, 1);
        assert_eq!(c.duplicado, 1);
    }

    #[test]
    fn a_lista_de_dominios_e_fechada() {
        assert!(DOMINIOS.contains(&"pt.wikipedia.org"));
        // Raspagem aberta e o caminho curto para envenenar um modelo sem perceber.
        assert!(!DOMINIOS.iter().any(|d| d.contains('*')));
        assert!(DOMINIOS.iter().all(|d| !d.starts_with("http")));
    }
}
