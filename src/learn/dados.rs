//! Gerador de exemplos supervisionados: pedido em português → (ferramenta, spans).
//!
//! ## Por que sintético
//!
//! A Teka precisa de milhares de pares (pedido, ação) e ninguém vai escrever isso à
//! mão. Templates com valores sorteados dão cobertura ampla de graça, e é o
//! bootstrap clássico antes do RL sobre uso real (fase 4) — aprender seleção de
//! ferramenta por reforço puro, do zero, levaria uma eternidade.
//!
//! Os pedidos são escritos **como alguém fala**: minúsculas, sem pontuação, com
//! gírias e formas truncadas. É o que vai sair do Whisper um dia, e é o que mata a
//! dependência de palavras-gatilho — o modelo tem que aprender que *"da uma olhada
//! em"*, *"o que tem em"* e *"lista"* pedem a mesma coisa.
//!
//! ## O gerador se verifica
//!
//! Cada exemplo só é aceito se o intervalo de patches, recortado de volta, reproduz
//! **exatamente** o valor pretendido. Um span mal calculado ensinaria a cabeça de
//! ponteiro a apontar para o lugar errado — e nada no treino acusaria.

use crate::model::heads::{Alvo, MAX_SLOTS};
use crate::model::patcher::Patcher;
use crate::rng::Rng;
use crate::tools::Registro;

#[derive(Clone, Debug)]
pub struct Exemplo {
    pub pedido: String,
    pub ferramenta: usize,
    /// `(slot, (byte_inicial, byte_final_exclusivo))`
    pub args: Vec<(usize, (usize, usize))>,
    /// Identidade da frase que gerou este exemplo: `(molde, frase)`.
    ///
    /// Existe por causa de um erro de medicao real. Dividir treino/validacao por
    /// EXEMPLO da 100% de acuracia e nao significa nada: os dois lados saem das
    /// mesmas ~60 frases, entao o modelo pode decorar as frases em vez de aprender
    /// a tarefa. Medido: 100% na validacao por exemplo, 40% em frases escritas na
    /// mao. Dividir por FRASE mede generalizacao de verdade.
    pub frase: (usize, usize),
}

impl Exemplo {
    /// Converte os spans de byte em spans de patch, validando por ida e volta.
    pub fn alvo<P: Patcher + ?Sized>(&self, patcher: &P) -> Option<Alvo> {
        let bytes = self.pedido.as_bytes();
        let mut fins = Vec::new();
        patcher.fronteiras(bytes, &mut fins);
        if fins.is_empty() {
            return None;
        }
        let patch_de = |x: usize| fins.iter().position(|&f| x < f);

        let mut spans = vec![None; MAX_SLOTS];
        let mut bytes_alvo = vec![None; MAX_SLOTS];
        for &(slot, (ini, fim)) in &self.args {
            if slot >= MAX_SLOTS || fim <= ini || fim > bytes.len() {
                return None;
            }
            let pi = patch_de(ini)?;
            let pf = patch_de(fim - 1)?;
            let b0 = if pi == 0 { 0 } else { fins[pi - 1] };
            let b1 = fins[pf];
            // O TREINO nao pode ser mais estrito que a INFERENCIA.
            //
            // Isto comparava o patch inteiro (so aparando espaco) com o valor. Com a
            // pontuacao final que a variacao de superficie adiciona, o patch vira
            // "dados." contra o valor "dados" e o exemplo era descartado — em
            // silencio, no `if ex.alvo(patcher).is_some()` de `gerar`.
            //
            // Medido: 66 de 8.133 exemplos pontuados tinham o argumento colado na
            // pontuacao. Devia ser ~8%. Eu gerava o caso e jogava fora exatamente
            // ele, e o modelo treinado mostrava a marca disso:
            //
            //     "Lista dados"   -> listar_pasta      "le notas.md"   -> ler_arquivo
            //     "Lista dados."  -> perguntar         "le notas.md."  -> memoria
            //
            // E o caso e comum de verdade: 21% do que uma pessoa digita termina em
            // "?", e a camada de contexto produz frases que acabam no proprio valor.
            //
            // `recortar` e a funcao que a inferencia usa para transformar os bytes
            // apontados de volta em texto — ela ja encaixa na palavra e apara
            // pontuacao. Se ELA reproduz o valor, o modelo consegue produzir este
            // exemplo, e nao ha motivo para esconde-lo do treino.
            let _ = (b0, b1);
            if crate::model::agente::recortar(bytes, ini, fim - 1) != self.pedido[ini..fim] {
                return None; // nem a inferencia reproduziria: descarta
            }
            spans[slot] = Some((pi, pf));
            // O nível fino aponta para os bytes exatos — sem o espaço que o patcher
            // gruda na frente de cada patch e sem o resto da palavra que o teto
            // eventualmente partiu.
            bytes_alvo[slot] = Some((ini, fim - 1));
        }
        Some(Alvo {
            ferramenta: self.ferramenta,
            spans,
            bytes: bytes_alvo,
            peso: 1.0,
            apenas_intencao: false,
            alvo_valor: None,
            // Supervisionado: o critico aprende a prever a PROPRIA correcao. Ver
            // `Alvo::auto_critico` — a cabeca existia e nunca era alimentada aqui.
            auto_critico: true,
        })
    }
}

/// Frases por ferramenta. `{0}` e `{1}` são os parâmetros na ordem do registro.
struct Molde {
    ferramenta: &'static str,
    frases: &'static [&'static str],
}

