# Teka — Arquitetura Atual e Plano de Modificação

| | |
|---|---|
| **Versão do documento** | 1.0 |
| **Data** | 2026-09-03 |
| **Commit de referência** | `cec5a95` |
| **Escopo** | Estado atual do sistema e mudanças planejadas até a fusão |
| **Documento irmão** | [`ROTEIRO.md`](ROTEIRO.md) — decisões, medições fechadas e o que não repetir |

> Este documento descreve **o que existe** e **o que vai mudar**. Ele não repete o
> histórico de experimentos: isso vive no `ROTEIRO.md`, e a seção 5 aqui só aponta
> para lá o que estiver encerrado.

---

## 1. Sumário executivo

A Teka é um agente **byte a byte** escrito em Rust sem dependências externas, que roda
em CPU. Ela lê um pedido em português, escolhe uma entre 20 ferramentas, extrai os
argumentos do próprio texto do pedido e executa — com confirmação antes de qualquer
ação que deixe marca.

Três propriedades definem o projeto e restringem todas as decisões abaixo:

1. **Byte a byte, sem tokenizador.** Não há vocabulário fixo, não há `<unk>`, não há
   pré-processamento que precise acompanhar o idioma.
2. **O backbone é O(n), não O(n²).** É um SSM (RG-LRU), não um transformer. É isso
   que torna o custo por byte pagável numa CPU — a barreira que historicamente
   inviabilizou modelos byte-level foi exatamente o custo quadrático da atenção.
3. **Segurança por construção, não por instrução.** O modo sandbox é o padrão, a
   confinação de escrita é verificada depois de normalizar `..`, e o que age sobre
   OUTRA PESSOA não sai de casamento aproximado de nome — a árvore de UI não tem
   identidade, só rótulo (ver 2.5.1).

O gargalo medido hoje **não é a arquitetura**. É a escolha da ferramenta (75,7%)
contra a extração do argumento (~92%). O plano da seção 4 ataca isso na ordem em que
o retorno foi medido, não na ordem em que é interessante de programar.

**O que mudou entre 30/08 e 09/09**, e vale para ler o resto do documento:

```
ferramentas    19 -> 20      `atalho`: música, volume e mudo sem sair do jogo
benchmark      110,67 -> 113,50
4.1 e 4.3      FECHADOS      poços de fora-de-escopo; canal de confirmação
o tick         medido        o "pensamento" é ruído e ninguém o lê (2.5.4)
o crítico      alimentado    tinha ZERO referências no supervisionado
```

E um padrão que apareceu três vezes em dois dias, registrado aqui porque é o tipo de
defeito que nem o compilador nem a suíte pegam: **o comentário sabia e o código não
sabia.** "Regra que age em silêncio não se depura" estava escrito e o fio não estava
ligado; "o crítico fica fora do gradcheck" estava escrito e a flag entrou assim
mesmo; e o "0,000" do crítico estava neste documento desde o começo. Requisito em
prosa não se cumpre sozinho — os três viraram `assert`.

---

## 2. Estado atual

### 2.1 Números

```
benchmark de 150       113,50 ± 3,97   20 ferramentas, 12 sementes (s19-30)
                       110,33          as mesmas 20, ANTES do conserto do
                                       fora-de-escopo — ver 4.7
                       112,92          19 ferramentas, mesma régua
argumento condicional  ~92%            quando a ferramenta sai certa
ferramenta correta     75,7%           ← o gargalo
parâmetros             1,6 M           preset "pequeno"
footprint de execução  ~24 MB          binário 1,6 + modelo 6,2 + n-grama 16
latência               ~103 ms         CPU, incluindo carregar do disco
corpus                 69,87 MB        9,7x o inicial
código                 30.057 linhas   Rust, 0 dependências
testes                 331             `#[test]`, suíte verde
```

**A ferramenta 20 está paga.** `atalho` custou −2,58 (t=−1,84, não significativo)
quando entrou; consertar a contradição do fora-de-escopo devolveu +3,17 (t=+2,55), e
o número final ficou **acima** do catálogo de 19. Isso era o portão que eu tinha
proposto para começar a fusão, e ele foi cruzado em 2026-09-08.

### 2.2 Mapa de módulos

```
src/
├── model/          o modelo e o agente
│   ├── hierarchy.rs   encoder local → backbone → decoder local          659
│   ├── heads.rs       as quatro cabeças de saída                        894
│   ├── patcher.rs     fronteiras de patch (PorPalavra | PorEntropia)    639
│   ├── ngrama.rs      n-grama de bytes, fonte de entropia               398
│   ├── agente.rs      laço de decisão: pedido → chamada                 635
│   ├── confianca.rs   margem, crítico, tamanho do argumento
│   ├── stack.rs       pilha de blocos
│   ├── io.rs          serialização do modelo
│   └── safetensors.rs
├── ssm/            o backbone recorrente
│   ├── rglru.rs       a unidade recorrente com gate                     325
│   └── block.rs       bloco = RG-LRU + MLP + norma                      294
├── nn/             primitivas: embed, linear, mlp, rmsnorm, quant
├── backend/        escalar e paralelo (escolha em tempo de execução)
├── tools/          o arsenal e as guardas
│   ├── prim.rs        as 19 primitivas e a classificação de efeito      830
│   ├── seguranca.rs   modo, política, denylist, confinação
│   ├── execucao.rs    confirmação antes de agir                         455
│   ├── diario.rs      registro do que foi feito                         386
│   └── oficina.rs     propor e testar ferramenta nova                   433
├── learn/          treino
│   ├── dados.rs       o gerador de exemplos sintéticos                2.532
│   ├── supervisionado.rs
│   ├── reforco.rs     REINFORCE com o crítico como linha de base        671
│   ├── consolidacao.rs
│   ├── adam.rs
│   └── train.rs
├── memory/         memória
│   ├── semantica.rs   conceitos, relações, proveniência                 809
│   ├── contexto.rs    janela de conversa                                646
│   └── io.rs          persistência (`TEKAS001`)                         439
├── coletor/        coleta de corpus da Wikipédia
│   ├── fonte.rs       backoff, enumeração, faixas alfabéticas           557
│   └── mod.rs         allowlist de domínio (host exato)                 394
├── gerador/        geração de texto e HTTP
├── grammar/        gramática de saída
├── ambiente/       observação da máquina
├── servidor.rs     interface web, 127.0.0.1 + token por execução        529
├── afeto.rs        estado afetivo                                       367
├── pulso.rs        laço de fundo                                        669
└── main.rs         CLI                                                2.455
```

### 2.3 Pipeline do modelo

```
bytes ─▶ [emb] ─▶ [ENCODER local] ──┬──────────────────────────────┐
                                    │  pool no fim de cada patch   │
                                    ▼                              │
                           [proj] ▶ [BACKBONE por patch] ▶ [proj]  │
                                                             │     │
                                                  broadcast  ▼     ▼
                                                        (+) ──▶ [DECODER local]
                                                                    │
                                                                    ▼
                                                    [norm] ▶ [cabeças de saída]
