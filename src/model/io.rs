//! Salvar e carregar o cérebro.
//!
//! Formato próprio, deliberadamente burro: cabeçalho + a lista de tensores na
//! ordem de `params_mut`, sempre em f32 little-endian. Sem dependência, sem
//! serialização mágica, legível por 20 linhas de Python se precisar.
//!
//! A **configuração vai gravada no arquivo**. Foi um problema recorrente na
//! nila_mind: um cérebro salvo em modo esparso rodado sem `--sparse` só produzia
//! lixo, e nada no arquivo dizia isso. Aqui o arquivo se descreve sozinho.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Result, Write};
use std::path::Path;

use crate::model::hierarchy::{Config, Teka};
use crate::num::Float;
use crate::rng::Rng;

const MAGICO: &[u8; 8] = b"TEKA0001";

fn escrever_u32<W: Write>(w: &mut W, v: usize) -> Result<()> {
    w.write_all(&(v as u32).to_le_bytes())
}

fn ler_u32<R: Read>(r: &mut R) -> Result<usize> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b) as usize)
}

fn campos(c: &Config) -> [usize; 9] {
    [
        c.d_loc, c.h_loc, c.f_loc, c.n_enc, c.n_dec, c.d_bb, c.h_bb, c.f_bb, c.n_bb,
    ]
}

impl<T: Float> Teka<T> {
    /// Devolve o número de bytes escritos.
    pub fn salvar(&mut self, caminho: &Path) -> Result<usize> {
        let arquivo = File::create(caminho)?;
        let mut w = BufWriter::new(arquivo);
        w.write_all(MAGICO)?;
        for v in campos(&self.cfg) {
            escrever_u32(&mut w, v)?;
        }

        let tensores = self.params_mut();
        escrever_u32(&mut w, tensores.len())?;
        let mut total = 8 + 9 * 4 + 4;
        for t in &tensores {
            escrever_u32(&mut w, t.len())?;
            total += 4;
            for &v in t.iter() {
                w.write_all(&(v.to_f64() as f32).to_le_bytes())?;
            }
            total += t.len() * 4;
        }
        w.flush()?;
        Ok(total)
    }

    pub fn carregar(caminho: &Path) -> Result<Self> {
        let arquivo = File::open(caminho)?;
        let mut r = BufReader::new(arquivo);

        let mut magico = [0u8; 8];
        r.read_exact(&mut magico)?;
        if &magico != MAGICO {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "não é um cérebro da Teka (assinatura errada)",
            ));
        }

        let mut c = [0usize; 9];
        for v in c.iter_mut() {
            *v = ler_u32(&mut r)?;
        }
        let cfg = Config {
            d_loc: c[0],
            h_loc: c[1],
            f_loc: c[2],
            n_enc: c[3],
            n_dec: c[4],
            d_bb: c[5],
            h_bb: c[6],
            f_bb: c[7],
            n_bb: c[8],
        };

        let mut rng = Rng::new(0);
        let mut modelo = Teka::<T>::new(cfg, &mut rng);
        let n = ler_u32(&mut r)?;
        {
            let mut tensores = modelo.params_mut();
            if n != tensores.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("arquivo tem {n} tensores, o modelo espera {}", tensores.len()),
                ));
            }
            let mut buf = Vec::new();
            for t in tensores.iter_mut() {
                let len = ler_u32(&mut r)?;
                if len != t.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("tensor com {len} valores, esperado {}", t.len()),
                    ));
                }
                buf.resize(len * 4, 0u8);
                r.read_exact(&mut buf)?;
                for (i, pedaco) in buf.chunks_exact(4).enumerate() {
                    let v = f32::from_le_bytes([pedaco[0], pedaco[1], pedaco[2], pedaco[3]]);
                    t[i] = T::from_f64(v as f64);
                }
            }
        }
        Ok(modelo)
    }
}

// ---------------------------------------------------------------------------
// agente (modelo + cabecas)
// ---------------------------------------------------------------------------

const MAGICO_AG: &[u8; 8] = b"TEKAG001";
/// Mesma coisa, com os pesos em int8 por bloco.
///
/// Assinatura **diferente** de propósito. Um arquivo quantizado lido como `f32` não
/// daria erro — daria pesos absurdos e uma Teka que responde lixo. É exatamente o
/// tipo de falha silenciosa que a nila_mind teve com o modo esparso e que este
/// formato foi desenhado para impedir.
const MAGICO_AGQ: &[u8; 8] = b"TEKAQ008";

use crate::model::agente::Agente;
use crate::tools::Registro;

