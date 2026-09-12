//! Ferramentas **como dados**.
//!
//! Uma ferramenta não é código: é uma entrada num registro — nome, descrição em
//! português, lista de parâmetros, e qual primitiva executa. Três consequências que
//! valem o desenho:
//!
//! 1. O registro **compila para um autômato** (ver [`crate::grammar`]) que torna
//!    impossível a Teka emitir uma chamada malformada.
//! 2. O registro **se auto-documenta**: [`Registro::manual`] gera o texto que a Teka
//!    lê como bytes durante o treino. Adicionar uma ferramenta já ensina ela sobre a
//!    ferramenta.
//! 3. Na fase 5, criar uma ferramenta nova vira **compor primitivas** — dado novo no
//!    registro, não código novo.

pub mod ponte_auto;
pub mod prim;
pub mod diario;
pub mod execucao;
pub mod busca_ddg;
pub mod gatilhos;
pub mod harness;
pub mod harness_proc;
pub mod harness_tcp;
pub mod oficina;
pub mod ponte;
pub mod regras;
pub mod seguranca;
pub mod teclado;
pub mod tela;
pub mod uia;

pub use prim::Primitiva;
pub use seguranca::{Modo, Politica};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoParam {
    /// Caminho de arquivo ou pasta — quase sempre copiado literalmente do pedido.
    Caminho,
    Texto,
    Numero,
}