const MOLDES: &[Molde] = &[
    // ── buscar_web ──────────────────────────────────────────────────────────
    //
    // O dominio ("internet", "web", "online") esta em quase toda frase, e nao por
    // enfeite: sem ele "procura {0}" e identico ao molde de `procurar_arquivo`, e as
    // duas ferramentas colidiriam na cabeca de intencao.
    Molde {
        ferramenta: "buscar_web",
        frases: &[
            "me sugere alguma coisa sobre {0}",
            "que opcoes existem para {0}",
            "quais as recomendacoes para {0}",
            "me recomenda algo pra {0}",
            "o que da pra fazer a respeito de {0}",
            "que alternativas tem pra {0}",
            "me da umas ideias de {0}",
            "quais as melhores praticas de {0}",
            "que caminhos existem pra {0}",
            "me indica algo sobre {0}",
            "o que costumam recomendar pra {0}",
            "quais as opcoes de {0}",
            "pesquisa na internet sobre {0}",
            "busca na web {0}",
            "da uma pesquisada online sobre {0}",
            "procura na internet {0}",
            "o que a internet diz sobre {0}",
            "pesquisa ai {0}",
            "consulta na web sobre {0}",
            "quero saber sobre {0}",
            "me explica {0}",
            "pesquisa pra mim {0}",
            "busca informacao sobre {0}",
            "da um google em {0}",
            "descobre na internet {0}",
            "levanta informacao sobre {0}",
            "o que voce acha na web sobre {0}",
            "pesquisa online {0}",
            "vai na internet e ve {0}",
            "acha na web {0}",
            "me traz o que tem sobre {0}",
            "consulta {0} na internet",
            "informacao sobre {0} por favor",
            "da uma olhada na web sobre {0}",
            "pesquisa o assunto {0} na web",
            "quero informacao de {0}",
            "busca online sobre {0}",
            "ve na internet {0}",
            "procura na web informacao sobre {0}",
            "me atualiza sobre {0}",
            "pesquisa e resume {0}",
            "o que se sabe sobre {0}",
        ],
    },
    // ── abrir_programa ──────────────────────────────────────────────────────
    Molde {
        ferramenta: "abrir_programa",
        frases: &[
            "abre o {0}",
            "inicia o {0}",
            "starta o {0}",
            "poe o {0} pra rodar",
            "abre o programa {0}",
            "chama o {0} ai",
            "quero abrir o {0}",
            "liga o {0}",
            "abre {0} pra mim",
            "sobe o {0}",
            "executa o programa {0}",
            "manda abrir o {0}",
            "roda o aplicativo {0}",
            "abre o app {0}",
            "preciso do {0} aberto",
            "inicializa o {0}",
            "poe o {0} na tela",
            "quero usar o {0}",
            "traz o {0} pra frente",
            "carrega o {0}",
            "abre a janela do {0}",
            "da um start no {0}",
            "coloca o {0} pra abrir",
            "faz abrir o {0}",
            "quero o {0} rodando",
            "dispara o {0}",
            "me abre o {0}",
            "consegue abrir o {0}",
            "vai la e abre o {0}",
            "abre o programa chamado {0}",
        ],
    },
    // ── copiar_arquivo ──────────────────────────────────────────────────────
    //
    // Duas obrigatorias. `escrever_arquivo` tem a mesma forma e e a ferramenta mais
    // fraca dela — das quatro formas naturais testadas, nenhuma acertou os dois
    // argumentos. Aqui os moldes deixam a ordem SEMPRE explicita ("de X para Y"),
    // que e a unica pista que separa origem de destino.
    Molde {
        ferramenta: "copiar_arquivo",
        frases: &[
            // DESTINO QUE E PASTA — a forma que faltava, e faltava inteira.
            //
            // Medido em 12/09 no benchmark de 329: ZERO acerto em 36 tentativas.
            // Todo molde aqui ensinava arquivo->arquivo, e o John pede
            // arquivo->PASTA. "pasta" no treino so existia com `listar_pasta` e
            // `criar_pasta`, entao ela via a palavra e ia para la, sempre.
            //
            // `valor_no_contexto` garante que o {1} depois de "pasta" venha de
            // PASTAS -- senao sairia "para a pasta notas.md".
            //
            // Os verbos canonicos (copia, move) sao inevitaveis: sao OS verbos. O
            // que nao se repete e a ESTRUTURA das frases do John -- conferido com
            // `checa_vazamento.py`, e `deposita`, `arquiva`, `transfere` e
            // `arrasta` estao livres.
            "copia {0} para a pasta {1}",
            "copia {0} pra pasta {1}",
            "deposita {0} na pasta {1}",
            "arquiva {0} na pasta {1}",
            "poe uma copia de {0} na pasta {1}",
            "copia {0} para dentro da pasta {1}",
            "quero {0} copiado na pasta {1}",
            "faz uma copia de {0} no diretorio {1}",
            "copia {0} para {1}",
            "faz uma copia de {0} em {1}",
            "duplica {0} como {1}",
            "copia o arquivo {0} para {1}",
            "quero uma copia de {0} em {1}",
            "clona {0} para {1}",
            "manda uma copia de {0} para {1}",
            "copia {0} e salva como {1}",
            "cria uma copia de {0} chamada {1}",
            "replica {0} em {1}",
            "copia de {0} para {1}",
            "faz backup de {0} em {1}",
            "copia o arquivo {0} la pra {1}",
            "gera uma copia de {0} como {1}",
            "duplica o arquivo {0} para {1}",
            "salva uma copia de {0} em {1}",
            "copia {0} pro {1}",
            "quero {0} copiado em {1}",
            "poe uma copia de {0} em {1}",
            "copia o conteudo de {0} para {1}",
            "faz {1} a partir de {0}",
            "copiar {0} destino {1}",
            "copia pra mim {0} em {1}",
            "backup de {0} para {1}",
            "cria {1} copiando {0}",
            "copia {0} ali em {1}",
            "me copia {0} para {1}",
            "consegue copiar {0} para {1}",
            "copia o de nome {0} para {1}",
            "faz uma segunda copia de {0} em {1}",
        ],
    },
    // ── mover_arquivo ───────────────────────────────────────────────────────
    Molde {
        ferramenta: "mover_arquivo",
        frases: &[
            // DESTINO QUE E PASTA — a forma que faltava, e faltava inteira.
            //
            // Medido em 12/09 no benchmark de 329: ZERO acerto em 60 tentativas.
            // Todo molde aqui ensinava arquivo->arquivo, e o John pede
            // arquivo->PASTA. "pasta" no treino so existia com `listar_pasta` e
            // `criar_pasta`, entao ela via a palavra e ia para la, sempre.
            //
            // `valor_no_contexto` garante que o {1} depois de "pasta" venha de
            // PASTAS -- senao sairia "para a pasta notas.md".
            //
            // Os verbos canonicos (copia, move) sao inevitaveis: sao OS verbos. O
            // que nao se repete e a ESTRUTURA das frases do John -- conferido com
            // `checa_vazamento.py`, e `deposita`, `arquiva`, `transfere` e
            // `arrasta` estao livres.
            "move {0} para a pasta {1}",
            "move {0} pra pasta {1}",
            "transfere {0} para a pasta {1}",
            "arrasta {0} para a pasta {1}",
            "tira {0} daqui e poe na pasta {1}",
            "move {0} para dentro da pasta {1}",
            "quero {0} na pasta {1}",
            "manda {0} para o diretorio {1}",
            "move {0} para {1}",
            "renomeia {0} para {1}",
            "tira {0} e poe em {1}",
            "move o arquivo {0} para {1}",
            "muda {0} de lugar para {1}",
            "transfere {0} para {1}",
            "leva {0} para {1}",
            "renomeia o arquivo {0} como {1}",
            "move de {0} para {1}",
            "quero {0} em {1}",
            "manda {0} para {1}",
            "muda o nome de {0} para {1}",
            "realoca {0} em {1}",
            "move o arquivo {0} la pra {1}",
            "passa {0} para {1}",
            "troca o nome de {0} por {1}",
            "move {0} pro {1}",
            "coloca {0} em {1}",
            "reposiciona {0} para {1}",
            "arrasta {0} para {1}",
            "mover {0} destino {1}",
            "me move {0} para {1}",
            "poe {0} dentro de {1}",
            "muda {0} para {1}",
            "consegue mover {0} para {1}",
            "leva o arquivo {0} ate {1}",
            "renomear {0} para {1}",
            "move o de nome {0} para {1}",
            "manda {0} la pra {1}",
            "faz {0} virar {1}",
        ],
    },
    // ── criar_pasta ─────────────────────────────────────────────────────────
    Molde {
        ferramenta: "criar_pasta",
        frases: &[
            "cria a pasta {0}",
            "faz uma pasta chamada {0}",
            "cria o diretorio {0}",
            "quero uma pasta {0}",
            "nova pasta {0}",
            "cria uma pasta {0} pra mim",
            "abre uma pasta nova chamada {0}",
            "monta a pasta {0}",
            "faz o diretorio {0}",
            "preciso de uma pasta {0}",
            "cria pasta {0}",
            "gera a pasta {0}",
            "poe uma pasta {0} ai",
            "cria um diretorio chamado {0}",
            "quero criar a pasta {0}",
            "faz uma pasta pra {0}",
            "adiciona a pasta {0}",
            "cria {0} como pasta",
            "novo diretorio {0}",
            "monta um diretorio {0}",
            "consegue criar a pasta {0}",
            "me cria a pasta {0}",
            "cria a pasta chamada {0}",
            "faz pasta {0}",
            "abre um diretorio {0}",
            "preciso da pasta {0} criada",
            "cria a pasta de nome {0}",
            "criar diretorio {0}",
            "quero um diretorio {0}",
            "faz uma nova pasta {0}",
        ],
    },
    // ── apagar_arquivo ──────────────────────────────────────────────────────
    //
    // A unica sem volta, e treinada de proposito: ferramenta que nunca aparece no
    // treino e ferramenta que ela nunca aprende a usar. O treino roda em container,
    // e fora dele a politica de sempre governa — sandbox por padrao, confinamento
    // de raiz, e o diario registrando ANTES de agir.
    Molde {
        ferramenta: "apagar_arquivo",
        frases: &[
            "apaga o {0}",
            "deleta {0}",
            "remove o arquivo {0}",
            "exclui {0}",
            "apaga o arquivo {0}",
            "quero apagar {0}",
            "manda {0} pro lixo",
            "some com {0}",
            "deleta o arquivo {0}",
            "tira {0} daqui",
            "elimina {0}",
            "apaga o arquivo de nome {0}",
            "remove {0}",
            "exclui o arquivo {0}",
            "pode apagar {0}",
            "descarta {0}",
            "joga fora o {0}",
            "apaga {0} pra mim",
            "quero {0} deletado",
            "deleta o arquivo {0} ai",
            "limpa o {0}",
            "remove o arquivo {0} de vez",
            "consegue apagar {0}",
            "me apaga o {0}",
            "faz sumir o {0}",
            "exclui o {0} ai",
            "apagar {0}",
            "deletar o arquivo {0}",
            "tira o arquivo {0}",
            "nao preciso mais de {0}",
        ],
    },
    // ── info_arquivo ────────────────────────────────────────────────────────
    Molde {
        ferramenta: "info_arquivo",
        frases: &[
            "qual o tamanho de {0}",
            "informacoes sobre {0}",
            "quando {0} foi modificado",
            "detalhes do arquivo {0}",
            "quanto ocupa {0}",
            "me da os dados de {0}",
            "propriedades de {0}",
            "que tamanho tem {0}",
            "quando mexeram em {0}",
            "info do {0}",
            "quantos bytes tem {0}",
            "data de {0}",
            "quao grande e {0}",
            "me fala do arquivo {0}",
            "ficha do {0}",
            "estatisticas de {0}",
            "quando foi criado {0}",
            "peso do arquivo {0}",
            "detalhes de {0}",
            "informacao do arquivo {0}",
            "ve o tamanho de {0}",
            "quanto pesa {0}",
            "me mostra os dados de {0}",
            "qual a data de {0}",
            "consulta as propriedades de {0}",
            "checa o tamanho de {0}",
            "sobre o arquivo {0}",
            "metadados de {0}",
            "quando {0} mudou pela ultima vez",
            "diz o tamanho do {0}",
        ],
    },
    // ── processos ───────────────────────────────────────────────────────────
    Molde {
        ferramenta: "processos",
        frases: &[
            "o que ta rodando ai atras",
            "tem coisa demais aberta",
            "quem ta consumindo tudo",
            "que programas estao de pe",
            "o que abriu sozinho aqui",
            "mostra o que nao fechei",
            "tem algo pesado ligado",
            "quem sao os gulosos agora",
            "quais programas estao rodando",
            "lista os processos",
            "o que esta aberto no pc",
            "mostra os programas abertos",
            "que processos tem rodando",
            "quais aplicativos estao ativos",
            "ve os processos ai",
            "o que esta em execucao",
            "lista o que esta rodando",
            "mostra os processos ativos",
            "quais programas estao ligados",
            "me diz o que esta aberto",
            "processos em execucao",
            "o que ta rodando agora",
            "quantos programas estao abertos",
            "mostra a lista de processos",
            "quais tarefas estao ativas",
            "ve o que esta consumindo o pc",
            "programas em execucao",
            "checa os processos",
            "quem esta rodando ai",
            "lista as tarefas ativas",
            "me mostra os processos",
            "o que o computador esta executando",
            "quais servicos estao rodando",
            "mostra tudo que esta aberto",
            "da uma olhada nos processos",
            "confere os programas abertos",
            "processos do sistema",
            "que aplicativos rodam agora",
        ],
    },
    // ── rede ────────────────────────────────────────────────────────────────
    Molde {
        ferramenta: "rede",
        frases: &[
            "qual e o meu ip",
            "como esta a rede",
            "informacoes de rede",
            "estou conectado",
            "ve a conexao ai",
            "qual o ip do computador",
            "mostra a configuracao de rede",
            "a internet esta funcionando",
            "checa a rede",
            "qual meu endereco ip",
            "estado da conexao",
            "ve se tem internet",
            "dados da rede",
            "me diz o ip",
            "como esta a conexao",
            "confere se estou online",
            "qual o gateway",
            "informacao da placa de rede",
            "a rede esta ok",
            "mostra meu ip",
            "diagnostico de rede",
            "esta conectado na internet",
            "ve a configuracao da rede",
            "qual rede estou usando",
            "checa a conexao de internet",
            "detalhes da rede",
            "meu ip qual e",
            "situacao da rede",
            "consulta a rede",
            "tem conexao ai",
        ],
    },
    // ── perguntar: o que ela NÃO deve tentar fazer ─────────────────────────
    //
    // Sem exemplos negativos, a cabeça de intenção nunca aprende a duvidar: toda
    // frase do treino tem uma ferramenta certa, então a resposta "nenhuma" nunca é
    // reforçada. Medido antes disto: ela erra com margem 0,833 — convicta.
    //
    // Três famílias, e as três aparecem no uso real:
    //   1. conversa e cumprimento (não é pedido de ação nenhuma)
    //   2. coisa que ela não sabe fazer (tocar música, mandar email, abrir site)
    //   3. pedido vago demais para virar chamada (falta o objeto)
    //
    // A família 2 é a que mais importa e a mais fácil de esquecer: são pedidos
    // legítimos, bem formados, de ferramentas que simplesmente não existem aqui.
    // Sem eles a Teka encaixa "toca uma musica" na ferramenta mais parecida.
    Molde {
        ferramenta: "perguntar",
        frases: &[
            // conversa
            "oi tudo bem com voce",
            "bom dia",
            "boa noite teka",
            "e ai beleza",
            "obrigado pela ajuda",
            "valeu mesmo",
            "voce e legal sabia",
            "quem e voce afinal",
            "o que voce sabe fazer",
            "me conta uma piada",
            "como foi o seu dia",
            "ate mais tarde",
            // Fora do que ela tem.
            //
            // ATENÇÃO ao escrever aqui: exemplo fora de escopo **não pode
            // compartilhar o verbo de ação nem o domínio do objeto** com nenhuma
            // ferramenta real. Esta lista já custou 8 abstenções falsas em 59
            // pedidos válidos porque a primeira versão tinha:
            //
            //   "manda um email pro meu chefe"  → matou "manda um whoami ai"
            //   "vai chover amanha"             → matou "que dia sera amanha"
            //   "que horas o mercado abre"      → matou "quero conferir a data"
            //   "escreve um poema"              → colidia com escrever_arquivo
            //   "abre o navegador"              → colidia com ler_arquivo
            //   "pesquisa no google"            → colidia com procurar_arquivo
            //
            // O modelo não aprendeu "isto está fora"; aprendeu "manda = fora",
            // "amanha = fora". Verbo compartilhado vira sinal, e o sinal vaza.
            // AQUI FICAVAM quatro pedidos de MUSICA, escritos quando tocar musica
            // nao era capacidade dela. `atalho` chegou em 4c02b67 e envelheceu os
            // quatro de uma vez:
            //
            //   "toca uma musica ai"    -> tocar_faixa
            //   "poe um som pra tocar"  -> tocar_faixa    (gatilho "poe pra tocar")
            //   "quero ouvir podcast"   -> tocar_faixa
            //   "aumenta o volume"      -> aumentar_volume
            //
            // O teste acusou uma quinta, "seria bom uma radio tocando", e ela era
            // FALSO POSITIVO: o gatilho "toca" casava dentro de "tocando", porque o
            // casamento nao respeitava fronteira de palavra. Consertei o medidor
            // antes de mexer no dado, e a frase ficou onde estava. Quase "consertei"
            // dado bom por causa de bug no instrumento.
            //
            // O ultimo e o caso puro: a MESMA string estava no poco de `atalho`
            // rotulada `atalho` e aqui rotulada `perguntar`. Dois rotulos para uma
            // frase; o modelo nao aprende a fronteira, aprende que ali e sorteio.
            //
            // Trocados um por um, e nao apagados, para o tamanho do poco nao mudar
            // junto — o que muda e QUAIS frases, nao QUANTAS.
            //
            // `nenhum_fora_de_escopo_e_coisa_que_ela_faz` percebe da proxima vez.
            "pede um uber pra mim",
            "acende a luz da sala",
            "regula o ar condicionado",
            "entra no youtube",
            "liga pro joao",
            "responde no whatsapp",
            "tira uma selfie",
            "reserva uma mesa no restaurante",
            "traduz isso pro ingles",
            "inventa um poema",
            "canta pra mim",
            "quem ganhou o campeonato",
            "vai fazer sol",
            "conta ate dez",
            "desenha um gato",
            // vago demais
            "faz aquilo la",
            "resolve isso pra mim",
            "da um jeito ai",
            "voce sabe do que eu preciso",
            "me ajuda com uma coisa",
            "tenta de novo",
            "continua",
            "e agora",
            "voce merece um carinho hoje",
            "vem ca me da um beijo",
            "sinto sua falta viu",
            "me da um colo agora",
            "voce me faz falta",
            "queria te conhecer pessoalmente",
            // NAO reintroduzir "e ai como voce ta hoje": e a frase
            // "e ai, como voce ta hoje" do benchmark, com uma virgula a menos.
            // Vazou aqui em 2026-09-02 e so foi vista em 04-09. O fenomeno
            // (saudacao + "como voce esta") ja esta coberto pelas quatro frases
            // vizinhas; nao precisa desta.
            "tudo certo por ai",
            "voce dormiu bem",
            "como foi seu fim de semana",
            "ta tudo tranquilo contigo",
            "voce anda cansada",
            "musica suave cairia bem agora",
            "to a fim de um rock",
            "quero uma musica calma",
            "que banda voce curte",
            "seria bom uma radio tocando",
            "tenho vontade de um cafe agora",
            "quero uma pizza agora",
            "que tal um lanche",
            "to com fome de alguma coisa boa",
            "deve estar quente na sua cidade",
            "vai chover no fim de semana",
            "ta frio ai na sua regiao",
            "que temperatura faz hoje na rua",
            "voce e uma pessoa de verdade",
            "quem te ensinou a falar",
            "voce tem sentimentos mesmo",
            "o que voce acha de mim",
            "voce fica triste as vezes",
            "voce sonha",
            "bora jogar alguma coisa",
            "me conta uma historia",
            "vamos brincar de adivinhacao",
            "qual seu filme preferido",
            "o preco de um carro novo ta salgado",
            "quem venceu a eleicao",
            "como esta o dolar hoje",
            "a china e enorme de gente",
            "aquele negocio de ontem, lembra",
            "voce lembra o que eu falei",
            "aquilo que a gente combinou",
            "sabe o que eu quero",
        ],
    },
    Molde {
        ferramenta: "hora",
        frases: &[
            "informa a hora ai",
            "aponta o horario pra mim",
            "revela que dia e hoje",
            "esclarece a data por favor",
            "indica o horario atual",
            "responde que horas sao",
            "solta a hora ai",
            "sinaliza o dia de hoje",
            "adianta que horas deu",
            "avisa a hora pra mim",
            "declara o dia de hoje",
            "cochicha a hora ai",
            "que dia caiu hoje",
            "estou perdido nos dias",
            "quanto falta pro fim de semana",
            "em que ponto do mes a gente esta",
            "ja passou do meio dia",
            "e cedo ou ja ta tarde",
            "amanha cai em que dia",
            "faz quanto tempo que virou o mes",
            "hoje e dia util",
            "que semana do mes e essa",
            "nao sei em que ponto do ano estamos",
            "ja virou o dia",
            "que dia da semana e hoje",
            "me diz agora que horas sao ai",
            "preciso do horario exato",
            "ta na hora de que agora",
            "hoje e que data",
            "consulta o calendario",
            "diz o dia e a hora certinho",
            "que horario marca agora",
            "que horas sao",
            "me diz a hora",
            "qual e a hora agora",
            "que dia e hoje",
            "qual a data de hoje",
            "ta que horas",
            "me fala a data",
            "hora certa por favor",
            "que horas ta",
            "diz ai a data e a hora",
            "e ai que dia e hoje mesmo",
            "que horas voce marca",
            "sabe me dizer a hora",
            "preciso saber que dia e hoje",
            "queria saber a hora",
            "hoje e dia quantos",
            "ta cedo ainda",
            "me lembra que dia e hoje",
            "consegue ver a hora pra mim",
            "olha no relogio ai",
            "qual o horario agora",
            "em que dia estamos",
        ],
    },
    Molde {
        ferramenta: "listar_pasta",
        frases: &[
            "desfia o que tem na pasta {0}",
            "revela o conteudo da pasta {0}",
            "escancara a pasta {0}",
            "desdobra a pasta {0}",
            "aponta o que mora em {0}",
            "destrincha a pasta {0}",
            "lista o conteudo do diretorio {0}",
            "mostra tudo que esta salvo em {0}",
            "queria uma listagem de {0}",
            "abre o diretorio {0}",
            "tem o que dentro de {0}",
            "faz um levantamento dos arquivos de {0}",
            "me diz o que existe em {0}",
            "inventario da pasta {0}",
            "lista os arquivos de {0}",
            "o que tem em {0}",
            "mostra a pasta {0}",
            "quais arquivos estao em {0}",
            "da uma olhada em {0}",
            "abre a pasta {0} e me diz o que tem",
            "ve o conteudo da pasta {0}",
            "quero ver {0}",
            "lista {0}",
            "me mostra os arquivos dentro de {0}",
            "consegue me mostrar o que tem dentro da pasta {0}",
            "mostra ai o que tem na pasta {0}",
            "queria ver os arquivos da pasta {0}",
            "pode listar a pasta {0}",
            "o que foi parar dentro de {0}",
            "quantos arquivos tem em {0}",
            "explora a pasta {0}",
            "da um dir na pasta {0}",
            "quero saber o que tem guardado em {0}",
            "abre {0} e lista tudo",
            "checa a pasta {0} pra mim",
            "navega ate {0} e mostra o conteudo",
        ],
    },
    Molde {
        ferramenta: "ler_arquivo",
        frases: &[
            "desenrola o conteudo de {0}",
            "revela o que tem dentro de {0}",
            "declama o arquivo {0}",
            "escancara o {0} pra mim",
            "destrincha o arquivo {0}",
            "cuspa o conteudo de {0}",
            "mostra o texto do arquivo {0}",
            "quero o conteudo do documento {0}",
            "escancara o arquivo {0}",
            "poe pra mim o {0} na tela",
            "le e me mostra {0}",
            "traz o conteudo de {0}",
            "quero ver por dentro do {0}",
            "mostra {0}",
            "abre o arquivo {0}",
            "mostra o conteudo de {0}",
            "le o arquivo {0}",
            "o que tem escrito em {0}",
            "me mostra o texto de {0}",
            "quero ler {0}",
            "abre {0} pra mim",
            "le pra mim o {0}",
            "mostra o que tem dentro do arquivo {0}",
            "da uma lida em {0}",
            "preciso saber o conteudo do {0}",
            "da uma lida no arquivo {0} por favor",
            "consegue abrir o {0}",
            "queria ver o que diz o {0}",
            "imprime o conteudo do arquivo {0}",
            "o que esta escrito no {0}",
            "me le o {0}",
            "exibe o arquivo {0}",
            "quero conferir o texto do {0}",
            "abre e mostra o {0}",
            "carrega o arquivo {0} pra eu ver",
            "poe na tela o conteudo de {0}",
        ],
    },
    // As tres da PONTE. Ver `Primitiva::Grep` para por que so estas tres.
    Molde {
        // A MARCA DE CONTEUDO E OBRIGATORIA EM TODA FRASE DAQUI.
        //
        // `procurar_arquivo` ja ocupa "procura {0}". Sem "dentro", "mencionam",
        // "onde aparece" ou "no codigo", as duas ferramentas viram a mesma coisa
        // para o modelo — que aprende superficie, e nao conceito (medido em 04/09,
        // sobre 447 erros).
        //
        // E o oposto tambem vale: nenhuma frase daqui pode ser so "procura {0}",
        // porque ai eu estaria ensinando a contradicao em vez de evita-la.
        ferramenta: "buscar_no_conteudo",
        frases: &[
            // VARIEDADE DE MARCADOR, acrescentada em 12/09.
            //
            // Nao faltava marcador -- faltava VARIEDADE dele. Os 16 moldes antigos
            // concentravam em poucas strings ("dentro dos arquivos" sozinho era 4
            // de 16), e medido no benchmark de 329 ela erra 26 vezes para
            // `procurar_arquivo`, a vizinha que acha arquivo por NOME.
            //
            // O que denuncia memorizacao de string, e nao conceito:
            //
            //     molde  "procura pela palavra {0} dentro dos arquivos"
            //     John   "procura a palavra senha nos arquivos"     erra 10/12
            //
            // Quase identicas. Se ela tivesse aprendido "busca em conteudo", a
            // troca de "pela/dentro dos" por "a/nos" nao derrubaria.
            //
            // Os marcadores do John (nos arquivos, dentro do projeto, arquivo que
            // fala de, a palavra X) NAO entram: sao a regua. Entram OUTROS da mesma
            // familia, e `checa_vazamento.py` confirmou livres: interior, linhas,
            // varrendo, grep, salvo, trecho, recheio, corpo.
            //
            // FALSEAMENTO: se a familia nao melhorar com o dobro de marcadores, ela
            // nao generaliza marcador -- memoriza -- e o conserto e outro.
            "faz um grep de {0} nos fontes",
            "procura {0} no interior dos arquivos",
            "ve se {0} aparece nas linhas de algum arquivo",
            "sai varrendo os arquivos atras de {0}",
            "que arquivo guarda {0} no corpo dele",
            "acha {0} no recheio dos arquivos",
            "olha o trecho salvo em cada arquivo por {0}",
            "esmiuca os arquivos procurando {0}",
            "onde é que {0} esta salvo dentro de algum arquivo",
            "varre o conteudo de tudo por {0}",
            "procura {0} dentro dos arquivos",
            "quais arquivos mencionam {0}",
            "onde aparece {0} no codigo",
            "acha o trecho {0} dentro dos fontes",
            "busca {0} no conteudo dos arquivos",
            "que arquivo tem {0} escrito dentro",
            "vasculha o conteudo atras de {0}",
            "me mostra onde o codigo cita {0}",
            "localiza a expressao {0} no texto dos arquivos",
            "quais fontes falam de {0}",
            "procura pela palavra {0} dentro dos arquivos",
            "em que arquivo aparece {0}",
            "caca {0} no meio do codigo",
            "peneira o conteudo dos arquivos por {0}",
            "quero achar {0} dentro dos arquivos",
            "olha dentro dos arquivos por {0}",
        ],
    },
    Molde {
        ferramenta: "procurar_arquivo",
        frases: &[
            "revira as pastas atras de {0}",
            "remexe tudo procurando {0}",
            "esquadrinha o disco por {0}",
            "investiga onde foi parar {0}",
            "apura se existe um {0} por aqui",
            "levanta pra mim o arquivo {0}",
            "recupera o caminho do {0}",
            "pinca o arquivo {0} pra mim",
            "filtra os arquivos por {0}",
            "mapeia onde vive o {0}",
            "varre o computador por {0}",
            "xereta as pastas por {0}",
            "desencava um arquivo {0}",
            "rastela o disco atras de {0}",
            "persegue o arquivo {0}",
            "topa achar um {0} pra mim",
            "garimpa o arquivo {0}",
            "rastreia onde esta {0}",
            "vasculha o pc atras de {0}",
            "cata todo arquivo {0}",
            "peneira os arquivos por {0}",
            "fareja {0} nas pastas",
            "caca o arquivo {0}",
            "tem algum arquivo {0} salvo",
            "escaneia atras de {0}",
            "acha pra mim qualquer coisa chamada {0}",
            "sumiu o {0}, acha ele",
            "pesquisa o arquivo {0}",
            "olha se existe um {0}",
            "identifica onde esta {0}",
            "procura um arquivo chamado {0}",
            "acha o {0} pra mim",
            "onde esta o arquivo {0}",
            "busca por {0}",
            "encontra o arquivo {0}",
            "procura {0} no computador",
            "sabe onde ta o {0}",
            "cade o arquivo {0}",
            "quero achar {0}",
            "faz uma busca por {0}",
            "sera que tem algum arquivo com nome {0}",
            "em que pasta esta o {0}",
            "localiza o arquivo {0}",
            "consegue encontrar {0}",
            "perdi o arquivo {0}",
            "vasculha o disco atras de {0}",
            "tem algum {0} salvo por ai",
            "me ajuda a achar o {0}",
            "descobre onde ficou o {0}",
            "rastreia o arquivo {0}",
            "queria localizar {0}",
            "onde foi parar o {0}",
        ],
    },
    Molde {
        ferramenta: "calcular",
        frases: &[
            "me diz o total de {0}",
            "efetua {0}",
            "processa a conta {0}",
            "chega a quanto {0}",
            "descobre o resultado de {0}",
            "aplica a conta {0}",
            "termina essa conta {0}",
            "acha o valor de {0}",
            "quanto e {0}",
            "calcula {0}",
            "faz a conta {0}",
            "resolve {0} pra mim",
            "qual o resultado de {0}",
            "me da o resultado de {0}",
            "calcula pra mim {0}",
            "quanto da {0}",
            "faz {0}",
            "preciso do resultado de {0}",
            "me ajuda com essa conta {0}",
            "quanto que da {0}",
            "resolve essa aqui {0}",
            "consegue calcular {0}",
            "qual o valor de {0}",
            "soma ai {0}",
            "faz essa continha {0}",
            "queria saber quanto e {0}",
            "computa {0}",
            "da o resultado dessa conta {0}",
            "resolve a expressao {0}",
            "quanto seria {0}",
        ],
    },
    Molde {
        ferramenta: "memoria",
        frases: &[
            "o pc ta patinando de novo",
            "esse note ta rastejando",
            "o sistema ta afogado hoje",
            "ta tudo capenga por aqui",
            "a maquina ta gemendo com tanta aba",
            "isso aqui ficou lerdo demais",
            "o computador ta moroso agora",
            "ta patinando pra abrir qualquer coisa",
            "aponta o uso de ram",
            "revela quanta memoria sobra",
            "declara o estado da memoria",
            "informa a ram disponivel",
            "o computador ta arrastado hoje",
            "por que essa lentidao toda",
            "da pra abrir mais um programa sem travar",
            "esse note nao aguenta muita coisa junta",
            "ta tudo se arrastando aqui",
            "sobra folego pra mais uma janela",
            "o sistema ta sufocado",
            "cabe mais coisa aberta ou vai travar",
            "anda devagar demais desde cedo",
            "a maquina nao da conta do que eu abri",
            "quanto ainda da pra abrir sem engasgar",
            "isso aqui empaca toda hora agora",
            "situacao da memoria agora",
            "quanto de ram livre",
            "me informa sobre a ram",
            "a memoria do sistema esta como",
            "verifica a ram",
            "quanto de memoria o pc gastou",
            "relatorio de memoria",
            "estado atual da ram",
            "quanta memoria esta em uso",
            "como esta a ram",
            "ver memoria",
            "quanto de ram ta sendo usado",
            "me diz a situacao da memoria",
            "a memoria ta cheia",
            "quanto de memoria sobrou",
            "status da ram",
            "ta usando muita memoria",
            "checa a memoria ai",
            "ta sobrando memoria no pc",
            "quanto de ram esse pc tem livre",
            "a ram ta aguentando",
            "consumo de memoria agora",
            "me fala da memoria do sistema",
            "quanta ram esta ocupada",
            "o pc ta com pouca memoria",
            "da uma olhada na memoria",
            "quanto de memoria disponivel",
            "informacao de ram por favor",
            "ver uso de memoria",
            "sobrou memoria",
        ],
    },
    Molde {
        ferramenta: "disco",
        frases: &[
            "o armazenamento ta apertado",
            "ja nao tem folga no ssd",
            "to no limite de espaco aqui",
            "sobra folga no hd ainda",
            "o disco ta espremido",
            "aponta o espaco que resta",
            "revela quanto de disco sobra",
            "declara o estado do armazenamento",
            "informa o espaco livre",
            "esse disco ainda respira",
            "vai caber mais coisa aqui",
            "quanto ainda da pra guardar",
            "ta acabando o lugar",
            "preciso apagar coisa ou ainda da",
            "cabe um jogo grande ainda",
            "quanto sobrou pra salvar arquivo",
            "esse ssd ta no limite",
            "ainda tem onde por os videos",
            "da pra baixar mais uns filmes",
            "to ficando sem onde por as coisas",
            "estado atual do disco",
            "quanto de espaco resta",
            "verifica o armazenamento",
            "me informa o espaco em disco",
            "o disco anda cheio",
            "relatorio de armazenamento",
            "quanto de disco ja foi usado",
            "espaco que ainda tem",
            "quanto espaco tem no disco",
            "ver o disco",
            "quanto sobrou de hd",
            "o disco ta cheio",
            "quanto de espaco livre eu tenho",
            "checa o espaco em disco",
            "situacao do disco",
            "quanto de armazenamento sobrou",
            "ver espaco livre",
            "me diz do disco",
            "o hd ta com quanto de espaco",
            "quanto de gigas sobrou no hd",
            "ta acabando o espaco",
            "quanto o disco tem de livre",
            "espaco disponivel no armazenamento",
            "da uma olhada no disco",
            "quantos gigas livres eu tenho",
            "o ssd ta cheio",
            "me fala do armazenamento",
            "capacidade livre do disco",
            "quanto ainda cabe no disco",
            "ver armazenamento",
        ],
    },
    Molde {
        ferramenta: "escrever_arquivo",
        frases: &[
            "escreve no {0} o seguinte {1}",
            "deixa {1} salvo no arquivo {0}",
            "cria {0} contendo {1}",
            "adiciona {1} ao arquivo {0}",
            "gera o arquivo {0} com {1}",
            "quero {1} gravado em {0}",
            "escreve {1} no arquivo {0}",
            "salva {1} em {0}",
            "cria o arquivo {0} com o texto {1}",
            "grava {1} no {0}",
            "anota {1} no arquivo {0}",
            "coloca {1} dentro de {0}",
            "guarda {1} no arquivo {0}",
            "poe {1} no {0}",
            "registra {1} em {0}",
            "faz um arquivo {0} escrito {1}",
            "salva no {0} o texto {1}",
            "quero gravar {1} dentro de {0}",
        ],
    },
    // ── atalho de teclado ───────────────────────────────────────────────────
    //
    // O argumento e um NOME DE LISTA FECHADA, nao texto livre — entao os moldes
    // ensinam a mapear "pula essa musica" -> `proxima_musica`, e nao a copiar um
    // trecho qualquer. Por isso o `{0}` fica quase sempre no fim: o nome do atalho
    // nao aparece no pedido do John, ele e a TRADUCAO do pedido.
    Molde {
        ferramenta: "atalho",
        // As frases sao os GATILHOS — exatamente o que o John diz. Nao quadros
        // genericos.
        //
        // A primeira versao usava "quero {0}", "preciso de {0}", "faz {0} ai" e
        // "{0} por favor", com o poco sendo os NOMES dos atalhos. Medido: **20 das
        // 150 frases do benchmark colidiam**, porque esses quadros nao tem palavra
        // propria — "quero {0}" rouba "quero conferir a data no sistema". E o erro
        // que o comentario do poco de `perguntar` documenta, cometido de novo.
        //
        // O desenho certo saiu da tabela de gatilhos: ela ja traduz a fala natural
        // para o nome do atalho, entao o que o modelo precisa aprender e RECONHECER
        // a frase inteira e copia-la. O `{0}` sozinho e o pedido todo virando
        // argumento, e `gatilhos::casar` faz o resto.
        frases: &["{0}", "teka, {0}", "{0} por favor", "{0} ai"],
    },
    Molde {
        ferramenta: "executar_comando",
        frases: &[
            "solta um {0} pra mim",
            "joga {0} no prompt",
            "bate um {0} ai",
            "chama o {0} no shell",
            "roda esse {0} rapidinho",
            "tacar {0} no terminal",
            "roda no terminal o {0}",
            "aciona {0}",
            "manda rodar {0}",
            "executa isso ai {0}",
            "quero que rode {0}",
            "toca o comando {0}",
            "roda o comando {0}",
            "executa {0}",
            "manda um {0} no terminal",
            "roda {0} pra mim",
            "executa o comando {0} ai",
            "chama o {0}",
            "roda um {0} ai",
            "dispara o comando {0}",
            "no prompt roda {0}",
            "faz um {0} no terminal",
            "queria rodar {0}",
            "consegue executar {0}",
        ],
    },
];