```

**Onde está o ganho.** O backbone concentra ~85% dos parâmetros e roda **uma vez por
patch**, não por byte. Com patches de ~4 bytes, ele roda 4x menos. É o que torna um
modelo byte-level viável sem GPU.

**Causalidade.** Ao prever o byte `t+1`, o decoder pode usar o estado do encoder em
`t` e a saída do backbone do último patch **completo**. Quem garante o segundo é o
campo `ctx` do `Plano`: dentro de um patch em curso, o contexto disponível é o patch
anterior. Sem isso o modelo veria o futuro dentro do próprio patch, e o bits/byte
ficaria bonito e mentiroso.

**Configurações:**

| preset | `d_loc` | `n_enc`/`n_dec` | `d_bb` | `n_bb` | uso |
|---|---|---|---|---|---|
| `pequeno` | 96 | 1 / 1 | 192 | 3 | **o treinado hoje** — 1,6 M |
| `padrao` | 192 | 2 / 2 | 384 | 6 | ~11,6 M, existe e não paga (ver §5) |
| `minusculo` | 4 | 1 / 1 | 5 | 2 | só checagem de gradiente |

### 2.4 As quatro cabeças

| cabeça | saída | papel |
|---|---|---|
| **intenção** | 20 classes (19 ferramentas + `Perguntar`) | o que fazer |
| **ponteiro** | `(byte_inicial, byte_final)` por slot | **decide se a chamada funciona** |
| **presença** | por slot | este argumento existe? |
| **crítico** | escalar | linha de base do REINFORCE — e, desde 09/09, prevê a própria correção |

`Perguntar` **não é uma ferramenta** — é a classe que permite não agir. Ela existe
porque foi medido que duvidar precisa ser **aprendido**, não medido depois: com
abstenção por limiar de confiança, nenhum dos três sinais separava acerto de erro (a
margem dos erros era 0,833 — ela erra convicta; o crítico saía 0,000 num modelo sem
reforço; e o tamanho do argumento separava na direção invertida).

**Aquele "0,000" ficou escrito aqui desde o começo e ninguém o leu como defeito** — inclusive
eu, que o redescobri em 2026-09-08 medindo do zero. A cabeça de crítico tinha ZERO
referências em `supervisionado.rs`: só o reforço a alimentava, e a corrida padrão é
supervisionada. Ela tinha parâmetros e produzia ruído.

Desde 09/09 ela é treinada com alvo auto-supervisionado — *"a minha própria escolha
vai estar certa?"* — pelo `Alvo::auto_critico`. É um sinal de **capacidade** e não de
superfície: hoje ela abstém porque o verbo é estranho; com o crítico treinado pode
abster porque **prevê que vai errar**. O braço de medição fecha na madrugada de
09/09.

O ponteiro tem dois níveis: patch (grosseiro, auxiliar) e **byte (fino, é o que
decide)**.

### 2.5 Registro de ferramentas — 20, classificadas por efeito

| `Nenhum` (11) | `Reversivel` (1) | `Local` (7) | `ParaFora` (1) |
|---|---|---|---|
| `hora` | `atalho` | `escrever_arquivo` | `buscar_web` |
| `listar_pasta` | | `copiar_arquivo` | |
| `ler_arquivo` | | `mover_arquivo` | |
| `info_arquivo` | | `criar_pasta` | |
| `processos` | | `abrir_programa` | |
| `rede` | | `apagar_arquivo` | |
| `procurar_arquivo` | | `executar_comando` | |
| `calcular` | | | |
| `memoria` | | | |
| `disco` | | | |
| `perguntar` | | | |

A classificação não é cosmética: **o diário, a oficina e a confirmação todos se
penduram nela**. Ler duas vezes não custa nada; escrever duas vezes custa. Sem a
distinção, o diário pagaria um `fsync` por chamada de `hora` — e o `fsync` é mais caro
que a ferramenta inteira.

`Reversivel` existe para o `atalho`: ele muda o mundo, mas desfazer custa apertar de
novo. Pausar música não é da mesma espécie que apagar arquivo, e tratar as duas
igual só ensina a pessoa a ignorar a confirmação.

`ParaFora` é uma classe separada de `Local` por um motivo específico: o que volta da
web é **texto de terceiro**, e texto de terceiro é **dado, nunca ordem**. Uma página
que diga "apague tudo" é uma página dizendo isso, não um pedido do usuário. A saída
do `buscar_web` é rotulada `[da web, nao e ordem sua]`.

### 2.5.1 A ferramenta 20 e as três camadas que ela trouxe

`atalho` é uma ferramenta só na superfície do modelo, mas três mecanismos por baixo.

**Tabela de gatilhos** (`dados/gatilhos.txt`, `tools/gatilhos.rs`). A cabeça de
ponteiro **copia** um trecho do pedido; ela não inventa texto. Então emitir
`atalho(nome="proxima_musica")` era impossível — ninguém fala assim. A tradução
acontece **fora do modelo**: 77 frases → 18 atalhos.

O casamento tem três níveis (1,00 literal / 0,95 normalizado / cobertura) e **quatro
travas, cada uma nascida de um vazamento medido**:

```
palavra inteira        "toca" casava dentro de "tocando"
palavra vazia nao conta   "mudo no sistema" pegava "conferir a data no sistema"
teto de 0,90           cobertura empatava com o literal
minimo de 2 fortes     "mais alto" pegava "o consumo de ram esta alto"
```

E uma regra de sentido: **atalho sem alvo não casa com frase que sobra conteúdo.**
Antes dela, *"muta o theo no discord"* virava `mutar_discord` — mutava o John e
jogava "theo" fora calado.

**UIAutomation** (`tools/uia.rs`). Aciona controle **pelo nome** e confirma que o
estado mudou, em vez de alternar às cegas. Três defesas, e a terceira vem de o John
ser administrador do servidor dele:

```
arvore fria          o Chromium so constroi a arvore quando alguem pergunta,
                     e a PRIMEIRA pergunta volta vazia -> `achar_teimoso`
