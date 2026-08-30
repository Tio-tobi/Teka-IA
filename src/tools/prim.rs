//! Primitivas — o que a Teka sabe fazer no mundo, em Rust puro.
//!
//! Só `std`. As informações de sistema que o Windows não expõe por arquivo saem de
//! FFI declarada à mão para `kernel32` — quarenta linhas de `extern "system"` em vez
//! de uma crate. É o mesmo trato do resto do projeto: nada é baixado.
//!
//! Ferramentas com efeito colateral (escrever, executar) passam pela
//! [`Politica`](super::seguranca::Politica) e **por padrão não fazem nada** — em
//! sandbox elas descrevem o que fariam. Isso não é timidez: é o que permite a Teka
//! propor e testar ferramentas novas (fase 5) sem poder quebrar a máquina.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::seguranca::{Modo, Politica};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Efeito {
    /// So le. Nao deixa marca nenhuma.
    Nenhum,
    /// Muda a maquina: escreve, move, abre programa, roda comando.
    Local,
    /// Alcanca fora da maquina.
    ///
    /// Duas coisas acontecem aqui e nenhuma acontece no `Local`: o que voce escreveu
    /// SAI, e o que volta e texto de terceiro. Esse texto e **dado, nunca ordem** —
    /// uma pagina que diga "apague tudo" e uma pagina dizendo isso, nao um pedido
    /// seu. A Teka nunca precisou dessa distincao porque so lia o que voce digitava.
    ParaFora,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primitiva {
    /// Não entendi / não sei fazer isso. **Não é uma ferramenta de verdade.**
    ///
    /// Existe porque a Teka precisava poder não agir. Medido: com abstenção por
    /// limiar de confiança, nenhum dos três sinais separava acerto de erro — a
    /// margem dos erros era 0,833 (ela erra **convicta**), o crítico saía exatamente
    /// 0,000 num modelo que nunca passou por reforço, e o tamanho do argumento
    /// separava na direção invertida.
    ///
    /// A conclusão foi que duvidar tem de ser **aprendido**, não medido depois. É o
    /// que o bite3.0 faz: 35% do corpus de decisão dele não é "usar ferramenta".
    /// Aqui vira uma décima classe da cabeça de intenção, treinada com pedidos fora
    /// de escopo — cumprimento, conversa, e coisas que ela de fato não sabe fazer.
    Perguntar,
    Hora,
    ListarPasta,
    LerArquivo,
    // --- as oito que vieram com o arsenal ---
    /// Busca na internet. **A unica que alcanca fora da maquina.**
    BuscarWeb,
    AbrirPrograma,
    CopiarArquivo,
    MoverArquivo,
    CriarPasta,
    InfoArquivo,
    Processos,
    Rede,
    ApagarArquivo,
    EscreverArquivo,
    ProcurarArquivo,
    Calcular,
    Memoria,
    Disco,
    ExecutarComando,
}

impl Primitiva {
    /// Esta ferramenta muda o mundo?
    ///
    /// Separa o que precisa de diário e de oficina do que não precisa. Ler duas
    /// vezes não custa nada; escrever duas vezes custa. Sem esta distinção, o diário
    /// pagaria um `fsync` por `hora` — e um `fsync` é mais caro que a ferramenta
    /// inteira.
    ///
    /// `ExecutarComando` conta como efeito colateral mesmo podendo ser um `dir`
    /// inofensivo: não há como saber sem interpretar o comando, e errar para o lado
    /// de tratar como perigoso custa um registro em log.
    /// Que tipo de marca a ferramenta deixa.
    ///
    /// Isto era um booleano — `EscreverArquivo | ExecutarComando` — e o booleano
    /// parou de servir quando a busca web entrou. Ler uma pagina e escrever um
    /// arquivo nao sao o mesmo risco, e as guardas (diario, oficina, confirmacao do
    /// contexto) todas se penduram nesta classificacao.
    ///
    /// A ideia de separar `local` de `para fora` veio do `bite3.0`, que ja media as
    /// duas coisas em separado nos testes de isolamento de autoridade.
    pub fn efeito(&self) -> Efeito {
        match self {
            Primitiva::EscreverArquivo
            | Primitiva::CopiarArquivo
            | Primitiva::MoverArquivo
            | Primitiva::CriarPasta
            | Primitiva::AbrirPrograma
            | Primitiva::ApagarArquivo
            | Primitiva::ExecutarComando => Efeito::Local,
            // Sai da maquina: manda o que voce escreveu para um servidor, e traz de
            // volta texto que NAO e ordem sua. Ver `Efeito::ParaFora`.
            Primitiva::BuscarWeb => Efeito::ParaFora,
            _ => Efeito::Nenhum,
        }
    }

