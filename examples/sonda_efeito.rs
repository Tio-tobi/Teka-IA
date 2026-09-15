//! A régua confere a CHAMADA. Esta sonda confere o EFEITO.
//!
//! `dados/frases_teste.txt` pergunta "ela escolheu a ferramenta e o argumento
//! certos?". Ninguém pergunta "e o disco, ficou como devia?". Já custou caro: houve
//! um período em que `ler_arquivo` deu **0 acertos em dez rodadas** com a ferramenta
//! e o argumento CORRETOS — só a execução falhava, porque `ler` resolvia caminho
//! contra o diretório do processo e `escrever` contra a raiz da política. A régua
//! não viu nada, porque não havia nada para ela ver: a chamada estava perfeita.
//!
//! Aqui cada tarefa monta um cenário conhecido num diretório temporário, deixa a
//! Teka escolher, **executa** o que ela escolheu, e depois olha o disco.
//!
//! ## As três categorias, e por que não podem virar um número só
//!
//! - **efeito aconteceu** — sucesso de verdade, o único número que interessa a quem usa.
//! - **ferramenta errada** — a régua JÁ mede isto. Não é achado novo, é o piso conhecido.
//! - **ferramenta certa, efeito não aconteceu** — o buraco. É a única coluna que a
//!   régua é cega para, e é a razão desta sonda existir.
//!
//! Somar as duas últimas esconderia exatamente a distinção que se quer medir.
//!
//! A terceira ainda se parte em duas, porque as causas são diferentes e os consertos
//! também: **argumento errado** (a régua vê, e o conserto é dado de treino) e
//! **argumento certo e mesmo assim nada** (a régua é cega, e o conserto é código).
//!
//! ## FALSIFICAÇÃO, dita antes de rodar
//!
//! A tese é "existe um buraco entre a chamada certa e o efeito certo, e ele não é
//! pequeno o bastante para se ignorar".
//!
//! - Se **ferramenta certa + efeito não aconteceu** der ZERO, a tese está ERRADA:
//!   a régua da chamada basta, e esta sonda não mede nada que já não se meça.
//! - Se quase tudo cair em **ferramenta errada**, a sonda FALHOU — e falhou por
//!   minha culpa, não por a tese estar errada: eu não cheguei perto o bastante do
//!   efeito para medi-lo. O número honesto nesse caso é "não medido", e o que tem
//!   de mudar são as frases, não a conclusão.
//! - Se o buraco aparecer **só** nas de duas obrigatórias (`copiar`, `mover`,
//!   `escrever`), então não é buraco de execução: é a fraqueza de argumento que já
//!   está documentada e que a régua já mede. Não vale como achado.
//! - O buraco que justifica a sonda é o das de UMA obrigatória (`ler`, `listar`,
//!   `info`, `apagar`, `criar_pasta`): chamada byte a byte igual à esperada, e disco
//!   ou saída errados mesmo assim.
//!
//! ## RESULTADO, 14/09 — a falsificação DISPAROU
//!
//! `teka_fechar_s19`, 40 tarefas executadas, 1 semente (é a única que existe):
//!
//! ```text
//!   efeito aconteceu                 31   77,5%
//!   ferramenta certa, SEM efeito      5   12,5%   <- 5 de argumento, 0 de execução
//!   ferramenta errada                 4   10,0%
//! ```
//!
//! **A coluna que a régua é cega para deu ZERO.** Pela falsificação escrita acima,
//! isso quer dizer que a tese está errada *hoje*: para estas 40 tarefas, chamada
//! certa implica efeito certo, e a régua da chamada basta. As 5 falhas com
//! ferramenta certa são todas de argumento — coisa que o benchmark já pontua.
//!
//! Isso não é um resultado vazio: é o conserto do `resolver_leitura` sendo
//! **verificado pelo efeito** pela primeira vez, e não pela chamada. E a sonda tinha
//! como ver a volta do bug: todo gabarito usa caminho RELATIVO (`notas.md`,
//! `docs/manual.txt`) e a área é uma pasta temporária. Se `ler` voltasse a resolver
//! contra o diretório do processo, procuraria `notas.md` na raiz do repositório, que
//! não tem nenhum — e o CONTROLE abaixo cairia de 40/40 antes de a Teka ser
//! consultada. O instrumento discrimina; o que ele não achou, não estava lá.
//!
//! A ressalva que não pode sumir: **uma semente**. Sem variância, isto é um ponto,
//! não uma distribuição. `teka_fechar_s20..s30` ainda não existem.
//!
//! ## SEGURANÇA — o que esta sonda não faz, e por quê
//!
//! A Teka escreve, apaga e lança processo na máquina de verdade. Três travas, e
//! nenhuma confia na outra:
//!
//! 1. `Politica::real_sem_processos(area)` — escrita confinada à raiz, e
//!    `processos: false`. `abrir_programa`, `fechar_programa`, `executar_comando` e
//!    `atalho` não são testados aqui de propósito.
//! 2. **A raiz não confina LEITURA.** Está documentado em `Politica::resolver_leitura`
//!    e é política, não bug: caminho absoluto passa. Então a sonda confere ela mesma,
//!    antes de executar, se todo argumento de tipo caminho cai dentro da área — e
//!    recusa executar quando não cai. Sem isso, um `procurar_arquivo(raiz="C:\")`
//!    varreria o disco do John por minutos.
//! 3. Tudo dentro de `std::env::temp_dir()`, montado do zero a cada tarefa e apagado
//!    no fim.
//!
//! ## O que fica de fora, e é diferente de "falhou"
//!
//! - **`editar`** não existe no registro. `Primitiva::Editar` está em `prim.rs`, mas
//!   `Registro::padrao()` não a registra — são 22 ferramentas e nenhuma é ela. A
//!   Teka não tem como emitir a chamada, então não há efeito a medir. As frases
//!   ficam aqui só para mostrar ONDE o pedido cai.
//! - **`buscar_no_conteudo`** executa pela ponte do Harness (`pela_ponte`), que
//!   **ignora a `Politica`** e ainda sobe um processo (`ponte_auto::garantir`). Duas
//!   razões para não executar, e a segunda é a regra dura. Mesmo tratamento: mede-se
//!   a escolha, não o efeito.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::seguranca::normalizar;
use teka::tools::{Politica, Registro, TipoParam};

