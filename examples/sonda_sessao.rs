//! De N tarefas de VARIOS PASSOS, quantas chegam ao fim?
//!
//! Tudo que o projeto mede hoje e frase ISOLADA: uma frase entra, uma chamada sai,
//! pontua. Ninguem usa assim. O John usa em SESSAO -- pede uma coisa, se refere ao
//! resultado dela, corrige, muda de assunto, pede de novo.
//!
//! E isso muda o numero de um jeito que media por frase nenhuma mostra: uma tarefa
//! de 4 passos com 75% por passo termina em 32% das vezes. O efeito e composto, e
//! ninguem mediu.
//!
//! ## O que cada familia responde
//!
//! - `explicito` CONTROLE. Todo passo nomeia tudo. E o teto: mede SO a composicao,
//!               sem dexis nenhuma. Sem isto eu nao consigo separar "morreu porque
//!               0,75^4 e pouco" de "morreu porque nao entende `la dentro`".
//! - `deixis`    O ALVO. "cria a pasta relatorios" -> "joga o resumo.md la dentro".
//! - `deixis_ok` A MESMA INTENCAO com o antecedente escrito na frase. O par
//!               `deixis`/`deixis_ok` e o que isola o custo da dexis de tudo o mais.
//! - `troca`     arquivo -> hora -> arquivo. O estado anterior atrapalha?
//! - `correcao`  "nao, era o outro arquivo", "nesse ai nao".
//! - `repete`    o mesmo pedido 3x seguidas. Determinismo.
//! - `composto`  duas acoes numa frase so. NAO PONTUADO -- so observado, porque nao
//!               existe uma resposta certa unica e fingir que existe seria inventar
//!               o resultado. O que vale aqui e a distribuicao e a PROVA no disco.
//!
//! ## O buraco que ja e conhecido, e que esta sonda MEDE em vez de esconder
//!
//! A cabeca de ponteiro COPIA UM TRECHO DO PEDIDO. Ela nao inventa texto que nao
//! esta na frase. Entao "la dentro" nao tem como virar `relatorios` -- e estrutura,
//! nao falta de treino. O resultado CERTO para um pedido assim e `perguntar`.
//!
//! Por isso a familia `deixis` tem `perguntar` como gabarito, e a pergunta real nao
//! e "ela acerta?" e sim **como ela erra**: `perguntar` (honesta e inutil) ou
//! ferramenta que AGE com um argumento copiado errado (pior que inutil).
//!
//! ## FALSIFICACAO, dita antes de rodar
//!
//! 1. Se `explicito` completar MUITO acima do produto dos acertos por passo, os
//!    erros sao correlacionados (sempre a mesma frase dificil) e a tese do "efeito
//!    composto" esta ERRADA -- a media por frase ja bastaria e esta sonda nao
//!    acrescenta nada.
//! 2. Se `deixis` morrer no passo 1, o problema nao e dexis: e a frase-base. A
//!    sonda nao provaria nada e o resultado tem de ser jogado fora.
//! 3. Se `deixis` completar 50% ou mais, a tese "nao da para conversar" morre.
//! 4. Se `deixis` e `deixis_ok` derem o mesmo numero, a dexis NAO custa nada e todo
//!    o desenho desta sonda estava errado.
//! 5. Cache: cada passo e respondido DUAS vezes, com o cache compartilhado da sessao
//!    e com um cache novinho. Se divergirem em UM passo que seja, `responder` vaza
//!    estado entre turnos -- e ai `sonda_fechar`, `sonda_voz` e todo o resto, que
//!    reusam um cache so, estao medindo com contaminacao. Espero 0. Qualquer numero
//!    acima de 0 e bug, e e a coisa mais importante deste arquivo.
//!
//! ## Duas reguas, e a diferenca entre elas e o ponto
//!
//! `ferramenta` e o que o benchmark de 329 frases pontua. `chamada+arg` exige
//! tambem o argumento. Para o John as duas nao sao a mesma coisa: `criar_pasta`
//! com o caminho errado cria a pasta errada, e o passo seguinte, que e certo,
//! erra por causa dela. Por isso a sessao e fechada pela regua estrita.
//!
//! ## SEGURANCA
//!
//! `Politica::real_sem_processos` numa pasta dentro de `temp_dir()`, apagada no fim.
//! Alem disso as seis ferramentas que alcancam a maquina do usuario NAO SAO
//! EXECUTADAS, nem com a politica neutralizada (ver `BLOQUEADAS`): a ESCOLHA delas
//! e registrada e pontuada, o efeito nao acontece. `atalho` esta na lista porque ela
//! e a unica que manda tecla de verdade mesmo com `processos: false` -- e o John tem
//! jogo aberto agora.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use teka::backend::Paralelo;
use teka::model::agente::{Agente, AgenteCache};
use teka::model::patcher::PorPalavra;
use teka::tools::{Chamada, Politica, Registro};

