# TEKA — Arquitetura

> Agente cognitivo byte a byte, 100% local, CPU, Rust.
> Alvo: AMD Ryzen 5 5500 (Zen 3, 6c/12t, AVX2+FMA, **sem AVX-512**), 16 GB DDR4-3200.

---

## Princípio que organiza tudo

Teka **entende bem e age certo**. Falar bonito é problema de outro dia.

Ela é primariamente um sistema **discriminativo** (pedido → ação estruturada) com um
modelo de linguagem por baixo servindo de representação — e não um gerador de texto
que por acaso chama ferramentas.

Isso não é uma limitação aceita a contragosto: é a jogada central. Gerar português bom
exige um modelo grande e muitos dados. Mapear *"dá uma olhada no espaço do disco"* →
`{"acao":"disco"}` é um problema **discriminativo**, ordens de grandeza mais barato em
dados e em parâmetros. Um modelo de ~14M byte-level fica genuinamente bom nisso,
enquanto o mesmo modelo jamais escreveria um parágrafo decente.

### Regras de projeto

1. **Núcleo sem dependências.** Modelo, cognição, agente e ferramentas: só `std`.
2. **Backend numérico atrás de um trait**, com implementação escalar de referência
   (o *oráculo*) e implementações rápidas atrás de *feature flag*.
3. **Tudo é byte.** Sem tokenizer, sem vocabulário, sem `<unk>`.
4. **Saída sintaticamente válida por construção**, não por sorte.
5. **Ferramentas são dados**, não código.
6. **Correção numérica verificada por diferenças finitas.** Herdado da nila_mind — foi
   a melhor decisão técnica daquele projeto.

---

## 1. Núcleo: SSM diagonal, não GRU

Isso não é preferência estética, é aritmética.

Na GRU a recorrência é `h_t = f(W_x·x_t + W_h·h_{t-1})`. Aquele `W_h·h_{t-1}` é um
**matmul de d² dentro do laço temporal** — com hidden 264 são ~70.000 multiplicações
por passo, por camada, e são **sequenciais**. Na CPU isso vira GEMV: limitado por banda
de memória, cache frio, ~3-5% do pico do processador.

Na recorrência linear diagonal:

```
h_t = a_t ⊙ h_{t-1} + n_t ⊙ (i_t ⊙ x_t)        (⊙ = elementwise)
```

O laço temporal tem **zero matmul** — são `d` multiplicações, não `d²`. Todos os
matmuls (as projeções que produzem `a_t`, `i_t`) saem de dentro do laço e viram **um
GEMM único sobre a sequência inteira**.

| | dentro do laço | forma do compute | eficiência na CPU |
|---|---|---|---|
| GRU | ~70k mults/passo | GEMV sequencial | ~3-5% do pico |
| SSM diagonal | ~d mults/passo | GEMM em bloco | ~60-80% do pico |

Sai de *memory-bound* e entra em *compute-bound*, que é onde AVX2/FMA e os 6 cores
realmente rendem.

### A variante exata: RG-LRU (Griffin / Hawk)

```
r_t = σ(W_r x_t + b_r)                  # portão de recorrência
i_t = σ(W_i x_t + b_i)                  # portão de entrada
p   = ln σ(Λ)                           # < 0, por canal, aprendido
a_t = exp(c · r_t · p)                  # ∈ (0,1), c = 8
n_t = sqrt(1 - a_t²)                    # normaliza a variância
h_t = a_t ⊙ h_{t-1} + n_t ⊙ (i_t ⊙ x_t)
```

- **real e diagonal** — sem números complexos, sem FFT, sem kernel convolucional
- **estável por construção** — `a_t ∈ (0,1)` sempre; nenhum autovalor explode
- **backward direto**, ~1/3 da complexidade do BPTT da GRU
- é, literalmente, uma GRU com o portão de reset e o `W_h` removidos

Inicialização de `Λ` tal que `a^c ∈ [0.9, 0.999]` — memória longa desde o nascimento.

**MoE fica fora da v0.** O ganho do MoE é parâmetro-por-FLOP, que importa quando se
está limitado por compute em escala grande. Aqui ela é pequena, e roteamento esparso
destrói localidade de cache. Se voltar, volta só no backbone e roteando por
**contexto** — não pelo byte cru, que era o defeito da nila_mind.

---

## 2. Hierarquia — três níveis

```
bytes crus  ──▶  [ENCODER LOCAL]  ──▶  patches  ──▶  [BACKBONE]  ──▶  estado
   (256)          SSM pequeno         ~6-8 bytes     SSM grande       cognitivo
                  roda por byte                      roda por patch        │
                                                                          ▼
                                                                    [CABEÇAS]
                                                                          │
bytes de saída ◀── [DECODER LOCAL] ◀──────────────────────────────────────┘
                    SSM pequeno
```

| componente | dimensões | params |
|---|---|---|
| encoder local | h≈96, 2 camadas | ~1,0 M |
| backbone | h≈384, 6 camadas | ~11 M |
| decoder local | h≈128, 2 camadas | ~1,5 M |
| cabeças | — | ~0,3 M |
| **total** | | **~14 M** |

**Orçamento de banda.** ~56 MB em f32. Por passo de backbone lê-se ~56 MB da RAM:

| configuração | patches/s | ≈ bytes/s |
|---|---|---|
| f32, single channel (~15 GB/s) | ~270 | ~1.800 |
| f32, dual channel (~30 GB/s) | ~540 | ~3.500 |
| int8, dual channel | ~2.100 | ~14.000 |

Uma chamada de ferramenta de 60 bytes sai em ~35 ms já na pior configuração.
**Entender é mais rápido que gerar**, porque a leitura do pedido paraleliza sobre a
sequência inteira (o GEMM único) em vez de ser passo a passo.

> **A máquina está em single channel** (um único DIMM de 16 GB). Um segundo pente dobra
> a banda de memória e é o maior ganho de performance disponível no projeto — vale mais
> que qualquer otimização de código.

### O patcher é o compactador

Um tokenizer BPE é um compactador com **dicionário fixo**, aprendido uma vez e
congelado. O patcher faz o mesmo trabalho de forma **dinâmica e contextual**:
previsível → patch grande; surpreendente → patch pequeno. Estritamente mais poderoso.

A taxa de compressão do patcher **é** o multiplicador de velocidade do backbone. Cada
0,5 byte a mais por patch é performance direta.

- **v0:** heurística por classe de byte — quebra em espaço, pontuação, transição
  letra↔dígito, com teto de 8 bytes. Custa zero e funciona bem em texto e comandos.
- **v1:** entropia do encoder local (estilo BLT). Aloca compute onde é difícil.

A fronteira de patch é um trait desde o dia 1 — é o ponto que mais vai evoluir.

---

## 3. Cabeças

Um tronco, quatro saídas:

| cabeça | o que faz | treino |
|---|---|---|
| **próximo byte** | prevê o byte seguinte | auto-supervisionado, sempre ligado |
| **intenção** | classifica qual ferramenta | supervisionado → RL |
| **ponteiro** | localiza os argumentos na entrada | supervisionado |
| **valor** | baseline do crítico | RL |

### A cabeça de ponteiro é a peça-chave

Os argumentos **não são gerados, são copiados** dos bytes da entrada. Para cada slot de
parâmetro, a cabeça pontua posições sobre a sequência de patches e refina para byte.

> *"abre o relatorio_final_v3.txt"* → ela não precisa nunca ter visto esse nome. Aponta
> pro intervalo de bytes e copia.

Como um pedido é curto (~100-300 bytes ≈ 40 patches), a atenção do ponteiro roda sobre
~40 estados: custo desprezível. É um híbrido SSM + atenção local, com a atenção
**confinada ao pedido atual** — nunca sobre o histórico. Preserva o custo O(1) do SSM
onde ele importa.

---

## 4. Saída sempre válida: gramática restrita

O registro de ferramentas é compilado num **autômato finito** na inicialização. A cada
byte gerado o autômato produz uma **máscara de 256 bits (32 bytes)** dos bytes legais;
os logits ilegais viram −∞.

**É impossível ela emitir uma chamada malformada.** Não é "raramente erra" — é
estruturalmente impossível. A confiabilidade sintática saiu do modelo e foi para a
máquina de estados; o modelo só decide **o quê**, nunca **como escrever**.

Com V=256 a máscara é trivialmente barata — outra vantagem específica de byte-level.

---

## 5. Ferramentas como dados

Registro persistido (formato próprio, parser à mão, zero deps):

```
nome · descrição em texto · slots de parâmetro (tipo, obrigatório) · implementação
```

A **descrição é lida pela Teka como bytes durante o treino**: o registro é
auto-documentado, e adicionar uma ferramenta já ensina ela sobre a ferramenta.

**Primitivas em Rust (só `std`):** ler/escrever/listar/procurar arquivo, executar
processo (com denylist), hora/data, navegar diretório, calcular, HTTP local. Infos de
sistema no Windows via `extern "system"` para `kernel32`/`psapi` — ~40 linhas de FFI,
sem baixar nada.

**Criação de ferramentas:** uma ferramenta nova é uma **composição de primitivas** com
template de parâmetros — dado novo no registro, não código novo.

1. Teka encontra um pedido que nenhuma ferramenta atende
2. propõe uma composição
3. roda em **sandbox / dry-run** (sem efeitos colaterais)
4. recompensa positiva + aprovação humana → entra no registro; autômato recompilado

---

## 6. Memória e aprendizado contínuo

Três escalas de tempo:

**Estado (ms)** — o hidden do SSM. O "agora", persistente entre ticks.

**Memória episódica (persistente)** — cada interação gravada como bytes: pedido, ação,
resultado, feedback. Indexada pelo estado do backbone para recuperação por similaridade.
Serve **duas funções ao mesmo tempo**: memória de longo prazo *e* replay buffer.

**Pesos (lento)** — consolidados a partir de lotes **amostrados da memória episódica**,
não do input recente. Essa é a resposta ao esquecimento catastrófico: todo lote mistura
fresco + antigo amostrado.

**Destilação (a compressão que importa).** Durante a consolidação ("sono"), episódios
similares colapsam num **protótipo + desvios**. Cinquenta pedidos de "ver espaço em
disco" viram um padrão abstrato, não cinquenta episódios. Resolve dois problemas de uma
vez: memória não explode, e o replay para de ser dominado por repetição.

*Fase futura:* adaptador rápido de baixo posto que se ajusta dentro da sessão e é
consolidado nos pesos lentos durante o sono.

---

## 7. Compressão

Prever e comprimir são o **mesmo problema** (Shannon): um codificador aritmético gasta
exatamente `−log₂ p` bits por byte. Ou seja, **bits/byte é a taxa de compressão**.

O módulo `codec/` acopla codificação aritmética à cabeça de bytes. Três usos:

1. **Métrica dura** — comparar com gzip é um teste que não mente e não depende de achar
   a saída bonita. Melhor termômetro que existe para esse tipo de modelo.
2. **Arquivamento** da memória episódica antiga.
3. **Demo.**

> Compressão neural é frágil a versão: comprimiu com os pesos W, só descomprime com
> **exatamente** W. Gravar o hash dos pesos junto de cada arquivo e manter snapshots
> congelados como codecs.

**O que Teka NÃO precisa:** compressão de prompt/contexto e compressão de KV cache.
Ambas existem por causa de atenção quadrática e cobrança por token — a Teka é recorrente
(O(1) por passo) e roda local. **O KV cache nem existe num SSM**: o estado é de tamanho
fixo. Comprimir a entrada dela seria jogar informação fora de graça.

---

## 8. Recompensa do RL

| sinal | peso |
|---|---|
| ferramenta executou sem erro | + |
| resultado relevante (usuário não repetiu/reformulou) | ++ |
| feedback explícito | +++ |
| ação bloqueada pela denylist / destrutiva | −−− |
| progresso de aprendizado (curiosidade) | + *(drive secundário)* |

Ator-crítico, com o **ator sendo a própria cabeça de intenção** — RL e supervisionado
compartilham parâmetros. **Começa supervisionado** com exemplos; o RL só refina. RL puro
do zero em seleção de ferramenta leva uma eternidade.

O sistema de afeto da nila_mind volta, mas no lugar certo: modulando exploração vs.
explotação e taxa de aprendizado. Ele funcionava; o problema era ser o único motor.

---

## 9. Organização do código

```
teka/
 ├─ num.rs      trait Float (f32 em produção, f64 nos testes de gradiente)
 ├─ rng.rs      PRNG próprio (splitmix64 + xorshift), zero deps
 ├─ backend/    trait Ops + impl escalar (oráculo) + impl AVX2 (feature)
 ├─ nn/         Linear, RMSNorm — blocos com forward/backward
 ├─ ssm/        RG-LRU: forward, BPTT, scan, testes de gradiente
 ├─ model/      encoder local · patcher · backbone · decoder local · cabeças
 ├─ grammar/    registro de ferramentas → autômato → máscara de decodificação
 ├─ tools/      primitivas em Rust puro + denylist + sandbox
 ├─ memory/     memória episódica, índice, replay, destilação
 ├─ codec/      codificação aritmética sobre a cabeça de bytes
 ├─ learn/      treino, consolidação, RL ator-crítico, afeto
 ├─ agent/      o laço: percebe → entende → age → observa → aprende
 └─ serve/      painel HTTP local
```

