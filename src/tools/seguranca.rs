//! Guardas de segurança para ações que tocam o mundo.
//!
//! Três camadas, e nenhuma delas confia na anterior:
//!
//! 1. **Modo.** Em [`Modo::Sandbox`] nenhuma ação com efeito colateral acontece —
//!    a ferramenta devolve a descrição do que *faria*. É assim que a Teka vai
//!    propor ferramentas novas na fase 5 sem poder quebrar nada.
//! 2. **Denylist.** Padrões destrutivos são recusados mesmo em modo real.
//! 3. **Raiz permitida.** A **escrita** fica confinada a uma pasta. Execução não:
//!    lançar processo não é operação de caminho, e essa linha dizia o contrário
//!    até 2026-09-04, quando um modelo não treinado provou a diferença abrindo
//!    dezenas de janelas. Quem controla processo é `Politica::processos`.
//! 4. **Tela virtual.** Quando há uma, o processo nasce numa área de trabalho que
//!    ninguém vê. Resolve interferência, não risco — ver `crate::tools::tela`.
//!
//! A denylist é herdada em espírito do `atuador.py` da nila_mind. Ela é uma rede,
//! não uma prova: o que garante segurança de verdade é o modo sandbox ser o padrão
//! e a raiz permitida ser estreita.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    /// Nada que altere o mundo acontece de verdade. **Padrão.**
    Sandbox,
    /// Executa, sujeito à denylist e à raiz permitida.
    Real,
}

impl Default for Modo {
    fn default() -> Self {
        Modo::Sandbox
    }
}