    /// Mexe no mundo de algum jeito? Mantido para quem so precisa do sim ou nao.
    pub fn efeito_colateral(&self) -> bool {
        self.efeito() != Efeito::Nenhum
    }

    pub fn executar(&self, args: &[(String, String)], pol: &Politica) -> Result<String, String> {
        let arg = |nome: &str| -> String {
            args.iter()
                .find(|(k, _)| k == nome)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        match self {
            // Não toca no mundo por construção: é a ação de NÃO agir.
            Primitiva::Perguntar => {
                Ok("nao entendi o que voce quer que eu faca. pode dizer de outro jeito?".into())
            }
            Primitiva::Hora => Ok(agora()),
            Primitiva::ListarPasta => listar(Path::new(&arg("caminho")), pol),
            Primitiva::LerArquivo => ler(Path::new(&arg("caminho")), pol),
            Primitiva::EscreverArquivo => escrever(Path::new(&arg("caminho")), &arg("texto"), pol),
            Primitiva::ProcurarArquivo => procurar(Path::new(&arg("raiz")), &arg("nome"), pol),
            Primitiva::Calcular => calcular(&arg("expressao")),
            Primitiva::Memoria => memoria(),
            Primitiva::BuscarWeb => buscar_web(&arg("consulta")),
            Primitiva::AbrirPrograma => abrir_programa(&arg("programa"), pol),
            Primitiva::CopiarArquivo => copiar(&arg("origem"), &arg("destino"), pol),
            Primitiva::MoverArquivo => mover(&arg("origem"), &arg("destino"), pol),
            Primitiva::CriarPasta => criar_pasta(Path::new(&arg("caminho")), pol),
            Primitiva::InfoArquivo => info_arquivo(Path::new(&arg("caminho")), pol),
            Primitiva::Processos => processos(),
            Primitiva::Rede => rede(),
            Primitiva::ApagarArquivo => apagar(Path::new(&arg("caminho")), pol),
            Primitiva::Disco => disco(&arg("caminho")),
            Primitiva::ExecutarComando => executar_cmd(&arg("comando"), pol),
        }
    }
}

// ---------------------------------------------------------------------------
// FFI — o mínimo do kernel32
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    #[repr(C)]
    #[derive(Default)]
    pub struct SystemTime {
        pub ano: u16,
        pub mes: u16,
        pub dia_semana: u16,
        pub dia: u16,
        pub hora: u16,
        pub minuto: u16,
        pub segundo: u16,
        pub ms: u16,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct MemoryStatusEx {
        pub tamanho: u32,
        pub carga: u32,
        pub total_fis: u64,
        pub disp_fis: u64,
        pub total_page: u64,
        pub disp_page: u64,
        pub total_virt: u64,
        pub disp_virt: u64,
        pub disp_ext: u64,
    }