`backend` com **duas implementações** é inegociável: a escalar é o oráculo de correção
contra o qual a rápida é validada. É ela que torna o projeto evolucional — a troca
escalar → AVX2 → int8 → GPU não toca em nenhum outro módulo.

---

## 10. Como medir (arnês desde o dia 1)

Sem isso o projeto vira fé. Cinco números:

1. **bits/byte** — qualidade da representação (comparável com a nila_mind: ~1,5)
2. **acurácia de intenção** — top-1 na escolha da ferramenta
3. **exact-match de argumento** — o ponteiro copiou certo?
4. **sucesso ponta a ponta** — o pedido foi atendido?
5. **patches/s e bytes/s** — medido a cada mudança de backend

---

## 11. Fases

| fase | o que |
|---|---|
| **0** | backend + SSM + testes de gradiente por diferenças finitas |
| **1** | modelo hierárquico + patcher + LM de próximo byte (PT-BR + descrições das ferramentas) |
| **2** | gramática + primitivas + cabeças de intenção/ponteiro (supervisionado) |
| **3** | laço do agente + memória episódica + consolidação |
| **4** | RL sobre uso real |
| **5** | criação de ferramentas |
| **6** | quantização int8, patching por entropia, adaptador rápido, codec |

A fase 2 fica boa **muito antes** da fase 1 estar madura — porque discriminar é barato.
Teka vai agir certo enquanto ainda escreve proto-português. É a ordem correta.

---

## 12. O que mudou em relação à nila_mind

| nila_mind | Teka |
|---|---|
| GRU | **SSM diagonal** — tira o matmul do laço temporal (~10x) |
| plano (1 forward/byte) | **hierárquico** — backbone roda ~7x menos vezes |
| gate cego ao contexto (`Wgate[e, byte]`) | **sem MoE na v0**; se voltar, roteia por contexto |
| geração livre | **gramática restrita** — saída inválida é impossível |
| argumentos gerados | **argumentos copiados** por ponteiro |
| memória só no estado oculto | **memória episódica** = também replay buffer |
| treino no input recente | **consolidação amostrada** — não esquece |
| recompensa = curiosidade | **recompensa = resultado no mundo** |
| ferramentas em Python | **primitivas em Rust**, composição como dado |
| kernels acoplados | **backend atrás de trait**, com oráculo escalar |
| fala via LLM externo (OpenRouter) | **100% local** — fala rústica é aceita por ora |

---

## 13. Registro de medições

Números medidos nesta máquina, para servir de linha de base. Toda otimização futura
se compara com estes.

### Fase 0 — backend escalar (oráculo), f32, 1 thread

| | GFLOP/s |
|---|---|
| `gemm_nn` (axpy no laço interno) | 31,0 |
| `gemm_tn` (axpy no laço interno) | 24,3 |
| `gemm_nt` (redução no laço interno) | 5,6 |

Pico teórico do Zen 3 com AVX2+FMA: ~58 GFLOP/s por core em f32. O `nn` escalar já
chega a ~53% disso porque o LLVM vetoriza o padrão axpy sozinho.

**Achado da fase 0 — layout dos pesos.** A variante `nt` tem laço interno de redução
(`acc += a·b`), que o compilador **não pode** vetorizar: soma de ponto flutuante não
é associativa. Fator ~5 contra as outras duas. Com os pesos no layout "natural"
`[n_out, n_in]` — o que a nila_mind usava — o **forward** cai nessa variante.

Trocando para `[n_in, n_out]` e transpondo `W` uma vez no backward:

| | antes | depois |
|---|---|---|
| RG-LRU forward (h=384) | 4.821 passos/s | **38.155** passos/s |
| RG-LRU backward | 22.480 passos/s | 23.689 passos/s |

**7,9x no forward** sem tocar em uma linha de matemática — só no layout. E o forward
é o que a inferência faz o dia inteiro. Com `batch=1` (responder a um pedido) o ganho
é ainda mais relevante, porque aí o GEMM vira GEMV e o layout decide se o laço interno
é contíguo.

### Corretude

`cargo test --release -- --nocapture` — 19 testes, incluindo checagem de gradiente por
diferenças finitas em f64 para `Linear`, `RMSNorm` e `RG-LRU` (pesos, Λ, entrada e
estado inicial). Pior erro relativo observado: **1,1e-7**, a ordem certa para diferença
central em f64.

Testes estruturais que acompanham:

- processar a sequência em janelas, carregando o estado, dá bit a bit o mesmo que
  processar de uma vez — o modelo treinado e o modelo em inferência são o mesmo objeto;
- 4.000 passos com pesos `~N(0, 10)` não divergem (`a ∈ (0,1)` por construção);
- Λ nasce com decaimento em [0,9 , 0,999].

---

## 14. Fase 1 — o que foi construído e o que foi medido

### Peças

```
src/backend/parallel.rs   backend multi-thread (o mesmo oráculo, 6 cores)
src/nn/embed.rs           tabela de embedding de bytes (256 × d)
src/nn/mlp.rs             bloco MLP com portão (SwiGLU), pré-norma, residual
src/ssm/block.rs          bloco recorrente: RG-LRU + portão paralelo, residual
src/model/patcher.rs      o compactador: trait + PorPalavra / PorClasse / Fixo
src/model/stack.rs        pilha de L camadas, com BPTT truncado entre janelas
src/model/hierarchy.rs    encoder local → pooling → backbone → broadcast → decoder
src/model/io.rs           salvar/carregar (a config vai gravada no arquivo)
src/learn/adam.rs         Adam com recorte de gradiente por norma global
src/learn/train.rs        laço de treino com cursores contíguos por lane
```

### Achado 1 — o patcher óbvio compactava metade do que devia

Quebrar sempre que a **classe do byte muda** é a regra natural, e foi a primeira
implementada. Medindo em português real:

| patcher | bytes/patch | backbone roda |
|---|---|---|
| `PorClasse` (quebra na mudança de classe) | 2,50 | 2,5x menos |
| `Fixo(4)` | 4,00 | 4x menos |
| **`PorPalavra`** (separador grudado na palavra) | **5,03** | **5x menos** |

O problema da regra por classe: o espaço vira um patch só dele, a palavra vira
outro. Em português, palavra ~4,5 letras + espaço = média de ~2,5.

Grudando o separador na palavra seguinte — patch = "espaço + palavra" — a média
dobra. Como a taxa de compressão **é** o multiplicador de velocidade do backbone,
essa mudança de três linhas vale um fator 2 no modelo inteiro.

### Achado 2 — a cabeça de saída nascia com ruído

Com inicialização Xavier normal na projeção final, os logits iniciais tinham desvio
~1 e o modelo recém-nascido marcava **9,4 bits/byte** em vez dos 8,0 do chute
uniforme sobre 256 símbolos. Ou seja: os primeiros passos de treino eram gastos
desfazendo o próprio ruído de inicialização.

Escalando a inicialização da cabeça por 0,05, o modelo nasce em **8,03 bits/byte**.
Além do ganho, "8,0 no passo zero" vira um sinal de sanidade confiável.

### Achado 3 — não existe um passo de diferença finita bom para todos

O erro da diferença central é `C·h² + |L|·ε/h`, e o `h` ótimo depende de `C` — a
curvatura — que varia várias ordens de grandeza **entre parâmetros da mesma rede**.
Medido: com `h = 1e-5` o embedding erra 1,4e-5 por truncamento; com `h = 1e-6` os
gradientes minúsculos erram 1,7e-5 por arredondamento.

A solução é o critério combinado (relativo **ou** absoluto), com piso absoluto de
1e-8 — que não esconde bug nenhum, porque um gradiente realmente errado erra por um
**fator**, não por 1e-8.

### Desempenho medido (Ryzen 5 5500, 12 threads, f32)

| GEMM | escalar | 12 threads |
|---|---|---|
| `gemm_nn` | 34,0 | 115,0 GFLOP/s |
| `gemm_tn` | 25,3 | 109,3 GFLOP/s |
| `gemm_nt` | 6,0 | 24,8 GFLOP/s |

| modelo | params | backbone | treino |
|---|---|---|---|
| `pequeno` | 1,52 M | 81% | 11.912 bytes/s (42,9 MB/h) |
| `padrao` | 11,64 M | 84% | 2.738 bytes/s (9,9 MB/h) |

Com o corpus de 5,4 MB, o preset `padrao` faz uma época em ~33 minutos.

### Corretude — 39 testes

O que está provado, não suposto:

- **gradiente do modelo inteiro** por diferenças finitas em f64: 65 tensores,
  3.046 parâmetros, pior erro relativo **4,4e-6**. Atravessa embedding → encoder →
  pooling nos fins de patch → backbone → broadcast causal → decoder → cabeça.
- **causalidade**: mudar o byte na posição `t` não altera nenhum logit anterior a
  `t`. É o teste que pega vazamento de futuro dentro do patch — o erro que faria o
  bits/byte ficar lindo e mentiroso.
- **backend paralelo == oráculo escalar**, bit a bit, inclusive nos gradientes.
  As três variantes fatiam por linhas de saída, então nenhuma soma muda de ordem.
- **continuidade de estado**: processar em janelas carregando o estado dá
  exatamente o mesmo que processar de uma vez.
- **o modelo aprende**: 8,00 → 0,02 bits/byte em 120 passos num corpus repetitivo.
- **estabilidade**: 4.000 passos com pesos `~N(0,10)` não divergem.
- **ida e volta em disco** preserva os pesos e a configuração.

### Primeiro treino em português (23/08/2026)

Preset `pequeno` (1,52 M params), 9 minutos, corpus de 5,38 MB de literatura
portuguesa de domínio público, `lr = 2e-3`, `seq = 256`, `batch = 8`:

```
     passo   bits/byte   b/patch   |grad|      bytes/s
         1      8.0023      4.98     0.91        10120     <- chute uniforme
       120      4.1233      4.98     0.38        11911
       560      2.8564      5.02     0.48        11374
      1500      2.4                                          (aprox.)
      2920      2.1534      4.96     0.51        11212

  validação (100 KB retidos, estado zerado): 2.3181 bits/byte
  2957 passos em 540s — 11.215 bytes/s — cérebro de 6,1 MB
```

**Comparação com a nila_mind.** O README dela registra ~2,2 bits/byte com o preset
de **10,06 M** parâmetros (12 experts, top-3) sobre ~5 MB do mesmo tipo de corpus.
A Teka chega a **2,32 bits/byte de validação com 1,52 M parâmetros e 9 minutos** —
mesma ordem de qualidade com **6,6x menos parâmetros**.

Ressalva honesta: o `mind_pulse.json` da Nyxara mostra 1,502 bits/byte, e esse número
**não é comparável** — é surpresa medida com estado quente sobre o próprio fluxo de
percepção (que inclui texto repetitivo gerado por ela mesma), não validação em trecho
retido com estado zerado. A comparação justa é contra os 2,2 documentados.

O `b/patch` ficou estável em ~5,0 ao longo do treino inteiro: o backbone rodou 5x
menos que o número de bytes, exatamente como projetado.

---

## 15. Fase 2 — a Teka age

### Peças

```
src/tools/seguranca.rs    modo sandbox (padrao), denylist, confinamento por raiz
src/tools/prim.rs         9 primitivas em Rust puro + FFI kernel32 + calculadora
src/tools/mod.rs          registro de ferramentas COMO DADOS, auto-documentado
src/grammar/mod.rs        registro → automato finito → mascara de 32 bytes
src/model/heads.rs        cabecas de intencao, ponteiro e presenca
src/model/agente.rs       pedido → compreensao → recorte → gramatica → execucao
src/learn/dados.rs        gerador de exemplos sinteticos, auto-verificavel
src/learn/supervisionado.rs  treino das cabecas + placar
```

### A gramática cumpre a promessa

O registro compila para **243 estados**. No início, exatamente **1 byte de 256** é
legal (`{`); depois de `{"acao":"`, apenas **7** (as iniciais distintas dos nomes de
ferramenta).

A prova está no teste `qualquer_caminho_aceito_e_uma_chamada_valida`: **4.000
passeios aleatórios** pelo autômato, escolhendo bytes ao acaso entre os permitidos.
Toda sequência aceita reparseia numa chamada válida, com todos os parâmetros
obrigatórios presentes, nenhum parâmetro estranho e nenhum argumento vazio.

E o teste `toda_resposta_do_agente_e_sintaticamente_valida` roda isso com o modelo
**não treinado** — pesos aleatórios, decisões sem sentido — e ainda assim: zero
chamadas malformadas. É essa a promessa: a confiabilidade sintática não depende de o
modelo estar bom.

### Achado 4 — a validação estava medindo memorização, não generalização

Este foi o erro mais caro da fase 2, e ele **não quebrou nada**: só produziu números
bonitos e falsos.

Dividindo treino/validação **por exemplo**, o placar deu:

| | |
|---|---|
| intenção | 100,0% |
| argumento | 100,0% |
| ponta a ponta | 100,0% |

O problema: os dois lados saem das mesmas ~57 frases-molde. Só os valores sorteados
mudam. Então 100% significava que o modelo decorou as frases — não que aprendeu a
tarefa.