/// Fragmentos que nunca passam, em minúsculas. Comparados por substring — grosseiro
/// de propósito: um falso positivo custa uma recusa, um falso negativo custa um
/// disco.
const PROIBIDOS: &[&str] = &[
    "format ",
    "mkfs",
    "diskpart",
    "shutdown",
    "reg delete",
    "reg add",
    "rmdir /s",
    "rd /s",
    "del /s",
    "del /q",
    "rm -rf",
    "rm -fr",
    ":(){",          // fork bomb
    "vssadmin",
    "bcdedit",
    "cipher /w",
    "takeown",
    "icacls",
    "net user",
    "schtasks",
    "wmic process call create",
    "powershell -enc",
    "invoke-expression",
    "iex(",
    "curl ",
    "wget ",
    "certutil -urlcache",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recusa {
    Destrutivo(String),
    ForaDaRaiz(PathBuf),
    CaminhoInvalido(String),
}

impl std::fmt::Display for Recusa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Recusa::Destrutivo(p) => write!(f, "recusado: comando contém padrão destrutivo ({p:?})"),
            Recusa::ForaDaRaiz(c) => write!(f, "recusado: {} está fora da raiz permitida", c.display()),
            Recusa::CaminhoInvalido(c) => write!(f, "recusado: caminho inválido ({c})"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Politica {
    pub modo: Modo,
    /// **Escrita** fica confinada aqui. `None` = nenhuma escrita permitida.
    ///
    /// Note o que esta linha NÃO diz. A raiz confina caminho, e só. Ela não tem
    /// como confinar `abrir_programa` nem `executar_comando`, porque lançar um
    /// processo não é uma operação de caminho: `cmd /C start "" bloco_de_notas` não
    /// tem um caminho para checar. Quem controla isso é [`Politica::processos`].
    pub raiz: Option<PathBuf>,
    /// Teto de bytes lidos de um arquivo, pra um `ler_arquivo` não engolir a RAM.
    pub max_leitura: usize,
    /// Pode lançar processo — `abrir_programa` e `executar_comando`?
    ///
    /// Existe porque a raiz não dá conta. Em 2026-09-04 o laço de prática rodava
    /// com `Modo::Real` confinado a uma pasta temporária, e mesmo assim um modelo
    /// **não treinado** escolhendo por sorteio abriu dezenas de janelas do Windows
    /// na área de trabalho do John — `carinho`, `nenhuma`, `pra eu` — e podia ter
    /// rodado qualquer comando fora da lista negra, na pasta do repositório. A
    /// pasta estava confinada; o processo nunca esteve.
    ///
    /// **O padrão é `false`**, e é de propósito: quem constrói uma política com
    /// `..Default::default()` recebe o lado seguro sem precisar saber que este
    /// campo existe. Só [`Politica::real_em`] liga, porque ela representa o
    /// usuário tendo pedido `--real` de viva voz.
    pub processos: bool,
    /// Nome da área de trabalho onde os processos nascem. `None` = a do usuário.
    ///
    /// Guarda o **nome**, não o handle, e de propósito: o handle é quem mantém a
    /// área viva ([`crate::tools::tela::TelaVirtual`]) e pertence a quem abriu o
    /// laço; o nome é a única coisa que o `CreateProcessW` precisa. Assim a
    /// `Politica` continua sendo dado simples, que dá para clonar e imprimir.
    ///
    /// Isto **não é uma fronteira de segurança** — o processo roda com o mesmo
    /// token do usuário. Resolve a interferência (janela na cara de quem está
    /// usando o PC), não o risco.
    pub tela: Option<String>,
}

impl Default for Politica {
    fn default() -> Self {
        Self {
            modo: Modo::Sandbox,
            raiz: None,
            max_leitura: 256 * 1024,
            processos: false,
            tela: None,
        }
    }
}

impl Politica {
    pub fn real_em(raiz: impl Into<PathBuf>) -> Self {
        Self {
            modo: Modo::Real,
            raiz: Some(raiz.into()),
            processos: true,
            ..Default::default()
        }
    }

    /// A mesma coisa, mas sem poder lançar processo: para o laço de prática, onde
    /// quem escolhe a ferramenta é um modelo explorando por sorteio.
    pub fn real_sem_processos(raiz: impl Into<PathBuf>) -> Self {
        Self {
            processos: false,
            ..Self::real_em(raiz)
        }
    }

    /// Processos permitidos, mas nascendo numa área de trabalho separada.
    ///
    /// **Não use isto para soltar o laço de prática.** A tela esconde a janela; ela
    /// não impede um `executar_comando` fora da lista negra de agir com o token do
    /// usuário. O caminho certo, quando `abrir_programa` virar tarefa verificável em
    /// `ambiente::dinamico`, é liberar **só** `abrir_programa` — não os dois.
    pub fn real_em_tela(raiz: impl Into<PathBuf>, tela: impl Into<String>) -> Self {
        Self {
            tela: Some(tela.into()),
            ..Self::real_em(raiz)
        }
    }

    pub fn checar_comando(&self, cmd: &str) -> Result<(), Recusa> {
        let baixo = cmd.to_lowercase();
        for p in PROIBIDOS {
            if baixo.contains(p) {
                return Err(Recusa::Destrutivo((*p).into()));
            }
        }
        Ok(())
    }

    /// Resolve um caminho de **leitura** contra a raiz, quando há uma.
    ///
    /// ## O bug que isto conserta
    ///
    /// `escrever` sempre passou por [`Politica::checar_escrita`], que junta caminho
    /// relativo com a raiz. `ler`, `listar` e `procurar` usavam o caminho cru — que
    /// o sistema resolve contra o **diretório de onde o processo foi lançado**.
    ///
    /// Com `--real <pasta>`, o resultado era incoerente: escrever ia para a pasta,
    /// ler procurava em outro lugar. Medido no ambiente de treino: `ler_arquivo`
    /// deu **0 acertos em dez rodadas** com a ferramenta e o argumento corretos —
    /// só a execução falhava.
    ///
    /// O benchmark não pegava isso porque ele confere a **chamada**, não o efeito.
    ///
    /// ## Por que isto não confina a leitura
    ///
    /// Caminho absoluto continua passando. Confinar leitura seria uma mudança de
    /// política, não um conserto de bug — e a raiz sempre foi documentada como
    /// limite de **escrita e execução**. Mudar isso em silêncio seria pior que o
    /// bug original.
    pub fn resolver_leitura(&self, caminho: &Path) -> PathBuf {
        match &self.raiz {
            Some(raiz) if !caminho.is_absolute() && !caminho.as_os_str().is_empty() => {
                raiz.join(caminho)
            }
            _ => caminho.to_path_buf(),
        }
    }

    /// A raiz, ou o diretório atual quando não há uma. Para ferramentas que agem
    /// sobre "aqui" quando não recebem caminho.
    pub fn raiz_ou_atual(&self) -> PathBuf {
        self.raiz.clone().unwrap_or_else(|| PathBuf::from("."))
    }

    /// Verifica que `caminho` cai dentro da raiz permitida.
    ///
    /// Normaliza `..` textualmente **antes** de comparar. Sem isso,
    /// `raiz/../../Windows` passaria por um prefixo ingênuo.
    pub fn checar_escrita(&self, caminho: &Path) -> Result<PathBuf, Recusa> {
        let Some(raiz) = &self.raiz else {
            return Err(Recusa::ForaDaRaiz(caminho.to_path_buf()));
        };
        let absoluto = if caminho.is_absolute() {
            caminho.to_path_buf()
        } else {
            raiz.join(caminho)
        };
        let limpo = normalizar(&absoluto);
        let raiz_limpa = normalizar(raiz);
        if limpo.starts_with(&raiz_limpa) {
            Ok(limpo)
        } else {
            Err(Recusa::ForaDaRaiz(limpo))
        }
    }
}

/// Resolve `.` e `..` sem tocar no disco (`canonicalize` exige que o arquivo já
/// exista, o que não serve pra checar antes de criar).
pub fn normalizar(p: &Path) -> PathBuf {
    let mut saida = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                saida.pop();
            }
            std::path::Component::CurDir => {}
            outro => saida.push(outro.as_os_str()),
        }
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denylist_pega_variacoes_de_caixa() {
        let p = Politica::default();
        assert!(p.checar_comando("DEL /S C:\\").is_err());
        assert!(p.checar_comando("Shutdown -r").is_err());
        assert!(p.checar_comando("rm -rf /").is_err());
        assert!(p.checar_comando("dir").is_ok());
        assert!(p.checar_comando("echo ola").is_ok());
    }

    #[test]
    fn escapar_da_raiz_com_dotdot_e_recusado() {
        let p = Politica::real_em("C:\\teka\\area");
        assert!(p.checar_escrita(Path::new("nota.txt")).is_ok());
        assert!(p.checar_escrita(Path::new("sub/nota.txt")).is_ok());
        assert!(p
            .checar_escrita(Path::new("..\\..\\Windows\\System32\\x.dll"))
            .is_err());
        assert!(p.checar_escrita(Path::new("C:\\Windows\\x.dll")).is_err());
        // O caso que um prefixo ingênuo deixaria passar:
        assert!(p
            .checar_escrita(Path::new("C:\\teka\\area\\..\\..\\segredo.txt"))
            .is_err());
    }

    #[test]
    fn sem_raiz_nada_e_escrevivel() {
        let p = Politica::default();
        assert!(p.checar_escrita(Path::new("qualquer.txt")).is_err());
    }

    #[test]
    fn sandbox_e_o_padrao() {
        assert_eq!(Politica::default().modo, Modo::Sandbox);
    }
}

