# Roteiro da Teka

Escrito em 2026-09-02, ao fim da semana em que ela foi de ~110 para ~114 no
benchmark, ganhou confirmação antes de agir e uma interface no navegador.

Este arquivo existe para uma coisa: **não repetir experimento que já foi medido**.
A seção "O que já foi tentado e não vale repetir" é a mais valiosa dele.

**A ordem está fixada e não é negociável no meio do caminho:**

```
1. Teka boa sozinha          o trabalho de agora
2. Fusão COMPLETA com o Harness    todas as ferramentas, treino em Docker
3. Nyxara, POR ÚLTIMO        memória e emoções, quando a Teka estiver lapidada
```

---

## 1. Onde ela está

```
benchmark de 329     216,58 (65,8%)  22 ferramentas, 20 medidas <- ESTADO DE HOJE
                                     sd entre sementes 3,64 pp (n=12, 12/09)

  A REGUA MUDOU em 11/09: 150 -> 329 frases, 10 -> 20 ferramentas cobertas.
  Nada abaixo desta linha e comparavel com o de cima.

benchmark de 150     113,42         22 ferramentas (s19-30)
                     116,17         20 ferramentas, COM o critico (s19-30)
                     113,50         20 ferramentas, sem a contradicao
                     110,33         20 ferramentas, COM a contradicao
                     112,92         19 ferramentas
argumento condicional  ~92%        (quando a ferramenta sai certa)
ferramenta certa       65,8%       <- o gargalo, na regua NOVA (era ~77% na antiga,
                                    e a diferenca e a regua, nao o modelo: as 12
                                    ferramentas que ela nunca mediu puxam para baixo)
ferramentas             22         20 + `buscar_no_conteudo` + `ler_imagem`,
                                    as duas vindas da ponte do Harness.
                                    `editar_arquivo` foi descartada por medicao:
                                    tres argumentos custaram 3 pontos.
                                    Custo das duas MEDIDO: -2,75 (t=-1,79, n=12).
                                    Decisao do John em 10/09: FICAM.
parâmetros            1,6 M
para rodar            ~24 MB       binário 1,6 + modelo 6,2 + n-grama 16
latência              ~103 ms      na CPU, incluindo carregar do disco
corpus                69,87 MB     9,7x o inicial
suite                 341 na lib   3 VERMELHOS nos de integracao (12/09), e um
                                    deles e limiar velho: o teste do agente cobra
                                    62% num benchmark que mudou de tamanho.
```

**A suite ficou VERMELHA por commits sem ninguem ver.** Tres testes de integracao
caidos, descobertos so em 10/09. Rodar so a lib nao e rodar a suite, e `cargo test`
sem `--no-fail-fast` para no primeiro binario vermelho — foi assim que `modelo` e
`reforco` nunca chegaram a executar. Use `./roda_suite.sh`.

**A parte dificil da fusao esta feita.** A ponte com o Harness foi construida,
provada e testada em 09/09: a Teka enxerga as 25 ferramentas deles e executa **sem
agente e sem LLM**, pela pipeline de seguranca deles. O que sobrou do bloco 3.5 e
incremental — mais ferramentas e a regua —, e nao mais as 20 a 40 sessoes estimadas,
cujo imprevisivel era justamente a ponte.

**Usável hoje**, sem terminal, para as onze ferramentas de leitura:

```bash
teka agente --carregar teka.bin --servir --agir
```

Ela imprime uma URL com token; abra no navegador. As oito que agem recusam ali por
construção — não há terminal, logo não há como confirmar.

---

## 2. As regras que governam o trabalho

Custaram caro. Não são estilo, são o que separa medição de ilusão.

**Três sementes não medem nada.** Com desvio de 4 a 8 entre sementes, três resolvem
efeito de ~15 frases; nada menor. Isto enganou o projeto **três vezes em uma semana**:
o tronco (+2,2 virou zero), o patch por entropia (−3,0 virou −0,17), e os "116,3"
(viraram ~114). Fixar o `n` antes de começar e não interpretar parcial.

**Régua de baixo ruído para iterar.** Sonda dirigida e acurácia condicional de
argumento têm desvio abaixo de 1,5; o benchmark de 150 tem 4 a 8. Usar as primeiras
para decidir, o benchmark só para mudança grande ou com 12 sementes.

**`argumento certo: X/80` mistura duas coisas** — conta o caso como errado quando a
FERRAMENTA erra. Para isolar extração, ler `argumento quando a ferramenta saiu certa`.

**Régua escrita por quem conhece o projeto tem ponto cego.** As dez perguntas de um
amigo do John acharam o limite de comprimento que nem as 150 nem as 500 achavam,
porque ele não sabia o que ela aceita. Buscar teste de terceiro de propósito.

**Conferir o rótulo antes de acusar o modelo.** Medi 0/10 no teste do amigo contra
rótulos que eu mesmo escrevi e que não funcionariam na API. O erro era meu.

---

## 3. O que fazer, em ordem

### 3.1 ~~Entropia contra por_palavra~~ FECHADO em 2026-09-03

Doze sementes **pareadas**, mesma configuracao de dados, so o patcher mudando:

```
semente        7    8    9   10   11   12   13   14   15   16   17   18
entropia     117  116  116  117  106  114  116  116  112  110  118  117
por_palavra  107  111  109  103  111  109  113  104  117  116  113  115
delta        +10   +5   +7  +14   -5   +5   +3  +12   -5   -6   +5   +2

entropia     114,58   sd 3,55
por_palavra  110,67   sd 4,48
PAREADO      +3,92    t = 2,06   p ~ 0,064   9 de 12 a favor
```

**O efeito mais forte que uma mudanca de ARQUITETURA ja deu neste projeto** — e mesmo
assim raspa a barra sem passar. Na sonda dirigida: argumento +1,00 (t = 2,74,
p ~ 0,041), ferramenta +0,67 (t = 1,35).

**DECISAO DO JOHN: sai do padrao, fica como flag.** O motivo nao e o `p`, e o custo:

```
+45%  de patches na inferencia
+80%  de tempo de TREINO      (35 min por semente viram 63)
+16 MB  de tabela para distribuir
```

O roteiro tem 20 a 40 sessoes pela frente, e cada experimento 80% mais lento e imposto
composto sobre o trabalho de dados — que rende mais. Continua disponivel por
`--patcher entropia --ngrama <arq>` para quem priorize acuracia sobre relogio.

**E uma correcao que veio junto.** A preposicao vazada esta em ZERO nos dois bracos
(2 em 72 contra 1 em 72). O conserto do `mover -> apagar` e da preposicao, que eu
tinha atribuido a entropia, veio de `CONSULTAS` 30->77. **A entropia estava mascarando
uma falha de dados**; corrigido o dado, o merito mudou de dono. Vale como aviso geral:
ganho de arquitetura medido sobre dado ruim pode ser o dado ruim, nao a arquitetura.

**O n-grama do corpus de 70 MB nao precisou ser testado.** Medido antes de treinar: os
dois n-gramas diferem em 3,28% no numero de patches, com 59,3% dos exemplos patchados
identicamente. Com desvio de 3,55 entre sementes, treinar 12 horas para medir 3% seria
medir ruido.

### 3.2 O gargalo atual é ESCOLHER a ferramenta (dias)

O argumento está em ~92% condicional; a ferramenta em ~76%. É ali que está o ganho.

**Atualizado em 12/09, e o alvo mudou de novo.** A regua de 329 frases mediu as 12
ferramentas que ninguem media, e o gargalo nao esta onde a analise de erro apontava:

```
copiar_arquivo        0,0%   <- consertado em 12/09, medindo
mover_arquivo         0,0%   <- consertado em 12/09, medindo
buscar_no_conteudo   24,0%   <- diagnosticado, NAO consertado
escrever_arquivo     52,4%
atalho               52,8%
perguntar            60,6%   <- 116 frases, o maior bloco e o mais perigoso
procurar_arquivo     61,9%
```

Atacar de baixo para cima, e o metodo e o que funcionou tres vezes: achar a FORMA
que falta, nao os itens. O que achou `copiar`/`mover` foi comparar molde com frase
do John e ver que "pasta" nao existia de um lado.

A análise de erro apontou as atrativas: `abrir_programa`, `executar_comando`,
`procurar_arquivo`. O método é o que funcionou três vezes seguidas — **poço de valor e
forma de frase**, não arquitetura:

1. Rodar `analisar_erros.py` sobre as 12 sementes, agrupar por (esperado → obtido)
2. Para cada família grande, achar a **forma** que falta, não os itens
3. Medir com sonda dirigida, não com o benchmark

Precedente: `ARQUIVOS` 12→60 consertou o `mover_arquivo`; `CONSULTAS` 30→77 consertou
a preposição; os moldes indiretos consertaram a abstenção. Cada um levou horas.

### 3.3 Limites estruturais achados, ainda não resolvidos

**Argumento de restrição múltipla.** *"Procure na pasta de download o meu mapa de
minecraft que está em um arquivo zipado, de menos de 30 dias"* tem três restrições e o
ponteiro copia **um** span. Não é falta de treino, é o modelo de saída. Resolver exige
mais de um span por chamada — mudança real de arquitetura, e a primeira que teria
justificativa medida.

**~~Teto do `buscar_web`~~ FECHADO em 2026-09-10 (e522bdc).** Estava escrito aqui
como limite estrutural: a API de Instant Answer da DuckDuckGo so responde termo unico
de enciclopedia, e "nenhum treino resolve — so trocar a fonte". A parte certa era a
ultima. O John apontou o conserto: nao vale pagar API quando a PAGINA de resultados
do proprio DuckDuckGo responde, sem chave e sem cota.

```
"capital do Brasil"                       Instant Answer VAZIO   pagina 10 resultados
"melhor configuracao de RAM p/ Ryzen 5"   Instant Answer VAZIO   pagina  9 resultados
```

