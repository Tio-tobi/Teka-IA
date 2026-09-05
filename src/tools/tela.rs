//! Uma área de trabalho que ninguém vê.
//!
//! ## Por que existe
//!
//! O laço de prática escolhe ferramenta **explorando com temperatura** — nos testes,
//! com um modelo não treinado, o que é sorteio pouco disfarçado. Em 2026-09-04 isso
//! encheu a área de trabalho do John de janelas do Windows (`carinho`, `nenhuma`,
//! `pra eu`), e o remendo foi desligar processos no laço inteiro
//! ([`Politica::processos`]).
//!
//! O remendo custou zero em aprendizado — o mundo de prática nunca soube montar
//! tarefa para `abrir_programa` nem `executar_comando`. Mas é justamente esse o
//! problema: **essas duas ferramentas não têm sinal de prática nenhum**, e são duas
//! das três famílias que a análise de erro apontou como o gargalo da escolha de
//! ferramenta.
//!
//! Um objeto de *desktop* do Windows resolve os dois lados: o processo nasce numa
//! área de trabalho separada, as janelas não encostam na tela de ninguém, e
//! "o processo subiu?" vira um sinal verificável.
//!
//! ## O que isto NÃO é
//!
//! **Não é uma fronteira de segurança.** O processo roda com o mesmo token do
//! usuário: mesmo acesso a arquivo, mesma rede. Um `del` continua apagando. Isto
//! resolve a *interferência*, não o *risco* — quem resolve o risco é a lista negra,
//! a raiz, e a confirmação antes de agir.
//!
//! Uma VM daria isolamento de verdade, e custaria 1 a 4 GB de RAM numa máquina cujo
//! gargalo medido é exatamente a RAM em single channel. Isto custa um handle.
//!
//! ## A armadilha ao usar isto para treinar
//!
//! Numa área de trabalho vazia, abrir programa quase sempre "dá certo". O
//! [`crate::pulso`] já registra para onde isso leva: recompensar sucesso ensina a
//! chamar a ferramenta infalível para tudo — foi o que aconteceu com `hora`. A
//! defesa existente no laço (acerto vale **zero**, erro custa) precisa continuar
//! valendo, senão a tela virtual vira uma máquina de ensinar o vício.

#[cfg(windows)]
mod win {
    /// Conjunto clássico de acesso a desktop **sem** `DESKTOP_SWITCHDESKTOP`.
    ///
    /// Deixar o switch de fora é de propósito: nada que rode aqui deve conseguir
    /// trazer esta área de trabalho para a frente da do usuário.
    pub const ACESSO: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0008 | 0x0040 | 0x0080;

    /// Sem console piscando na tela, mesmo fora da área de trabalho virtual.
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[repr(C)]
    pub struct StartupInfoW {
        pub cb: u32,
        pub reservado: *mut u16,
        /// **O campo que importa.** É por aqui que o processo nasce em outra tela.
        pub desktop: *mut u16,
        pub titulo: *mut u16,
        pub x: u32,
        pub y: u32,
        pub xsize: u32,
        pub ysize: u32,
        pub xchars: u32,
        pub ychars: u32,
        pub preenchimento: u32,
        pub flags: u32,
        pub mostrar: u16,
        pub reservado2: u16,
        pub reservado2p: *mut u8,
        pub entrada: isize,
        pub saida: isize,
        pub erro: isize,
    }

