//! Geração de exemplos por LLM local — **ferramenta offline**.
//!
//! A Teka não fala com o LM Studio. Este módulo pede frases, filtra, e escreve num
//! arquivo. O runtime dela continua sem dependência e sem rede.
//!
//! ## Por que quarentena e não anexar direto
//!
//! Medido com `ministral-3-3b`: cerca de **40% do que sai é lixo ou desvio de
//! intenção**. Não erro de forma — erro de sentido:
//!
//! ```text
//! ✓ quanto do total esta sendo consumido no momento
//! ✗ quando a minha memoria vai comecar a falhar        (previsão, não status)
//! ✗ sou feliz que ainda tenho margem para mais de nada  (sem sentido)
//! ```
//!
//! E o filtro de span — o que já existia — **não pega nada disso**: `hora`,
//! `memoria` e `disco` não têm argumento, então não há o que validar. Anexar
//! direto envenenaria justamente as ferramentas mais indefesas.
//!
//! Por isso o gerador escreve em `dados/propostas.txt`, e a aprovação é um passo
//! separado. Os dados são o ativo mais valioso do projeto; 40% de ruído neles
//! custaria mais que todo o tempo economizado.
//!
//! ## Os três filtros
//!
//! 1. **Forma** — linha parseável, tamanho plausível, sem duplicata contra o corpus
//!    existente **nem contra o benchmark** (que deixaria de medir generalização).
//! 2. **Span** — para ferramentas com argumento, ele tem de aparecer literalmente no
//!    pedido. O ponteiro copia, não inventa.
//! 3. **Semântico** — embedding da frase comparado ao **centroide de cada
//!    ferramenta**. Aceita só se a ferramenta alvo for a mais próxima. Medido: frase
//!    boa da mesma ferramenta 0,43; de outra ferramenta 0,15; lixo 0,08.
//!
//! O terceiro é o que ataca o desvio de intenção, e é o motivo de valer a pena ter
//! um modelo de embedding no laço.

pub mod http;

use crate::learn::dados::{ler_casos_teste, ler_exemplos, Exemplo};
use crate::rng::Rng;
use crate::tools::Registro;

#[derive(Clone, Debug)]
pub struct CfgGerador {
    pub host: String,
    pub porta: u16,
    pub modelo: String,
    pub modelo_embedding: String,
    /// Quantas frases pedir por chamada.
    pub por_chamada: usize,
    /// Quantas chamadas por ferramenta.
    pub rodadas: usize,
    pub temperatura: f64,
    pub max_tokens: usize,
    pub timeout_s: u64,
    /// Quanto o alvo precisa ganhar do segundo colocado, em similaridade.
    pub margem_semantica: f32,
}

impl Default for CfgGerador {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            porta: 1234,
            // Medido em 108 pedidos por modelo, mesmo codigo, mesmos parametros:
            // ministral-3-3b 39% (so 5 das 9 ferramentas vivas), gemma-3-4b-it 55%,
            // qwen2.5-7b 72% com as 9. Ver README.
            modelo: "qwen2.5-7b-instruct".into(),
            modelo_embedding: "text-embedding-paraphrase-multilingual-minilm-l12-v2.gguf".into(),
            por_chamada: 15,
            rodadas: 2,
            // Alta de propósito: o objetivo é VARIEDADE. Frase repetida é
            // descartada de graça pelo filtro de forma; frase entediante não.
            temperatura: 1.1,
            max_tokens: 500,
            timeout_s: 300,
            margem_semantica: 0.02,
        }
    }
}

pub struct Proposta {
    pub ferramenta: String,
    pub pedido: String,
    pub argumentos: Vec<String>,
}

#[derive(Default, Debug)]
pub struct Contagem {
    pub pedidas: usize,
    pub recebidas: usize,
    pub fora_de_idioma: usize,
    pub fora_de_forma: usize,
    pub duplicadas: usize,
    pub sem_argumento: usize,
    pub fora_de_sentido: usize,
    pub aceitas: usize,
}

