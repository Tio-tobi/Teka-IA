//! Persistência da memória episódica.
//!
//! Formato próprio, sem dependência, no mesmo espírito do `model/io.rs`. A memória
//! precisa sobreviver a reinício — é o que separa "aprende com o tempo" de "aprende
//! até você fechar o programa".

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Result, Write};
use std::path::Path;

use super::semantica::{Camada, Conceito, Fato, Fonte, MemoriaSemantica};
use super::{Episodio, Feedback, MemoriaEpisodica, Resultado};

const MAGICO: &[u8; 8] = b"TEKAM001";
/// Marca da memoria SEMANTICA. Formato proprio e separado: os dois arquivos tem
/// ciclos de vida diferentes — episodios saem de cada turno, fatos saem do que ela
/// consultou e devem sobreviver a uma limpeza de episodios.
const MAGICO_SEM: &[u8; 8] = b"TEKAS001";

fn w_u32<W: Write>(w: &mut W, v: usize) -> Result<()> {
    w.write_all(&(v as u32).to_le_bytes())
}
fn w_u64<W: Write>(w: &mut W, v: u64) -> Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn w_txt<W: Write>(w: &mut W, s: &str) -> Result<()> {
    w_u32(w, s.len())?;
    w.write_all(s.as_bytes())
}
fn w_f32<W: Write>(w: &mut W, v: f32) -> Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn w_f64<W: Write>(w: &mut W, v: f64) -> Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn r_f32<R: Read>(r: &mut R) -> Result<f32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(f32::from_le_bytes(b))
}
fn r_f64<R: Read>(r: &mut R) -> Result<f64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(f64::from_le_bytes(b))
}
fn r_u8<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}
fn r_u32<R: Read>(r: &mut R) -> Result<usize> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b) as usize)
}
fn r_u64<R: Read>(r: &mut R) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn r_txt<R: Read>(r: &mut R) -> Result<String> {
    let n = r_u32(r)?;
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

impl MemoriaEpisodica {
    pub fn salvar(&self, caminho: &Path) -> Result<usize> {
        let mut w = BufWriter::new(File::create(caminho)?);
        w.write_all(MAGICO)?;
        w_u32(&mut w, self.episodios.len())?;
        for e in &self.episodios {
            w_txt(&mut w, &e.pedido)?;
            w_u32(&mut w, e.ferramenta)?;
            w_u32(&mut w, e.args.len())?;
            for &(slot, (i, f)) in &e.args {
                w_u32(&mut w, slot)?;
                w_u32(&mut w, i)?;
                w_u32(&mut w, f)?;
            }
            w.write_all(&[match e.resultado {
                Resultado::Executou => 0,
                Resultado::Falhou => 1,
                Resultado::NaoTentou => 2,
            }])?;
            match &e.feedback {
                Feedback::Nenhum => w.write_all(&[0])?,
                Feedback::Aprovado => w.write_all(&[1])?,
                Feedback::Corrigido { ferramenta, args } => {
                    w.write_all(&[2])?;
                    w_u32(&mut w, *ferramenta)?;
                    w_u32(&mut w, args.len())?;
                    for &(slot, (i, f)) in args {
                        w_u32(&mut w, slot)?;
                        w_u32(&mut w, i)?;
                        w_u32(&mut w, f)?;
                    }
                }
            }
            w_u64(&mut w, e.quando)?;
            w_u32(&mut w, e.peso as usize)?;
            w_u32(&mut w, e.assinatura.len())?;
            for &v in &e.assinatura {
                w.write_all(&v.to_le_bytes())?;
            }
        }
        w.flush()?;
        Ok(self.episodios.len())
    }

    pub fn carregar(caminho: &Path) -> Result<Self> {
        let mut r = BufReader::new(File::open(caminho)?);
        let mut magico = [0u8; 8];
        r.read_exact(&mut magico)?;
        if &magico != MAGICO {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "não é uma memória da Teka",
            ));
        }
        let n = r_u32(&mut r)?;
        let mut episodios = Vec::with_capacity(n);
        let mut maior = 0u64;

        for _ in 0..n {
            let pedido = r_txt(&mut r)?;
            let ferramenta = r_u32(&mut r)?;
            let n_args = r_u32(&mut r)?;
            let mut args = Vec::with_capacity(n_args);
            for _ in 0..n_args {
                let (s, i, f) = (r_u32(&mut r)?, r_u32(&mut r)?, r_u32(&mut r)?);
                args.push((s, (i, f)));
            }
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            let resultado = match b[0] {
                0 => Resultado::Executou,
                1 => Resultado::Falhou,
                _ => Resultado::NaoTentou,
            };
            r.read_exact(&mut b)?;
            let feedback = match b[0] {
                1 => Feedback::Aprovado,
                2 => {
                    let ferr = r_u32(&mut r)?;
                    let na = r_u32(&mut r)?;
                    let mut ar = Vec::with_capacity(na);
                    for _ in 0..na {
                        let (s, i, f) = (r_u32(&mut r)?, r_u32(&mut r)?, r_u32(&mut r)?);
                        ar.push((s, (i, f)));
                    }
                    Feedback::Corrigido {
                        ferramenta: ferr,
                        args: ar,
                    }
                }
                _ => Feedback::Nenhum,
            };
            let quando = r_u64(&mut r)?;
            maior = maior.max(quando);
            let peso = r_u32(&mut r)? as u32;
            let na = r_u32(&mut r)?;
            let mut assinatura = vec![0.0f32; na];
            let mut buf = vec![0u8; na * 4];
            r.read_exact(&mut buf)?;
            for (i, p) in buf.chunks_exact(4).enumerate() {
                assinatura[i] = f32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            }
            episodios.push(Episodio {
                pedido,
                ferramenta,
                args,
                resultado,
                feedback,
                quando,
                assinatura,
                peso,
            });
        }
        let mut m = MemoriaEpisodica::nova();
        m.episodios = episodios;
        // O contador precisa continuar de onde parou, senão episódios novos nascem
        // com `quando` repetido e a ordenação por recência quebra em silêncio.
        m.recomecar_contador(maior + 1);
        Ok(m)
    }
}