padrao, nao nome     o Discord publica DOIS "Silenciar" e so um alterna
nome exato + lista   "Silenciar voz no servidor" CONTEM "Silenciar", e e um
                     ato publico sobre outra pessoa -> `NUNCA_SOZINHA`
```

**Ponte WebSocket** (`tools/ponte.rs`). RFC 6455 escrito à mão, `127.0.0.1:8767`,
falando com uma extensão do Spicetify. É o que troca faixa **sem roubar o foco** — e
sem a API do Spotify, que exige Premium.

### 2.5.2 A fronteira da atuação, medida

O que decide se a Teka consegue agir em segundo plano não é ela: é como o aplicativo
alvo lê entrada.

```
le a FILA DE MENSAGENS da janela   Discord, Spotify, Chromium   2o plano FUNCIONA
le ENTRADA BRUTA do dispositivo    Minecraft, jogos em geral    precisa do FOCO
```

Medido em 2026-09-09 com controle positivo (`examples/sonda_tecla_sem_foco.rs`):
`SendInput` com o jogo em foco funciona; `PostMessage` sem foco entra na fila e o
jogo ignora.

E a tela virtual **não contorna isso**: `SendInput` só é aceito quando a thread está
no desktop de **entrada** — o que recebe o teclado físico. Num desktop escondido ele
devolve `ACCESS_DENIED` (`examples/sonda_tela_virtual.rs`). Tela virtual esconde
janela; não dirige nada escondido.

### 2.5.3 Regras permanentes

`tools/regras.rs` e `dados/regras.txt`. Ligadas **pela fala** ("toda vez que a música
parar, você retoma"), não por configuração. `Vigia::olhar` roda no topo de cada tick,
com teto de 3 ações seguidas e espaço de 3s entre elas, e desiste se o dono pausou
de propósito.

Desligadas por padrão. E o resultado aparece no relatório do tick — o que **não**
acontecia até 2026-09-08: a regra agia e nada mostrava, contrariando a própria doc
dela.

### 2.5.4 O tick, e o que ele NÃO faz

`pulso.rs` roda um laço de fundo com esta forma:

```
PERCEBER    quantos episódios novos
VIGIAR      as regras permanentes — o ÚNICO ponto que age no mundo
PENSAR      gera bytes a partir do "fio", com temperatura vinda do afeto
REVER       quantos fatos estão prestes a ser esquecidos
CONSOLIDAR  re-treina sobre memória REAL
```

Parece um laço cognitivo inteiro. **Medido em 2026-09-08, não é** — e a diferença
importa para quem for mexer aqui:

```
o pensamento não é lido    fora do módulo, o único consumidor é `rel.pensamento.len()`
                           — o TAMANHO, não o conteúdo
e o pensamento é ruído     byte solto, nem palavra (`examples/sonda_pensamento.rs`).
                           Ela nunca foi treinada a gerar texto livre, só a emitir
                           chamada
```

**Ensiná-la a escrever não a faria pensar**: pré-treinar tronco em corpus já foi
medido e não transferiu para acurácia de ferramenta (ver §5).

O caminho é o **crítico** (4.8), não o pensamento: fazer o modelo prever a própria
correção dá um sinal de **capacidade**, e sobre ele cabe deliberação de verdade —
top-3 intenções, pontuar, escolher ou abster. Isso muda o resultado por causa de uma
avaliação interna, que é o que "raciocinar" significa aqui.

**O teto, dito antes de tentar:** isso faz ela **deliberar**, não **entender**. Ela
passa a saber quando não sabe — enorme, porque 53% dos erros envolvem `perguntar`.
Mas continua sem entender que *"estou desanimado"* pede música animada. Semântica
precisa de um leitor, e o leitor é a fusão (4.5).

### 2.6 Camada de segurança

| guarda | comportamento |
|---|---|
| **Modo sandbox** | **é o padrão.** Escrita e execução apenas *descrevem*. Sair exige `--real <pasta>` |
| **Denylist** | bloqueia `format`, `mkfs`, `diskpart`, `shutdown`, `reg delete`, `rm -rf` — substring em minúsculas |
| **Confinação de raiz** | normaliza `..` **antes** de comparar. Limita **escrita e execução**; leitura é deliberadamente livre, e isso está documentado no código |
| **`mover_arquivo`** | checa **as duas pontas** |
| **Confirmação** | `IsTerminal` decide. Sem terminal → sem pergunta → **sem permissão** |
| **Servidor** | `127.0.0.1` fixo no código, token de 24 hex por execução, 403 sem token |
| **Allowlist de domínio** | **uma** lista, host exato. Testes bloqueiam `pt.wikipedia.org.evil.com` e `pt.wikipedia.org@evil.com` |
| **`apagar_arquivo`** | recusa diretórios |

A guarda do `IsTerminal` merece nota porque foi paga com um travamento: `read_line`
num cano aberto e vazio **não devolve EOF, ele bloqueia**. Perguntar sem terminal não
é inseguro — é uma espera infinita.

### 2.7 Ciclo de treino

```
gerador sintético (dados.rs) ──▶ supervisionado ──▶ reforço ──▶ consolidação
     poços de valor                 seq = 128        REINFORCE
     + 776 exemplos à mão           12 épocas        crítico como linha de base