/// Pede frases ao LLM para uma ferramenta, usando exemplos como semente.
///
/// `params` sao os nomes dos parametros obrigatorios. Quando ha algum, o modelo e
/// obrigado a **declarar** o argumento depois de `|`, em vez de deixar que a gente
/// adivinhe onde ele esta na frase.
///
/// Isso trocou heuristica por contrato. A versao anterior tentava achar o argumento
/// sozinha e devolvia `None` sempre que nao havia marca formal (barra, ponto, sinal
/// de operacao). Resultado medido: `procurar_arquivo`, `escrever_arquivo` e
/// `executar_comando` ficaram com **zero** exemplos aceitos nas tres rodadas — nao
/// por culpa do modelo, mas porque nao existe como recortar "procura o relatorio"
/// sem saber que "relatorio" era o alvo. Quem sabe e quem escreveu a frase.
pub fn pedir_frases(
    cfg: &CfgGerador,
    ferramenta: &str,
    descricao: &str,
    sementes: &[String],
    params: &[String],
) -> Result<Vec<String>, String> {
    let regra_arg = if params.is_empty() {
        "Esta ferramenta nao recebe argumento. Escreva SO a frase, sem barra vertical. \
         Nao invente nomes de arquivo nem caminhos."
            .to_string()
    } else {
        format!(
            "Esta ferramenta recebe {} argumento(s): {}.\n\
             Formato de cada linha:  frase | {}\n\
             O argumento depois da barra tem de aparecer LITERALMENTE dentro da frase, \
             identico, sem reescrever nem traduzir. Se nao aparecer igual, a linha e \
             descartada.",
            params.len(),
            params.join(", "),
            params.join(" | ")
        )
    };
    let sistema = "Voce gera variacoes de pedidos em portugues do Brasil para um assistente \
                   de computador. Responda SOMENTE com as frases, uma por linha, sem \
                   numeracao, sem aspas, sem comentario e sem explicacao. Fala informal, \
                   como alguem falaria de verdade.";
    let usuario = format!(
        "Ferramenta: {ferramenta} — {descricao}\n\n\
         Exemplos do que ela deve entender:\n{}\n\n\
         {regra_arg}\n\n\
         Escreva {} variacoes NOVAS, diferentes entre si e diferentes dos exemplos. \
         Todas devem pedir EXATAMENTE a mesma coisa que os exemplos.",
        sementes.join("\n"),
        cfg.por_chamada
    );

    let corpo = format!(
        r#"{{"model":"{}","temperature":{},"max_tokens":{},"messages":[{{"role":"system","content":"{}"}},{{"role":"user","content":"{}"}}]}}"#,
        http::escapar(&cfg.modelo),
        cfg.temperatura,
        cfg.max_tokens,
        http::escapar(sistema),
        http::escapar(&usuario)
    );
    let resp = http::post_json(&cfg.host, cfg.porta, "/v1/chat/completions", &corpo, cfg.timeout_s)?;
    let conteudo = http::extrair_texto(&resp, "content").ok_or_else(|| {
        // O caso mais comum aqui e um modelo de RACIOCINIO: ele gasta tudo pensando
        // e devolve `content` vazio. Ver a nota no README sobre escolha de modelo.
        format!(
            "resposta sem campo `content` utilizavel (modelo de raciocinio?): {}",
            &resp[..resp.len().min(240)]
        )
    })?;
    if conteudo.trim().is_empty() {
        return Err("o modelo devolveu conteudo vazio — se for modelo de raciocinio, \
                    troque por um instruct puro"
            .into());
    }
    Ok(conteudo
        .lines()
        .map(limpar_linha)
        .filter(|l| !l.is_empty())
        .collect())
}

