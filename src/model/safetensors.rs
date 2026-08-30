//! Exporta os pesos em **safetensors**, para inspeção fora do Rust.
//!
//! ## Por que existe
//!
//! O `.bin` nativo (`model::io`) guarda magia, `Config` e depois cada tensor como
//! `comprimento + f32`. É suficiente para carregar de volta aqui e **inútil para
//! qualquer outra ferramenta**: sem nome, sem forma, sem tipo declarado.
//!
//! Safetensors resolve isso com um formato que cabe em vinte linhas:
//!
//! ```text
//! [8 bytes]  N = tamanho do cabeçalho, u64 little-endian
//! [N bytes]  cabeçalho JSON
//! [resto]    os dados, crus
//! ```
//!
//! e cada entrada do JSON é
//! `"nome": {"dtype":"F32","shape":[...],"data_offsets":[inicio, fim]}`, com os
//! deslocamentos relativos ao início do bloco de dados.
//!
//! Nenhuma dependência: é JSON à mão e bytes.
//!
//! ## O que isto NÃO dá
//!
//! Safetensors é um **contêiner**, não uma definição de modelo. Quem abrir o arquivo
//! vê os tensores com nome e forma, e não tem como executar a Teka a partir dele —
//! não existe um RG-LRU hierárquico byte a byte do outro lado esperando esses pesos.
//! Serve para inspecionar, comparar, plotar distribuição, medir esparsidade. Não
//! serve para rodar.

use std::fs::File;
use std::io::{BufWriter, Result, Write};
use std::path::Path;

/// Escreve `tensores` em safetensors.
///
/// `tensores` é `(nome, forma, valores)`. A soma dos elementos de `forma` tem de
/// bater com `valores.len()` — quem chama garante, e o teste
/// `os_descritores_batem_com_os_pesos` é quem cobra.
pub fn escrever(
    caminho: &Path,
    tensores: &[(String, Vec<usize>, Vec<f32>)],
    metadados: &[(&str, String)],
) -> Result<u64> {
    // 1. Cabeçalho, com os deslocamentos calculados antes de escrever qualquer byte.
    let mut json = String::from("{");
    let mut inicio = 0usize;
    for (nome, forma, valores) in tensores {
        let fim = inicio + valores.len() * 4;
        json.push_str(&format!(
            "\"{}\":{{\"dtype\":\"F32\",\"shape\":[{}],\"data_offsets\":[{},{}]}},",
            escapar(nome),
            forma.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(","),
            inicio,
            fim
        ));
        inicio = fim;
    }
    // `__metadata__` por último e sempre presente: é onde vai o que o formato não
    // tem campo para dizer (arquitetura, contagem de parâmetros, de onde veio).
    json.push_str("\"__metadata__\":{");
    for (i, (k, v)) in metadados.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("\"{}\":\"{}\"", escapar(k), escapar(v)));
    }
    json.push_str("}}");

    // O bloco de dados começa alinhado em 8 bytes. Não é exigência do formato, mas
    // é o que as implementações esperam para poder mapear em memória sem cópia.
    while (8 + json.len()) % 8 != 0 {
        json.push(' ');
    }

    let arquivo = File::create(caminho)?;
    let mut w = BufWriter::new(arquivo);
    w.write_all(&(json.len() as u64).to_le_bytes())?;
    w.write_all(json.as_bytes())?;
    for (_, _, valores) in tensores {
        for v in valores {
            w.write_all(&v.to_le_bytes())?;
        }
    }
    w.flush()?;
    Ok(8 + json.len() as u64 + inicio as u64)
}

