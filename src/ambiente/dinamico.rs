//! O mundo construído **para a frase**, e não a frase sorteada do mundo.
//!
//! ## O erro que isto conserta
//!
//! A primeira versão tinha um sorteador próprio com **26 aberturas**. Ela praticou
//! 1.200 tentativas sobre essas 26 frases e o resultado foi inequívoco, em três
//! sementes:
//!
//! ```text
//! ambiente:  72% → 81%     (subiu)
//! benchmark: 108,7 → 95,0  (caiu 13,7 frases)
//! delta:     −18, −10, −13
//! ```
//!
//! Era uma sala de espelhos. O reforço fez exatamente o que foi mandado: empurrou a
//! política para o que funciona **ali**. É o Achado 4 outra vez — memorização em vez
//! de generalização — só que do lado do treino em vez do teste.
//!
//! ## A inversão
//!
//! Agora a frase vem de [`crate::learn::dados::gerar`], o mesmo gerador do treino
//! supervisionado: **292 aberturas** e os pools de valor inteiros. Recebida a frase,
//! o mundo é montado para que ela seja executável e verificável — se o exemplo fala
//! de `notas.md`, `notas.md` passa a existir, com conteúdo conhecido.
//!
//! Construir um sorteador paralelo quando `gerar()` já existia foi desperdício e foi
//! a causa direta do fracasso. O que muda aqui não é a ideia do ambiente; é parar de
//! inventar uma segunda distribuição de frases.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ambiente::{Criterio, Tarefa};
use crate::learn::dados::Exemplo;
use crate::rng::Rng;
use crate::tools::{Politica, Registro};

/// Um mundo que se molda a cada exemplo.
pub struct MundoDinamico {
    raiz: PathBuf,
}