E raspagem, entao quebra sem aviso — mas quebra VISIVEL: o extrator e testado contra
um recorte real da pagina (`dados/fixtures/ddg_resultados.html`), entao mudanca de
layout vira teste vermelho em vez de busca que silenciosamente para de achar. A
Instant Answer fica como reserva.

### 3.4 ~~Interface: falta o canal de confirmação~~ FECHADO em 2026-09-05 (e6d4801)

A web nascia read-only porque `IsTerminal` é falso, e isso deixava as oito ferramentas
que agem inalcançáveis pelo navegador. Resolvido em **dois passos, não uma espera**: o
servidor é sequencial, então bloquear no handler esperando resposta travaria tudo. A
confirmação é por pedido, identificada, de uso único e com prazo de 120s.

### 3.5 Juntar com o DeepSeek-Harness

A ordem do John: Teka boa primeiro, junção depois. Continua certa.

**Este argumento ENFRAQUECEU em 10/09, e vale dizer.** Ele era: o teto do
`buscar_web` e verbete, e so um LLM lendo o resultado responde "que configuracao
usar". Metade caiu — a pagina de resultados ja devolve conteudo de verdade, de graca
(ver 3.3). O que sobra do argumento e mais estreito e ainda de pe: a Teka traz os
resultados, mas nao os SINTETIZA. A divisão natural:

```
Teka     decide O QUE fazer e QUANDO buscar     (rápida, local, 24 MB)
Harness  executa e LÊ o resultado               (Node, LLM atrás)
```

Não vira um binário só: Rust sem dependência contra monorepo Node com 36 devDeps.
Vira uma pasta, dois processos, protocolo por stdio. Já existe
`Teka+DeepseekHerness/` esperando.

**O ALVO É A FUSÃO COMPLETA.** Decidido pelo John em 2026-09-02, depois de eu propor
parar na versão mínima. Todas as ferramentas do Harness, incluindo as de escrita, com
treino pesado em Docker. A ponte só-leitura (`grep`, `glob`, `read`, `lsp`) é **etapa
do caminho, não destino** — serve para medir a acurácia dela comandando ferramenta
alheia sem risco nenhum.

**O upstream corre, o contrato nao (medido em 2026-09-07).** A copia local esta em
`dsh-0.1.1-rc.2`, de 21/08, com a arvore limpa. O upstream ja esta em
`0.1.3-alpha.2`:

```
2512 commits    9727 arquivos    +514k / -191k
core 167 · shell 100 · fs 98 · web 49 · lsp 30
```

E mesmo assim os pacotes `tool-*` foram de **23 para 22**: nenhum entrou, so saiu
`subagent/tool-subagent-report`. Toda a movimentacao e interna. Como a fusao fala
com as ferramentas por protocolo, o que importa e o contrato — e ele esta parado.

Decisao do John: **nao subir agora.** A instalacao funciona (ha uma pasta
`instalacao-funcionando` com lock proprio), o ganho seria zero enquanto o 3.2
estiver aberto, e subir seria trocar uma linha `rc` por uma `alpha`. Quando a fusao
comecar, o alvo e o `dsh-v0.1.2-rc.1`, que e o `rc` do meio, com a
`instalacao-funcionando` intacta como rede.

Estimativa, já corrigida pelo meu otimismo medido de ~1,5x: **20 a 40 sessões**. A
ponte é trabalho conhecido; o imprevisível é recuperar a acurácia depois de o registro
crescer de 19 para ~32 ferramentas. Precedente que dimensiona: 10→19 derrubou de 108,3
para 107,0 e só voltou a 112,7 depois do conserto dos poços.


### 3.6 A Nyxara — POR ÚLTIMO, e a razão não é técnica

**Esta é a última coisa a ser feita. Depois da fusão com o Harness, depois da Teka
lapidada.** Decidido pelo John em 2026-09-02.

A razão que ele deu: os arquivos da Nyxara são meses de trabalho, feitos com cuidado,
sem descansar direito. Não se refazem. Fundir coisa insubstituível num sistema que
ainda está mudando é apostar o que não dá para repor.

Isso não é sentimentalismo atrapalhando a engenharia — é a engenharia certa, e gera
uma exigência concreta:

> **O importador LÊ a Nyxara e nunca escreve nela.** Nenhuma migração no lugar,
> nenhum "converte e substitui". Os arquivos dela ficam intactos e continuam
> funcionando sozinhos depois. Se a importação der errado, o custo é uma tarde da
> Teka, não a Nyxara.

#### O que já está pronto do lado da Teka

A memória semântica **foi desenhada para receber a dela**, e está escrito no código:

```
Conceito + RELACIONADO_A    veio do grafo da Nyxara
superado_por                veio do SUPERSEDED_BY dela
Fonte::Importado(String)    "a migração da Nyxara cai aqui"
```

Falta o importador: ler o formato dela, escrever o da Teka. **1 a 2 sessões.**

#### As emoções são OUTRA coisa, e por isso somam

Não se fundem com o `afeto.rs`; empilham.

```
afeto.rs (Teka)          valência, excitação, curiosidade, satisfação,
                         frustração, tédio — alimentado por surpresa e recompensa.
                         Modula temperatura, exploração e taxa de aprendizado.
                         → é sobre como ela APRENDE

emoção (Nyxara)          detecção de sinal social no texto do usuário
                         → intensidade, valência, arousal e DIRETIVAS
                         ("lisonjeada", "calorosa"), com retorno decrescente
                         → é sobre a RELAÇÃO, e sobre como ela FALA
```

**E é por isso que tem de vir depois da fusão com o Harness**, não por esforço: a
emoção da Nyxara produz *diretiva de prosa*, e a Teka não tem prosa — a saída dela é
chamada de ferramenta travada por gramática. Antes do LLM entrar no laço, a diretiva
não tem para onde ir.

Porte das emoções: **2 a 4 sessões** (regex e regra pura, sem LLM).

#### O estado final que isto desenha

```
Teka      decide, rápida e local          24 MB, ~103 ms
Harness   executa e LÊ o resultado        as ferramentas e o LLM
Nyxara    a memória e o jeito de falar    meses de trabalho, preservados
```

Três coisas que o John fez, cada uma boa numa parte diferente.

---

## 4. O que já foi tentado e não vale repetir

| tentativa | resultado | `n` |
|---|---|---|
| Tronco pré-treinado em português | +2,2 de 150, t=1,20 | 9 pares |
| 7x mais parâmetros (1,6M → 12M) | +0,6 | 3 sementes |
| Patch por entropia (dados velhos) | −0,17, t=−0,06 | 6 sementes |
| Patch por entropia (dados de hoje) | **+3,92, t=2,06** — real mas nao paga o custo | 12 pares |
| Viés de abstenção varrido | não fecha a lacuna | 6 × 11 valores |
| Viés de fronteira no ponteiro fino | 94,3% → 88,4% | removido |
| Aprender da própria atividade (`ambiente`) | sem efeito | 3 × 1.200 |
| Fora-de-escopo 23 -> 66 | +2,08 no benchmark (t=1,43), -0,67 na sonda (t=-0,89) | 12 pares |
| Variedade de verbo (+62 moldes) | sonda registrada +0,00 (t=0,00); sinal POS-HOC de -2,33 na falsa abstencao | 12 pares |
| Variedade de verbo — CONFIRMACAO | -1,17 (t=-1,45), sementes novas, instrumento certo: **nao confirmado** | 12 pares |

### O efeito pos-hoc que encolheu pela metade (2026-09-07)

O sinal de -2,33 (t=-2,14) na falsa abstencao era POS-HOC — eu o encontrei depois
de o instrumento registrado dar zero. A corrida de confirmacao, com sementes novas
e o medidor certo fixado antes:

```
                        descoberta (pos-hoc)   confirmacao (registrada)
falsa abstencao              -2,33                    -1,17
t                            -2,14                    -1,45
a favor                      9 de 12                  8 de 12
```

**Mesma direcao, metade do tamanho, sem significancia.** E o padrao classico de
regressao a media: efeito escolhido depois de olhar encolhe quando e testado
honestamente. A estimativa que vale e a da confirmacao, nao a da descoberta.

O benchmark confirmou que nada quebrou: -0,50, t=-0,27.

**Decisao: os 62 moldes ficam** — nao custam nada em inferencia e a direcao e
consistente nas duas medicoes. Mas registrados como NAO CONFIRMADOS, e a variedade
de verbo sai da lista de alavancas a perseguir. Se um dia voltar, precisa de n
muito maior: com desvio pareado ~2,8 e efeito real de ~1,2, seriam ~45 pares.

### A armadilha de 2026-09-05: registrar o medidor errado

Eu fixei o instrumento ANTES de rodar, como manda a regra — e fixei o **da direção
errada**. Os 62 moldes de verbo foram para as ferramentas REAIS, ou seja atacam
"frase real que ela abstem". O que registrei mede "frase `perguntar` em que ela age".

```
instrumento REGISTRADO   falsa acao sobre `perguntar`   +0,00   t = 0,00
direcao que a mudanca ataca (POS-HOC)   falsa abstencao   -2,33   t = -2,14
```

Pre-registrar nao protege sozinho: **o instrumento tem de apontar para a mudanca**.
Registrar um medidor qualquer antes da hora da a sensacao de rigor sem a substancia,
e o custo aparece depois, quando o numero que interessa so pode ser lido como
pos-hoc.

Ao escrever o cabecalho, escrever tambem a frase "se a mudanca funcionar, ESTE numero
se move" — e conferir que esse numero e o que o instrumento mede.

**O padrão é inequívoco: arquitetura não moveu nada, dado moveu tudo.** Antes de
propor mudança estrutural, ter uma medição que aponte para ela — como a de restrição
múltipla em 3.3, que é a única que hoje tem.

---