impl MemoriaSemantica {
    /// Grava fatos e grafo. O TEXTO e a fonte da verdade, e por isso vai sempre;
    /// a assinatura vai junto so para evitar reindexar tudo na abertura, e e
    /// descartavel — `precisa_reindexar` a refaz quando o modelo muda.
    pub fn salvar(&self, caminho: &Path) -> Result<usize> {
        let mut w = BufWriter::new(File::create(caminho)?);
        w.write_all(MAGICO_SEM)?;
        w_u64(&mut w, self.marca_atual)?;

        w_u32(&mut w, self.fatos.len())?;
        for f in &self.fatos {
            w_txt(&mut w, &f.texto)?;
            w_u32(&mut w, f.assinatura.len())?;
            for v in &f.assinatura {
                w_f32(&mut w, *v)?;
            }
            w_u64(&mut w, f.marca)?;
            w.write_all(&[f.camada.codigo()])?;
            w_u32(&mut w, f.confirmacoes as usize)?;
            w_u32(&mut w, f.acessos as usize)?;
            w_f64(&mut w, f.criado_em)?;
            w_f64(&mut w, f.ultimo_acesso_em)?;
            w_f32(&mut w, f.importancia)?;
            w.write_all(&[f.fonte.codigo()])?;
            // Ferramenta e Importado carregam de ONDE veio. Perder isso apagaria a
            // diferenca entre "o dono disse" e "a web disse", que e a diferenca que
            // `Fonte::peso` usa para nao deixar a memoria se autoconvencer.
            match &f.fonte {
                Fonte::Ferramenta(q) | Fonte::Importado(q) => w_txt(&mut w, q)?,
                _ => w_txt(&mut w, "")?,
            }
            // `superado_por` como usize+1, zero = None.
            w_u32(&mut w, f.superado_por.map(|i| i + 1).unwrap_or(0))?;
            w_u32(&mut w, f.conceitos.len())?;
            for c in &f.conceitos {
                w_u32(&mut w, *c)?;
            }
        }

        w_u32(&mut w, self.conceitos.len())?;
        for c in &self.conceitos {
            w_txt(&mut w, &c.nome)?;
            w_u32(&mut w, c.fatos.len())?;
            for i in &c.fatos {
                w_u32(&mut w, *i)?;
            }
            w_u32(&mut w, c.vizinhos.len())?;
            for i in &c.vizinhos {
                w_u32(&mut w, *i)?;
            }
        }
        w.flush()?;
        Ok(self.fatos.len())
    }