Testando com frases genuinamente novas, escritas à mão: **4 de 10**.

```
ERR  e ai, que dia e hoje mesmo                     → {"acao":"memoria"}
ok   consegue me mostrar o que tem dentro da pasta target → {"acao":"listar_pasta","caminho":"target"}
ERR  preciso saber o conteudo do leia_me.txt        → {"acao":"escrever_arquivo",...}
ERR  sera que tem algum arquivo com nome config     → {"acao":"executar_comando","comando":"config"}
ok   me ajuda com essa conta 78*3                   → {"acao":"calcular","expressao":"78*3"}
ERR  ta sobrando memoria no pc                      → {"acao":"executar_comando","comando":"pc"}
ERR  o hd ta com quanto de espaco                   → {"acao":"procurar_arquivo","nome":"com quanto"}
ok   roda um dir ai                                 → {"acao":"executar_comando","comando":"dir"}
ok   mostra ai o que tem na pasta dados             → {"acao":"listar_pasta","caminho":"dados"}
ERR  da uma lida no arquivo notas.md por favor      → {"acao":"procurar_arquivo","nome":"notas.md"}
```

O conserto é [`dividir_por_frase`]: as frases da validação **nunca** aparecem no
treino, com teste provando que não vaza. Toda medida de agente daqui pra frente usa
essa divisão.

### Duas coisas que a falha confirma

**A gramática cumpriu.** 10 de 10 chamadas sintaticamente válidas, mesmo nas 6 em que
a decisão estava errada. Aquela garantia não depende de o modelo estar bom — era
exatamente a promessa.

**O ponteiro generaliza; a intenção não.** Repare nos argumentos: `target`, `78*3`,
`dir`, `dados`, `leia_me.txt`, `notas.md` — todos recortados corretamente, inclusive
nas linhas em que a ferramenta saiu errada. Faz sentido: apontar é uma tarefa
**estrutural** (o argumento é o substantivo depois da preposição), enquanto escolher
a ferramenta exige **semântica**.

### Resultados honestos (divisão por frase)

Preset `pequeno`, 4.231 exemplos de treino / 1.769 de validação, 10 épocas:

| | intenção | argumento | ponta a ponta | perda (val) |
|---|---|---|---|---|
| do zero — época 1 | 43,6% | 69,1% | 34,8% | 4,34 |
| do zero — época 10 | 63,8% | 90,4% | 58,0% | **14,46** |
| sobre o cérebro PT — época 1 | 53,1% | 63,9% | 45,4% | 11,04 |
| sobre o cérebro PT — época 10 | 56,8% | **95,5%** | 55,3% | **15,75** |

*(chute cego na intenção: 11,1%)*

**A perda de validação está explodindo** (4,3 → 14,5) enquanto a acurácia sobe
devagar. É overfitting clássico: o modelo fica confiantíssimo nas frases decoradas e
catastroficamente errado nas novas. Mais épocas está piorando — daí o registro da
melhor época no treino, em vez de reportar a última.

### O que falta para isso ficar bom

1. **Mais frases distintas.** 57 é pouco. É o gargalo, não o número de épocas.
2. **Taxas discriminativas.** Ajustar um tronco pré-treinado com a mesma taxa das
   cabeças recém-nascidas apaga o pré-treino nos primeiros passos — provavelmente a
   razão de o cérebro em português não ter ajudado a intenção. `CfgSup::lr_tronco`
   existe agora para isso.
3. **Corpus de pré-treino mais próximo do domínio.** O cérebro leu literatura do
   século XIX; os pedidos são fala informal. O tronco ajudou o ponteiro (tarefa
   estrutural) e não a intenção (tarefa semântica) — consistente com o descasamento.

### Achado 5 — o ponteiro sabia apontar, mas não sabia se abster

O primeiro treino deu 98,7% de intenção e produziu isto:

```
procura um arquivo chamado relatorio → {"acao":"procurar_arquivo","nome":"relatorio","raiz":"relator"}
quanto de espaco livre no disco      → {"acao":"disco","caminho":"quanto"}
```

Parâmetros **opcionais** sendo preenchidos com lixo recortado do próprio pedido. A
causa é estrutural: a cabeça de ponteiro só sabe apontar. Não havia nenhuma saída
que significasse "este argumento não aparece aqui", e o treino nunca supervisionava
o caso — os slots ausentes eram simplesmente pulados na perda.

O conserto é uma terceira cabeça, `presenca`: um logit por slot, treinado com
entropia cruzada binária. Obrigatório sempre entra (a gramática exige); opcional só
entra se ela disser que sim. Depois:

```
procura um arquivo chamado relatorio → {"acao":"procurar_arquivo","nome":"relatorio"}
quanto de espaco livre no disco      → {"acao":"disco"}    C:\ 452.3 GB usados de 476.0 GB
```

As duas chamadas passaram de falhar para funcionar.

### Achado 6 — o gradcheck estava medindo a aritmética, não o gradiente

Ao adicionar a cabeça de presença, o gradcheck acusou 1,8e-5 de erro relativo no
embedding. Erro relativo dessa ordem **não é assinatura de bug** — um gradiente
errado erra por um fator, não por 1e-5 — mas afrouxar a tolerância seria trocar uma
prova por uma esperança.

A causa é conhecida: a diferença central tem erro `C·h² + |L|·ε/h`, e o `h` ótimo
depende da curvatura `C`, que varia várias ordens de grandeza **entre parâmetros da
mesma rede**. Não existe um `h` bom para todos.

**Extrapolação de Richardson** resolve em vez de escolher: combinando `D(h)` e
`D(h/2)`, o termo em `h²` se cancela e sobra `O(h⁴)`.

```
D(h)   = f'(x) + C·h²   + O(h⁴)
D(h/2) = f'(x) + C·h²/4 + O(h⁴)
(4·D(h/2) − D(h)) / 3 = f'(x) + O(h⁴)
```

Custa duas avaliações a mais por parâmetro. Resultado: o pior erro relativo do agente
inteiro caiu de 1,8e-5 para **0,00** — todo gradiente agora bate abaixo do piso de
arredondamento. O teste voltou a medir o gradiente em vez da aritmética.

### Segurança

Três camadas, e nenhuma confia na anterior:

1. **Sandbox é o padrão.** Escrever e executar não fazem nada — descrevem o que
   fariam. É o que vai permitir a Teka propor ferramentas novas na fase 5 sem poder
   quebrar a máquina.
2. **Denylist** de padrões destrutivos, comparada em minúsculas e por substring:
   grosseiro de propósito, porque um falso positivo custa uma recusa e um falso
   negativo custa um disco.
3. **Confinamento por raiz**, com `..` normalizado **antes** da comparação — sem isso
   `raiz/../../Windows` passaria por um prefixo ingênuo. Tem teste.

### Corretude — 71 testes

Somando aos das fases anteriores:

- **gradiente do agente inteiro** (modelo + 3 cabeças): 74 tensores, erro relativo
  **0,00** com Richardson;
- **4.000 passeios aleatórios** pelo autômato, todos produzindo chamadas válidas;
- **agente não treinado** nunca emite chamada malformada;
- gerador de exemplos **auto-verificável**: um exemplo só entra se o span de patches,
  recortado de volta, reproduz exatamente o argumento pretendido;
- fuga da raiz por `..` recusada, denylist ativa, sandbox não toca no disco.

### O pré-treino de linguagem não ajudou (e a hipótese estava meio certa)

A arquitetura promete que o tronco é **o mesmo** do modelo de linguagem, então o que
ele aprendeu lendo 5 MB de literatura deveria ajudar a entender pedidos. Testado:

| configuração | intenção | argumento | ponta a ponta |
|---|---|---|---|
| do zero | 63,8% | 90,4% | 58,0% |
| sobre o cérebro PT, taxa única | 56,8% | **95,5%** | 55,3% |
| sobre o cérebro PT, tronco a 5% da taxa | 61,6% | 88,9% | **59,0%** |

A hipótese era que ajustar um tronco pré-treinado com a **mesma** taxa das cabeças
recém-nascidas apagaria o pré-treino nos primeiros passos. Ela se confirmou pela
metade: segurar o tronco recuperou a intenção (56,8 → 61,6), mas devolveu o ganho do
ponteiro (95,5 → 88,9).

**Ponta a ponta, as três configurações empatam dentro do ruído.** O pré-treino não
ajuda nesta escala. Duas leituras plausíveis, ambas consistentes com os números:

- **descasamento de domínio** — o cérebro leu português do século XIX, os pedidos são
  fala informal de hoje;
- **representação fraca** — 9 minutos de treino, 2,32 bits/byte. Não há muito ali
  para transferir.

O trade-off interno é informativo: segurar o tronco ajuda a **semântica** (intenção)
e atrapalha a **estrutura** (ponteiro). Faz sentido — o ponteiro precisa que o tronco
se adapte ao formato dos pedidos; a intenção precisa que ele preserve o que sabe da
língua. A conclusão não é "taxa única para o tronco inteiro", e sim taxas por
profundidade — camadas de baixo mais congeladas que as de cima.

### Achado 7 — `NaN` nunca é maior que nada

O rastreio da melhor época saiu como `melhor epoca: 0 — intencao NaN%`. A causa:
`Placar::default()` tem `n = 0`, `acuracia_total()` devolve `NaN`, e **toda**
comparação com `NaN` é falsa — inclusive `>`. O melhor placar nunca era atualizado.

É o mesmo padrão de bug que aparece em qualquer código que inicializa um "melhor até
agora" com um valor calculado a partir de estado vazio. A guarda é explícita:
`melhor_epoca == 0 || placar > melhor`.

### O gargalo era o gerador, e a conta fecha

Expandindo os moldes de **57 para 178 frases** (mesmo modelo, mesmo treino, só mais
variedade de fraseado):

| | 57 frases | 178 frases |
|---|---|---|
| intenção | 63,8% | **77,4%** |
| argumento | 90,4% | **95,3%** |
| ponta a ponta | 58,0% | **74,6%** |
| pico da perda de validação | 14,46 | **5,18** |

O pico da perda caindo de 14,5 para 5,2 é a confirmação de que o diagnóstico estava
certo: o problema era **overfitting por falta de variedade**, não capacidade do
modelo, não número de épocas, não falta de pré-treino. A melhor época passou de "a
última" para a 7ª, o que também é sinal de um treino saudável.

### Contaminação de conjunto de teste (erro de método)

Ao expandir os moldes, as 10 frases escritas à mão que serviam de teste foram
**adicionadas como templates** — pegar os casos que falham e transformá-los em dados
de treino é vazamento de conjunto de teste, e invalidaria qualquer número medido
com elas.

Com 12 frases genuinamente novas, escritas depois da expansão: **7 de 12**.

```
ok   me atualiza sobre o dia de hoje                 → {"acao":"hora"}
ERR  quais sao os itens guardados na pasta target    → {"acao":"hora"}
ERR  transcreve o que tem no arquivo notas.md        → {"acao":"escrever_arquivo",...}
ok   faz uma varredura atras de um arquivo config    → {"acao":"procurar_arquivo","nome":"config"}
ok   a ram esta sob pressao                          → {"acao":"memoria"}
ok   reproduz na tela o arquivo leia_me.txt          → {"acao":"ler_arquivo","caminho":"leia_me.txt"}
ok   resulta em quanto 1024*8                        → {"acao":"calcular","expressao":"1024*8"}
ERR  informa o instante atual                        → {"acao":"executar_comando","comando":"instante"}
```

Frases inéditas são mais duras que a validação por frase (74,6%) porque a validação
ainda compartilha vocabulário com as frases de treino da mesma ferramenta. **58% é o
número honesto de hoje.**

### Achado 8 — o ponteiro quebrava, mas não pelo motivo que eu escrevi primeiro

```
faz essa multiplicacao 78*3  →  {"acao":"calcular","expressao":"icacao 78*3"}
```

A primeira leitura foi "o recorte começou no meio de *multiplicacao*", o que sugere
um erro de **offset dentro do patch**. Olhando o patching de verdade:

```
patch 0: "faz"        patch 3: "icacao"
patch 1: " essa"      patch 4: " 78*3"   ← o argumento e EXATAMENTE este
patch 2: " multipl"
```

O argumento coincide perfeitamente com o patch 4. O modelo escolheu o **patch 3**.
É erro de **escolha de patch**, não de offset — e a diferença importa: um
refinamento confinado ao patch escolhido não consertaria isto. Só o teste do caso
real revelou a causa certa.

A causa de fundo é o teto do patcher partir palavras longas, o que torna o índice de
patch um péssimo sistema de coordenadas: `" multiplicacao"` tem 14 bytes e vira dois
patches, deslocando todos os índices seguintes de um jeito que depende do
comprimento das palavras anteriores.

**A solução é grosso-para-fino**, somando os dois níveis **antes** do argmax:

```text
score_byte(t) = score_patch(patch_de(t))  +  ⟨q_byte, e_t⟩ / √d
                └── o prior grosseiro ──┘    └── a correção fina ──┘
```

- o prior preserva o acerto que o nível de patch já tinha;
- o termo fino pode puxar o argmax para o byte certo mesmo quando o patch escolhido
  é o vizinho;