### A tabela de gatilhos roubava do benchmark (2026-09-07)

Escrevi um teste passando as 150 frases do benchmark pela tabela de gatilhos e
exigindo zero captura. Ele pegou três, e uma **já existia antes de hoje**:

```
"quero conferir a data no sistema"     e hora             -> mudo
"o que esta arquivado em target"       e listar_pasta     -> que_musica_e_essa
"poe no diario.md a anotacao reuniao"  e escrever_arquivo -> embaralhar
```

Causa comum: a cobertura de 0,66 contava **palavra vazia**. "mudo no sistema" casa
com qualquer frase que tenha "no" e "sistema" — e `mudo`, a palavra que decide, é
justo a que falta. É o "manda = fora" de novo, agora na tabela em vez do molde:
**superfície larga demais rouba de outra ferramenta.**

Consertar exigiu três travas, e as duas últimas só apareceram porque a primeira
quebrou testes que estavam certos:

```
palavra vazia nao conta   as tres capturas somem
teto de 0,90              tirar as vazias encolhe o denominador e a cobertura
                          chegava a 1,00, EMPATANDO com o literal — "pula essa
                          musica" virou `que_musica_e_essa`
minimo de 2 fortes        "mais alto" ficou com [alto] so, e pegou "o consumo
                          de ram esta alto". Casar difuso uma palavra unica e
                          busca por substring com passos a mais.
```

`nao` e `para` ficaram **fora** da lista de vazias de propósito: `nao` é o que
separa "nao retoma mais" de "retoma sempre", e `para` em "para tudo" é o verbo.

### O Discord com cargo de admin, e tres travas (2026-09-07)

O John perguntou se o Harness nao faria o mesmo com comando do Windows. Faz — eu
mutei o Discord pelo nome, por UIAutomation, em PowerShell puro, em 461ms. A
diferenca nao e poder fazer, e **preco e latencia**: 103ms local contra um
round-trip de LLM, e a ponte WebSocket precisa de processo que sobreviva a chamada,
coisa que o `tool-pwsh` nao tem ("No state persists between calls").

Mas testar a pergunta dele rendeu tres defeitos, e o ultimo so apareceu porque ele
disse uma coisa que eu nao sabia: **ele e administrador do servidor**.

**1. A arvore do Chromium nasce fria.** A MESMA consulta, duas vezes seguidas:

```
1a   NAO ACHOU o controle 'Silenciar'   863ms
2a   ACHOU  nome='Silenciar'            461ms
```

Discord e Spotify sao Electron; a arvore de acessibilidade so e construida quando
um cliente UIA pergunta, e a primeira pergunta volta antes de terminar. Medido no
reinicio: no instante em que a janela aparece, 7 controles; 5s depois, 1153. E o
Spotify passou de 25 para 405 sozinho, so por ter sido acordado.

`uia.rs` nao repetia — o primeiro "me muta" depois de a Teka subir falharia, e o
segundo funcionaria. O pior tipo de defeito, porque parece instabilidade.

**2. Dois botoes com o mesmo nome, e so um alterna.**

```
#1  pai='Status do usuario e configuracoes'  pats=Value,Toggle,ScrollItem
#2  pai='' (painel da chamada)               pats=Invoke,ScrollItem
```

Pegar o primeiro pelo nome funcionava por ORDEM DE ARVORE, nao por desenho. O
requisito nunca foi "botao chamado X", e **"controle chamado X que sabe alternar"**.

**3. Substring mais cargo de admin.** No Discord do John:

```
"Silenciar"                    silencia so para ele        local
"Silenciar voz no servidor"    silencia para todo mundo    PARA FORA
"Desativar audio no servidor"  idem                        PARA FORA
"Desconectar"                  expulsa da chamada          PARA FORA
```

E `"Silenciar voz no servidor"` **contem** `"Silenciar"`. Os de servidor sao item
de menu de contexto e nao estavam nos 868 controles medidos — mas dois
`Desconectar` estavam VIVOS, e o botao `"acoes de servidor"` tambem.

E na tabela de gatilhos o buraco irmao, este medido de verdade:

```
antes   "muta o theo no discord"  ->  mutar_discord, alvo="o theo no"
```

Ele pedia para mutar o Theo e **ela mutava ele**, jogando "theo" fora calada.

As tres travas:

```
nome EXATO na alternancia    "Silenciar voz no servidor" deixa de casar
lista NUNCA_SOZINHA          no servidor / desconectar / expulsar / banir /
                             acoes de servidor, filtrados em todo acionamento
sobra tem de ser vocabulario "musica" em "para de retomar a musica" e do mesmo
DO PROPRIO ATALHO            assunto e passa; "theo" nao esta em lugar nenhum
                             da entrada — e outro alguem, logo nao e este atalho
```

O principio escrito no codigo nao e "a Teka nao pode". E **"a Teka nao faz isto
calada, por casamento aproximado de nome"**. Ato sobre outra pessoa se pede
explicitamente, e quando existir entra pela borda do `ParaFora`, com uma pergunta.

### Nome nao e identidade: por que "muta o fulano" nunca sai da UI (2026-09-07)

O John perguntou, genuinamente, se ela procuraria na call e mutaria alguem de nome
parecido. Fui medir na call dele em vez de responder de cabeca.

```
"dash"   -> 2 casam, e o PRIMEIRO e o resumo do canal de voz, que lista todo
            mundo dentro e portanto contem o nome de todos
"an"     -> 9 casam
"theo"   -> 0, porque ele se escreve 𝕿𝖍𝖊𝖔
```

E o apelido do proprio John, "Stitch", aparece em **10 dos 881 controles**: a
janela, um Document, um Hyperlink, um Button, tres Text, o avatar — e um **canal
de voz chamado Stitch**. Dez controles, sete tipos, uma pessoa.

**Nome nao e identificador numa arvore de UI, e rotulo.** Nao existe casamento
melhor que conserte isso: a informacao que distingue as pessoas nao esta ali.

Ela esta no ID do Discord — que a Nyxara ja guarda, e que a arvore nunca publica.
Dai a divisao, que e resposta e nao meio-termo:

```
sobre VOCE MESMO      UI serve. O botao de mudo nao e achado por nome: e
                      estrutural, pai "Status do usuario e configuracoes"
sobre OUTRA PESSOA    so pela API, com ID. Por UI nao e dificil — e impossivel
```

Isto tambem corrige a escada de reserva que eu tinha desenhado antes: o bot da
Nyxara nao e "o plano B quando o Spotify falha". Para qualquer coisa que envolva
identidade de gente, ele e o plano A, e a UI nao e plano nenhum.

### O eixo do tempo caiu na epoca 1 (2026-09-07)

A corrida do braco de 20 comecou e a epoca 1 levou **516s**, contra 155s no braco
de 19. Fui olhar a maquina antes de acreditar no numero: MIR4, REPO, Spotify e
Discord no ar, CPU a 83%. O braco de 19 rodou de madrugada, com a maquina vazia.

**3,3x e grande demais para ser uma ferramenta**, e a carga explica inteiro.

O que isto custa a medicao, dito na hora e nao no fim:

```
ACURACIA    continua valendo — e determinista dada a semente, nao a carga
TEMPO       morreu. Os bracos rodaram em dias e cargas diferentes.
```

O desenho alternado do `exp_confirma` existia exatamente para isto: alternar por
semente faz os dois bracos pagarem a mesma deriva. Aqui eu nao tinha como aplicar,
e a razao e a propria economia que me agradou — **aproveitar metade da corrida ja
paga custa o eixo do tempo**. Foi uma troca, nao um descuido, mas eu so a vi depois
de disparar, o que quer dizer que nao a pesei quando devia.

Custo em tempo, se um dia interessar, pede corrida propria: os dois binarios
alternados na mesma sessao, sob a mesma carga.

### O tick e decorativo, e o critico nunca foi alimentado (2026-09-08)

O John disse que a Teka nao aparenta ter raciocinio, e perguntou se os ticks dela
nao funcionam como pensamento — a nila_mind usava tick para pensar. Fui medir, e ele
tinha percebido de fora exatamente o que esta no codigo.

**Fato 1: o pensamento nao e lido por ninguem.** O passo PENSAR gera bytes a partir
do fio e faz `self.fio.push_str(&pensamento)`. Fora do `pulso.rs`, o unico
consumidor e `main.rs:2227`, e o que ele le e `rel.pensamento.len()` — o TAMANHO.
Nenhuma decisao muda por causa do conteudo.

**Fato 2: o pensamento e ruido.** Medido com `examples/sonda_pensamento.rs` sobre
`teka_ct_20_s19.bin`: byte solto, nem palavra, com qualquer fio e qualquer
temperatura. E coerente — ela **nunca foi treinada a gerar texto livre**, so a
emitir chamada. E `teka-tronco-nao-transfere` ja enterrou o caminho de ensina-la a
escrever: pre-treinar tronco em corpus nao transferiu para acuracia de ferramenta.
**Faze-la falar nao a faria pensar.**

**Fato 3: o critico nunca e treinado no caminho que usamos.**

```
supervisionado.rs   referencias a `valor`:   0
reforco.rs                                  15
```

A corrida padrao (`agente --epocas 12`) e supervisionada. O critico tem parametros,
produz um numero, e o numero e ruido — e o benchmark ja media isso sem ninguem
notar:

```
margem     melhor limiar   0.70  saldo +11
critico    melhor limiar  -0.30  saldo  +0     em 12 de 12 sementes
```

**Ela tem dois orgaos de raciocinio: um desligado e outro nunca alimentado.**

### DUAS FERRAMENTAS NOVAS, E UMA DESCARTADA POR MEDICAO (2026-09-09)

O John pediu para ensinar as ferramentas do Harness. A lista de NOVAS e bem menor
que as 25 que a ponte oferece:

