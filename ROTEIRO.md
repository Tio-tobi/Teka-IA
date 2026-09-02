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
benchmark de 150     114,58 ± 3,55  (12 sementes, FECHADO — 7 a 18)
argumento condicional  ~92%        (quando a ferramenta sai certa)
ferramentas             19         11 sem efeito, 7 com efeito local, 1 para fora
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

### 3.1 ~~Fechar o benchmark~~ FEITO em 2026-09-02

Doze sementes (7 a 18) — o maior `n` que este projeto já usou:

```
117, 116, 116, 117, 106, 114, 116, 116, 112, 110, 118, 117

media 114,58   sd 3,55   min 106   max 118
contra o braco de entropia (110,17):   +4,42   t = 1,79   p ~ 0,11
```

**Não passa da barra, nem com doze sementes.** O que os números sustentam é mais
modesto e ainda assim útil: o piso subiu (1 de 12 abaixo de 110, contra 2 de 6) e a
variância caiu de 5,49 para 3,55 — o mesmo código entregando modelos mais parecidos.

E a ressalva: a comparação empacota **quatro** mudanças — moldes indiretos, CONSULTAS
30→77, frases longas e janela 64→128. É "a semana" contra "antes", não teste de
variável isolada. Não dá para dizer qual delas rendeu.

**O elo que ainda não foi testado.** O corpus foi colhido apostando em
`mais texto → n-grama melhor → mais acurácia`. O primeiro elo está medido (ordem 6 de
21,4% para 7,5% de "não sei"). O **segundo nunca foi**, e a evidência anterior é
desfavorável (−0,17):

```bash
teka ngrama --texto dados/corpus_misto10.txt --ordem 6 --bits 22 --saida dados/ngrama.bin
```

Se der zero de novo, **encerrar o patch por entropia como linha de pesquisa**.

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

### 3.4 Interface: falta o canal de confirmação (dias)

A web nasce read-only porque `IsTerminal` é falso. Para ela **agir** pelo navegador
falta um canal de confirmação que não seja terminal — um "faz? [sim/não]" na página,
com o token e um identificador de pedido. É trabalho pequeno e é o que libera as oito
ferramentas restantes fora do terminal.

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
| Patch por entropia no agregado | −0,17, t=−0,06 | 6 sementes |
| Viés de abstenção varrido | não fecha a lacuna | 6 × 11 valores |
| Viés de fronteira no ponteiro fino | 94,3% → 88,4% | removido |
| Aprender da própria atividade (`ambiente`) | sem efeito | 3 × 1.200 |

**O padrão é inequívoco: arquitetura não moveu nada, dado moveu tudo.** Antes de
propor mudança estrutural, ter uma medição que aponte para ela — como a de restrição
múltipla em 3.3, que é a única que hoje tem.

---

## 5. Coisas pequenas que ficaram no caminho

- `sonda2.sh` e `tmp_*.rs` são resto de diagnóstico; podem sumir
- A memória semântica só é alimentada por `/buscar`; nada mais grava fato
- O grafo de conceitos existe e fica vazio — falta quem extraia conceito
- `--porta` não é o nome da flag de porta do servidor (descobri errando)
- Os 152 `.log` na raiz não são versionados, mas atrapalham; valeria uma pasta