/// Caminhos que são PASTA.
///
/// A separação de pastas e arquivos não é organização: é sinal de treino. Com um
/// pool único, `listar_pasta` e `ler_arquivo` sorteavam dos mesmos valores, e o
/// modelo aprendia que o tipo do argumento não distingue nada. Medido — pedindo
/// *"despeja na tela o que tem em notas.md"* ela respondia `listar_pasta`, porque
/// nada no treino ligava "termina em .md" a "é arquivo".
const PASTAS: &[&str] = &[
    "C:\\Users\\User\\Projetos",
    "C:\\Users\\User\\Downloads",
    "C:\\Users\\User\\Documentos",
    "C:\\Windows\\System32",
    "dados",
    "src",
    "src\\model",
    "src\\backend",
    "D:\\backup",
    "C:\\temp",
    "anotacoes",
    "target\\release",
    "fotos\\2026",
    "musicas",
    // Palavra solta = PASTA.
    //
    // O pool tinha 14 entradas e so QUATRO eram palavra simples (dados, src,
    // anotacoes, musicas), contra 12 arquivos todos com extensao. Com um sinal tao
    // fraco, qualquer ruido empurra para arquivo — medido:
    //
    //     "Lista dados"   -> listar_pasta      "Lista dados."  -> ler_arquivo
    //     "Lista fotos"   -> perguntar         "Lista fotos."  -> ler_arquivo
    //
    // `fotos` errava ate SEM pontuacao: o pool tinha "fotos\\2026" e nunca "fotos".
    // Mesmo caso de "target\\release". Nome composto nao ensina o nome simples.
    "fotos",
    "target",
    "documentos",
    "downloads",
    "projetos",
    "backup",
    "temp",
    "imagens",
    "videos",
    "scripts",
    "logs",
    "build",
    "relatorios",
    "planilhas",
];

/// Caminhos que são ARQUIVO.
const ARQUIVOS: &[&str] = &[
    "notas.md",
    "relatorio_final_v3.txt",
    "config.json",
    "dados\\corpus_pt.txt",
    "lista_de_compras.txt",
    "anotacoes\\ideias.md",
    "leia_me.txt",
    "Cargo.toml",
    "orcamento.csv",
    "contrato.pdf",
    "src\\main.rs",
    "C:\\temp\\saida.log",
    // De 12 para 60.
    //
    // Doze nomes nao ensinam "isto e forma de nome de arquivo", e o sintoma foi
    // cirurgico:
    //
    //     move notas.md para config.json   (destino no poco)   -> mover_arquivo
    //     move notas.md para backup.md     (destino inedito)   -> apagar_arquivo
    //
    // Nome inedito na SEGUNDA posicao faz a cabeca de presenca dizer "nao ha segundo
    // argumento", e a intencao cai para a ferramenta de UM argumento mais proxima.
    // `copiar` sobreviveu por nao ter gemea de um argumento; `mover` tem — `apagar`.
    //
    // Terceiro poco fino a causar erro medido, depois de PASTAS (4 palavras simples)
    // e COMANDOS (6 valores). O padrao: o poco tem de ensinar a FORMA, nao o item.
    "backup.md", "rascunho.txt", "diario.txt", "agenda.md", "resumo.txt",
    "planilha.csv", "config.ini", "dados.json", "notas2.md", "teste.txt",
    "relatorio.md", "docs.txt", "leia.md", "setup.ini", "lista.csv",
    "projeto.toml", "script.py", "estilo.css", "index.html", "app.js",
    "readme.md", "licenca.txt", "changelog.md", "makefile", "requisitos.txt",
    "senha.txt", "backup2.csv", "historico.log", "erro.log", "acesso.log",
    "esquema.drawio", "planilha.ods", "video.mp4", "musica.mp3", "documento.pdf",
    "apresentacao.pptx", "tabela.xlsx", "carta.docx", "banco.db", "chave.pem",
    "notas\\antigas.md", "docs\\guia.txt", "src\\lib.rs", "config\\app.json",
    "logs\\ontem.log", "backup\\copia.zip", "temp\\rascunho.txt",
    "D:\\arquivos\\nota.txt",
];