impl MundoDinamico {
    pub fn novo(raiz: impl AsRef<Path>) -> io::Result<Self> {
        let raiz = raiz.as_ref().to_path_buf();
        let _ = fs::remove_dir_all(&raiz);
        fs::create_dir_all(&raiz)?;
        Ok(Self { raiz })
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// Caminho seguro dentro do mundo, ou `None` se o exemplo aponta para fora.
    ///
    /// Um exemplo pode trazer `C:\Windows\System32`; o mundo não vai criar isso.
    /// Nesse caso a tarefa é descartada em vez de o ambiente sair da caixa.
    fn dentro(&self, rel: &str) -> Option<PathBuf> {
        let pol = Politica::real_em(self.raiz.clone());
        pol.checar_escrita(Path::new(rel)).ok()
    }

    /// Monta o mundo para este exemplo e devolve a tarefa verificável.
    ///
    /// `None` quando o exemplo não vira tarefa: argumento fora do mundo, ou
    /// ferramenta cujo efeito não dá para verificar aqui.
    pub fn preparar(
        &self,
        reg: &Registro,
        ex: &Exemplo,
        rng: &mut Rng,
    ) -> io::Result<Option<Tarefa>> {
        let nome = match reg.ferramentas.get(ex.ferramenta) {
            Some(f) => f.nome.as_str(),
            None => return Ok(None),
        };
        // Os argumentos como texto, na ordem dos slots.
        let args: Vec<String> = ex
            .args
            .iter()
            .filter_map(|&(_, (i, f))| ex.pedido.get(i..f).map(str::to_string))
            .collect();
        let marca = format!("m{}", (rng.uniform01() * 90000.0) as u32 + 10000);

        let tarefa = match nome {
            "ler_arquivo" => {
                let Some(rel) = args.first() else { return Ok(None) };
                let Some(alvo) = self.dentro(rel) else { return Ok(None) };
                if let Some(pai) = alvo.parent() {
                    fs::create_dir_all(pai)?;
                }
                fs::write(&alvo, format!("conteudo {marca} do arquivo"))?;
                Tarefa {
                    pedido: ex.pedido.clone(),
                    criterio: Criterio::SaidaContem(marca),
                    ferramenta_esperada: "ler_arquivo",
                }
            }
            "listar_pasta" => {
                let Some(rel) = args.first() else { return Ok(None) };
                let Some(alvo) = self.dentro(rel) else { return Ok(None) };
                fs::create_dir_all(&alvo)?;
                let dentro = format!("{marca}.txt");
                fs::write(alvo.join(&dentro), "x")?;
                Tarefa {
                    pedido: ex.pedido.clone(),
                    criterio: Criterio::SaidaContem(dentro),
                    ferramenta_esperada: "listar_pasta",
                }
            }
            "procurar_arquivo" => {
                let Some(alvo_nome) = args.first() else { return Ok(None) };
                // O arquivo procurado passa a existir, com o nome pedido dentro.
                let arq = format!("{alvo_nome}_{marca}.txt");
                let Some(alvo) = self.dentro(&arq) else { return Ok(None) };
                if let Some(pai) = alvo.parent() {
                    fs::create_dir_all(pai)?;
                }
                fs::write(&alvo, "x")?;
                Tarefa {
                    pedido: ex.pedido.clone(),
                    criterio: Criterio::SaidaContem(marca),
                    ferramenta_esperada: "procurar_arquivo",
                }
            }
            "escrever_arquivo" => {
                let (Some(rel), Some(texto)) = (args.first(), args.get(1)) else {
                    return Ok(None);
                };
                if self.dentro(rel).is_none() {
                    return Ok(None);
                }
                Tarefa {
                    pedido: ex.pedido.clone(),
                    criterio: Criterio::ArquivoContem {
                        caminho: rel.clone(),
                        texto: texto.clone(),
                    },
                    ferramenta_esperada: "escrever_arquivo",
                }
            }
            "calcular" => {
                let Some(expr) = args.first() else { return Ok(None) };
                // A verdade sai da própria primitiva: se ela não sabe calcular,
                // a tarefa não teria gabarito e seria descartada.
                let Ok(saida) = crate::tools::Primitiva::Calcular
                    .executar(&[("expressao".into(), expr.clone())], &Politica::default())
                else {
                    return Ok(None);
                };
                let Some(v) = primeiro_numero(&saida) else {
                    return Ok(None);
                };
                Tarefa {
                    pedido: ex.pedido.clone(),
                    criterio: Criterio::ContaBate { esperado: v },
                    ferramenta_esperada: "calcular",
                }
            }
            "hora" => Tarefa {
                pedido: ex.pedido.clone(),
                criterio: Criterio::SaidaContem(":".into()),
                ferramenta_esperada: "hora",
            },
            "perguntar" => Tarefa {
                pedido: ex.pedido.clone(),
                criterio: Criterio::Recusou,
                ferramenta_esperada: "perguntar",
            },
            // `memoria`, `disco` e `executar_comando` ficam de fora: o efeito delas
            // depende da máquina, e um critério que passa sempre não ensina nada.
            _ => return Ok(None),
        };
        Ok(Some(tarefa))
    }

    /// Limpa o mundo entre exemplos, para um não herdar arquivo do outro.
    pub fn limpar(&self) -> io::Result<()> {
        let _ = fs::remove_dir_all(&self.raiz);
        fs::create_dir_all(&self.raiz)
    }

    pub fn destruir(self) -> io::Result<()> {
        fs::remove_dir_all(&self.raiz)
    }
}

fn primeiro_numero(s: &str) -> Option<f64> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.' || c == ','))
        .filter(|p| !p.is_empty())
        .find_map(|p| p.replace(',', ".").parse::<f64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::dados::gerar;
    use crate::model::patcher::PorPalavra;

    fn temp(n: &str) -> PathBuf {
        std::env::temp_dir().join(format!("teka_din_{n}"))
    }

    #[test]
    fn a_variedade_de_frase_vem_dos_moldes_do_treino() {
        // O ponto do modulo. O sorteador antigo tinha 26 aberturas; aqui a fonte e
        // a mesma do treino supervisionado.
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut r = Rng::new(1);
        let exs = gerar(&reg, &patcher, 2000, &mut r);
        let aberturas: std::collections::HashSet<&str> = exs
            .iter()
            .filter_map(|e| e.pedido.split_whitespace().next())
            .collect();
        assert!(
            aberturas.len() > 100,
            "esperava centenas de aberturas, vi {}",
            aberturas.len()
        );
    }

    #[test]
    fn o_mundo_cria_o_arquivo_que_a_frase_menciona() {
        let m = MundoDinamico::novo(temp("cria")).unwrap();
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut r = Rng::new(2);
        let exs = gerar(&reg, &patcher, 400, &mut r);
        let i_ler = reg.indice("ler_arquivo").unwrap();

        let mut viu = false;
        for ex in exs.iter().filter(|e| e.ferramenta == i_ler).take(5) {
            m.limpar().unwrap();
            if let Some(t) = m.preparar(&reg, ex, &mut r).unwrap() {
                // O arquivo do pedido tem de existir agora.
                let arg = ex
                    .args
                    .first()
                    .and_then(|&(_, (i, f))| ex.pedido.get(i..f))
                    .unwrap();
                assert!(
                    m.raiz().join(arg).exists(),
                    "{arg:?} devia existir para {:?}",
                    t.pedido
                );
                viu = true;
            }
        }
        assert!(viu, "nenhuma tarefa de leitura foi preparada");
        m.destruir().unwrap();
    }

    #[test]
    fn argumento_fora_do_mundo_e_descartado() {
        // Um exemplo pode trazer C:\Windows. O ambiente nao sai da caixa: descarta.
        let m = MundoDinamico::novo(temp("fora")).unwrap();
        assert!(m.dentro("..\\..\\Windows\\x.dll").is_none());
        assert!(m.dentro("C:\\Windows\\System32\\x").is_none());
        assert!(m.dentro("sub\\ok.txt").is_some());
        m.destruir().unwrap();
    }

    #[test]
    fn o_gabarito_da_conta_sai_da_propria_primitiva() {
        // Sem isto o criterio seria uma segunda implementacao de aritmetica, que
        // poderia discordar da ferramenta e ensinar o errado.
        let m = MundoDinamico::novo(temp("conta")).unwrap();
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut r = Rng::new(3);
        let exs = gerar(&reg, &patcher, 400, &mut r);
        let i_calc = reg.indice("calcular").unwrap();
        for ex in exs.iter().filter(|e| e.ferramenta == i_calc).take(8) {
            if let Some(t) = m.preparar(&reg, ex, &mut r).unwrap() {
                match t.criterio {
                    Criterio::ContaBate { esperado } => assert!(esperado.is_finite()),
                    outro => panic!("criterio errado para calcular: {outro:?}"),
                }
            }
        }
        m.destruir().unwrap();
    }

    #[test]
    fn ferramenta_sem_efeito_verificavel_nao_vira_tarefa() {
        // `memoria` e `disco` passariam sempre — criterio que passa sempre nao
        // ensina nada e ainda inflaria a taxa do ambiente.
        let m = MundoDinamico::novo(temp("semver")).unwrap();
        let reg = Registro::padrao();
        let patcher = PorPalavra::default();
        let mut r = Rng::new(4);
        let exs = gerar(&reg, &patcher, 600, &mut r);
        for nome in ["memoria", "disco", "executar_comando"] {
            let i = reg.indice(nome).unwrap();
            for ex in exs.iter().filter(|e| e.ferramenta == i).take(3) {
                assert!(
                    m.preparar(&reg, ex, &mut r).unwrap().is_none(),
                    "{nome} nao devia virar tarefa"
                );
            }
        }
        m.destruir().unwrap();
    }

    #[test]
    fn limpar_nao_deixa_arquivo_de_um_exemplo_para_o_outro() {
        let m = MundoDinamico::novo(temp("limpa")).unwrap();
        fs::write(m.raiz().join("sujeira.txt"), "x").unwrap();
        m.limpar().unwrap();
        assert!(!m.raiz().join("sujeira.txt").exists());
        assert!(m.raiz().is_dir());
        m.destruir().unwrap();
    }
}