impl TipoParam {
    pub fn rotulo(&self) -> &'static str {
        match self {
            TipoParam::Caminho => "caminho",
            TipoParam::Texto => "texto",
            TipoParam::Numero => "numero",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub nome: String,
    pub tipo: TipoParam,
    pub obrigatorio: bool,
}

impl Param {
    pub fn obrigatorio(nome: &str, tipo: TipoParam) -> Self {
        Self {
            nome: nome.into(),
            tipo,
            obrigatorio: true,
        }
    }
    pub fn opcional(nome: &str, tipo: TipoParam) -> Self {
        Self {
            nome: nome.into(),
            tipo,
            obrigatorio: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ferramenta {
    pub nome: String,
    pub descricao: String,
    pub params: Vec<Param>,
    pub prim: Primitiva,
}

#[derive(Clone, Debug, Default)]
pub struct Registro {
    pub ferramentas: Vec<Ferramenta>,
}

/// Uma chamada concreta: qual ferramenta, com quais argumentos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chamada {
    pub ferramenta: usize,
    pub args: Vec<(String, String)>,
}

impl Registro {
    pub fn n(&self) -> usize {
        self.ferramentas.len()
    }

    pub fn indice(&self, nome: &str) -> Option<usize> {
        self.ferramentas.iter().position(|f| f.nome == nome)
    }

    pub fn nomes(&self) -> Vec<&str> {
        self.ferramentas.iter().map(|f| f.nome.as_str()).collect()
    }

    pub fn executar(&self, c: &Chamada, pol: &Politica) -> Result<String, String> {
        let f = self
            .ferramentas
            .get(c.ferramenta)
            .ok_or_else(|| format!("ferramenta {} não existe", c.ferramenta))?;
        for p in &f.params {
            if p.obrigatorio && !c.args.iter().any(|(k, _)| *k == p.nome) {
                return Err(format!("{}: falta o parâmetro obrigatório {}", f.nome, p.nome));
            }
        }
        f.prim.executar(&c.args, pol)
    }

    /// O texto que a Teka lê como bytes no treino. O registro se ensina.
    pub fn manual(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        for f in &self.ferramentas {
            let _ = writeln!(s, "ferramenta: {}", f.nome);
            let _ = writeln!(s, "  o que faz: {}", f.descricao);
            if !f.params.is_empty() {
                let ps: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| {
                        format!(
                            "{} ({}{})",
                            p.nome,
                            p.tipo.rotulo(),
                            if p.obrigatorio { "" } else { ", opcional" }
                        )
                    })
                    .collect();
                let _ = writeln!(s, "  parametros: {}", ps.join(", "));
            }
            let _ = writeln!(s, "  uso: {}\n", exemplo_de_uso(f));
        }
        s
    }

    /// O registro inicial da Teka. Nove primitivas: quatro que só observam, três que
    /// calculam ou medem, e duas que tocam o mundo (e por isso passam pela política).
    pub fn padrao() -> Self {
        use Primitiva as P;
        use TipoParam::{Caminho, Numero, Texto};
        let _ = Numero;
        let f = |nome: &str, descricao: &str, params: Vec<Param>, prim: Primitiva| Ferramenta {
            nome: nome.into(),
            descricao: descricao.into(),
            params,
            prim,
        };
        Self {
            ferramentas: vec![
                // Primeira de propósito: é o que ela deve escolher quando nada
                // encaixa, e deixá-la no índice 0 torna óbvio nos dumps de política
                // quando a dúvida está ganhando.
                f(
                    "perguntar",
                    "diz que nao entendeu e pede para reformular",
                    vec![],
                    P::Perguntar,
                ),
                f("hora", "diz a data e a hora de agora", vec![], P::Hora),
                f(
                    "listar_pasta",
                    "mostra os arquivos e pastas de um diretorio",
                    vec![Param::obrigatorio("caminho", Caminho)],
                    P::ListarPasta,
                ),
                f(
                    "ler_arquivo",
                    "mostra o conteudo de um arquivo de texto",
                    vec![Param::obrigatorio("caminho", Caminho)],
                    P::LerArquivo,
                ),
                f(
                    "procurar_arquivo",
                    "procura arquivos cujo nome contenha um trecho",
                    vec![
                        Param::obrigatorio("nome", Texto),
                        Param::opcional("raiz", Caminho),
                    ],
                    P::ProcurarArquivo,
                ),
                f(
                    "calcular",
                    "resolve uma conta matematica",
                    vec![Param::obrigatorio("expressao", Texto)],
                    P::Calcular,
                ),
                f("memoria", "diz quanta memoria RAM esta em uso", vec![], P::Memoria),
                f(
                    "disco",
                    "diz quanto espaco livre tem no disco",
                    vec![Param::opcional("caminho", Caminho)],
                    P::Disco,
                ),
                f(
                    "escrever_arquivo",
                    "grava um texto num arquivo",
                    vec![
                        Param::obrigatorio("caminho", Caminho),
                        Param::obrigatorio("texto", Texto),
                    ],
                    P::EscreverArquivo,
                ),
                // ── as oito que vieram com o arsenal do Harness ──
                //
                // A inversao que importa: la o modelo remoto decide e o codigo e
                // encanamento; aqui quem decide e a Teka e a ferramenta e musculo.
                f(
                    "buscar_web",
                    "procura na internet e traz um resumo",
                    vec![Param::obrigatorio("consulta", Texto)],
                    P::BuscarWeb,
                ),
                f(
                    "abrir_programa",
                    "abre um programa no computador",
                    vec![Param::obrigatorio("programa", Texto)],
                    P::AbrirPrograma,
                ),
                // Duas obrigatorias, como `escrever_arquivo` — que e a ferramenta
                // mais fraca dela hoje justamente por isso: das quatro formas
                // naturais testadas, nenhuma acertou os dois argumentos.
                f(
                    "copiar_arquivo",
                    "copia um arquivo para outro lugar",
                    vec![
                        Param::obrigatorio("origem", Caminho),
                        Param::obrigatorio("destino", Caminho),
                    ],
                    P::CopiarArquivo,
                ),
                f(
                    "mover_arquivo",
                    "move ou renomeia um arquivo",
                    vec![
                        Param::obrigatorio("origem", Caminho),
                        Param::obrigatorio("destino", Caminho),
                    ],
                    P::MoverArquivo,
                ),
                f(
                    "criar_pasta",
                    "cria uma pasta",
                    vec![Param::obrigatorio("caminho", Caminho)],
                    P::CriarPasta,
                ),
                f(
                    "info_arquivo",
                    "diz o tamanho e a data de um arquivo",
                    vec![Param::obrigatorio("caminho", Caminho)],
                    P::InfoArquivo,
                ),
                f(
                    "apagar_arquivo",
                    "apaga um arquivo",
                    vec![Param::obrigatorio("caminho", Caminho)],
                    P::ApagarArquivo,
                ),
                f("processos", "lista os programas rodando agora", vec![], P::Processos),
                f("rede", "diz o IP e o estado da rede", vec![], P::Rede),
                f(
                    "executar_comando",
                    "roda um comando do sistema",
                    vec![Param::obrigatorio("comando", Texto)],
                    P::ExecutarComando,
                ),
                // A vigésima, e a primeira pedida de dentro de uma partida: trocar
                // música, mutar o microfone, mexer no volume sem alt-tab. O nome do
                // atalho vem de uma lista fechada (`teclado::ATALHOS`), então o
                // argumento não é texto livre — é um de dez.
                // --- as tres que vieram pela PONTE do Harness ---
                //
                // O NOME importa tanto quanto a ferramenta. `buscar_no_conteudo` e
                // lexicalmente distante de `procurar_arquivo`, e essa distancia e
                // parte do que a torna aprendivel: medido em 04/09, a Teka aprende
                // SUPERFICIE, nao conceito. Chamar de "grep" nao ajudaria ninguem
                // que fala portugues, e chamar de "procurar_conteudo" colidiria com
                // a que ja existe.
                f(
                    "buscar_no_conteudo",
                    "procura um trecho DENTRO dos arquivos, nao pelo nome deles",
                    vec![
                        Param::obrigatorio("padrao", Texto),
                        Param::opcional("raiz", Caminho),
                    ],
                    P::Grep,
                ),
                f(
                    "ler_imagem",
                    "abre uma imagem e descreve o que ela contem",
                    vec![Param::obrigatorio("caminho", Caminho)],
                    P::LerImagem,
                ),
                f(
                    "atalho",
                    "manda um atalho de teclado (musica, volume, mudo)",
                    vec![Param::obrigatorio("nome", Texto), Param::opcional("alvo", Texto)],
                    P::Atalho,
                ),
            ],
        }
    }
}

fn exemplo_de_uso(f: &Ferramenta) -> String {
    let mut s = format!("{{\"acao\":\"{}\"", f.nome);
    for p in f.params.iter().filter(|p| p.obrigatorio) {
        s.push_str(&format!(",\"{}\":\"...\"", p.nome));
    }
    s.push('}');
    s
}

impl Chamada {
    /// Serializa no formato que o autômato aceita.
    pub fn texto(&self, reg: &Registro) -> String {
        let mut s = format!("{{\"acao\":\"{}\"", reg.ferramentas[self.ferramenta].nome);
        for (k, v) in &self.args {
            s.push_str(&format!(",\"{k}\":\"{v}\""));
        }
        s.push('}');
        s
    }

