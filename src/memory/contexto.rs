//! Contexto de conversa: o que "esse arquivo" quer dizer.
//!
//! ## O problema, medido
//!
//! Numa régua de 500 frases escritas por outra pessoa, **265 não dizem qual** é o
//! objeto: *"lê esse arquivo pra mim"*, *"lista essa pasta"*, *"roda esse comando"*.
//! Não é descuido de quem escreveu — é como gente fala. O objeto ficou no turno
//! anterior.
//!
//! A Teka não tem como resolver isso sozinha. A cabeça de ponteiro **copia um trecho
//! da entrada**, e nessa entrada não existe trecho nenhum para copiar. Ela abstém, e
//! abstém certo.
//!
//! ## O desenho que eu tentei primeiro, e por que ele não funciona
//!
//! A ideia era ler a **segunda colocada** da cabeça de intenção: ela estaria dizendo
//! *"eu ia ler um arquivo, mas falta o objeto"*. Medido, ela não diz nada disso:
//!
//! ```text
//! "mostra esse arquivo de novo"  ->  perguntar p=0,989 | 2o memoria  p=0,009
//! "abre esse arquivo"            ->  perguntar p=1,000 | 2o hora     p=0,000
//! "le esse arquivo pra mim"      ->  perguntar p=1,000 | 2o memoria  p=0,000
//! ```
//!
//! A segunda colocada é ruído. Faz sentido: no treino "le esse arquivo" nunca
//! aparece, então ela aprendeu a mapear a **superfície** "verbo sem nome de arquivo"
//! direto para `perguntar` — não como disputa entre duas ferramentas.
//!
//! ## O desenho que funciona
//!
//! Trocar o pronome pelo objeto lembrado e **perguntar de novo**. A decisão continua
//! sendo do modelo, sobre uma frase que agora tem objeto:
//!
//! ```text
//! você:  le o notas.md                → ler_arquivo("notas.md")   guarda o arquivo
//! você:  mostra esse arquivo de novo  → abstém
//!        reescrito: "mostra notas.md de novo"
//!                                     → ler_arquivo("notas.md")   [pelo contexto]
//! ```
//!
//! A diferença é de princípio. Eu **não** escrevo regra de verbo — isso seria
//! reimplementar a cabeça de intenção à mão, e o Achado 19 mostra o estrago de
//! ensinar "verbo X = tal coisa". Aqui eu só forneço o **referente**, que é
//! literalmente o que "contexto" quer dizer. Anáfora é classe fechada: meia dúzia de
//! demonstrativos, não um vocabulário aberto.
//!
//! Medido antes de escrever — a frase reescrita é resolvida certo mesmo quando fica
//! agramatical, porque o modelo é byte a byte e o ponteiro só precisa achar o trecho:
//!
//! ```text
//! "Mostra o que tem dados"           -> listar_pasta("dados")
//! "Da uma olhada dados pra mim"      -> listar_pasta("dados")
//! "Mostra o que tem na pasta dados"  -> listar_pasta("dados")
//! ```
//!
//! ## As três guardas
//!
//! 1. **Só quando ela abstém.** Se escolheu uma ferramenta, o pedido tinha objeto e o
//!    contexto não tem o que opinar. Nunca passa por cima de pedido explícito.
//!
//! 2. **Validade curta** ([`VALIDADE`] turnos). Esquecer cedo devolve um `perguntar`,
//!    que é chato; lembrar tarde age sobre o arquivo errado, que é pior.
//!
//! 3. **O que mexe no mundo não é adivinhado.** `escrever_arquivo` e
//!    `executar_comando` passam pela confirmação de quem chama, não pela ação direta.
//!    Essa guarda mora em `main`, porque só lá a chamada final é conhecida.
//!
//! E toda reescrita é anunciada com o valor usado. Errar barato e às claras é
//! aceitável; errar em silêncio não é.

use crate::tools::{Chamada, Registro};

/// Por quantos turnos uma lembrança continua valendo.
pub const VALIDADE: u64 = 5;

#[derive(Clone, Debug)]
struct Lembranca {
    valor: String,
    turno: u64,
}

/// Que tipo de coisa uma ferramenta consome, para saber de qual gaveta puxar.
///
/// Existe porque o tipo declarado no registro não basta: `listar_pasta` e
/// `ler_arquivo` recebem os dois um `Caminho`, mas um quer pasta e o outro quer
/// arquivo. Uma gaveta só faria "lê esse arquivo" abrir um diretório.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gaveta {
    Arquivo,
    Pasta,
    Comando,
    Expressao,
}