// ---------------------------------------------------------------------------
// o cenário
// ---------------------------------------------------------------------------

/// Montado do ZERO antes de cada tarefa. Independência entre tarefas importa mais
/// que velocidade: uma `mover` que falhou não pode envenenar a `apagar` seguinte.
///
/// Os conteúdos são distintos entre si de propósito — é o que deixa `Conf::Saida`
/// distinguir "leu o arquivo certo" de "leu qualquer arquivo".
const CENARIO: &[(&str, &str)] = &[
    ("notas.md", "lista de compras\npao\nleite\ncafe\n"),
    ("config.json", "{\"tema\":\"escuro\",\"fonte\":14}\n"),
    ("relatorio.txt", "relatorio trimestral\nvendas subiram 12 por cento\n"),
    ("leia_me.txt", "teka le isto aqui\n"),
    ("agenda.txt", "reuniao segunda as dez\n"),
    ("docs/manual.txt", "manual do usuario\nsecao 1 instalacao\n"),
    ("backup/antigo.txt", "arquivo velho guardado\n"),
];

fn montar(area: &Path) {
    let _ = std::fs::remove_dir_all(area);
    std::fs::create_dir_all(area).expect("criar area");
    for (rel, txt) in CENARIO {
        let alvo = area.join(rel);
        if let Some(p) = alvo.parent() {
            std::fs::create_dir_all(p).expect("criar pai");
        }
        std::fs::write(&alvo, txt).expect("escrever cenario");
    }
}

fn conteudo_original(rel: &str) -> &'static str {
    CENARIO
        .iter()
        .find(|(r, _)| *r == rel)
        .map(|(_, c)| *c)
        .expect("arquivo do cenario")
}

// ---------------------------------------------------------------------------
// o que se confere
// ---------------------------------------------------------------------------