    pub fn carregar(caminho: &Path) -> Result<Self> {
        let mut r = BufReader::new(File::open(caminho)?);
        let mut magico = [0u8; 8];
        r.read_exact(&mut magico)?;
        if &magico != MAGICO_SEM {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "nao e uma memoria semantica da Teka",
            ));
        }
        let marca_atual = r_u64(&mut r)?;

        let n = r_u32(&mut r)?;
        let mut fatos = Vec::with_capacity(n);
        for _ in 0..n {
            let texto = r_txt(&mut r)?;
            let na = r_u32(&mut r)?;
            let mut assinatura = Vec::with_capacity(na);
            for _ in 0..na {
                assinatura.push(r_f32(&mut r)?);
            }
            let marca = r_u64(&mut r)?;
            let camada = Camada::de_codigo(r_u8(&mut r)?);
            let confirmacoes = r_u32(&mut r)? as u32;
            let acessos = r_u32(&mut r)? as u32;
            let criado_em = r_f64(&mut r)?;
            let ultimo_acesso_em = r_f64(&mut r)?;
            let importancia = r_f32(&mut r)?;
            let cod_fonte = r_u8(&mut r)?;
            let de_onde = r_txt(&mut r)?;
            let fonte = match cod_fonte {
                1 => Fonte::Ferramenta(de_onde),
                2 => Fonte::Importado(de_onde),
                3 => Fonte::Inferido,
                _ => Fonte::Usuario,
            };
            let sp = r_u32(&mut r)?;
            let superado_por = if sp == 0 { None } else { Some(sp - 1) };
            let nc = r_u32(&mut r)?;
            let mut conceitos = Vec::with_capacity(nc);
            for _ in 0..nc {
                conceitos.push(r_u32(&mut r)?);
            }
            fatos.push(Fato {
                texto,
                assinatura,
                marca,
                camada,
                confirmacoes,
                acessos,
                criado_em,
                ultimo_acesso_em,
                importancia,
                fonte,
                superado_por,
                conceitos,
            });
        }

        let nc = r_u32(&mut r)?;
        let mut conceitos = Vec::with_capacity(nc);
        for _ in 0..nc {
            let nome = r_txt(&mut r)?;
            let nf = r_u32(&mut r)?;
            let mut fs = Vec::with_capacity(nf);
            for _ in 0..nf {
                fs.push(r_u32(&mut r)?);
            }
            let nv = r_u32(&mut r)?;
            let mut vz = Vec::with_capacity(nv);
            for _ in 0..nv {
                vz.push(r_u32(&mut r)?);
            }
            conceitos.push(Conceito {
                nome,
                fatos: fs,
                vizinhos: vz,
            });
        }

        Ok(Self {
            fatos,
            conceitos,
            marca_atual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Feedback, Resultado};

    #[test]
    fn ida_e_volta_em_disco() {
        let mut m = MemoriaEpisodica::nova();
        m.gravar(
            "lista a pasta src",
            1,
            vec![(0, (16, 19))],
            Resultado::Executou,
            Feedback::Aprovado,
            vec![0.1, -0.2, 0.3],
        );
        m.gravar(
            "quanto de disco",
            1,
            vec![],
            Resultado::Falhou,
            Feedback::Corrigido {
                ferramenta: 6,
                args: vec![],
            },
            vec![0.5, 0.5, 0.5],
        );
        m.gravar("oi", 0, vec![], Resultado::NaoTentou, Feedback::Nenhum, vec![1.0]);

        let caminho = std::env::temp_dir().join("teka_memoria_teste.bin");
        assert_eq!(m.salvar(&caminho).unwrap(), 3);
        let volta = MemoriaEpisodica::carregar(&caminho).unwrap();

        assert_eq!(volta.len(), 3);
        assert_eq!(volta.episodios[0].pedido, "lista a pasta src");
        assert_eq!(volta.episodios[0].args, vec![(0, (16, 19))]);
        assert_eq!(volta.episodios[1].resultado, Resultado::Falhou);
        assert!(matches!(
            volta.episodios[1].feedback,
            Feedback::Corrigido { ferramenta: 6, .. }
        ));
        assert_eq!(volta.episodios[2].feedback, Feedback::Nenhum);
        assert_eq!(volta.episodios[0].assinatura, vec![0.1, -0.2, 0.3]);
        assert_eq!(volta.n_ensinaveis(), 2);
        let _ = std::fs::remove_file(&caminho);
    }

    #[test]
    fn o_contador_continua_apos_carregar() {
        let mut m = MemoriaEpisodica::nova();
        for i in 0..5 {
            m.gravar(&format!("p{i}"), 0, vec![], Resultado::Executou, Feedback::Aprovado, vec![1.0]);
        }
        let caminho = std::env::temp_dir().join("teka_memoria_contador.bin");
        m.salvar(&caminho).unwrap();
        let mut volta = MemoriaEpisodica::carregar(&caminho).unwrap();
        let id = volta.gravar("novo", 0, vec![], Resultado::Executou, Feedback::Aprovado, vec![1.0]);
        assert_eq!(id, 5, "o novo episodio nasceu com `quando` repetido");
        let _ = std::fs::remove_file(&caminho);
    }

    #[test]
    fn a_memoria_semantica_volta_igual() {
        use crate::memory::semantica::{Fonte, MemoriaSemantica};

        let mut m = MemoriaSemantica::nova(4242);
        let a = m.gravar(r"o projeto fica em C:\Projetos", 0.0);
        m.indexar(a, vec![0.1, -0.5, 0.25]);
        let b = m.gravar("fotofobia: reduzir brilho e usar tema escuro", 1.0);
        m.indexar(b, vec![0.9, 0.0, -0.3]);
        // A procedencia e o que separa "o dono disse" de "a web disse", e e o que o
        // peso da fonte usa para a memoria nao se autoconvencer. Tem de sobreviver.
        m.fatos[b].fonte = Fonte::Ferramenta("buscar_web".into());
        m.fatos[b].importancia = 0.75;
        m.ligar(b, "acessibilidade");

        let caminho = std::env::temp_dir().join("teka_semantica_teste.bin");
        assert_eq!(m.salvar(&caminho).unwrap(), 2);
        let volta = MemoriaSemantica::carregar(&caminho).unwrap();

        assert_eq!(volta.marca_atual, 4242);
        assert_eq!(volta.fatos.len(), 2);
        assert_eq!(volta.fatos[a].texto, m.fatos[a].texto);
        assert_eq!(volta.fatos[a].assinatura, m.fatos[a].assinatura);
        assert_eq!(volta.fatos[b].importancia, 0.75);
        match &volta.fatos[b].fonte {
            Fonte::Ferramenta(q) => assert_eq!(q, "buscar_web"),
            outro => panic!("a procedencia se perdeu: {outro:?}"),
        }
        assert_eq!(volta.conceitos.len(), 1);
        assert_eq!(volta.conceitos[0].nome, "acessibilidade");
        assert_eq!(volta.fatos[b].conceitos, m.fatos[b].conceitos);
        let _ = std::fs::remove_file(&caminho);
    }

    #[test]
    fn arquivo_de_outro_tipo_e_recusado() {
        // Marca errada tem de falhar alto. Ler uma memoria episodica como semantica
        // devolveria fatos de lixo em vez de erro.
        use crate::memory::semantica::MemoriaSemantica;
        let caminho = std::env::temp_dir().join("teka_marca_errada.bin");
        std::fs::write(&caminho, b"TEKAM001qualquercoisa").unwrap();
        assert!(MemoriaSemantica::carregar(&caminho).is_err());
        let _ = std::fs::remove_file(&caminho);
    }
}
