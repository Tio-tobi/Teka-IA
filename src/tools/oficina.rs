//! Oficina: uma cópia viva de uma pasta, onde a Teka mexe de verdade.
//!
//! Ideia tomada do `ShadowWorkspace` do bite3.0, reescrita em `std` puro.
//!
//! ## Por que existir
//!
//! A política de [`super::seguranca`] só tinha dois estados, e os dois são ruins:
//!
//! ```text
//! Sandbox  →  não faz nada, só descreve      (não dá pra testar de verdade)
//! Real     →  faz, e já era                  (irreversível na primeira tentativa)
//! ```
//!
//! Testar exigia `--real`, e `--real` já é o modo sem volta. Não havia degrau entre
//! "fingir" e "apostar". A oficina é esse degrau: ela **executa de verdade**, mas
//! numa cópia. Depois se olha o [`Oficina::diff`], e só então [`Oficina::aplicar`]
//! ou [`Oficina::descartar`].
//!
//! Isso é estritamente melhor que pedir permissão antes, que é o que a Nyxara faz:
//! autorizar uma escrita antes de ver o resultado é decidir no escuro. Aqui a
//! pergunta vira "isto aqui está certo?" em vez de "posso tentar?".
//!
//! ## O que ela NÃO é
//!
//! Não é isolamento de segurança. Um comando arbitrário continua podendo escrever
//! fora da oficina — quem impede isso é a denylist e a raiz permitida, que
//! continuam valendo. A oficina protege contra **erro**, não contra malícia.
//!
//! Por isso ela também tem teto de tamanho: apontar para `C:\` por engano copiaria
//! o disco inteiro, e um limite grosseiro é melhor que descobrir isso pelo travamento.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Teto de arquivos copiados. Passar disso quase sempre significa que a pasta
/// apontada estava errada.
pub const MAX_ARQUIVOS: usize = 2_000;
/// Teto de bytes copiados (64 MB).
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mudanca {
    Criado { caminho: PathBuf, bytes: u64 },
    Alterado { caminho: PathBuf, antes: u64, depois: u64 },
    Removido { caminho: PathBuf, bytes: u64 },
}

impl Mudanca {
    pub fn caminho(&self) -> &Path {
        match self {
            Mudanca::Criado { caminho, .. }
            | Mudanca::Alterado { caminho, .. }
            | Mudanca::Removido { caminho, .. } => caminho,
        }
    }
}

impl std::fmt::Display for Mudanca {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mudanca::Criado { caminho, bytes } => {
                write!(f, "  + {}  ({bytes} bytes)", caminho.display())
            }
            Mudanca::Alterado { caminho, antes, depois } => {
                write!(f, "  ~ {}  ({antes} → {depois} bytes)", caminho.display())
            }
            Mudanca::Removido { caminho, bytes } => {
                write!(f, "  - {}  ({bytes} bytes)", caminho.display())
            }
        }
    }
}

#[derive(Debug)]
pub struct Oficina {
    /// A pasta de verdade, que ninguém toca até `aplicar`.
    origem: PathBuf,
    /// A cópia onde a Teka trabalha.
    raiz: PathBuf,
    aplicada: bool,
    descartada: bool,
}