```

Uma corrida de 12 épocas leva **~34 min** por semente nesta máquina (Ryzen 5 5500,
10 threads pedidas, ~3 núcleos efetivos — o teto é a **RAM em single channel**, não a
contagem de núcleos).

---

## 3. Restrições que qualquer mudança precisa respeitar

Não são estilo. São o que separa medição de ilusão, e todas custaram caro.

1. **Três sementes não medem nada.** Com desvio de 4 a 8, três sementes resolvem
   efeito de ~15 frases e nada menor. Isso enganou o projeto **três vezes numa
   semana**. Fixar o `n` antes de começar; não interpretar parcial.
2. **Régua de baixo ruído para iterar.** Sonda dirigida e acurácia condicional de
   argumento têm desvio abaixo de 1,5. O benchmark de 150 só serve para mudança
   grande ou com 12 sementes.
3. **`argumento certo: X/80` mistura duas coisas** — conta como errado quando a
   *ferramenta* errou. Para isolar extração, ler `argumento quando a ferramenta saiu
   certa`.
4. **A régua tem que ser independente do gerador.** `dados/frases_teste.txt` nunca
   entra no treino, e dois testes impedem isso.
5. **Conferir o rótulo antes de acusar o modelo.** Já houve um 0/10 que media a
   régua, não a Teka.
6. **Nunca comparar dois braços sem conferir o carimbo de data do binário.** Se o
   executável mudou entre eles, a diferença medida pode ser dos dados.

---

## 4. O que vai ser modificado

### 4.0 Visão geral e ordem

| # | mudança | tipo | tamanho | estado |
|---|---|---|---|---|
| 4.1 | ~~Poços de fora-de-escopo (23 → 66)~~ | dados | — | **FECHADO: sem efeito medível** |
| 4.2 | ~~Escolha de ferramenta — variedade de verbo~~ | dados | — | **FECHADO: não confirmado** |
| 4.3 | ~~Canal de confirmação na interface web~~ | código | — | **FEITO em 2026-09-05** |
| 4.4 | Argumento de restrição múltipla | **arquitetura** | semanas | só com medição na mão |
| 4.5 | Fusão COMPLETA com o DeepSeek-Harness | integração | 20–40 sessões | **portão cruzado** |
| 4.6 | Importação da Nyxara | integração | — | **por último, sempre** |
| 4.7 | ~~Contradição no fora-de-escopo~~ | dados | — | **FECHADO: +3,17** |
| 4.8 | Crítico prevê a própria correção | código | — | medindo (09/09) |

A ordem é por retorno medido, não por interesse. O padrão histórico é inequívoco:
**arquitetura não moveu nada, dado moveu tudo** (ver §5).

**O portão da 4.5 foi cruzado.** Ele estava escrito assim: *"a ferramenta 20 estar
paga — o benchmark voltar a ≥112,9, o nível de 19 ferramentas"*. Fechou em 113,50,
com a mesma régua e as mesmas 12 sementes. O critério existia justamente porque
"estar boa" não tem definição, e enquanto a régua fosse essa a fusão nunca começaria.

---

### 4.1 ~~Poços de fora-de-escopo~~ — FECHADO em 2026-09-03, sem efeito

**Motivação medida.** Sobre 11 sementes, 14 frases de 150 falham em 9 ou mais. Cinco
delas são o **mesmo modo**: a Teka deveria abster-se e age, agarrando qualquer
ferramenta que compartilhe uma palavra com o pedido.

```
"queria te dar um abraco"   → procurar_arquivo
"e ai, como voce ta hoje"   → hora
```

A causa é assimetria de poço: `perguntar` tinha **23** frases para cobrir o
complemento de 18 ferramentas, enquanto `hora` sozinho tinha **42** para uma coisa só.

**Mudança.** `src/learn/dados.rs`: fora-de-escopo de 23 → 66.

**Armadilha conhecida e evitada.** Das 43 frases adicionadas, 11 na primeira versão
compartilhavam verbo de ação com ferramentas reais — o que ensinaria a abster-se de
pedidos legítimos. Foram reescritas.

**Critério de aceitação, fixado ANTES de rodar.** O veredito não sai do benchmark:
cinco frases de 150 valem no máximo 5 pontos contra um desvio de 4,48. Decide a sonda
dirigida sobre as frases de abstenção.

**RESULTADO — 12 sementes pareadas, `por_palavra` nos dois braços, só os dados mudando:**

```
                                      23 fora    66 fora    delta      t
sonda dirigida (falhas de 28)           10,83      10,17     -0,67   -0,89   <- decide
benchmark de 150                       110,67     112,75     +2,08   +1,43
abstencao INDEVIDA (agir -> perguntar)  10,83       9,67      -1,17   -0,75
argumento condicional                   94,30%     93,71%     -0,59   -1,46
```

**Nenhum dos quatro resolve.** Três inclinam de leve a favor, um de leve contra, nada
perto de significância. Quarenta e três frases novas e sete horas de treino não
produziram efeito medível.

**A tabela por frase mostra REDISTRIBUIÇÃO, não ganho** (falhas em 12 sementes):

```
consertadas                      quebradas
e ai, como voce ta hoje   12→2   dirige ate o mercado      2→6
leva o lixo pra fora       7→2   canta parabens            4→7
prega um prego na parede   6→2   faz o que eu te pedi      4→6
ta calor ai onde voce ta   9→5   resolve a equacao         2→4
voce prefere manha/noite   5→2   quem sabe depois          3→5
```

Uma das duas frases nomeadas no diagnóstico foi consertada quase por completo
(12 → 2). **A outra não** — `queria te dar um abraco` foi de 4 para 5. E as quatro
mais duras não se moveram: `me explica como funciona uma rede neural` 10 → 10,
`quero ouvir uma playlist relaxante` 10 → 10, `manda um zap pro pedro` 10 → 10,
`cria uma planilha com os meus gastos` 11 → 12.

**Erro meu na previsão de potência.** Eu calculei o alcance como "5 frases, no máximo
5 pontos". Moveram-se ~20 frases, nos dois sentidos: mexer no poço de `perguntar` não
ajusta as frases-alvo, **desloca a fronteira inteira da classe**. O raio de impacto era
maior do que eu modelei — e o líquido ainda assim deu zero.

**Decisão: as 66 ficam.** Não porque funcionaram — não funcionaram — mas porque não
custam nada em inferência e as três medidas de abstenção inclinam de leve a favor.
Fica registrado como **não medido melhor**, nunca como vitória.

**O achado que vale mais que o experimento.** `perguntar` é de longe a pior classe, e
falha nos DOIS sentidos ao mesmo tempo:

```
semente 7, braço de 23:   43 erros no total
                          11  deveria abster e agiu
                          13  deveria agir e absteve
                          ──
                          24 dos 43 erros (56%) envolvem `perguntar`