/// Escolher e pontuar: sim. Executar: nunca.
///
/// `abrir_programa`, `fechar_programa` e `executar_comando` ja seriam barradas por
/// `processos: false`, mas estao aqui de novo de proposito -- uma sonda nao deve
/// depender de uma trava so. `atalho` NAO e barrada por aquele campo: ela chama
/// `teclado::mandar_com` direto assim que o modo e `Real`. `buscar_web` sai para a
/// rede, e `buscar_no_conteudo` SOBE a ponte do harness, que e um processo.
const BLOQUEADAS: &[&str] = &[
    "abrir_programa",
    "fechar_programa",
    "executar_comando",
    "atalho",
    "buscar_web",
    "buscar_no_conteudo",
];

struct Passo {
    pedido: &'static str,
    /// Ferramentas aceitaveis. VAZIO = passo observado, nao pontuado.
    esperado: &'static [&'static str],
    /// (parametro, valor esperado). Vazio = nao confere argumento.
    args: &'static [(&'static str, &'static str)],
}

struct Sessao {
    nome: &'static str,
    familia: &'static str,
    passos: &'static [Passo],
    /// Caminho relativo que tem de existir na raiz no fim. Vazio = sem prova.
    ///
    /// Isto e a medida de USO de verdade: nao "emitiu a chamada certa", e sim "o
    /// arquivo esta la". O benchmark confere a chamada; o John confere a pasta.
    prova: &'static str,
}

const fn p(
    pedido: &'static str,
    esperado: &'static [&'static str],
    args: &'static [(&'static str, &'static str)],
) -> Passo {
    Passo { pedido, esperado, args }
}

