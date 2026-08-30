//! Diário de execução: um efeito colateral acontece **no máximo uma vez**.
//!
//! Ideia tomada do `tool_journal.py` do bite3.0, que usa SQLite com
//! `journal_mode=WAL` e `synchronous=FULL`. Aqui é um log append-only em `std`
//! puro — a garantia que importa não vem do banco, vem da **ordem das escritas**.
//!
//! ## O problema
//!
//! ```text
//! escrever_arquivo(notas.md, "comprar pão")   ← executa
//! <processo morre antes de registrar>
//! escrever_arquivo(notas.md, "comprar pão")   ← executa DE NOVO
//! ```
//!
//! Ler duas vezes não custa nada. Escrever, apagar ou rodar um comando duas vezes
//! custa. E a Teka é um agente que aprende: repetir um pedido é o caso **comum**,
//! não o excepcional — o laço de reforço reapresenta o mesmo pedido de propósito.
//!
//! ## Como resolve
//!
//! Antes de executar, grava `iniciado` e **sincroniza com o disco**. Depois de
//! executar, grava `concluido` com o recibo. A ordem é o que dá a garantia:
//!
//! | o que está no disco | o que aconteceu | veredito |
//! |---|---|---|
//! | nada | nunca tentou | `Executar` |
//! | `iniciado` + `concluido` | terminou | `Repetir(recibo)` — devolve o recibo, não reexecuta |
//! | só `iniciado` | morreu no meio | `Incerto` — **falha fechada** |
//!
//! `Incerto` é o caso que justifica o `fsync`. Sem sincronizar, o `iniciado` podia
//! estar só no buffer do sistema quando a energia caiu, e a reexecução seguinte
//! pareceria a primeira. É melhor parar e perguntar do que escrever duas vezes.
//!
//! ## O que ele não faz
//!
//! Não autoriza e não executa nada — quem faz isso é [`super::seguranca`]. O diário
//! só responde "isto já aconteceu?". Manter as duas coisas separadas é de propósito:
//! um bug aqui não pode virar uma permissão a mais.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Veredito {
    /// Nunca foi tentado: pode executar.
    Executar,
    /// Já concluiu. O recibo é o que se devolve em vez de reexecutar.
    Repetir(String),
    /// Começou e não se sabe se terminou. Falha fechada.
    Incerto,
    /// Tentou e a **própria ferramenta** relatou erro.
    ///
    /// Distinto de [`Self::Incerto`] de propósito. `Incerto` é "começou e sumiu" —
    /// uma queda no meio, onde não se sabe o que ficou feito. Isto aqui é a
    /// ferramenta tendo rodado e dito que não deu: o efeito não está no mundo, e
    /// bloquear para sempre seria transformar uma falha comum (rede fora, arquivo
    /// inexistente, consulta sem resultado) em travamento permanente.
    ///
    /// Repetir é permitido, mas o erro anterior vai junto — quem repete merece saber
    /// que já falhou antes e por quê.
    FalhouAntes(String),
}

#[derive(Clone, Debug)]
struct Entrada {
    concluida: bool,
    /// A tentativa anterior terminou em erro relatado pela ferramenta.
    falhou: bool,
    recibo: String,
}

pub struct Diario {
    caminho: PathBuf,
    entradas: HashMap<String, Entrada>,
}