impl<T: Float> Agente<T> {
    /// Salva modelo e cabecas juntos.
    ///
    /// O numero de ferramentas vai gravado: carregar um agente treinado com outro
    /// registro produziria uma cabeca de intencao com o numero errado de saidas, e
    /// o erro so apareceria como "ela escolhe a ferramenta errada".
    pub fn salvar(&mut self, caminho: &Path) -> Result<usize> {
        let mut w = BufWriter::new(File::create(caminho)?);
        w.write_all(MAGICO_AG)?;
        for v in campos(&self.modelo.cfg) {
            escrever_u32(&mut w, v)?;
        }
        escrever_u32(&mut w, self.registro.n())?;

        let tensores = self.params_mut();
        escrever_u32(&mut w, tensores.len())?;
        let mut total = 8 + 10 * 4 + 4;
        for t in &tensores {
            escrever_u32(&mut w, t.len())?;
            total += 4;
            for &v in t.iter() {
                w.write_all(&(v.to_f64() as f32).to_le_bytes())?;
            }
            total += t.len() * 4;
        }
        w.flush()?;
        Ok(total)
    }

    /// Salva com os pesos em int8 por bloco. ~3,8x menor.
    ///
    /// A perda não é presumida: `teka quantizar` mede o erro de reconstrução e manda
    /// comparar no benchmark.
    pub fn salvar_q8(&mut self, caminho: &Path) -> Result<usize> {
        use crate::nn::quant;
        let mut w = BufWriter::new(File::create(caminho)?);
        w.write_all(MAGICO_AGQ)?;
        for v in campos(&self.modelo.cfg) {
            escrever_u32(&mut w, v)?;
        }
        escrever_u32(&mut w, self.registro.n())?;

        let tensores = self.params_mut();
        escrever_u32(&mut w, tensores.len())?;
        let mut total = 8 + 10 * 4 + 4;
        for t in &tensores {
            let f: Vec<f32> = t.iter().map(|v| v.to_f64() as f32).collect();
            let qz = quant::quantizar(&f);
            escrever_u32(&mut w, t.len())?;
            total += 4;
            for e in &qz.escala {
                w.write_all(&e.to_le_bytes())?;
            }
            // `i8 as u8` é reinterpretação de bits, não conversão de valor: −1 vira
            // 255 e volta a −1 na leitura.
            let cru: Vec<u8> = qz.q.iter().map(|v| *v as u8).collect();
            w.write_all(&cru)?;
            total += qz.bytes();
        }
        w.flush()?;
        Ok(total)
    }

    pub fn carregar(caminho: &Path, registro: Registro) -> Result<Self> {
        let mut r = BufReader::new(File::open(caminho)?);
        let mut magico = [0u8; 8];
        r.read_exact(&mut magico)?;
        let quantizado = &magico == MAGICO_AGQ;
        if &magico != MAGICO_AG && !quantizado {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "nao e um agente da Teka (assinatura errada)",
            ));
        }
        let mut c = [0usize; 9];
        for v in c.iter_mut() {
            *v = ler_u32(&mut r)?;
        }
        let cfg = Config {
            d_loc: c[0],
            h_loc: c[1],
            f_loc: c[2],
            n_enc: c[3],
            n_dec: c[4],
            d_bb: c[5],
            h_bb: c[6],
            f_bb: c[7],
            n_bb: c[8],
        };
        let n_ferr = ler_u32(&mut r)?;
        if n_ferr != registro.n() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "agente treinado com {n_ferr} ferramentas, o registro atual tem {}",
                    registro.n()
                ),
            ));
        }
        let mut rng = Rng::new(0);
        let mut ag = Agente::<T>::novo(cfg, registro, &mut rng);
        let n = ler_u32(&mut r)?;
        {
            let mut tensores = ag.params_mut();
            if n != tensores.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("arquivo tem {n} tensores, o agente espera {}", tensores.len()),
                ));
            }
            let mut buf = Vec::new();
            for t in tensores.iter_mut() {
                let len = ler_u32(&mut r)?;
                if len != t.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("tensor com {len} valores, esperado {}", t.len()),
                    ));
                }
                if quantizado {
                    use crate::nn::quant;
                    let n_blocos = len.div_ceil(quant::BLOCO);
                    let mut escala = vec![0.0f32; n_blocos];
                    buf.resize(n_blocos * 4, 0u8);
                    r.read_exact(&mut buf)?;
                    for (i, p) in buf.chunks_exact(4).enumerate() {
                        escala[i] = f32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                    }
                    buf.resize(len, 0u8);
                    r.read_exact(&mut buf)?;
                    let qz = quant::Quantizado {
                        // `u8 as i8` é reinterpretação de bits: 255 volta a ser −1.
                        q: buf.iter().map(|v| *v as i8).collect(),
                        escala,
                    };
                    let mut f = vec![0.0f32; len];
                    quant::desquantizar(&qz, &mut f);
                    for (i, v) in f.iter().enumerate() {
                        t[i] = T::from_f64(*v as f64);
                    }
                } else {
                    buf.resize(len * 4, 0u8);
                    r.read_exact(&mut buf)?;
                    for (i, p) in buf.chunks_exact(4).enumerate() {
                        t[i] = T::from_f64(f32::from_le_bytes([p[0], p[1], p[2], p[3]]) as f64);
                    }
                }
            }
        }
        Ok(ag)
    }
}