```

Ela ocupa 19% das frases do benchmark e responde por mais da metade dos erros. E
**dobrar o poço não moveu isso**, o que é evidência de que a alavanca de tamanho de
poço se esgotou para esta classe.

---

### 4.2 O gargalo real — escolher a ferramenta

**Motivação medida.**

```
argumento (condicional)   ~92%
ferramenta                ~76%   ← 16 pontos de diferença
```

Não adianta extrair melhor um argumento para a ferramenta errada.

**Alvos apontados pela análise de erro:** `abrir_programa`, `executar_comando`,
`procurar_arquivo`.

**Reordenação vinda de 4.1.** A maior família de erro não é nenhuma dessas três — é
`perguntar`, envolvida em 238 dos 447 erros (53%), nas duas direções. Ver 4.2.1.

---

#### 4.2.1 Por que a fronteira do `perguntar` é porosa — investigado em 2026-09-04

Três hipóteses testadas sobre os 447 erros das 12 sementes. Duas morreram.

| hipótese | medida | veredito |
|---|---|---|
| Colisão com o vocabulário das ferramentas | r = +0,117; 4,86 contra 4,19 falhas | **morta** |
| Poço não cobre a região (similaridade com o vizinho mais próximo) | r = −0,277; 3,54 contra 5,07 | fraca, direção certa |
| **Verbo de ação compartilhado com ferramenta real** | **7,00 contra 3,37 falhas, p ≈ 0,0096** | **viva** |

**A hipótese que sobreviveu.** Uma frase de abstenção cujo verbo de ação também
aparece nos moldes de alguma ferramenta real falha **duas vezes mais**:

```
com verbo de ferramenta real   n= 8   7,00 falhas de 12   cria explica manda faz monta resolve leva
sem                            n=19   3,37 falhas de 12
permutacao (200 mil sorteios)  p = 0,0096
```

Isso é exatamente o que o comentário dentro de `dados.rs` já previa desde uma correção
anterior: *"o modelo não aprendeu «isto está fora»; aprendeu «manda = fora»"*.

**A direção inversa confirma o mesmo mecanismo, espelhado.** 16 frases de 122
concentram **80% das 116 abstenções indevidas**, e se dividem em dois grupos:

```
verbo raro para a ferramenta          frase figurada de consulta de estado
reune tudo que se chame logo    10    esse pc ta engasgando de tanto programa   7
seleciona os arquivos de nome…  10    a maquina esta pesada de novo             7
resgata um arquivo chamado…      8    to sem nocao de tempo hoje                6
cade o documento de nome…        3    falta muito pro fim do dia                6
                                      o hd ta perto de encher                   5
```

**A conclusão.** `perguntar` não é aprendida como um conceito — é aprendida como **o
complemento das superfícies dos moldes**. Verbo familiar ⇒ ferramenta; verbo estranho
ou frase conversacional ⇒ abstenção. Ela não decide se o pedido *cabe numa capacidade
que ela tem*; decide se o pedido *se parece com o que ela viu*.

Daí por que 23 → 66 não podia funcionar: **o complemento de 19 ferramentas é
ilimitado**, e nenhum número de exemplos fora-de-escopo o cobre. Pior, há uma trava
estrutural — a regra escrita no próprio arquivo proíbe que exemplo fora-de-escopo
compartilhe verbo com ferramenta real (senão mata o pedido legítimo, o que já custou
8 abstenções falsas em 59). Ou seja: **o poço está proibido de cobrir justamente a
região onde as falhas vivem.**

**O que isso implica para 4.2.** Parar de escrever frase de abstenção. As duas saídas
que restam atacam a mesma causa pelos dois lados:

1. **Alargar os poços das ferramentas REAIS** em variedade de verbo e de frase
   figurada. Reduz as duas direções de erro ao mesmo tempo, e é o método já provado.
2. **Dar um sinal de capacidade** em vez de superfície — algo que compare o pedido com
   o que o registro sabe fazer, não com o que o gerador escreveu.

**Ressalva de método.** A heurística de "verbo de ação" foi escolhida por mim olhando
para estes mesmos dados, então o p ≈ 0,01 é otimista. Antes de virar plano, confirmar
numa régua independente (`dados/frases_john_2.txt`, `dados/teste_do_amigo.txt`) ou numa
sonda nova escrita para isso.

#### 4.2.3 Variedade de verbo — medido em 2026-09-05, NÃO confirmado

62 moldes de verbo novo nas ferramentas reais, nenhum verbo do benchmark nem das
réguas independentes. 12 sementes pareadas contra o braço `f66`.

```
                                        f66      verbos    delta      t
instrumento REGISTRADO (falsa acao)     10,00    10,00     +0,00    0,00
benchmark de 150                       112,75   114,25     +1,50   +0,98
falsa abstencao — POS-HOC               9,67     7,33      -2,33   -2,14
```

**Pelo critério pré-registrado o resultado é nulo**, e é isso que vale como evidência.

**O erro foi meu, e é de método.** Registrei como decisor a sonda de abstenção —
frases `perguntar` em que ela age — mas os 62 moldes foram para as ferramentas
**reais**, atacando a direção oposta. Pré-registrar não protege sozinho: o
instrumento tem de apontar para a mudança.

**E a predição pontual falhou.** Eu disse que, se o mecanismo fosse "quadro familiar
+ verbo novo", estas cairiam primeiro:

```
resgata um arquivo chamado chave         8 ->  8
seleciona os arquivos de nome banner    11 -> 11
reune tudo que se chame logo            12 -> 12
cade o documento de nome atestado        8 -> 10   (piorou)
```

Nenhuma se moveu. O −2,33 veio de outras frases, por um caminho que não é o que a
hipótese previa — a combinação clássica de achar um número sem achar a explicação.

**RESULTADO DA CONFIRMAÇÃO (2026-09-07), sementes 19–30, instrumento certo:**

```
                    descoberta (pos-hoc)   confirmacao (registrada)
falsa abstencao          -2,33                    -1,17
t                        -2,14                    -1,45
benchmark                                          -0,50  (t=-0,27)
```

**Não confirmado.** Mesma direção, metade do tamanho, sem significância — regressão
à média, que é o que acontece com efeito escolhido depois de olhar os dados. Os 62
moldes ficam (não custam nada em inferência), registrados como não confirmados.

~~Corrida de confirmação em andamento~~, com o instrumento certo registrado antes e
sementes NOVAS (19–30), porque confirmar no mesmo dado que gerou a hipótese não
confirma nada. Braços: código atual com e sem os 62 moldes, o que isola exatamente a
mudança.

#### 4.2.2 Uma régua vazada, achada no caminho

A frase `"e ai, como voce ta hoje"` do benchmark estava no poço de treino como
`"e ai como voce ta hoje"` — **diferença de uma vírgula**. O teste
`o_benchmark_nao_vazou_para_o_gerador` comparava string exata e deixou passar.

Ela foi a maior "melhora" do experimento 4.1: **12 falhas viraram 2**. Descontando-a:

```
                          com a vazada        sem a vazada