/// Onde guardar o objeto de cada ferramenta.
fn gaveta_de(nome: &str) -> Option<Gaveta> {
    match nome {
        "ler_arquivo" | "escrever_arquivo" => Some(Gaveta::Arquivo),
        "listar_pasta" => Some(Gaveta::Pasta),
        "executar_comando" => Some(Gaveta::Comando),
        "calcular" => Some(Gaveta::Expressao),
        // `procurar_arquivo` recebe um PEDAÇO de nome ("config"), não um caminho:
        // guardá-lo como arquivo faria `ler_arquivo` tentar abrir "config".
        // `hora`, `memoria` e `disco` não têm objeto que valha lembrar.
        _ => None,
    }
}

/// Expressões anafóricas, e para qual gaveta cada uma aponta.
///
/// Classe fechada de propósito — demonstrativo e artigo definido com o substantivo
/// do domínio. Nada de verbo aqui: verbo é o que a cabeça de intenção decide, e
/// duplicar isso em regra escrita à mão é o erro que este módulo existe para evitar.
///
/// Comparadas em minúscula e sem acento; a substituição acontece no texto original.
const ANAFORAS: &[(&str, Gaveta)] = &[
    // arquivo
    ("nesse arquivo", Gaveta::Arquivo),
    ("neste arquivo", Gaveta::Arquivo),
    ("desse arquivo", Gaveta::Arquivo),
    ("deste arquivo", Gaveta::Arquivo),
    ("esse arquivo", Gaveta::Arquivo),
    ("este arquivo", Gaveta::Arquivo),
    ("aquele arquivo", Gaveta::Arquivo),
    ("o arquivo", Gaveta::Arquivo),
    ("nesse documento", Gaveta::Arquivo),
    ("desse documento", Gaveta::Arquivo),
    ("esse documento", Gaveta::Arquivo),
    ("este documento", Gaveta::Arquivo),
    ("o documento", Gaveta::Arquivo),
    // pasta
    ("nessa pasta", Gaveta::Pasta),
    ("nesta pasta", Gaveta::Pasta),
    ("dessa pasta", Gaveta::Pasta),
    ("desta pasta", Gaveta::Pasta),
    ("essa pasta", Gaveta::Pasta),
    ("esta pasta", Gaveta::Pasta),
    ("aquela pasta", Gaveta::Pasta),
    ("a pasta", Gaveta::Pasta),
    ("nesse diretorio", Gaveta::Pasta),
    ("desse diretorio", Gaveta::Pasta),
    ("esse diretorio", Gaveta::Pasta),
    ("este diretorio", Gaveta::Pasta),
    ("o diretorio", Gaveta::Pasta),
    ("desse caminho", Gaveta::Pasta),
    ("esse caminho", Gaveta::Pasta),
    // comando
    ("nesse comando", Gaveta::Comando),
    ("desse comando", Gaveta::Comando),
    ("esse comando", Gaveta::Comando),
    ("este comando", Gaveta::Comando),
    ("aquele comando", Gaveta::Comando),
    ("o comando", Gaveta::Comando),
    // locativo: "de onde eu estou" e um lugar, e lugar e pasta
    ("por aqui", Gaveta::Pasta),
    ("daqui", Gaveta::Pasta),
    ("aqui dentro", Gaveta::Pasta),
    ("da pasta", Gaveta::Pasta),
    ("no diretorio", Gaveta::Pasta),
    ("do diretorio", Gaveta::Pasta),
    // conta
    ("dessa conta", Gaveta::Expressao),
    ("essa conta", Gaveta::Expressao),
    ("esta conta", Gaveta::Expressao),
    ("essa operacao", Gaveta::Expressao),
    ("essa expressao", Gaveta::Expressao),
    ("esse calculo", Gaveta::Expressao),
];

/// Pronomes que so dizem o GENERO, e o genero ja elimina metade das gavetas.
///
/// "dela" nao pode se referir a "o arquivo"; "dele" nao pode se referir a "a pasta".
/// E informacao de graca que o portugues carrega e que nao custa nada usar.
const POR_GENERO: &[(&str, &[Gaveta])] = &[
    ("dele", &[Gaveta::Arquivo, Gaveta::Comando]),
    ("nele", &[Gaveta::Arquivo, Gaveta::Comando]),
    ("dela", &[Gaveta::Pasta, Gaveta::Expressao]),
    ("nela", &[Gaveta::Pasta, Gaveta::Expressao]),
];