/// Troca pontuação tipográfica pela equivalente ASCII.
///
/// LLM adora aspa curva, travessão e reticências de um caractere só. Nada disso é
/// outro idioma, mas tudo isso passa de U+024F e seria barrado por
/// `so_alfabeto_latino`. Normalizar antes resolve os dois problemas de uma vez: o
/// filtro de escrita fica simples e estrito, e o corpus fica com pontuação uniforme
/// — que importa mais aqui do que no normal, porque a Teka lê **bytes**, e `'` de
/// três bytes contra `'` de um byte são dois símbolos diferentes para ela.
fn normalizar_pontuacao(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '\u{201B}' => saida.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}' => saida.push('"'),
            '\u{2010}'..='\u{2015}' | '\u{2212}' => saida.push('-'),
            '\u{2026}' => saida.push_str("..."),
            '\u{00A0}' | '\u{2007}' | '\u{202F}' => saida.push(' '),
            c => saida.push(c),
        }
    }
    saida
}

/// Tira numeração, marcadores, aspas e pontuação final.
fn limpar_linha(l: &str) -> String {
    let mut s = normalizar_pontuacao(l.trim());
    // "1. ", "1) ", "- ", "* "
    if let Some(p) = s.find(['.', ')']) {
        if p <= 2 && s[..p].chars().all(|c| c.is_ascii_digit()) {
            s = s[p + 1..].trim().to_string();
        }
    }
    s = s.trim_start_matches(['-', '*', '•']).trim().to_string();
    s = s.trim_matches('"').trim_matches('\'').trim().to_string();
    s = s.trim_end_matches(['.', '!', '?', ';', ':']).trim().to_string();
    s.to_lowercase()
}

/// A frase usa só escrita latina?
///
/// Existe por causa de um furo real. O modelo de embedding é o
/// `paraphrase-**multilingual**`, e o Qwen2.5 — que é chinês — despejou oito pedidos
/// em mandarim no meio de `memoria`. **Todos passaram no filtro semântico**, porque
/// 「当前的内存使用情况」 cai praticamente em cima do centroide português de
/// `memoria`. O filtro fez exatamente o que foi treinado para fazer: ele é cego a
/// idioma de propósito.
///
/// A Teka lê **bytes**, então uma frase em mandarim não é só inútil — ela ensina o
/// tokenizador-que-não-existe a gastar capacidade em faixas de UTF-8 que nunca vão
/// aparecer no uso real.
///
/// O corte é em U+024F (fim do Latin Extended-A): passa todo acento português
/// (`ção`, `memória`, `você`) e barra CJK, cirílico, árabe, grego.
fn so_alfabeto_latino(s: &str) -> bool {
    s.chars().all(|c| (c as u32) <= 0x024F)
}

/// Separa `pedido | arg1 | arg2` nos seus campos.
///
/// Devolve `None` se a contagem nao bate — melhor perder a linha do que adivinhar
/// qual pedaco era o argumento.
fn separar_campos(linha: &str, n_args: usize) -> Option<(String, Vec<String>)> {
    let campos: Vec<&str> = linha.split('|').map(|c| c.trim()).collect();
    if campos.len() != n_args + 1 {
        return None;
    }
    let pedido = campos[0].trim().to_string();
    if pedido.is_empty() {
        return None;
    }
    let args: Vec<String> = campos[1..].iter().map(|a| a.to_string()).collect();
    if args.iter().any(|a| a.is_empty()) {
        return None;
    }
    Some((pedido, args))
}

/// Similaridade de cosseno.
fn cos(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        ab += a[i] * b[i];
        aa += a[i] * a[i];
        bb += b[i] * b[i];
    }
    if aa <= 0.0 || bb <= 0.0 {
        return 0.0;
    }
    ab / (aa.sqrt() * bb.sqrt())
}

/// Pede embeddings de um lote de frases.
pub fn embeddings(cfg: &CfgGerador, frases: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if frases.is_empty() {
        return Ok(Vec::new());
    }
    let lista: Vec<String> = frases.iter().map(|f| format!("\"{}\"", http::escapar(f))).collect();
    let corpo = format!(
        r#"{{"model":"{}","input":[{}]}}"#,
        http::escapar(&cfg.modelo_embedding),
        lista.join(",")
    );
    let resp = http::post_json(&cfg.host, cfg.porta, "/v1/embeddings", &corpo, cfg.timeout_s)?;
    let v = http::extrair_embeddings(&resp);
    if v.len() != frases.len() {
        return Err(format!(
            "esperava {} embeddings, vieram {} — {}",
            frases.len(),
            v.len(),
            &resp[..resp.len().min(200)]
        ));
    }
    Ok(v)
}