sonda dirigida            -0,67  (t=-0,89)    +0,17  (t=+0,23)
benchmark de 150          +2,08  (t=+1,43)    +1,25  (t=+0,85)
```

Na sonda dirigida — o instrumento que decidia — **o efeito inteiro era o vazamento**,
e sem ele o sinal muda de lado. Corrigido nos dois lugares: a frase saiu do poço, e o
teste passou a normalizar (minúsculas, só alfanumérico ASCII, espaço colapsado) antes
de comparar. Uma varredura do benchmark inteiro contra os 660 moldes achou **esta e
mais nenhuma** duplicata; 40 pares ficam entre 0,70 e 0,87 de similaridade, que é
reformulação legítima do mesmo fenômeno.

**Método** — o mesmo que funcionou três vezes seguidas, e que **não é arquitetura**:

1. Rodar `analisar_erros.py` sobre as 12 sementes, agrupar por (esperado → obtido)
2. Para cada família grande, achar a **forma de frase** que falta, não os itens
3. Medir com sonda dirigida, não com o benchmark

**Precedente que dimensiona:**

| poço | mudança | o que consertou |
|---|---|---|
| `ARQUIVOS` | 12 → 60 | `mover_arquivo` |
| `CONSULTAS` | 30 → 77 | a preposição vazada |
| moldes indiretos | +54 | a abstenção |

O princípio por trás dos três: **poço estreito ensina os itens, não a forma.**

**Arquivos:** `src/learn/dados.rs`, e as réguas em `dados/`.

---

### 4.3 ~~Canal de confirmação~~ — FEITO em 2026-09-05

**Estado atual.** A interface web nasce somente-leitura *por construção*:
`IsTerminal` é falso, logo não há como confirmar, logo as 8 ferramentas que agem
recusam. Isso é correto e não é um bug.

**Mudança.** Um canal de confirmação que não seja o terminal: um `faz? [sim/não]` na
própria página, carregando o token da execução e um **identificador de pedido**, para
que a resposta não possa ser reaproveitada para autorizar outra ação.

**Escopo:** `src/servidor.rs`, `src/tools/execucao.rs`.

**Por que vale.** É o trabalho menor da lista e é o que **libera as 8 ferramentas
restantes fora do terminal** — ou seja, o maior ganho de utilidade por hora gasta.

**COMO FICOU: dois passos, não uma espera.**

O servidor é sequencial — um `for` sobre `incoming()`, com `&mut Executor`. Bloquear
dentro do handler esperando o "sim" travaria o processo inteiro: a própria resposta
nunca seria aceita, porque não há quem aceite a conexão. Então:

```
POST /pedido     decide, GUARDA a chamada, devolve {id, chamada, segundos}
                 e nao executa nada