- como o prior entra **por soma**, o gradiente da perda por byte volta para os
  scores de patch — o nível grosseiro passa a ser treinado pelo objetivo que
  realmente importa;
- `consulta_byte` nasce **zerada**, então no primeiro passo o score de byte é
  exatamente o de patch: o nível fino começa neutro e só aprende a corrigir.

O gradiente agora atravessa também o encoder local (o termo fino lê `e`), o que
acrescentou um caminho novo ao backward — checado por diferenças finitas com
Richardson: 77 tensores, erro relativo **0,00**.

### O que o ponteiro fino entregou — e o que não entregou

| | patch-exato | byte-exato |
|---|---|---|
| argumento | 95,3% | **94,3%** |
| ponta a ponta | 74,6% | **75,1%** |

Mesmo número num critério bem mais duro: `acuracia_span` passou a medir **byte
exato**, que é o que decide se a ferramenta executa ou falha.

Mas o ganho agregado que se esperava **não apareceu**. O refinamento fino não move a
média — ele evita o erro catastrófico quando o teto do patcher parte uma palavra, e
esse caso é raro demais para aparecer numa média de 3.273 exemplos. O nível de patch
segue 1,5 ponto acima do de byte no critério dele (95,8% contra 94,3%).

### Duas hipóteses derrubadas por medição

**1. "Os argumentos-fragmento vêm da ferramenta errada."** A leitura vinha de cinco
anedotas fora da distribuição, em que argumentos ruins pareciam acompanhar escolhas
de ferramenta ruins. Instrumentando (`acuracia_span_dado_intencao`):

```
argumento (byte exato) ....... 94,3%
argumento | ferramenta certa   94,9%
```

Praticamente idênticos. O ponteiro erra na mesma taxa independentemente de a
ferramenta estar certa. A métrica ficou no código: é dela que sai a resposta de onde
investir esforço.

**2. "O nível de byte precisa do prior de fronteira de palavra."** A hipótese era que
apontar livremente para bytes tinha jogado fora um prior que o nível de patch tinha
de graça. Um viés aprendido por (slot, extremidade) sobre "abre palavra" / "fecha
palavra" — 16 parâmetros — **piorou**: argumento de 94,3% para 88,4%, ponta a ponta
de 75,1% para 70,1%. Revertido. As marcas de fronteira ficaram no `Plano` porque são
baratas e corretas, mas hoje ninguém as usa.

### Achado 9 — reportar uma época e salvar outra

O treino imprimia a melhor época e gravava os pesos da **última**. Como a perda de
validação sobe muito antes de a acurácia parar de subir, as duas divergem bastante:
75,1% na melhor época contra 67,7% na última. O número reportado descrevia um modelo
que ninguém ia rodar.

Conserto: instantâneo dos pesos na melhor época, restaurado antes de salvar. E o
modo `--avaliar` carrega o arquivo salvo e recalcula tudo, para que a verificação não
dependa de acreditar no log do treino.

### Erros de método desta rodada

Vale registrar junto com os achados, porque custaram tempo real:

- **Comparei dois modelos sem verificar que um deles carregou.** A guarda de versão
  funcionou (`arquivo tem 115 tensores, o agente espera 116`), mas o script de
  comparação engoliu a mensagem e imprimiu dez linhas em branco como se fossem
  respostas.
- **Generalizei de dez anedotas antes de medir.** As duas hipóteses acima nasceram
  de olhar saídas, e as duas caíram quando viraram número.
- **Contaminei o conjunto de teste** ao transformar em templates as frases que
  falhavam (registrado no Achado 4).

---

## 16. Fase 3 — memória episódica e consolidação

### Peças

```
src/memory/mod.rs          Episodio, MemoriaEpisodica, recuperacao, destilacao
src/memory/io.rs           persistencia (a memoria sobrevive a reinicio)
src/learn/consolidacao.rs  o "sono": fixa nos pesos o que foi corrigido
main.rs                    REPL com /certo, /errado, /parecido, /consolidar
```

### Uma estrutura, duas funções

A mesma lista de episódios é **memória de longo prazo** (buscar por similaridade o
que já aconteceu) e ***replay buffer*** (amostrar lotes para consolidar). Não é
economia de código: consolidar a partir de lotes que misturam episódios novos e
antigos é precisamente o que impede o esquecimento catastrófico.

Cada episódio guarda a **assinatura** — o estado do backbone no último patch, que é
a leitura que ela fez do pedido. É por cosseno entre assinaturas que se recupera "o
que já aconteceu parecido com isto". Funciona sobre sentido, não sobre palavras:

```
"como esta a ram do computador"  →  recuperou  "quanto de memoria esta em uso"  (cos 0,752)
```

### Destilação — a compressão que o §7 prometeu

`destilar(limiar)` colapsa episódios com decisão idêntica e assinatura quase igual
num só, somando o peso. Cinquenta pedidos de "ver disco" viram um padrão com peso
50, não cinquenta cópias.

O detalhe que decide se está certo: **só colapsa quando a decisão ensinada é a
mesma**. Dois pedidos parecidíssimos com decisões diferentes são exatamente o par
mais informativo que existe, e o teste
`destilar_colapsa_repeticao_mas_preserva_discordancia` trava isso.

### O laço fechado, ao vivo

```
teka> modo turbo              → {"acao":"hora"}          ← errado
teka> /errado memoria           corrigido para memoria
      (mais duas correcoes)
teka> /consolidar               destilacao colapsou 1 episodio repetido
                                consolidado: 2 episodios
                                intencao 93,7% → 93,1%
teka> modo turbo              → {"acao":"memoria"}       ← aprendeu
                                RAM: 12.1 GB em uso de 15.9 GB
```

Uma expressão que não existe em nenhum template, aprendida com três correções, ao
custo de **0,6 ponto** na validação.

### O experimento com controle

Mostrar que ela aprende o que foi corrigido não prova nada — qualquer treino faz
isso. O que precisa ser demonstrado é que o mecanismo anti-esquecimento carrega
peso. A única forma honesta é desligá-lo:

| | correções | intenção (val) | ponta a ponta (val) |
|---|---|---|---|
| antes | 3/6 | 80,3% | 69,8% |
| consolidou **com** replay | **6/6** | 88,2% | 83,0% |
| consolidou **sem** replay | **6/6** | 79,0% | 73,7% |

**Leitura honesta.** O "+7,9 pontos com replay" **não é mérito do replay**: o modelo
base treina poucas épocas neste teste (ele precisa rodar em ~1 minuto) e não
converge, então consolidar com replay é, em parte, apenas mais treino no corpus
base. O efeito limpo do mecanismo é o outro lado — **sem replay a intenção cai**.

E a queda medida é modesta (~1 ponto), não catastrófica, porque a pressão de
esquecimento aqui é fraca: 36 episódios de correção contra milhares de exemplos já
aprendidos, com taxa de consolidação uma ordem de grandeza abaixo da do treino
inicial. **O teste prova a direção do efeito, não a magnitude que ele teria depois
de meses de uso.** Quem for mexer nisso precisa saber disso antes de confiar no
número.

### Por que a taxa de consolidação é baixa

Consolidar não é treinar do zero: os pesos já estão bons. Taxa alta com poucos
exemplos novos move o modelo para longe do que ele sabia antes de os novos dados
terem estatística para justificar. Padrão: `2e-4`, contra `1,5e-3` do treino inicial.

---

## 17. Fase 4 — reforço sobre uso real

### Peças

```
src/learn/reforco.rs   recompensa, vantagem por pedido, REINFORCE, replay
model/heads.rs         peso por exemplo no gradiente da intenção
model/agente.rs        responder_explorando — amostra da política em vez de argmax
```

### REINFORCE é entropia cruzada com peso

```text
entropia cruzada, alvo a:   ∇ −log π(a)
REINFORCE, ação a:          ∇ −(r − b)·log π(a)
```

O mesmo gradiente, escalado pela vantagem. Não houve caminho novo de backward para
escrever nem para provar: bastou um peso por exemplo, e todo o percurso já
verificado por diferenças finitas continua sendo o único percurso. Vantagem negativa
empurra para longe da ação tomada — é assim que uma falha ensina **sem ninguém dizer
qual era a resposta certa**.

### Achado 10 — a tabela de recompensa do §8 tinha reward hacking embutido

O §8 previa recompensa positiva para "a ferramenta executou sem erro". Só que `hora`
**nunca** falha, e `memoria` também não. Recompensar sucesso ensina a chamar as
ferramentas infalíveis para tudo.

O conserto é assimetria: **falha é informativa, sucesso não é.** Sucesso entra
valendo **zero** — não como prêmio, mas como o ponto de comparação sem o qual a
falha não significa nada.

### Achado 11 — o baseline global anulava o reforço inteiro

Este só apareceu relendo o próprio teste depois que ele passou.

A primeira versão usava média móvel global das recompensas. Como todas as transições
do teste eram falhas, a EMA convergiu para o valor da falha e **toda vantagem virou
zero**. O reforço não fez absolutamente nada — e o teste passou assim mesmo, porque
o replay supervisionado mexeu numa decisão por acaso. Um teste verde descrevendo um
mecanismo desligado.

A correção não é remover o baseline: é calculá-lo **dentro de cada pedido**.

```text
vantagem(pedido, ação) = r − média das recompensas DAQUELE pedido
```

Resolve dois problemas de uma vez: dá variância onde ela existe (o mesmo pedido
tentado com ferramentas diferentes), e mata o resto do reward hacking (a ferramenta
infalível só ganha vantagem se for melhor **naquele** pedido, não em geral).

### O que está demonstrado

| propriedade | resultado |
|---|---|
| sucesso silencioso não gera pressão | 48 transições, **0** com vantagem não-nula |
| falha gera sinal sem rótulo | recompensa negativa, vantagem não-nula |
| âncora contra recompensa mentirosa | **6/6** decisões corretas mantidas |
| vantagem soma zero dentro do pedido | provado por teste |

A âncora é a mais interessante: marcando decisões **corretas** como falhas, o replay
supervisionado sobrepôs o sinal mentiroso em 6 de 6. Esse teste nasceu de um erro
meu de desenho — a primeira versão do teste de aprendizado usava essas frases e
falhava, e a "falha" era o comportamento certo.

### O crítico — e o que ele mudou

O baseline por pedido só existe para pedidos **repetidos**. Num pedido visto uma
vez, a média é a própria recompensa, a vantagem sai zero, e nada é aprendido. Era a
limitação que aparecia nos números do laço fechado: **20 de 30** transições úteis.

`V(s)` é aprendido a partir do estado, então um pedido novo herda a estimativa dos
pedidos parecidos. Com ele: **30 de 30** transições úteis.

| | sem crítico | com crítico |
|---|---|---|
| transições úteis | 20/30 | **30/30** |
| acertos no ambiente | 4/6 oscilando, termina em 4 | **4/6 → 5/6, estável** |
| intenção (validação) | 77,3% → 73,9% (caiu 12 pts no meio) | 77,3% → **84,1%** |
| erro do crítico | — | 0,033 → **0,023** |

```text
antes:    4/6 | intencao(val) 77,3%
rodada 1: 5/6 | erro critico 0,033 | 86,4%
rodada 3: 5/6 | erro critico 0,032 | 83,0%
rodada 5: 5/6 | erro critico 0,023 | 84,1%
```

O laço fechado passou a **aprender do desfecho sem ninguém dizer a resposta**, e a
melhorar a validação no caminho — o replay supervisionado continua treinando junto.

### Achado 12 — o crítico não pode moldar o tronco

A primeira versão deixava o gradiente do crítico fluir para o tronco, como é comum
em ator-crítico com corpo compartilhado. Resultado medido: o erro do crítico saltou
de 0,05 para **171** e arrastou a acurácia de intenção de 77% para 56% em cinco
rodadas.

O tronco existe para **entender pedidos**. Deixá-lo ser puxado por "prever
recompensa" troca a tarefa que importa por uma auxiliar. O crítico compartilha a
representação, mas não a molda: o gradiente dele para em `V`.

Consequência para os testes, que vale registrar: **um caminho destacado é invisível
para diferenças finitas.** Perturbar um peso do tronco muda `zf` e portanto muda `V`,
mesmo com o gradiente cortado — então o numérico veria uma contribuição que o
analítico corretamente não tem. Por isso o gradcheck do agente roda com
`alvo_valor = None`, e a corretude dessa cabeça vem do gradcheck do `Linear`, que é
o que ela é.

### Achado 13 — um baseline ruim é pior que baseline nenhum

Mesmo destacado do tronco, o crítico ainda derruba a política se divergir. Com
`lr = 5e-4` (5x o padrão) o erro dele foi a 40 e a intenção caiu para 47%.

O mecanismo: a vantagem é `r − V`. Um `V` disparado vira empurrão constante no teto
do recorte, em **todos** os exemplos de reforço — sinal errado com magnitude máxima.
Um baseline que só não reduzisse variância seria inofensivo; um que erra
sistematicamente injeta ruído estruturado.

Por isso o erro do crítico entrou no relatório e virou asserção de teste. É o número
que separa "reforço funcionando" de "reforço empurrando lixo", e ele não aparece em
nenhuma métrica de acurácia.