impl Oficina {
    /// Copia `origem` para `destino` e devolve a oficina.
    ///
    /// `destino` tem de não existir: reaproveitar uma pasta suja misturaria mudanças
    /// de duas sessões e o diff mentiria.
    pub fn abrir(origem: impl AsRef<Path>, destino: impl AsRef<Path>) -> io::Result<Self> {
        let origem = origem.as_ref();
        let destino = destino.as_ref();
        if !origem.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("oficina precisa de uma PASTA existente: {}", origem.display()),
            ));
        }
        if destino.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} já existe; use uma pasta nova", destino.display()),
            ));
        }
        let (n, bytes) = medir(origem)?;
        if n > MAX_ARQUIVOS || bytes > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "pasta grande demais para oficina ({n} arquivos, {} MB); \
                     aponte uma subpasta",
                    bytes / 1024 / 1024
                ),
            ));
        }
        copiar_arvore(origem, destino)?;
        Ok(Self {
            origem: origem.to_path_buf(),
            raiz: destino.to_path_buf(),
            aplicada: false,
            descartada: false,
        })
    }

    /// Onde a Teka deve escrever.
    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    pub fn origem(&self) -> &Path {
        &self.origem
    }

    /// O que mudou entre a original e a cópia.
    pub fn diff(&self) -> io::Result<Vec<Mudanca>> {
        let antes = listar(&self.origem)?;
        let depois = listar(&self.raiz)?;
        let mut saida = Vec::new();

        for (rel, tam_depois) in &depois {
            match antes.iter().find(|(r, _)| r == rel) {
                None => saida.push(Mudanca::Criado {
                    caminho: rel.clone(),
                    bytes: *tam_depois,
                }),
                Some((_, tam_antes)) => {
                    // Tamanho igual não prova conteúdo igual — compara os bytes.
                    if tam_antes != tam_depois
                        || !mesmo_conteudo(&self.origem.join(rel), &self.raiz.join(rel))?
                    {
                        saida.push(Mudanca::Alterado {
                            caminho: rel.clone(),
                            antes: *tam_antes,
                            depois: *tam_depois,
                        });
                    }
                }
            }
        }
        for (rel, tam) in &antes {
            if !depois.iter().any(|(r, _)| r == rel) {
                saida.push(Mudanca::Removido {
                    caminho: rel.clone(),
                    bytes: *tam,
                });
            }
        }
        saida.sort_by(|a, b| a.caminho().cmp(b.caminho()));
        Ok(saida)
    }

    /// Integra as mudanças na pasta original, guardando um backup antes.
    ///
    /// O backup é o que torna `aplicar` reversível — sem ele isto seria só um
    /// `--real` com passo extra.
    pub fn aplicar(&mut self, backup: &Path) -> io::Result<Vec<Mudanca>> {
        if self.descartada {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "oficina já descartada",
            ));
        }
        let mudancas = self.diff()?;
        if mudancas.is_empty() {
            self.aplicada = true;
            return Ok(mudancas);
        }
        fs::create_dir_all(backup)?;
        for m in &mudancas {
            let rel = m.caminho();
            let alvo = self.origem.join(rel);
            // Backup antes de qualquer escrita: se o processo morrer no meio, o que
            // já foi tocado tem cópia.
            if alvo.exists() {
                let guardado = backup.join(rel);
                if let Some(pai) = guardado.parent() {
                    fs::create_dir_all(pai)?;
                }
                fs::copy(&alvo, &guardado)?;
            }
            match m {
                Mudanca::Removido { .. } => match fs::remove_file(&alvo) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                },
                _ => {
                    if let Some(pai) = alvo.parent() {
                        fs::create_dir_all(pai)?;
                    }
                    fs::copy(self.raiz.join(rel), &alvo)?;
                }
            }
        }
        self.aplicada = true;
        Ok(mudancas)
    }

    /// Joga a cópia fora sem tocar na original.
    pub fn descartar(mut self) -> io::Result<()> {
        if self.raiz.exists() {
            fs::remove_dir_all(&self.raiz)?;
        }
        self.descartada = true;
        Ok(())
    }

    pub fn foi_aplicada(&self) -> bool {
        self.aplicada
    }
}

// ─────────────────────────────────────────────────────────── auxiliares

fn medir(raiz: &Path) -> io::Result<(usize, u64)> {
    let (mut n, mut bytes) = (0usize, 0u64);
    percorrer(raiz, &mut |_rel, meta| {
        n += 1;
        bytes += meta.len();
        // Para cedo: contar 200.000 arquivos só para depois recusar é desperdício.
        if n > MAX_ARQUIVOS || bytes > MAX_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "grande demais"));
        }
        Ok(())
    })
    .or_else(|e| {
        if e.kind() == io::ErrorKind::InvalidInput {
            Ok(())
        } else {
            Err(e)
        }
    })?;
    Ok((n, bytes))
}

/// Caminhos relativos e tamanhos de todos os arquivos sob `raiz`.
fn listar(raiz: &Path) -> io::Result<Vec<(PathBuf, u64)>> {
    let mut saida = Vec::new();
    percorrer(raiz, &mut |rel, meta| {
        saida.push((rel.to_path_buf(), meta.len()));
        Ok(())
    })?;
    saida.sort();
    Ok(saida)
}

fn percorrer(
    raiz: &Path,
    f: &mut impl FnMut(&Path, &fs::Metadata) -> io::Result<()>,
) -> io::Result<()> {
    fn rec(
        base: &Path,
        atual: &Path,
        f: &mut impl FnMut(&Path, &fs::Metadata) -> io::Result<()>,
    ) -> io::Result<()> {
        for entrada in fs::read_dir(atual)? {
            let entrada = entrada?;
            let caminho = entrada.path();
            let meta = entrada.metadata()?;
            if meta.is_dir() {
                rec(base, &caminho, f)?;
            } else if meta.is_file() {
                let rel = caminho.strip_prefix(base).unwrap_or(&caminho);
                f(rel, &meta)?;
            }
            // Link simbólico é ignorado de propósito: seguir um daria pra sair da
            // oficina sem que o diff mostrasse nada.
        }
        Ok(())
    }
    if !raiz.exists() {
        return Ok(());
    }
    rec(raiz, raiz, f)
}

fn copiar_arvore(origem: &Path, destino: &Path) -> io::Result<()> {
    fs::create_dir_all(destino)?;
    for entrada in fs::read_dir(origem)? {
        let entrada = entrada?;
        let meta = entrada.metadata()?;
        let alvo = destino.join(entrada.file_name());
        if meta.is_dir() {
            copiar_arvore(&entrada.path(), &alvo)?;
        } else if meta.is_file() {
            fs::copy(entrada.path(), &alvo)?;
        }
    }
    Ok(())
}