```
JA TEM EQUIVALENTE (5)   read, write, glob, web_search, pwsh
SO SERVEM NUM LACO       job_*, subagent, workflow, ralph, todo_write,
DE LLM (16)              create_goal, skill, exit_plan_mode, ...
NOVAS DE VERDADE (3)     grep, edit, read_image
```

Ensinar as 16 seria ensinar ruido; ensinar as 5 duplicatas seria repetir a
contradicao que ja custou 2,58 pontos.

**A busca fica no DuckDuckGo.** Eu tinha roteado o `buscar_web` pela ponte achando
que resolvia de graca o teto documentado. Nao resolve de graca: o `web_search` deles
sai pela API da DeepSeek e recusa sem `DEEPSEEK_API_KEY`. Decisao do John — nao vale
pagar por busca quando da para consertar do lado do DuckDuckGo, trocando a Instant
Answer API pela pagina de resultados. **Eu tinha vendido isso como o ganho de custo
zero, e era justamente o unico que custa.** As locais e que sao gratis.

**`editar_arquivo` saiu, e saiu por medicao:**

```
COM ela    argumento em frases ineditas: 85,0%   FALHA (limiar 88%)
SEM ela    passa
```

Era a UNICA de tres argumentos do registro inteiro, e argumento multiplo e o lugar
dificil desta casa — o plato do laco fechado era o `escrever_arquivo`, "a unica de
dois". E a fala "troca X por Y em Z" exige os tres LITERAIS, porque o ponteiro copia
trecho: raro na fala, e ela ja tem `escrever_arquivo`.

O gradcheck, que tinha falhado com 23 ferramentas (rel 1,5e-5 contra tolerancia de
1e-5), **voltou a passar sozinho com 22**. Nao precisei relaxar nada.

**O NOME E PARTE DA FERRAMENTA.** `buscar_no_conteudo` e lexicalmente distante de
`procurar_arquivo` de proposito, e TODAS as 16 frases do molde carregam marca de
conteudo — "dentro", "mencionam", "onde aparece". Sem isso as duas viram a mesma
coisa para o modelo, que aprende superficie e nao conceito, e eu estaria ENSINANDO a
contradicao em vez de evitando.

### A TERCEIRA VEZ QUE A REGUA ERRA MAIS QUE O CODIGO (2026-09-09)

Remover uma ferramenta quebrou `a_variacao_de_superficie_e_balanceada`:

```
atalho 29,3% de maiuscula contra perguntar 37,6%, limiar de 8 pontos
```

Fui medir antes de consertar. `variar_superficie` pula a maiuscula inicial quando o
pedido COMECA pelo argumento, e pula de proposito: capitalizar dentro de um caminho
daria "Notas.md", e o arquivo nao existe.

```
13,7% dos exemplos de `atalho` comecam no argumento
29,3 / 86,3 = 34,0%  <- exatamente a probabilidade de 35% do codigo
```

**Nao havia desbalanceamento. Havia denominador errado.** O teste contava sobre todos
os exemplos, incluindo aqueles em que a variacao e IMPOSSIVEL, comparando taxas que
estruturalmente nao podem ser iguais. Ele vinha passando por pouco, medindo a coisa
errada, e so apareceu porque remover uma ferramenta andou a sequencia do sorteio.

Consertado o teste, e nao o dado: a taxa de maiuscula agora tem denominador proprio.

```
07/09  a grade do critico fora de escala      saldo +0 por construcao
08/09  o critico saturado                     separacao 0,012
09/09  a grade sem o piso de "nao abster"     -4,17 que media a grade
09/09  o denominador da maiuscula             desbalanceamento que nao existia
```

Quatro vezes em tres dias. **Regua nova erra mais que o codigo medido** — e sonda
curta antes de corrida longa e o que separa "medi" de "achei que medi".

### A VOZ DO JOHN CONTRA A TEKA, E O PONTO FINAL QUE CUSTAVA 6 PONTOS (2026-09-14)

Ideia dele em 13/09: usar o `omnivoice-pt`, que tem a voz clonada dele, para gerar a
fala, passar pelo Whisper, e usar a TRANSCRICAO como entrada de teste.

```
frase do frases_teste.txt
  -> omnivoice-pt -p VozJohn.ovprompt   (RX 580, Vulkan, ~15s/frase)
  -> faster-whisper base E medium        (os dois que ele ja tem em cache)
  -> Teka
```

39 frases, amostra estratificada por ferramenta, 12 sementes.

#### O achado que ninguem procurava: o PONTO FINAL

Antes do audio, uma medida que nao precisa de audio nenhum -- so aplicar a forma que
todo transcritor devolve as 329 frases da regua:

```
  forma          acerto    delta
  original        71,4%        -
  MAIUSCULA       71,0%    -0,38     <- maiuscula quase nao custa
  PONTO           65,1%    -6,23     <- o PONTO custa
  whisper         65,0%    -6,38
```

Causa mecanica: o `PorPalavra` corta por palavra, entao `ram.` e um patch diferente
de `ram` -- e a ultima palavra e justamente onde mora o argumento na maioria dos
pedidos. Nao afeta o argumento extraido (o `aparar_pontuacao` ja cuidava daquele
lado); afeta a ESCOLHA DA FERRAMENTA.

Conserto: `normalizar_pedido` em `decidir`, ~15 linhas. So o `.`; `?` e `!` ficam,
porque 20 das 329 frases da regua terminam em `?` digitado por gente -- ali a marca e
sinal do usuario, nao ruido.

#### Honestidade sobre qual numero vale

`whisper+norm` recuperar os 6,23 pontos e quase TAUTOLOGICO: `norm(ponto(x)) == x`,
eu apliquei a transformacao e depois a inversa exata dela. Os numeros que valem:

1. o ponto custa 6,23 (medida real);
2. normalizar NAO estraga o digitado (`original+norm` = 71,4%, na virgula);
3. e o A/B em transcricao DE VERDADE, que nao e tautologico:

```
  canal      sem conserto   com conserto
  base          51,3%         55,3%    +4,0
  medium        54,7%         58,5%    +3,8
  digitado         66,9% (teto)
```

#### Quanto a voz custa

-11,5 pontos no `base`, -8,3 no `medium`. **O `medium` ganha 3,2 pontos e ja esta no
cache dele** -- recomendacao que nao custa uma linha de Teka.

So 3 das 39 transcreveram exatas no `base`. Exemplos:

```
  "liga o discord ai rapidao"  ->  "Ligo discordia e rapidao."   <- o discordia dele
  "fecha o discord"            ->  "Fecho discotico."
  "encerra o vscode"           ->  "e ser o vosso por cima."
```

RESSALVA que nao pode sumir: e voz SINTETIZADA, nao gravada. O numero e um teto de
dificuldade, nao a taxa de erro do John no microfone. O que sustenta a fidelidade e o
"discordia" ter saido igual ao que ele descreveu de memoria, antes de medir.

Transcricoes guardadas em `dados/transcricoes_voz.tsv`, com a mesma proibicao do
`frases_teste.txt`: NUNCA viram molde.

#### O tamanho da inversao "fecha"/"abre", agora medido

`examples/sonda_fechar.rs`, 12 sementes, 28 frases:

```
  abrir  (controle)   90% certo     0% inversao
  fechar              15% certo    71% INVERSAO
  matar  (comando)    48% certo    23% inversao
```

`encerra o vscode` e `finaliza o vscode` invertem 12/12. Tese sustentada: o verbo nao
e sinal nenhum naquele espaco. Conserto ainda NAO feito -- ver a proposta abaixo.

### "FECHA" E "ABRE" SAO A MESMA COISA PARA ELA (2026-09-13)

Achado por acaso, e o acaso tem metodo: o John pediu para ela fechar o Discord
escrevendo "discordia", porque e assim que o Whisper transcreve a fala dele.

```
"abre o discord"    ->  abrir_programa(discord)
"fecha o discord"   ->  abrir_programa(discord)    IDENTICO
"fecha o notepad"   ->  abrir_programa(notepad)
"fecha o spotify"   ->  abrir_programa(spotify)
```

Nao e errar a ferramenta: e fazer o CONTRARIO do pedido, com ferramenta que age. A
confirmacao salva, mas so se alguem ler e notar a inversao.

DUAS CAUSAS, as duas de registro e dado:

1. Nao existe `fechar_programa`. `abrir_programa` e dona sozinha do espaco "verbo +
   nome de programa", entao qualquer verbo com um programa cai nela.

2. Nos moldes de `abrir_programa` o sinal e o NOME do programa, nao o verbo. Ela
   nunca precisou distinguir verbo naquele espaco.

O mesmo apareceu no Mir4 na mesma noite: "fecha o mir4" -> `calcular("mir4")`, e
"encerra o processo Mir4G" -> `perguntar` (certo, ela nao tem a ferramenta). Pelo
caminho do comando ela acerta em cheio: `taskkill /F /IM Mir4G.exe`. A capacidade
existe; falta o nome.

(O Mir4 nao fecha nem assim -- roda protegido, `taskkill` devolve Acesso negado.)

#### O METODO QUE ACHOU ISTO, e que vale mais que o achado

Eu nao teria testado "fecha o discord". Nao me ocorreria, porque eu SEI que a
ferramenta nao existe -- e quem sabe o codigo nao testa o que o codigo nao faz.

Todo dado de teste que eu escrevo sai da minha cabeca e herda meus pontos cegos. E a
mesma razao de as 196 frases do John terem valido mais que tudo que eu escrevi
sozinho na semana (ver [[teka-regua-independente]]).

PROXIMO PASSO COMBINADO (13/09): usar o `omnivoice-pt` do John, que tem a voz dele,
para gerar fala, passar pelo Whisper, e usar a TRANSCRICAO como entrada de teste.
Erro de transcricao e ruido real com estrutura real -- palavra parecida trocada
("discordia"), palavra colada, pontuacao sumida. Nenhum deles eu inventaria.

