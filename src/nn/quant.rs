//! Quantização int8 dos pesos, **por bloco**.
//!
//! Cada bloco de [`BLOCO`] valores consecutivos ganha um fator de escala em `f32`:
//!
//! ```text
//! w[k] ≈ q[k] · escala[k / BLOCO]        q ∈ [−127, 127]
//! ```
//!
//! ## Por que por bloco, e não um fator por tensor
//!
//! Uma escala única é decidida pelo maior peso do tensor inteiro. Basta um outlier
//! para espremer todo o resto nos primeiros níveis do int8 e destruir a resolução —
//! e peso de rede treinada tem outlier com frequência.
//!
//! ## Por que por bloco, e não por coluna
//!
//! Por coluna seria melhor ainda: cada saída da camada usaria a faixa inteira. Mas o
//! formato de arquivo guarda **tensores achatados, sem forma** — só o comprimento.
//! Quantizar por coluna exigiria gravar `n_in`/`n_out` de cada tensor e manter isso
//! em sincronia com o modelo, que é justamente o tipo de acoplamento que o formato
//! evitou de propósito.
//!
//! Bloco de 64 é o meio-termo: captura variação local sem saber a forma, e o custo
//! é 4 bytes a cada 64, ou seja, 3,8x de economia em vez dos 4x teóricos.
//!
//! ## O que isto NÃO faz
//!
//! **Não acelera.** A conta continua em `f32`; os pesos são reconstruídos ao
//! carregar. O ganho é tamanho em disco.
//!
//! E vale dizer com todas as letras: num modelo de 1,6M de parâmetros, 6 MB → 1,6 MB
//! resolve um problema que ainda não existe — os 6 MB já cabiam no L3 de 16 MB. A
//! peça vale para quando o modelo crescer ou para carregar vários de uma vez.

/// Valores por bloco de escala.
pub const BLOCO: usize = 64;
/// Maior valor absoluto usado. 127 e não 128: a faixa fica simétrica.
pub const NIVEIS: f32 = 127.0;

/// Um tensor quantizado.
#[derive(Clone, Debug, PartialEq)]
pub struct Quantizado {
    pub q: Vec<i8>,
    /// Um fator a cada [`BLOCO`] valores.
    pub escala: Vec<f32>,
}

impl Quantizado {
    pub fn bytes(&self) -> usize {
        self.q.len() + self.escala.len() * 4
    }
    pub fn len(&self) -> usize {
        self.q.len()
    }
    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }
}

/// Quantiza um tensor achatado.
pub fn quantizar(w: &[f32]) -> Quantizado {
    let n_blocos = w.len().div_ceil(BLOCO);
    let mut escala = vec![1.0f32; n_blocos];
    let mut q = vec![0i8; w.len()];

    for (b, esc) in escala.iter_mut().enumerate() {
        let ini = b * BLOCO;
        let fim = (ini + BLOCO).min(w.len());
        let maior = w[ini..fim].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        // Bloco inteiramente zero existe (cabeça nascida perto de zero). Escala zero
        // faria `0/0`; deixar 1 devolve zeros, que é a resposta certa.
        *esc = if maior > 0.0 { maior / NIVEIS } else { 1.0 };
        for k in ini..fim {
            // `round` e não truncamento: truncar enviesa tudo em direção a zero, e o
            // viés se acumula ao longo de uma soma de centenas de termos.
            q[k] = (w[k] / *esc).round().clamp(-NIVEIS, NIVEIS) as i8;
        }
    }
    Quantizado { q, escala }
}

/// Reconstrói o tensor em `f32`.
pub fn desquantizar(qz: &Quantizado, saida: &mut [f32]) {
    for (k, s) in saida.iter_mut().enumerate().take(qz.q.len()) {
        let esc = qz.escala.get(k / BLOCO).copied().unwrap_or(1.0);
        *s = qz.q[k] as f32 * esc;
    }
}