/// Deiticos puros: nao dizem nada sobre o que apontam.
///
/// "isso" aparece em `calcular` ("Calcula isso ai"), `executar_comando` ("Roda isso
/// no terminal") e `ler_arquivo` ("Le isso aqui") — 20 das 118 frases descobertas.
/// Quem desambigua e o VERBO, e escrever regra de verbo e o que o Achado 19 proibe.
///
/// Entao aqui se oferece UM CANDIDATO POR GAVETA CHEIA, em ordem de quem foi
/// mencionado por ultimo, e quem escolhe e o modelo: vence a reescrita que ele
/// resolve sem abster. Continua sendo ele decidindo — eu so listo os referentes.
const DEITICOS: &[&str] = &[
    "isso ai", "isso aqui", "isso", "aquilo", "disso", "nisso", "esse negocio",
];

/// O pedido reescrito, e o objeto que entrou no lugar do pronome.
#[derive(Clone, Debug, PartialEq)]
pub struct Reescrita {
    pub pedido: String,
    pub valor: String,
}

/// O que a conversa mencionou por último, por gaveta.
#[derive(Clone, Debug, Default)]
pub struct Contexto {
    arquivo: Option<Lembranca>,
    pasta: Option<Lembranca>,
    comando: Option<Lembranca>,
    expressao: Option<Lembranca>,
    turno: u64,
}

/// Minúscula e sem acento, com o mapa de volta para os bytes do original.
///
/// O mapa é necessário porque tirar acento **muda o tamanho em bytes**: `á` ocupa 2
/// e `a` ocupa 1. Sem ele, um casamento encontrado no texto normalizado recortaria a
/// posição errada do original.
fn dobrar(t: &str) -> (String, Vec<usize>) {
    let mut norm = String::with_capacity(t.len());
    // offsets[i] = byte no original onde comeca o i-esimo char de `norm`
    let mut offsets = Vec::with_capacity(t.len());
    for (b, c) in t.char_indices() {
        let simples = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            outro => outro.to_ascii_lowercase(),
        };
        norm.push(simples);
        offsets.push(b);
    }
    offsets.push(t.len());
    (norm, offsets)
}

impl Contexto {
    pub fn novo() -> Self {
        Self::default()
    }

    /// Marca a passagem de um turno. Chame uma vez por pedido do usuário.
    pub fn avancar(&mut self) {
        self.turno += 1;
    }

    fn gaveta_mut(&mut self, g: Gaveta) -> &mut Option<Lembranca> {
        match g {
            Gaveta::Arquivo => &mut self.arquivo,
            Gaveta::Pasta => &mut self.pasta,
            Gaveta::Comando => &mut self.comando,
            Gaveta::Expressao => &mut self.expressao,
        }
    }

    fn ler(&self, g: Gaveta) -> Option<&str> {
        let l = match g {
            Gaveta::Arquivo => self.arquivo.as_ref(),
            Gaveta::Pasta => self.pasta.as_ref(),
            Gaveta::Comando => self.comando.as_ref(),
            Gaveta::Expressao => self.expressao.as_ref(),
        }?;
        if self.turno.saturating_sub(l.turno) <= VALIDADE {
            Some(l.valor.as_str())
        } else {
            None
        }
    }

    /// Guarda o objeto de uma chamada que aconteceu.
    ///
    /// Chame **depois** de executar e só quando deu certo: guardar o alvo de uma
    /// chamada que falhou faria "esse arquivo" apontar para um caminho inexistente.
    pub fn observar(&mut self, reg: &Registro, c: &Chamada) {
        let Some(f) = reg.ferramentas.get(c.ferramenta) else {
            return;
        };
        let Some(g) = gaveta_de(f.nome.as_str()) else {
            return;
        };
        // O primeiro parâmetro obrigatório é o objeto da ação.
        let Some(p) = f.params.iter().find(|p| p.obrigatorio) else {
            return;
        };
        let Some((_, v)) = c.args.iter().find(|(k, _)| *k == p.nome) else {
            return;
        };
        if v.trim().is_empty() {
            return;
        }
        let turno = self.turno;
        *self.gaveta_mut(g) = Some(Lembranca {
            valor: v.clone(),
            turno,
        });
    }

    /// Troca o pronome pelo objeto lembrado. Devolve o primeiro candidato.
    ///
    /// Atalho para quem só quer um. Ver [`Contexto::candidatos`] para o caso do
    /// pronome ambíguo, onde há mais de uma leitura possível.
    pub fn reescrever(&self, pedido: &str) -> Option<Reescrita> {
        self.candidatos(pedido).into_iter().next()
    }