fn centroide(vs: &[Vec<f32>]) -> Vec<f32> {
    if vs.is_empty() {
        return Vec::new();
    }
    let d = vs[0].len();
    let mut c = vec![0.0f32; d];
    for v in vs {
        for i in 0..d.min(v.len()) {
            c[i] += v[i];
        }
    }
    for x in c.iter_mut() {
        *x /= vs.len() as f32;
    }
    c
}

/// O laço completo: pede, filtra, devolve propostas para revisão.
pub fn gerar_propostas(
    cfg: &CfgGerador,
    reg: &Registro,
    corpus_atual: &str,
    benchmark: &str,
    quarentena: &str,
    rng: &mut Rng,
) -> Result<(Vec<Proposta>, Contagem), String> {
    let existentes = ler_exemplos(corpus_atual, reg);
    let casos = ler_casos_teste(benchmark);

    // Nada que já exista, e NADA do benchmark — ele deixaria de medir generalização.
    let mut vistos: std::collections::HashSet<String> =
        existentes.iter().map(|e| e.pedido.to_lowercase()).collect();
    for c in &casos {
        vistos.insert(c.pedido.to_lowercase());
    }
    // Nem o que já está esperando revisão. Sem isto, rodar duas vezes geraria as
    // mesmas frases de novo e dobraria o trabalho de revisão à toa.
    for (pedido, _) in ler_quarentena(quarentena) {
        vistos.insert(pedido.to_lowercase());
    }

    // Centroide de cada ferramenta, a partir do que já se sabe que está certo.
    println!("  calculando centroides de {} ferramentas...", reg.n());
    let mut centroides: Vec<Vec<f32>> = Vec::with_capacity(reg.n());
    for (fi, f) in reg.ferramentas.iter().enumerate() {
        let amostra: Vec<String> = existentes
            .iter()
            .filter(|e| e.ferramenta == fi)
            .map(|e| e.pedido.clone())
            .take(20)
            .collect();
        if amostra.is_empty() {
            centroides.push(Vec::new());
            println!("    {} — sem exemplos, filtro semantico desligado", f.nome);
            continue;
        }
        centroides.push(centroide(&embeddings(cfg, &amostra)?));
    }

    let mut propostas = Vec::new();
    let mut cont = Contagem::default();

    for (fi, f) in reg.ferramentas.iter().enumerate() {
        let obrigatorios: Vec<String> = f
            .params
            .iter()
            .filter(|p| p.obrigatorio)
            .map(|p| p.nome.clone())
            .collect();

        let sementes: Vec<String> = {
            // A semente mostra o formato de saida junto com o conteudo: para
            // ferramenta com argumento ela ja vem como `frase | argumento`, entao o
            // modelo imita a forma em vez de precisar deduzi-la da instrucao.
            let mut v: Vec<String> = existentes
                .iter()
                .filter(|e| e.ferramenta == fi)
                .map(|e| {
                    let mut linha = e.pedido.clone();
                    for (_, (ini, fim)) in &e.args {
                        if let Some(t) = e.pedido.get(*ini..*fim) {
                            linha.push_str(" | ");
                            linha.push_str(t);
                        }
                    }
                    linha
                })
                .collect();
            // Sementes sorteadas a cada rodada: sempre as mesmas produziriam sempre
            // as mesmas variacoes.
            for i in (1..v.len()).rev() {
                let j = (rng.uniform01() * (i + 1) as f64) as usize % (i + 1);
                v.swap(i, j);
            }
            v.truncate(6);
            v
        };
        for rodada in 1..=cfg.rodadas {
            cont.pedidas += cfg.por_chamada;
            print!("  {} (rodada {rodada}/{}) ... ", f.nome, cfg.rodadas);
            use std::io::Write as _;
            let _ = std::io::stdout().flush();

            let linhas = match pedir_frases(cfg, &f.nome, &f.descricao, &sementes, &obrigatorios) {
                Ok(l) => l,
                Err(e) => {
                    println!("falhou: {e}");
                    continue;
                }
            };
            cont.recebidas += linhas.len();

            // --- filtros de forma e duplicata ---
            let mut candidatas: Vec<(String, Vec<String>)> = Vec::new();
            for linha in linhas {
                // Antes de tudo: o filtro semantico nao separa idioma, entao este
                // separa. Ver `so_alfabeto_latino`.
                if !so_alfabeto_latino(&linha) {
                    cont.fora_de_idioma += 1;
                    continue;
                }
                // O modelo declara os argumentos; se nao declarou, ainda tentamos a
                // heuristica antiga como rede de seguranca.
                let (pedido, args) = match separar_campos(&linha, obrigatorios.len()) {
                    Some(x) => x,
                    None => {
                        if obrigatorios.is_empty() {
                            cont.fora_de_forma += 1;
                            continue;
                        }
                        let base = linha.split('|').next().unwrap_or("").trim().to_string();
                        let mut achados = Vec::new();
                        for nome in &obrigatorios {
                            match achar_argumento(&base, nome) {
                                Some(a) => achados.push(a),
                                None => break,
                            }
                        }
                        if achados.len() != obrigatorios.len() {
                            cont.sem_argumento += 1;
                            continue;
                        }
                        (base, achados)
                    }
                };

                if pedido.len() < 6 || pedido.len() > 90 || pedido.contains('\t') {
                    cont.fora_de_forma += 1;
                    continue;
                }
                if vistos.contains(&pedido) {
                    cont.duplicadas += 1;
                    continue;
                }
                // --- filtro de span: o ponteiro COPIA, entao o argumento tem de
                // estar identico dentro do pedido. Sem isso nao ha o que apontar.
                if args.iter().any(|a| !pedido.contains(a.as_str())) {
                    cont.sem_argumento += 1;
                    continue;
                }
                vistos.insert(pedido.clone());
                candidatas.push((pedido, args));
            }
            if candidatas.is_empty() {
                println!("0 aceitas");
                continue;
            }

            // --- filtro semantico: a ferramenta alvo tem de ser a mais proxima ---
            let textos: Vec<String> = candidatas.iter().map(|(l, _)| l.clone()).collect();
            let vs = match embeddings(cfg, &textos) {
                Ok(v) => v,
                Err(e) => {
                    println!("embedding falhou: {e}");
                    continue;
                }
            };
            let mut aceitas = 0;
            for ((linha, args), v) in candidatas.into_iter().zip(vs) {
                let alvo = centroides.get(fi).map(|c| cos(c, &v)).unwrap_or(1.0);
                let melhor_outro = centroides
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| *i != fi && !c.is_empty())
                    .map(|(_, c)| cos(c, &v))
                    .fold(f32::NEG_INFINITY, f32::max);
                if alvo < melhor_outro + cfg.margem_semantica {
                    cont.fora_de_sentido += 1;
                    continue;
                }
                propostas.push(Proposta {
                    ferramenta: f.nome.clone(),
                    pedido: linha,
                    argumentos: args,
                });
                aceitas += 1;
                cont.aceitas += 1;
            }
            println!("{aceitas} aceitas");
        }
    }
    Ok((propostas, cont))
}