/// Raízes de disco.
const DISCOS: &[&str] = &["C:", "D:", "C:\\", "D:\\"];

/// Os nomes de atalho — os MESMOS de `teclado::ATALHOS`, e um teste prende os dois.
///
/// Poco fechado por natureza: se ela extrair um nome que nao existe la, a chamada
/// falha com "nao conheco o atalho". Ensinar nome inventado seria ensinar a errar.
pub const NOMES_DE_ATALHO: &[&str] = &[
    // As FRASES de `dados/gatilhos.txt`, nao os nomes dos atalhos.
    //
    // O ponteiro copia trecho do pedido; ele nunca produziria "proxima_musica" a
    // partir de "pula essa musica". Entao o argumento que ele aprende a copiar e a
    // FRASE, e `gatilhos::casar` traduz depois.
    //
    // A lista e copia manual da tabela, e copia manual seca: eu acrescentei dois
    // atalhos em `gatilhos.txt` e o poco continuou com os antigos, deixando as
    // capacidades novas inalcancaveis. `o_poco_de_atalhos_e_a_tabela_inteira`
    // prende as duas listas nas DUAS direcoes agora.
    // Acrescentados em 11/09, das frases do John. `tira o mudo` e `desmuta` nao
    // eram capacidade faltando: `VK_VOLUME_MUTE` ALTERNA, entao desmutar sempre
    // existiu -- faltava so o jeito de pedir. Eu tinha dito que ela nao sabia
    // desmutar; estava errado.
    "muta o pc",
    "tira o mudo",
    "desmuta",
    "volta o som",
    "pausa o som",
    "pula essa musica",
    "pula a musica",
    "proxima musica",
    "proxima faixa",
    "passa a musica",
    "avanca a musica",
    "pula essa",
    "proxima",
    "musica anterior",
    "volta a musica",
    "faixa anterior",
    "volta pra anterior",
    "musica de antes",
    "pausa a musica",
    "pausa ai",
    "para a musica",
    "pausar",
    "continua a musica",
    "retoma a musica",
    "volta a tocar",
    "despausa",
    "para tudo",
    "encerra a musica",
    "aumenta o volume",
    "aumenta o som",
    "sobe o volume",
    "sobe o som",
    "mais alto",
    "diminui o volume",
    "abaixa o volume",
    "abaixa o som",
    "diminui o som",
    "mais baixo",
    "muta o som",
    "tira o som",
    "silencia o pc",
    "mudo no sistema",
    "me muta no discord",
    "muta meu microfone",
    "desativa meu microfone",
    "me silencia",
    "muta o discord",
    "me muta",
    "ensurdece no discord",
    "desativa o audio do discord",
    "me ensurdece",
    "toca a playlist",
    "poe a playlist",
    "coloca a playlist",
    "quero a playlist",
    "toca a musica",
    "poe pra tocar",
    "quero ouvir",
    "coloca a musica",
    "bota pra tocar",
    "toca",
    "que musica e essa",
    "qual musica ta tocando",
    "que musica ta tocando",
    "qual e essa musica",
    "o que esta tocando",
    "embaralha as musicas",
    "modo aleatorio",
    "embaralha ai",
    "poe no aleatorio",
    "embaralhar",
    "para de retomar a musica",
    "para de retomar",
    "pode parar de retomar",
    "nao retoma mais a musica",
    "nao retoma mais",
    "cancela o retomar",
    "toda vez que a musica parar voce retoma",
    "sempre que a musica parar retoma",
    "retoma sempre a musica",
    "retoma sempre",
    "fica retomando a musica",
];



const NOMES: &[&str] = &[
    "relatorio",
    "config",
    "teka",
    "notas",
    "backup",
    "orcamento",
    "cv.pdf",
    "foto",
    "musica.mp3",
    "contrato",
    "planilha",
    "recibo_2026",
];

/// Contas, em SIMBOLO e POR EXTENSO.
///
/// Eram 12 e todas simbolicas. Medido nas 500 frases de fora: `calcular` errava 13
/// vezes, 9 delas virando `executar_comando`, porque "500 dividido por 8" e
/// "15% de 300" sao formas que o treino nunca tinha visto. Quem escreve conta em
/// portugues escreve com palavra, nao com `/`.
const EXPRESSOES: &[&str] = &[
    // simbolico
    "2+2",
    "15*340",
    "(100-25)/3",
    "1024*8",
    "45+55",
    "3^4",
    "7*8-12",
    "1000/25",
    "(12+8)*5",
    "99-33",
    "2,5*4",
    "144/12",
    "18*24",
    "512/16",
    "37+128",
    "900-357",
    "64*8",
    "1500/6",
    "81/9",
    "17*14",
    "300-75",
    "45*23",
    "2^10",
    "(45+55)/2",
    // por extenso — o que gente digita
    "500 dividido por 8",
    "72 menos 19",
    "45 vezes 23",
    "128 vezes 7",
    "340 vezes 12",
    "1000 dividido por 25",
    "99 menos 43",
    "64 mais 128",
    "37 vezes 9",
    "900 menos 357",
    "15 vezes 15",
    "81 dividido por 9",
    "7 vezes 8",
    "250 mais 500",
    "1500 dividido por 6",
    "17 vezes 14",
    "33 vezes 11",
    "144 dividido por 12",
    "12 ao quadrado",
    "5 ao cubo",
    "2 elevado a 10",
    "raiz de 144",
    "raiz quadrada de 81",
    // porcentagem
    "15% de 300",
    "25% de 800",
    "10% de 450",
    "50% de 1200",
    "200% de 50",
    "5% de 2000",
    "30% de 90",
    "75% de 400",
];

/// TODOS os pocos, num lugar so.
///
/// Existe porque a lista estava escrita a mao dentro dos testes, e quem acrescenta
/// poco tem de lembrar de entrar la. Em 2026-09-10 eu acrescentei `IMAGENS` e
/// `PADROES` e nao lembrei — o teste do span caiu, e caiu CERTO. Foi a segunda vez
/// no mesmo dia que uma lista manual cobrou memoria de quem edita: a outra foi o
/// `match` do [`valor_para`], que deixou duas ferramentas sem poco por nove dias.
///
/// Poco novo entra AQUI e os testes o enxergam sozinhos.
#[cfg(test)]
pub(crate) const POCOS: &[&[&str]] = &[
    PASTAS, ARQUIVOS, DISCOS, NOMES, EXPRESSOES, TEXTOS, PADROES, COMANDOS,
    PROGRAMAS, CONSULTAS, NOMES_DE_ATALHO,
];


/// Trechos que se PROCURA dentro de arquivo, para `buscar_no_conteudo`.
///
/// Tambem nao existia: o `padrao` vinha de `TEXTOS`, e "comprar pao" nao e o que
/// alguem procura dentro de um codigo. Quem busca conteudo busca nome de funcao,
/// mensagem de erro, chave de configuracao.
const PADROES: &[&str] = &[
    "TODO",
    "FIXME",
    "fn main",
    "senha",
    "panic!",
    "unwrap",
    "import react",
    "console.log",
    "SELECT",
    "erro fatal",
    "deprecated",
    "api_key",
    "localhost",
    "version",
    "def __init__",
    "null pointer",
    "timeout",
    "token",
];

const TEXTOS: &[&str] = &[
    "ola",
    "lembrete",
    "comprar pao",
    "teste",
    "reuniao amanha",
    "senha antiga",
    "anotacao rapida",
];

/// Comandos de sistema. Eram SEIS.
///
/// O poco mais fino do gerador, e o maior ralo de erro: `executar_comando` atraia
/// 37 dos 114 erros medidos. A causa e estrutural — os moldes dele usam verbos
/// genericos ("executa", "roda", "faz", "manda", "chama") que aparecem em qualquer
/// pedido. Com seis valores, o argumento nao distingue nada, entao o verbo generico
/// vira a unica pista e ele engole frase sobre pasta e sobre conta.
///
/// Compare com `ler_arquivo`: doze arquivos, todos com extensao. O argumento grita
/// "sou arquivo", e ele nao atrai erro nenhum.
const COMANDOS: &[&str] = &[
    "dir", "echo ola", "ver", "hostname", "date /t", "tasklist",
    "ipconfig", "whoami", "cls", "systeminfo", "ping google.com", "netstat -an",
    "tree", "vol", "time /t", "chkdsk", "sfc /scannow", "ipconfig /all",
    "ipconfig /flushdns", "net user", "net session", "driverquery", "wmic cpu get name",
    "getmac", "arp -a", "route print", "nslookup google.com", "tracert 8.8.8.8",
    "assoc", "path", "set", "wmic os get caption", "wmic diskdrive get model",
    "powershell Get-Process", "powershell Get-Service", "powershell Get-Date",
    "git status", "git log --oneline", "git diff", "git branch",
    "cargo build", "cargo test", "cargo check", "cargo run",
    "npm install", "npm run build", "npm test", "npm list",
    "python --version", "node --version", "java -version", "rustc --version",
    "pip list", "docker ps", "docker images", "code .",
    "notepad", "calc", "explorer .", "taskkill /IM notepad.exe",
    "shutdown /a", "echo teste",
];

/// Programas para `abrir_programa`.
const PROGRAMAS: &[&str] = &[
    "notepad", "calculadora", "explorador de arquivos", "navegador", "chrome",
    "firefox", "edge", "discord", "spotify", "steam", "vscode", "paint",
    "prompt de comando", "powershell", "gerenciador de tarefas", "configuracoes",
    "word", "excel", "bloco de notas", "terminal", "obs", "whatsapp",
];

/// Consultas para `buscar_web`. Assunto, nao comando.
///
/// De proposito NAO compartilham vocabulario com nenhuma ferramenta local: nenhuma
/// comeca com "procura por arquivo" nem menciona pasta. Achado 19 — se o negativo
/// dividisse vocabulario com `procurar_arquivo`, ela aprenderia o verbo em vez do
/// dominio, e as duas quebrariam juntas.
const CONSULTAS: &[&str] = &[
    "o que e psicologia cognitiva", "historia do brasil imperio", "como funciona um motor eletrico",
    "quem foi Ada Lovelace", "capital da Australia", "receita de pao caseiro",
    "o que e fotossintese", "diferenca entre virus e bacteria", "como surgiu a internet",
    "o que significa entropia", "quantos planetas tem o sistema solar",
    "o que e inteligencia artificial", "quem escreveu Dom Casmurro",
    "como funciona a vacina", "o que e o efeito estufa", "historia da musica classica",
    "o que e programacao funcional", "quem inventou o telefone",
    "como se forma um furacao", "o que e teoria dos jogos", "biografia de Marie Curie",
    "o que e machine learning", "como funciona o GPS", "origem da lingua portuguesa",
    "o que e uma black friday", "regras do xadrez", "como treinar um cachorro",
    "beneficios da caminhada", "o que e juros compostos", "como fazer compostagem",
    "fotossintese",
    "entropia",
    "kubernetes",
    "mitocondria",
    "blockchain",
    "penicilina",
    "hieroglifos",
    "tsunami",
    "algoritmo",
    "vacina",
    "impressionismo",
    "supercondutor",
    "microbioma",
    "criptografia",
    "hidroponia",
    "termodinamica",
    "renascimento",
    "antibiotico",
    "sismografo",
    "polinizacao",
    "estoicismo",
    "nanotecnologia",
    "arqueologia",
    "meteorologia",
    "genoma",
    "Marie Curie",
    "Ada Lovelace",
    "Santos Dumont",
    "Machado de Assis",
    "Alan Turing",
    "Chico Mendes",
    "Carlos Chagas",
    "Cecilia Meireles",
    "energia solar fotovoltaica",
    "buraco negro supermassivo",
    "vacina de RNA mensageiro",
    "computacao quantica",
    "aquecimento global",
    "inteligencia coletiva",
    "musica barroca",
    "cultura de tecidos",
    "reciclagem de plastico",
    "seguranca de senhas",
    "poluicao dos oceanos",
    "extincao dos dinossauros",
    "ferrovia transcontinental",
    "moeda digital de banco central",
];

/// Enchimentos que aparecem na frente de um pedido falado.
const PREFIXOS: &[&str] = &[
    "", "", "", "", "ai", "entao", "olha", "opa", "teka", "ei teka", "por favor",
    "rapidinho", "cara", "pf", "ó", "escuta", "hum", "so", "vamos la",
];

/// Enchimentos LONGOS na frente.
///
/// Existem para a **cauda** de comprimento, nao para a mediana — por isso ficam num
/// poco separado, aplicado com [`PROB_ENCHIMENTO_LONGO`], em vez de entrarem no
/// [`PREFIXOS`] comum. Pedido curto e o caso comum e deve continuar dominando.
///
/// Medido: sem estes, o gerador produzia mediana de 37 bytes e uma frase de 86+
/// bytes a cada vinte mil. As dez perguntas que um amigo do John escreveu para
/// testa-la tinham mediana 106, e ela acertava a ferramenta em 9 de 10 e o argumento
/// em ZERO — o ponteiro nunca vira entrada daquele tamanho.
const PREFIXOS_LONGOS: &[&str] = &[
    "voce pode me dizer",
    "eu queria saber se voce consegue",
    "sera que da pra voce",
    "me faz um favor e",
    "se nao for muito incomodo",
    "estou precisando que voce",
    "antes de mais nada eu queria",
    "olha so eu queria pedir uma coisa",
    "desculpa incomodar mas voce pode",
    "se voce tiver um tempinho ai",
    "queria muito que voce conseguisse",
    "to aqui pensando e resolvi pedir",
];

/// Enchimentos LONGOS no fim. Ver [`PREFIXOS_LONGOS`].
const SUFIXOS_LONGOS: &[&str] = &[
    "quando puder sem pressa nenhuma",
    "se voce conseguir claro",
    "e me avisa quando terminar",
    "obrigado desde ja pela ajuda",
    "se nao der tudo bem tambem",
    "eu agradeco muito de verdade",
    "mas so se nao atrapalhar nada",
    "que eu fico te devendo essa",
];

/// Com que frequencia um pedido ganha enchimento longo.
///
/// Baixa de proposito. Cauda, nao mediana: subir isto encheria o treino de frase
/// verbosa e ensinaria o modelo que pedido normal e comprido, que e falso.
pub const PROB_ENCHIMENTO_LONGO: f64 = 0.15;

/// Enchimentos no fim.
const SUFIXOS: &[&str] = &[
    "", "", "", "", "por favor", "pra mim", "ai", "rapidinho", "obrigado", "vai",
    "agora", "beleza", "valeu", "hein",
];

/// Aplica UM erro de digitacao numa posicao alfabetica fora dos argumentos.
///
/// Byte-level tem uma vantagem aqui que tokenizer nao tem: "arquivvo" e "arqivo"
/// ficam a um byte de distancia de "arquivo", enquanto num vocabulario BPE viram
/// sequencias de tokens completamente diferentes. Treinar com ruido explora isso, e
/// e o que o texto real traz -- teclado, pressa, e um dia o Whisper.
///
/// O erro nunca cai dentro de um argumento: corromper o argumento ensinaria o
/// ponteiro a apontar para um valor que nao e o que a ferramenta precisa. Os spans
/// que vem depois da posicao alterada sao deslocados junto.
fn talvez_errar(pedido: &mut String, args: &mut [(usize, (usize, usize))], rng: &mut Rng) {
    if rng.uniform01() > 0.18 {
        return;
    }
    let bytes = pedido.as_bytes().to_vec();
    let dentro_de_arg = |i: usize| args.iter().any(|&(_, (a, b))| i >= a && i < b);
    let candidatos: Vec<usize> = (0..bytes.len())
        .filter(|&i| bytes[i].is_ascii_alphabetic() && !dentro_de_arg(i))
        .collect();
    if candidatos.len() < 3 {
        return;
    }
    let i = candidatos[(rng.uniform01() * candidatos.len() as f64) as usize % candidatos.len()];

    let (novo, delta): (String, isize) = match (rng.uniform01() * 3.0) as usize {
        // some uma letra
        0 => {
            let mut t = pedido.clone();
            t.remove(i);
            (t, -1)
        }
        // letra dobrada
        1 => {
            let mut t = pedido.clone();
            t.insert(i, bytes[i] as char);
            (t, 1)
        }
        // duas letras trocadas
        _ => {
            if i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_alphabetic() || dentro_de_arg(i + 1) {
                return;
            }
            let mut t = bytes.clone();
            t.swap(i, i + 1);
            (String::from_utf8_lossy(&t).into_owned(), 0)
        }
    };
    for (_, (a, b)) in args.iter_mut() {
        if *a > i {
            *a = (*a as isize + delta) as usize;
        }
        if *b > i {
            *b = (*b as isize + delta) as usize;
        }
    }
    *pedido = novo;
}