    /// Todas as leituras possíveis do pedido, da mais provável para a menos.
    ///
    /// Uma só para anáfora tipada ("esse arquivo"); até quatro para dêitico puro
    /// ("isso"), uma por gaveta cheia, ordenadas por quem foi mencionado mais
    /// recentemente. Não decide ferramenta nenhuma — quem decide é o modelo, sobre
    /// cada pedido reescrito.
    pub fn candidatos(&self, pedido: &str) -> Vec<Reescrita> {
        let (norm, offsets) = dobrar(pedido);

        // Acha o casamento mais longo em qualquer das tres tabelas. Mais longo
        // primeiro porque "nessa pasta" contem "a pasta", e casar o curto deixaria
        // "ness<valor>" grudado.
        let mut melhor: Option<(usize, usize, Vec<Gaveta>)> = None;
        let mut considerar = |i: usize, fim: usize, gs: Vec<Gaveta>| {
            let b = norm.as_bytes();
            let antes_ok = i == 0 || !b[i - 1].is_ascii_alphanumeric();
            let depois_ok = fim >= norm.len() || !b[fim].is_ascii_alphanumeric();
            if !antes_ok || !depois_ok {
                return;
            }
            if melhor.as_ref().map(|(a, z, _)| z - a).unwrap_or(0) < fim - i {
                melhor = Some((i, fim, gs));
            }
        };
        for (padrao, gs) in POR_GENERO {
            if let Some(i) = norm.find(padrao) {
                considerar(i, i + padrao.len(), gs.to_vec());
            }
        }
        for padrao in DEITICOS {
            if let Some(i) = norm.find(padrao) {
                // Todas as gavetas: a ordem por recencia vem depois.
                considerar(
                    i,
                    i + padrao.len(),
                    vec![Gaveta::Arquivo, Gaveta::Pasta, Gaveta::Comando, Gaveta::Expressao],
                );
            }
        }
        for (padrao, g) in ANAFORAS {
            // `norm` e ASCII depois de `dobrar`, entao byte == indice de char.
            if let Some(i) = norm.find(padrao) {
                considerar(i, i + padrao.len(), vec![*g]);
            }
        }

        let Some((ini, fim, mut gavetas)) = melhor else {
            return Vec::new();
        };
        // Mencionado por ultimo vem primeiro: e a leitura natural de "isso".
        gavetas.sort_by_key(|g| std::cmp::Reverse(self.turno_de(*g)));

        let (a, b) = (offsets[ini], offsets[fim]);
        gavetas
            .into_iter()
            .filter_map(|g| {
                let valor = self.ler(g)?.to_string();
                let mut novo = String::with_capacity(pedido.len() + valor.len());
                novo.push_str(&pedido[..a]);
                novo.push_str(&valor);
                novo.push_str(&pedido[b..]);
                Some(Reescrita { pedido: novo, valor })
            })
            .collect()
    }

    /// Em que turno esta gaveta foi preenchida. 0 = vazia ou vencida.
    fn turno_de(&self, g: Gaveta) -> u64 {
        let l = match g {
            Gaveta::Arquivo => self.arquivo.as_ref(),
            Gaveta::Pasta => self.pasta.as_ref(),
            Gaveta::Comando => self.comando.as_ref(),
            Gaveta::Expressao => self.expressao.as_ref(),
        };
        match l {
            Some(l) if self.turno.saturating_sub(l.turno) <= VALIDADE => l.turno,
            _ => 0,
        }
    }