    unsafe extern "system" {
        pub fn GetLocalTime(st: *mut SystemTime);
        pub fn GlobalMemoryStatusEx(m: *mut MemoryStatusEx) -> i32;
        pub fn GetDiskFreeSpaceExW(
            dir: *const u16,
            livre_usuario: *mut u64,
            total: *mut u64,
            livre_total: *mut u64,
        ) -> i32;
    }

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

// ---------------------------------------------------------------------------
// implementações
// ---------------------------------------------------------------------------

pub fn agora() -> String {
    #[cfg(windows)]
    {
        let mut st = win::SystemTime::default();
        unsafe { win::GetLocalTime(&mut st) };
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            st.ano, st.mes, st.dia, st.hora, st.minuto, st.segundo
        );
    }
    #[cfg(not(windows))]
    {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("unix:{d}")
    }
}

fn listar(caminho: &Path, pol: &Politica) -> Result<String, String> {
    // Caminho relativo se resolve contra a raiz da politica, nao contra o
    // diretorio de onde o processo subiu. Ver `Politica::resolver_leitura`.
    let caminho = if caminho.as_os_str().is_empty() {
        pol.raiz_ou_atual()
    } else {
        pol.resolver_leitura(caminho)
    };
    let caminho = caminho.as_path();
    let mut itens: Vec<(bool, String, u64)> = Vec::new();
    for e in std::fs::read_dir(caminho).map_err(|e| format!("{}: {e}", caminho.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let md = e.metadata().map_err(|e| e.to_string())?;
        itens.push((md.is_dir(), e.file_name().to_string_lossy().into(), md.len()));
    }
    itens.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    itens.truncate(200);

    let mut s = format!("{} ({} itens)\n", caminho.display(), itens.len());
    for (dir, nome, tam) in itens {
        if dir {
            let _ = writeln!(s, "  [pasta] {nome}");
        } else {
            let _ = writeln!(s, "  {:>10}  {nome}", humano(tam));
        }
    }
    Ok(s)
}

fn ler(caminho: &Path, pol: &Politica) -> Result<String, String> {
    let caminho = pol.resolver_leitura(caminho);
    let caminho = caminho.as_path();
    let dados = std::fs::read(caminho).map_err(|e| format!("{}: {e}", caminho.display()))?;
    let corte = dados.len().min(pol.max_leitura);
    let mut s = String::from_utf8_lossy(&dados[..corte]).into_owned();
    if corte < dados.len() {
        let _ = write!(s, "\n[... truncado, {} bytes no total]", dados.len());
    }
    Ok(s)
}

// ─────────────────────────── as oito novas ───────────────────────────

fn buscar_web(consulta: &str) -> Result<String, String> {
    if consulta.trim().is_empty() {
        return Err("buscar_web precisa de uma consulta".into());
    }
    // A Instant Answer API devolve resumo e definicao, quase sempre da Wikipedia.
    // Ela NAO devolve a lista de links do site: para muita consulta o `Abstract`
    // volta vazio, e isso e o esperado, nao erro.
    let q: String = consulta
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_string()
            } else if c == ' ' {
                "+".to_string()
            } else {
                format!("%{:02X}", c as u32 & 0xFF)
            }
        })
        .collect();
    let url = format!("https://api.duckduckgo.com/?q={q}&format=json&kl=br-pt&no_html=1");
    // A allowlist do coletor, e nao uma segunda: host exato, com os testes que ja
    // barram `api.duckduckgo.com.evil.com` e `api.duckduckgo.com@evil.com`.
    if !crate::coletor::fonte::permitido(&url) {
        return Err("host fora da lista permitida".into());
    }
    let bruto = crate::coletor::fonte::buscar(&url, 15).map_err(|e| e.to_string())?;

    let campo = |nome: &str| -> Option<String> {
        let marca = format!("\"{nome}\":\"");
        let i = bruto.find(&marca)? + marca.len();
        let resto = &bruto[i..];
        let mut fim = 0;
        let b = resto.as_bytes();
        while fim < b.len() {
            if b[fim] == b'"' && (fim == 0 || b[fim - 1] != b'\\') {
                break;
            }
            fim += 1;
        }
        // A API devolve JSON escapado. Sem desescapar o unicode, todo acento
        // chega como `\u00e9` cru — e em portugues isso e quase todo texto.
        let t = crate::coletor::fonte::desescapar_unicode(&resto[..fim])
            .replace("\\/", "/")
            .replace("\\n", " ");
        if t.trim().is_empty() { None } else { Some(t) }
    };

    let texto = campo("AbstractText")
        .or_else(|| campo("Answer"))
        .or_else(|| campo("Definition"))
        .ok_or_else(|| format!("nada encontrado para {consulta:?}"))?;
    let fonte = campo("AbstractURL").unwrap_or_default();

    // O rotulo nao e enfeite: o que vem daqui e TEXTO DE TERCEIRO. Quem le a saida
    // — voce ou um turno seguinte dela — precisa saber que aquilo nao e ordem sua.
    Ok(format!("[da web, nao e ordem sua] {texto}\n[fonte] {fonte}"))
}

