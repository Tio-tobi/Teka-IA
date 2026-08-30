//! Ambiente de treino: um mundo pequeno onde a Teka pratica sozinha.
//!
//! ## Por que isto existe
//!
//! Toda a melhoria medida neste projeto veio de **exemplo escrito à mão** — e essa
//! alavanca acaba. Uma sessão inteira rende ~120 frases.
//!
//! Aqui a fonte de sinal é outra: ela **age no mundo e o mundo responde**. Uma pasta
//! com conteúdo conhecido, uma tarefa, e a verificação do que aconteceu de verdade.
//! Isso roda a noite inteira sem ninguém escrever nada.
//!
//! ## A regra que separa isto de supervisionado disfarçado
//!
//! [`verificar_em`] olha o **efeito**, não a chamada:
//!
//! ```text
//! escrever_arquivo → o arquivo contém o texto?       (lê o disco)
//! calcular         → o número bate com a conta?      (recalcula)
//! ler_arquivo      → a saída contém o que estava lá?
//! ```
//!
//! Comparar com um gabarito de ferramenta seria entropia cruzada com passos extras,
//! e ensinaria ela a imitar a opinião de quem escreveu o treino. Verificar o efeito
//! deixa **caminhos alternativos válidos** — é assim que um agente descobre coisa
//! que o autor do treino não sabia.
//!
//! ## O erro que já foi cometido aqui, e não pode voltar
//!
//! A primeira versão tinha um sorteador próprio de frases, com **26 aberturas**. Ela
//! praticou 1.200 tentativas sobre elas, e o resultado em três sementes foi:
//!
//! ```text
//! ambiente:  72% → 81%     (subiu)
//! benchmark: 108,7 → 95,0  (caiu 13,7 de 150)
//! delta:     −18, −10, −13
//! ```
//!
//! Uma sala de espelhos: o reforço empurrou a política para o que funciona **ali**.
//! É o Achado 4 outra vez — memorização em vez de generalização — só que do lado do
//! treino em vez do teste.
//!
//! Por isso as frases agora vêm de [`crate::learn::dados::gerar`], a **mesma** fonte
//! do treino supervisionado, com 292 aberturas e os pools de valor inteiros. O mundo
//! é montado para a frase, em [`dinamico::MundoDinamico`]. Manter uma segunda
//! distribuição de frases foi a causa direta do fracasso, e o mundo de conteúdo fixo
//! foi removido junto com ela.
//!
//! ## Isolamento
//!
//! O mundo é uma pasta temporária refeita a cada exemplo e apagada no fim. Nada aqui
//! toca arquivo real, e as guardas de [`crate::tools::seguranca`] continuam valendo
//! por cima — a pasta é conveniência, não é a proteção.

pub mod dinamico;
pub mod laco;

use std::fs;
use std::path::Path;

use crate::tools::{Chamada, Registro};

/// O que a tarefa espera que aconteça no mundo.
#[derive(Clone, Debug)]
pub enum Criterio {
    /// A saída da ferramenta contém este texto.
    SaidaContem(String),
    /// O arquivo (relativo à raiz) existe e contém este texto.
    ArquivoContem { caminho: String, texto: String },
    /// A saída contém o resultado numérico desta conta.
    ContaBate { esperado: f64 },
    /// A saída contém todos estes nomes.
    SaidaContemTodos(Vec<String>),
    /// Ela deveria ter recusado — a tarefa é fora de escopo.
    Recusou,
}

#[derive(Clone, Debug)]
pub struct Tarefa {
    pub pedido: String,
    pub criterio: Criterio,
    /// Só para o relatório por família. **Não entra na recompensa.**
    pub ferramenta_esperada: &'static str,
}