/// A pergunta "o disco ficou como deveria?", uma variante por forma de efeito.
///
/// Declarativo em vez de um `fn` por tarefa: 45 ponteiros de função seriam 45
/// lugares para o mesmo erro de digitação se esconder.
enum Conf {
    /// A pasta passou a existir.
    Pasta(&'static str),
    /// O arquivo existe e o conteúdo contém o trecho pedido. Conferir só a
    /// existência deixaria passar um arquivo vazio, que é o efeito NÃO acontecendo.
    Arquivo(&'static str, &'static str),
    /// O destino tem os bytes da origem, e a origem continua lá. A segunda metade é
    /// o que separa copiar de mover.
    Copia { origem: &'static str, destino: &'static str },
    /// O destino tem os bytes da origem, e a origem sumiu.
    Movido { origem: &'static str, destino: &'static str },
    /// O arquivo sumiu — e o resto do cenário continua inteiro. A segunda metade
    /// existe porque `apagar` errando de alvo é um efeito diferente de `apagar`
    /// não acontecendo, e sem ela os dois pontuariam igual.
    Sumiu(&'static str),
    /// A SAÍDA da ferramenta contém o trecho.
    ///
    /// Para as que só leem, "o estado do disco depois" é o disco intacto — o que
    /// pode estar errado é a OBSERVAÇÃO. E foi exatamente aí que o bug do
    /// `resolver_leitura` morava: chamada certa, disco certo, resposta errada.
    Saida(&'static str),
    /// A saída contém o tamanho em bytes do arquivo do cenário. Mais duro que
    /// procurar o nome: `info_arquivo` que ecoe o caminho e erre o arquivo passa
    /// na checagem de nome e falha nesta.
    SaidaTamanho(&'static str),
    /// Não dá para medir com segurança. O motivo vai impresso no relatório.
    NaoMedido(&'static str),
}

/// Nada aqui toca o disco: recebe a área DEPOIS da execução e a saída da ferramenta.
fn avaliar(c: &Conf, area: &Path, saida: &str) -> Result<(), String> {
    match c {
        Conf::Pasta(rel) => {
            let p = area.join(rel);
            if p.is_dir() {
                Ok(())
            } else {
                Err(format!("{rel} nao e uma pasta que exista"))
            }
        }
        Conf::Arquivo(rel, trecho) => {
            let p = area.join(rel);
            let lido = std::fs::read_to_string(&p).map_err(|e| format!("{rel}: {e}"))?;
            if lido.to_lowercase().contains(&trecho.to_lowercase()) {
                Ok(())
            } else {
                Err(format!("{rel} existe mas nao contem {trecho:?}: {:?}", recorte(&lido)))
            }
        }
        Conf::Copia { origem, destino } => {
            let esperado = conteudo_original(origem);
            let d = std::fs::read_to_string(area.join(destino))
                .map_err(|e| format!("destino {destino}: {e}"))?;
            if d != esperado {
                return Err(format!("{destino} nao tem os bytes de {origem}: {:?}", recorte(&d)));
            }
            if !area.join(origem).exists() {
                return Err(format!("copiou mas a origem {origem} sumiu — isso e mover"));
            }
            Ok(())
        }
        Conf::Movido { origem, destino } => {
            let esperado = conteudo_original(origem);
            let d = std::fs::read_to_string(area.join(destino))
                .map_err(|e| format!("destino {destino}: {e}"))?;
            if d != esperado {
                return Err(format!("{destino} nao tem os bytes de {origem}: {:?}", recorte(&d)));
            }
            if area.join(origem).exists() {
                return Err(format!("copiou em vez de mover: {origem} continua la"));
            }
            Ok(())
        }
        Conf::Sumiu(rel) => {
            if area.join(rel).exists() {
                return Err(format!("{rel} continua no disco"));
            }
            for (outro, _) in CENARIO {
                if outro != rel && !area.join(outro).exists() {
                    return Err(format!("apagou {rel} e levou {outro} junto"));
                }
            }
            Ok(())
        }
        Conf::Saida(trecho) => {
            if saida.to_lowercase().contains(&trecho.to_lowercase()) {
                Ok(())
            } else {
                Err(format!("a saida nao contem {trecho:?}: {:?}", recorte(saida)))
            }
        }
        Conf::SaidaTamanho(rel) => {
            let n = conteudo_original(rel).len();
            if saida.contains(&format!("{n} bytes")) {
                Ok(())
            } else {
                Err(format!("a saida nao diz \"{n} bytes\": {:?}", recorte(saida)))
            }
        }
        Conf::NaoMedido(m) => Err((*m).to_string()),
    }
}

fn recorte(s: &str) -> String {
    s.chars().take(90).collect::<String>().replace('\n', "\\n")
}

// ---------------------------------------------------------------------------
// as tarefas
// ---------------------------------------------------------------------------

struct Tarefa {
    pedido: &'static str,
    /// A ferramenta certa. Vazio = nenhuma serve (ver `editar`).
    ferramenta: &'static str,
    /// O gabarito de argumentos, e a regra é UMA, aplicada sem exceção: entra todo
    /// argumento cujo valor aparece **literalmente** no pedido.
    ///
    /// A regra existe porque a primeira versão não a tinha. Eu havia deixado o
    /// `texto` de fora em duas das cinco frases de `escrever_arquivo` "porque é texto
    /// livre" — e o resultado foi a Teka emitir `texto:"lembrete"` para "escreve
    /// lembrete de reuniao", cair na coluna **argumento certo, sem efeito** e virar o
    /// único caso do achado. Um gabarito frouxo fabrica o achado que a sonda foi
    /// escrita para procurar. As cinco frases citam o texto entre palavras exatas,
    /// então as cinco comparam `texto`.
    ///
    /// O `raiz` de `procurar_arquivo` continua fora: é opcional, e omiti-lo cai em
    /// `Politica::raiz_ou_atual`, que é a área. Ausência é a resposta certa ali.
    args: &'static [(&'static str, &'static str)],
    confere: Conf,
}

/// Frases NOVAS. Nenhuma sai de `dados/frases_teste.txt` — copiar de lá contaminaria
/// o benchmark — e nenhuma é molde de treino verbatim. A consequência honesta: o
/// número de "ferramenta errada" daqui NÃO é comparável com o do benchmark. Ele é
/// só o piso desta amostra.
const TAREFAS: &[Tarefa] = &[
    // ── criar_pasta ─────────────────────────────────────────────────────────
    Tarefa {
        pedido: "cria a pasta recibos",
        ferramenta: "criar_pasta",
        args: &[("caminho", "recibos")],
        confere: Conf::Pasta("recibos"),
    },
    Tarefa {
        pedido: "faz uma pasta chamada fotos",
        ferramenta: "criar_pasta",
        args: &[("caminho", "fotos")],
        confere: Conf::Pasta("fotos"),
    },
    Tarefa {
        pedido: "preciso de um diretorio chamado planilhas",
        ferramenta: "criar_pasta",
        args: &[("caminho", "planilhas")],
        confere: Conf::Pasta("planilhas"),
    },
    Tarefa {
        pedido: "monta a pasta arquivo_morto pra mim",
        ferramenta: "criar_pasta",
        args: &[("caminho", "arquivo_morto")],
        confere: Conf::Pasta("arquivo_morto"),
    },
    // ── escrever_arquivo ────────────────────────────────────────────────────
    //
    // A mais fraca do registro por construção: duas obrigatórias e o segundo
    // argumento é texto livre. Aqui o `texto` só entra na comparação quando o pedido
    // o cita entre palavras exatas.
    Tarefa {
        pedido: "grava o texto comprar cafe em compras.txt",
        ferramenta: "escrever_arquivo",
        args: &[("caminho", "compras.txt"), ("texto", "comprar cafe")],
        confere: Conf::Arquivo("compras.txt", "comprar cafe"),
    },
    Tarefa {
        pedido: "escreve lembrete de reuniao no arquivo aviso.txt",
        ferramenta: "escrever_arquivo",
        args: &[("caminho", "aviso.txt"), ("texto", "lembrete de reuniao")],
        confere: Conf::Arquivo("aviso.txt", "lembrete de reuniao"),
    },
    Tarefa {
        pedido: "salva anotacao rapida dentro de rascunho.txt",
        ferramenta: "escrever_arquivo",
        args: &[("caminho", "rascunho.txt"), ("texto", "anotacao rapida")],
        confere: Conf::Arquivo("rascunho.txt", "anotacao rapida"),
    },
    Tarefa {
        pedido: "poe a frase teste de gravacao em saida.txt",
        ferramenta: "escrever_arquivo",
        args: &[("caminho", "saida.txt"), ("texto", "teste de gravacao")],
        confere: Conf::Arquivo("saida.txt", "teste de gravacao"),
    },
    Tarefa {
        pedido: "anota dia bom no arquivo diario.txt",
        ferramenta: "escrever_arquivo",
        args: &[("caminho", "diario.txt"), ("texto", "dia bom")],
        confere: Conf::Arquivo("diario.txt", "dia bom"),
    },
    // ── copiar_arquivo ──────────────────────────────────────────────────────
    Tarefa {
        pedido: "copia notas.md para lista.md",
        ferramenta: "copiar_arquivo",
        args: &[("origem", "notas.md"), ("destino", "lista.md")],
        confere: Conf::Copia { origem: "notas.md", destino: "lista.md" },
    },
    Tarefa {
        // Destino que é PASTA: a forma que em 12/09 deu zero em 36 tentativas.
        pedido: "copia notas.md para a pasta backup",
        ferramenta: "copiar_arquivo",
        args: &[("origem", "notas.md"), ("destino", "backup")],
        confere: Conf::Copia { origem: "notas.md", destino: "backup/notas.md" },
    },
    Tarefa {
        pedido: "faz uma copia de config.json em config_velho.json",
        ferramenta: "copiar_arquivo",
        args: &[("origem", "config.json"), ("destino", "config_velho.json")],
        confere: Conf::Copia { origem: "config.json", destino: "config_velho.json" },
    },
    Tarefa {
        pedido: "duplica relatorio.txt como relatorio_copia.txt",
        ferramenta: "copiar_arquivo",
        args: &[("origem", "relatorio.txt"), ("destino", "relatorio_copia.txt")],
        confere: Conf::Copia { origem: "relatorio.txt", destino: "relatorio_copia.txt" },
    },
    Tarefa {
        pedido: "deposita leia_me.txt na pasta docs",
        ferramenta: "copiar_arquivo",
        args: &[("origem", "leia_me.txt"), ("destino", "docs")],
        confere: Conf::Copia { origem: "leia_me.txt", destino: "docs/leia_me.txt" },
    },
    // ── mover_arquivo ───────────────────────────────────────────────────────
    Tarefa {
        pedido: "move agenda.txt para a pasta backup",
        ferramenta: "mover_arquivo",
        args: &[("origem", "agenda.txt"), ("destino", "backup")],
        confere: Conf::Movido { origem: "agenda.txt", destino: "backup/agenda.txt" },
    },
    Tarefa {
        // Destino INÉDITO na segunda posição: a forma que faz a cabeça de presença
        // dizer "não há segundo argumento" e desabar para `apagar_arquivo`. Está
        // aqui de propósito — se desabar, o efeito é o arquivo SUMIR.
        pedido: "renomeia relatorio.txt para balanco.txt",
        ferramenta: "mover_arquivo",
        args: &[("origem", "relatorio.txt"), ("destino", "balanco.txt")],
        confere: Conf::Movido { origem: "relatorio.txt", destino: "balanco.txt" },
    },
    Tarefa {
        pedido: "move config.json para a pasta docs",
        ferramenta: "mover_arquivo",
        args: &[("origem", "config.json"), ("destino", "docs")],
        confere: Conf::Movido { origem: "config.json", destino: "docs/config.json" },
    },
    Tarefa {
        pedido: "transfere leia_me.txt para a pasta backup",
        ferramenta: "mover_arquivo",
        args: &[("origem", "leia_me.txt"), ("destino", "backup")],
        confere: Conf::Movido { origem: "leia_me.txt", destino: "backup/leia_me.txt" },
    },
    Tarefa {
        pedido: "muda o nome de notas.md para lembretes.md",
        ferramenta: "mover_arquivo",
        args: &[("origem", "notas.md"), ("destino", "lembretes.md")],
        confere: Conf::Movido { origem: "notas.md", destino: "lembretes.md" },
    },
    // ── apagar_arquivo ──────────────────────────────────────────────────────
    Tarefa {
        pedido: "apaga o arquivo agenda.txt",
        ferramenta: "apagar_arquivo",
        args: &[("caminho", "agenda.txt")],
        confere: Conf::Sumiu("agenda.txt"),
    },
    Tarefa {
        pedido: "remove leia_me.txt",
        ferramenta: "apagar_arquivo",
        args: &[("caminho", "leia_me.txt")],
        confere: Conf::Sumiu("leia_me.txt"),
    },
    Tarefa {
        pedido: "exclui o arquivo relatorio.txt",
        ferramenta: "apagar_arquivo",
        args: &[("caminho", "relatorio.txt")],
        confere: Conf::Sumiu("relatorio.txt"),
    },
    Tarefa {
        pedido: "joga fora o config.json",
        ferramenta: "apagar_arquivo",
        args: &[("caminho", "config.json")],
        confere: Conf::Sumiu("config.json"),
    },
    // ── ler_arquivo ─────────────────────────────────────────────────────────
    //
    // O grupo que deu nome à sonda. O bug de 0/10 era aqui, e a régua não o viu
    // porque a chamada estava correta.
    Tarefa {
        pedido: "quero ver o que tem dentro de notas.md",
        ferramenta: "ler_arquivo",
        args: &[("caminho", "notas.md")],
        confere: Conf::Saida("leite"),
    },
    Tarefa {
        pedido: "mostra o conteudo de config.json",
        ferramenta: "ler_arquivo",
        args: &[("caminho", "config.json")],
        confere: Conf::Saida("escuro"),
    },
    Tarefa {
        pedido: "le o arquivo relatorio.txt pra mim",
        ferramenta: "ler_arquivo",
        args: &[("caminho", "relatorio.txt")],
        confere: Conf::Saida("vendas subiram"),
    },
    Tarefa {
        pedido: "despeja na tela o que tem em leia_me.txt",
        ferramenta: "ler_arquivo",
        args: &[("caminho", "leia_me.txt")],
        confere: Conf::Saida("teka le isto aqui"),
    },
    Tarefa {
        // Caminho com subpasta: se a junção com a raiz estiver errada, é AQUI que
        // aparece primeiro.
        pedido: "exibe o miolo de docs/manual.txt",
        ferramenta: "ler_arquivo",
        args: &[("caminho", "docs/manual.txt")],
        confere: Conf::Saida("secao 1 instalacao"),
    },
    // ── listar_pasta ────────────────────────────────────────────────────────
    Tarefa {
        pedido: "mostra os arquivos da pasta docs",
        ferramenta: "listar_pasta",
        args: &[("caminho", "docs")],
        confere: Conf::Saida("manual.txt"),
    },
    Tarefa {
        pedido: "me diz o que tem guardado na pasta backup",
        ferramenta: "listar_pasta",
        args: &[("caminho", "backup")],
        confere: Conf::Saida("antigo.txt"),
    },
    Tarefa {
        pedido: "percorre o diretorio docs e me diz o que achou",
        ferramenta: "listar_pasta",
        args: &[("caminho", "docs")],
        confere: Conf::Saida("manual.txt"),
    },
    Tarefa {
        pedido: "lista o conteudo do diretorio backup",
        ferramenta: "listar_pasta",
        args: &[("caminho", "backup")],
        confere: Conf::Saida("antigo.txt"),
    },
    // ── procurar_arquivo ────────────────────────────────────────────────────
    //
    // `raiz` é opcional; omitida, a primitiva cai em `Politica::raiz_ou_atual`, que
    // é a área. Então o gabarito compara só `nome`.
    Tarefa {
        pedido: "procura um arquivo chamado manual",
        ferramenta: "procurar_arquivo",
        args: &[("nome", "manual")],
        confere: Conf::Saida("manual.txt"),
    },
    Tarefa {
        pedido: "acha o arquivo relatorio no computador",
        ferramenta: "procurar_arquivo",
        args: &[("nome", "relatorio")],
        confere: Conf::Saida("relatorio.txt"),
    },
    Tarefa {
        pedido: "quero localizar um arquivo de nome config",
        ferramenta: "procurar_arquivo",
        args: &[("nome", "config")],
        confere: Conf::Saida("config.json"),
    },
    Tarefa {
        pedido: "vasculha o computador atras de antigo",
        ferramenta: "procurar_arquivo",
        args: &[("nome", "antigo")],
        confere: Conf::Saida("antigo.txt"),
    },
    // ── info_arquivo ────────────────────────────────────────────────────────
    Tarefa {
        pedido: "qual o tamanho de notas.md",
        ferramenta: "info_arquivo",
        args: &[("caminho", "notas.md")],
        confere: Conf::SaidaTamanho("notas.md"),
    },
    Tarefa {
        pedido: "me diz a data de modificacao de config.json",
        ferramenta: "info_arquivo",
        args: &[("caminho", "config.json")],
        confere: Conf::SaidaTamanho("config.json"),
    },
    Tarefa {
        pedido: "quantos bytes tem relatorio.txt",
        ferramenta: "info_arquivo",
        args: &[("caminho", "relatorio.txt")],
        confere: Conf::SaidaTamanho("relatorio.txt"),
    },
    Tarefa {
        pedido: "detalhes do arquivo leia_me.txt",
        ferramenta: "info_arquivo",
        args: &[("caminho", "leia_me.txt")],
        confere: Conf::SaidaTamanho("leia_me.txt"),
    },
    // ── buscar_no_conteudo — ESCOLHA medida, EFEITO não ──────────────────────
    //
    // `Primitiva::Grep` vai por `pela_ponte`, que não recebe a `Politica` e ainda
    // sobe um processo com `ponte_auto::garantir`. Executar aqui quebraria as duas
    // travas de uma vez.
    Tarefa {
        pedido: "procura a palavra vendas dentro dos arquivos",
        ferramenta: "buscar_no_conteudo",
        args: &[("padrao", "vendas")],
        confere: Conf::NaoMedido("executa pela ponte do Harness: ignora a Politica e sobe processo"),
    },
    Tarefa {
        pedido: "quais arquivos mencionam escuro",
        ferramenta: "buscar_no_conteudo",
        args: &[("padrao", "escuro")],
        confere: Conf::NaoMedido("executa pela ponte do Harness: ignora a Politica e sobe processo"),
    },
    Tarefa {
        pedido: "busca onde aparece o trecho pao nos arquivos",
        ferramenta: "buscar_no_conteudo",
        args: &[("padrao", "pao")],
        confere: Conf::NaoMedido("executa pela ponte do Harness: ignora a Politica e sobe processo"),
    },
    // ── editar — não existe ferramenta ──────────────────────────────────────
    //
    // `Registro::padrao()` tem 22 e nenhuma é `editar`. A Teka não pode emitir a
    // chamada, então não há efeito a medir — só onde o pedido cai.
    Tarefa {
        pedido: "troca a palavra pao por biscoito em notas.md",
        ferramenta: "",
        args: &[],
        confere: Conf::NaoMedido("nao ha ferramenta `editar` no registro de 22"),
    },
    Tarefa {
        pedido: "substitui escuro por claro no config.json",
        ferramenta: "",
        args: &[],
        confere: Conf::NaoMedido("nao ha ferramenta `editar` no registro de 22"),
    },
    Tarefa {
        pedido: "no relatorio.txt troca vendas por receitas",
        ferramenta: "",
        args: &[],
        confere: Conf::NaoMedido("nao ha ferramenta `editar` no registro de 22"),
    },
];

// ---------------------------------------------------------------------------
// a trava que a Politica não dá
// ---------------------------------------------------------------------------

/// Todo argumento de tipo caminho cai dentro da área?
///
/// A raiz da política confina ESCRITA e não confina leitura — está documentado em
/// `Politica::resolver_leitura`, e é decisão de política, não descuido. Para uma
/// sonda que executa o que um modelo escolheu, isso não basta: um
/// `procurar_arquivo(raiz="C:\\")` varre o disco do John com profundidade 8.
///
/// Recusar é o lado seguro do erro. Uma recusa custa uma tarefa contada como efeito
/// não acontecido (com o motivo impresso); um falso negativo custa minutos de disco
/// alheio.
fn dentro_da_area(reg: &Registro, ferramenta: usize, args: &[(String, String)], area: &Path) -> Result<(), String> {
    let raiz = normalizar(area);
    for p in &reg.ferramentas[ferramenta].params {
        if p.tipo != TipoParam::Caminho {
            continue;
        }
        let Some((_, v)) = args.iter().find(|(k, _)| *k == p.nome) else {
            continue;
        };
        if v.trim().is_empty() {
            continue;
        }
        let bruto = PathBuf::from(v);
        let absoluto = if bruto.is_absolute() { bruto } else { area.join(bruto) };
        if !normalizar(&absoluto).starts_with(&raiz) {
            return Err(format!("a sonda recusou executar: {}={v:?} aponta para fora da area", p.nome));
        }
    }
    Ok(())
}

/// Compara argumento por argumento, e só os que o gabarito lista.
///
/// Normaliza a barra e a caixa: `docs/manual.txt` e `docs\manual.txt` são o mesmo
/// caminho, e tratá-los como diferentes inventaria erro que não existe.
fn args_batem(esperado: &[(&str, &str)], vindo: &[(String, String)]) -> bool {
    let norm = |s: &str| s.trim().to_lowercase().replace('\\', "/");
    esperado.iter().all(|(k, v)| {
        vindo
            .iter()
            .find(|(kv, _)| kv == k)
            .is_some_and(|(_, vv)| norm(vv) == norm(v))
    })
}

// ---------------------------------------------------------------------------
// o controle do instrumento
// ---------------------------------------------------------------------------

/// Executa o GABARITO — a chamada que a tarefa diz ser a certa — e confere.
///
/// Sem isto a sonda não tem direito de apontar para a Teka. Uma tarefa cujo gabarito
/// não produz o efeito não está medindo a Teka: está medindo um erro meu, e
/// contaria como "ferramenta certa, efeito não aconteceu" — exatamente a coluna do
/// achado. Seria a sonda fabricando o próprio resultado.
///
/// Todo gabarito aqui é uma chamada COMPLETA: os argumentos obrigatórios de todas as
/// nove ferramentas medidas são valores que o pedido cita literalmente, então o
/// gabarito não precisa de nada que eu tenha inventado.
fn controle(reg: &Registro, pol: &Politica, area: &Path) -> Vec<(&'static str, String)> {
    let mut falhas = Vec::new();
    for t in TAREFAS {
        if matches!(t.confere, Conf::NaoMedido(_)) {
            continue;
        }
        montar(area);
        let Some(i) = reg.indice(t.ferramenta) else {
            falhas.push((t.pedido, format!("{} nao esta no registro", t.ferramenta)));
            continue;
        };
        let c = teka::tools::Chamada {
            ferramenta: i,
            args: t.args.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
        };
        let saida = match reg.executar(&c, pol) {
            Ok(s) => s,
            Err(e) => format!("<falhou> {e}"),
        };
        if let Err(motivo) = avaliar(&t.confere, area, &saida) {
            falhas.push((t.pedido, format!("{}  ->  {motivo}", c.texto(reg))));
        }
    }
    falhas
}

// ---------------------------------------------------------------------------

/// Em que caixa a tentativa caiu.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Caixa {
    Efeito,
    /// Ferramenta certa, argumento diferente do gabarito. A régua JÁ vê isto.
    SemEfeitoArgumento,
    /// Ferramenta certa, argumento certo, e o disco mesmo assim errado. O buraco.
    SemEfeitoExecucao,
    FerramentaErrada,
    NaoMedido,
}

impl Caixa {
    fn rotulo(self) -> &'static str {
        match self {
            Caixa::Efeito => "efeito aconteceu",
            Caixa::SemEfeitoArgumento => "ferr. certa, arg. errado",
            Caixa::SemEfeitoExecucao => "ferr. e arg. certos, SEM EFEITO",
            Caixa::FerramentaErrada => "ferramenta errada",
            Caixa::NaoMedido => "nao medido",
        }
    }
}

fn main() {
    let molde = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "modelos/teka_fechar_s%.bin".into());
    let sementes: Vec<u32> = (19..=30).collect();

    // DOIS fios, não `auto`: há um treino ocupando 10 threads nesta máquina, e uma
    // sonda que briga por CPU com o treino mede escalonador, não modelo.
    let ops = Paralelo::new(2);
    let patcher = PorPalavra::default();

    let area = std::env::temp_dir().join(format!("teka_sonda_efeito_{}", std::process::id()));
    // `real_sem_processos`, NUNCA `real_em`: a segunda liga `processos: true`.
    let pol = Politica::real_sem_processos(&area);
    assert!(!pol.processos, "a politica desta sonda nao pode lancar processo");

    // O INSTRUMENTO PRIMEIRO. Se o gabarito não produz o efeito, o problema é meu, e
    // qualquer número abaixo estaria medindo a sonda em vez da Teka.
    let reg = Registro::padrao();
    let falhas = controle(&reg, &pol, &area);
    println!("\n  CONTROLE do instrumento: {} de {} gabaritos produzem o efeito",
        TAREFAS.iter().filter(|t| !matches!(t.confere, Conf::NaoMedido(_))).count() - falhas.len(),
        TAREFAS.iter().filter(|t| !matches!(t.confere, Conf::NaoMedido(_))).count());
    for (pedido, motivo) in &falhas {
        println!("    TAREFA QUEBRADA  {pedido}\n            {motivo}");
    }
    if !falhas.is_empty() {
        println!("\n  tarefa quebrada conta como erro MEU, nao da Teka -- conserte antes de ler o resto");
    }

    let mut total: BTreeMap<Caixa, usize> = BTreeMap::new();
    // por ferramenta esperada -> (efeito, ferr. certa sem efeito, ferr. errada)
    let mut por_ferramenta: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    // pedido -> (quantas sementes sem efeito com ferramenta certa, um exemplo)
    let mut buraco: BTreeMap<&str, (usize, String)> = BTreeMap::new();
    // pedido nao medido -> onde a escolha caiu
    let mut nao_medido: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut n_modelos = 0usize;

    for s in &sementes {
        let caminho = molde.replace('%', &s.to_string());
        let Ok(ag) = Agente::<f32>::carregar(std::path::Path::new(&caminho), Registro::padrao())
        else {
            continue;
        };
        n_modelos += 1;
        let mut cache = AgenteCache::new();

        for t in TAREFAS {
            // Cenário do ZERO por tarefa: uma tarefa não pode herdar o estrago da
            // anterior, senão o número depende da ORDEM da lista.
            montar(&area);

            let chamada = ag.responder(&ops, &patcher, t.pedido, &mut cache);
            let (nome, texto_chamada) = match &chamada {
                Ok(c) => (ag.registro.ferramentas[c.ferramenta].nome.clone(), c.texto(&ag.registro)),
                Err(e) => ("<erro>".to_string(), e.clone()),
            };

            // Fora de medição: registra ONDE caiu e segue. Não executa.
            if matches!(t.confere, Conf::NaoMedido(_)) {
                *nao_medido.entry(t.pedido).or_default().entry(nome).or_default() += 1;
                *total.entry(Caixa::NaoMedido).or_default() += 1;
                continue;
            }

            let ferramenta_certa = nome == t.ferramenta;
            let entrada = por_ferramenta.entry(t.ferramenta).or_default();
            if !ferramenta_certa {
                entrada.2 += 1;
                *total.entry(Caixa::FerramentaErrada).or_default() += 1;
                continue;
            }

            let c = chamada.expect("ferramenta certa implica chamada");
            // A trava própria, antes de executar. Ver `dentro_da_area`.
            let saida = match dentro_da_area(&ag.registro, c.ferramenta, &c.args, &area) {
                Ok(()) => match ag.registro.executar(&c, &pol) {
                    Ok(s) => s,
                    // Erro de execução NÃO é exceção: é exatamente o caso que a régua
                    // não vê. Vira string e a checagem de disco decide.
                    Err(e) => format!("<falhou> {e}"),
                },
                Err(e) => format!("<recusado> {e}"),
            };

            match avaliar(&t.confere, &area, &saida) {
                Ok(()) => {
                    entrada.0 += 1;
                    *total.entry(Caixa::Efeito).or_default() += 1;
                }
                Err(motivo) => {
                    entrada.1 += 1;
                    let caixa = if args_batem(t.args, &c.args) {
                        Caixa::SemEfeitoExecucao
                    } else {
                        Caixa::SemEfeitoArgumento
                    };
                    *total.entry(caixa).or_default() += 1;
                    let e = buraco.entry(t.pedido).or_insert_with(|| (0, String::new()));
                    e.0 += 1;
                    if e.1.is_empty() {
                        e.1 = format!("{texto_chamada}  ->  {motivo}");
                    }
                }
            }
        }
    }

    let _ = std::fs::remove_dir_all(&area);

    if n_modelos == 0 {
        eprintln!("nenhum modelo casou com {molde}");
        return;
    }

    let medidas = TAREFAS
        .iter()
        .filter(|t| !matches!(t.confere, Conf::NaoMedido(_)))
        .count();
    println!("\n  {n_modelos} modelo(s) x {} tarefas ({medidas} executadas, {} fora)",
        TAREFAS.len(), TAREFAS.len() - medidas);
    println!("  area: {}   politica: real, raiz confinada, processos=false\n", area.display());

    let n = (n_modelos * medidas) as f64;
    let g = |c: Caixa| *total.get(&c).unwrap_or(&0);
    let sem_efeito = g(Caixa::SemEfeitoArgumento) + g(Caixa::SemEfeitoExecucao);
    println!("  AS TRES CATEGORIAS, sobre {} tentativas executadas\n", n as usize);
    println!("  {:<34} {:>6} {:>7}", "", "n", "%");
    for (r, v) in [
        ("efeito aconteceu", g(Caixa::Efeito)),
        ("ferramenta certa, SEM efeito", sem_efeito),
        ("ferramenta errada", g(Caixa::FerramentaErrada)),
    ] {
        println!("  {r:<34} {v:>6} {:>6.1}%", 100.0 * v as f64 / n);
    }

    println!("\n  a coluna do meio, aberta -- as causas sao diferentes:");
    for c in [Caixa::SemEfeitoArgumento, Caixa::SemEfeitoExecucao] {
        println!("  {:<34} {:>6} {:>6.1}%", c.rotulo(), g(c), 100.0 * g(c) as f64 / n);
    }
    println!("  (a segunda linha e a que a regua do projeto e CEGA para)");

    println!("\n  por ferramenta esperada:");
    println!("  {:<20} {:>8} {:>10} {:>10}", "", "efeito", "sem efeito", "ferr.errada");
    let mut v: Vec<_> = por_ferramenta.iter().collect();
    v.sort_by_key(|(_, (ok, _, _))| std::cmp::Reverse(*ok));
    for (f, (ok, sem, err)) in v {
        let tot = ok + sem + err;
        println!("  {f:<20} {ok:>4}/{tot:<3} {:>3.0}% {sem:>9} {err:>10}", 100.0 * *ok as f64 / tot as f64);
    }

    println!("\n  O ACHADO -- ferramenta certa e o efeito nao aconteceu, de {n_modelos} modelo(s):");
    let mut v: Vec<_> = buraco.iter().collect();
    v.sort_by_key(|(_, (c, _))| std::cmp::Reverse(*c));
    if v.is_empty() {
        println!("    (nenhuma -- a tese esta ERRADA e a regua da chamada basta)");
    }
    for (pedido, (c, exemplo)) in v {
        println!("    {c:>3}/{n_modelos}  {pedido}");
        println!("            {exemplo}");
    }

    println!("\n  FORA DA MEDICAO (nao e o mesmo que ter falhado):");
    for t in TAREFAS {
        if let Conf::NaoMedido(motivo) = t.confere {
            let onde = nao_medido.get(t.pedido).cloned().unwrap_or_default();
            let mut o: Vec<_> = onde.into_iter().collect();
            o.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            let resumo: Vec<String> = o.iter().map(|(n, c)| format!("{n} x{c}")).collect();
            println!("    {}", t.pedido);
            println!("      motivo: {motivo}");
            println!("      caiu em: {}", resumo.join(", "));
        }
    }
}
