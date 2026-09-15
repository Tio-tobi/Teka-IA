# Teka — o que falta, o que está quebrado, e quanto custa consertar

Estado em **2026-09-15, 01h30**. Branch `tres_formas`, publicada em `origin`.

Este documento é escrito para alguém **de fora** do projeto. Todo número aqui foi medido;
onde não foi, está escrito que não foi. Se você encontrar uma afirmação sem método, é um
defeito deste documento.

---

## 1. O que é a Teka, em cinco linhas

Agente em **Rust puro, zero dependências, CPU-only**, que lê um pedido em português e
emite uma **chamada de ferramenta** em JSON, que depois é executada na máquina.

```
"lê o notas.md pra mim"   ->   {"acao":"ler_arquivo","caminho":"notas.md"}
```

1.616.059 parâmetros. Modelo hierárquico **byte a byte** (sem tokenizador), tronco RG-LRU,
quatro cabeças: intenção (qual ferramenta), ponteiro (qual trecho vira argumento), presença
(o argumento existe?) e crítico. Roda num Ryzen 5 5500 sem GPU.

---

## 2. A restrição que explica 80% dos defeitos

> **A cabeça de ponteiro COPIA UM TRECHO DO PEDIDO. Ela não gera texto.**

Isso não é limitação de treino, é a arquitetura. Consequências que aparecem o tempo todo:

| Pedido | Por que ela não consegue |
|---|---|
| `"fecha o discord"` → `executar_comando("taskkill /IM Discord.exe")` | a string `taskkill /IM` não está no pedido |
| `"joga esse arquivo lá dentro"` | `"lá dentro"` não é um caminho; não há de onde copiar |
| `"cria a pasta backup e joga o notas.md nela"` | duas ações, e a segunda depende do resultado da primeira |

**Consequência de projeto:** ferramentas como `fechar_programa(programa="discord")` não são
"outra ferramenta" — são **moldes onde o Rust escreve a parte fixa** (`taskkill /IM`) e ela
só preenche o buraco com a palavra que o usuário disse. É o único formato que ela produz.

**Verificado em 14/09**, semente 19, o teste que resolve a discussão:

```
SEM a palavra "taskkill" no pedido:
  "fecha o discord"             -> fechar_programa(discord)
  "encerra o processo Mir4G"    -> fechar_programa(Mir4G)          6 de 6

COM a palavra no pedido:
  "roda taskkill no spotify"    -> executar_comando("taskkill no spotify")   <- copia lixo
```

Ela nunca escreveu `taskkill` sozinha. Nenhuma vez.

---

## 3. Estado medido hoje

| medida | valor | como foi medido |
|---|---|---|
| ferramenta certa (régua de 339) | **69,7%** | 12 sementes, dois instrumentos independentes concordam |
| efeito acontece no disco | **77,5%** | 40 tarefas, 1 semente, `examples/sonda_efeito.rs` |
| chamada certa → efeito certo | **100%** | 0 casos de "chamada boa, nada aconteceu" |
| `executar_comando` ponta a ponta | **70,0%** | 20 frases × 6 sementes |
| comando escrito que de fato roda | **88,4%** | `where` de dentro do `cmd /C` |
| custo da voz (`faster-whisper base`) | **−11,5 pontos** | voz clonada → Whisper → Teka |
| custo da voz (`medium`) | **−8,3 pontos** | idem |
| carregar o modelo | ~163 ms | corrida de 2 min |
| por pedido | ~1,2 ms | ~400 pedidos/s sustentado |
| crescimento de RAM | **+966 B por pedido, sem teto** | 35 → 120 MB em 250 s |
| suíte | 364 testes | `cargo test --release --lib` |

**Nota de honestidade sobre o 77,5%:** uma semente só. Pode ser sorteio. A sonda tem um
controle — o gabarito de cada tarefa é executado antes, e 40/40 produzem o efeito — então
o instrumento está calibrado, mas a amostra é fina.

---

### 3.1 Resultado do braço `fechar_programa` (fechado 15/09, 01h13)

Criada a gêmea de `abrir_programa`: mesmo parâmetro, diferindo só no verbo. A hipótese
era de **registro**, não de capacidade — o verbo não carregava informação porque nada
dependia dele.

Falsificação registrada **antes** da execução, e o que aconteceu:

| condição que mataria a hipótese | resultado | veredito |
|---|---|---|
| `fechar` abaixo de 50% | **84%** (era 15%) | passou |
| controle `abrir` cair mais de 3 pontos | 90% → **86%** | **cruzou a linha** |
| primário cair mais de 2 pontos | **subiu 3,77** (t pareado +2,97) | passou |