Fica em `C:/Users/User/Projetos/omnivoice-pt` (edicao AMD, Vulkan+GGUF, roda na
RX 580 sem CUDA).

### CINCO FERRAMENTAS QUEBRADAS QUE NINGUEM SABIA (2026-09-12)

As 196 frases do John entraram no benchmark e ele foi de 150 para 329, de 10 para 20
ferramentas cobertas. A base nova, 12 sementes:

```
216,58 de 329 = 65,8%     sd entre sementes 3,64 pp
```

O 65,8% contra os ~77% da regua antiga NAO e regressao: sao as 12 ferramentas que
nunca tinham sido medidas puxando para baixo. E o que elas mostraram:

```
copiar_arquivo       0,0%    36 tentativas, ZERO acertos
mover_arquivo        0,0%    60 tentativas, ZERO acertos
apagar_arquivo       0,0%    (1 frase — amostra nao decide)
buscar_web           8,3%    (1 frase — amostra nao decide)
buscar_no_conteudo  24,0%    8 frases
```

Zero absoluto em 36 e 60 tentativas nao e ruido. E ficou invisivel por meses porque
o benchmark de 150 nao tinha uma frase sequer delas -- inclusive de `atalho`, a
ferramenta 20, que custou -2,58 e foi medida por tres dias sem nunca ter um teste.

#### A CAUSA, E ELA NAO ERA SO DADO

`copiar`/`mover` iam para `criar_pasta` e `listar_pasta`. Medido: ZERO moldes delas
citavam "pasta". Todos ensinavam arquivo->arquivo, e o John pede arquivo->PASTA. No
treino, "pasta" so existia com as ferramentas de pasta.

E a primitiva RECUSAVA pasta como destino:

```
copy(arquivo, PASTA)   -> Err PermissionDenied "Acesso negado."
rename(arquivo, PASTA) -> Err PermissionDenied
```

Ensinar a forma sem consertar isso trocaria "escolhe a ferramenta errada" por
"escolhe a certa e falha executando". Consertado nas duas pontas (`a5ef7db`).

Um palpite morreu no caminho: achei que fosse "numero de argumentos obrigatorios",
ja que as duas tem dois. Mas `escrever_arquivo` tem dois e esta em 52%, e
`apagar_arquivo` tem um e esta em 0%.

#### O QUE SOBRA DIAGNOSTICADO E NAO CONSERTADO

`buscar_no_conteudo` perde 26 erros para `procurar_arquivo`. Os moldes tem marcador
de conteudo, mas um conjunto FIXO e estreito -- e ela nao generaliza entre variantes
quase identicas:

```
molde   "dentro dos arquivos"    John   "nos arquivos"
molde   "quais fontes falam de"  John   "arquivo que fala de"
```

Nao e falta de marcador, e falta de VARIEDADE de marcador. Mesmo padrao dos verbos
de `listar_pasta`.

### EU PREVI QUE A REGUA MAIOR BAIXARIA O RUIDO. NAO BAIXOU (2026-09-12)

Ao propor crescer o benchmark eu escrevi, com numero e tudo, que o ruido cairia para
~68% (raiz de 150/329) e que a mesma corrida de 8h enxergaria efeito 1,5x menor.

```
150 frases -> sd 3,23 pp
329 frases -> sd 3,64 pp     113% do anterior
```

Subiu. O erro: modelei o ruido como amostragem binomial sobre ITENS, que media para
baixo com mais itens. Mas o ruido dominante esta no MODELO -- cada semente produz uma
Teka diferente, e isso e correlacionado entre todas as frases. Mais frases nao diluem
variacao de modelo.

RESSALVA que impede a conclusao oposta: o que decide experimento e o desvio da
diferenca PAREADA, e o pareamento cancela a variacao de semente. Isso so se mede com
DOIS bracos na regua nova, e ha um. O que esta errado e a minha justificativa, nao
necessariamente a decisao.

E a decisao se paga de outro jeito, que e maior: sem as 329 frases, as cinco
ferramentas quebradas continuariam invisiveis.

### DUAS MECANICAS INVENTADAS E MORTAS PELA MEDICAO EM UMA NOITE (2026-09-12)

```
responder_ou_falta    dispara 0 vezes em 94 frases deiticas
checagem de tipo      pega 1 erro de 119
```

A primeira: eu li que argumento obrigatorio sem recorte vira
`Err("nao consegui recortar")` e construi para transformar aquilo em pergunta
dirigida. Medido, o ramo e caminho quase morto -- `recortar` ENCAIXA NA PALAVRA,
entao quase nunca sai vazio. Ela nao falha em recortar; ela recorta lixo com
confianca:

```
"pega esse txt e faz uma copia dele"  ->  executar_comando(comando="dele")
```

A segunda: propus, no commit da primeira, que o lever certo era checar se o
argumento parece do TIPO do parametro. Medi antes de construir: pega 1 erro de 119.
Ela nao confunde o tipo -- escolhe a ferramenta errada com argumento que parece
certo.

As duas nasceram de LER O CODIGO E ACREDITAR. As sondas que as mataram levaram 15
minutos cada. E nas duas, o que sobrou de pe foi o que a secao 3.2 ja dizia: o
gargalo e ESCOLHER a ferramenta, e isso se move com dado.

### FECHADO: as duas ferramentas da ponte custaram -2,75, e FICAM (2026-09-10)

A guarda de 12 sementes registrada em `exp_22.sh` fechou. Base: braco `cr_*`, 20
ferramentas com o critico.

```
PRIMARIO   ferramenta certa de 150
           116,17  ->  113,42     delta -2,75   t=-1,79   n=12
           caiu em 7, subiu em 3, empatou em 2
           chao de ruido: 1,2

           argumento condicional  -1,27 pontos  (t=-1,92)
```

**O custo e real mas nao e conclusivo.** -2,75 e 2,3x o chao de ruido e cai em 7 de
12, com o argumento apontando junto — mas t=-1,79 com 11 graus de liberdade da p~0,10,
e o corte pedia |t| > 2,20. "Provavelmente custou", nao "custou".

**DECISAO DO JOHN, 10/09: as duas FICAM.**

Vale registrar a tensao, porque ela e legitima: `editar_arquivo` foi descartada por
custar 3 pontos de argumento, e estas custam 2,75 de ferramenta mais 1,27 de
argumento. A diferenca que sustenta as duas decisoes: a `editar_arquivo` DERRUBAVA o
teste de integracao (85,0% contra limiar de 88%) e estas passam; e estas trazem
capacidade que a Teka nao tinha.

E o numero e piso, nao teto: ele mede ACURACIA, e o custo das mesmas duas aparecia
tambem na geometria da assinatura, com acuracia identica (ver a entrada anterior).

#### A SUSPEITA PRE-REGISTRADA ESTAVA ERRADA — QUARTA VEZ

O `exp_22.sh` nomeou `procurar_arquivo` de antemao, por ser vizinha semantica de
`buscar_no_conteudo`. Ela MELHOROU.

```
escrever_arquivo   +14        procurar_arquivo   -7   <- a suspeita nomeada
listar_pasta       +14        ler_arquivo        -3
memoria            +11        perguntar, disco    0
hora                +3        executar_comando   +1
```

Os erros somam exatamente os 33 do primario: o secundario explica o primario inteiro.

O custo caiu na familia de CAMINHO DE ARQUIVO, e nao na de busca. Faz sentido depois
do fato — `buscar_no_conteudo` e `ler_imagem` carregam ambas um caminho — mas *depois
do fato* e a parte que importa: isto e historia pos-hoc, e vale como hipotese para a
proxima sonda, nao como achado. Ver [[teka-patch-por-entropia]] para o precedente de
efeito pos-hoc que encolheu pela metade na confirmacao.

Placar de prever qual superficie uma mudanca de dado toca: **quatro tentativas, zero
acertos** (verbos 05/09, fora-de-escopo 08/09, geometria da assinatura 10/09, e esta).
Ver [[teka-registrar-a-medida-ampla]]. A licao nao e "prever melhor" — e registrar a
medida AMPLA e deixar o secundario dizer onde caiu.

#### O QUE ISSO ABRE

O secundario aponta onde trabalhar no 3.2: `escrever_arquivo`, `listar_pasta` e
`memoria`, e nao as tres que a analise de erro antiga sugeria. Poco de valor e forma
de frase, o metodo que funcionou tres vezes.

### A SUITE ESTAVA VERMELHA, E OS TESTES E QUE ESTAVAM CERTOS (2026-09-10)

Tres testes de integracao caidos ha commits, em silencio. O motivo de ninguem ver e
mecanico: a suite exige `--release` e leva ~50 min, entao os commits foram assinados
olhando so a lib. O `983b60e` diz no proprio corpo "Suite: 304 na lib, verdes".

E eu quase repeti o erro maior. Reportei "suite inteira verde, 360 testes" e commitei
em cima disso. Eram 371 e tres vermelhos. Duas causas somadas:

```
sem --no-fail-fast   o cargo para no 1o BINARIO vermelho.  `memoria` caiu,
                     `modelo` e `reforco` NUNCA rodaram
saida em tarefa      o laco de espera da tarefa quebrou e ela saiu ANTES do
de fundo             cargo, levando o resto da saida junto
```

As 7 linhas de `test result` que sobraram tinham cara de suite completa. **Ausencia
de falha nao e prova de passagem** — a suite tem 10 binarios, e um resumo com menos
de 10 linhas esta incompleto, nao verde. `roda_suite.sh` conta isso agora.

#### A TENTACAO, e por que ela estava errada

Os tres pediam limiar afrouxado: subir as epocas do `memoria`, afrouxar o `0.5` do
`reforco`. Os dois comentarios dos proprios testes ate sugeriam o caminho — o do
`memoria` diz "e o terceiro teste a cair pelo mesmo motivo... o modelo demora mais
para assentar".