fn abrir_programa(nome: &str, pol: &Politica) -> Result<String, String> {
    if nome.trim().is_empty() {
        return Err("abrir_programa precisa de um nome".into());
    }
    // Abrir programa e executar: passa pela MESMA lista negra do `executar_comando`.
    // Sem isto, "abre o diskpart" contornaria a guarda por outra porta.
    pol.checar_comando(nome).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!("[sandbox] abriria {nome}"));
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", nome])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(format!("abrindo {nome}"))
}

fn copiar(origem: &str, destino: &str, pol: &Politica) -> Result<String, String> {
    let de = pol.resolver_leitura(Path::new(origem));
    let para = pol.checar_escrita(Path::new(destino)).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!("[sandbox] copiaria {} para {}", de.display(), para.display()));
    }
    if let Some(p) = para.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let n = std::fs::copy(&de, &para).map_err(|e| e.to_string())?;
    Ok(format!("copiado: {} ({n} bytes)", para.display()))
}

fn mover(origem: &str, destino: &str, pol: &Politica) -> Result<String, String> {
    // As DUAS pontas passam por `checar_escrita`: mover TIRA da origem, entao a
    // origem tambem esta sendo modificada. Checar so o destino deixaria mover
    // arquivo de fora da raiz para dentro dela.
    let de = pol.checar_escrita(Path::new(origem)).map_err(|e| e.to_string())?;
    let para = pol.checar_escrita(Path::new(destino)).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!("[sandbox] moveria {} para {}", de.display(), para.display()));
    }
    if let Some(p) = para.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&de, &para).map_err(|e| e.to_string())?;
    Ok(format!("movido para {}", para.display()))
}

/// Apaga um arquivo. A unica primitiva sem volta.
///
/// Existe porque o treino roda em container: la o pior caso e um volume descartavel,
/// e uma ferramenta que nunca aparece no treino e uma ferramenta que ela nunca
/// aprende a usar. Manter fora do treino nao a torna segura — torna ela ignorante
/// sobre ela.
///
/// Fora do container, quem decide e a politica de sempre: sandbox por padrao,
/// confinamento de raiz, e o diario registrando ANTES de agir. Aqui vao duas guardas
/// a mais, que as outras nao precisam:
///
///   - pasta nao, so arquivo — `remove_dir_all` num caminho errado e a diferenca
///     entre perder um arquivo e perder uma arvore
///   - o diario ja impede repetir; isto impede a primeira vez ser catastrofica
fn apagar(caminho: &Path, pol: &Politica) -> Result<String, String> {
    let alvo = pol.checar_escrita(caminho).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!("[sandbox] apagaria {}", alvo.display()));
    }
    let m = std::fs::metadata(&alvo).map_err(|e| e.to_string())?;
    if m.is_dir() {
        return Err(format!(
            "{} e uma pasta — apagar_arquivo nao apaga arvore",
            alvo.display()
        ));
    }
    std::fs::remove_file(&alvo).map_err(|e| e.to_string())?;
    Ok(format!("apagado: {} ({} bytes)", alvo.display(), m.len()))
}

fn criar_pasta(caminho: &Path, pol: &Politica) -> Result<String, String> {
    let alvo = pol.checar_escrita(caminho).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!("[sandbox] criaria a pasta {}", alvo.display()));
    }
    std::fs::create_dir_all(&alvo).map_err(|e| e.to_string())?;
    Ok(format!("pasta criada: {}", alvo.display()))
}

fn info_arquivo(caminho: &Path, pol: &Politica) -> Result<String, String> {
    let alvo = pol.resolver_leitura(caminho);
    let m = std::fs::metadata(&alvo).map_err(|e| e.to_string())?;
    let tipo = if m.is_dir() { "pasta" } else { "arquivo" };
    let quando = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(format!(
        "{tipo}: {} | {} bytes | modificado ha {} dias",
        alvo.display(),
        m.len(),
        (agora_segundos().saturating_sub(quando)) / 86_400
    ))
}