/// Que fracao das frases de ferramenta vira exemplo de "verbo sem objeto".
pub const PROB_SEM_OBJETO: f64 = 0.06;

/// Como falar do objeto de uma ferramenta sem nomea-lo.
///
/// `(ferramenta, nucleos, nominativo, com "de", com "em")`. Os nucleos sao os
/// substantivos que a propria frase-molde ja pode conter: se a frase e "escancara o
/// arquivo {0}", trocar `{0}` por "esse arquivo" daria "o arquivo esse arquivo".
/// Nesses casos a derivacao e descartada.
const SEM_OBJETO: &[(&str, &[&str], &str, &str, &str)] = &[
    (
        "ler_arquivo",
        &["arquivo", "documento"],
        "esse arquivo",
        "desse arquivo",
        "nesse arquivo",
    ),
    (
        "listar_pasta",
        &["pasta", "diretorio", "caminho"],
        "essa pasta",
        "dessa pasta",
        "nessa pasta",
    ),
    (
        "executar_comando",
        &["comando"],
        "esse comando",
        "desse comando",
        "nesse comando",
    ),
    (
        "calcular",
        &["conta", "expressao", "operacao", "calculo"],
        "essa conta",
        "dessa conta",
        "nessa conta",
    ),
    (
        "procurar_arquivo",
        &["arquivo", "nome"],
        "esse nome",
        "desse nome",
        "nesse nome",
    ),
    // As nove novas entram pelo mesmo motivo: "apaga esse arquivo" sem dizer qual
    // tem de virar `perguntar`, igual a "le esse arquivo".
    //
    // E ha uma razao aritmetica para nao deixar de fora: o sorteador escolhe MOLDE
    // uniformemente. Com 10 ferramentas e 5 cobertas, a familia saia em ~6% dos
    // exemplos; com 19 e as mesmas 5, caiu para 0,9% — o caso praticamente sumiu do
    // treino sem ninguem mexer nele. Crescer o registro DILUI tudo que nao cresce
    // junto.
    ("apagar_arquivo", &["arquivo"], "esse arquivo", "desse arquivo", "nesse arquivo"),
    ("info_arquivo", &["arquivo"], "esse arquivo", "desse arquivo", "nesse arquivo"),
    ("copiar_arquivo", &["arquivo"], "esse arquivo", "desse arquivo", "nesse arquivo"),
    ("mover_arquivo", &["arquivo"], "esse arquivo", "desse arquivo", "nesse arquivo"),
    ("criar_pasta", &["pasta", "diretorio"], "essa pasta", "dessa pasta", "nessa pasta"),
    ("abrir_programa", &["programa", "app", "aplicativo"], "esse programa", "desse programa", "nesse programa"),
    ("buscar_web", &["assunto", "isso"], "esse assunto", "desse assunto", "nesse assunto"),
];

/// A frase-molde da ferramenta, com o objeto trocado por um pronome.
///
/// ## Por que isto existe
///
/// Das 500 frases escritas por alguem de fora, **265 tem verbo certo e nenhum
/// objeto**: "le esse arquivo pra mim", "lista essa pasta". O treino nao tinha
/// nenhum exemplo dessa forma — a familia "vaga" que existia ("faz aquilo la", "da
/// um jeito ai") nao carrega verbo de ferramenta.
///
/// Medido, e a diferenca entre estavel e sorteio:
///
/// ```text
/// abstencao DENTRO da distribuicao   20, 16, 20 de 28    amplitude   4
/// abstencao em "verbo sem objeto"    96, 248, 246        amplitude 152
/// ```
///
/// Nao e fragilidade da cabeca: e extrapolacao. Ela nunca viu essa forma.
///
/// ## Por que derivar em vez de escrever a mao
///
/// O Achado 19 diz que exemplo negativo nao pode dividir vocabulario com o positivo,
/// senao o modelo aprende "este verbo = fora de escopo". **Aqui a partilha e o
/// ponto**: derivando da frase-molde da propria ferramenta, `le` aparece nos dois
/// lados e o que distingue e a presenca de um objeto concreto — que e exatamente a
/// regra a aprender, e exatamente o que a cabeca de presenca representa.
///
/// Escrever a mao correria o risco de escolher verbos que so aparecem do lado
/// negativo, que e o Achado 19 de volta. Derivar torna isso impossivel.
fn frase_sem_objeto(ferramenta: &str, frase: &str) -> Option<String> {
    let (_, nucleos, nom, de, em) = SEM_OBJETO.iter().find(|(f, ..)| *f == ferramenta)?;
    let pos = frase.find("{0}")?;
    // Molde de mais de um marcador vira frase truncada, nao frase sem objeto.
    if frase.matches('{').count() != 1 {
        return None;
    }
    let antes = &frase[..pos];
    let depois = &frase[pos + 3..];

    // "o arquivo {0}" -> "o arquivo esse arquivo". Descarta.
    let ultima = antes.split_whitespace().last().unwrap_or("");
    if nucleos.contains(&ultima) {
        return None;
    }

    // Contrai a preposicao que vier logo antes: "de {0}" -> "desse arquivo".
    for (prep, subst) in [
        (" de ", de),
        (" do ", de),
        (" da ", de),
        (" em ", em),
        (" no ", em),
        (" na ", em),
        (" o ", nom),
        (" a ", nom),
        (" um ", nom),
        (" uma ", nom),
    ] {
        if let Some(base) = antes.strip_suffix(prep) {
            return Some(format!("{base} {subst}{depois}"));
        }
    }
    Some(format!("{antes}{nom}{depois}"))
}

/// Palavras dos moldes na forma acentuada. Tabela, e nao acento aleatorio: trocar
/// vogal a esmo produz "memoriA" (com acento na ultima), que nao e portugues e nao
/// ensina nada. O que se quer e que os DOIS jeitos que gente escreve de verdade —
/// "memoria" com pressa e "memoria" certinho — cheguem na mesma intencao.
const ACENTUADAS: &[(&str, &str)] = &[
    ("memoria", "memória"), ("horario", "horário"), ("diretorio", "diretório"),
    ("espaco", "espaço"), ("voce", "você"), ("musica", "música"),
    ("numero", "número"), ("pagina", "página"), ("codigo", "código"),
    ("ultimo", "último"), ("disponivel", "disponível"), ("sao", "são"),
    ("nao", "não"), ("ta", "tá"), ("ai", "aí"), ("ve", "vê"),
    ("so", "só"), ("ja", "já"), ("la", "lá"), ("historico", "histórico"),
    ("relatorio", "relatório"), ("proximo", "próximo"), ("maquina", "máquina"),
    ("informacao", "informação"), ("execucao", "execução"), ("acao", "ação"),
];

/// Siglas que gente digita em caixa alta. Tambem tabela, pelo mesmo motivo: o que
/// quebrou de verdade na medicao foi `RAM` — o modelo respondia `perguntar` para
/// "Quanta RAM ta usando?" e `memoria` para "Quanta ram ta usando?". Tres bytes.
const SIGLAS: &[(&str, &str)] = &[
    ("ram", "RAM"), ("pc", "PC"), ("hd", "HD"), ("ssd", "SSD"), ("cpu", "CPU"),
    ("gb", "GB"), ("mb", "MB"), ("kb", "KB"), ("usb", "USB"),
];

/// Troca UMA ocorrencia de `de` por `para`, fora dos argumentos, com fronteira de
/// palavra. Desloca os spans que vierem depois, igual `talvez_errar`.
fn trocar_palavra(
    pedido: &mut String,
    args: &mut [(usize, (usize, usize))],
    de: &str,
    para: &str,
) -> bool {
    let mut inicio = 0usize;
    while let Some(rel) = pedido[inicio..].find(de) {
        let i = inicio + rel;
        let fim = i + de.len();
        let b = pedido.as_bytes();
        let antes_ok = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        let depois_ok = fim >= b.len() || !b[fim].is_ascii_alphanumeric();
        let livre = !args.iter().any(|&(_, (a, z))| i < z && fim > a);
        if antes_ok && depois_ok && livre {
            pedido.replace_range(i..fim, para);
            let delta = para.len() as isize - de.len() as isize;
            if delta != 0 {
                for (_, (a, z)) in args.iter_mut() {
                    if *a > i {
                        *a = (*a as isize + delta) as usize;
                    }
                    if *z > i {
                        *z = (*z as isize + delta) as usize;
                    }
                }
            }
            return true;
        }
        inicio = fim;
    }
    false
}

/// Variacao de SUPERFICIE: maiuscula, acento, pontuacao.
///
/// Existe por uma medicao especifica. O treino tinha 0% de maiuscula inicial, 0%
/// de acento e 0% de "?"; 500 frases escritas por outra pessoa tinham 100%, 60% e
/// 21%. Em 45 frases que so testam intencao:
///
/// ```text
/// como a pessoa escreveu           34/45   76%
/// minuscula, sem acento, sem "?"   45/45  100%
/// ```
///
/// O modelo entendia as 45. Perdia 24 pontos para a tecla Shift — bytes que nunca
/// tinha visto naquela posicao. E o benchmark nao podia achar isso, porque eu
/// escrevi a regua com o mesmo ponto cego dos dados.
///
/// Nunca mexe dentro de argumento, pelo mesmo motivo de `talvez_errar`: o ponteiro
/// COPIA o trecho, entao corromper o argumento ensina a apontar para um valor que a
/// ferramenta nao aceita. `configuracao.ini` e `configuração.ini` sao arquivos
/// diferentes para o sistema de arquivos.
fn talvez_variar_superficie(
    pedido: &mut String,
    args: &mut [(usize, (usize, usize))],
    rng: &mut Rng,
) {
    // Sorteia entre as palavras QUE ESTAO na frase, nao entre as da tabela.
    //
    // A primeira versao sorteava da tabela e tentava aplicar. Como a sorteada quase
    // nunca estava na frase, `RAM` aparecia em 0,05% dos exemplos em vez dos ~15%
    // pretendidos — e `RAM` era justamente o caso que motivou tudo isto. O teste
    // `a_variacao_de_superficie_acontece_de_fato` pegou.
    let presentes = |tab: &'static [(&'static str, &'static str)], p: &str| {
        tab.iter()
            .filter(|(de, _)| {
                p.match_indices(de).any(|(i, _)| {
                    let b = p.as_bytes();
                    let fim = i + de.len();
                    (i == 0 || !b[i - 1].is_ascii_alphanumeric())
                        && (fim >= b.len() || !b[fim].is_ascii_alphanumeric())
                })
            })
            .copied()
            .collect::<Vec<_>>()
    };

    // Sigla em caixa alta. Mesmo numero de bytes, span nao se move.
    if rng.uniform01() < 0.35 {
        let c = presentes(SIGLAS, pedido);
        if !c.is_empty() {
            let (de, para) = c[(rng.uniform01() * c.len() as f64) as usize % c.len()];
            trocar_palavra(pedido, args, de, para);
        }
    }
    // Acento. Muda o tamanho, e `trocar_palavra` desloca os spans.
    if rng.uniform01() < 0.30 {
        let c = presentes(ACENTUADAS, pedido);
        if !c.is_empty() {
            let (de, para) = c[(rng.uniform01() * c.len() as f64) as usize % c.len()];
            trocar_palavra(pedido, args, de, para);
        }
    }
    // Maiuscula inicial. So se a primeira letra estiver fora de argumento — um
    // pedido pode comecar direto pelo caminho, e "C:" ja e maiusculo de qualquer jeito.
    if rng.uniform01() < 0.35 {
        let primeira_livre = !args.iter().any(|&(_, (a, _))| a == 0);
        if primeira_livre {
            if let Some(c) = pedido.chars().next() {
                if c.is_lowercase() {
                    let up: String = c.to_uppercase().collect();
                    // Só quando o tamanho em bytes nao muda; senao os spans todos
                    // andariam, e nao vale a complicacao por uma letra.
                    if up.len() == c.len_utf8() {
                        pedido.replace_range(0..c.len_utf8(), &up);
                    }
                }
            }
        }
    }
    // Pontuacao no fim. Vem depois de tudo, entao nenhum span se mexe.
    if rng.uniform01() < 0.30 {
        pedido.push(if rng.uniform01() < 0.6 { '?' } else { '.' });
    }
}

fn escolher<'a>(pool: &[&'a str], rng: &mut Rng) -> &'a str {
    pool[(rng.uniform01() * pool.len() as f64) as usize % pool.len()]
}

/// O valor sorteado depende da FERRAMENTA, não só do nome do parâmetro.
///
/// É o que permite ao modelo aprender que "notas.md" pede `ler_arquivo` e "src"
/// pede `listar_pasta`, mesmo quando a frase é ambígua.
/// A palavra ANTES do slot pode obrigar o poço, e às vezes obriga.
///
/// `"copia {0} para a pasta {1}"` só faz sentido com `{1}` vindo de [`PASTAS`].
/// Sortear um arquivo ali geraria *"para a pasta notas.md"* — frase que ensina
/// errado e que nenhuma pessoa escreve.
///
/// Isto existe porque `Molde` não tem controle de poço: ele é só (ferramenta,
/// frases). Em vez de acrescentar um campo que 700 moldes não usariam, a restrição
/// sai de onde ela já está — o texto. É a MESMA restrição que uma pessoa sente ao
/// escrever a frase.
fn valor_no_contexto(
    ferramenta: &str,
    nome_param: &str,
    ate_aqui: &str,
    rng: &mut Rng,
) -> &'static str {
    let cauda = ate_aqui.trim_end().to_lowercase();
    for marca in ["pasta", "diretorio", "diretório"] {
        if cauda.ends_with(marca) {
            return escolher(PASTAS, rng);
        }
    }
    valor_para(ferramenta, nome_param, rng)
}