    /// Esquece tudo. Para quando o assunto muda de vez.
    pub fn limpar(&mut self) {
        let t = self.turno;
        *self = Self::default();
        self.turno = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(reg: &Registro, nome: &str, args: &[(&str, &str)]) -> Chamada {
        Chamada {
            ferramenta: reg.indice(nome).unwrap(),
            args: args.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        }
    }

    fn com_arquivo(reg: &Registro, v: &str) -> Contexto {
        let mut c = Contexto::novo();
        c.avancar();
        c.observar(reg, &ch(reg, "ler_arquivo", &[("caminho", v)]));
        c.avancar();
        c
    }

    /// Deitico puro oferece UMA leitura POR GAVETA, da mais recente para a mais velha.
    #[test]
    fn deitico_puro_oferece_uma_leitura_por_gaveta() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "notas.md")]));
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "listar_pasta", &[("caminho", "dados")]));
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "executar_comando", &[("comando", "tasklist")]));
        ctx.avancar();

        let c = ctx.candidatos("Roda isso no terminal");
        assert_eq!(c.len(), 3, "esperava uma leitura por gaveta cheia: {c:?}");
        // Mencionado por ultimo primeiro: comando, pasta, arquivo.
        assert_eq!(c[0].valor, "tasklist");
        assert_eq!(c[1].valor, "dados");
        assert_eq!(c[2].valor, "notas.md");
        assert_eq!(c[0].pedido, "Roda tasklist no terminal");
    }

    /// Anafora TIPADA continua dando uma leitura so — o tipo ja decidiu.
    #[test]
    fn anafora_tipada_nao_vira_lista() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "notas.md")]));
        ctx.observar(&reg, &ch(&reg, "listar_pasta", &[("caminho", "dados")]));
        ctx.avancar();
        let c = ctx.candidatos("le esse arquivo");
        assert_eq!(c.len(), 1, "anafora tipada devia decidir sozinha: {c:?}");
        assert_eq!(c[0].valor, "notas.md");
    }

    /// O genero do pronome elimina metade das gavetas.
    ///
    /// "dela" nao pode se referir a "o arquivo". E informacao que o portugues da de
    /// graca e que evita oferecer leitura impossivel ao modelo.
    #[test]
    fn o_genero_do_pronome_corta_as_gavetas_impossiveis() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "notas.md")]));
        ctx.observar(&reg, &ch(&reg, "listar_pasta", &[("caminho", "dados")]));
        ctx.observar(&reg, &ch(&reg, "calcular", &[("expressao", "340*12")]));
        ctx.observar(&reg, &ch(&reg, "executar_comando", &[("comando", "tasklist")]));
        ctx.avancar();

        let fem: Vec<String> = ctx.candidatos("mostra o conteudo dela").into_iter()
            .map(|r| r.valor).collect();
        assert!(fem.iter().any(|x| x == "dados") && fem.iter().any(|x| x == "340*12"), "feminino: {fem:?}");
        assert!(!fem.iter().any(|x| x == "notas.md"), "\"dela\" nao aponta para o arquivo: {fem:?}");

        let masc: Vec<String> = ctx.candidatos("mostra o conteudo dele").into_iter()
            .map(|r| r.valor).collect();
        assert!(masc.iter().any(|x| x == "notas.md") && masc.iter().any(|x| x == "tasklist"), "masculino: {masc:?}");
        assert!(!masc.iter().any(|x| x == "dados"), "\"dele\" nao aponta para a pasta: {masc:?}");
    }

    /// "isso ai" tem de ganhar de "isso": casamento mais longo primeiro.
    #[test]
    fn o_deitico_mais_longo_ganha() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "calcular", &[("expressao", "340*12")]));
        ctx.avancar();
        let c = ctx.candidatos("Calcula isso ai rapidinho");
        assert!(!c.is_empty());
        // Se casasse so "isso", sobraria " ai" no meio.
        assert_eq!(c[0].pedido, "Calcula 340*12 rapidinho");
    }

    /// Gaveta vazia nao vira candidato.
    #[test]
    fn gaveta_vazia_nao_entra_na_lista() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "notas.md")]));
        ctx.avancar();
        let c = ctx.candidatos("Roda isso");
        assert_eq!(c.len(), 1, "so o arquivo estava cheio: {c:?}");
        assert_eq!(c[0].valor, "notas.md");
    }
    #[test]
    fn troca_o_pronome_pelo_arquivo_lembrado() {
        let reg = Registro::padrao();
        let ctx = com_arquivo(&reg, "notas.md");
        let r = ctx.reescrever("mostra esse arquivo de novo").expect("devia reescrever");
        assert_eq!(r.pedido, "mostra notas.md de novo");
        assert_eq!(r.valor, "notas.md");
    }

    #[test]
    fn casa_com_maiuscula_e_acento() {
        // O texto que gente digita tem maiuscula e acento; as anaforas da tabela sao
        // minusculas e sem acento. Sem `dobrar`, "Nessa Pasta" nao casaria.
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "listar_pasta", &[("caminho", "dados")]));
        ctx.avancar();
        let r = ctx.reescrever("Mostra o que tem Nessa Pasta").expect("devia reescrever");
        assert_eq!(r.pedido, "Mostra o que tem dados");
    }

    #[test]
    fn o_acento_nao_desloca_o_recorte() {
        // A armadilha do mapa de offsets: "á" ocupa 2 bytes e vira "a" com 1. Se o
        // recorte usasse o indice do texto normalizado, cortaria no lugar errado.
        let reg = Registro::padrao();
        let ctx = com_arquivo(&reg, "notas.md");
        let r = ctx.reescrever("é urgente, lê esse arquivo já").expect("devia reescrever");
        assert_eq!(r.pedido, "é urgente, lê notas.md já");
    }

    #[test]
    fn o_casamento_mais_longo_ganha() {
        // "a pasta" esta contido em "nessa pasta". Casar o curto deixaria "ness"
        // grudado no valor.
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "listar_pasta", &[("caminho", "dados")]));
        ctx.avancar();
        let r = ctx.reescrever("lista tudo nessa pasta").expect("devia reescrever");
        assert_eq!(r.pedido, "lista tudo dados");
    }

    #[test]
    fn pasta_e_arquivo_sao_gavetas_separadas() {
        // Uma gaveta so faria "le esse arquivo" abrir a ultima PASTA.
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "listar_pasta", &[("caminho", "dados")]));
        ctx.avancar();
        assert!(ctx.reescrever("le esse arquivo").is_none());
        assert!(ctx.reescrever("lista essa pasta").is_some());
    }

    #[test]
    fn sem_anafora_nao_reescreve() {
        let reg = Registro::padrao();
        let ctx = com_arquivo(&reg, "notas.md");
        assert!(ctx.reescrever("que horas sao").is_none());
        assert!(ctx.reescrever("le o config.json").is_none());
    }

    #[test]
    fn respeita_fronteira_de_palavra() {
        let reg = Registro::padrao();
        let ctx = com_arquivo(&reg, "notas.md");
        // "o arquivo" nao pode casar dentro de "os arquivos".
        assert!(ctx.reescrever("quantos arquivos tem ai").is_none());
    }

    #[test]
    fn procurar_arquivo_nao_vira_caminho() {
        // `procurar_arquivo` recebe um PEDACO de nome ("config"), nao um caminho.
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "procurar_arquivo", &[("nome", "config")]));
        ctx.avancar();
        assert!(ctx.reescrever("le esse arquivo").is_none());
    }

    #[test]
    fn a_lembranca_vence() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "notas.md")]));
        for _ in 0..VALIDADE {
            ctx.avancar();
        }
        assert!(ctx.reescrever("le esse arquivo").is_some());
        ctx.avancar();
        assert!(
            ctx.reescrever("le esse arquivo").is_none(),
            "lembrou de um arquivo velho demais"
        );
    }

    #[test]
    fn a_lembranca_mais_recente_ganha() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "antigo.txt")]));
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "ler_arquivo", &[("caminho", "novo.txt")]));
        ctx.avancar();
        assert_eq!(ctx.reescrever("le esse arquivo").unwrap().valor, "novo.txt");
    }

    #[test]
    fn comando_e_conta_tambem() {
        let reg = Registro::padrao();
        let mut ctx = Contexto::novo();
        ctx.avancar();
        ctx.observar(&reg, &ch(&reg, "executar_comando", &[("comando", "tasklist")]));
        ctx.observar(&reg, &ch(&reg, "calcular", &[("expressao", "340*12")]));
        ctx.avancar();
        assert_eq!(ctx.reescrever("roda esse comando").unwrap().pedido, "roda tasklist");
        assert_eq!(ctx.reescrever("refaz essa conta").unwrap().pedido, "refaz 340*12");
    }

    #[test]
    fn limpar_esquece_tudo() {
        let reg = Registro::padrao();
        let mut ctx = com_arquivo(&reg, "notas.md");
        ctx.limpar();
        ctx.avancar();
        assert!(ctx.reescrever("le esse arquivo").is_none());
    }

    #[test]
    fn o_valor_aparece_inteiro_no_pedido_reescrito() {
        // O ponteiro COPIA um trecho da entrada: se o valor nao aparecer literal no
        // texto reescrito, nao ha o que copiar e a reescrita nao serve para nada.
        let reg = Registro::padrao();
        for v in ["notas.md", "C:\\temp\\saida.log", "configuração.ini", "src\\main.rs"] {
            let ctx = com_arquivo(&reg, v);
            let r = ctx.reescrever("abre esse arquivo pra mim").unwrap();
            assert!(r.pedido.contains(v), "{v:?} sumiu de {:?}", r.pedido);
        }
    }
}