impl Diario {
    /// Abre (ou cria) o diário, relendo o que já está gravado.
    pub fn abrir(caminho: impl Into<PathBuf>) -> std::io::Result<Self> {
        let caminho = caminho.into();
        let mut entradas = HashMap::new();
        if caminho.exists() {
            let arquivo = File::open(&caminho)?;
            for linha in BufReader::new(arquivo).lines() {
                let linha = linha?;
                // Linha truncada por queda de energia é ignorada em silêncio: ela
                // não pode ser um `concluido` válido, então o pior que causa é um
                // `Incerto` — que é o lado seguro.
                // Quatro campos: estado, chave, quando, recibo. O recibo vem por
                // último e escapado, então nenhum tab dele confunde o corte.
                let mut campos = linha.splitn(4, '\t');
                let (Some(estado), Some(chave)) = (campos.next(), campos.next()) else {
                    continue;
                };
                let _quando = campos.next();
                let recibo = campos.next().unwrap_or("");
                match estado {
                    "iniciado" => {
                        entradas.entry(chave.to_string()).or_insert(Entrada {
                            concluida: false,
                            falhou: false,
                            recibo: String::new(),
                        });
                    }
                    "falhou" => {
                        entradas.insert(
                            chave.to_string(),
                            Entrada {
                                concluida: false,
                                falhou: true,
                                recibo: recibo.to_string(),
                            },
                        );
                    }
                    "concluido" => {
                        entradas.insert(
                            chave.to_string(),
                            Entrada {
                                concluida: true,
                                falhou: false,
                                recibo: desescapar(recibo),
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(Self { caminho, entradas })
    }

    /// Identidade de uma execução: ferramenta + argumentos.
    ///
    /// Usa FNV-1a de 64 bits — não é criptográfico e não precisa ser. O que se quer
    /// é que dois pedidos idênticos colidam de propósito e dois diferentes não
    /// colidam por acidente.
    pub fn chave(ferramenta: &str, args: &[(String, String)]) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mistura = |s: &str| {
            for b in s.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= 0xff;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        mistura(ferramenta);
        // Ordena para que a ordem dos argumentos não mude a identidade.
        let mut ord: Vec<&(String, String)> = args.iter().collect();
        ord.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in ord {
            mistura(k);
            mistura(v);
        }
        format!("{h:016x}")
    }

    /// O que fazer com esta execução.
    pub fn consultar(&self, chave: &str) -> Veredito {
        match self.entradas.get(chave) {
            None => Veredito::Executar,
            Some(e) if e.concluida => Veredito::Repetir(e.recibo.clone()),
            Some(e) if e.falhou => Veredito::FalhouAntes(e.recibo.clone()),
            Some(_) => Veredito::Incerto,
        }
    }

    /// Marca que a ferramenta rodou e relatou erro. Chamar quando `executar` devolve
    /// `Err` — sem isto a entrada fica `Incerto` e a operação trava para sempre.
    pub fn falhar(&mut self, chave: &str, erro: &str) -> std::io::Result<()> {
        self.gravar("falhou", chave, erro)?;
        self.entradas.insert(
            chave.to_string(),
            Entrada {
                concluida: false,
                falhou: true,
                recibo: erro.to_string(),
            },
        );
        Ok(())
    }

    /// Marca o início e **sincroniza**. Chamar ANTES de executar.
    ///
    /// O `sync_all` é o ponto inteiro desta função: sem ele o registro pode ficar no
    /// buffer do sistema, e uma queda faria a próxima tentativa parecer a primeira.
    pub fn iniciar(&mut self, chave: &str) -> std::io::Result<()> {
        self.gravar("iniciado", chave, "")?;
        self.entradas.insert(
            chave.to_string(),
            Entrada {
                concluida: false,
                falhou: false,
                recibo: String::new(),
            },
        );
        Ok(())
    }

    /// Marca a conclusão com o recibo. Chamar DEPOIS de executar.
    pub fn concluir(&mut self, chave: &str, recibo: &str) -> std::io::Result<()> {
        self.gravar("concluido", chave, recibo)?;
        self.entradas.insert(
            chave.to_string(),
            Entrada {
                concluida: true,
                falhou: false,
                recibo: recibo.to_string(),
            },
        );
        Ok(())
    }

    fn gravar(&self, estado: &str, chave: &str, recibo: &str) -> std::io::Result<()> {
        let quando = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut arquivo = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.caminho)?;
        writeln!(arquivo, "{estado}\t{chave}\t{quando}\t{}", escapar(recibo))?;
        arquivo.sync_all()
    }

    pub fn quantas(&self) -> usize {
        self.entradas.len()
    }
}

/// O recibo vai numa linha de log separada por tabulação: nem tab nem quebra podem
/// sobreviver crus.
fn escapar(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn desescapar(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            saida.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => saida.push('\t'),
            Some('n') => saida.push('\n'),
            Some('\\') => saida.push('\\'),
            Some(outro) => saida.push(outro),
            None => saida.push('\\'),
        }
    }
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporario(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("teka_diario_{nome}.log"));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn args(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn primeira_vez_executa_segunda_vez_repete_o_recibo() {
        let p = temporario("basico");
        let mut d = Diario::abrir(&p).unwrap();
        let k = Diario::chave("escrever_arquivo", &args(&[("caminho", "a.txt")]));

        assert_eq!(d.consultar(&k), Veredito::Executar);
        d.iniciar(&k).unwrap();
        d.concluir(&k, "escrevi 12 bytes").unwrap();
        assert_eq!(d.consultar(&k), Veredito::Repetir("escrevi 12 bytes".into()));

        // Reabrir do disco tem de dar o mesmo veredito.
        let d2 = Diario::abrir(&p).unwrap();
        assert_eq!(d2.consultar(&k), Veredito::Repetir("escrevi 12 bytes".into()));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn morrer_no_meio_deixa_incerto_e_nao_reexecuta() {
        let p = temporario("incerto");
        let k = Diario::chave("executar_comando", &args(&[("comando", "dir")]));
        {
            let mut d = Diario::abrir(&p).unwrap();
            d.iniciar(&k).unwrap();
            // aqui o processo "morre": nunca chamou concluir
        }
        let d = Diario::abrir(&p).unwrap();
        assert_eq!(
            d.consultar(&k),
            Veredito::Incerto,
            "iniciado sem concluido tem de falhar fechado"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn a_chave_nao_depende_da_ordem_dos_argumentos_mas_depende_do_valor() {
        let a = Diario::chave("f", &args(&[("x", "1"), ("y", "2")]));
        let b = Diario::chave("f", &args(&[("y", "2"), ("x", "1")]));
        assert_eq!(a, b, "ordem dos argumentos nao pode mudar a identidade");

        let c = Diario::chave("f", &args(&[("x", "1"), ("y", "3")]));
        assert_ne!(a, c, "valor diferente tem de dar chave diferente");
        let d = Diario::chave("g", &args(&[("x", "1"), ("y", "2")]));
        assert_ne!(a, d, "ferramenta diferente tem de dar chave diferente");

        // O separador entre campos importa: sem ele ("ab","c") e ("a","bc") colidem.
        assert_ne!(
            Diario::chave("f", &args(&[("k", "ab"), ("l", "c")])),
            Diario::chave("f", &args(&[("k", "a"), ("l", "bc")]))
        );
    }

    #[test]
    fn recibo_com_tab_e_quebra_de_linha_sobrevive() {
        let p = temporario("escape");
        let mut d = Diario::abrir(&p).unwrap();
        let k = Diario::chave("ler_arquivo", &args(&[("caminho", "x")]));
        let recibo = "linha um\nlinha\tdois\\fim";
        d.iniciar(&k).unwrap();
        d.concluir(&k, recibo).unwrap();

        let d2 = Diario::abrir(&p).unwrap();
        assert_eq!(d2.consultar(&k), Veredito::Repetir(recibo.into()));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn linha_truncada_nao_vira_concluido() {
        let p = temporario("truncado");
        let k = Diario::chave("f", &args(&[("a", "b")]));
        std::fs::write(&p, format!("iniciado\t{k}\t0\t\nconclui")).unwrap();
        let d = Diario::abrir(&p).unwrap();
        assert_eq!(d.consultar(&k), Veredito::Incerto);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn falha_nao_trava_para_sempre() {
        // O defeito que isto tranca: `concluir` so era chamado no ramo Ok, entao uma
        // ferramenta que devolvia Err deixava a entrada como `Incerto` — e `Incerto`
        // BLOQUEIA. Qualquer falha comum (rede fora, consulta sem resultado) tornava
        // aquela operacao permanentemente irrepetivel, e o unico recurso do dono era
        // editar o log a mao. Foi assim que uma busca web ficou travada de verdade.
        let caminho = std::env::temp_dir().join("teka_diario_falha.log");
        let _ = std::fs::remove_file(&caminho);
        let mut d = Diario::abrir(&caminho).unwrap();
        let chave = Diario::chave("buscar_web", &[("consulta".into(), "xyz".into())]);

        assert!(matches!(d.consultar(&chave), Veredito::Executar));
        d.iniciar(&chave).unwrap();
        assert!(matches!(d.consultar(&chave), Veredito::Incerto));

        d.falhar(&chave, "nada encontrado").unwrap();
        match d.consultar(&chave) {
            Veredito::FalhouAntes(e) => assert_eq!(e, "nada encontrado"),
            outro => panic!("depois de falhar deveria permitir repetir, veio {outro:?}"),
        }

        // E sobrevive a reabertura: o estado esta no arquivo, nao so na memoria.
        let d2 = Diario::abrir(&caminho).unwrap();
        assert!(matches!(d2.consultar(&chave), Veredito::FalhouAntes(_)));

        // Concluir depois de falhar volta a valer: a falha nao e permanente.
        let mut d3 = Diario::abrir(&caminho).unwrap();
        d3.concluir(&chave, "achei").unwrap();
        match d3.consultar(&chave) {
            Veredito::Repetir(r) => assert_eq!(r, "achei"),
            outro => panic!("concluir depois de falhar deveria valer, veio {outro:?}"),
        }
        let _ = std::fs::remove_file(&caminho);
    }
}