Inversão `"fecha X"` → `abrir_programa`: **de 71% para 9%.**

O primário é as 329 frases comuns aos dois braços, repontuadas com os rótulos atuais
**nos dois lados** — repontuar só um lado produziria ganho fabricado. Nove sementes
subiram, três caíram.

**Sobre o gatilho que cruzou:** o controle caiu 3 sorteios em 96 (8 frases × 12
sementes), e nessa amostra 1 sorteio vale 1,04 ponto. O limiar de 3 pontos era apertado
demais — deveria ter sido definido em sorteios, não em pontos. Fica registrado como
cruzado porque foi o que estava escrito antes; afrouxar limiar depois do resultado é o
que este projeto proíbe. A leitura honesta é que o controle não se moveu de forma
distinguível de ruído.

### 3.2 Discrepância de instrumento, em aberto

Este documento citava **71,4%** como linha de base, número da sonda ao vivo sobre os
modelos do braço anterior. O log de treino dos **mesmos modelos** declara **67,4%**, e
o recálculo independente bate com o log.

Nos modelos novos os dois instrumentos concordam exatamente (69,7% e 69,7%), então não
há viés sistemático. Mas os modelos antigos **não carregam mais** (21 contra 22
ferramentas) e a discrepância não tem como ser resolvida.

**Use 69,7%.** Se você viu 71,4% em qualquer lugar, está velho.

### 3.3 Instrumento que mede o mundo que acabou

Padrão que apareceu **três vezes esta semana** e que vale conhecer antes de escrever
sonda:

- `examples/sonda_fechar.rs` imprimiu `fechar: 3% certo` depois do braço, porque o
  critério dela era `perguntar` — escrito quando não existia ferramenta de fechar. Os
  3% eram do instrumento.
- `examples/sonda_robustez.rs` continuou acusando um defeito **já consertado**, porque
  tinha a regra de decisão copiada do código de produção.
- A trava anti-binário-velho do script de experimento se derrotou a si mesma ao
  comparar `mtime` de um arquivo que ela mesma recopiava.

Quando você consertar algo, **confirme que a sonda mudou de resposta.** Se o número não
se moveu, a primeira suspeita é a sonda, não o conserto.

## 4. Defeitos conhecidos, por gravidade

### 4.1 GRAVE — `editar` não existe no registro (perda silenciosa de dado)

`Primitiva::Editar` está escrita em `src/tools/prim.rs`, tem despacho, tem checagem de
sandbox. **Nada a registra.** `Registro::padrao()` tem 22 ferramentas e nenhuma é ela.

Efeito: *"troca X por Y nesse arquivo"* cai em `escrever_arquivo` e **sobrescreve o arquivo
inteiro**. Não é erro de escolha, é destruição de dado sem aviso.

**Custo:** registrar + moldes + 1 braço de treino. Mas veja 4.2 antes — ela executa pela
ponte, que hoje ignora a política.

### 4.2 GRAVE — `buscar_no_conteudo` fura a trava de processos

`pela_ponte()` em `prim.rs:368` **não recebe a `Politica`** e chama `ponte_auto::garantir`,
que **sobe um processo**. Então `Politica::real_sem_processos` (`processos: false`) não
segura aquele caminho.

Essa trava existe porque em 04/09 um modelo **não treinado**, sorteando ferramenta dentro
do `cargo test`, abriu dezenas de janelas na área de trabalho.

`Editar` checa o sandbox antes da ponte; `Grep` não. **Custo: ~20 linhas, zero treino.**

### 4.3 MÉDIO — memória episódica sem teto

+966 bytes por pedido, linear, sem limite. 35 → 120 MB em 250 s (109 mil pedidos).
Irrelevante para uso normal; fatal para deixar ligada uma semana.

**Causa não confirmada** — a investigação morreu junto com o agente que a fazia. Suspeita:
`MemoriaEpisodica` sem política de descarte. **Custo: investigar + teto configurável, zero
treino.**

### 4.4 MÉDIO — `abrir_programa` rouba pedido de comando

O maior ladrão de `executar_comando`, e é **defeito antigo**, anterior à gêmea de hoje:

```
ANTES da gêmea    177/240 (73,8%)   -> abrir_programa 30, atalho 12
DEPOIS da gêmea   110/140 (78,6%)   -> abrir_programa 16, fechar_programa 4
```

Causa medida: contradição nos moldes. `"chama o {0} ai"` (abrir) contra `"chama o {0}"`
(executar) — a mesma superfície ensinando duas ferramentas. Doze primeiros-verbos são
compartilhados entre as ferramentas que agem.