POST /confirmar  id + sim/nao  ->  executa, e so entao
```

Isso saiu melhor que a espera que eu tinha desenhado. A permissão deixa de ser um
instante e vira **um objeto com identidade** — o que permite ser de uso único, ter
validade, e valer para aquela chamada e nenhuma outra.

**As quatro travas, e o que cada uma impede:**

| trava | o buraco que fecha |
|---|---|
| só a pendência atual | um "sim" sem pergunta não inventa ação |
| o `id` tem de bater | um "sim" dado a `criar_pasta` autorizar o `apagar_arquivo` seguinte |
| validade de 120 s | pergunta velha respondida sem contexto |
| uso único (`take()` antes de executar) | clicar duas vezes executar duas vezes |

**Verificado contra o servidor de pé, e a prova é o efeito, não a resposta:**

```
A  id errado     -> recusa      pasta `documentos` NAO criada
B  resposta nao  -> recusado    pasta `imagens`    NAO criada
C  resposta sim  -> executou    pasta `backups`    criada
D  repetir o sim -> nada        nao duplicou
```

**O que NÃO foi afrouxado.** A permissão pula a *pergunta*, não as guardas. Dois
testes prendem isso: escrita fora da raiz e comando na lista negra continuam sendo
recusados mesmo com `ja_autorizado = true`. E `precisa_confirmar` espelha
`executar` por teste sobre o registro inteiro — se as duas saírem de sincronia, o
servidor passaria a executar direto algo que deveria perguntar.

O `bind` continua `127.0.0.1` no código, o token continua por execução, e negar
continua sendo o padrão em todo caminho ambíguo.

---

### 4.3.1 A tela virtual: o que ela resolve, e o que ela nunca vai resolver

Análise pedida pelo John em 2026-09-06, depois de uma sessão inteira medindo.
Tudo abaixo é medido, não estimado.

**O que ela FAZ, e faz bem:**

```
processo lancado la    janela nasce numa area que ninguem ve
custo                  um handle; ~0 MB   (VM daria isolamento por 1 a 4 GB)
diretorio de trabalho  chega certo (lpCurrentDirectory), provado com caminho relativo
```

**O que ela NÃO faz, e isso é definitivo:**

| não faz | prova |
|---|---|
| mover app que já está rodando | Discord: 6 processos antes, 6 depois; janela ficou na tela do John |
| conter risco | mesmo token do usuário — `del` continua apagando, rede continua aberta |
| esconder app single-instance | lançar o Discord entrega o pedido à instância existente |
| **receber tecla injetada** | `SendInput` devolve `ACCESS_DENIED` (5) num desktop escondido |

**A última linha entrou em 2026-09-09 e encerra uma ideia inteira.** A esperança era
rodar um jogo numa tela virtual e dirigi-lo lá, com a tela do John livre — a única
configuração que juntaria "personagem dele, mundo dele, PC liberado".

`examples/sonda_tela_virtual.rs`:

```
0. tela criada                    OK
1. processo lancado nela          OK
2. thread atachada no desktop     OK
3. SendInput                      0 de 2, GetLastError = 5 (ACCESS_DENIED)
```

`SendInput` só é aceito quando a thread está no desktop de **entrada** — o que recebe
o teclado físico. Escondido não é. Para virar o de entrada precisaria `SwitchDesktop`,
que **toma a tela** — o oposto do objetivo.

**Não há meio-termo: ou o desktop recebe entrada e está visível, ou está escondido e
não recebe.** Tela virtual esconde janela; não dirige nada escondido.

A sonda usa Bloco de Notas e não um jogo de propósito: desktop escondido ninguém vê,
então ela manda texto e **lê de volta pela árvore de acessibilidade** — a verificação
não depende de alguém olhar.

A segunda linha é a que mais engana. A tela virtual **não é fronteira de
segurança**; ela é uma cortina. Quem contém é a lista negra, a raiz, o
`Politica::processos` e a confirmação.

**A regra que resume:** a tela virtual serve para o que a Teka **lança**, nunca para
o que ela **encontra rodando**.

```
LANCAR (vai para a tela)        ENCONTRAR RODANDO (nao vai)
navegador para uma consulta     Discord numa chamada
instalador                      Spotify tocando
script com janela               qualquer app single-instance
o mundo do laco de pratica      qualquer coisa que o John esteja usando
```

#### O que falta para ela ser 100% confiável

Três buracos, em ordem de importância:

**1. Ela não sabe o que tem lá dentro.** Hoje a Teka lança e esquece. Existe
`EnumDesktopWindows` (provei num rascunho: listou as janelas da tela virtual,
inclusive uma que o PowerShell não enxergava porque enumera só a área de trabalho
dele). Sem isso, "abriu?" é fé.

**2. Processo órfão.** Fechar o handle do desktop não mata o que está rodando nele —
o Windows só destrói o objeto quando o último processo sai. Um programa que trava lá
fica invisível **e** vivo. Falta uma varredura que liste e encerre.

**3. Não há como saber se o que subiu é o que foi pedido.** O teste do Discord
mostrou uma janela chamada `discord` na tela virtual que era o **`cmd.exe`** —
título vindo do argumento. Conferir por PID resolveu; a ferramenta ainda não confere.

#### Onde ela ganharia mais, e ainda não está ligada

O laço de prática está com `processos: false` desde que um modelo não treinado abriu
dezenas de janelas na área de trabalho. Com a tela virtual + PID conferido,
`abrir_programa` volta a ser praticável **num mundo fechado**: o mundo escreve um
`.bat` de mentira chamado `chrome.bat`, e `abrir_programa("chrome")` resolve para
esse arquivo em vez do Chrome real do John.

Isso daria sinal de prática a uma das três famílias-gargalo do §4.2 — que hoje não
tem nenhum. **Mas só vale depois de o laço provar que serve para alguma coisa**: a
medição dele é "sem efeito, 3 sementes × 1.200".

### 4.4 Argumento de restrição múltipla — a única mudança de arquitetura justificada

**Motivação medida.**

> *"Procure na pasta de download o meu mapa de minecraft que está em um arquivo
> zipado, de menos de 30 dias"*

Três restrições; o ponteiro copia **um** span. Isso **não é falta de treino** — é o
formato da saída. Nenhum poço de valor resolve.

**Mudança.** Mais de um span por chamada. Mexe na cabeça de ponteiro, na de presença,
no formato serializado e no gerador de exemplos.

**Por que fica depois de 4.2.** Porque é cara, e porque o histórico manda: toda
mudança de arquitetura tentada até hoje rendeu ~zero. Esta é a **primeira que tem uma
medição apontando para ela** — o que a qualifica para a fila, não para a frente dela.

**Limite vizinho, que esta mudança NÃO resolve.** A API de Instant Answer da
DuckDuckGo só responde termo único de enciclopédia; frase composta volta vazia. Quem
pede conselho de configuração recebe verbete. **Nenhum treino conserta isso** — só
trocar a fonte, ou 4.5.

---

### 4.5 Fusão COMPLETA com o DeepSeek-Harness

**Decisão do John, 2026-09-02: o alvo é a fusão COMPLETA**, todas as ferramentas do
Harness, incluindo as de escrita, com treino pesado em Docker. A ponte somente-leitura
(`grep`, `glob`, `read`, `lsp`) é **etapa do caminho, não destino** — ela serve para
medir a acurácia da Teka comandando ferramenta alheia sem risco nenhum.

**A divisão natural:**

```
Teka      decide O QUE fazer e QUANDO buscar     rápida, local, 24 MB
Harness   executa e LÊ o resultado               Node, LLM atrás
```

**Não vira um binário só.** Rust sem dependência contra um monorepo Node com 36
devDeps. Vira uma pasta, dois processos, protocolo por stdio. A pasta
`Teka+DeepseekHerness/` já existe.

**O argumento a favor melhorou** e não é "mais ferramentas": é que o teto do
`buscar_web` é verbete, e o que falta para responder *"que configuração usar"* é um
LLM que **leia** o resultado.

**Estimativa: 20 a 40 sessões**, já corrigida por um otimismo medido de ~1,5x. A ponte
é trabalho conhecido; o imprevisível é **recuperar a acurácia** depois de o registro
crescer de 20 para ~32 ferramentas. Precedente que dimensiona: 10 → 19 derrubou de
108,3 para 107,0, e só voltou a 112,7 depois do conserto dos poços.

**E agora há um segundo precedente, com o preço de UMA ferramenta isolado**
(2026-09-07/08): `atalho` custou **−2,58** ao entrar, e o custo tinha dono — ela
disparando quando não devia, em frase curta e imperativa. Consertar a contradição do
fora-de-escopo devolveu **+3,17**. Ou seja: **o custo de uma ferramenta nova é da
ordem de 2 a 3 pontos, e é recuperável por trabalho de poço.** Doze ferramentas de
uma vez não escalam linearmente, mas o mecanismo agora é conhecido e o instrumento
existe.

**O estado do upstream, medido em 2026-09-07.** A cópia local está presa em
`dsh-0.1.1-rc.2` (21/08) **de propósito**, com a árvore limpa. O upstream já estava
2512 commits à frente, em `0.1.3-alpha.2` — 9727 arquivos, +514k/−191k, com 167
commits só em `core`.

**E mesmo assim os pacotes `tool-*` foram de 23 para 22**: nenhum entrou, só saiu
`subagent/tool-subagent-report`. Toda a movimentação é interna. Como a fusão fala com
as ferramentas por protocolo, **o contrato está parado** — a premissa de ~32
ferramentas no registro unido continua valendo.

Decisão do John: não subir agora. A instalação funciona (há uma pasta
`instalacao-funcionando` com lock próprio), o ganho seria zero enquanto o gargalo de
escolha estiver aberto, e subir trocaria uma linha `rc` por uma `alpha`. Quando a
fusão começar, o alvo é a tag `dsh-v0.1.2-rc.1`, com a `instalacao-funcionando`
intacta como rede.

**Um caminho lateral que apareceu em 09/09 e vale guardar:** `Mineflayer` é uma
biblioteca **Node** que controla um jogador de Minecraft por protocolo. O Harness é
um monorepo Node com LLM atrás, e o `Mindcraft` já fala DeepSeek. Se um dia interessar,
a fusão deixa de comprar só ferramentas e passa a comprar **um corpo num mundo** — que
é um lugar bem mais interessante para medir se ela decide bem do que 150 frases. Fora
do roteiro por enquanto; anotado em `Projetos/Assistente/MINECRAFT_AUTOMACAO.md`.

---

### 4.6 Importação da Nyxara — por último

**Decisão do John, 2026-09-02.** Esta é a última coisa. Depois da fusão, depois da
Teka lapidada.

A razão que ele deu: os arquivos da Nyxara são meses de trabalho, feitos com cuidado,
sem descansar direito. **Não se refazem.** Fundir coisa insubstituível num sistema que
ainda está mudando é apostar o que não dá para repor.

Isso não é sentimentalismo atrapalhando a engenharia — é a engenharia certa, e gera
uma exigência concreta e inegociável:

> **O importador LÊ a Nyxara e NUNCA escreve nela.** Nenhuma migração no lugar,
> nenhum "converte e substitui". Os arquivos dela ficam intactos e continuam
> funcionando sozinhos depois. Se a importação der errado, o custo é uma tarde da
> Teka, não a Nyxara.

**O que já está pronto do lado da Teka** — a memória semântica foi desenhada para
receber a dela, e está escrito no código:

```
Conceito + RELACIONADO_A    veio do grafo da Nyxara
superado_por                veio do SUPERSEDED_BY dela
Fonte::Importado(String)    "a migração da Nyxara cai aqui"
```

---

### 4.7 ~~Contradição no fora-de-escopo~~ — FECHADO em 2026-09-08

Quatro exemplos ensinados como `perguntar` eram pedidos que ela **atende**:

```
"aumenta o volume"      -> aumentar_volume   (é gatilho LITERAL da tabela)
"toca uma musica ai"    -> tocar_faixa
"poe um som pra tocar"  -> tocar_faixa
"quero ouvir podcast"   -> tocar_faixa
```

O primeiro é o caso puro: **a mesma string** estava no poço de `atalho` rotulada
`atalho` e no fora-de-escopo rotulada `perguntar`. Dois rótulos para uma frase — ali
o modelo não aprende a fronteira, aprende que é sorteio.

Foram escritos quando tocar música não era capacidade dela. **Capacidade nova
envelhece o fora-de-escopo antigo**, e nada percebia. Hoje
`nenhum_fora_de_escopo_e_coisa_que_ela_faz` percebe.

Trocados um por um, para o tamanho do poço não mudar junto.

```
INSTRUMENTO (registrado)   falsos `atalho`    -0,08/semente   t=-0,23
GUARDA                     benchmark de 150   +3,17           t=+2,55
```

**A hipótese registrada morreu e a intervenção funcionou** — por um caminho que o
próprio `dados.rs` já descrevia. O ganho caiu nas ferramentas REAIS
(`procurar_arquivo` −15, `escrever_arquivo` −10) e a abstenção piorou (+6). Aqueles
quatro exemplos ensinavam `poe`, `quero`, `aumenta`, `toca` **= fora de escopo**, e
esses verbos aparecem em pedido de ferramenta real. Envenenavam os verbos, não a
fronteira.

Foi a **segunda vez** que eu pré-registrei a superfície errada (a primeira foi a
direção, em 05/09). A regra que saiu: ao mudar **dado**, o primário é a medida
**ampla**; o mecanismo estreito entra como explicação, nunca como juiz.

### 4.8 O crítico prevê a própria correção — medindo em 09/09

A cabeça de crítico tinha **zero referências** em `supervisionado.rs`. Agora
`Alvo::auto_critico` a treina com alvo auto-supervisionado: *"a minha própria escolha
vai estar certa?"* — ferramenta E argumento, alvo 0/1, no mesmo passo, sem forward
extra.

Três coisas ditas antes de medir:

```
o alvo se MOVE           é a correção do modelo atual; cedo no treino quase tudo é 0
porta lateral do clip    o gradiente do crítico não vai ao tronco, mas o clipping do
                         Adam é de NORMA GLOBAL -> pode estrangular a tarefa
                         principal por fora. REGRA DE PARADA própria.