### O que ainda falta

O ambiente do teste tem 6 pedidos e ela chega a 5. O caso restante e a taxa de
aprendizado estreita (estável em `1e-4`, instável em `5e-4`) sugerem que falta
**recozer a temperatura de exploração** — hoje fixa em 1,0, o que mantém a
amostragem quase uniforme mesmo depois de a política já saber a resposta.

### Recozimento da exploração

Faltava fechar o ciclo: com `T = 1,0` fixo, a amostragem continua quase uniforme
mesmo depois de a política já saber a resposta — ela desaprende o que acabou de
aprender.

O critério certo não é "quantas rodadas passaram", é **quantas vezes aquele pedido
já foi tentado**. Um pedido novo merece ser experimentado mesmo que a Teka já esteja
velha de casa; um pedido tentado dez vezes merece a melhor resposta conhecida.

```text
T(n) = t_final + (t_inicial − t_final) · exp(−n / escala)
```

Com `t_final = 0,15` — não vai a zero de propósito: um piso pequeno mantém a porta
aberta para o mundo mudar de ideia sobre qual ferramenta funciona.

| | T fixo em 1,0 | recozido |
|---|---|---|
| acertos | 4/6 → 5/6 na rodada 2, oscilando | **5/6 já na rodada 1, estável** |
| recompensa final | −0,127 | **−0,107** |
| erro do crítico | 0,023 | 0,030 (estável) |

```text
rodada 1: 5/6 | T=0,60 | recompensa -0,200 | intencao(val) 86,4%
rodada 3: 5/6 | T=0,28 | recompensa -0,122 | intencao(val) 84,1%
rodada 5: 5/6 | T=0,19 | recompensa -0,107 | intencao(val) 83,5%
```

Continua em 5 de 6. O caso restante é `escrever_arquivo`, a única ferramenta do
ambiente com **dois** argumentos — e o reforço só treina a cabeça de intenção, não o
ponteiro. Faz sentido que seja o mais difícil, e aponta o próximo lugar a olhar.

### O laço no REPL

```
teka> /explorar          liga o modo de experimentar
teka*> modo turbo        (amostra em vez de pegar a melhor)
teka*> /reforcar         aprende do desfecho
teka*> /aprender         consolidação + reforço, na ordem certa
```

`/aprender` roda a consolidação **antes** do reforço: a correção explícita é o sinal
mais forte que existe, e o reforço deve partir de uma política já corrigida.

O prompt muda para `teka*>` quando a exploração está ligada — a Teka fica
deliberadamente não-determinística nesse modo, e isso precisa estar visível.

E se o erro do crítico passar de 0,5, o REPL avisa. É o número que separa "reforço
funcionando" de "reforço empurrando ruído", e ele não aparece em nenhuma métrica de
acurácia.

## 18. Fase 5 — um LLM local escreve os dados

O problema que motivou isto não é técnico. Corrigir a Teka uma frase por vez, no
REPL, é lento demais para produzir as centenas de exemplos que ela precisa — e este
projeto já mostrou cinco vezes que **dado bate arquitetura**.

A saída é destilação: um modelo grande escreve, o modelo pequeno aprende. O grande
roda no LM Studio, offline, e o produto dele é um **arquivo de texto**. O runtime da
Teka continua sem rede e sem dependência — `src/gerador/` só existe para o
desenvolvedor, nunca para ela.

### Quarentena, não anexo direto

A primeira versão do plano anexava direto em `dados/exemplos_teka.txt`. Medi antes de
implementar e desisti: com `ministral-3-3b`, ~40% da saída era lixo ou desvio de
intenção. Não erro de forma — erro de **sentido**:

```text
✓ quanto do total esta sendo consumido no momento
✗ quando a minha memoria vai comecar a falhar        (previsão, não status)
✗ sou feliz que ainda tenho margem para mais de nada  (sem sentido)
```

E o filtro que já existia — o de span — **não pega nada disso**, porque `hora`,
`memoria` e `disco` não têm argumento; não há o que validar. As ferramentas mais
indefesas seriam justamente as mais envenenadas.

Por isso a saída vai para `dados/propostas.txt` e a aprovação é humana. O corpus é o
ativo mais valioso do projeto; 40% de ruído nele custaria mais do que todo o tempo
economizado.

### Os três filtros

1. **Forma** — linha parseável, 6 a 90 bytes, sem duplicata contra o corpus, contra a
   quarentena, **nem contra o benchmark** (que deixaria de medir generalização).
2. **Span** — o argumento tem de aparecer idêntico dentro do pedido. O ponteiro
   copia, não inventa.
3. **Semântico** — embedding da frase contra o **centroide de cada ferramenta**,
   calculado a partir dos exemplos já aprovados. Aceita só se o alvo for o mais
   próximo. Medido contra `"como anda a ram"`: mesma intenção 0,43, outra ferramenta
   0,15, lixo 0,08 — folga de ~3x.

O terceiro é o que ataca o desvio de intenção, e é o que justifica ter um modelo de
embedding no laço.

### Achado 14 — a heurística de recorte matava três ferramentas

Nas primeiras três rodadas, `procurar_arquivo`, `escrever_arquivo` e
`executar_comando` deram **zero exemplos aceitos** — nos três modelos testados.

Diagnostiquei como culpa do modelo e estava errado. A causa era minha
`achar_argumento`, que tentava localizar o argumento dentro da frase por marca formal
(barra, ponto, sinal de operação) e devolvia `None` quando não achava:

```text
"lista a pasta src\model"   → src\model    ✓ tem barra
"procura o relatorio"       → None         ✗ e agora?
"executa o dir"             → None         ✗
```

Não existe como recortar `"procura o relatorio"` sem saber que `relatorio` era o
alvo. **Quem sabe é quem escreveu a frase.** A heurística estava tentando recuperar
uma informação que o gerador tinha e jogou fora.

A correção troca heurística por **contrato**: o modelo é obrigado a declarar o
argumento depois de `|`, e o filtro só verifica se ele aparece idêntico no pedido.

```text
antes:  procura o relatorio
depois: procura o relatorio | relatorio
```

| | heurística | contrato |
|---|---|---|
| `sem argumento` (descartes) | 42 | **3** |
| ferramentas com exemplos | 6 de 9 | **9 de 9** |
| aproveitamento (qwen2.5-7b) | 59% | **72%** |

Ganho comparável ao de trocar de modelo — e de graça. A lição repete a do Achado 8:
**o primeiro diagnóstico culpou o componente mais visível**, e a medição por
ferramenta mostrou que o defeito era do lado de cá.

O efeito colateral que mais importa: `escrever_arquivo`, a única ferramenta de dois
argumentos e exatamente onde o reforço empacava em 5/6, finalmente tem dados com os
dois spans corretos.

### Escolha do modelo — medida, não chutada

Errei duas recomendações antes de medir. A primeira, `qwen3.5-4b`, é modelo de
**raciocínio**: gasta 100% dos tokens em `reasoning_content` e devolve `content`
vazio, e nesta build nem `enable_thinking:false` nem `/no_think` desligam. A segunda,
`gemma-3-12b-it`, não cabia — eu raciocinei sobre RAM *total* em vez de RAM *livre*.

Com o código já corrigido, 108 pedidos por modelo, mesmos parâmetros:

| modelo | aceitas | taxa | ferramentas vivas |
|---|---|---|---|
| ministral-3-3b | 44 | 39% | 5 de 9 |
| gemma-3-4b-it | 59 | 55% | 9 de 9, fraco em 3 |
| **qwen2.5-7b-instruct** | **78** | **72%** | **9 de 9** |

O 12B nunca foi necessário: o que o 3B errava era **seguir instrução**, e isso melhora
forte de 3B para 7B e pouco de 7B para 12B. `pedir_frases` hoje detecta o `content`
vazio e sugere trocar por um instruct puro, para o próximo não cair na mesma armadilha.

### O que ainda falta

O benchmark tem 24 frases. Se centenas de exemplos entrarem no corpus, ele fica
pequeno demais para arbitrar a diferença — precisa crescer para ~60 **antes** da
primeira aprovação em massa, e crescer à mão, porque um benchmark escrito pelo mesmo
modelo que escreve o treino não mede nada.

## 19. Fase 6 — a régua, e o que ela revelou

### Achado 15 — o benchmark de 24 estava lisonjeando

Com 24 frases, cada uma vale **4,2 pontos**. Depois que o gerador encheu a fila com
256 propostas, ficou impossível arbitrar: qualquer mudança mexeria o número dentro do
próprio ruído da régua.

O benchmark foi para **59 frases**, escritas à mão, nenhuma delas no gerador (dois
testes garantem isso). Cada grupo novo mira um fenômeno que o corpus cobre mal: frase
declarativa em vez de ordem, pergunta em vez de comando, vocabulário incomum, e
acento — que o corpus de treino quase não tem.

O mesmo `teka.bin`, sem retreinar nada, nas duas réguas:

| | benchmark de 24 | benchmark de 59 |
|---|---|---|
| ferramenta certa | 22/24 — **92%** | 49/59 — **83%** |
| argumento certo | 14/15 — **93%** | 27/38 — **71%** |

Os 92% eram reais e eram enganosos ao mesmo tempo: mediam bem as frases que a régua
tinha, e a régua tinha as frases fáceis. **Nenhum modelo piorou — a medição melhorou.**

O que as 35 frases novas expuseram, e que as 24 escondiam:

```text
chama o ipconfig no terminal   → {"acao":"procurar_arquivo","nome":"g"}
manda um whoami ai             → {"acao":"hora"}
queria disparar o systeminfo   → {"acao":"hora"}
salva a observacao urgente ... → {"texto":"rgente"}
```

Três padrões, nenhum aleatório:

1. **Valor inédito quebra a ferramenta.** `executar_comando` tinha 4 comandos
   distintos no corpus inteiro. `ipconfig`, `whoami` e `systeminfo` são inéditos e os
   três falharam. Não é falta de frase — é falta de **valor**.
2. **`hora` é o ímã da incerteza.** É a ferramenta com mais exemplos e sem argumento:
   quando nada casa, ela cai ali. Um viés de prior, não de compreensão.
3. **O ponteiro fino erra por bytes.** `"rgente"` em vez de `urgente`, `"g"` em vez de
   `ipconfig`. O nível grosseiro acerta o patch e o fino escorrega dentro dele.

### Achado 16 — metade da decisão nunca foi reforçada

`Alvo::reforco` ligava `apenas_intencao: true`. O comentário justificava assim:

> Um episódio aprendido por reforço não traz argumento anotado. Deixar as cabeças de
> ponteiro e presença verem `None` ensinaria "este pedido não tem argumento", que é
> falso.

A premissa está certa e a conclusão era forte demais. O episódio não traz o argumento
**certo** — mas traz o que ela **escolheu**, em `Episodio::args`. E para REINFORCE é
exatamente essa a informação necessária: o gradiente de `−(r − b)·log π(a)` precisa da
ação tomada e do sinal, nunca da resposta certa. É a mesma lógica que já funcionava na
cabeça de intenção, aplicada a uma cabeça diferente.

A consequência de deixar assim era severa e silenciosa: **corrigir um argumento errado
não ensinava nada.** O gradiente ia só para a intenção. O platô de 5/6 do laço fechado
era `escrever_arquivo` — a única ferramenta de dois argumentos — e nenhuma correção do
usuário poderia tê-lo resolvido.

O conserto tem duas metades, e a segunda é a que quase passou batido:

1. `Transicao` passa a carregar `args`, e `alvo_de_transicao` monta o alvo
   reaproveitando `Exemplo::alvo` — que já converte span de byte em span de patch **e
   valida por ida e volta**. Se o recorte não reproduz o valor, cai para só-intenção
   em vez de ensinar um ponteiro torto.
2. As perdas de presença e ponteiro passam a ser escaladas por `alvo.peso`, como a de
   intenção já era. **Sem isto o conserto seria pior que o problema**: um episódio de
   recompensa negativa entraria com peso 1,0 e *reforçaria* o span errado em vez de
   fugir dele.

Com `peso = 1,0` — todo alvo supervisionado — a mudança é a identidade. E o placar
deixa de contar alvos de reforço, porque a escolha dela não é gabarito e poluiria o
relatório de treino com a própria opinião do modelo.

### Achado 17 — o cérebro de produção era o cérebro de depuração

```rust
let cfg_modelo = match args.preset.as_str() {
    "padrao" => Config::padrao(),
    _ => Config::pequeno(),      // ← fall-through
};
```

`Config::pequeno` existe para "iterar rápido em CPU enquanto se depura o treino". Por
causa desse `_`, ele virou o padrão de produção sem ninguém decidir: **1.515.904
parâmetros no lugar dos 11.640.832** que a fase 1 projetou — 13% do alvo. Todos os
resultados publicados até aqui saíram do modelo de depuração.

Um erro de digitação em `--preset` também levava para lá, calado. Virou
`resolver_preset`, que recusa o que não reconhece.

Vale dizer o que isto **não** significa: não há evidência de que crescer resolva. A
lição repetida deste projeto é que dado ganhou de arquitetura em todas as medições, e
o pré-treino deu ~zero. O que incomoda é que a comparação nunca foi feita.