fn agora_segundos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn processos() -> Result<String, String> {
    let s = std::process::Command::new("tasklist")
        .arg("/FO")
        .arg("CSV")
        .output()
        .map_err(|e| e.to_string())?;
    let txt = String::from_utf8_lossy(&s.stdout);
    let n = txt.lines().count().saturating_sub(1);
    let topo: Vec<&str> = txt.lines().skip(1).take(12).collect();
    Ok(format!("{n} processos\n{}", topo.join("\n")))
}

fn rede() -> Result<String, String> {
    let s = std::process::Command::new("ipconfig")
        .output()
        .map_err(|e| e.to_string())?;
    let txt = String::from_utf8_lossy(&s.stdout);
    let linhas: Vec<&str> = txt
        .lines()
        .filter(|l| {
            let b = l.to_lowercase();
            b.contains("ipv4") || b.contains("gateway") || b.contains("adaptador")
        })
        .take(12)
        .collect();
    if linhas.is_empty() {
        return Err("nao consegui ler a rede".into());
    }
    Ok(linhas.join("\n"))
}

fn escrever(caminho: &Path, texto: &str, pol: &Politica) -> Result<String, String> {
    let alvo = pol.checar_escrita(caminho).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!(
            "[sandbox] escreveria {} bytes em {}",
            texto.len(),
            alvo.display()
        ));
    }
    if let Some(pai) = alvo.parent() {
        std::fs::create_dir_all(pai).map_err(|e| e.to_string())?;
    }
    std::fs::write(&alvo, texto).map_err(|e| e.to_string())?;
    Ok(format!("escrito: {} ({} bytes)", alvo.display(), texto.len()))
}

fn procurar(raiz: &Path, nome: &str, pol: &Politica) -> Result<String, String> {
    if nome.is_empty() {
        return Err("procurar_arquivo precisa de um nome".into());
    }
    let raiz = if raiz.as_os_str().is_empty() {
        pol.raiz_ou_atual()
    } else {
        pol.resolver_leitura(raiz)
    };
    let alvo = nome.to_lowercase();
    let mut achados = Vec::new();
    // Profundidade e contagem limitadas: uma busca sem teto numa raiz errada
    // trava o agente por minutos e não ajuda ninguém.
    varrer(&raiz, &alvo, 0, 8, &mut achados);
    if achados.is_empty() {
        return Ok(format!("nada encontrado com \"{nome}\" em {}", raiz.display()));
    }
    let mut s = format!("{} resultado(s):\n", achados.len());
    for c in achados.iter().take(50) {
        let _ = writeln!(s, "  {}", c.display());
    }
    Ok(s)
}

fn varrer(dir: &Path, alvo: &str, prof: usize, max_prof: usize, saida: &mut Vec<PathBuf>) {
    if prof > max_prof || saida.len() >= 50 {
        return;
    }
    let Ok(itens) = std::fs::read_dir(dir) else {
        return; // pasta sem permissão: ignora e segue
    };
    for e in itens.flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().to_lowercase().contains(alvo) {
            saida.push(p.clone());
            if saida.len() >= 50 {
                return;
            }
        }
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            varrer(&p, alvo, prof + 1, max_prof, saida);
        }
    }
}

fn memoria() -> Result<String, String> {
    #[cfg(windows)]
    {
        let mut m = win::MemoryStatusEx {
            tamanho: std::mem::size_of::<win::MemoryStatusEx>() as u32,
            ..Default::default()
        };
        if unsafe { win::GlobalMemoryStatusEx(&mut m) } == 0 {
            return Err("GlobalMemoryStatusEx falhou".into());
        }
        return Ok(format!(
            "RAM: {} em uso de {} ({}% ocupado), {} livre",
            humano(m.total_fis - m.disp_fis),
            humano(m.total_fis),
            m.carga,
            humano(m.disp_fis)
        ));
    }
    #[cfg(not(windows))]
    Err("memoria: só implementado no Windows".into())
}

