// Ponte da Teka: as ferramentas do Harness, servidas por JSON-RPC de linha.
//
// ## O que isto resolve
//
// O protocolo SDK do Harness tem cinco metodos e todos passam por um agente com
// LLM decidindo (`session/prompt`). Nao existe "execute esta ferramenta". Por ali,
// quem escolhe a ferramenta e o LLM deles — a cabeca de intencao da Teka fica de
// fora, e ela e justamente o que a gente passou semanas medindo.
//
// Mas `ctx.tools` e servico publico e o `ToolExecutionInput.agent` e OPCIONAL
// ("set by the agent loop"). Entao um plugin comum ve e executa, sem agente e sem
// LLM. Medido antes de escrever isto: 25 ferramentas visiveis, e `glob` executando.
//
// ## Por que SOCKET e nao stdio
//
// A doc do servidor JSON-RPC deles diz que "stdout is the protocol". Mas num perfil
// montado, o stdout ja pertence ao aplicativo — no `headless`, ele imprime a
// resposta da tarefa. Dividir aquele cano quebraria o parser dos dois lados.
//
// Socket em 127.0.0.1 nao disputa com ninguem. E do lado da Teka nao custa nada: o
// `Protocolo` dela e generico sobre `BufRead`/`Write`, entao fala TCP sem mudar
// uma linha.
//
// ## Seguranca
//
// Escuta SO em 127.0.0.1 — nunca em 0.0.0.0. E exige um segredo em
// `TEKA_PONTE_TOKEN`: sem ele o servidor **nao sobe**. Falha fechada, e nao aberta,
// porque um socket que executa `pwsh` sem autenticacao e um buraco e nao um
// atalho. Mesma postura do servidor da Teka, que tambem prende o laco em
// 127.0.0.1 e exige token por execucao.
//
// ## Metodos
//
//   tools/list   ->  { tools: [ {name, description, parameters}, ... ] }
//   tools/call   ->  { isError, content, meta, value }
//   ping         ->  { ok: true, tools: <quantas> }
//
// Os nomes sao os do MCP de proposito: se um dia a Teka falar com outro servidor
// de ferramenta, e o mesmo vocabulario.
//
// A LISTA E LIDA A CADA PEDIDO, e nao na partida. O Cordis monta as entradas em
// PARALELO, e `inject: ['tools']` espera o SERVICO existir — nao espera os outros
// plugins registrarem nada nele. Medido: 0 ferramentas no `apply`, 25 em 0,25s.
// Ler tarde resolve isso sem precisar adivinhar o nome de nenhum evento.
//
// Diagnostico vai para STDERR: stdout nao e meu.

import net from 'node:net'

export const name = 'teka-ponte'
export const inject = ['tools']

const PORTA_PADRAO = 8768 // 8767 e a ponte do Spotify
const LIMITE_LINHA = 1 << 20 // 1 MB: argumento de ferramenta nao passa disso

export function apply(ctx, config) {
  const porta = Number(config?.porta ?? PORTA_PADRAO)
  const segredo = process.env.TEKA_PONTE_TOKEN

  if (!segredo) {
    process.stderr.write(
      '[teka-ponte] TEKA_PONTE_TOKEN nao definido — o servidor NAO vai subir.\n' +
        '[teka-ponte] Isto e proposital: socket que executa ferramenta sem\n' +
        '[teka-ponte] autenticacao e buraco, nao atalho.\n',
    )
    return
  }

  const servidor = net.createServer((sock) => atender(ctx, sock, segredo))

  servidor.on('error', (e) => {
    process.stderr.write(`[teka-ponte] erro no servidor: ${e.message}\n`)
  })

  // SO LOOPBACK. Nunca 0.0.0.0.
  servidor.listen(porta, '127.0.0.1', () => {
    process.stderr.write(`[teka-ponte] escutando em 127.0.0.1:${porta}\n`)
  })

  // Cordis descarrega o plugin com a fibra; fechar o socket junto evita porta
  // presa depois de um recarregamento.
  ctx.on('dispose', () => servidor.close())
}