## 20. Fase 7 — duvidar, lembrar, e o que veio do bite3.0 e da Nyxara

### Achado 18 — abster por limiar não funciona; abster tem de ser aprendido

A Teka sempre escolhia uma ferramenta. Não existia "não sei", e era assim que ela
errava pior:

```text
manda um whoami ai              → {"acao":"hora"}
o consumo de ram esta alto      → {"acao":"procurar_arquivo","nome":"alto"}
```

A primeira tentativa foi o caminho óbvio: recusar quando a confiança fosse baixa.
Três sinais medidos lado a lado, no mesmo modelo, no mesmo benchmark:

| sinal | média nos acertos | média nos erros | melhor saldo |
|---|---|---|---|
| margem `p1 − p2` | 0,973 | **0,833** | +1 de 10 erros |
| crítico `V(s)` | **0,000** | **0,000** | 0 |
| menor argumento / pedido | 0,494 | **0,711** | +1, **invertido** |

Três coisas que a medição disse e a intuição não diria:

1. **Ela erra convicta.** 0,833 de margem nos erros não deixa limiar nenhum separar.
2. **O crítico estava morto.** Saía exatamente `0,000` — aquele modelo nunca passou
   por reforço, então a cabeça nasceu perto de zero e nunca foi treinada. Era um
   número morto prestes a virar sinal de confiança.
3. **A hipótese do argumento estava de cabeça para baixo.** Recorte curto (`"g"` de
   `ipconfig`) parecia sinal de confusão; os erros têm argumento **maior**.

Então `perguntar` virou a **décima ação da cabeça de intenção**, treinada com pedidos
fora de escopo — que é o que o bite3.0 faz: 35% do corpus de decisão dele não é "usar
ferramenta" (`ask_user` 1.464, `observe` 1.337, `wait` 1.003 de 11.025).

E isto é o que despacho por palavra-chave **não consegue ter**. O `executor.py` da
Nyxara tem 6.797 linhas e 33 sítios de `any(k in norm_text for k in [...])`. Um
casamento de substring casa ou não casa; não existe uma décima saída que signifique
"nenhuma das anteriores, e eu sei disso". Política probabilística é o que dá o
direito de duvidar.

### Achado 19 — exemplo negativo não pode dividir vocabulário com o positivo

A primeira safra de `perguntar` custou **8 abstenções falsas em 59 pedidos válidos**,
algumas com margem 1,000 — recusa convicta do que ela sabia fazer. E um viés no logit
não consertou nada: de −4 a +1, o resultado em escopo ficou parado em 42/59, porque
convicção alta demais não se move com deslocamento de logit.

A causa estava nos dados, e era minha:

| ela passou a recusar | porque eu ensinei como fora de escopo |
|---|---|
| `hora` — "que dia sera **amanha**" | "vai chover **amanha**", "**que horas** o mercado abre" |
| `executar_comando` — "**manda** um whoami ai" | "**manda** um email", "**manda** mensagem no whatsapp" |
| `procurar_arquivo` — "localiza… esboco" | "**pesquisa** no google", "baixa esse video" |

O modelo não aprendeu "isto está fora de escopo". Aprendeu **"`manda` = fora"**,
**"`amanha` = fora"**. Verbo compartilhado vira sinal, e o sinal vaza para dentro.

A regra que faltava, agora escrita ao lado da lista: **exemplo fora de escopo não
pode compartilhar o verbo de ação nem o domínio do objeto com nenhuma ferramenta
real.** Trocar ~20 frases:

| | antes | depois |
|---|---|---|
| em escopo | 42/59 — 71% | **46/59 — 78%** |
| fora de escopo | 6/10 — 60% | 6/10 — 60% |
| abstenções falsas | 8 | **4** |

Contra o mesmo corpus sem `perguntar` (76%), a ação de abster deixou de custar
acurácia — passou a somar.

### O que veio do bite3.0

**Oficina** (`tools/oficina.rs`), do `ShadowWorkspace`. A política só tinha dois
estados e os dois eram ruins: sandbox não deixa testar, real não deixa errar. A
oficina é o degrau do meio — copia a pasta, a Teka mexe **de verdade** na cópia, e o
`diff` decide. `aplicar` guarda backup de cada arquivo antes de sobrescrever.

Isto é melhor que pedir permissão antes, que é o que a Nyxara faz
(`_request_permission`, timeout de 300s): autorizar uma escrita antes de ver o
resultado é decidir no escuro. Aqui a pergunta vira "isto está certo?" em vez de
"posso tentar?".

**Diário** (`tools/diario.rs`), do `tool_journal.py`. Grava `iniciado`, **sincroniza
com o disco**, executa, grava `concluido`. `iniciado` sem `concluido` é `Incerto` e
falha fechada. O `fsync` é o ponto inteiro: sem ele o registro pode ficar em buffer e
a próxima tentativa pareceria a primeira. Importa mais aqui que num agente comum —
**o laço de reforço reapresenta o mesmo pedido de propósito**, então repetir é o caso
normal.

### Achado 20 — a repetição espaçada da Nyxara estava invertida

`memory/semantica.rs` nasceu do `memory_tiers.py` e do `ebbinghaus_forgetting.py`
dela. As camadas são boas e foram copiadas inteiras: `Nucleo` (piso 1,0, imune),
`Estavel` (0,4, 90 dias), `Provisorio` (7 dias), `Trabalho` (0,5 dia).

A fórmula de força, não. O `compute_memory_strength` dela faz

```python
strength = base_retention * boost * emotional_factor * importance_factor
boost = ebbinghaus_retention(last_retrieved_days, extended)   # = 0.5 ** algo
```

O nome engana: `boost` é `0,5^algo`, sempre ≤ 1 — é um **segundo decaimento**, não um
impulso. E como um fato nunca lembrado recebe `boost = 1,0` exato (`if retrievals <=
0`), **lembrar de um fato o deixa mais fraco do que nunca tê-lo lembrado.** Copiada
fielmente, a fórmula deu 0,062 depois de quatro acessos contra 0,104 sem acesso
nenhum, e o teste acusou.

Repetição espaçada **estende a meia-vida**; não multiplica uma segunda curva. E o
relógio corre desde o **último reforço**, não desde a criação — é isso que a torna
espaçada:

```text
meia_vida_efetiva = meia_vida(camada) · 2^min(acessos, 6)
força = 0,5^(dias desde o último reforço / meia_vida_efetiva) · importância
```

### Achado 21 — o embedding sai de graça, mas o espaço gira

A Nyxara resolve busca semântica com Chroma mais um modelo de embedding externo. A
Teka não precisa: o tronco já produz a **assinatura** na mesma passada que decide a
ferramenta. Um modelo, uma passada, dois usos — e melhor alinhado, porque foi treinado
para separar intenção, não similaridade genérica.

O preço é real e a Nyxara não o paga: o embedder dela é congelado, **o da Teka muda a
cada treino**. Assinatura guardada com um modelo não é comparável com a de outro.
Ignorar isso apodreceria a memória em silêncio — os cossenos continuariam saindo, sem
significado, sem erro nenhum.

Por isso o texto é a fonte da verdade e a assinatura é derivada, com a marca do modelo
que a gerou. Modelo diferente ⇒ o fato sai da busca até ser reindexado.

### O que veio do grafo da Nyxara, sem o Neo4j

O grafo dela tem **775 nós e 1.000 relações** — os 3,2 GB em disco são store vazio. Um
serviço separado e 3,2 GB para guardar 551 fatos de 116 bytes não cabe num agente que
roda em 6 MB. As três ideias couberam sem o banco:

**Supersessão** (`SUPERSEDED_BY`, 5 arestas). Sem ela, "prefiro resposta curta" e
"prefiro resposta longa" coexistem e a busca devolve as duas: a memória fica
**errada**, não só grande. `substituir` tira o velho da busca mas não do arquivo, e o
novo **herda camada e confirmações** — corrigir um fato consolidado não deve fazê-lo
renascer provisório.

**Procedência** (`VEM_DE`, 578 arestas). Hoje a Teka só tem uma fonte, então parece
excesso. Está aqui porque procedência **não dá para retrofitar**: quando a segunda
fonte aparecer, o histórico acumulado não tem como ganhar a informação depois. E o
peso por fonte impede o pior caso — sem ele, uma inferência errada da própria Teka
vira "fato" com o mesmo peso de algo que o dono falou, e a memória se autoconvence.

**Grafo de conceitos** (`Conceito` + `RELACIONADO_A`). A estrutura e a travessia
existem; o **extrator não**. Na Nyxara um LLM extraía conceitos de cada fato, e a Teka
não tem LLM no runtime. Enquanto ninguém alimentar `ligar`, o grafo fica vazio e a
expansão não faz nada — há teste garantindo que esse é o comportamento.

Um erro de raciocínio que o teste pegou: eu pontuava o vizinho por cosseno com a
busca. Mas ele chega por associação **justamente porque** o cosseno dele é ~0 — era
isso o ponto. Pontuar assim anulava a expansão inteira. Agora o vizinho herda a nota
da semente com penalidade 0,5, o que também garante que salto nunca passe na frente de
quem casou direto.

## 21. Achado 22 — o chão de ruído, e o que ele apaga

Este achado corrige vários dos anteriores. Vem antes de qualquer outra leitura
deles.

### A medição

Mesmo corpus, mesmo código, mesmos hiperparâmetros. **Só a semente do modelo
mudou**, de 7 para 8 — um número que não significa nada:

```text
predicoes diferentes ......... 17 de 69
acerto <-> erro trocou ........  5
em escopo ..................... 46/59 (78%)  ->  48/59 (81%)
fora de escopo ................  6/10 (60%)  ->   7/10 (70%)
```

**Trocar nada mexeu 3 pontos.** E o benchmark inteiro tem 69 frases, então 3 pontos
são 2 frases.

Uma segunda medição, independente, dá o mesmo recado: três modelos treinados com
corpora e capacidades diferentes — `pequeno` 1,6M, a variante sem 106 exemplos, e
`padrao` com 12M — acertaram **exatamente 46/59 cada um**, errando 10 frases em comum
e 13 que trocam de lugar conforme a rodada.

Sinal: 10 erros. Ruído: ~13 a 17 predições que se remexem sozinhas.

### O que isso apaga

| afirmado antes | o que a medição sustenta |
|---|---|
| "os dados gerados pioraram: 83% → 76%" | 4 frases — **dentro do ruído, não sustenta** |
| "corrigir o vocabulário deu 71% → 78%" | 4 frases — no limite, fraco |
| "o benchmark de 24 lisonjeava" (§19) | ✓ 92% → 83% são 9 pontos, acima do ruído |
| "`padrao` não vale a pena" | ✓ três modelos, 46/59 cada; o de 12M é pior fora de escopo |
| "10 erros são sistemáticos" | ✓ sobrevivem a 7,5x de capacidade e à troca de semente |

O caso mais desconfortável é o primeiro. Um pipeline de geração por LLM foi
construído, medido, e declarado prejudicial. A semente 8 dá 81% — indistinguível dos
83% que o corpus escrito à mão dava. **A conclusão era ruído**, e ela foi afirmada
com números que pareciam precisos.

### Por que passou despercebido

Porque o número parecia grande. 83% contra 76% *soa* como sete pontos de diferença;
em 59 frases são **quatro frases**. A porcentagem esconde o tamanho da amostra, e é
exatamente esse o erro que ela induz.

A regra, agora: **diferença menor que ~5 frases neste benchmark não é interpretável.**
Para afirmar qualquer coisa abaixo disso, ou o benchmark cresce, ou a comparação roda
com três sementes e reporta a faixa.

### O que resta de sólido

Os 10 erros que sobrevivem a tudo:

```text
[executar_comando] chama o ipconfig no terminal
[executar_comando] manda um whoami ai
[executar_comando] queria disparar o systeminfo
[listar_pasta    ] o que esta arquivado em target
[procurar_arquivo] localiza pra mim algo batizado de esboco
[hora            ] me situa no tempo ai
[disco           ] quantos giga ainda cabem aqui dentro
[disco           ] nao consigo baixar nada, deve ser espaço
[perguntar       ] e ai, como voce ta hoje
[perguntar       ] cria uma planilha com os meus gastos
```

Três de dez são `executar_comando` com comando inédito — o corpus tem **4 comandos
distintos no total**. Os outros seguem o mesmo padrão: `arquivado`, `batizado`, "me
situa no tempo" são vocabulário que o corpus não contém.

Não é capacidade: 12M de parâmetros erram igual a 1,6M. É variedade de valor e de
vocabulário — a mesma lição que este documento já registrou cinco vezes, e a única
que sobreviveu a esta rodada de medição.

### Achado 23 — o modelo grande converge feio antes de convergir bem

Dito para o próximo que olhar o log do `padrao` na primeira época e tirar a conclusão
que eu tirei:

```text
pequeno  epoca  1: perda  2,185  intencao 73,9%  argumento 70,5%
padrao   epoca  1: perda 14,666  intencao 24,6%  argumento  4,4%
padrao   epoca 12: perda  2,644  intencao 90,6%  argumento 98,5%
```