pode nao haver sinal     prever a própria correção em 1,6 M é mais difícil que
                         classificar. Nulo aqui empurra a fusão para cima da fila.
```

E a régua quase me enganou: a grade de limiares do crítico ia até 0,10, escrita para
o crítico do **reforço**, que prevê recompensa perto de zero. O crítico novo prevê
perto de 0,97 — **todo limiar abaixo de todo valor**, saldo `+0` por construção. Eu
quase li aquele zero como "a hipótese morreu". Hoje a grade sai dos próprios dados,
por quantil.

---

## 5. O que NÃO vai ser modificado, e por quê

Estas medições estão **fechadas**. Repeti-las é gastar dia por resultado conhecido.

| tentativa | resultado | `n` |
|---|---|---|
| Tronco pré-treinado em português | +2,2 de 150, t = 1,20 | 9 pares |
| 7x mais parâmetros (1,6M → 12M) | +0,6 | 3 sementes |
| Patch por entropia (dados velhos) | −0,17, t = −0,06 | 6 sementes |
| Patch por entropia (dados de hoje) | **+3,92, t = 2,06** — real, mas não paga o custo | 12 pares |
| Viés de abstenção varrido | não fecha a lacuna | 6 × 11 valores |
| Viés de fronteira no ponteiro fino | 94,3% → 88,4% | removido |
| Aprender da própria atividade (`ambiente`) | sem efeito | 3 × 1.200 |

**O patch por entropia merece nota** porque é a exceção que confirma a regra. Ele é o
efeito mais forte que uma mudança de arquitetura já produziu aqui — e mesmo assim saiu
do padrão. O motivo não é o `p` de 0,064, é o custo:

```
+45%   de patches na inferência
+80%   de tempo de TREINO
+16 MB de tabela para distribuir
```

Com dezenas de sessões pela frente, um experimento 80% mais lento é imposto composto
sobre o trabalho de dados — que rende mais. Continua disponível por
`--patcher entropia --ngrama <arq>`.

**E veio com uma correção que vale como aviso geral:** a preposição vazada ficou em
ZERO nos **dois** braços. O conserto que eu havia atribuído à entropia veio de
`CONSULTAS` 30 → 77. A entropia estava **mascarando uma falha de dados**; corrigido o
dado, o mérito mudou de dono.

> Ganho de arquitetura medido sobre dado ruim pode ser o dado ruim, não a arquitetura.

---

## 6. Riscos

| risco | impacto | mitigação |
|---|---|---|
| Ler resultado parcial de experimento | conclusão falsa, já ocorreu 3x | `n` fixado por escrito antes de rodar |
| Régua vazar para o gerador | benchmark mede a si mesmo | 2 testes; já pegou 3 frases vazadas |
| Crescimento do registro derrubar a acurácia | fusão fica pior que o estado atual | medir antes/depois; precedente 10→19 dimensiona |
| Importar Nyxara cedo demais | perda irreversível | ordem fixada; importador somente-leitura |
| Régua escrita por quem conhece o projeto | ponto cego estrutural | teste de terceiro de propósito |

O último risco não é teórico: **dez perguntas de um amigo do John acharam o limite de
comprimento de frase que nem as 150 nem as 500 acharam** — mediana de 106 bytes contra
uma janela de treino de 64 e uma mediana de gerador de 37. Ele achou porque não sabia
o que ela aceita.