function atender(ctx, sock, segredo) {
  let autenticado = false
  let restante = ''

  sock.setEncoding('utf8')
  sock.on('error', () => sock.destroy())

  sock.on('data', (pedaco) => {
    restante += pedaco
    if (restante.length > LIMITE_LINHA) {
      // Linha sem fim e ou defeito ou ataque. Cortar e mais honesto que crescer
      // sem teto ate o processo morrer de memoria.
      sock.destroy()
      return
    }
    let quebra
    while ((quebra = restante.indexOf('\n')) >= 0) {
      const linha = restante.slice(0, quebra).trim()
      restante = restante.slice(quebra + 1)
      if (linha) {
        autenticado = tratar(ctx, sock, linha, segredo, autenticado)
      }
    }
  })
}

function responder(sock, id, resultado) {
  sock.write(JSON.stringify({ jsonrpc: '2.0', id, result: resultado }) + '\n')
}

function reclamar(sock, id, codigo, mensagem) {
  sock.write(JSON.stringify({ jsonrpc: '2.0', id, error: { code: codigo, message: mensagem } }) + '\n')
}

function tratar(ctx, sock, linha, segredo, autenticado) {
  let pedido
  try {
    pedido = JSON.parse(linha)
  } catch {
    reclamar(sock, null, -32700, 'nao e json')
    return autenticado
  }
  const id = pedido.id ?? null
  const metodo = String(pedido.method ?? '')
  const params = pedido.params ?? {}

  // O PRIMEIRO pedido tem de ser `hello` com o segredo. Qualquer outra coisa
  // antes disso fecha a conexao — sem dizer o que faltou, para nao virar oraculo
  // de adivinhacao.
  if (!autenticado) {
    if (metodo !== 'hello' || params.token !== segredo) {
      reclamar(sock, id, -32000, 'nao autenticado')
      sock.destroy()
      return false
    }
    responder(sock, id, { ok: true })
    return true
  }

  switch (metodo) {
    case 'ping':
      responder(sock, id, { ok: true, tools: contar(ctx) })
      return true

    case 'tools/list': {
      // Lido AGORA, e nao na partida: ver a nota do cabecalho sobre ordem.
      const tools = ctx.tools.schemas().map((s) => ({
        name: s.name,
        description: s.description,
        parameters: s.parameters,
      }))
      responder(sock, id, { tools })
      return true
    }

    case 'tools/call': {
      chamar(ctx, sock, id, params)
      return true
    }

    default:
      reclamar(sock, id, -32601, `metodo desconhecido: ${metodo}`)
      return true
  }
}

function contar(ctx) {
  try {
    return ctx.tools.schemas().length
  } catch {
    return -1
  }
}

async function chamar(ctx, sock, id, params) {
  const nome = String(params.name ?? '')
  if (!nome) {
    reclamar(sock, id, -32602, 'tools/call sem `name`')
    return
  }
  try {
    // AGENTE SINTETICO, e so quando quem chama pede.
    //
    // Por padrao `agent` fica OMITIDO — e o que permite executar sem LLM no
    // caminho, e e o motivo de a ponte existir. Mas nem toda ferramenta aceita
    // isso. Medido em 12/09, `read_image` do `dsh-tool-fs` faz:
    //
    //   const provider = routed?.provider ?? exec.agent?.options.provider
    //   if (provider === undefined || model === undefined) throw ...
    //
    // Sem agente, `provider` e `undefined` e ela recusa. Nao e falta de chave nem
    // de configuracao: a ferramenta LE do agente que chamou.
    //
    // Entao quem chama pode mandar `modelo: {provider, model}` e a ponte fabrica o
    // minimo que aquele codigo toca. `session.requestHeader()` devolvendo undefined
    // e de proposito: faz o `??` cair no `options`, que e o que a gente controla.
    const m = params.modelo
    const agente = m && m.provider && m.model
      ? { options: { provider: m.provider, model: m.model },
          session: { requestHeader: () => undefined } }
      : undefined
    const r = await ctx.tools.execute({
      callId: `teka-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      name: nome,
      arguments: params.arguments ?? {},
      // `signal` e OBRIGATORIO: sem ele `callerCancelled` le `.aborted` de
      // undefined e estoura antes de a ferramenta rodar.
      signal: new AbortController().signal,
      ...(agente ? { agent: agente } : {}),
    })
    responder(sock, id, r)
  } catch (e) {
    reclamar(sock, id, -32001, `${nome} falhou: ${e && e.message ? e.message : e}`)
  }
}