    impl Default for StartupInfoW {
        fn default() -> Self {
            Self {
                cb: std::mem::size_of::<StartupInfoW>() as u32,
                reservado: std::ptr::null_mut(),
                desktop: std::ptr::null_mut(),
                titulo: std::ptr::null_mut(),
                x: 0,
                y: 0,
                xsize: 0,
                ysize: 0,
                xchars: 0,
                ychars: 0,
                preenchimento: 0,
                flags: 0,
                mostrar: 0,
                reservado2: 0,
                reservado2p: std::ptr::null_mut(),
                entrada: 0,
                saida: 0,
                erro: 0,
            }
        }
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct ProcessInformation {
        pub processo: isize,
        pub thread: isize,
        pub pid: u32,
        pub tid: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn CreateDesktopW(
            nome: *const u16,
            dispositivo: *const u16,
            devmode: *const core::ffi::c_void,
            flags: u32,
            acesso: u32,
            sa: *const core::ffi::c_void,
        ) -> isize;
        pub fn CloseDesktop(h: isize) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[allow(clippy::too_many_arguments)]
        pub fn CreateProcessW(
            aplicacao: *const u16,
            linha: *mut u16,
            sa_proc: *const core::ffi::c_void,
            sa_thread: *const core::ffi::c_void,
            herdar: i32,
            flags: u32,
            ambiente: *const core::ffi::c_void,
            dir_atual: *const u16,
            si: *const StartupInfoW,
            pi: *mut ProcessInformation,
        ) -> i32;
        pub fn CloseHandle(h: isize) -> i32;
        pub fn WaitForSingleObject(h: isize, ms: u32) -> u32;
        pub fn GetLastError() -> u32;
    }

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

/// Caracteres que não podem entrar num argumento.
///
/// `std::process::Command` cita os argumentos por nós; `CreateProcessW` recebe uma
/// string única e **não cita nada**. Montar a linha na mão reabre uma superfície de
/// injeção que o caminho antigo não tinha, então cada argumento é recusado antes de
/// virar texto. A lista negra de `seguranca` continua rodando por cima — isto é o
/// cinto, não o suspensório.
///
/// `%` está na lista porque `cmd` expande `%VAR%` **depois** da citação: aspas não
/// protegem contra isso.
const PROIBIDOS: [char; 8] = ['"', '&', '|', '<', '>', '^', '%', '\n'];

/// Cita um argumento do jeito que o `CreateProcessW` espera.
///
/// Só cita quando precisa — `cmd.exe` não reconhece `"/C"` com aspas. Como `"` está
/// entre os [`PROIBIDOS`], não há caso de aspa interna para escapar; a única sobra
/// seria uma contrabarra final comendo a aspa de fechamento, e ela também é recusada.
fn citar(arg: &str) -> Result<String, String> {
    if let Some(c) = arg.chars().find(|c| PROIBIDOS.contains(c)) {
        return Err(format!("caractere {c:?} nao pode entrar num argumento"));
    }
    if arg.ends_with('\\') {
        return Err("argumento nao pode terminar em contrabarra".into());
    }
    Ok(if arg.is_empty() || arg.contains(' ') {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    })
}

/// Uma área de trabalho separada, viva enquanto este valor existir.
///
/// O handle fecha no `Drop`. Fechar o desktop **não** mata o que estiver rodando
/// nele: o Windows só destrói o objeto quando o último processo sai.
pub struct TelaVirtual {
    nome: String,
    #[cfg(windows)]
    handle: isize,
}

impl TelaVirtual {
    /// Cria a área de trabalho. O nome precisa ser único dentro da sessão.
    #[cfg(windows)]
    pub fn nova(nome: &str) -> Result<Self, String> {
        if nome.is_empty() || nome.contains('\\') {
            return Err(format!("nome de tela invalido: {nome:?}"));
        }
        let w = win::wide(nome);
        let h = unsafe {
            win::CreateDesktopW(
                w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                win::ACESSO,
                std::ptr::null(),
            )
        };
        if h == 0 {
            return Err(format!(
                "CreateDesktopW falhou (erro {})",
                unsafe { win::GetLastError() }
            ));
        }
        Ok(Self { nome: nome.to_string(), handle: h })
    }

    #[cfg(not(windows))]
    pub fn nova(_nome: &str) -> Result<Self, String> {
        Err("tela virtual so existe no Windows".into())
    }

    pub fn nome(&self) -> &str {
        &self.nome
    }
}

#[cfg(windows)]
impl Drop for TelaVirtual {
    fn drop(&mut self) {
        unsafe { win::CloseDesktop(self.handle) };
    }
}

/// Um processo lançado. `pid` serve de sinal verificável: subiu ou não subiu.
pub struct Lancado {
    pub pid: u32,
    #[cfg(windows)]
    processo: isize,
}

impl Lancado {
    /// Espera terminar, em milissegundos. `None` se estourou o tempo.
    ///
    /// Serve ao teste e ao mundo de prática, que precisa saber se o processo
    /// realmente rodou antes de julgar a tentativa.
    #[cfg(windows)]
    pub fn esperar(&self, ms: u32) -> Option<()> {
        match unsafe { win::WaitForSingleObject(self.processo, ms) } {
            0 => Some(()),
            _ => None,
        }
    }