/// Tenta identificar o argumento dentro da frase, pelo tipo do parâmetro.
///
/// Heurística deliberadamente conservadora: na dúvida devolve `None` e o exemplo é
/// descartado. Um argumento mal recortado ensina o ponteiro a apontar errado, o que
/// é pior do que não ter o exemplo.
fn achar_argumento(frase: &str, nome_param: &str) -> Option<String> {
    let palavras: Vec<&str> = frase.split_whitespace().collect();
    match nome_param {
        "expressao" => palavras
            .iter()
            .find(|p| {
                p.chars().any(|c| c.is_ascii_digit())
                    && p.chars().any(|c| "+-*/^%".contains(c))
            })
            .map(|s| s.to_string()),
        "caminho" | "raiz" => palavras
            .iter()
            .find(|p| p.contains('\\') || p.contains('/') || p.contains('.'))
            .map(|s| s.trim_matches(',').to_string()),
        "nome" | "comando" | "texto" => {
            // Sem marca formal para achar: é o caso em que a heurística erraria com
            // frequência, então prefere não propor.
            None
        }
        _ => None,
    }
}

/// Lê `dados/propostas.txt` de volta como `(pedido, linha inteira)`.
///
/// Serve a dois propósitos: não regerar o que já espera revisão, e preservar as
/// linhas antigas quando o gerador roda outra vez.
pub fn ler_quarentena(texto: &str) -> Vec<(String, String)> {
    texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut campos = l.split('|').map(str::trim);
            let _ferramenta = campos.next()?;
            let pedido = campos.next()?.to_string();
            if pedido.is_empty() {
                return None;
            }
            Some((pedido, l.to_string()))
        })
        .collect()
}