**Os tres testes estavam certos.** Eram dois bugs de codigo, e nenhum limiar foi
tocado.

#### Bug 1: uma cabeca respondendo duas perguntas em escalas diferentes

`cache.valor` era ao mesmo tempo `V(s)` do reforco e o auto-critico do supervisionado:

```
valor   V(s)    previsao da RECOMPENSA    ~ -0,5 a 0
auto    sigma   "acertei?"                logit, ~ +-3
```

O `983b60e` passou a treinar o auto-critico. Dai `V(s)` vivia em espaco de LOGIT e
`vantagem = r - V(s)` nunca dava zero — contrariando a propriedade que a doc de
`learn::reforco` declara como projeto ("a ferramenta infalivel nao acumula credito").

```
                              antes    depois
sucesso silencioso      48/48 nao-nulas   0/48
laco fechado                     1/6      4/6   no ambiente
erro do critico                0,544    0,044   corte de 0,5, intocado
```

O laco de reforco estava TRAVADO em 1/6 desde `983b60e`. De brinde, um bug que
ninguem tinha visto: rodar o reforco sobrescrevia a cabeca que `model::confianca` le
para decidir se pergunta.

A bisseccao (`roda_biss_reforco.sh`) nomeou o suspeito antes de rodar e confirmou —
`sucesso_silencioso` falha em `983b60e` com os MESMOS 48. E mostrou o outro lado:
`983b60e` CONSERTOU o `a_ancora`, que era vermelho antes. Por isso a cabeca foi
separada, e nao o commit desfeito.

#### Bug 2: a assinatura lia so o fim da frase

`assinatura()` devolvia o estado do ultimo patch. O argumento parecia bom — backbone
recorrente, no fim ja passou tudo — mas o estado tem porta e DECAI, e o assunto esta
no meio: *"como esta a ram do computador"* termina em "computador".

```
                 certo   distratores      recuperou
ultimo patch     0,043   0,079..0,158     ERRADO
media da frase   0,612   0,370..0,465     certo
```

0,043 no certo nao e "quase": os quatro ficavam entre 0,04 e 0,16, argmax sobre
ruido. Descartar os dois ultimos patches dava margem melhor (+0,217 contra +0,147),
mas esse `k` seria constante escolhida em cima de UMA consulta — regua feita para o
teste passar. Media simples, sem botao.

#### O QUE ISSO DIZ SOBRE A GUARDA DE 22 FERRAMENTAS

A bisseccao (`roda_bisseca.sh`) fixou a causa do bug 2 na entrada das duas
ferramentas da ponte: verde em `aeacf77` (20), vermelho em `e355765` (22). **E a
acuracia e IGUAL dos dois lados: intencao 53,9% contra 54,0%.**

O custo do registro crescer NAO apareceu em acuracia. Apareceu na geometria da
assinatura — cosseno do episodio certo caindo 59%, de 0,382 para 0,158.

A guarda de 12 sementes mede `ferramenta certa` no benchmark de 150. **Ela nao pega
isto.** O pre-registro do `exp_22.sh` chegou a nomear a suspeita de antemao:
`procurar_arquivo` roubada pela vizinha `buscar_no_conteudo` — efeito de acuracia. O
custo real caiu noutra superficie.

Terceira vez que eu erro qual superficie uma mudanca de dado toca. Ver
[[teka-registrar-a-medida-ampla]]: agora sao tres pontos e zero acertos.

#### E a regua errou de novo, no mesmo dia

O resumo que eu escrevi para impedir a proxima leitura errada anexava o resumo ao
log e SO ENTAO contava, em cima do arquivo que agora repetia as linhas. 371 viraram
742.

```
07/09  a grade do critico fora de escala      saldo +0 por construcao
08/09  o critico saturado                     separacao 0,012
09/09  a grade sem o piso de "nao abster"     -4,17 que media a grade
09/09  o denominador da maiuscula             desbalanceamento que nao existia
10/09  o resumo da suite contando dobrado     371 lidos como 742
```

**Mas 10/09 tem a excecao, e ela importa mais que a regra.** Nas quatro primeiras o
instrumento estava errado e o codigo certo. Desta vez foi o contrario: os tres testes
mediam propriedades quebradas de verdade, e a saida facil — afrouxar o limiar — teria
escondido um laco de reforco travado em 1/6 e uma memoria recuperando por sorteio.

"Regua nova erra mais que o codigo medido" e heuristica de onde OLHAR primeiro, nao
licenca para desconfiar do teste. O que separa os dois casos e a mesma coisa de
sempre: medir antes de consertar. A sonda previu cos 0,612 e o teste mediu 0,612.

Commits: `e522bdc` (busca), `7ea0241` (os tres testes), `eb3c266` (a contagem dobrada).

### PROVADO: a Teka pode usar as ferramentas do Harness sem o LLM (2026-09-09)

O John perguntou se nao dava para importar as ferramentas do Harness, "o legal e
ela usar para realizar o pedido melhor". Fui verificar, e da.

**O problema que isso resolve.** O protocolo SDK deles tem cinco metodos
(`initialize`, `session/prompt`, `shutdown`, e duas notificacoes) e **todos passam
por um agente com LLM decidindo**. Nao existe "execute esta ferramenta". Pela porta
do SDK, quem escolhe a ferramenta e o LLM deles — a cabeca de intencao da Teka fica
de fora. A divisao que estava escrita aqui nao cabia por ali, e eu tinha documentado
sem conferir a lista de metodos.

**A porta que existe.** `ctx.tools` e servico publico, e o
`ToolExecutionInput.agent` e **opcional** ("set by the agent loop"). Entao um plugin
comum, montado no `cordis.yml`, ve e executa.

**Medido, com um plugin de 100 linhas (`teka-ponte.mjs`):**

```
ctx.tools.schemas()   ->  25 ferramentas
   grep · glob · read · write · edit · str_replace_editor · read_image
   web_search · pwsh · todo_write · skill
   job_list · job_output · job_kill
   subagent · subagent_fork · workflow · ralph · send_message
   get_goal · create_goal · update_goal · list_agents · interrupt_agent

ctx.tools.execute()   ->  `glob` rodou SEM agente e devolveu arquivos de verdade
   { isError: false, content: [...], meta: {...}, value: {...} }
```

Basta `callId`, `name`, `arguments` e `signal`. **Sem chave de API, sem token por
acao, sem latencia de rede** — e com a pipeline de seguranca deles inteira no
caminho (politica, guardas, tempo-limite).

**TRES TROPECOS, e o terceiro e o que ensina.**

```
0 ferramentas no apply   era ORDEM, nao escopo. O Cordis monta em PARALELO, e
                         `inject` espera o SERVICO existir, nao o conteudo chegar.
                         Amostrar ao longo do tempo separou as duas explicacoes:
                         0 no apply, 25 em 0,25s

signal faltando          `callerCancelled` lia `.aborted` de undefined

CONTROLE POSITIVO        `*.json` num diretorio COM package.json devolveu "No
FALHOU                   files found". Fui ler o contrato em vez de chutar:
                         `path` "defaults to the session workspace", e nao havia
                         sessao. Com caminho explicito, achou tudo
```

O terceiro quase passou: **a primeira execucao devolveu vazio e eu quase li como
sucesso.** Sem o controle positivo eu teria concluido que executava quando so estava
devolvendo nada.

### O DESENHO NOVO DA FUSAO

```
MCP/ferramentas   a Teka decide (103 ms, local) e as ferramentas DELES executam
                  -- sem LLM, sem custo por acao
LLM               so quando falta SENTIDO: "toca algo pra me animar" virando busca
```

Isso entrega o que o John decidiu como alvo — **as ferramentas do Harness** — sem
terceirizar a decisao, que era o preco da porta do SDK.

**O que falta:** trocar o plugin de diagnostico por um servidor de linha
(`tools/list`, `tools/call`) e ligar o cliente que ja existe do lado da Teka
(`json.rs`, `harness.rs`, `harness_proc.rs`). O transporte e o mesmo; muda so o
vocabulario.

**E a regua nova continua pendente**, e ela nao e trabalho meu sozinho:
[[teka-regua-independente]] registra que treino e benchmark meus ja compartilharam
um ponto cego de 24 pontos.

### FECHADO: o critico saiu do zero, mas nao paga como abstencao (2026-09-09)

```
PRIMARIO (corrigido)   saldo do critico    base 0,00  ->  0,67
                       10 de 12 sementes dao ZERO, ou seja "nao abster"
A CABECA SAIU DO ZERO  separacao bruta     0,000  ->  0,850
                       normalizada (d')            0,62
GUARDA                 benchmark de 150    113,50 ->  116,17   (+2,67, t=1,88)
```

**Funcionou como mecanismo e falhou como ferramenta.** A cabeca estava morta —
previa `0,000` para tudo, em 12 de 12. Agora discrimina, com d' ~ 0,62. E a primeira
vez que ela faz alguma coisa no caminho supervisionado.

**Mas como regra de abstencao nao paga.** Em 10 das 12 sementes o melhor limiar e
"nao abster". A margem, que ja existia, rende +5 por semente. d' de 0,62 nao e
sobreposicao pequena o bastante: para evitar um erro, destroi mais de um acerto.

A regra de parada NAO disparou — o benchmark nao desabou, entao a suspeita do
clipping esta descartada. Ele ate subiu 2,67, o que eu **nao previ** (eu previa risco
de queda). Nao e significativo em n=12 e nao tenho explicacao; fica como **nao
explicado**, e nao como vitoria.

**O PRIMARIO BRUTO ESTAVA INVALIDO, e o defeito era meu.** Ele deu -4,17 com
t=-3,07. Comparando as duas tabelas:

```
MARGEM    comeca em 0,05 -> saldo 0   (a grade OFERECE nao abster) e sobe a +5
CRITICO   comeca em 2,93 -> saldo -5  (a grade comeca DENTRO da distribuicao)
```