static SESSOES: &[Sessao] = &[
    // ---- CONTROLE: todo passo nomeia tudo. So a composicao. ----
    Sessao {
        nome: "backup-explicito",
        familia: "explicito",
        prova: "backup/nota.txt",
        passos: &[
            p("cria a pasta backup", &["criar_pasta"], &[("caminho", "backup")]),
            p("escreve lembrete no arquivo backup/nota.txt", &["escrever_arquivo"], &[("caminho", "backup/nota.txt")]),
            p("lista a pasta backup", &["listar_pasta"], &[("caminho", "backup")]),
            p("mostra o conteudo do arquivo backup/nota.txt", &["ler_arquivo"], &[("caminho", "backup/nota.txt")]),
        ],
    },
    Sessao {
        nome: "copia-explicita",
        familia: "explicito",
        prova: "",
        passos: &[
            p("lista a pasta documentos", &["listar_pasta"], &[("caminho", "documentos")]),
            p("mostra o conteudo do notas.md", &["ler_arquivo"], &[("caminho", "notas.md")]),
            p("copia o notas.md para o arquivo copia.md", &["copiar_arquivo"], &[("origem", "notas.md"), ("destino", "copia.md")]),
            p("apaga o arquivo copia.md", &["apagar_arquivo"], &[("caminho", "copia.md")]),
        ],
    },
    Sessao {
        nome: "maquina-explicita",
        familia: "explicito",
        prova: "",
        passos: &[
            p("que horas sao", &["hora"], &[]),
            p("quanta memoria esta em uso", &["memoria"], &[]),
            p("quanto espaco livre tem no disco", &["disco"], &[]),
        ],
    },
    // ---- O ALVO: o antecedente esta no turno ANTERIOR. ----
    //
    // Gabarito `perguntar` nos passos 2 e 3: a cabeca de ponteiro copia trecho do
    // pedido, entao nao existe recorte de "la dentro" que de `relatorios`.
    Sessao {
        nome: "relatorios-la-dentro",
        familia: "deixis",
        prova: "relatorios",
        passos: &[
            p("cria uma pasta chamada relatorios", &["criar_pasta"], &[("caminho", "relatorios")]),
            p("agora joga o resumo.md la dentro", &["perguntar"], &[]),
            p("e me mostra o que tem nela", &["perguntar"], &[]),
        ],
    },
    Sessao {
        nome: "backup-pra-ela",
        familia: "deixis",
        prova: "backup",
        passos: &[
            p("cria a pasta backup", &["criar_pasta"], &[("caminho", "backup")]),
            p("copia o notas.md pra ela", &["perguntar"], &[]),
            p("lista ela", &["perguntar"], &[]),
        ],
    },
    Sessao {
        nome: "abre-o-primeiro",
        familia: "deixis",
        prova: "",
        passos: &[
            p("lista a pasta documentos", &["listar_pasta"], &[("caminho", "documentos")]),
            p("abre o primeiro arquivo", &["perguntar"], &[]),
            p("apaga esse ai", &["perguntar"], &[]),
        ],
    },
    Sessao {
        nome: "esse-mesmo",
        familia: "deixis",
        prova: "",
        passos: &[
            p("mostra o conteudo do notas.md", &["ler_arquivo"], &[("caminho", "notas.md")]),
            p("qual o tamanho dele", &["perguntar"], &[]),
            p("copia esse mesmo pra pasta documentos", &["perguntar"], &[]),
            p("agora apaga o original", &["perguntar"], &[]),
        ],
    },
    // ---- A MESMA INTENCAO, antecedente escrito. O par que isola o custo. ----
    Sessao {
        nome: "relatorios-nomeado",
        familia: "deixis_ok",
        prova: "relatorios/resumo.md",
        passos: &[
            p("cria uma pasta chamada relatorios", &["criar_pasta"], &[("caminho", "relatorios")]),
            p("move o resumo.md para a pasta relatorios", &["mover_arquivo"], &[("origem", "resumo.md"), ("destino", "relatorios")]),
            p("mostra o que tem na pasta relatorios", &["listar_pasta"], &[("caminho", "relatorios")]),
        ],
    },
    Sessao {
        nome: "backup-nomeado",
        familia: "deixis_ok",
        prova: "backup",
        passos: &[
            p("cria a pasta backup", &["criar_pasta"], &[("caminho", "backup")]),
            p("copia o notas.md para a pasta backup", &["copiar_arquivo"], &[("origem", "notas.md"), ("destino", "backup")]),
            p("lista a pasta backup", &["listar_pasta"], &[("caminho", "backup")]),
        ],
    },
    Sessao {
        nome: "notas-nomeado",
        familia: "deixis_ok",
        prova: "",
        passos: &[
            p("mostra o conteudo do notas.md", &["ler_arquivo"], &[("caminho", "notas.md")]),
            p("qual o tamanho do notas.md", &["info_arquivo"], &[("caminho", "notas.md")]),
            p("copia o notas.md para a pasta documentos", &["copiar_arquivo"], &[("origem", "notas.md"), ("destino", "documentos")]),
            p("apaga o arquivo rascunho.txt", &["apagar_arquivo"], &[("caminho", "rascunho.txt")]),
        ],
    },
    // ---- TROCA DE ASSUNTO: o estado anterior atrapalha? ----
    Sessao {
        nome: "arquivo-hora-arquivo",
        familia: "troca",
        prova: "",
        passos: &[
            p("lista a pasta documentos", &["listar_pasta"], &[("caminho", "documentos")]),
            p("que horas sao", &["hora"], &[]),
            p("mostra o conteudo do notas.md", &["ler_arquivo"], &[("caminho", "notas.md")]),
        ],
    },
    Sessao {
        nome: "pasta-maquina-pasta",
        familia: "troca",
        prova: "backup",
        passos: &[
            p("cria a pasta backup", &["criar_pasta"], &[("caminho", "backup")]),
            p("quanta memoria esta em uso", &["memoria"], &[]),
            p("quanto espaco livre tem no disco", &["disco"], &[]),
            p("lista a pasta backup", &["listar_pasta"], &[("caminho", "backup")]),
            p("que horas sao", &["hora"], &[]),
        ],
    },
    // ---- CORRECAO. ----
    //
    // O passo 1 de `corrige-apagar` APAGA DE VERDADE (dentro da raiz temporaria).
    // Isso e o ponto, nao um descuido: a correcao do passo 2 chega DEPOIS do efeito.
    // Nao existe desfazer, e nenhuma medida por frase isolada mostra isso.
    Sessao {
        nome: "corrige-ler",
        familia: "correcao",
        prova: "",
        passos: &[
            p("mostra o conteudo do notas.md", &["ler_arquivo"], &[("caminho", "notas.md")]),
            p("nao, era o outro arquivo", &["perguntar"], &[]),
            p("mostra o conteudo do lista.txt entao", &["ler_arquivo"], &[("caminho", "lista.txt")]),
        ],
    },
    Sessao {
        nome: "corrige-apagar",
        familia: "correcao",
        prova: "",
        passos: &[
            p("apaga o arquivo rascunho.txt", &["apagar_arquivo"], &[("caminho", "rascunho.txt")]),
            p("nesse ai nao", &["perguntar"], &[]),
            p("apaga o arquivo temp.txt", &["apagar_arquivo"], &[("caminho", "temp.txt")]),
        ],
    },
    // ---- REPETICAO: o mesmo pedido 3x seguidas. ----
    Sessao {
        nome: "repete-listar",
        familia: "repete",
        prova: "",
        passos: &[
            p("lista a pasta documentos", &["listar_pasta"], &[("caminho", "documentos")]),
            p("lista a pasta documentos", &["listar_pasta"], &[("caminho", "documentos")]),
            p("lista a pasta documentos", &["listar_pasta"], &[("caminho", "documentos")]),
        ],
    },
    Sessao {
        nome: "repete-escrever",
        familia: "repete",
        prova: "nota.txt",
        passos: &[
            p("escreve lembrete no arquivo nota.txt", &["escrever_arquivo"], &[("caminho", "nota.txt")]),
            p("escreve lembrete no arquivo nota.txt", &["escrever_arquivo"], &[("caminho", "nota.txt")]),
            p("escreve lembrete no arquivo nota.txt", &["escrever_arquivo"], &[("caminho", "nota.txt")]),
        ],
    },
    // ---- COMPOSTO: duas acoes numa frase so. Passo 1 nao pontuado. ----
    Sessao {
        nome: "composto-backup",
        familia: "composto",
        prova: "backup/notas.md",
        passos: &[
            p("cria a pasta backup e joga o notas.md dentro", &[], &[]),
            p("lista a pasta backup", &["listar_pasta"], &[("caminho", "backup")]),
            p("mostra o conteudo do arquivo backup/notas.md", &["ler_arquivo"], &[("caminho", "backup/notas.md")]),
        ],
    },
    Sessao {
        nome: "composto-saida",
        familia: "composto",
        prova: "saida/log.txt",
        passos: &[
            p("cria a pasta saida e escreve pronto no arquivo saida/log.txt", &[], &[]),
            p("lista a pasta saida", &["listar_pasta"], &[("caminho", "saida")]),
            p("que horas sao", &["hora"], &[]),
        ],
    },
];