/// Erro relativo da ida e volta. É a métrica que decide se vale a pena.
pub fn erro_relativo(original: &[f32], reconstruido: &[f32]) -> f64 {
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (a, b) in original.iter().zip(reconstruido) {
        num += ((*a - *b) as f64).powi(2);
        den += (*a as f64).powi(2);
    }
    if den <= 0.0 {
        return 0.0;
    }
    (num / den).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn volta(w: &[f32]) -> Vec<f32> {
        let qz = quantizar(w);
        let mut v = vec![0.0f32; w.len()];
        desquantizar(&qz, &mut v);
        v
    }

    #[test]
    fn a_ida_e_volta_erra_pouco_em_peso_realista() {
        let mut rng = Rng::new(3);
        let mut w = vec![0.0f32; 8192];
        rng.fill_normal(&mut w, 0.05);
        let e = erro_relativo(&w, &volta(&w));
        assert!(e < 0.01, "erro relativo alto demais: {e}");
    }

    #[test]
    fn um_outlier_nao_esmaga_o_bloco_vizinho() {
        // O caso que justifica blocos: um pico gigante no comeco do tensor. Com
        // escala UNICA, tudo depois dele perderia resolucao.
        let mut rng = Rng::new(4);
        let mut w = vec![0.0f32; BLOCO * 4];
        rng.fill_normal(&mut w, 0.01);
        w[0] = 1000.0; // o outlier, sozinho no bloco 0

        let v = volta(&w);
        // Erro medido SO nos blocos que nao contem o outlier.
        let e = erro_relativo(&w[BLOCO..], &v[BLOCO..]);
        assert!(e < 0.01, "o outlier vazou para os vizinhos: erro {e}");
    }

    #[test]
    fn bloco_todo_zero_nao_vira_nan() {
        let mut w = vec![0.0f32; BLOCO * 3];
        for k in BLOCO..BLOCO * 2 {
            w[k] = 0.5; // só o bloco do meio tem valor
        }
        let v = volta(&w);
        assert!(v.iter().all(|x| x.is_finite()), "apareceu NaN ou inf");
        assert_eq!(v[0], 0.0);
        assert!((v[BLOCO] - 0.5).abs() < 1e-3);
        assert_eq!(v[BLOCO * 2], 0.0);
    }

    #[test]
    fn o_arredondamento_nao_envieza_para_zero() {
        // Truncar puxaria toda a soma na direcao do zero, e o vies se acumula ao
        // longo de centenas de termos numa GEMM.
        let mut rng = Rng::new(5);
        let mut w = vec![0.0f32; 4096];
        rng.fill_normal(&mut w, 0.1);
        let v = volta(&w);
        let so: f64 = w.iter().map(|x| *x as f64).sum();
        let sv: f64 = v.iter().map(|x| *x as f64).sum();
        let desvio = (so - sv).abs() / w.len() as f64;
        assert!(desvio < 1e-4, "vies por elemento alto demais: {desvio}");
    }

    #[test]
    fn economiza_quase_quatro_vezes() {
        let w = vec![0.1f32; 384 * 768];
        let qz = quantizar(&w);
        let razao = (w.len() * 4) as f64 / qz.bytes() as f64;
        assert!(razao > 3.7, "esperava ~3,8x, deu {razao:.2}x");
        assert!(razao < 4.01, "nao pode passar do teorico: {razao:.2}x");
    }

    #[test]
    fn tensor_que_nao_e_multiplo_do_bloco_funciona() {
        // O ultimo bloco fica pela metade; sem `div_ceil` ele ficaria de fora.
        let mut rng = Rng::new(6);
        let mut w = vec![0.0f32; BLOCO * 2 + 7];
        rng.fill_normal(&mut w, 0.1);
        let v = volta(&w);
        assert_eq!(v.len(), w.len());
        assert!(erro_relativo(&w, &v) < 0.01);
        // O rabo tem de ter sido quantizado de verdade, nao zerado.
        assert!(v[BLOCO * 2 + 6] != 0.0);
    }

    #[test]
    fn tensor_vazio_nao_quebra() {
        let qz = quantizar(&[]);
        assert!(qz.is_empty());
        assert_eq!(qz.bytes(), 0);
    }
}