/// Escapa o que o JSON não aceita cru. Os nomes aqui são identificadores ASCII, mas
/// metadado é texto livre e um caminho do Windows traz `\` — que sem escapar produz
/// um arquivo que nenhum leitor abre.
fn escapar(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => r.push_str("\\\""),
            '\\' => r.push_str("\\\\"),
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            '\t' => r.push_str("\\t"),
            c if (c as u32) < 0x20 => r.push_str(&format!("\\u{:04x}", c as u32)),
            c => r.push(c),
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::agente::Agente;
    use crate::model::hierarchy::{Config, Teka};
    use crate::rng::Rng;
    use crate::tools::Registro;

    /// A ordem dos descritores tem de bater com a dos pesos, posicao por posicao.
    ///
    /// Este e o teste que torna a exportacao segura. `descritores()` e `params_mut()`
    /// sao duas listas escritas a mao que precisam andar juntas; se alguem adicionar
    /// um tensor a uma e esquecer a outra, o arquivo exportado sai com nomes
    /// deslocados — todos os tensores com o rotulo do vizinho, e nada avisando.
    #[test]
    fn os_descritores_batem_com_os_pesos() {
        for cfg in [Config::minusculo(), Config::pequeno(), Config::padrao()] {
            let mut rng = Rng::new(1);
            let mut m = Teka::<f32>::new(cfg, &mut rng);
            let desc = m.descritores();
            let pesos = m.params_mut();
            assert_eq!(
                desc.len(),
                pesos.len(),
                "descritores e pesos com tamanhos diferentes: {} contra {}",
                desc.len(),
                pesos.len()
            );
            for (i, ((nome, forma), peso)) in desc.iter().zip(pesos.iter()).enumerate() {
                let n: usize = forma.iter().product();
                assert_eq!(
                    n,
                    peso.len(),
                    "tensor {i} ({nome}): forma {forma:?} da {n}, mas o peso tem {}",
                    peso.len()
                );
            }
        }
    }

    /// O mesmo para o agente, que soma as quatro cabecas.
    #[test]
    fn os_descritores_do_agente_batem() {
        let mut rng = Rng::new(2);
        let mut ag = Agente::<f32>::novo(Config::pequeno(), Registro::padrao(), &mut rng);
        let desc = ag.descritores();
        let pesos = ag.params_mut();
        assert_eq!(desc.len(), pesos.len());
        for ((nome, forma), peso) in desc.iter().zip(pesos.iter()) {
            let n: usize = forma.iter().product();
            assert_eq!(n, peso.len(), "{nome}: forma {forma:?} contra {} valores", peso.len());
        }
    }

    /// Nome repetido apagaria um tensor em silencio: JSON com chave duplicada faz o
    /// leitor ficar com a ultima, e o arquivo teria menos tensores do que o modelo.
    #[test]
    fn nenhum_nome_se_repete() {
        let mut rng = Rng::new(3);
        let ag = Agente::<f32>::novo(Config::padrao(), Registro::padrao(), &mut rng);
        let nomes: Vec<String> = ag.descritores().into_iter().map(|(n, _)| n).collect();
        let unicos: std::collections::HashSet<&String> = nomes.iter().collect();
        assert_eq!(unicos.len(), nomes.len(), "ha nome de tensor repetido");
    }

    /// O arquivo tem de ser lido de volta: cabecalho valido, deslocamentos batendo e
    /// os valores identicos aos que entraram.
    #[test]
    fn o_arquivo_volta_igual() {
        let t = vec![
            ("a.w".to_string(), vec![2, 3], vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0]),
            ("b.g".to_string(), vec![2], vec![-1.5f32, 0.25]),
        ];
        let p = std::env::temp_dir().join("teka_st_teste.safetensors");
        let total = escrever(&p, &t, &[("origem", "teste".into())]).unwrap();

        let bruto = std::fs::read(&p).unwrap();
        assert_eq!(bruto.len() as u64, total, "tamanho declarado nao bate");

        let n = u64::from_le_bytes(bruto[..8].try_into().unwrap()) as usize;
        let cab = std::str::from_utf8(&bruto[8..8 + n]).unwrap();
        assert!(cab.contains("\"a.w\"") && cab.contains("\"shape\":[2,3]"));
        assert!(cab.contains("__metadata__"));
        assert_eq!((8 + n) % 8, 0, "os dados nao comecam alinhados em 8 bytes");

        // Os valores, relidos pelos deslocamentos do cabecalho.
        let dados = &bruto[8 + n..];
        let mut off = 0usize;
        for (_, _, vs) in &t {
            for &v in vs {
                let lido = f32::from_le_bytes(dados[off..off + 4].try_into().unwrap());
                assert_eq!(lido, v);
                off += 4;
            }
        }
        assert_eq!(off, dados.len(), "sobrou ou faltou dado");
        std::fs::remove_file(&p).ok();
    }

    /// Barra invertida em metadado — um caminho do Windows — nao pode quebrar o JSON.
    #[test]
    fn caminho_do_windows_no_metadado_nao_quebra_o_json() {
        let t = vec![("x".to_string(), vec![1], vec![0.5f32])];
        let p = std::env::temp_dir().join("teka_st_escape.safetensors");
        escrever(&p, &t, &[("origem", "C:\\Users\\User\\teka.bin".into())]).unwrap();
        let bruto = std::fs::read(&p).unwrap();
        let n = u64::from_le_bytes(bruto[..8].try_into().unwrap()) as usize;
        let cab = std::str::from_utf8(&bruto[8..8 + n]).unwrap();
        assert!(
            cab.contains(r"C:\\Users\\User\\teka.bin"),
            "a barra nao foi escapada: {cab}"
        );
        std::fs::remove_file(&p).ok();
    }
}