/// Compara argumento com tolerancia ao que nao muda o efeito: caixa, aspas, barra
/// invertida do Windows, e o `./` que as vezes vem colado na frente.
fn normal(s: &str) -> String {
    let t = s.trim().trim_matches('"').to_lowercase().replace('\\', "/");
    t.trim_start_matches("./").trim_end_matches('/').to_string()
}

/// `(ferramenta_ok, chamada_ok)`. `None` quando o passo nao e pontuado.
fn julgar(passo: &Passo, ch: &Chamada, reg: &Registro) -> (Option<bool>, Option<bool>) {
    if passo.esperado.is_empty() {
        return (None, None);
    }
    let nome = reg.ferramentas[ch.ferramenta].nome.as_str();
    if !passo.esperado.iter().any(|e| *e == nome) {
        return (Some(false), Some(false));
    }
    let args_ok = passo.args.iter().all(|(k, v)| {
        ch.args
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, got)| normal(got) == normal(v))
            .unwrap_or(false)
    });
    (Some(true), Some(args_ok))
}

/// Os arquivos que a sessao encontra ja no lugar. Fixos para todas de proposito:
/// se cada sessao tivesse o seu, uma falha de execucao poderia ser do cenario.
fn preparar(raiz: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(raiz.join("documentos"))?;
    std::fs::write(raiz.join("notas.md"), "anotacoes da semana\nsegunda: comecar\n")?;
    std::fs::write(raiz.join("resumo.md"), "resumo curto\n")?;
    std::fs::write(raiz.join("lista.txt"), "leite\npao\n")?;
    std::fs::write(raiz.join("rascunho.txt"), "rascunho\n")?;
    std::fs::write(raiz.join("temp.txt"), "temporario\n")?;
    std::fs::write(raiz.join("documentos").join("ata.txt"), "ata da reuniao\n")?;
    Ok(())
}

