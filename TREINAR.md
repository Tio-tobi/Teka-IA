# Treinar a Teka em outra máquina

Guia para rodar o treino num PC que não é o de origem. Não precisa de internet,
não precisa de GPU, não precisa instalar biblioteca nenhuma além do compilador.

## 1. O que a máquina precisa

**Só o Rust.** O projeto tem **zero dependências** — nada de `pip install`, nada de
CUDA, nada de baixar modelo.

- Instale em <https://rustup.rs> (Windows: baixa um `.exe` e clica)
- Feche e reabra o terminal depois

Confira:

```bash
cargo --version
```

## 2. Compilar

Dentro da pasta do projeto:

```bash
cargo build --release
```

Demora uns 2 minutos na primeira vez. O executável fica em
`target/release/teka.exe` (ou `teka` no Linux).

Confira que está inteiro:

```bash
cargo test --release
```

O que importa é a última linha dizer **`0 failed`** (são 215 testes hoje, mas o
número muda quando o projeto anda — o que não pode mudar é o zero).

Os testes levam ~15 min porque três deles treinam modelos de verdade. Se algum
falhar, **não treine**: o resultado não valeria nada.

## 3. Descobrir o `--batch` desta máquina

O padrão é **24**, medido no PC de origem. Se a sua máquina tiver bem mais (ou bem
menos) núcleos, vale remedir — mas leia o parágrafo seguinte antes, porque a forma
óbvia de medir dá a resposta errada.

**Não escolha pelo `bytes/s`.** Ele mede vazão, não aprendizado. Um batch grande
processa mais bytes e dá menos passos de gradiente, e termina sabendo menos. Também
não escolha pela coluna `bits/byte` do treino: ela mede o quanto o modelo decorou o
texto que já viu.

O que decide é `bits/byte` num texto que ficou **fora** do treino, com o mesmo tempo
de relógio para todos os candidatos. Foi assim no PC de origem:

| batch | passos | bytes/s | treino | **val_tec** |
|---|---|---|---|---|
| 8 | 1633 | 2787 | 2,2753 | 2,8343 |
| **24** | **679** | **3475** | **2,4714** | **2,5776** |
| 48 | 350 | 3578 | 2,8507 | 2,9480 |

O `8` ganha no treino e perde feio fora dele — distância de 0,559 contra 0,106 do 24.
Batch pequeno não é só gradiente mais ruidoso: o corpus é repartido em `batch` faixas,
então com 8 o modelo lê 8 pedaços do texto e com 24 lê 24. Menos batch, menos variedade.

Para remedir na sua máquina:

```bash
for b in 8 24 48; do
  ./target/release/teka.exe treinar --preset padrao \
    --corpus dados/corpus_misto.txt --minutos 20 --batch $b --saida mb_$b.bin
  ./target/release/teka.exe medir-lm --carregar mb_$b.bin --texto dados/val_tec.txt
done
```

São 20 minutos por candidato, e é tempo bem gasto: escolher errado aqui custa mais do
que isso em toda sessão de treino que vier depois.

Uma ressalva: acima de metade da RAM livre nada disso importa, porque o treino vai
para o arquivo de paginação e fica dez vezes mais lento. No PC de origem o batch 48
usava 2,7 GB e o 96 usava 5,1 GB.

## 4. Treinar o modelo de linguagem

É a parte longa e é a que mais se beneficia de máquina boa.

```bash
./target/release/teka.exe treinar \
  --corpus dados/corpus_misto.txt \
  --preset padrao \
  --minutos 240 \
  --batch 24 \
  --saida lm_novo.bin
```

- `--preset pequeno` (1,6M params) treina ~4x mais rápido; `padrao` (11,6M) é o alvo
- `--minutos` pode ser qualquer coisa; ele salva no fim
- Acompanhe a coluna `bits/byte`: **menor é melhor**, e ela cai devagar

Referência do que já foi obtido aqui: `5,52 → 1,98 bits/byte` em texto técnico.

## 5. Medir se ficou melhor

```bash
./target/release/teka.exe medir-lm --carregar lm_novo.bin --texto dados/val_tec.txt
```

`dados/val_tec.txt` ficou **fora** do treino de propósito. É a única comparação
honesta entre dois cérebros.

## 6. Treinar a agente

Rápido (~15 min). Usa o cérebro do passo 4 como tronco:

```bash
./target/release/teka.exe agente \
  --epocas 12 --exemplos 16000 \
  --cerebro lm_novo.bin \
  --saida teka_nova.bin \
  --benchmark
```

O `--benchmark` mede em 150 frases escritas à mão que **não estão** no treino.

### Rode com três sementes, não com uma

```bash
for s in 7 8 9; do
  ./target/release/teka.exe agente --epocas 12 --exemplos 16000 --semente $s \
    --cerebro lm_novo.bin --saida teka_s$s.bin --benchmark
done
```

**O chão de ruído deste benchmark é ~3 pontos** — trocar só a semente já mexe 2 a 3
frases de 150. Uma medição só não distingue melhora de acaso. Isso custou uma sessão
inteira de conclusões erradas aqui; não repita.

Antes de dizer que algo melhorou: **converta a porcentagem em número de frases.**
"73% contra 71%" soa como dois pontos; em 150 frases são três frases.

## 7. Mandar o resultado de volta

Só os `.bin` interessam (~6 MB cada). O formato guarda a configuração dentro do
arquivo e tem verificação de versão: um cérebro treinado com outro número de
ferramentas é **recusado ao carregar** em vez de produzir lixo em silêncio.

## Perguntas comuns

**Precisa de internet?** Não. Só para instalar o Rust.

**Estraga alguma coisa no PC?** O treino só lê `dados/` e escreve o `.bin` que você
pedir. As ferramentas que tocam o sistema (escrever arquivo, rodar comando) **não
são usadas no treino** — e mesmo fora dele o padrão é sandbox, onde elas apenas
descrevem o que fariam.

**Dá para usar a GPU?** Não. É CPU por decisão de projeto: `std` puro, sem CUDA, sem
nenhuma dependência.

**Quantos núcleos usa?** Todos, por padrão. `--threads N` limita — útil se a máquina
estiver sendo usada para outra coisa ao mesmo tempo.

**O treino pode ser interrompido?** Sim, `Ctrl-C`. Mas ele só salva no fim, então o
que foi feito até ali se perde. Prefira `--minutos` menores e mais rodadas.