/// O mundo ficou como a tarefa pedia?
///
/// Não olha qual ferramenta ela escolheu: se o efeito está certo, o caminho valeu.
pub fn verificar_em(
    raiz: &Path,
    reg: &Registro,
    t: &Tarefa,
    chamada: &Chamada,
    saida: &Result<String, String>,
) -> bool {
    let nome = reg
        .ferramentas
        .get(chamada.ferramenta)
        .map(|f| f.nome.as_str())
        .unwrap_or("");
    if let Criterio::Recusou = t.criterio {
        return nome == "perguntar";
    }
    // Recusar quando havia trabalho a fazer não é acerto — senão abster viraria a
    // estratégia ótima para nunca errar, e ela aprenderia a nunca fazer nada.
    if nome == "perguntar" {
        return false;
    }
    let Ok(texto) = saida else { return false };
    match &t.criterio {
        Criterio::SaidaContem(s) => texto.contains(s.as_str()),
        Criterio::SaidaContemTodos(v) => v.iter().all(|s| texto.contains(s.as_str())),
        Criterio::ContaBate { esperado } => texto
            .split(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.'))
            .filter_map(|p| p.parse::<f64>().ok())
            .any(|v| (v - esperado).abs() < 1e-6),
        Criterio::ArquivoContem { caminho, texto: t } => fs::read_to_string(raiz.join(caminho))
            .map(|c| c.contains(t.as_str()))
            .unwrap_or(false),
        Criterio::Recusou => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chamada(reg: &Registro, nome: &str, args: &[(&str, &str)]) -> Chamada {
        Chamada {
            ferramenta: reg.indice(nome).unwrap(),
            args: args.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        }
    }

    fn temp(n: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("teka_verif_{n}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_verificacao_olha_o_efeito_e_nao_a_ferramenta() {
        // O ponto central. Se ela conseguir o resultado por outro caminho, isso e
        // acerto — senao o ambiente so ensinaria a imitar o gabarito.
        let reg = Registro::padrao();
        let raiz = temp("efeito");
        let t = Tarefa {
            pedido: "quanto e 2+2".into(),
            criterio: Criterio::ContaBate { esperado: 4.0 },
            ferramenta_esperada: "calcular",
        };
        let por_fora = chamada(&reg, "executar_comando", &[("comando", "echo 4")]);
        assert!(verificar_em(&raiz, &reg, &t, &por_fora, &Ok("4".into())));
        let certa_errada = chamada(&reg, "calcular", &[("expressao", "2+3")]);
        assert!(!verificar_em(&raiz, &reg, &t, &certa_errada, &Ok("5".into())));
        fs::remove_dir_all(&raiz).ok();
    }

    #[test]
    fn escrever_e_verificado_lendo_o_disco() {
        let reg = Registro::padrao();
        let raiz = temp("escrita");
        let t = Tarefa {
            pedido: "escreve t4242 em saida.txt".into(),
            criterio: Criterio::ArquivoContem {
                caminho: "saida.txt".into(),
                texto: "t4242".into(),
            },
            ferramenta_esperada: "escrever_arquivo",
        };
        let ch = chamada(&reg, "escrever_arquivo", &[("caminho", "saida.txt"), ("texto", "t4242")]);
        // Chamada perfeita, mas nada no disco: nao passa.
        assert!(!verificar_em(&raiz, &reg, &t, &ch, &Ok("ok".into())));
        fs::write(raiz.join("saida.txt"), "t4242").unwrap();
        assert!(verificar_em(&raiz, &reg, &t, &ch, &Ok("ok".into())));
        fs::remove_dir_all(&raiz).ok();
    }

    #[test]
    fn recusar_quando_havia_trabalho_nao_e_acerto() {
        let reg = Registro::padrao();
        let raiz = temp("recusa");
        let t = Tarefa {
            pedido: "mostra o notas.md".into(),
            criterio: Criterio::SaidaContem("zk9".into()),
            ferramenta_esperada: "ler_arquivo",
        };
        assert!(!verificar_em(
            &raiz,
            &reg,
            &t,
            &chamada(&reg, "perguntar", &[]),
            &Ok("nao entendi".into())
        ));
        fs::remove_dir_all(&raiz).ok();
    }

    #[test]
    fn fora_de_escopo_so_passa_recusando() {
        let reg = Registro::padrao();
        let raiz = temp("fora");
        let t = Tarefa {
            pedido: "toca uma musica".into(),
            criterio: Criterio::Recusou,
            ferramenta_esperada: "perguntar",
        };
        assert!(verificar_em(&raiz, &reg, &t, &chamada(&reg, "perguntar", &[]), &Ok("x".into())));
        assert!(!verificar_em(&raiz, &reg, &t, &chamada(&reg, "hora", &[]), &Ok("12:00".into())));
        fs::remove_dir_all(&raiz).ok();
    }

    #[test]
    fn erro_de_execucao_nunca_e_acerto() {
        let reg = Registro::padrao();
        let raiz = temp("erro");
        let t = Tarefa {
            pedido: "mostra o notas.md".into(),
            criterio: Criterio::SaidaContem("zk9".into()),
            ferramenta_esperada: "ler_arquivo",
        };
        let ch = chamada(&reg, "ler_arquivo", &[("caminho", "notas.md")]);
        assert!(!verificar_em(&raiz, &reg, &t, &ch, &Err("nao existe".into())));
        fs::remove_dir_all(&raiz).ok();
    }
}
