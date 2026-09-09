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
benchmark de 150     113,50         20 ferramentas, sem a contradicao (s19-30)
                     110,33         20 ferramentas, com a contradicao
                     112,92         19 ferramentas
                     114,58 ± 3,55  com --patcher entropia (opcional)
argumento condicional  ~92%        (quando a ferramenta sai certa)
ferramentas             20         `atalho` custou -2,58 (t=-1,84, nao
                                    significativo). Ver secao 4.
parâmetros            1,6 M
para rodar            ~24 MB       binário 1,6 + modelo 6,2 + n-grama 16
latência              ~103 ms      na CPU, incluindo carregar do disco
corpus                69,87 MB     9,7x o inicial
```

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

**Teto do `buscar_web`.** A API de Instant Answer da DuckDuckGo só responde termo
único de enciclopédia; frase composta devolve vazio. Quem pede conselho de
configuração recebe verbete. Nenhum treino resolve — só trocar a fonte.

### 3.4 ~~Interface: falta o canal de confirmação~~ FECHADO em 2026-09-05 (e6d4801)

A web nascia read-only porque `IsTerminal` é falso, e isso deixava as oito ferramentas
que agem inalcançáveis pelo navegador. Resolvido em **dois passos, não uma espera**: o
servidor é sequencial, então bloquear no handler esperando resposta travaria tudo. A
confirmação é por pedido, identificada, de uso único e com prazo de 120s.

### 3.5 Juntar com o DeepSeek-Harness

A ordem do John: Teka boa primeiro, junção depois. Continua certa.

**O argumento a favor melhorou esta semana** e não é "mais ferramentas": é que o teto
do `buscar_web` é verbete, e o que falta para responder "que configuração usar" é um
LLM que **leia** o resultado. A divisão natural:

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