    #[cfg(not(windows))]
    pub fn esperar(&self, _ms: u32) -> Option<()> {
        None
    }
}

#[cfg(windows)]
impl Drop for Lancado {
    fn drop(&mut self) {
        unsafe { win::CloseHandle(self.processo) };
    }
}

/// Lança um programa, opcionalmente numa área de trabalho separada.
///
/// Recebe programa e argumentos **separados**, como `Command::args`, e monta a linha
/// aqui — é o que impede que um argumento vire comando. `tela: None` usa a área de
/// trabalho de quem chamou, que é o comportamento de sempre.
///
/// `dir` é o diretório de trabalho do processo. Passar `None` faz ele herdar o de
/// quem chamou, que é **a pasta de onde o binário foi lançado** — quase nunca o que
/// se quer quando existe uma raiz confinada. É o mesmo descasamento que já mordeu
/// este projeto uma vez em [`crate::tools::seguranca::Politica::resolver_leitura`]:
/// escrever ia para a raiz e ler procurava em outro lugar, e `ler_arquivo` deu zero
/// acertos em dez rodadas com a ferramenta e o argumento certos.
#[cfg(windows)]
pub fn lancar(
    programa: &str,
    args: &[&str],
    tela: Option<&str>,
    dir: Option<&std::path::Path>,
) -> Result<Lancado, String> {
    let mut linha = citar(programa)?;
    for a in args {
        linha.push(' ');
        linha.push_str(&citar(a)?);
    }
    // `CreateProcessW` pode ESCREVER nesta string — o contrato exige buffer mutável.
    let mut linha_w = win::wide(&linha);
    let mut nome_w = tela.map(win::wide);
    let dir_w = dir.map(|d| win::wide(&d.display().to_string()));

    let si = win::StartupInfoW {
        desktop: nome_w.as_mut().map_or(std::ptr::null_mut(), |v| v.as_mut_ptr()),
        ..Default::default()
    };
    let mut pi = win::ProcessInformation::default();

    let ok = unsafe {
        win::CreateProcessW(
            std::ptr::null(),
            linha_w.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            win::CREATE_NO_WINDOW,
            std::ptr::null(),
            dir_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(format!("CreateProcessW falhou (erro {})", unsafe { win::GetLastError() }));
    }
    unsafe { win::CloseHandle(pi.thread) };
    Ok(Lancado { pid: pi.pid, processo: pi.processo })
}

#[cfg(not(windows))]
pub fn lancar(
    _programa: &str,
    _args: &[&str],
    _tela: Option<&str>,
    _dir: Option<&std::path::Path>,
) -> Result<Lancado, String> {
    Err("tela virtual so existe no Windows".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Nome único por teste: duas telas com o mesmo nome na mesma sessão colidem.
    fn nome(s: &str) -> String {
        format!("teka_{s}_{}", std::process::id())
    }

    #[test]
    fn cria_e_fecha_uma_tela() {
        let t = TelaVirtual::nova(&nome("cria")).expect("CreateDesktopW falhou");
        assert!(t.nome().starts_with("teka_"));
    }

    #[test]
    fn nome_com_barra_e_recusado() {
        assert!(TelaVirtual::nova("a\\b").is_err());
        assert!(TelaVirtual::nova("").is_err());
    }

    /// A prova de que o processo REALMENTE rodou na tela separada: ele cria uma
    /// pasta. Sem isto o teste provaria só que `CreateProcessW` devolveu sucesso.
    ///
    /// Usa `mkdir` e não `echo > arquivo` porque redirecionar precisa de `>`, e o
    /// `>` está entre os caracteres que [`PROIBIDOS`] recusa. A primeira versão
    /// deste teste morreu no próprio filtro — o que também serviu de prova de que
    /// o filtro pega.
    #[test]
    fn o_processo_roda_de_verdade_na_tela_separada() {
        let t = TelaVirtual::nova(&nome("roda")).expect("tela");
        let alvo = std::env::temp_dir().join(format!("teka_tela_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&alvo);

        let p = lancar(
            "cmd.exe",
            &["/C", "mkdir", &alvo.display().to_string()],
            Some(t.nome()),
            None,
        )
        .expect("lancar");
        assert!(p.pid != 0, "pid zero");
        p.esperar(10_000).expect("o processo nao terminou em 10s");

        assert!(alvo.is_dir(), "o processo nao criou {}", alvo.display());
        let _ = std::fs::remove_dir_all(&alvo);
    }

    /// Montar a linha à mão reabre injeção que `Command::args` não tinha.
    #[test]
    fn argumento_com_injecao_e_recusado() {
        let t = TelaVirtual::nova(&nome("inj")).expect("tela");
        for ruim in ["a & del b", "x\"y", "a | b", "%USERPROFILE%", "c:\\pasta\\"] {
            assert!(
                lancar("cmd.exe", &["/C", "echo", ruim], Some(t.nome()), None).is_err(),
                "deixou passar: {ruim}"
            );
        }
    }

    /// O processo nasce NA PASTA que a política manda, não na de quem lançou.
    ///
    /// Prova com caminho relativo: `mkdir marca` só cai no lugar certo se o
    /// `lpCurrentDirectory` tiver chegado. Passar `None` aqui faria a pasta aparecer
    /// no diretório do binário de teste — que é exatamente o descasamento que já
    /// custou dez rodadas de `ler_arquivo` a zero neste projeto.
    #[test]
    fn o_processo_nasce_na_pasta_pedida() {
        let t = TelaVirtual::nova(&nome("dir")).expect("tela");
        let casa = std::env::temp_dir().join(format!("teka_dir_{}", std::process::id()));
        std::fs::create_dir_all(&casa).expect("casa");

        let p = lancar("cmd.exe", &["/C", "mkdir", "marca"], Some(t.nome()), Some(&casa))
            .expect("lancar");
        p.esperar(10_000).expect("o processo nao terminou em 10s");

        assert!(
            casa.join("marca").is_dir(),
            "o processo nao rodou em {} — o diretorio de trabalho nao chegou",
            casa.display()
        );
        let _ = std::fs::remove_dir_all(&casa);
    }

    /// Argumento com espaço precisa de aspas; `/C` não pode ganhar aspas, senão o
    /// `cmd` deixa de reconhecer o switch.
    #[test]
    fn a_citacao_so_cita_quem_precisa() {
        assert_eq!(citar("/C").unwrap(), "/C");
        assert_eq!(citar("notepad").unwrap(), "notepad");
        assert_eq!(citar("meu arquivo.txt").unwrap(), "\"meu arquivo.txt\"");
        assert_eq!(citar("").unwrap(), "\"\"");
    }
}