fn mesmo_conteudo(a: &Path, b: &Path) -> io::Result<bool> {
    Ok(fs::read(a)? == fs::read(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pasta temporária isolada por teste.
    fn temporaria(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("teka_oficina_{nome}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn escrever(raiz: &Path, rel: &str, conteudo: &str) {
        let c = raiz.join(rel);
        if let Some(p) = c.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(c, conteudo).unwrap();
    }

    #[test]
    fn o_diff_ve_criado_alterado_e_removido() {
        let base = temporaria("diff");
        let origem = base.join("orig");
        fs::create_dir_all(&origem).unwrap();
        escrever(&origem, "fica.txt", "igual");
        escrever(&origem, "muda.txt", "antes");
        escrever(&origem, "some.txt", "vai sumir");

        let mut of = Oficina::abrir(&origem, base.join("of")).unwrap();
        escrever(of.raiz(), "muda.txt", "depois bem maior");
        escrever(of.raiz(), "novo.txt", "nasceu");
        fs::remove_file(of.raiz().join("some.txt")).unwrap();

        let d = of.diff().unwrap();
        assert_eq!(d.len(), 3, "esperava 3 mudancas, veio {d:?}");
        assert!(d.iter().any(|m| matches!(m, Mudanca::Criado { caminho, .. }
            if caminho == Path::new("novo.txt"))));
        assert!(d.iter().any(|m| matches!(m, Mudanca::Alterado { caminho, .. }
            if caminho == Path::new("muda.txt"))));
        assert!(d.iter().any(|m| matches!(m, Mudanca::Removido { caminho, .. }
            if caminho == Path::new("some.txt"))));
        // O que nao mudou nao aparece.
        assert!(!d.iter().any(|m| m.caminho() == Path::new("fica.txt")));

        of.aplicar(&base.join("bkp")).unwrap();
        assert_eq!(fs::read_to_string(origem.join("muda.txt")).unwrap(), "depois bem maior");
        assert!(origem.join("novo.txt").exists());
        assert!(!origem.join("some.txt").exists());
        // O backup permite desfazer.
        assert_eq!(fs::read_to_string(base.join("bkp/muda.txt")).unwrap(), "antes");
        assert_eq!(fs::read_to_string(base.join("bkp/some.txt")).unwrap(), "vai sumir");

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn mesmo_tamanho_com_conteudo_diferente_conta_como_alterado() {
        let base = temporaria("tamanho");
        let origem = base.join("orig");
        fs::create_dir_all(&origem).unwrap();
        escrever(&origem, "a.txt", "aaaa");

        let of = Oficina::abrir(&origem, base.join("of")).unwrap();
        escrever(of.raiz(), "a.txt", "bbbb"); // mesmo tamanho, outro conteudo

        let d = of.diff().unwrap();
        assert_eq!(d.len(), 1, "comparar so por tamanho perderia esta: {d:?}");
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn descartar_nao_toca_na_original() {
        let base = temporaria("descarte");
        let origem = base.join("orig");
        fs::create_dir_all(&origem).unwrap();
        escrever(&origem, "intacto.txt", "original");

        let of = Oficina::abrir(&origem, base.join("of")).unwrap();
        escrever(of.raiz(), "intacto.txt", "estragado");
        escrever(of.raiz(), "lixo.txt", "nem deveria existir");
        of.descartar().unwrap();

        assert_eq!(fs::read_to_string(origem.join("intacto.txt")).unwrap(), "original");
        assert!(!origem.join("lixo.txt").exists());
        assert!(!base.join("of").exists());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn recusa_destino_que_ja_existe_e_origem_que_nao_e_pasta() {
        let base = temporaria("recusa");
        let origem = base.join("orig");
        fs::create_dir_all(&origem).unwrap();
        fs::create_dir_all(base.join("ocupado")).unwrap();

        assert!(Oficina::abrir(&origem, base.join("ocupado")).is_err());
        escrever(&base, "arquivo.txt", "nao sou pasta");
        assert!(Oficina::abrir(base.join("arquivo.txt"), base.join("of2")).is_err());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn subpasta_entra_no_diff() {
        let base = temporaria("sub");
        let origem = base.join("orig");
        fs::create_dir_all(&origem).unwrap();
        escrever(&origem, "a/b/fundo.txt", "1");

        let of = Oficina::abrir(&origem, base.join("of")).unwrap();
        escrever(of.raiz(), "a/b/fundo.txt", "2");
        escrever(of.raiz(), "a/novo.txt", "3");

        let d = of.diff().unwrap();
        assert_eq!(d.len(), 2, "{d:?}");
        fs::remove_dir_all(&base).ok();
    }
}