fn valor_para(ferramenta: &str, nome_param: &str, rng: &mut Rng) -> &'static str {
    let pool: &[&str] = match (ferramenta, nome_param) {
        ("listar_pasta", _) => PASTAS,
        ("procurar_arquivo", "raiz") => PASTAS,
        ("ler_arquivo", _) => ARQUIVOS,
        ("escrever_arquivo", "caminho") => ARQUIVOS,
        ("disco", _) => DISCOS,
        ("atalho", _) => NOMES_DE_ATALHO,
        ("buscar_web", _) => CONSULTAS,
        ("abrir_programa", _) => PROGRAMAS,
        // A ORIGEM e sempre arquivo. Sortear origem e destino do mesmo poco pode
        // dar os dois iguais, e isso e um caso real — "copia notas.md pra notas.md"
        // e um pedido bobo que ela tem de saber executar sem quebrar.
        ("copiar_arquivo", "origem") | ("mover_arquivo", "origem") => ARQUIVOS,
        // O DESTINO e arquivo OU pasta, e isso mudou em 12/09 por medicao: as duas
        // deram ZERO acerto no benchmark de 329 (36 e 60 tentativas, nenhum acerto)
        // porque todo molde ensinava arquivo->arquivo e o John pede
        // arquivo->PASTA: "joga uma copia desse arquivo na pasta backup".
        //
        // Copiar PARA uma pasta e o caso mais comum na vida real, e era o unico que
        // ela nunca tinha visto.
        ("copiar_arquivo", "destino") | ("mover_arquivo", "destino") => {
            if rng.uniform01() < 0.45 {
                PASTAS
            } else {
                ARQUIVOS
            }
        }
        ("criar_pasta", _) => PASTAS,
        ("apagar_arquivo", _) | ("info_arquivo", _) => ARQUIVOS,
        (_, "nome") => NOMES,
        (_, "expressao") => EXPRESSOES,
        (_, "comando") => COMANDOS,
        // As duas que entraram em `033d533` e nao tinham braco. Sem isto elas caiam
        // no `_ => TEXTOS` e aprendiam caminho a partir de recado.
        ("buscar_no_conteudo", "padrao") => PADROES,
        // REDE POR NOME DE PARAMETRO, e nao so por ferramenta.
        //
        // Os bracos acima sao por (ferramenta, parametro), entao TODA ferramenta
        // nova precisa lembrar de entrar aqui — e foi exatamente isso que ninguem
        // lembrou. Estes dois ultimos pegam pelo NOME: qualquer ferramenta futura
        // com `raiz` ou `caminho` ja nasce com valor plausivel em vez de recado.
        // O teste `todo_caminho_parece_caminho` cobra isso.
        (_, "raiz") => PASTAS,
        (_, "caminho") => ARQUIVOS,
        _ => TEXTOS,
    };
    escolher(pool, rng)
}

/// Gera exemplos, distribuídos igualmente entre as ferramentas.
///
/// Exemplos cuja verificação de ida e volta falha são descartados silenciosamente —
/// por isso o resultado pode vir com menos de `n`.
pub fn gerar<P: Patcher + ?Sized>(
    reg: &Registro,
    patcher: &P,
    n: usize,
    rng: &mut Rng,
) -> Vec<Exemplo> {
    let mut saida = Vec::with_capacity(n);
    let mut tentativas = 0;
    while saida.len() < n && tentativas < n * 20 {
        tentativas += 1;
        let mi = (rng.uniform01() * MOLDES.len() as f64) as usize % MOLDES.len();
        let m = &MOLDES[mi];
        let Some(fi) = reg.indice(m.ferramenta) else {
            continue;
        };
        let fri = (rng.uniform01() * m.frases.len() as f64) as usize % m.frases.len();

        // ---- "verbo certo, objeto ausente" ----
        //
        // Uma fracao das frases de ferramenta vira exemplo de ABSTENCAO, com o
        // objeto trocado por pronome. Ver `frase_sem_objeto` para por que derivar em
        // vez de escrever a mao.
        //
        // A fracao e sobre as 9 moldes de ferramenta (90% dos exemplos), entao 6%
        // delas adiciona ~5,4% de abstencao ao total, que ja tinha 10% pelo molde
        // `perguntar`. Fica em ~15%, contra os 19% que o benchmark cobra. Mexer
        // aqui e o mesmo tipo de botao que `vies_abster`: abster mais erra menos e
        // recusa mais, e onde parar depende de quanto custa cada erro.
        let derivada = if rng.uniform01() < PROB_SEM_OBJETO {
            frase_sem_objeto(m.ferramenta, m.frases[fri])
        } else {
            None
        };
        // Fica com o indice de `perguntar`, mas guarda a MESMA identidade de frase
        // (mi, fri) da positiva de onde veio: assim `dividir_por_frase` mantem as
        // duas do mesmo lado, e a negativa nunca cai na validacao com a gemea dela
        // no treino.
        let fi = match &derivada {
            Some(_) => match reg.indice("perguntar") {
                Some(p) => p,
                None => continue,
            },
            None => fi,
        };
        let frase: &str = derivada.as_deref().unwrap_or(m.frases[fri]);
        let params = &reg.ferramentas[fi].params;

        let mut pedido = String::new();
        let mut args = Vec::new();
        // O prefixo entra ANTES de qualquer span ser calculado, entao os offsets
        // ja saem certos.
        // Enchimento longo entra pelo mesmo lugar do curto: ANTES de qualquer span
        // ser calculado, entao os offsets ja saem certos apesar do prefixo grande.
        let longo = rng.uniform01() < PROB_ENCHIMENTO_LONGO;
        let pref = if longo {
            escolher(PREFIXOS_LONGOS, rng)
        } else {
            escolher(PREFIXOS, rng)
        };
        if !pref.is_empty() {
            pedido.push_str(pref);
            pedido.push(' ');
        }
        let mut resto = frase;
        while let Some(pos) = resto.find('{') {
            pedido.push_str(&resto[..pos]);
            let fim = resto[pos..].find('}').map(|x| pos + x);
            let Some(fim) = fim else { break };
            let slot: usize = resto[pos + 1..fim].parse().unwrap_or(0);
            let Some(p) = params.get(slot) else { break };
            let v = valor_no_contexto(m.ferramenta, &p.nome, &pedido, rng);
            let ini_byte = pedido.len();
            pedido.push_str(v);
            args.push((slot, (ini_byte, pedido.len())));
            resto = &resto[fim + 1..];
        }
        pedido.push_str(resto);
        // Casado com o prefixo: quando o pedido ja vem verboso na frente, o fim
        // tambem costuma vir. Sortear os dois de forma independente produziria
        // frase meio-longa, que e a forma que ninguem escreve.
        let suf = if longo {
            escolher(SUFIXOS_LONGOS, rng)
        } else {
            escolher(SUFIXOS, rng)
        };
        if !suf.is_empty() {
            pedido.push(' ');
            pedido.push_str(suf);
        }
        talvez_errar(&mut pedido, &mut args, rng);
        talvez_variar_superficie(&mut pedido, &mut args, rng);

        let ex = Exemplo {
            pedido,
            ferramenta: fi,
            args,
            frase: (mi, fri),
        };
        // Só entra se os spans sobreviverem à ida e volta pelos patches.
        if ex.alvo(patcher).is_some() {
            saida.push(ex);
        }
    }
    saida
}

/// Um caso do conjunto de teste escrito à mão (`dados/frases_teste.txt`).
#[derive(Clone, Debug)]
pub struct CasoTeste {
    pub ferramenta: String,
    pub pedido: String,
    pub argumento: Option<String>,
}

/// Lê o benchmark honesto: frases que **nunca** aparecem no gerador.
///
/// Formato por linha: `ferramenta | pedido | argumento`. Linhas vazias e as que
/// começam com `#` são ignoradas.
pub fn ler_casos_teste(texto: &str) -> Vec<CasoTeste> {
    texto
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut campos = l.split('|').map(|c| c.trim());
            let ferramenta = campos.next()?.to_string();
            let pedido = campos.next()?.to_string();
            let argumento = campos.next().filter(|a| !a.is_empty()).map(|a| a.to_string());
            if ferramenta.is_empty() || pedido.is_empty() {
                return None;
            }
            Some(CasoTeste {
                ferramenta,
                pedido,
                argumento,
            })
        })
        .collect()
}

/// Todas as frases-molde, em texto — serve para provar que o benchmark não vazou
/// para o gerador.
pub fn frases_molde() -> Vec<&'static str> {
    MOLDES.iter().flat_map(|m| m.frases.iter().copied()).collect()
}