fn main() {
    let molde = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "modelos/teka_fechar_s%.bin".into());
    let sementes: Vec<u32> = (19..=30).collect();

    // Um treino roda agora com 10 threads. Nao brigo por CPU com ele.
    let ops = Paralelo::new(2);
    let patcher = PorPalavra::default();

    let base = std::env::temp_dir().join("teka_sonda_sessao");
    // A trava: se por qualquer motivo a raiz nao cair no temp, nao roda nada.
    assert!(
        base.starts_with(std::env::temp_dir()),
        "raiz fora do temp -- abortado por seguranca"
    );
    let _ = std::fs::remove_dir_all(&base);

    // familia -> (ferramenta_ok, chamada_ok, n)
    let mut por_familia: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    // familia -> (completas por ferramenta, por chamada, provas no disco, n)
    let mut sess: BTreeMap<&str, (usize, usize, usize, usize)> = BTreeMap::new();
    // posicao do passo (1..) -> (chamada_ok, n)
    let mut por_posicao: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    // onde morreu (1..) -> quantas sessoes; 0 = chegou ao fim
    let mut morte: BTreeMap<usize, usize> = BTreeMap::new();
    // pedido nao pontuado -> chamada emitida -> quantas vezes
    let mut composto: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    // pedido -> (vezes que errou a chamada, exemplo do que saiu)
    let mut erros: BTreeMap<&str, (usize, String)> = BTreeMap::new();
    // divergencias entre cache compartilhado e cache novo
    let mut divergiu: Vec<String> = Vec::new();
    // passos com a chamada CERTA que mesmo assim falharam ao executar
    let mut exec_falhou: BTreeMap<String, String> = BTreeMap::new();
    // sessoes de `repete` em que as 3 repeticoes nao deram a mesma coisa
    let mut repete_instavel: Vec<String> = Vec::new();

    let mut n_modelos = 0usize;
    let mut detalhe: Vec<String> = Vec::new();
    let mut carregados: Vec<String> = Vec::new();

    for s in &sementes {
        let caminho = molde.replace('%', &s.to_string());
        let Ok(ag) = Agente::<f32>::carregar(Path::new(&caminho), Registro::padrao()) else {
            continue;
        };
        n_modelos += 1;
        carregados.push(caminho.clone());
        let primeiro = n_modelos == 1;

        for (is, sessao) in SESSOES.iter().enumerate() {
            let raiz: PathBuf = base
                .join(format!("s{s}"))
                .join(format!("{is:02}_{}", sessao.nome));
            if preparar(&raiz).is_err() {
                continue;
            }
            let pol = Politica::real_sem_processos(&raiz);

            // O cache COMPARTILHADO e o uso normal: `sonda_fechar` e `sonda_voz`
            // reusam um so para todas as frases. E ele que a sessao usa.
            let mut cache = AgenteCache::new();
            let mut morreu_ferr = 0usize;
            let mut morreu_cham = 0usize;
            let mut pos = 0usize;
            let mut saidas_repete: Vec<String> = Vec::new();

            if primeiro {
                detalhe.push(format!("  [{}] {}", sessao.familia, sessao.nome));
            }

            for passo in sessao.passos {
                pos += 1;
                let r = ag.responder(&ops, &patcher, passo.pedido, &mut cache);
                // O mesmo pedido, com cache virgem. Se divergir, `responder` vaza.
                let mut limpo = AgenteCache::new();
                let r2 = ag.responder(&ops, &patcher, passo.pedido, &mut limpo);
                let txt = |x: &Result<Chamada, String>| match x {
                    Ok(c) => c.texto(&ag.registro),
                    Err(e) => format!("<erro: {e}>"),
                };
                let (t1, t2) = (txt(&r), txt(&r2));
                if t1 != t2 {
                    divergiu.push(format!("s{s} {} p{pos}: {t1}  !=  {t2}", sessao.nome));
                }
                saidas_repete.push(t1.clone());

                let (ferr_ok, cham_ok) = match &r {
                    Ok(c) => julgar(passo, c, &ag.registro),
                    Err(_) if passo.esperado.is_empty() => (None, None),
                    Err(_) => (Some(false), Some(false)),
                };

                if passo.esperado.is_empty() {
                    *composto
                        .entry(passo.pedido)
                        .or_default()
                        .entry(t1.clone())
                        .or_default() += 1;
                } else {
                    let e = por_familia.entry(sessao.familia).or_default();
                    e.2 += 1;
                    if ferr_ok == Some(true) {
                        e.0 += 1;
                    }
                    if cham_ok == Some(true) {
                        e.1 += 1;
                    }
                    let q = por_posicao.entry(pos).or_default();
                    q.1 += 1;
                    if cham_ok == Some(true) {
                        q.0 += 1;
                    } else {
                        let x = erros.entry(passo.pedido).or_default();
                        x.0 += 1;
                        x.1 = t1.clone();
                        if morreu_cham == 0 {
                            morreu_cham = pos;
                        }
                    }
                    if ferr_ok != Some(true) && morreu_ferr == 0 {
                        morreu_ferr = pos;
                    }
                }

                // ---- execucao: so o que nao alcanca a maquina do usuario ----
                let mut exec = "-".to_string();
                if let Ok(c) = &r {
                    let nome = ag.registro.ferramentas[c.ferramenta].nome.clone();
                    if BLOQUEADAS.contains(&nome.as_str()) {
                        exec = "BLOQUEADA pela sonda".into();
                    } else {
                        match ag.registro.ferramentas[c.ferramenta]
                            .prim
                            .executar(&c.args, &pol)
                        {
                            Ok(o) => {
                                exec = o.lines().next().unwrap_or("").chars().take(46).collect();
                                if exec.is_empty() {
                                    exec = "(vazio)".into();
                                }
                            }
                            Err(e) => {
                                exec =
                                    format!("ERRO {}", e.chars().take(40).collect::<String>());
                                if cham_ok == Some(true) {
                                    exec_falhou.insert(passo.pedido.into(), e.clone());
                                }
                            }
                        }
                    }
                }

                if primeiro {
                    let marca = match cham_ok {
                        Some(true) => "ok ",
                        Some(false) if ferr_ok == Some(true) => "ARG",
                        Some(false) => "X  ",
                        None => "?  ",
                    };
                    detalhe.push(format!(
                        "    {marca} {pos}. {:<50} {:<42} {exec}",
                        passo.pedido, t1
                    ));
                }
            }

            if sessao.familia == "repete" && saidas_repete.windows(2).any(|w| w[0] != w[1]) {
                repete_instavel.push(format!("s{s} {}: {saidas_repete:?}", sessao.nome));
            }

            let e = sess.entry(sessao.familia).or_default();
            e.3 += 1;
            if morreu_ferr == 0 {
                e.0 += 1;
            }
            if morreu_cham == 0 {
                e.1 += 1;
            }
            // O denominador desta coluna e so o das sessoes que declaram prova.
            if !sessao.prova.is_empty() && raiz.join(sessao.prova).exists() {
                e.2 += 1;
            }
            *morte.entry(morreu_cham).or_default() += 1;
        }
    }

    if n_modelos == 0 {
        eprintln!("nenhum modelo casou com {molde}");
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    let n_passos_pont: usize = SESSOES
        .iter()
        .map(|s| s.passos.iter().filter(|p| !p.esperado.is_empty()).count())
        .sum();
    println!(
        "\n  {n_modelos} modelo(s), {} sessoes, {n_passos_pont} passos pontuados por modelo",
        SESSOES.len()
    );
    println!("  modelos: {}", carregados.join(", "));
    println!("  raiz de execucao: {}", base.display());
    println!(
        "  escolhidas mas NAO executadas: {}\n",
        BLOQUEADAS.join(", ")
    );

    println!("  === ACERTO POR PASSO ===");
    println!("  {:<12} {:>16} {:>16}", "familia", "ferramenta", "chamada+arg");
    for (f, (ferr, cham, n)) in &por_familia {
        println!(
            "  {f:<12} {ferr:>8}/{n:<3} {:>3.0}% {cham:>8}/{n:<3} {:>3.0}%",
            100.0 * *ferr as f64 / *n as f64,
            100.0 * *cham as f64 / *n as f64
        );
    }
    let (tf, tc, tn) = por_familia
        .values()
        .fold((0usize, 0usize, 0usize), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
    println!(
        "  {:<12} {tf:>8}/{tn:<3} {:>3.0}% {tc:>8}/{tn:<3} {:>3.0}%",
        "TOTAL",
        100.0 * tf as f64 / tn as f64,
        100.0 * tc as f64 / tn as f64
    );

    println!("\n  === SESSOES COMPLETAS (todos os passos pontuados certos) ===");
    println!(
        "  {:<12} {:>15} {:>15} {:>16}",
        "familia", "por ferram.", "por chamada", "prova no disco"
    );
    for (f, (cf, cc, pr, n)) in &sess {
        let com_prova = SESSOES
            .iter()
            .filter(|s| s.familia == *f && !s.prova.is_empty())
            .count()
            * n_modelos;
        let sp = if com_prova == 0 {
            "          --".to_string()
        } else {
            format!(
                "{pr:>4}/{com_prova:<3} {:>3.0}%",
                100.0 * *pr as f64 / com_prova as f64
            )
        };
        println!(
            "  {f:<12} {cf:>6}/{n:<3} {:>3.0}% {cc:>6}/{n:<3} {:>3.0}%  {sp}",
            100.0 * *cf as f64 / *n as f64,
            100.0 * *cc as f64 / *n as f64
        );
    }
    let (sf, sc, _, sn) = sess.values().fold((0usize, 0usize, 0usize, 0usize), |a, b| {
        (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3)
    });
    println!(
        "  {:<12} {sf:>6}/{sn:<3} {:>3.0}% {sc:>6}/{sn:<3} {:>3.0}%",
        "TOTAL",
        100.0 * sf as f64 / sn as f64,
        100.0 * sc as f64 / sn as f64
    );

    println!("\n  === EM QUAL PASSO MORREM (regua chamada+arg) ===");
    let total_sess: usize = morte.values().sum();
    for (pos, c) in &morte {
        let rot = if *pos == 0 {
            "chegou ao fim".to_string()
        } else {
            format!("morreu no passo {pos}")
        };
        println!(
            "  {rot:<22} {c:>4}/{total_sess}  {:>4.0}%",
            100.0 * *c as f64 / total_sess as f64
        );
    }

    println!("\n  === ACERTO POR POSICAO DO PASSO (chamada+arg, todas as familias) ===");
    for (pos, (ok, n)) in &por_posicao {
        println!(
            "  passo {pos}      {ok:>4}/{n:<4} {:>4.0}%",
            100.0 * *ok as f64 / *n as f64
        );
    }

    println!("\n  === DETERMINISMO E VAZAMENTO DE ESTADO ===");
    let n_passos_tot = n_modelos * SESSOES.iter().map(|s| s.passos.len()).sum::<usize>();
    println!(
        "  cache compartilhado != cache novo: {} de {n_passos_tot} passos",
        divergiu.len()
    );
    for d in divergiu.iter().take(10) {
        println!("    {d}");
    }
    println!(
        "  repeticoes instaveis (3x o mesmo pedido deu coisas diferentes): {}",
        repete_instavel.len()
    );
    for r in repete_instavel.iter().take(6) {
        println!("    {r}");
    }

    println!("\n  === PEDIDO COMPOSTO (nao pontuado, so observado) ===");
    for (pedido, dist) in &composto {
        println!("    {pedido}");
        let mut v: Vec<_> = dist.iter().collect();
        v.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
        for (saida, c) in v {
            println!("      {c:>3}/{n_modelos}  {saida}");
        }
    }

    println!("\n  === CHAMADA CERTA QUE MESMO ASSIM NAO EXECUTOU ===");
    if exec_falhou.is_empty() {
        println!("    nenhuma");
    }
    for (pedido, e) in &exec_falhou {
        println!("    {pedido}\n      -> {e}");
    }

    println!("\n  === OS PASSOS QUE MAIS ERRAM ===");
    let mut v: Vec<_> = erros.iter().collect();
    v.sort_by_key(|(_, (c, _))| std::cmp::Reverse(*c));
    for (pedido, (c, exemplo)) in v.iter().take(25) {
        println!("    {c:>3}/{n_modelos}  {pedido}\n              -> {exemplo}");
    }

    println!("\n  === SESSAO A SESSAO (primeiro modelo carregado) ===");
    println!("  ok=chamada certa  ARG=ferramenta certa e argumento errado  X=ferramenta errada  ?=nao pontuado\n");
    for l in &detalhe {
        println!("{l}");
    }

    match std::fs::remove_dir_all(&base) {
        Ok(_) => println!("\n  raiz temporaria apagada."),
        Err(e) => println!("\n  ATENCAO: nao consegui apagar {}: {e}", base.display()),
    }
}