A grade por quantil nunca oferecia "nao abster". O base tinha essa opcao (limiar
abaixo dos zeros de uma cabeca morta) e o tratado nao. O -4,17 media A GRADE.
Corrigido em `candidatos_dos_dados`, com teste.

**TERCEIRA VEZ que o instrumento morde neste mesmo experimento:** grade fora de
escala, critico saturado, grade sem o piso. As tres vezes eu quase li o defeito da
regua como resposta do modelo. Vira regra: **regua nova erra mais que o codigo
medido** — sonda curta antes de corrida longa, sempre que o instrumento for novo.

### O QUE ISSO FAZ COM O PLANO

Pelo que estava registrado antes de rodar: *"se o saldo continuar em zero, a fusao
sobe na fila"*. O saldo e zero. **A fusao sobe.** Ela ja tem o portao cruzado (4.5) e
as tres pecas de plumbing construidas (4.9).

**Ressalva que nao pode se perder:** a medicao mata a abstencao POR LIMIAR, nao o
RANQUEAMENTO. O passo 2 era top-3 intencoes, pontuar, escolher ou abster. Ordenar
candidatos DENTRO do mesmo pedido e tarefa diferente e mais facil que um limiar
global — a sobreposicao entre pedidos nao atrapalha. Continua nao testado e continua
plausivel.

### PASSO 1 DO CRITICO — REGISTRADO EM 2026-09-08, ANTES DE RODAR

**A MUDANCA.** `Alvo::auto_critico`, ligado em todo alvo supervisionado. O critico
passa a prever *"a minha propria escolha vai estar certa?"* — 1 se a chamada que ela
emitiria bate com o rotulo (ferramenta E argumento), 0 se nao. Alvo discreto,
calculado no mesmo passo, sem forward extra.

Campo NOVO e nao reuso de `alvo_valor`: `e_reforco = alvo_valor.is_some()` governa a
contabilidade do placar, e encher aquele campo no supervisionado corromperia as
metricas em silencio.

**A LINHA DE BASE E O BRACO DE HOJE** (`oos_*`), nao o de ontem. O binario do
experimento do fora-de-escopo foi compilado ANTES do critico, entao comparar contra
ontem misturaria duas mudancas. Contra hoje, a unica diferenca e o critico.
Decidido pelo John em 08/09, **antes de qualquer numero aparecer**.

**INSTRUMENTO — ja existe e ja esta zerado.**

```
sep_critico   melhor limiar  -0.30  saldo  +0     em 12 de 12 sementes
(comparar)    margem                saldo +11
```

Se o passo funcionar, o saldo do critico sai do zero. Nao inventei regua: ela ja e
impressa em todo log de benchmark, e ja diz zero.

O benchmark de 150 entra como GUARDA, nao como alvo: treinar mais uma cabeca nao
pode estragar as outras.

**TRES MECANISMOS DITOS ANTES, porque mordem em silencio.**

1. **O alvo se move.** E a correcao do modelo ATUAL. Cedo no treino quase tudo e 0,
   e o critico so aprende algo util quando a politica estabiliza. Convergencia mais
   lenta que o resto, por construcao.

2. **A PORTA LATERAL DO CLIPPING.** O gradiente do critico nao flui para o tronco —
   ja e destacado de proposito, e esta medido no codigo que deixa-lo passar derrubou
   a intencao de 77% para 56%. **Mas o clipping do Adam e de NORMA GLOBAL**: soma o
   quadrado de todos os gradientes e, passando do teto, encolhe tudo pela mesma
   escala. Mais gradiente na soma, escala menor, tronco andando menos por passo.

   **REGRA DE PARADA:** se o benchmark de 150 desabar ja na primeira semente, o
   suspeito e o clipping e NAO a hipotese; o conserto conhecido e tirar a cabeca de
   critico da norma. Isto e distinto de "o efeito nao apareceu", e nao vou confundir
   os dois depois.

3. **Prever a propria correcao pode simplesmente nao ter sinal em 1,6 M.** E mais
   dificil que classificar. Se o saldo continuar em zero, isso nao e fracasso do
   plano — e a resposta de que deliberacao precisa de mais capacidade, e ai a fusao
   sobe na fila.

**DESENHO.** Mesmas 12 sementes (19-30), contra `oos_*`. n=12, FIXADO.

### O CAMINHO: deliberacao cabe na Teka, compreensao precisa do Harness

**Passo 1 — alimentar o critico.** Treinar `valor` no supervisionado com alvo
auto-supervisionado: *"a minha propria escolha vai estar certa?"*. Amostra o argmax
do modelo no exemplo, rotula certo/errado, ensina o critico a prever. Um termo de
perda a mais.

Isso da o que `teka-fronteira-do-perguntar` pediu em 04/09 e eu nao tinha
conectado: **sinal de CAPACIDADE em vez de sinal de SUPERFICIE**. Hoje ela abstem
porque o verbo e estranho; com o critico treinado, ela abstem porque **preve que vai
errar**. E o instrumento ja existe e ja esta zerado: `sep_critico`, saldo +0.

**Passo 2 — deliberar.** Trocar o argmax unico por: top-3 intencoes, montar a
chamada candidata de cada, pontuar com o critico, escolher a melhor ou abster se
todas forem ruins. **O resultado muda por causa de uma avaliacao interna.**

**O TETO, dito antes de comecar.** Isto faz ela DELIBERAR, nao ENTENDER. Ela passa a
saber quando nao sabe — enorme, porque 53% dos erros envolvem `perguntar`. Mas
continua sem entender que "estou desanimado" pede musica animada. Semantica precisa
de um leitor, e o leitor e a fusao.

**Decisao do John (08/09): fazer o passo 1 assim que o experimento do fora-de-escopo
fechar.**

### FECHADO: a contradicao envenenava os VERBOS, nao a fronteira (2026-09-08)

```
INSTRUMENTO (registrado)   falsos `atalho`    -0,08/semente   t=-0,23   4/12
                           base 1,08  ->  tratado 1,00

GUARDA                     benchmark de 150   +3,17           t=+2,55
                           base 110,33  ->  tratado 113,50
```

**A hipotese registrada MORREU.** O mecanismo que eu afirmei — menos falsos
`atalho` — nao aconteceu. E ruido.

**A intervencao funcionou por outro caminho**, e o caminho esta documentado no
proprio `dados.rs`:

```
  ferramenta esperada   base  trat   dif
  procurar_arquivo        81    66   -15
  escrever_arquivo        61    51   -10
  listar_pasta            35    28    -7
  memoria                 40    33    -7
  disco                   36    30    -6
  perguntar              135   141    +6     <- PIOROU
  TOTAL                  476   438   -38
```

Os quatro exemplos ensinavam **`poe`, `quero`, `aumenta`, `toca` = fora de escopo**,
e esses verbos aparecem em pedido de ferramenta REAL. A contradicao nao envenenava
so a fronteira do `atalho`: envenenava os verbos das ferramentas. E o comentario do
proprio arquivo ja dizia — *"o modelo nao aprendeu 'isto esta fora'; aprendeu
'manda = fora'"*.

**Duas ressalvas sobre o +3,17.** Ele estava pre-registrado, mas como GUARDA: eu
disse "nao pode cair" e ele subiu — nao e garimpo, mas a minha previsao sobre ele
estava errada. E foram DUAS comparacoes declaradas; corrigindo, o limiar sobe de
2,20 para ~2,56 e o t=2,55 fica na linha. Sozinho o numero e marginal.

O que sustenta nao e o t: e a **coerencia do mecanismo** — 38 erros a menos,
concentrados nas ferramentas reais, com a abstencao piorando, exatamente como a
falha documentada preve. Numero marginal com mecanismo coerente vale mais que
numero forte com mecanismo inventado.

**Consequencia:** 113,50 esta ACIMA do braco de 19 ferramentas (112,92). O custo de
-2,58 da ferramenta 20 nao so foi pago — sobrou.

### A SEGUNDA VEZ que eu registro a superficie errada

```
05/09  variedade de verbo   registrei a direcao errada  (falsa acao, nao abstencao)
08/09  fora-de-escopo       registrei a superficie errada (fronteira, nao verbos)
```

Nas duas eu previ QUAL superficie a mudanca de dado ia tocar, e errei. Nao e falta
de cuidado — e que **eu nao sei prever isso**, e o historico agora tem dois pontos.

**Regra:** ao mudar DADO, o primario e a medida AMPLA (benchmark de 150). O
mecanismo estreito entra como secundario e como explicacao, nunca como o numero que
decide. O inverso — estreito primario, amplo como guarda — ja falhou duas vezes.

### A INTERVENCAO MUDOU antes de rodar — e por que (2026-09-08)

O plano de ontem era acrescentar a FORMA que falta (curta, imperativa) ao poco do
`perguntar`. Antes de escrever uma linha, reli a minha propria memoria
`teka-fronteira-do-perguntar`, de 04/09, que diz o contrario com dado:

> `perguntar` e aprendida como o COMPLEMENTO das superficies dos moldes, nao como
> conceito. Ampliar o poco fora-de-escopo nao funciona; e a regra do proprio
> `dados.rs` proibe exemplo fora-de-escopo de compartilhar verbo com ferramenta
> real — o poco esta proibido de cobrir a regiao onde as falhas vivem.

Entao fui olhar o poco em vez de ampliar. **Quatro exemplos fora-de-escopo eram
pedidos que ela hoje ATENDE:**

```
"aumenta o volume"      -> aumentar_volume    (e gatilho LITERAL da tabela)
"toca uma musica ai"    -> tocar_faixa
"poe um som pra tocar"  -> tocar_faixa
"quero ouvir podcast"   -> tocar_faixa
```