#[cfg(test)]
mod testes_leitura {
    use super::*;

    const RAIZ: &str = "C:\\mundo";

    #[test]
    fn caminho_relativo_de_leitura_cai_na_raiz() {
        // O bug real: com `--real <pasta>`, escrever ia para a pasta e ler
        // procurava no diretorio de onde o processo subiu. Medido no ambiente:
        // `ler_arquivo` deu 0 acertos em DEZ rodadas com a chamada correta.
        let p = Politica::real_em(RAIZ);
        assert_eq!(
            p.resolver_leitura(Path::new("notas.md")),
            PathBuf::from(RAIZ).join("notas.md")
        );
        assert_eq!(
            p.resolver_leitura(Path::new("docs\\manual.txt")),
            PathBuf::from(RAIZ).join("docs\\manual.txt")
        );
    }

    #[test]
    fn caminho_absoluto_continua_passando() {
        // Confinar leitura seria mudanca de POLITICA, nao conserto de bug: a raiz
        // sempre foi documentada como limite de escrita e execucao.
        let p = Politica::real_em(RAIZ);
        let abs = Path::new("C:\\Windows\\System32\\drivers");
        assert_eq!(p.resolver_leitura(abs), abs.to_path_buf());
    }

    #[test]
    fn sem_raiz_nada_muda() {
        let p = Politica::default();
        assert_eq!(p.resolver_leitura(Path::new("x.txt")), PathBuf::from("x.txt"));
        assert_eq!(p.raiz_ou_atual(), PathBuf::from("."));
    }

    #[test]
    fn a_escrita_continua_confinada() {
        // O conserto da leitura nao pode ter afrouxado a escrita.
        let p = Politica::real_em(RAIZ);
        assert!(p.checar_escrita(Path::new("..\\..\\Windows\\x")).is_err());
        assert!(p.checar_escrita(Path::new("ok.txt")).is_ok());
    }
}
