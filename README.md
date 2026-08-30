# TEKA 🧠

Agente cognitivo que pensa **byte a byte**. 100% local, só CPU, Rust puro,
**zero dependências**.

Não é um chatbot. O objetivo é uma IA que **entende pedidos e age com ferramentas**
— e que aprende a fazer isso melhor com o tempo. Falar bonito vem depois.

**Como usar no dia a dia: [USO.md](USO.md).**
O desenho completo, com o porquê de cada escolha: [ARQUITETURA.md](ARQUITETURA.md).

---

## Rodar

Precisa do Rust (https://rustup.rs). Dentro desta pasta:

```bash
cargo run --release
```

Isso mede o backend, o patcher e o modelo na sua máquina. Nenhum download, nenhuma
GPU, nenhum runtime externo.

Para treinar o modelo de linguagem por byte no corpus de português incluído:

```bash
cargo run --release -- treinar --minutos 30 --saida cerebro.bin
```

Para conversar com ela — ela treina as cabeças de decisão e abre um prompt:

```bash
cargo run --release -- agente
```

Com um agente já treinado, e para conferir os números sem acreditar no log do treino:

```bash
cargo run --release -- agente --carregar ag_final.bin --avaliar
```

```
teka> da uma olhada na pasta src
  → {"acao":"listar_pasta","caminho":"src"}
    src (11 itens)
      [pasta] backend
      [pasta] model
      ...

teka> quanto e 15*340
  → {"acao":"calcular","expressao":"15*340"}
    5100
```

Por padrão ela roda em **sandbox**: nada que altere o mundo acontece de verdade.
`--real <pasta>` libera, confinado àquela pasta.

Ela aprende de três fontes, e cada uma tem um comando:

```
/certo | /errado <ferramenta>   você diz     → /consolidar
/explorar                       ela testa    → /reforcar
                                             → /aprender  (os dois, na ordem certa)
```

O prompt vira `teka*>` quando a exploração está ligada: nesse modo ela é
deliberadamente não-determinística, e isso precisa estar visível.

```
teka>  abre o arquivo naoexiste_xyz.txt
       (erro) O sistema não pode encontrar o arquivo especificado.
teka>  /reforcar
       6 transicoes (6 uteis) | recompensa media -0.083 | erro do critico 0.000
       intencao 93.6% -> 93.8%
```

A falha virou sinal de aprendizado sem ninguém dizer qual era a resposta certa.

Para provar que a matemática está certa:

```bash
cargo test --release -- --nocapture
```

---

## O que já existe (fases 0 e 1)

| | |
|---|---|
| **Núcleo** | RG-LRU — recorrência linear diagonal com portões (Griffin/Hawk) |
| **Hierarquia** | encoder local por byte → patcher → backbone por patch → decoder local |
| **Compactador** | patcher `PorPalavra`: ~5 bytes/patch, o backbone roda 5x menos |
| **Backend** | trait `Ops` com implementação escalar (oráculo) e multi-thread |
| **Treino** | Adam com recorte por norma, BPTT truncado, cursores contíguos |
| **Persistência** | salvar/carregar com a configuração gravada no arquivo |

## O que já existe (fase 2 — agir)

| | |
|---|---|
| **Ferramentas** | 9 primitivas em Rust puro (`std` + FFI kernel32), como **dados** |
| **Gramática** | registro → autômato de 243 estados → máscara de 32 bytes |
| **Cabeça de intenção** | classifica qual ferramenta — 78% em frases inéditas |
| **Cabeça de ponteiro** | argumentos **copiados** do pedido — grosso (patch) + fino (byte) |
| **Cabeça de presença** | decide se um argumento opcional aparece |
| **Segurança** | sandbox por padrão, denylist, confinamento por raiz |

### Saída sempre válida

O registro de ferramentas vira um autômato finito. A cada byte, os ilegais viram −∞.
**É impossível emitir uma chamada malformada** — não "raramente erra", é
estruturalmente impossível. Testado com 4.000 passeios aleatórios pelo autômato e
com o modelo *não treinado*: zero chamadas tortas.

### Sem palavras-gatilho

*"da uma olhada em"*, *"o que tem em"* e *"lista"* caem todas em `listar_pasta`.
E o argumento sai **copiado do pedido** — o modelo não precisa saber escrever
`relatorio_final_v3.txt` para acertá-lo.

### Onde ela está hoje, sem maquiagem

Medindo com frases que **nunca apareceram no treino** (o gerador reserva frases
inteiras para a validação, não só exemplos):

| | |
|---|---|
| escolher a ferramenta certa | **78%** *(chute cego: 11%)* |
| recortar o argumento certo (byte exato) | **94%** |
| pedido atendido ponta a ponta | **75%** |

Em frases escritas à mão, totalmente fora do gerador, cai para **~58%** — é o número
mais honesto que existe aqui.

A sintaxe, essa, é 100% por construção: nem um modelo de pesos aleatórios consegue
emitir uma chamada malformada.

### Por que SSM e não GRU

Na GRU a recorrência tem um matmul de `d²` **dentro** do laço temporal: sequencial,
GEMV, ~3-5% do pico da CPU. Na recorrência linear diagonal o laço temporal é
puramente elementwise e **as projeções saem do laço**, viram um GEMM único sobre a
sequência inteira. Sai de *memory-bound* e entra em *compute-bound* — que é onde
AVX2 e os 6 cores rendem.

### Por que hierarquia

O backbone tem 84% dos parâmetros e roda **uma vez por patch**, não por byte. Com
patches de ~5 bytes, isso é 5x menos trabalho no pedaço caro do modelo. É o que
torna byte-level viável sem GPU.

---

## Medido nesta máquina (Ryzen 5 5500, 12 threads)

```
GEMM f32          escalar    12 threads
  gemm_nn           34,0        115,0  GFLOP/s
  gemm_tn           25,3        109,3  GFLOP/s
  gemm_nt            6,0         24,8  GFLOP/s

patcher           bytes/patch
  por_palavra          5,03
  fixo(4)              4,00
  por_classe           2,50

modelo            params   backbone   treino
  pequeno         1,52 M       81%    11.912 bytes/s
  padrao         11,64 M       84%     2.738 bytes/s
```

### Primeiro treino em portugues

Preset `pequeno`, 9 minutos, 5,38 MB de literatura portuguesa:

```
  passo 1     8,00 bits/byte   <- chute uniforme sobre 256 simbolos
  passo 2920  2,15 bits/byte
  validacao (100 KB retidos, estado zerado): 2,32 bits/byte
```

Para comparacao: a nila_mind registra ~2,2 bits/byte com **10,06 M** parametros
sobre corpus equivalente. A Teka chega perto disso com **1,52 M** e 9 minutos.

> A máquina está com **um pente de RAM só** (single channel). Um segundo pente dobra
> a banda de memória e é o maior ganho de performance disponível no projeto — vale
> mais que qualquer otimização de código.

---

## Corretude

92 testes. Os que importam:

- **gradiente do modelo inteiro** por diferenças finitas em f64 — 65 tensores,
  pior erro relativo 4,4e-6. Todo o BPTT é escrito à mão; sem esta prova, todo
  resultado de treino depois seria fé.
- **causalidade** — mudar um byte não altera nenhuma previsão anterior a ele.
- **backend paralelo == oráculo escalar**, bit a bit, inclusive nos gradientes.
- **continuidade de estado** entre janelas de treino.
- **gradiente do agente inteiro** (modelo + 3 cabeças) com extrapolação de
  Richardson: erro relativo **0,00**.
- **4.000 passeios aleatórios** pelo autômato, todos produzindo chamadas válidas.
- **o replay impede o esquecimento** — verificado desligando o mecanismo.
- **sucesso silencioso não move a política** — a trava contra reward hacking.
- **o crítico converge** — o erro dele é asserção de teste, porque um baseline que
  diverge injeta ruído estruturado em vez de só deixar de ajudar.

---

## Estrutura

```
src/
 ├─ num.rs      trait Float (f32 em produção, f64 nos testes de gradiente)
 ├─ rng.rs      PRNG próprio, determinístico por semente
 ├─ backend/    trait Ops + escalar (oráculo) + paralelo
 ├─ nn/         Embed, Linear, RMSNorm, BlocoMlp
 ├─ ssm/        RG-LRU e o bloco recorrente
 ├─ model/      patcher, pilha, hierarquia, io
 └─ learn/      Adam, laço de treino
```

## O que já existe (fases 3 e 4 — aprender com o uso)

| | |
|---|---|
| **Memória episódica** | cada interação guardada com a *assinatura* — a leitura que ela fez do pedido |
| **Recuperação** | por cosseno entre assinaturas: funciona sobre sentido, não palavras |
| **Destilação** | episódios repetidos colapsam num protótipo com peso |
| **Consolidação** | fixa nos pesos o que você corrigiu, com replay para não esquecer |
| **Reforço** | aprende do desfecho, sem ninguém dizer a resposta certa |

```
teka> modo turbo              → {"acao":"hora"}          ← errado
teka> /errado memoria           corrigido para memoria
teka> /consolidar               intencao 93,7% → 93,1%
teka> modo turbo              → {"acao":"memoria"}       ← aprendeu
```

### O laço fechado

Ela age, o mundo responde, ela ajusta — **sem ninguém dizer a resposta certa**:

```
antes:    acerta 4/6 | intencao(val) 77,3%
rodada 1: 5/6 | erro critico 0,033 | 86,4%
rodada 5: 5/6 | erro critico 0,023 | 84,1%
```

Duas travas contra reward hacking, cada uma com teste:

- **sucesso silencioso vale zero.** `hora` nunca falha; se sucesso virasse
  recompensa, ela chamaria `hora` para tudo. Sucesso entra como ponto de comparação,
  não como prêmio.
- **âncora supervisionada.** Marcando decisões corretas como falhas, o replay
  sobrepõe o sinal mentiroso.

## Próximo

Fase 5: criação de ferramentas — composições de primitivas testadas em sandbox.
