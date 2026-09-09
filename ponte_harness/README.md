# A ponte: as ferramentas do Harness, com a Teka decidindo

Provado em 2026-09-09. O que está aqui é a cópia versionada do que roda dentro do
`dsh_home` — o original vive lá porque é o Harness que o carrega.

## O problema que isto resolve

O protocolo SDK do Harness tem **cinco métodos** e todos passam por um agente com
LLM decidindo (`session/prompt`). Não existe "execute esta ferramenta". Por aquela
porta, quem escolhe a ferramenta é o LLM deles — e a cabeça de intenção da Teka,
que é o que a gente passou semanas medindo, fica de fora.

Mas `ctx.tools` é serviço público, e o `ToolExecutionInput.agent` é **opcional**
("set by the agent loop"). Então um plugin comum vê e executa, sem agente e sem LLM.

## As peças

```
teka-ponte.mjs        o plugin: escuta em 127.0.0.1:8768 e serve
                      hello / ping / tools/list / tools/call
cordis.patch.yml      o que entra no perfil
perfil.package.json   o perfil `teka`: SÓ o bundle `dsh-base`, sem aplicativo
```

## Como subir

```bash
DSH_HOME=.../DeepSeek-Harness/dsh_home \
TEKA_PONTE_TOKEN=<segredo> \
  dsh --profile teka
```

E do lado da Teka:

```bash
TEKA_PONTE_TOKEN=<segredo> cargo run --release --example sonda_harness
```

## Por que um perfil próprio, e não `headless` nem `web`

Medido, nesta ordem:

```
headless   25 ferramentas — mas roda uma tarefa e SAI
web        fica de pé — mas 0 ferramentas: só registra quando nasce sessão
teka       só `dsh-base`: 25 ferramentas E fica de pé
```

No perfil `teka` não há tarefa nem sessão. O socket da própria ponte é o que segura
o processo vivo.

## Quatro coisas que custaram tentativa

**A lista é lida a cada pedido, não na partida.** O Cordis monta as entradas em
**paralelo**, e `inject: ['tools']` espera o *serviço* existir — não espera os
outros plugins registrarem nada nele. Medido: 0 ferramentas no `apply`, 25 um
quarto de segundo depois.

**`signal` é obrigatório** em `ctx.tools.execute`. Sem ele, `callerCancelled` lê
`.aborted` de `undefined` e estoura antes de a ferramenta rodar.

**`path` do `glob` usa a "session workspace"**, e aqui não há sessão. A primeira
chamada devolveu "No files found" num diretório que tinha os arquivos — e eu quase
li como sucesso. Foi o controle positivo que pegou.

**Caminho de plugin precisa de `file://` no Windows.** Caminho relativo resolve
contra o diretório do *perfil*, e absoluto cru é recusado pelo carregador ESM
("Received protocol 'c:'").

## Segurança

Escuta **só** em `127.0.0.1`, nunca em `0.0.0.0`. E **não sobe** sem
`TEKA_PONTE_TOKEN`: um socket que executa `pwsh` sem autenticação é um buraco, não
um atalho. O primeiro pedido tem de ser `hello` com o segredo; qualquer outra coisa
derruba a conexão.