fn disco(caminho: &str) -> Result<String, String> {
    let alvo = if caminho.is_empty() { "C:\\" } else { caminho };
    #[cfg(windows)]
    {
        let w = win::wide(alvo);
        let (mut livre_u, mut total, mut livre_t) = (0u64, 0u64, 0u64);
        if unsafe { win::GetDiskFreeSpaceExW(w.as_ptr(), &mut livre_u, &mut total, &mut livre_t) }
            == 0
        {
            return Err(format!("nao consegui ler o disco de {alvo}"));
        }
        let pct = if total > 0 {
            100.0 * (total - livre_t) as f64 / total as f64
        } else {
            0.0
        };
        return Ok(format!(
            "{alvo}  {} usados de {} ({pct:.0}%), {} livres",
            humano(total - livre_t),
            humano(total),
            humano(livre_t)
        ));
    }
    #[cfg(not(windows))]
    Err(format!("disco {alvo}: só implementado no Windows"))
}

fn executar_cmd(cmd: &str, pol: &Politica) -> Result<String, String> {
    if cmd.trim().is_empty() {
        return Err("comando vazio".into());
    }
    pol.checar_comando(cmd).map_err(|e| e.to_string())?;
    if pol.modo == Modo::Sandbox {
        return Ok(format!("[sandbox] executaria: {cmd}"));
    }
    let saida = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", cmd]).output()
    } else {
        std::process::Command::new("sh").args(["-c", cmd]).output()
    }
    .map_err(|e| e.to_string())?;

    let mut s = String::from_utf8_lossy(&saida.stdout).into_owned();
    if !saida.stderr.is_empty() {
        let _ = write!(s, "\n[stderr] {}", String::from_utf8_lossy(&saida.stderr));
    }
    Ok(s)
}