Perda 6,7x maior e argumento em 4,4% pareciam taxa de aprendizado alta demais para
12M de parâmetros. **Não era.** Ele fecha em 90,6% de intenção com `lr = 1e-3`, o
mesmo do `pequeno`. Modelo maior só demora mais para sair do início.

O diagnóstico prematuro quase custou um retreino de uma hora e quarenta com `lr`
menor, que teria "confirmado" uma hipótese sobre um problema que não existia.

## 22. Fase 8 — agir de verdade: oficina, diário, coletor, ambiente, residente

As fases 1 a 7 treinaram uma agente que **escolhe** a ferramenta certa. Esta fase é
sobre o que acontece quando ela **executa** — e sobre descobrir que a régua das fases
anteriores não enxergava metade dos problemas.

**Oficina** (`src/tools/oficina.rs`) — um espaço de sombra. Copia a pasta, trabalha na
cópia, e só então `diff()` / `aplicar()` (com backup) / `descartar()`. Teto de 2.000
arquivos e 64 MB, porque copiar uma pasta sem limite é como se apaga um disco por
engano.

**Diário** (`src/tools/diario.rs`) — registro append-only com `fsync` **antes** de
agir, não depois. Se travar no meio, na volta existe a linha dizendo que ia agir. O
veredito é `Executar` / `Repetir(recibo)` / `Incerto`.

**Coletor** (`src/coletor/`) — busca corpus em português na internet, com allowlist
fixa **no código** (não em config: config é editável por quem não devia editar) e
comparação de host exata, o que barra `pt.wikipedia.org.evil.com` e
`pt.wikipedia.org@evil.com`. Cada trecho carrega `# fonte:`.

**Ambiente** (`src/ambiente/`) — uma pasta temporária onde ela pratica e o mundo
responde. A verificação olha o **efeito**, não a chamada.

**Residente** (`src/servidor.rs`) — TCP em `127.0.0.1:8931` **fixo no código, sem
flag**, e `executar: false` por padrão: ela observa e descreve, não age.

### Achado 24 — o diário não pode valer dentro da oficina

O diário existe para não repetir efeito irreversível. A oficina existe para tornar o
efeito reversível. Ligar os dois parecia rigor a mais e é um travamento:

```text
1. escreve na oficina  -> diário registra o recibo
2. descarta a oficina  -> o efeito some do mundo
3. tenta de novo       -> diário responde Repetir(recibo): "já fiz isso"
```

Ela recusa a refazer um trabalho que foi jogado fora. **Dentro da oficina o diário não
engata** — o que o justifica é a irreversibilidade, e ali não há nenhuma.

### Achado 25 — o corpus era literatura de 1898, e isso explicava um resultado antigo

Desde a fase 1 este documento registra "pré-treino ~ 0" sem explicação. A explicação
estava no arquivo: `corpus_pt.txt` é português literário de 1898, onde a palavra
**`arquivo` aparece zero vezes**. O tronco aprendia uma língua que a agente não fala.

Trocado por uma mistura (74% literatura + 26% técnico coletado):

```text
bits/byte em texto técnico:  5,52 -> 1,98   (-64%)
frases do benchmark:                 +0,3   (nada)
```

O modelo de linguagem melhorou muito. **A agente não.** É o resultado mais teimoso do
projeto: quatro tentativas de tronco, quatro vezes o mesmo. Ver Achado 29.

### Achado 26 — a API avisava, num campo que eu não lia

Três bugs no coletor, os três **silenciosos** — nenhum deu erro, todos deram menos
texto do que o log dizia:

```text
\uXXXX decodificado pela metade
lote sem exlimit                      -> 1 resultado onde eu contava 20
exlimit=max nao vale para artigo inteiro
```

O terceiro é o que ensina. A MediaWiki **dizia isso**, num campo `warnings` do JSON
que eu descartava por só ler `query`. Enquanto isso eu insistia em lotes que não
existiam e levei **HTTP 429 duas vezes** — de uma tempestade de requisições minha.

Ler o corpo inteiro da resposta, inclusive o que parece decoração, é mais barato do
que descobrir isso por rate limit.

### Achado 27 — o ambiente é um diagnóstico, não um treinador

**Como treinador, falhou.** A primeira versão tinha um sorteador próprio de frases com
26 aberturas. 1.200 tentativas depois:

```text
no ambiente:  72% -> 81%    (subiu)
no benchmark: 108,7 -> 95,0 (caiu 13,7 de 150; -18, -10, -13)
```

Sala de espelhos: o reforço empurrou a política para o que funciona **ali**. É o
Achado 4 do lado do treino em vez do teste. A causa foi manter uma segunda
distribuição de frases quando `learn::dados::gerar` já existia com 292 aberturas — o
sorteador paralelo foi removido, e o mundo fixo junto com ele.

**Como diagnóstico, achou o que a régua não via.** `escrever` resolvia caminho com
`checar_escrita` (contra a raiz); `ler` / `listar` / `procurar` usavam o caminho cru
(contra o CWD). Resultado: `ler_arquivo` fez **0 de 21** em dez rodadas — com a chamada
perfeitamente correta.

O benchmark **nunca** poderia pegar isso: ele confere a **chamada**, e a chamada
estava certa. Só executar de verdade e olhar o mundo depois revela um bug de caminho.

Corolário do mesmo dia: eu li "escrever_arquivo em 21%" desse ambiente e montei uma
hipótese sobre troca de campos. Medido direito, é **86%**, com **zero** troca. O
ambiente é bom para achar o que quebrou; ele é péssimo como placar.

### Achado 28 — `bytes/s` mede vazão, e vazão não é aprendizado

Escolhi `--batch 48` para uma sessão de quatro horas porque ele processava 36% mais
bytes por segundo que o padrão. **Métrica errada.** Vazão alta com poucos passos de
gradiente aprende menos, e o log do treino não conta isso — ele mostra a perda no
texto que o modelo acabou de ver.

Medido de novo, 1200s de relógio para cada candidato e `bits/byte` em
`dados/val_tec.txt`, que fica fora do treino:

| batch | passos | bytes/s | treino | **val_tec** |
|---|---|---|---|---|
| 8 | 1633 | 2787 | 2,2753 | 2,8343 |
| **24** | **679** | **3475** | **2,4714** | **2,5776** |
| 48 | 350 | 3578 | 2,8507 | 2,9480 |

Duas métricas erradas apontam dois números errados diferentes:

```text
bytes/s          aponta 48   (vazao)
perda de TREINO  aponta  8   (decoreba)
val_tec          aponta 24   (o que importa)
```

E o `8` — que eu tinha adotado depois de me corrigir do `48` — é o **pior dos três**
onde importa. A distância treino→validação denuncia: 0,559 no batch 8 contra 0,106 no
24 e 0,097 no 48.

A causa não é só "gradiente pequeno é ruidoso". Está no `Fluxo` (`src/learn/train.rs`):
ele reparte o corpus em `batch` faixas e lê uma por lane. Com batch 8 o modelo lê **8
pedaços** do corpus; com 24, **24 pedaços**. Batch menor é literalmente menos texto
distinto por unidade de tempo. O 48 tem a variedade e não tem passo de gradiente que
chegue no mesmo relógio — 24 é onde as duas coisas se cruzam.

Lição que vale além do batch: quando um número é fácil de coletar (`bytes/s` sai de
graça no log) e o número certo custa 20 minutos de validação, a tentação é usar o
fácil. Aqui isso teria comprometido **toda** sessão de treino seguinte.

### Achado 29 — os dois bracos do teste de tronco nao tinham o mesmo tamanho

Quatro vezes este documento registrou "o tronco pre-treinado nao transfere". As
quatro comparavam **1,6M do zero** contra **12M com tronco**:

```text
base    (bal_s*)     preset pequeno, sem tronco     1.613.743 params
grande  (grande_s*)  tronco lm_grande              12.020.815 params
```

Carregar o tronco tambem TROCA O TAMANHO do modelo. O resultado media duas coisas
somadas — pre-treino e 7,5x parametros — e nao dava para separar. Mais sementes nao
consertam isso: um confundimento fica igualzinho, so mais bem medido.

A celula que faltava e `--preset padrao` **sem** `--cerebro`: 12M do zero, os mesmos
12.020.815 parametros, mudando so a origem dos pesos. Seis sementes por braco:

```text
                        s7   s8   s9  s10  s11  s12   media   desvio
base 1,6M              109  107  110  110  109  109   109,0     1,1
CONTROLE 12M do zero   104  115  109  114  114  109   110,8     4,3
12M com tronco         107  111  116  122  116  119   115,2     5,4
```

Separando os dois efeitos:

```text
1,6M -> 12M, ambos do zero   +1,8 frase    nada
12M do zero -> com tronco    +4,3 frase    5 de 6 sementes positivas
```

Pareado por semente os deltas sao `+3, -4, +7, +8, +2, +10`: `t = 2,06`, `p ~ 0,09`.
Com 3 sementes o MESMO experimento deu `+2,0` e `t = 0,5`.

> **Este paragrafo dizia "e o sinal mais forte que o tronco ja produziu" e "duas ou
> tres sementes a mais decidem".** Decidiram, e decidiram contra: com 9 pares o efeito
> volta para `+2,22` e `t = 1,20`. Ver o **Achado 34**, que encerra a questao.

Licao dupla: (a) quando uma manipulacao muda duas variaveis, o controle vale mais que
amostra; (b) tres sementes num braco com desvio 5,4 nao mede coisa nenhuma.

**Correcao do Achado 22.** O chao de ruido nao e um numero do projeto, e um numero
**da configuracao**:

```text
preset pequeno (1,6M)   amplitude  3 frases   desvio 1,5
preset padrao  (12M)    amplitude 15 frases   desvio 5,4
```

As sementes 7-9 do braco com tronco dao media 111,3 e as 10-12 dao 119,0 — mesma
configuracao, 7,7 frases de diferenca so pela semente. Eu medi o ruido uma vez, no
modelo pequeno, e usei aquele numero para julgar comparacoes de 12M pelo resto do
projeto. Regua errada, tres vezes menor que a real.

### Achado 30 — treino e benchmark compartilhavam um ponto cego de 19 pontos

O John escreveu 500 frases de teste do jeito dele. A primeira coisa que elas
revelaram nao estava em lugar nenhum do benchmark que eu escrevi:

```text
                      maiuscula inicial   acento    "?"
treino (686 frases)          0%             0%      0%
benchmark meu (150)          0%             0%      0%
frases do John (500)       100%          60,4%    21,4%
```

Em 45 frases de `hora`/`memoria`/`disco` — sem argumento, teste puro de intencao:

```text
como a pessoa escreveu           34/45    76%
minuscula, sem acento, sem "?"   45/45   100%
```

Ela entendia as 45. Isolando o culpado:

```text
"Quanta RAM ta usando?"  ->  perguntar
"Quanta ram ta usando?"  ->  memoria
```

Tres bytes. A Teka e byte a byte: `R` e `r` sao bytes diferentes, vogal acentuada
sao dois bytes que o treino da agente nunca viu, `?` idem.

Nas 500 completas, cru contra normalizado:

```text
                   cru        normalizado
sem argumento    149/200      187/200      +38   74,5% -> 93,5%
com alvo          19/35        21/35        +2
sem alvo         245/265      186/265      -59   92,5% -> 70,2%
```

O terceiro grupo ensina a desconfiar de acerto. Eu previ que as frases sem objeto
("le esse arquivo", sem dizer qual) iriam mal, e elas fizeram 92,5% — mas maiuscula
**tambem** empurra para `perguntar`, e `perguntar` e a resposta certa daquele grupo.
Ela acertava pelo motivo errado; com o texto limpo a abstencao real e 70,2%.

**A correcao nao e normalizar a entrada.** Baixar tudo para minuscula quebraria o
argumento: `configuracao.ini` e `configuração.ini` sao arquivos diferentes para o
sistema de arquivos, e o ponteiro COPIA o trecho. A correcao e ensinar — o gerador
produz as duas superficies (`talvez_variar_superficie`), por tabela e nunca dentro de
um span de argumento. E a mesma ideia que o codigo ja defendia para erro de digitacao.

Duas armadilhas ao escrever isso, as duas pegas por teste:

```text
acento muda o TAMANHO em bytes   -> o span depois dele tem de andar junto
sortear da tabela nao funciona   -> "RAM" caiu em 0,05% dos exemplos, nao 15%
```

A segunda: eu sorteava uma sigla da tabela e tentava aplicar; como a sorteada quase
nunca estava na frase, o caso que motivou tudo isto quase nunca era treinado. Sortear
entre as palavras **presentes** conserta. Sem o teste
`a_variacao_de_superficie_acontece_de_fato`, a correcao teria sido publicada sem
corrigir nada.

**A licao que vale mais que a correcao:** enquanto eu escrever os dados de treino
**e** a regua, a regua herda os meus vicios e nao pode detectar meus pontos cegos por
construcao. Cada vez que o benchmark cresceu, a nota caiu (92% -> 83% -> 71%). Uma
regua escrita por outra pessoa achou em uma mensagem o que nenhuma quantidade de
sementes acharia.

### Achado 31 — a segunda colocada nao diz o que eu achei que dizia