/// Serializa as propostas no formato de `dados/exemplos_teka.txt`.
pub fn formatar(propostas: &[Proposta], anteriores: &[String]) -> String {
    let mut s = String::from(
        "# Propostas do gerador — REVISE ANTES DE APROVAR.\n\
         #\n\
         # Cerca de 40% do que um modelo pequeno produz e lixo ou desvio de intencao,\n\
         # e os filtros automaticos nao pegam erro de SENTIDO. Apague as linhas ruins\n\
         # e depois anexe o resto a dados/exemplos_teka.txt.\n\
         #\n\
         # formato:  ferramenta | pedido | argumento1 | argumento2\n\n",
    );
    for linha in anteriores {
        s.push_str(linha);
        s.push('\n');
    }
    for p in propostas {
        s.push_str(&p.ferramenta);
        s.push_str(" | ");
        s.push_str(&p.pedido);
        for a in &p.argumentos {
            s.push_str(" | ");
            s.push_str(a);
        }
        s.push('\n');
    }
    s
}

/// Só para deixar claro que `Exemplo` participa do contrato deste módulo.
pub fn _tipo(_: &Exemplo) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limpa_numeracao_marcador_aspas_e_pontuacao() {
        assert_eq!(limpar_linha("1. Como anda a RAM?"), "como anda a ram");
        assert_eq!(limpar_linha("- lista a pasta src"), "lista a pasta src");
        assert_eq!(limpar_linha("  \"que horas sao\"  "), "que horas sao");
        assert_eq!(limpar_linha("2) Ver o disco."), "ver o disco");
        assert_eq!(limpar_linha("* memória em uso!"), "memória em uso");
        assert_eq!(limpar_linha(""), "");
    }

    #[test]
    fn acha_argumento_quando_ha_marca_e_desiste_quando_nao_ha() {
        assert_eq!(
            achar_argumento("quanto e 45+55 mesmo", "expressao"),
            Some("45+55".into())
        );
        assert_eq!(
            achar_argumento("lista a pasta src\\model", "caminho"),
            Some("src\\model".into())
        );
        assert_eq!(
            achar_argumento("abre o notas.md agora", "caminho"),
            Some("notas.md".into())
        );
        // Sem marca formal: prefere nao propor a propor errado.
        assert_eq!(achar_argumento("procura o relatorio", "nome"), None);
        assert_eq!(achar_argumento("lista a pasta", "caminho"), None);
    }

    #[test]
    fn cosseno_separa_o_que_deve_separar() {
        assert!((cos(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cos(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cos(&[], &[]), 0.0);
        assert_eq!(cos(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn centroide_e_a_media() {
        let c = centroide(&[vec![1.0, 0.0], vec![0.0, 2.0]]);
        assert_eq!(c, vec![0.5, 1.0]);
        assert!(centroide(&[]).is_empty());
    }

    #[test]
    fn normaliza_pontuacao_tipografica_para_ascii() {
        assert_eq!(normalizar_pontuacao("\u{2018}oi\u{2019}"), "'oi'");
        assert_eq!(normalizar_pontuacao("\u{201C}oi\u{201D}"), "\"oi\"");
        assert_eq!(normalizar_pontuacao("a\u{2014}b"), "a-b");
        assert_eq!(normalizar_pontuacao("espera\u{2026}"), "espera...");
        assert_eq!(normalizar_pontuacao("a\u{00A0}b"), "a b");
        // Acento portugues nao e pontuacao: fica intacto.
        assert_eq!(normalizar_pontuacao("memória"), "memória");
        // E o resultado tem de sobreviver ao filtro de escrita.
        assert!(so_alfabeto_latino(&normalizar_pontuacao(
            "tá difícil\u{2026} me diz a \u{201C}hora\u{201D}"
        )));
    }

    #[test]
    fn barra_escrita_nao_latina_mas_aceita_acento_portugues() {
        assert!(so_alfabeto_latino("como anda a memória do meu pc"));
        assert!(so_alfabeto_latino("ação, coração, você, êxito, ünïcode"));
        assert!(so_alfabeto_latino("abre o c:\\temp\\notas.md"));
        // O que o Qwen2.5 despejou de verdade em `memoria`.
        assert!(!so_alfabeto_latino("系统内存够不够用啊？"));
        // Mistura tambem nao passa.
        assert!(!so_alfabeto_latino("verificar se tem algum processo占用内存了"));
        assert!(!so_alfabeto_latino("проверить память"));
    }

    #[test]
    fn separa_pedido_e_argumentos_ou_desiste() {
        assert_eq!(
            separar_campos("abre o notas.md | notas.md", 1),
            Some(("abre o notas.md".into(), vec!["notas.md".into()]))
        );
        assert_eq!(
            separar_campos("salva oi em a.txt | a.txt | oi", 2),
            Some(("salva oi em a.txt".into(), vec!["a.txt".into(), "oi".into()]))
        );
        // Contagem errada: desiste em vez de adivinhar qual pedaco era o argumento.
        assert_eq!(separar_campos("abre o notas.md", 1), None);
        assert_eq!(separar_campos("a | b | c", 1), None);
        // Campo vazio nao vale argumento.
        assert_eq!(separar_campos("abre isso |  ", 1), None);
        // Sem argumento: a linha inteira e o pedido.
        assert_eq!(separar_campos("que horas sao", 0), Some(("que horas sao".into(), vec![])));
    }

    #[test]
    fn formatar_produz_o_formato_do_corpus() {
        let p = vec![Proposta {
            ferramenta: "listar_pasta".into(),
            pedido: "ve a pasta src".into(),
            argumentos: vec!["src".into()],
        }];
        let s = formatar(&p, &[]);
        assert!(s.contains("listar_pasta | ve a pasta src | src"));
        assert!(s.starts_with('#'), "tem que comecar com o aviso de revisao");

        // Rodar de novo nao pode apagar o que ja estava esperando revisao.
        let com_antigas = formatar(&p, &["hora | que horas sao".to_string()]);
        assert!(com_antigas.contains("hora | que horas sao"));
        assert!(com_antigas.contains("listar_pasta | ve a pasta src | src"));
    }

    #[test]
    fn le_a_quarentena_ignorando_comentario_e_vazio() {
        let t = "# aviso\n\nhora | que horas sao\nler_arquivo | abre a.md | a.md\n";
        let v = ler_quarentena(t);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "que horas sao");
        assert_eq!(v[1].0, "abre a.md");
        assert_eq!(v[1].1, "ler_arquivo | abre a.md | a.md");
        assert!(ler_quarentena("# so comentario\n").is_empty());
    }
}
