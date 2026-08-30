# Como usar a Teka

Tudo roda de dentro de `C:\Users\User\Projetos\Assistente\Teka-IA`.

---

## Uso normal

```bash
cargo run --release -- agente --carregar teka.bin
```

Abre o prompt. Você escreve pedidos em português; ela escolhe a ferramenta, recorta
os argumentos do seu próprio texto, e executa.

Uma pergunta só, sem abrir o prompt:

```bash
cargo run --release -- agente --carregar teka.bin --pedido "quanto de espaco tem no disco"
```

### Sandbox é o padrão

`escrever_arquivo` e `executar_comando` **não fazem nada** — descrevem o que fariam.
Para liberar, confinada a uma pasta:

```bash
cargo run --release -- agente --carregar teka.bin --real C:\teka\area
```

Mesmo em modo real, uma denylist recusa comandos destrutivos, e a raiz é checada com
`..` normalizado antes da comparação. Ver `src/tools/seguranca.rs`.

---

## Ensinando (é aqui que ela melhora de verdade)

| comando | quando |
|---|---|
| `/certo` | ela acertou |
| `/errado listar_pasta` | ela errou, e a certa era essa |
| `/consolidar` | fixa nos pesos o que você corrigiu |

```
teka> da uma varrida na pasta dados
  → {"acao":"ler_arquivo","caminho":"dados"}
teka> /errado listar_pasta
  corrigido para listar_pasta. Use /consolidar para fixar.
teka> /consolidar
  consolidado: 1 episodios | intencao 94.4% -> 94.1%
```

**Não precisa corrigir na hora.** A memória fica em `memoria.bin`, no diretório
atual, e sobrevive a reinício. Dá para acumular correções por dias e consolidar uma
vez só.

Umas **3 a 5 correções da mesma coisa** costumam bastar.

### O que a consolidação faz por baixo

Cada lote mistura episódios seus com replay do corpus base. É isso que impede o
esquecimento catastrófico — sem o replay, ela aprende as dez frases novas e perde as
mil antigas. Verificado desligando o mecanismo (§16 da arquitetura).

Episódios quase idênticos são colapsados antes (destilação), então repetir a mesma
correção dez vezes não faz o replay virar eco.

---

## Deixando ela experimentar

```
teka>  /explorar        →  o prompt vira teka*>
teka*> ...              →  ela AMOSTRA em vez de sempre dar a melhor
teka*> /reforcar        →  aprende do desfecho (o que executou, o que falhou)
```

`/aprender` faz consolidação **e** reforço, na ordem certa (correção explícita
primeiro — é o sinal mais forte).

Isso é opcional e de ganho mais discreto. **O que move a agulha é `/errado` +
`/consolidar`.** O reforço aproveita os episódios em que você não disse nada, e isso
é margem, não motor.

O prompt vira `teka*>` de propósito: nesse modo ela é deliberadamente
não-determinística, e isso precisa estar visível.

### Se aparecer o aviso do crítico

```
ATENCAO: o critico esta divergindo. A vantagem vira ruido —
vale reduzir a taxa antes de continuar.
```

Leve a sério. Um baseline que erra sistematicamente é **pior** que baseline nenhum:
injeta sinal errado com magnitude máxima em vez de apenas deixar de reduzir
variância. Pare de reforçar e recarregue o último agente salvo.

---

## Retreinar do zero

```bash
cargo run --release -- agente --epocas 12 --exemplos 16000 --saida teka.bin
```

~14 minutos. Chega a ~94% de intenção.

> **Copie `teka.bin` antes** — o treino salva por cima.

Treinar o modelo de linguagem (prever o próximo byte) é outra coisa, mais longa, e
hoje **não conectada ao agente**: testado, não ajudou a intenção nesta escala (§15).

```bash
cargo run --release -- treinar --minutos 30 --saida cerebro.bin
```

---

## Conferir se ela está boa

```bash
cargo run --release -- agente --carregar teka.bin --benchmark
```

24 frases escritas à mão que **nunca** entraram no gerador
(`dados/frases_teste.txt`). Hoje: **22/24** na ferramenta, **13/15** no argumento.

É o número honesto. Rode depois de treinar ou consolidar para ver se melhorou ou
piorou — a validação interna é otimista porque compartilha vocabulário com o treino.

> Nunca acrescente essas frases ao gerador. Um teste (`o_benchmark_nao_vazou_para_o_gerador`)
> quebra se isso acontecer, porque já aconteceu uma vez.

Validação completa, com a decomposição das métricas:

```bash
cargo run --release -- agente --carregar teka.bin --avaliar
```

---

## Ensinar uma ferramenta nova

Hoje isso é código, não configuração. Em `src/tools/mod.rs`, `Registro::padrao()`:

```rust
f("nome", "o que faz, em portugues",
  vec![Param::obrigatorio("caminho", Caminho)],
  Primitiva::AlgumaCoisa),
```

Depois acrescente frases em `src/learn/dados.rs` (`MOLDES`) e retreine. A gramática
se recompila sozinha a partir do registro.

Criar ferramentas por composição, sem código, é a fase 5.

---

## Arquivos

| | |
|---|---|
| `teka.bin` | o agente — é o que você carrega |
| `memoria.bin` | os episódios; apagar = ela esquece o que aprendeu com você |
| `dados/frases_teste.txt` | o benchmark honesto |
| `dados/corpus_pt.txt` | literatura em português, para o modelo de linguagem |
| `cerebro_pequeno.bin` | modelo de linguagem treinado (separado do agente) |

Arquivos `ag_*.bin` e `agente.bin` são de arquiteturas anteriores e **não carregam
mais** — a guarda de versão os recusa com mensagem explícita. Podem ser apagados.

---

## Quando algo der errado

| sintoma | causa provável |
|---|---|
| `arquivo tem N tensores, o agente espera M` | agente de uma arquitetura anterior; retreine |
| ela escolhe a ferramenta errada sempre | `/errado` + `/consolidar` algumas vezes |
| argumento sai cortado no meio da palavra | limitação conhecida do ponteiro (§15, achado 8) |
| `/consolidar` diz que não há nada | falta `/certo` ou `/errado` antes |
| tudo ficou pior depois de reforçar | veja o erro do crítico; recarregue o agente salvo |