`src/memory/contexto.rs` existe por um numero: das 500 frases escritas de fora,
**265 nao dizem qual e o objeto** — "le esse arquivo pra mim", "lista essa pasta".
Nao e descuido, e como gente fala: o objeto ficou no turno anterior. A Teka abstem,
e abstem certo — o ponteiro COPIA um trecho da entrada, e ali nao ha trecho.

**O desenho que eu escrevi inteiro antes de medir.** A ideia era ler a segunda
colocada da cabeca de intencao: ela estaria dizendo *"eu ia ler um arquivo, mas falta
o objeto"*. Escrevi o modulo, 9 testes, liguei no `main`. So entao rodei:

```text
"mostra esse arquivo de novo"  ->  perguntar p=0,989 | 2o memoria  p=0,009
"abre esse arquivo"            ->  perguntar p=1,000 | 2o hora     p=0,000
"le esse arquivo pra mim"      ->  perguntar p=1,000 | 2o memoria  p=0,000
```

A segunda colocada e **ruido**. E faz sentido depois de visto: no treino
"le esse arquivo" nunca aparece, entao ela aprendeu a mapear a superficie "verbo sem
nome de arquivo" direto para `perguntar` — nao como disputa entre duas ferramentas.
A cabeca de intencao nao representa intencao frustrada; ela classifica o que veio.

**O desenho que funciona.** Trocar o pronome pelo objeto lembrado e **perguntar de
novo**. A decisao continua sendo do modelo, sobre uma frase que agora tem objeto:

```text
le o notas.md                 -> ler_arquivo("notas.md")   guarda o arquivo
mostra esse arquivo de novo   -> abstem
  reescrito: "mostra notas.md de novo"
                              -> ler_arquivo("notas.md")   [pelo contexto]
```

Medido antes de escrever a segunda versao — a reescrita e resolvida certo mesmo
agramatical, porque o modelo e byte a byte e o ponteiro so precisa achar o trecho:

```text
"Mostra o que tem dados"       -> listar_pasta("dados")
"Da uma olhada dados pra mim"  -> listar_pasta("dados")
```

**Por que nao regra de verbo.** A alternativa obvia seria casar "le" com
`ler_arquivo` no Rust. Isso e reimplementar a cabeca de intencao a mao, e o Achado 19
registra o estrago de ensinar "verbo X = tal coisa". Anafora e classe fechada — meia
duzia de demonstrativos, que da para enumerar; vocabulario de acao nao e.

Tres guardas: so dispara quando ela **propria** abstem (nunca passa por cima de
pedido explicito); a lembranca vale 5 turnos; e o que tem efeito colateral
(`escrever_arquivo`, `executar_comando`) **nunca e adivinhado** — oferece e para.
Toda reescrita e anunciada com o valor usado, porque errar barato e as claras e
aceitavel e errar em silencio nao e.

**A licao de processo:** eu escrevi um modulo inteiro, com testes passando, em cima
de uma premissa que custou trinta segundos para refutar. Os 9 testes passavam porque
testavam o codigo que eu escrevi, nao a premissa dele. Teste verde nao valida
hipotese sobre o comportamento do modelo — so medicao no modelo valida.

### Achado 32 — um limiar que a producao nunca alcancou

Ao ligar o contexto, um teste de integracao caiu: `argumento` no benchmark exigia 80%
e deu 63,75%. A primeira leitura foi "regressao da variacao de superficie". Medido nos
seis modelos de verdade:

```text
sem variacao de superficie   63, 56, 58 de 80    media 59,0
com variacao de superficie   58, 61, 56 de 80    media 58,3
```

Diferenca de 0,7 com amplitude de 5 a 7 dentro de cada braco. Nao ha regressao.

O que havia: **o limiar de 80% nunca foi alcancado por nenhum modelo de producao** —
o melhor de seis foi 63/80, ou 78,75%. Ele passava porque aquele teste treina num
regime mais fraco e com outra distribuicao (6.000 exemplos gerados, sem os escritos a
mao), nao porque a barra fosse realista.

Duas correcoes diferentes, e a distincao importa:

```text
epocas 6 -> 12   mudar as CONDICOES da medicao      legitimo: o teste media no meio
                                                    da subida, e o Achado 23 explica
limiar 80% -> 60%  mudar o CRITERIO                 legitimo SO porque o numero de
                                                    producao esta documentado ao lado
```

Baixar limiar para a propria mudanca passar e o anti-padrao. O que torna isto outra
coisa e a evidencia independente: os seis modelos medidos em pares dizem que nao
houve regressao, e esse par de numeros ficou escrito no teste como referencia real.

### Achado 33 — a abstencao nao era fragil, era extrapolacao

O grupo "verbo certo, objeto ausente" variava 152 frases entre sementes. A leitura
obvia — "abstencao e a decisao mais delicada, porque e a unica sem evidencia positiva
no texto" — soa bem e estava errada. Medido, a abstencao **dentro** da distribuicao e
das coisas mais estaveis do modelo:

```text
                                    3 sementes        amplitude
abstencao DENTRO da distribuicao   20, 16, 20 de 28        4
  (meu benchmark, frases que eu escrevi no estilo do treino)
abstencao em "verbo sem objeto"    96, 248, 246 de 270   152
  (frases do John: "le esse arquivo pra mim")
```

Nao e a cabeca que e fragil: e que aquela **forma de frase nao existe no treino**. A
familia "vaga" que havia — "faz aquilo la", "da um jeito ai" — nao carrega verbo de
ferramenta. Extrapolacao depende de inicializacao; interpolacao nao.

**A correcao, e por que ela e derivada e nao escrita.** O Achado 19 diz que exemplo
negativo nao pode ter vocabulario proprio, senao o modelo aprende "este verbo = fora
de escopo". Escrever a familia a mao correria exatamente esse risco. Derivando da
frase-molde da propria ferramenta (`frase_sem_objeto`), o verbo aparece nos dois lados
**por construcao**:

```text
"traz o conteudo de {0}"       -> traz o conteudo desse arquivo    -> perguntar
"quero ver por dentro do {0}"  -> quero ver por dentro desse arq.  -> perguntar
"escancara o arquivo {0}"      -> descartada: "o arquivo esse arquivo"
```

O teste `o_verbo_e_compartilhado_entre_o_positivo_e_o_negativo` falha se algum verbo
ficar so de um lado. O que distingue positivo de negativo passa a ser a presenca de um
objeto concreto — que e a regra a aprender, e o que a cabeca de presenca representa.

**Resultado, 3 sementes, nas 500 frases de fora:**

```text
                              s7    s8    s9    media   amplitude
sem alvo   antes (bal)        96   248   246    196,7      152
           + superficie      220   185   157    187,3       63
           + sem objeto      241   245   239    241,7        6
TOTAL      antes (bal)       199   381   414    331,3      215
           + superficie      405   375   323    367,7       82
           + sem objeto      434   432   429    431,7        5
```

66,3% -> 86,3%, e de um sorteio para algo previsivel. As ferramentas seguraram
(`disco` 35,7 -> 49,0, `memoria` 20,3 -> 43,7): o Achado 19 **nao** disparou.

**O que custou, e o botao.** Ela ficou mais medrosa: recusa falsa foi de 12,3 para
19,7 de 150. Isso e ponto de operacao, nao defeito, e `--vies-abster` move sem
retreinar. Medido nas 3 sementes:

```text
vies    minha regua   recusa falsa   500 do John
 0,0       105,0          19,7          431,7
-1,0       106,0          18,0            —
-3,0       107,0          14,0          420,3
```

Trocar 11,3 abstencoes certas por 2 ferramentas certas e 5,7 recusas falsas a menos.
E trade de verdade, entre dois tipos de erro — e a assimetria decide: recusa falsa
custa uma reformulacao, acao falsa pode escrever arquivo ou rodar comando. O padrao
fica em `0,0` por isso, e nao porque o numero seja melhor.

### Achado 34 — o tronco nao transfere, e o `p` subindo foi quem avisou

Nove pares de sementes, mesmo tamanho nos dois bracos (12.020.815 parametros), unica
diferenca sendo a origem dos pesos:

```text
semente      7    8    9   10   11   12   13   14   15
com tronco 107  111  116  122  116  119  112  112  100
do zero    104  115  109  114  114  109  109  117  104
delta       +3   -4   +7   +8   +2  +10   +3   -5   -4

delta medio +2,22 frase de 150   desvio 5,56   t = 1,20   p ~ 0,26
6 positivos de 9      medias: com tronco 112,8, do zero 110,6
```

**Quatro horas de pre-treino, sobre um corpus 64% melhor em bits/byte, valem 2,2
frases de 150 — indistinguivel de zero.** Isto encerra uma pergunta que atravessou o
projeto inteiro.

**O sinal de alarme foi a trajetoria do `p`:**

```text
3 pares   +2,00   t = 0,50   p ~ 0,64
6 pares   +4,33   t = 2,06   p ~ 0,09
7 pares   +4,14   t = 2,34   p ~ 0,058
9 pares   +2,22   t = 1,20   p ~ 0,26
```

Efeito real tem `p` que **cai** com mais dados. Este subiu de 0,058 para 0,26 ao
receber dois pares. E o formato de um efeito que nao existe, visto de perto cedo
demais.

Eu chamei o resultado duas vezes antes da hora — com 6 pares ("o sinal mais forte que
o tronco ja produziu") e com 7 ("encostando em 0,05"). Os dois pares que faltavam
vieram `-5` e `-4`. Sem eles, este documento teria registrado o contrario.

**A regra que fica:** com efeito da ordem do desvio, o `p` de uma analise parcial nao
e estimativa do `p` final — e ruido com aparencia de tendencia. Ou se fixa o `n` antes
de olhar, ou nao se olha.

**O contraste que da o roteiro.** As duas mudancas de DADOS desta mesma sessao, meia
hora de treino cada, contra as quatro horas de tronco:

```text
                                     efeito                  custo
tronco de 4h (12M)      +2,2 de 150 = +1,5 ponto, t = 1,20    4h
variacao de superficie  +36,3 de 500                          30 min
familia "sem objeto"    +64,0 de 500                          30 min
  os dois juntos       +100,4 de 500 = +20 pontos
                       amplitude entre sementes 215 -> 5
```

Treze vezes mais efeito por um oitavo do tempo — e a segunda mudanca transformou um
sorteio (199 ou 414 de 500 conforme a semente) em algo previsivel. O gargalo da Teka
nunca foi quanto portugues o tronco sabe; e quantas FORMAS de pedido ela ja viu.

### Achado 35 — cada regua tem o SEU chao de ruido, e o da conversa e 8x o do benchmark

O Achado 29 ja corrigiu isto uma vez: o chao de ruido nao e um numero do projeto, e
um numero **da configuracao** (1,5 frase no 1,6M, 5,4 no 12M). Faltava a outra
metade: ele tambem e um numero **da regua**.

Tres sementes da mesma configuracao, medidas nas duas reguas:

```text
regua                        s7    s8    s9    media   desvio   amplitude
conversa (265 casos)         89   103   108   100,0      9,9       19
benchmark (150 frases)      109   109   107   108,3      1,2        2
```

**Oito vezes.** E o motivo esta na estrutura da medicao, nao no modelo: cada caso de
conversa encadeia intencao no turno 1, execucao, gravacao na gaveta, casamento de
anafora, reescrita, intencao no turno 2 e extracao de argumento. Sete etapas com
variancia, compostas. O benchmark tem uma.

**Como isso apareceu.** Uma mudanca no pool de pastas mediu `104 -> 89` na conversa e
parecia regressao clara. Com tres sementes, a configuracao nova deu `89, 103, 108` — e
o 104 do lado antigo cai dentro dessa faixa. Nao houve regressao nem melhora: houve
uma semente azarada.

O benchmark, medido nos mesmos tres modelos, deu `109, 109, 107`. Uma regua dizia
"caiu 15" e a outra dizia "nao mudou", e quem estava certa era a mais quieta.

**A regra:** diferenca menor que ~20 frases na regua de conversa nao diz nada com uma
semente. Comparar duas versoes ali exige tres sementes de cada lado — ou uma sonda
**deterministica** (`--pedido` numa frase escolhida), que nao tem semente nem
amostragem e por isso responde com uma execucao so.

Foi assim que o defeito real daquela mudanca ficou estabelecido enquanto o agregado
nao dizia nada:

```text
"Lista fotos"           perguntar         -> listar_pasta
"lista a pasta backup"  procurar_arquivo  -> listar_pasta
```

**A causa, medida:** o pool tinha 14 pastas e so QUATRO eram palavra simples (`dados`,
`src`, `anotacoes`, `musicas`), contra 12 arquivos todos com extensao. Com sinal tao
fraco, qualquer ruido empurrava para arquivo. E `fotos` errava ate sem pontuacao
porque o pool trazia `fotos\\2026` e nunca `fotos` sozinho — **nome composto nao
ensina o nome simples**.

Fica de pe a segunda causa, e ela nao cedeu a dados: pontuacao colada numa pasta
(`"Lista dados."`) continua virando arquivo, e o caso CHEGA ao treino em 5,7% dos
exemplos de pasta simples contra 7,8% dos de arquivo. Nao e ausencia de exemplo — e
que o ponto imita extensao, e extensao e o unico sinal que separa arquivo de pasta
numa palavra solta.