    /// Lê de volta o formato acima.
    ///
    /// Deliberadamente simples: quem gera é o autômato, então a entrada é sempre
    /// bem formada. Isto existe para testes, para o modo texto, e para o dia em que
    /// alguém colar uma chamada à mão.
    pub fn parse(txt: &str, reg: &Registro) -> Result<Chamada, String> {
        let t = txt.trim();
        let corpo = t
            .strip_prefix('{')
            .and_then(|x| x.strip_suffix('}'))
            .ok_or("chamada deve estar entre chaves")?;

        let mut ferramenta = None;
        let mut args = Vec::new();
        for parte in dividir_pares(corpo) {
            let (k, v) = parte.split_once(':').ok_or("par sem dois-pontos")?;
            let k = k.trim().trim_matches('"');
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .ok_or_else(|| format!("valor de {k} nao esta entre aspas"))?;
            if k == "acao" {
                ferramenta = reg.indice(v);
                if ferramenta.is_none() {
                    return Err(format!("ferramenta desconhecida: {v}"));
                }
            } else {
                args.push((k.to_string(), v.to_string()));
            }
        }
        Ok(Chamada {
            ferramenta: ferramenta.ok_or("chamada sem campo acao")?,
            args,
        })
    }
}

/// Divide por vírgulas que estão FORA de aspas — um caminho do Windows pode conter
/// vírgula, e um `split(',')` ingênuo o quebraria ao meio.
fn dividir_pares(s: &str) -> Vec<&str> {
    let mut partes = Vec::new();
    let (mut ini, mut dentro) = (0usize, false);
    for (i, c) in s.char_indices() {
        match c {
            '"' => dentro = !dentro,
            ',' if !dentro => {
                partes.push(&s[ini..i]);
                ini = i + 1;
            }
            _ => {}
        }
    }
    if ini < s.len() {
        partes.push(&s[ini..]);
    }
    partes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registro_padrao_esta_consistente() {
        let r = Registro::padrao();
        // 22 ferramentas de verdade + `perguntar`, que é a ação de NÃO agir.
        //
        // Este numero e conferido de proposito: crescer o registro CUSTA ACURACIA, e
        // ferramenta nova tem de ser uma decisao, nunca efeito colateral de um
        // commit. O historico:
        //
        //   10 -> 19   derrubou de 108,3 para 107,0, e so voltou a 112,7 depois do
        //              conserto dos pocos
        //   19 -> 20   `atalho`, em 06/09: -2,58 (t=-1,84), pago depois pelo
        //              conserto da contradicao do fora-de-escopo
        //   20 -> 23   `buscar_no_conteudo`, `editar_arquivo` e `ler_imagem`, em
        //              09/09 -- as tres que vieram pela ponte do Harness. O custo
        //              esta SENDO MEDIDO com o benchmark de 150 como guarda
        //
        // Decisao do John em 09/09, depois de eu mostrar que das 25 da ponte, 16 so
        // servem dentro de um laco de LLM, 5 ela ja tem, e 1 (web_search) exige
        // chave paga -- que ele cortou, mantendo a busca no DuckDuckGo.
        assert_eq!(r.n(), 22);
        assert_eq!(
            r.ferramentas[0].nome, "perguntar",
            "perguntar tem de ser a primeira: e o que ela escolhe quando nada encaixa"
        );
        assert!(
            r.ferramentas[0].params.is_empty(),
            "perguntar nao recebe argumento — nao ha o que apontar num pedido que ela nao entendeu"
        );
        let mut nomes = r.nomes();
        nomes.sort();
        let antes = nomes.len();
        nomes.dedup();
        assert_eq!(nomes.len(), antes, "nomes de ferramenta duplicados");
        for f in &r.ferramentas {
            assert!(!f.nome.is_empty() && !f.descricao.is_empty());
            // O nome vai virar caminho num autômato: só minúsculas e underscore.
            assert!(
                f.nome.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "nome invalido: {}",
                f.nome
            );
            // Obrigatórios antes dos opcionais — o autômato depende disso.
            let mut viu_opcional = false;
            for p in &f.params {
                if !p.obrigatorio {
                    viu_opcional = true;
                } else {
                    assert!(!viu_opcional, "{}: obrigatorio depois de opcional", f.nome);
                }
            }
        }
    }

    #[test]
    fn ida_e_volta_de_chamada() {
        let r = Registro::padrao();
        let c = Chamada {
            ferramenta: r.indice("listar_pasta").unwrap(),
            args: vec![("caminho".into(), "C:\\Users\\User\\Projetos".into())],
        };
        let t = c.texto(&r);
        assert_eq!(t, "{\"acao\":\"listar_pasta\",\"caminho\":\"C:\\Users\\User\\Projetos\"}");
        assert_eq!(Chamada::parse(&t, &r).unwrap(), c);
    }

    #[test]
    fn parse_nao_quebra_valor_com_virgula() {
        let r = Registro::padrao();
        let t = "{\"acao\":\"calcular\",\"expressao\":\"1,5*2\"}";
        let c = Chamada::parse(t, &r).unwrap();
        assert_eq!(c.args[0].1, "1,5*2");
    }

    #[test]
    fn executar_de_verdade_as_ferramentas_que_so_observam() {
        let r = Registro::padrao();
        let pol = Politica::default(); // sandbox
        for nome in ["hora", "memoria", "disco"] {
            if !cfg!(windows) && nome != "hora" {
                continue;
            }
            let c = Chamada {
                ferramenta: r.indice(nome).unwrap(),
                args: vec![],
            };
            let saida = r.executar(&c, &pol).unwrap_or_else(|e| panic!("{nome}: {e}"));
            assert!(!saida.is_empty(), "{nome} devolveu vazio");
            println!("  {nome:<16} → {}", saida.lines().next().unwrap_or(""));
        }

        let c = Chamada {
            ferramenta: r.indice("calcular").unwrap(),
            args: vec![("expressao".into(), "(15/100)*340".into())],
        };
        assert_eq!(r.executar(&c, &pol).unwrap(), "51");
    }

    #[test]
    fn parametro_obrigatorio_faltando_e_erro() {
        let r = Registro::padrao();
        let c = Chamada {
            ferramenta: r.indice("listar_pasta").unwrap(),
            args: vec![],
        };
        assert!(r.executar(&c, &Politica::default()).is_err());
    }

    #[test]
    fn manual_menciona_todas_as_ferramentas() {
        let r = Registro::padrao();
        let m = r.manual();
        for f in &r.ferramentas {
            assert!(m.contains(&f.nome), "manual nao cita {}", f.nome);
        }
        println!("\n--- manual ({} bytes) ---\n{}", m.len(), &m[..m.len().min(400)]);
    }
}