**Conserto já escrito, esperando braço** (ver §6.1).

### 4.5 MÉDIO — `escrever_arquivo` recorta o argumento errado

4 das 5 falhas de efeito são dela:

```
"escreve lembrete de reuniao no arquivo aviso.txt"  -> texto:"lembrete"       (truncou)
"grava o texto comprar cafe em compras.txt"         -> caminho:"cafe em compras.txt"
```

Duas obrigatórias (`caminho` + `texto`) e a fronteira entre elas é o que ela erra.
Documentada como a ferramenta mais fraca do registro desde o começo.

### 4.6 BAIXO — `checar_escrita` é sensível a maiúscula

`Path::starts_with` compara caixa; no Windows caminho não é sensível a caixa. `C:\USERS\...`
é recusado sendo a mesma pasta. **Falha para o lado seguro** (recusa escrita válida).

### 4.7 BAIXO — `copiar_arquivo` e `mover_arquivo` sem frases na régua

As 10 que existiam foram rerrotuladas para `perguntar` (eram dêiticas: *"joga esse arquivo
lá"*, sem dizer qual). As duas ferramentas hoje têm **zero** frases de benchmark próprias.

### 4.8 BAIXO — limiar de 62% em `tests/agente.rs` está velho

Escrito para a régua de 329; a de hoje tem 339. **Baixar limiar para passar é proibido neste
projeto** — a decisão de mexer é do dono.

---

## 5. O que ainda NÃO foi medido

Três frentes de teste começaram e morreram em limite de cota antes de terminar. Os arquivos
compilam e rodam; falta a análise.

| frente | arquivo | o que responde |
|---|---|---|
| **sessão** | `examples/sonda_sessao.rs` | de N tarefas de vários passos, quantas chegam ao fim. 4 passos a 75% terminam em 32% — esse efeito composto ninguém mediu |
| **carga** | `examples/sonda_carga.rs` | causa do vazamento, degradação de vazão, latência × tamanho do pedido, escala com threads |
| **efeito nas 12 sementes** | `examples/sonda_efeito.rs` | o 77,5% com variância em vez de um ponto |

**Aviso sobre a de sessão:** o próprio autor dela escreveu *"several things need fixing
before I trust the numbers"* antes de morrer. **Não relate número dela sem revisar o código
primeiro.**

---

## 6. Melhorias propostas, com custo em treinos

Cada braço = **12 sementes × ~32 min ≈ 6,5 horas** numa máquina de 12 threads lógicos.
Sementes fixas 19–30, sempre pareadas contra o braço de controle.

### 6.1 Braço 1 — separar comando de programa  `[JÁ ESCRITO, ESPERANDO]`

Duas mudanças, já em `src/learn/dados.rs`:

- **A:** tirar de `abrir_programa` os dois moldes que colidem de frente com
  `executar_comando` (`"chama o {0} ai"`, `"dispara o {0}"`). 30 → 28 moldes.
- **B:** +16 moldes em `executar_comando` com marcador de lugar novo (cmd, shell, linha de
  comando, janela preta) e verbo de fazer sem marcador. 24 → 40 moldes.

**Falsificação, escrita antes:**
- `executar_comando` não subir acima de ~79% + ruído → a explicação de superfície está errada
- `abrir_programa` cair mais de 3 pontos → troquei um erro por outro
- primário cair mais de 2 pontos → não vale

**Ruído:** 20 frases × 12 sementes = 240 sorteios. Uma frase vale 5 pontos no subgrupo.
**Não leia movimento de 5 pontos como sinal.**

**Custo: 1 braço, 6,5 h.**

### 6.2 Braço 2 — `editar` entra no registro

Depende de 4.2 estar consertado (senão ela executa pela ponte sem política).
Registrar, escrever ~25 moldes e ~10 exemplos à mão, 10 frases novas de benchmark.

Risco conhecido e medido em outro contexto: **crescer o registro dilui tudo que não cresce
junto.** Com 10 ferramentas uma família saía em ~6% dos exemplos; com 19, caiu para 0,9%.
Entrar a 23ª ferramenta cobra de todas as outras.

**Custo: 1 braço, 6,5 h.**

### 6.3 Braço 3 — `escrever_arquivo`, a fronteira dos dois argumentos

O defeito 4.5. Hipótese a testar: a fronteira entre `caminho` e `texto` precisa de marcador
explícito nos moldes ("o texto X no arquivo Y" contra "no arquivo Y o texto X"), nas duas
ordens, senão ela aprende posição em vez de papel.

**Custo: 1 braço, 6,5 h.**

### 6.4 SEM TREINO — os consertos de código

| item | custo |
|---|---|
| 4.2 passar `Politica` para `pela_ponte` | ~20 linhas |
| 4.3 teto na memória episódica | investigar + ~40 linhas |
| 4.6 comparação de caminho insensível a caixa no Windows | ~10 linhas |
| terminar as três frentes de teste (§5) | análise, sem treino |

### 6.5 Entrada por voz — a maior relação ganho/custo do projeto

Já medido: trocar `faster-whisper base` por **`medium`** vale **3,2 pontos** e o modelo já
está no cache da máquina. **Zero linha de Teka.**

O resto: o ponto final custava 6,23 pontos e já foi consertado (`normalizar_pedido`). Falta
medir vírgula no meio da frase e palavra colada, que o transcritor também produz.

**Orçamento total sugerido: 3 braços de treino ≈ 20 h de máquina**, mais os consertos de
código que não precisam de treino.

---

## 7. As regras da casa (e por que elas existem)

Quem for mexer nisto precisa saber destas cinco. Todas nasceram de erro cometido.

**1. Escreva a falsificação ANTES de rodar.** No doc-comment da sonda, dizendo o que faria a
tese estar errada. Sem isso, qualquer resultado vira confirmação.

**2. NUNCA copie frase de `dados/frases_teste.txt` para os moldes.** Já aconteceu: as frases
que ela errava viraram templates e a medição seguinte perdeu o sentido. Para consertar um
erro visto na régua, escreva superfície **nova** que cubra o mesmo fenômeno. Existe um
verificador: `python checa_vazamento.py`.

**3. Meça a propriedade, nunca o proxy.** Este é o erro que mais custou tempo aqui. Em 14/09
uma sonda de segurança tinha a regra de decisão **copiada** do código de produção. A regra
foi consertada e **a sonda continuou imprimindo o resultado antigo**, porque media a cópia.
Se você precisa copiar lógica para testar, o conserto é **tornar a função testável**, não
escrever a cópia com mais cuidado.

**4. A suíte exige `--release`.** Em debug o teste de integração leva ~6 horas — foi por isso
que ficou vermelho por meses sem ninguém saber. Use `--no-fail-fast`.

**5. Baixar limiar para o teste passar é proibido.** Se o número caiu, o número caiu.

---

## 8. Armadilhas já pisadas (não pise de novo)

- **`where` do Git Bash ≠ `where` do `cmd`.** Produziu um "9 comandos rodariam" falso quando
  o número honesto era zero. A Teka roda `cmd /C`, que tem outro PATH.
- **`AdapterRAM` do WMI satura em 4 GB.** A GPU da máquina tem 8. Use o registro (`qwMemorySize`).
- **`Drop` de um `static` nunca roda no fim do processo em Rust.** A ponte precisa de
  `derrubar()` explícito no `main`.
- **`cargo test --lib` não reconstrói `teka.exe`.** Binário velho já custou 4 horas aqui.
- **A guarda contra binário velho custou outras 4 horas como falso positivo**, porque grepava
  uma palavra que aparecia em comentário de arquivo embutido. Confira a string da descrição
  da ferramenta no registro, não uma palavra qualquer.
- **Amaciar saída de erro esconde a informação útil.** Um amaciador trocou o "Acesso negado"
  do `taskkill` por "não entendi". Anote, não substitua.
- **Relatório de agente não é palavra final.** Das 5 "fugas de raiz" reportadas por uma sonda
  automática em 14/09, **zero** eram reais — todas resolviam para dentro da raiz, e uma o
  próprio Windows recusava.

---

## 9. Antes de publicar (`git push`)

Feito: a branch `tres_formas` foi publicada em `origin`. Clone e `git checkout tres_formas`.

Falta decidir: **merge em `main`**. A branch está ~34 commits à frente e `main` continua
no estado antigo (21 ferramentas, régua de 329). Quem clonar o `main` pega uma árvore
que não carrega nenhum modelo produzido nesta branch.

---

## 10. Se você tem só uma hora

Nesta ordem, por relação ganho/custo:

1. **Trocar o Whisper de `base` para `medium`** — 3,2 pontos, zero código.
2. **Consertar 4.2** (`pela_ponte` sem política) — é furo de segurança, ~20 linhas.
3. **Registrar `editar`** — hoje "troca X por Y" apaga o arquivo inteiro.
4. **Rodar o braço 1**, que já está escrito, e ler a falsificação antes do resultado.