O primeiro e o caso puro: **a mesma string** estava no poco de `atalho` rotulada
`atalho` e no fora-de-escopo rotulada `perguntar`. Dois rotulos para uma frase. O
modelo nao aprende a fronteira ali — aprende que ali e sorteio.

Foram escritos quando tocar musica nao era capacidade dela. **Capacidade nova
envelhece o fora-de-escopo antigo**, e nada percebia.

**A NOVA INTERVENCAO** e trocar esses quatro por pedidos de dominios que ela
realmente nao tem (uber, luz da sala, ar condicionado, mesa em restaurante),
**um por um**, para o tamanho do poco nao mudar junto: o que muda e QUAIS frases,
nao QUANTAS. Nao e ampliar o poco — e tirar contradicao. Intervencao diferente da
que a memoria enterrou.

**O INSTRUMENTO CONTINUA O MESMO**, e continua apontando para a mudanca: frases de
`perguntar` e `hora` que viram `atalho`, por semente, excluindo a linha 117. Base:
1,67 por semente.

**E o teste que achou isso quase me fez consertar dado bom.** Ele acusou uma quinta
frase, "seria bom uma radio tocando" — falso positivo: o gatilho "toca" casava
dentro de "tocando", porque o casamento nao respeitava fronteira de palavra. O
mesmo defeito estragava o alvo ("poe um som pra tocar" devolvia "poe um som pra r").
**Consertei o medidor antes de agir sobre o que ele dizia**, e a frase ficou onde
estava.

### PROXIMO: estreitar a fronteira do `atalho` — REGISTRADO EM 2026-09-07, ANTES

A medicao do custo entregou a familia de erro inteira, com nome:

```
25 erros, 13 frases, uma forma so: curta e imperativa
"leva o lixo pra fora"   "cuida disso ai pra mim"   "me situa no tempo ai"
```

**HIPOTESE.** O poco do `perguntar` nao tem frase curta e imperativa fora-de-escopo.
O do `atalho` tem 77 delas ("pula essa", "pausa ai", "toca", "proxima"). Entao toda
frase dessa forma que aparece vai para o `atalho`, por falta de concorrente.

**A MUDANCA.** Acrescentar a FORMA que falta ao poco do `perguntar` — curta e
imperativa, conteudo diferente. Nao encolher o `atalho`: e o metodo que ja
funcionou tres vezes (ARQUIVOS 12->60, CONSULTAS 30->77, moldes indiretos).

Cuidado que a suite ja prende: mesma forma, **conteudo diferente**. As 13 frases
acima estao no benchmark, e frase de teste no gerador e a linha que nao se cruza.

**INSTRUMENTO, e ele aponta para a mudanca.** Frases de `perguntar` e `hora` que
viram `atalho`, contadas por semente, **excluindo a linha 117** — naquela o modelo
esta certo e o gabarito e que envelheceu.

```
base    20 erros em 12 sementes = 1,67 por semente
se a mudanca funcionar, ESTE numero cai
benchmark de 150 entra so para confirmar que nada quebrou em volta
```

**A RESSALVA, dita antes.** O -2,58 nao e significativo, e consertar efeito nao
significativo e a receita de perseguir ruido. O que sustenta este passo nao e o
agregado: sao 25 erros concretos numa forma unica. O mecanismo foi **observado**,
nao inferido de um p. Se o instrumento acima nao se mover, a hipotese morre — nao
se procura outro numero depois.

**DESENHO.** Mesmas 12 sementes (19-30), braco novo contra `ct_20_*` que ja esta no
disco. ~7h desatendidas. n=12, FIXADO.

### FECHADO: a ferramenta 20 custou -2,58, e o custo tem dono (2026-09-07)

```
PRIMARIO   150 cruas       -2,58   t = -1,84   n=12   pior em 7/12
SECUNDARIO 149 sem a 117   -2,67   t = -1,86

19 ferramentas  112,92        20 ferramentas  110,33
```

**Nao significativo** — o t critico em n=12 e 2,20. E e exatamente o caso declarado
antes de rodar: com desvio pareado ~3,8 o detectavel a 80% era ~3,2, e o observado
e menor. Le-se "nao custou mais que ~3 pontos", nao "custou zero". A melhor
estimativa continua sendo perda real de ~2,6.

**A mina da linha 117 nao valeu quase nada, e eu errei ao teme-la.** O braco de 20
respondeu `perguntar` ali em 2 de 12 — mas o de 19 tambem errava aquela frase quase
sempre. Tirar a linha mudou o resultado em 0,09. Eu tratei como vies sistematico e
era ruido. Registrar os dois numeros foi o que permitiu ver isso.

**O custo tem dono.** Onde o braco de 20 piorou, somando as 12 sementes:

```
perguntar -> atalho   0 -> 14        hora -> atalho   0 -> 4
```

25 erros em 13 frases, ~2,1 por semente contra um total de 2,58. **A ferramenta 20
disparando quando nao devia explica quase todo o custo.** As frases:

```
5x  [perguntar]  quero ouvir uma playlist relaxante
4x  [hora]       me situa no tempo ai
3x  [perguntar]  leva o lixo pra fora
3x  [perguntar]  cuida disso ai pra mim
```

As 5 primeiras sao a linha 117, onde o modelo esta CERTO e o gabarito e velho. As
outras sao **superficie, nao valor**: frase curta e imperativa, que e a forma exata
das 77 frases do poco do `atalho` ("pula essa", "pausa ai", "toca", "proxima"). E o
"manda = fora" outra vez — molde de superficie larga rouba de outra ferramenta.

E houve ganho, que e honesto registrar: `perguntar -> executar_comando` caiu de 29
para 17, e `perguntar -> rede` de 16 para 4. O -2,58 e o liquido.

**Para a fusao**, que era o motivo de medir: uma ferramenta custou ~2,6 pontos, o
mecanismo e conhecido, e o conserto e o metodo do 3.2 — forma de frase, nao
arquitetura. Bate com o precedente de 10->19, que caiu e depois passou do ponto de
partida. O proximo passo obvio e estreitar a superficie do `atalho` e remedir.

### Quanto custa a ferramenta 20 — REGISTRADO EM 2026-09-07, ANTES DE RODAR

O John pediu: *"treina com as 20 ferramentas e mede o custo"*. Uma ferramenta nova
acrescenta uma classe à cabeça de intenção e 75 frases de gatilho ao poço de
argumento. A pergunta é se as outras 19 pioram.

**Metade da medição já estava paga.** O braço `vb` da confirmação (`cf_vb_s19..s30`,
12 sementes no disco) foi treinado com `teka_vb.exe`, de 41ab886 — e `atalho` só
entrou em 4c02b67. Conferido no próprio binário: `hora` está lá, `atalho` e
`pula essa musica` não estão. O diff de `dados.rs` entre 41ab886 e HEAD são 37
linhas, todas dos 4 commits do `atalho`. Então falta rodar **12 corridas, não 24**.

```
INSTRUMENTO   `ferramenta certa` de 150, pareado por semente
              `frases_teste.txt` NAO mudou entre 41ab886 e HEAD (git)
              base do braco de 19: 112,9 de 150 (desvio 4,4)
PODER         desvio pareado ~3,8 -> detectavel a 80% e ~3,2 pontos
              um nulo aqui diz "nao custou mais que ~2 p.p.", nao "custou zero"
n             12, FIXADO
```

**A mina da linha 117, dita antes de rodar.** O benchmark tem
`perguntar | quero ouvir uma playlist relaxante |`, numa seção cujo cabeçalho diz
*"coisa que ela nao tem"*. Tocar música virou capacidade dela; "quero a playlist" e
"quero ouvir" são gatilhos. **O gabarito envelheceu**, e o braço de 20 leva um erro
numa frase em que está certo — viés sistemático contra o braço que estou medindo.

Não conserto a linha: consertar mudaria a régua entre os braços e nada seria
comparável. Registro os dois, e **lidero pelo primário de propósito, porque é o que
não me favorece** — se ele não mostrar custo, o secundário só pode estar melhor.

```
PRIMARIO     150 cruas         conservador, penaliza o braco novo
SECUNDARIO   149, sem a 117    o gabarito honesto de hoje
```

### O poço copiado à mão que silenciou duas capacidades (2026-09-07)

`NOMES_DE_ATALHO` em `dados.rs` era cópia manual de `dados/gatilhos.txt`, e a doc
dizia *"um teste prende as duas listas juntas"*. **O teste não existia.** Havia só a
direção fácil (frase do poço → atalho existente); a direção que pega esquecimento
— gatilho da tabela que não chegou ao poço — faltava.

Resultado: acrescentei `embaralhar` e `que_musica_e_essa` à tabela e o poço
continuou com as 65 frases antigas. As duas capacidades ficaram inalcançáveis pelo
treino, e nada reclamou. `o_poco_de_atalhos_e_a_tabela_inteira` prende as duas
direções agora, e `todo_atalho_e_alcancavel` exige que todo atalho tenha fala,
regra, ou uma linha escrita dizendo por que é peça interna e não pedido.

Duas das quatro exceções que eu tinha escrito na primeira tentativa estavam erradas
— `ensurdecer_discord` e `parar_musica` **tinham** gatilho. Eu as declarei internas
por suposição, sem medir. A tabela de tradução das 75 frases, impressa, mostrou que
cada uma cai no seu próprio atalho, sem uma ambiguidade. **Exceção declarada sem
medir é dívida, não documentação.**

---

## 5. Coisas pequenas que ficaram no caminho

- `sonda2.sh` e `tmp_*.rs` são resto de diagnóstico; podem sumir
- A memória semântica só é alimentada por `/buscar`; nada mais grava fato
- O grafo de conceitos existe e fica vazio — falta quem extraia conceito
- `--porta` não é o nome da flag de porta do servidor (descobri errando)
- Os 152 `.log` na raiz não são versionados, mas atrapalham; valeria uma pasta