/// Achata uma frase para comparar vazamento: só letras e dígitos ASCII, minúsculos,
/// separados por um espaço.
///
/// Deliberadamente grosseira. Um caractere acentuado vira separador em vez de virar
/// a letra sem acento, o que junta mais frases do que o estritamente correto — e
/// para uma trava de segurança errar juntando é o lado certo: o custo é um alarme
/// falso que alguém lê, contra um vazamento que ninguém vê.
///
/// Existe porque a comparação exata deixou passar uma vírgula. Ver
/// `o_benchmark_nao_vazou_para_o_gerador`.
pub fn normalizar_para_vazamento(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '@' {
            out.push(c);
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// Divide por EXEMPLO. Mede memorizacao, nao generalizacao — ver [`dividir_por_frase`].
pub fn dividir(mut exs: Vec<Exemplo>, fracao_val: f64) -> (Vec<Exemplo>, Vec<Exemplo>) {
    let n_val = ((exs.len() as f64) * fracao_val) as usize;
    let val = exs.split_off(exs.len() - n_val);
    (exs, val)
}

/// Divide por FRASE: as frases da validacao **nunca** aparecem no treino.
///
/// Esta e a divisao honesta. Com a divisao por exemplo o modelo chega a 100% de
/// acuracia decorando as frases; com esta, o numero que sai e o que ele realmente
/// faria com um pedido escrito de um jeito novo.
///
/// Reserva `1 de cada n_reserva` frases de cada ferramenta para a validacao — assim
/// toda ferramenta aparece dos dois lados.
pub fn dividir_por_frase(
    exs: Vec<Exemplo>,
    n_reserva: usize,
) -> (Vec<Exemplo>, Vec<Exemplo>) {
    let mut treino = Vec::new();
    let mut val = Vec::new();
    for e in exs {
        if e.frase.1 % n_reserva == 0 {
            val.push(e);
        } else {
            treino.push(e);
        }
    }
    (treino, val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::patcher::PorPalavra;

    #[test]
    fn gera_exemplos_validos_de_todas_as_ferramentas() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(1);
        let exs = gerar(&reg, &patcher, 3000, &mut rng);
        assert!(exs.len() > 2500, "gerou pouco: {}", exs.len());
        // A variedade de superficie tem que ser muito maior que a de frases-molde:
        // e o prefixo/sufixo/ruido multiplicando cada molde.
        let distintos: std::collections::HashSet<&str> =
            exs.iter().map(|e| e.pedido.as_str()).collect();
        assert!(
            distintos.len() > exs.len() * 3 / 4,
            "pouca variedade de superficie: {} distintos em {}",
            distintos.len(),
            exs.len()
        );

        let mut vistas = std::collections::HashSet::new();
        for e in &exs {
            vistas.insert(e.ferramenta);
            let alvo = e.alvo(&patcher).expect("exemplo aceito mas sem alvo");
            assert_eq!(alvo.ferramenta, e.ferramenta);
            // Todo parâmetro obrigatório tem span.
            for (i, p) in reg.ferramentas[e.ferramenta].params.iter().enumerate() {
                if p.obrigatorio {
                    assert!(
                        alvo.spans[i].is_some(),
                        "{}: slot {i} sem span em {:?}",
                        reg.ferramentas[e.ferramenta].nome,
                        e.pedido
                    );
                }
            }
        }
        assert_eq!(vistas.len(), reg.n(), "nem toda ferramenta apareceu");
        println!("\n  {} exemplos, {} ferramentas", exs.len(), vistas.len());
        for e in exs.iter().take(6) {
            println!(
                "    {:<18} {:?}",
                reg.ferramentas[e.ferramenta].nome, e.pedido
            );
        }
    }

    #[test]
    fn o_recorte_reproduz_o_argumento() {
        // O ponto do gerador auto-verificavel: se isto quebrar, a cabeca de ponteiro
        // estaria sendo treinada com alvos errados sem ninguem perceber.
        //
        // Sao DOIS niveis, e o teste cobra os dois separados:
        //
        //   grosso (patch)  o patch escolhido tem de CONTER o valor
        //   fino (byte)     `recortar`, que e o que a inferencia usa, tem de
        //                   reproduzir o valor EXATO
        //
        // Este teste comparava o patch inteiro com o valor, exigindo igualdade. Era
        // estrito demais e escondia um defeito: com "lista dados." o patch e "dados."
        // e o valor e "dados", entao `alvo()` devolvia None e `gerar` descartava o
        // exemplo em silencio. O modelo nunca via frase terminada em pontuacao colada
        // no argumento — 66 de 8.133 — e no uso respondia `memoria` para
        // "le notas.md.".
        //
        // A inferencia sempre soube lidar com isso (`recortar` apara pontuacao). O
        // que faltava era o treino nao ser mais rigoroso que ela.
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(2);
        for e in gerar(&reg, &patcher, 800, &mut rng) {
            let alvo = e.alvo(&patcher).unwrap();
            let bytes = e.pedido.as_bytes();
            let mut fins = Vec::new();
            patcher.fronteiras(bytes, &mut fins);
            for &(slot, (i, f)) in &e.args {
                let valor = &e.pedido[i..f];

                let (pi, pf) = alvo.spans[slot].unwrap();
                let b0 = if pi == 0 { 0 } else { fins[pi - 1] };
                let b1 = fins[pf];
                let patch = String::from_utf8_lossy(&bytes[b0..b1]);
                assert!(
                    patch.contains(valor),
                    "o patch {patch:?} nao contem o valor {valor:?} em {:?}",
                    e.pedido
                );

                let (bi, bf) = alvo.bytes[slot].unwrap();
                assert_eq!(
                    crate::model::agente::recortar(bytes, bi, bf),
                    valor,
                    "a inferencia nao reproduziria o argumento em {:?}",
                    e.pedido
                );
            }
        }
    }

    #[test]
    fn dividir_por_frase_nao_vaza_frase_entre_os_lados() {
        let reg = Registro::padrao();
        let mut rng = Rng::new(11);
        let exs = gerar(&reg, &PorPalavra::default(), 3000, &mut rng);
        let (t, v) = dividir_por_frase(exs, 4);
        assert!(!t.is_empty() && !v.is_empty());

        let frases_treino: std::collections::HashSet<_> = t.iter().map(|e| e.frase).collect();
        for e in &v {
            assert!(
                !frases_treino.contains(&e.frase),
                "frase {:?} aparece dos dois lados: {:?}",
                e.frase,
                e.pedido
            );
        }
        // Toda ferramenta precisa aparecer nos dois lados, senao a validacao mede
        // uma tarefa diferente da treinada.
        let f_t: std::collections::HashSet<_> = t.iter().map(|e| e.ferramenta).collect();
        let f_v: std::collections::HashSet<_> = v.iter().map(|e| e.ferramenta).collect();
        assert_eq!(f_t.len(), reg.n());
        assert_eq!(f_v.len(), reg.n());
        println!(
            "\n  {} treino / {} validacao, {} frases distintas no treino",
            t.len(),
            v.len(),
            frases_treino.len()
        );
    }

    /// A trava contra o erro que já foi cometido: se alguém transformar uma frase
    /// do benchmark em template, este teste quebra.
    #[test]
    fn o_benchmark_nao_vazou_para_o_gerador() {
        let texto = include_str!("../../dados/frases_teste.txt");
        let casos = ler_casos_teste(texto);
        assert!(casos.len() >= 20, "benchmark curto demais: {}", casos.len());

        // Compara pelo esqueleto: a frase do benchmark com o argumento trocado por
        // {0}, do mesmo jeito que um molde a escreveria.
        //
        // NORMALIZADO desde 2026-09-04, e o motivo custou um experimento inteiro:
        // "e ai, como voce ta hoje" (benchmark) e "e ai como voce ta hoje" (molde)
        // sao a mesma frase, e a comparacao exata deixou passar. Ela foi a maior
        // "melhora" do experimento dos 66 fora-de-escopo — 12 falhas viraram 2 — e
        // a melhora era memorizacao de regua. Sem ela, a sonda dirigida sai de
        // -0,67 para +0,17: o efeito inteiro era o vazamento.
        //
        // Uma virgula nao pode ser a diferenca entre "esta no gerador" e "nao esta".
        let moldes: std::collections::HashSet<String> = frases_molde()
            .iter()
            .map(|f| normalizar_para_vazamento(&f.replace("{0}", "@").replace("{1}", "@")))
            .collect();
        // Junta TODAS antes de falhar. Com `assert!` dentro do laço, o teste
        // estourava na primeira e escondia as outras — e quem conserta uma, roda de
        // novo, descobre a segunda, e assim por diante. Aconteceu: das três frases
        // que eu vazei de uma vez, o teste mostrou uma só.
        let vazadas: Vec<&str> = casos
            .iter()
            .filter(|c| {
                let esqueleto = match &c.argumento {
                    Some(a) => c.pedido.replace(a.as_str(), "@"),
                    None => c.pedido.clone(),
                };
                moldes.contains(&normalizar_para_vazamento(&esqueleto))
            })
            .map(|c| c.pedido.as_str())
            .collect();
        assert!(
            vazadas.is_empty(),
            "{} frase(s) de teste viraram template — o benchmark deixou de medir              generalizacao:
  {}",
            vazadas.len(),
            vazadas.join("
  ")
        );
        let reg = Registro::padrao();
        for c in &casos {
            assert!(
                reg.indice(&c.ferramenta).is_some(),
                "ferramenta desconhecida no benchmark: {}",
                c.ferramenta
            );
        }
        println!("\n  {} casos de teste, nenhum presente no gerador", casos.len());
    }

    #[test]
    fn dividir_nao_sobrepoe() {
        let reg = Registro::padrao();
        let mut rng = Rng::new(3);
        let exs = gerar(&reg, &PorPalavra::default(), 500, &mut rng);
        let n = exs.len();
        let (t, v) = dividir(exs, 0.2);
        assert_eq!(t.len() + v.len(), n);
        assert!(v.len() >= n / 6);
    }
}

// ---------------------------------------------------------------------------
// exemplos escritos à mão (destilação)
// ---------------------------------------------------------------------------

/// Lê exemplos escritos à mão e converte em [`Exemplo`], achando os spans sozinho.
///
/// ## Por que isto existe além do gerador por moldes
///
/// Moldes multiplicam o **mesmo esqueleto** com valores diferentes: 246 frases
/// viram milhares de exemplos que, linguisticamente, são 246 coisas. Frases
/// escritas por fora são genuinamente distintas — e é onde a Teka ainda erra
/// (medido: `"consegue me dizer as horas"` caiu em `disco`, apesar de o molde
/// `"consegue ver a hora pra mim"` existir).
///
/// É destilação: um modelo grande escreve, o modelo pequeno aprende.
///
/// ## Formato
///
/// ```text
/// ferramenta | pedido | argumento1 | argumento2
/// ```
///
/// Os argumentos são localizados no pedido por busca de substring. Um exemplo cujo
/// argumento não apareça literalmente no pedido é **descartado** — o ponteiro copia,
/// não inventa, então um alvo que não está lá ensinaria a coisa errada.
pub fn ler_exemplos(texto: &str, reg: &Registro) -> Vec<Exemplo> {
    let mut saida = Vec::new();
    for (i, linha) in texto.lines().enumerate() {
        let l = linha.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut campos = l.split('|').map(|c| c.trim());
        let Some(nome) = campos.next() else { continue };
        let Some(pedido) = campos.next() else { continue };
        let Some(ferramenta) = reg.indice(nome) else {
            continue;
        };
        if pedido.is_empty() {
            continue;
        }

        let mut args = Vec::new();
        let mut ok = true;
        for (slot, valor) in campos.filter(|c| !c.is_empty()).enumerate() {
            if slot >= MAX_SLOTS {
                break;
            }
            match pedido.find(valor) {
                Some(ini) => args.push((slot, (ini, ini + valor.len()))),
                None => {
                    // O argumento não está no pedido: o ponteiro não teria de onde
                    // copiá-lo. Descarta em vez de ensinar um alvo impossível.
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        saida.push(Exemplo {
            pedido: pedido.to_string(),
            ferramenta,
            args,
            // Cada frase escrita à mão é a sua própria "frase" para efeito da
            // divisão treino/validação. `usize::MAX - 1` reserva a faixa: nunca
            // colide com molde (0..) nem com episódio (`usize::MAX`).
            frase: (usize::MAX - 1, i),
        });
    }
    saida
}

#[cfg(test)]
mod testes_destilacao {
    use super::*;
    use crate::model::patcher::PorPalavra;

    const ESCRITOS: &str = include_str!("../../dados/exemplos_teka.txt");

    #[test]
    fn os_exemplos_escritos_a_mao_carregam_e_valem() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let exs = ler_exemplos(ESCRITOS, &reg);
        assert!(exs.len() > 200, "carregou pouco: {}", exs.len());

        // Todo exemplo tem que sobreviver a conversao em alvo — se o span nao
        // reproduz o argumento, o ponteiro aprenderia a apontar errado.
        let mut sem_alvo = Vec::new();
        for e in &exs {
            if e.alvo(&patcher).is_none() {
                sem_alvo.push(e.pedido.clone());
            }
        }
        assert!(
            sem_alvo.len() * 20 < exs.len(),
            "{} de {} exemplos nao viram alvo valido: {:?}",
            sem_alvo.len(),
            exs.len(),
            &sem_alvo[..sem_alvo.len().min(5)]
        );

        // Toda ferramenta precisa aparecer, senao o conjunto ensina uma tarefa
        // desbalanceada.
        let vistas: std::collections::HashSet<usize> = exs.iter().map(|e| e.ferramenta).collect();
        assert_eq!(vistas.len(), reg.n(), "nem toda ferramenta tem exemplo escrito");

        // Todo parametro obrigatorio precisa de span.
        for e in &exs {
            for (i, p) in reg.ferramentas[e.ferramenta].params.iter().enumerate() {
                if p.obrigatorio {
                    assert!(
                        e.args.iter().any(|&(s, _)| s == i),
                        "{}: falta o argumento {} em {:?}",
                        reg.ferramentas[e.ferramenta].nome,
                        p.nome,
                        e.pedido
                    );
                }
            }
        }
        println!(
            "\n  {} exemplos escritos a mao, {} descartados por span, {} ferramentas",
            exs.len(),
            sem_alvo.len(),
            vistas.len()
        );
    }

    /// A mesma trava do benchmark, agora para o conjunto de destilação: se uma
    /// frase de teste aparecer aqui, ele deixa de medir generalização.
    #[test]
    fn a_destilacao_nao_contamina_o_benchmark() {
        let reg = Registro::padrao();
        let casos = ler_casos_teste(include_str!("../../dados/frases_teste.txt"));
        let escritos: std::collections::HashSet<String> = ler_exemplos(ESCRITOS, &reg)
            .into_iter()
            .map(|e| e.pedido)
            .collect();
        for c in &casos {
            assert!(
                !escritos.contains(&c.pedido),
                "a frase de teste {:?} entrou no conjunto de treino escrito a mao",
                c.pedido
            );
        }
        println!("\n  {} casos de teste, nenhum no conjunto escrito a mao", casos.len());
    }

    /// Nenhum exemplo FORA DE ESCOPO pode ser um pedido que ela sabe atender.
    ///
    /// Quatro deles eram, e ficaram meses assim porque `atalho` chegou depois deles:
    ///
    /// ```text
    /// "aumenta o volume"      -> aumentar_volume   (e gatilho LITERAL da tabela)
    /// "toca uma musica ai"    -> tocar_faixa
    /// "poe um som pra tocar"  -> tocar_faixa
    /// "quero ouvir podcast"   -> tocar_faixa
    /// ```
    ///
    /// "aumenta o volume" e o caso puro: a MESMA string aparece no poco de `atalho`
    /// (moldura `{0}`) rotulada `atalho`, e aqui rotulada `perguntar`. Dois rotulos
    /// para uma frase — o modelo nao aprende a fronteira, aprende que ali e sorteio.
    ///
    /// E o comentario que avisa disto esta escrito tres linhas acima da lista. Saber
    /// a regra nao basta: **capacidade nova envelhece o fora-de-escopo antigo**, e
    /// so um teste percebe.
    #[test]
    fn nenhum_fora_de_escopo_e_coisa_que_ela_faz() {
        let tab = crate::tools::gatilhos::tabela();
        let mut presos = Vec::new();
        for m in MOLDES.iter().filter(|m| m.ferramenta == "perguntar") {
            for f in m.frases {
                if let Some((a, _)) = crate::tools::gatilhos::casar_em(&tab, f) {
                    presos.push(format!("{f:?} e ensinado como `perguntar`, mas a tabela faz {a}"));
                }
            }
        }
        assert!(presos.is_empty(), "fora-de-escopo que ela sabe atender:
  {}", presos.join("
  "));
    }

    /// Nenhuma frase do benchmark pode ser capturada pela tabela de gatilhos.
    ///
    /// O molde de `atalho` usava quadros genéricos — "quero {0}", "preciso de {0}",
    /// "faz {0} ai". Vinte das 150 frases do benchmark casavam com eles, entre elas
    /// *"quero conferir a data no sistema"* (que é `hora`) e *"faz a matematica de
    /// (5+3)*2"* (que é `calcular`). O molde estava ensinando que aquelas superfícies
    /// são atalho — roubando de outras ferramentas, o mesmo erro que já está escrito
    /// aqui em cima como "manda = fora" e que eu repeti mesmo assim.
    ///
    /// Hoje o molde usa as PRÓPRIAS frases de gatilho, e a colisão tem de ser zero.
    #[test]
    fn nenhuma_frase_do_benchmark_cai_na_tabela_de_gatilhos() {
        /// O gabarito que envelheceu, e a razão de ele ficar como está.
        ///
        /// A seção do arquivo se chama "coisa que ela nao tem". Tocar música virou
        /// capacidade dela em 4c02b67, então esta linha hoje está ERRADA: o certo é
        /// `tocar_playlist`. Não conserto agora porque a régua tem de ser idêntica
        /// nos dois braços da medição de custo da ferramenta 20 (ver ROTEIRO);
        /// mudá-la no meio invalidaria as 12 corridas já no disco.
        const GABARITO_VELHO: &[&str] = &["quero ouvir uma playlist relaxante"];

        let casos = ler_casos_teste(include_str!("../../dados/frases_teste.txt"));
        let tab = crate::tools::gatilhos::tabela();
        let mut presos = Vec::new();
        for c in &casos {
            if c.ferramenta == "atalho" || GABARITO_VELHO.contains(&c.pedido.as_str()) {
                continue;
            }
            if let Some((a, _)) = crate::tools::gatilhos::casar_em(&tab, &c.pedido) {
                presos.push(format!("{:?} e {} e a tabela leva para {a}", c.pedido, c.ferramenta));
            }
        }
        assert!(presos.is_empty(), "a tabela de gatilhos rouba do benchmark:
  {}", presos.join("
  "));
    }

    /// O exemplo com pontuacao COLADA no argumento chega ao treino?
    ///
    /// `gerar` descarta em silencio todo exemplo cujo span nao sobrevive a ida e
    /// volta pelo patcher. A suspeita: a pontuacao final que a variacao de
    /// superficie adiciona cola no argumento quando ele e a ultima coisa da frase, o
    /// span deixa de bater com o patch, e o caso e filtrado fora — de modo que o
    /// modelo nunca ve o que eu acho que estou ensinando.
    ///
    /// Medido no modelo treinado, o sintoma:
    ///
    /// ```text
    /// "Lista dados"    -> listar_pasta      "le notas.md"   -> ler_arquivo
    /// "Lista dados."   -> perguntar         "le notas.md."  -> memoria
    /// ```
    #[test]
    fn pontuacao_colada_no_argumento_chega_ao_treino() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(21);
        let exs = gerar(&reg, &patcher, 30000, &mut rng);

        let mut com_pontuacao = 0usize;
        let mut colada = 0usize;
        for e in &exs {
            if !(e.pedido.ends_with('?') || e.pedido.ends_with('.')) {
                continue;
            }
            com_pontuacao += 1;
            // O argumento termina exatamente antes da pontuacao final?
            let fim_texto = e.pedido.len() - 1;
            if e.args.iter().any(|&(_, (_, b))| b == fim_texto) {
                colada += 1;
            }
        }
        println!(
            "
  exemplos com pontuacao final: {com_pontuacao}
               com o argumento colado nela:  {colada}"
        );
        assert!(
            colada > com_pontuacao / 50,
            "so {colada} de {com_pontuacao} exemplos pontuados tem o argumento colado              na pontuacao — o filtro de span esta comendo o caso"
        );
    }
    /// Pasta SIMPLES colada na pontuacao — o caso que "Lista dados." exercita.
    #[test]
    fn mede_pasta_simples_colada_na_pontuacao() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(41);
        let exs = gerar(&reg, &patcher, 30000, &mut rng);
        let li = reg.indice("listar_pasta").unwrap();
        let ri = reg.indice("ler_arquivo").unwrap();
        let mut c = [0usize; 4]; // [pasta simples, pasta simples+pont, arq, arq+pont]
        for e in &exs {
            let pont = e.pedido.ends_with('?') || e.pedido.ends_with('.');
            let colado = pont && e.args.iter().any(|&(_, (_, b))| b == e.pedido.len() - 1);
            let Some(&(_, (a, b))) = e.args.first() else { continue };
            let valor = &e.pedido[a..b];
            let simples = !valor.contains('\\') && !valor.contains(':');
            if e.ferramenta == li && simples {
                c[0] += 1;
                if colado { c[1] += 1; }
            }
            if e.ferramenta == ri {
                c[2] += 1;
                if colado { c[3] += 1; }
            }
        }
        println!(
            "
  listar_pasta com pasta SIMPLES: {} ({} colados na pontuacao, {:.1}%)
               ler_arquivo:                    {} ({} colados, {:.1}%)",
            c[0], c[1], 100.0 * c[1] as f64 / c[0].max(1) as f64,
            c[2], c[3], 100.0 * c[3] as f64 / c[2].max(1) as f64
        );
    }
    /// Quantos exemplos de cada ferramenta terminam em <argumento><pontuacao>.
    ///
    /// Diagnostico do `listar_pasta`: ele resolve 11 de 50 na conversa e erra 29.
    #[test]
    fn mede_pontuacao_colada_por_ferramenta() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(31);
        let exs = gerar(&reg, &patcher, 30000, &mut rng);
        let mut por: std::collections::BTreeMap<&str, [usize; 2]> = Default::default();
        for e in &exs {
            let nome = reg.ferramentas[e.ferramenta].nome.as_str();
            let c = por.entry(nome).or_insert([0; 2]);
            c[0] += 1;
            if (e.pedido.ends_with('?') || e.pedido.ends_with('.'))
                && e.args.iter().any(|&(_, (_, b))| b == e.pedido.len() - 1)
            {
                c[1] += 1;
            }
        }
        println!("
  ferramenta          n   colada   %");
        for (nome, c) in &por {
            println!("  {:<16} {:>5} {:>7}  {:>5.2}%", nome, c[0], c[1],
                     100.0 * c[1] as f64 / c[0] as f64);
        }
    }
    /// A familia "verbo certo, objeto ausente" aparece, e sai rotulada `perguntar`.
    #[test]
    fn a_familia_sem_objeto_e_gerada_e_rotulada_como_abstencao() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(5);
        let exs = gerar(&reg, &patcher, 20000, &mut rng);
        let pi = reg.indice("perguntar").unwrap();

        // As anaforas so podem existir em exemplos de abstencao. Se uma delas
        // aparecer com ferramenta, o ponteiro estara sendo ensinado a copiar um
        // pronome como se fosse caminho.
        //
        // "essa conta" fica de fora: tres moldes POSITIVOS de `calcular` ja a usam
        // com o objeto junto — "me ajuda com essa conta 1024*8". A derivacao os
        // descarta corretamente (o nucleo "conta" vem logo antes do marcador), entao
        // o que sobra sao positivos legitimos. E o contraste que se quer ensinar:
        // o pronome COM expressao e conta, sem expressao e abstencao.
        let anaforas = ["esse arquivo", "essa pasta", "esse comando", "esse nome"];
        // Nenhum molde POSITIVO pode conter essas expressoes: elas sao o gatilho da
        // camada de contexto (`memory::contexto`), e um positivo que as use faz a
        // reescrita disputar com um pedido que ja tem objeto.
        let mut derivados = 0usize;
        for e in &exs {
            let tem = anaforas.iter().any(|a| e.pedido.contains(a));
            if tem {
                derivados += 1;
                assert_eq!(
                    e.ferramenta, pi,
                    "frase sem objeto rotulada como ferramenta: {:?}",
                    e.pedido
                );
                assert!(e.args.is_empty(), "abstencao com argumento: {:?}", e.pedido);
            }
        }
        let frac = derivados as f64 / exs.len() as f64;
        // ~6% das 9 moldes de ferramenta, e nem toda frase-molde deriva (as que ja
        // trazem o substantivo antes do marcador sao descartadas).
        assert!(
            (0.01..0.09).contains(&frac),
            "familia sem objeto em {frac:.3} dos exemplos"
        );
    }

    /// O verbo tem de aparecer nos DOIS lados. E o que faz o modelo aprender
    /// "falta o objeto" em vez de "este verbo e fora de escopo" — Achado 19.
    #[test]
    fn o_verbo_e_compartilhado_entre_o_positivo_e_o_negativo() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(6);
        let exs = gerar(&reg, &patcher, 30000, &mut rng);
        let pi = reg.indice("perguntar").unwrap();
        let li = reg.indice("ler_arquivo").unwrap();

        let com_verbo = |f: usize, v: &str| {
            exs.iter().any(|e| e.ferramenta == f && e.pedido.contains(v))
        };
        for verbo in ["mostra", "traz", "quero"] {
            assert!(com_verbo(li, verbo), "{verbo:?} sumiu do lado positivo");
            assert!(
                com_verbo(pi, verbo),
                "{verbo:?} nunca aparece do lado negativo — o modelo vai aprender                  o verbo em vez da ausencia do objeto"
            );
        }
    }

    /// A derivacao descarta o que ficaria duplicado.
    #[test]
    fn nao_deriva_quando_o_substantivo_ja_esta_na_frase() {
        // "escancara o arquivo {0}" viraria "escancara o arquivo esse arquivo".
        assert_eq!(frase_sem_objeto("ler_arquivo", "escancara o arquivo {0}"), None);
        assert_eq!(frase_sem_objeto("listar_pasta", "lista a pasta {0}"), None);
        // Dois marcadores nao sao frase sem objeto, sao frase truncada.
        assert_eq!(
            frase_sem_objeto("escrever_arquivo", "escreve {1} em {0}"),
            None
        );
        // Ferramenta sem objeto que valha pronome.
        assert_eq!(frase_sem_objeto("hora", "que horas sao {0}"), None);
    }

    /// A preposicao contrai, senao sai "conteudo de esse arquivo".
    #[test]
    fn a_preposicao_contrai() {
        let f = |x| frase_sem_objeto("ler_arquivo", x).unwrap();
        assert_eq!(f("traz o conteudo de {0}"), "traz o conteudo desse arquivo");
        assert_eq!(f("quero ver por dentro do {0}"), "quero ver por dentro desse arquivo");
        assert_eq!(f("poe pra mim o {0} na tela"), "poe pra mim esse arquivo na tela");
        assert_eq!(f("le e me mostra {0}"), "le e me mostra esse arquivo");
        let g = |x| frase_sem_objeto("listar_pasta", x).unwrap();
        assert_eq!(g("o que tem em {0}"), "o que tem nessa pasta");
    }
    /// O span de argumento tem que sobreviver a variacao de superficie.
    ///
    /// Este e o teste que importa nesta mudanca. A acentuacao MUDA O TAMANHO em
    /// bytes ("memoria" tem 7, "memória" tem 8), entao todo span depois da troca
    /// anda um byte. Se o deslocamento estiver errado, o ponteiro passa a copiar
    /// "otas.md" no lugar de "notas.md" — e nada quebra, nada avisa: o treino
    /// simplesmente ensina a apontar errado.
    ///
    /// A verificacao e exata de proposito: o trecho apontado tem que ser
    /// TODO parametro de tipo `Caminho` tem de receber valor que PARECE caminho.
    ///
    /// Esta e a trava que faltava. `valor_para` casa por (ferramenta, parametro), e
    /// quem adiciona ferramenta tem de lembrar de entrar la — em 2026-09-09 duas
    /// entraram e ninguem lembrou. Resultado medido no dia seguinte:
    ///
    /// ```text
    /// ler_imagem          caminho -> "ola" | "lembrete" | "comprar pao"   (7 valores)
    /// buscar_no_conteudo  raiz    -> os mesmos 7
    /// ler_arquivo         caminho -> "notas.md" | "Cargo.toml" | ...     (59 valores)
    /// ```
    ///
    /// A ferramenta que le IMAGEM nunca tinha visto um `.png`. E nao foi descuido de
    /// quem escreveu: o `_ => TEXTOS` aceita em silencio, e nada apontava.
    ///
    /// O teste nao confere lista de ferramenta — confere a PROPRIEDADE. Ferramenta
    /// nova com parametro de caminho cai aqui sozinha, sem ninguem lembrar de nada.
    #[test]
    fn todo_caminho_parece_caminho() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(11);
        let exs = gerar(&reg, &patcher, 6000, &mut rng);

        // A propriedade e "veio de um poco de CAMINHO", e nao "tem cara de caminho".
        //
        // A primeira versao deste teste cobrava extensao ou separador, e caiu em
        // `makefile` -- que e nome de arquivo de verdade, sem extensao nenhuma. A
        // heuristica era proxy; a pertinencia ao poco e a propriedade. Regua errada
        // no primeiro uso, de novo, e desta vez o dado e que estava certo.
        let caminhos: Vec<&str> = PASTAS
            .iter()
            .chain(ARQUIVOS)
            .chain(DISCOS)
            .copied()
            .collect();
        let parece = |v: &str| caminhos.contains(&v);

        let mut conferidos = 0usize;
        let mut faltas: Vec<String> = Vec::new();
        for e in &exs {
            let f = &reg.ferramentas[e.ferramenta];
            for &(slot, (a, b)) in &e.args {
                let Some(p) = f.params.get(slot) else { continue };
                if p.tipo != crate::tools::TipoParam::Caminho {
                    continue;
                }
                conferidos += 1;
                let v = &e.pedido[a..b];
                if !parece(v) {
                    let q = format!("{}.{} = {v:?}", f.nome, p.nome);
                    if !faltas.contains(&q) {
                        faltas.push(q);
                    }
                }
            }
        }
        assert!(conferidos > 1000, "poucos caminhos conferidos: {conferidos}");
        assert!(
            faltas.is_empty(),
            "parametro de CAMINHO recebendo valor que nao parece caminho              (falta braco em `valor_para`?):
  {}",
            faltas.join("
  ")
        );
    }

    /// IDENTICO a um valor de algum pool. Um byte de deslize e o teste cai.
    #[test]
    fn o_span_sobrevive_a_variacao_de_superficie() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(7);
        let exs = gerar(&reg, &patcher, 6000, &mut rng);

        let mut todos: Vec<&str> = Vec::new();
        for p in POCOS {
            todos.extend_from_slice(p);
        }

        let mut conferidos = 0usize;
        for e in &exs {
            for &(_, (a, b)) in &e.args {
                assert!(b <= e.pedido.len(), "span fora do pedido: {a}..{b} em {:?}", e.pedido);
                let trecho = &e.pedido[a..b];
                assert!(
                    todos.contains(&trecho),
                    "span deslizou: {trecho:?} nao e valor de pool nenhum, no pedido {:?}",
                    e.pedido
                );
                conferidos += 1;
            }
        }
        assert!(conferidos > 3000, "poucos spans conferidos: {conferidos}");
    }

    /// A variacao tem que APARECER. Sem isto, alguem baixa a probabilidade para
    /// zero um dia, o teste de span continua passando (span nao desliza se nada
    /// muda) e o defeito volta em silencio.
    #[test]
    fn a_variacao_de_superficie_acontece_de_fato() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(11);
        let exs = gerar(&reg, &patcher, 4000, &mut rng);
        let n = exs.len() as f64;

        let maiuscula = exs.iter().filter(|e| {
            e.pedido.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
        }).count() as f64 / n;
        let acento = exs.iter().filter(|e| {
            e.pedido.chars().any(|c| "áàâãéêíóôõúçÁÉÍÓÚÃÇ".contains(c))
        }).count() as f64 / n;
        let pontuacao = exs.iter().filter(|e| {
            e.pedido.ends_with('?') || e.pedido.ends_with('.')
        }).count() as f64 / n;
        let sigla = exs.iter().filter(|e| {
            e.pedido.contains("RAM") || e.pedido.contains("PC") || e.pedido.contains("SSD")
        }).count() as f64 / n;

        // Faixas largas: o objetivo e pegar "virou zero" ou "virou tudo", nao
        // fixar a probabilidade exata.
        assert!((0.15..0.60).contains(&maiuscula), "maiuscula inicial em {maiuscula:.2}");
        assert!((0.05..0.60).contains(&acento), "acento em {acento:.2}");
        assert!((0.15..0.50).contains(&pontuacao), "pontuacao final em {pontuacao:.2}");
        assert!(sigla > 0.005, "sigla em caixa alta quase nunca: {sigla:.4}");

        // E o modelo tem que continuar vendo o jeito relaxado tambem: se TUDO
        // virasse maiusculo, so teriamos trocado um ponto cego por outro.
        assert!(maiuscula < 0.60 && pontuacao < 0.50);
    }

    /// A variacao de superficie tem de ser BALANCEADA entre positivo e negativo.
    ///
    /// O Achado 19 diz que exemplo negativo nao pode dividir vocabulario com o
    /// positivo, senao o modelo aprende "esta palavra = fora de escopo". Ha uma
    /// versao pelo avesso: se `perguntar` recebesse MENOS maiuscula e acento que as
    /// ferramentas, o modelo aprenderia "texto com maiuscula = e uma ferramenta", e
    /// a abstencao quebraria em todo texto escrito normalmente.
    ///
    /// Eu levantei exatamente essa hipotese (as tabelas sao de vocabulario tecnico:
    /// "memoria", "espaco", "ram") e ia corrigi-la. Medido, ela e falsa:
    ///
    /// ```text
    /// memoria     maiuscula 33,6%   acento 28,2%
    /// perguntar   maiuscula 35,2%   acento 17,7%   <- no meio, nao no fim
    /// ler_arquivo maiuscula 35,6%   acento 10,9%
    /// ```
    ///
    /// Maiuscula sai uniforme porque nao depende de tabela nenhuma. O acento varia
    /// com o vocabulario, e `perguntar` fica acima de cinco das nove ferramentas.
    ///
    /// Este teste existe para que continue assim. Ele falha se alguem crescer as
    /// tabelas so com termo tecnico e desbalancear o negativo sem perceber.
    #[test]
    fn a_variacao_de_superficie_e_balanceada_entre_positivo_e_negativo() {
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut rng = Rng::new(3);
        let exs = gerar(&reg, &patcher, 20000, &mut rng);

        let mut por: std::collections::BTreeMap<&str, [usize; 4]> = Default::default();
        // Quantos exemplos PODIAM receber maiuscula inicial, por ferramenta.
        let mut elegiveis: std::collections::BTreeMap<&str, usize> = Default::default();
        for e in &exs {
            let nome = reg.ferramentas[e.ferramenta].nome.as_str();
            let c = por.entry(nome).or_insert([0; 4]);
            c[0] += 1;
            // MAIUSCULA SO CONTA ENTRE OS ELEGIVEIS.
            //
            // `variar_superficie` pula a maiuscula inicial quando o pedido COMECA
            // pelo argumento, e pula de proposito: capitalizar dentro de um caminho
            // daria "Notas.md" e o arquivo nao existe.
            //
            // Entao contar sobre TODOS os exemplos compara taxas que estruturalmente
            // nao podem ser iguais. Medido no `atalho`: 13,7% dos exemplos comecam no
            // argumento, e a taxa dele saiu 29,3% -- que dividido por 86,3% da 34%,
            // exatamente a probabilidade do codigo. Nao havia desbalanceamento
            // nenhum; havia denominador errado.
            //
            // Isto apareceu quando eu removi uma ferramenta e a sequencia do sorteio
            // andou. O teste vinha passando por pouco, medindo a coisa errada.
            let comeca_no_arg = e.args.iter().any(|&(_, (a, _))| a == 0);
            if !comeca_no_arg {
                elegiveis.entry(nome).and_modify(|n| *n += 1).or_insert(1usize);
                if e.pedido.chars().next().map(|x| x.is_uppercase()).unwrap_or(false) {
                    c[1] += 1;
                }
            }
            if e.pedido.chars().any(|x| "áàâãéêíóôõúçÁÉÍÓÚÃÇ".contains(x)) {
                c[2] += 1;
            }
            if e.pedido.ends_with('?') || e.pedido.ends_with('.') {
                c[3] += 1;
            }
        }
        let taxa = |c: &[usize; 4], i: usize| 100.0 * c[i] as f64 / c[0] as f64;
        // A de maiuscula tem denominador proprio: so os elegiveis.
        let taxa_mai = |nome: &str, c: &[usize; 4]| {
            let n = *elegiveis.get(nome).unwrap_or(&0);
            if n == 0 { 0.0 } else { 100.0 * c[1] as f64 / n as f64 }
        };
        println!("
  ferramenta         n   maiuscula   acento   pontuacao");
        println!("  ---------------------------------------------------------");
        for (nome, c) in &por {
            println!(
                "  {:<16} {:>5}   {:>7.1}%  {:>6.1}%   {:>7.1}%",
                nome, c[0], taxa_mai(nome, c), taxa(c, 2), taxa(c, 3)
            );
        }

        let neg = por["perguntar"];
        for (nome, c) in &por {
            if *nome == "perguntar" {
                continue;
            }
            // Maiuscula e pontuacao nao dependem de tabela: tem de sair praticamente
            // iguais. Uma folga de 8 pontos ja e generosa.
            assert!(
                (taxa_mai(nome, c) - taxa_mai("perguntar", &neg)).abs() < 8.0,
                "maiuscula desbalanceada entre os ELEGIVEIS: {nome} {:.1}% contra                  perguntar {:.1}%",
                taxa_mai(nome, c),
                taxa_mai("perguntar", &neg)
            );
            assert!(
                (taxa(c, 3) - taxa(&neg, 3)).abs() < 8.0,
                "pontuacao desbalanceada: {nome} {:.1}% contra perguntar {:.1}%",
                taxa(c, 3),
                taxa(&neg, 3)
            );
            // Acento depende do vocabulario de cada molde, entao varia de verdade.
            // O que nao pode e o NEGATIVO ficar para tras da ferramenta: e isso que
            // ensinaria "texto acentuado = ferramenta".
            assert!(
                taxa(&neg, 2) > taxa(c, 2) - 12.0,
                "acento desbalanceado contra o negativo: {nome} {:.1}% contra perguntar {:.1}%",
                taxa(c, 2),
                taxa(&neg, 2)
            );
        }
    }
    /// `trocar_palavra` so troca palavra inteira e nunca encosta em argumento.
    #[test]
    fn trocar_palavra_respeita_fronteira_e_argumento() {
        let mut rng = Rng::new(1);
        let _ = &mut rng;

        // "ram" dentro de "programa" nao pode ser trocado.
        let mut p = String::from("abre o programa ai");
        let mut args: Vec<(usize, (usize, usize))> = vec![];
        assert!(!trocar_palavra(&mut p, &mut args, "ram", "RAM"));
        assert_eq!(p, "abre o programa ai");

        // Palavra inteira, sim — e o span depois dela anda com a mudanca de tamanho.
        let mut p = String::from("le memoria de notas.md");
        let ini = p.find("notas.md").unwrap();
        let mut args = vec![(0usize, (ini, ini + "notas.md".len()))];
        assert!(trocar_palavra(&mut p, &mut args, "memoria", "memória"));
        let (a, b) = args[0].1;
        assert_eq!(&p[a..b], "notas.md", "o span nao acompanhou o byte a mais do acento");

        // Dentro do argumento, nunca.
        let mut p = String::from("le o memoria.txt");
        let ini = p.find("memoria.txt").unwrap();
        let mut args = vec![(0usize, (ini, ini + "memoria.txt".len()))];
        assert!(!trocar_palavra(&mut p, &mut args, "memoria", "memória"));
        assert_eq!(p, "le o memoria.txt");
    }
}