pub fn humano(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

// ---------------------------------------------------------------------------
// calculadora
// ---------------------------------------------------------------------------

/// Descida recursiva sobre `+ - * / % ^ ( )`.
///
/// Existe porque "quanto é 15% de 340" é um pedido comum e delegar isso ao shell
/// seria abrir uma porta de execução por um motivo ridículo.
pub fn calcular(expr: &str) -> Result<String, String> {
    let bytes: Vec<u8> = expr.bytes().collect();
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Err("expressao vazia".into());
    }
    let mut p = Parser { b: &bytes, i: 0 };
    let v = p.expr()?;
    p.olhar(); // consome espaco final
    if p.i != p.b.len() {
        return Err(format!("lixo no fim da expressao (posicao {})", p.i));
    }
    if !v.is_finite() {
        return Err("resultado nao finito (divisao por zero?)".into());
    }
    Ok(if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    })
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    /// Pula espaco e espia. E `&mut self` de proposito: o espaco entre TOKENS e
    /// irrelevante, mas o espaco DENTRO de um numero nao e -- "2 3" tem que ser
    /// erro, nao 23. Por isso a varredura de digitos em `primario` indexa os bytes
    /// crus, sem passar por aqui.
    fn olhar(&mut self) -> Option<u8> {
        while matches!(self.b.get(self.i), Some(c) if c.is_ascii_whitespace()) {
            self.i += 1;
        }
        self.b.get(self.i).copied()
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.termo()?;
        while let Some(op @ (b'+' | b'-')) = self.olhar() {
            self.i += 1;
            let r = self.termo()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn termo(&mut self) -> Result<f64, String> {
        let mut v = self.fator()?;
        while let Some(op @ (b'*' | b'/' | b'%')) = self.olhar() {
            self.i += 1;
            let r = self.fator()?;
            v = match op {
                b'*' => v * r,
                b'/' => {
                    if r == 0.0 {
                        return Err("divisao por zero".into());
                    }
                    v / r
                }
                _ => {
                    if r == 0.0 {
                        return Err("resto por zero".into());
                    }
                    v % r
                }
            };
        }
        Ok(v)
    }

    fn fator(&mut self) -> Result<f64, String> {
        let base = self.unario()?;
        if self.olhar() == Some(b'^') {
            self.i += 1;
            let exp = self.fator()?; // associativo à direita
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn unario(&mut self) -> Result<f64, String> {
        match self.olhar() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.unario()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unario()
            }
            _ => self.primario(),
        }
    }

    fn primario(&mut self) -> Result<f64, String> {
        match self.olhar() {
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if self.olhar() != Some(b')') {
                    return Err("parentese nao fechado".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' || c == b',' => {
                let ini = self.i;
                // Peek CRU, sem passar por `olhar`: um espaço aqui termina o
                // número. É o que faz "2 3" ser erro em vez de virar 23.
                while matches!(self.b.get(self.i), Some(d) if d.is_ascii_digit() || *d == b'.' || *d == b',')
                {
                    self.i += 1;
                }
                // Vírgula decimal: é português, o usuário vai escrever "3,5".
                let txt: String = self.b[ini..self.i]
                    .iter()
                    .map(|&c| if c == b',' { '.' } else { c as char })
                    .collect();
                txt.parse::<f64>().map_err(|_| format!("numero invalido: {txt}"))
            }
            Some(c) => Err(format!("caractere inesperado: {:?}", c as char)),
            None => Err("expressao terminou cedo demais".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculadora_respeita_precedencia() {
        assert_eq!(calcular("2+3*4").unwrap(), "14");
        assert_eq!(calcular("(2+3)*4").unwrap(), "20");
        assert_eq!(calcular("2^3^2").unwrap(), "512"); // direita: 2^(3^2)
        assert_eq!(calcular("-3+5").unwrap(), "2");
        assert_eq!(calcular("10/4").unwrap(), "2.5");
        assert_eq!(calcular("10%3").unwrap(), "1");
    }

    #[test]
    fn espaco_entre_tokens_ok_mas_dentro_do_numero_nao() {
        assert_eq!(calcular(" 2 + 3 * 4 ").unwrap(), "14");
        assert!(calcular("2 3").is_err(), "\"2 3\" deveria ser erro, nao 23");
        // Consequencia disso: separador de milhar nao e suportado, e melhor
        // recusar do que adivinhar. "1.000" em portugues e mil; aqui e 1,0.
        assert_eq!(calcular("1.000").unwrap(), "1");
    }

    #[test]
    fn calculadora_aceita_virgula_decimal() {
        assert_eq!(calcular("3,5*2").unwrap(), "7");
        assert_eq!(calcular("0,15*340").unwrap(), "51");
    }

    #[test]
    fn calculadora_recusa_lixo() {
        assert!(calcular("2+").is_err());
        assert!(calcular("(2+3").is_err());
        assert!(calcular("2 3").is_err());
        assert!(calcular("1/0").is_err());
        assert!(calcular("").is_err());
        assert!(calcular("del /s").is_err());
    }

    #[test]
    fn hora_tem_formato_utilizavel() {
        let h = agora();
        assert!(h.len() >= 10, "hora estranha: {h}");
    }

    #[test]
    fn memoria_e_disco_respondem_no_windows() {
        if cfg!(windows) {
            let m = memoria().expect("memoria falhou");
            assert!(m.contains("RAM"), "{m}");
            let d = disco("C:\\").expect("disco falhou");
            assert!(d.contains("livres"), "{d}");
            println!("\n  {m}\n  {d}");
        }
    }

    #[test]
    fn escrita_fora_da_raiz_e_recusada_mesmo_em_modo_real() {
        let pol = Politica::real_em(std::env::temp_dir().join("teka_area"));
        let r = escrever(Path::new("..\\..\\fuga.txt"), "x", &pol);
        assert!(r.is_err(), "deveria recusar: {r:?}");
    }

    #[test]
    fn sandbox_nao_toca_no_disco() {
        let dir = std::env::temp_dir().join("teka_sandbox_teste");
        let pol = Politica {
            modo: Modo::Sandbox,
            raiz: Some(dir.clone()),
            ..Default::default()
        };
        let r = escrever(Path::new("nao_deve_existir.txt"), "ola", &pol).unwrap();
        assert!(r.starts_with("[sandbox]"), "{r}");
        assert!(!dir.join("nao_deve_existir.txt").exists());
    }

    #[test]
    fn comando_destrutivo_e_recusado_antes_de_rodar() {
        let pol = Politica::real_em(std::env::temp_dir());
        assert!(executar_cmd("del /s C:\\", &pol).is_err());
        assert!(executar_cmd("shutdown -s", &pol).is_err());
    }

    #[test]
    fn listar_pasta_funciona() {
        let s = listar(Path::new("."), &Politica::default()).expect("listar falhou");
        assert!(s.contains("itens"), "{s}");
    }
}
