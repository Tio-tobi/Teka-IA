//! Adam com recorte de gradiente por norma global.
//!
//! O recorte não é opcional num modelo recorrente. Mesmo com `a ∈ (0,1)` garantindo
//! que o *estado* não diverge, o **gradiente** ainda pode explodir quando uma
//! sequência rara alinha muitos passos na mesma direção. Um único passo com
//! gradiente gigante destrói horas de treino, e num treino contínuo como o da Teka
//! não há checkpoint anterior a que voltar. Recortar é barato e converte esse modo
//! de falha em nada.
//!
//! Os momentos ficam em `f64` independentemente do tipo dos pesos: `v` acumula
//! quadrados de gradiente, que em f32 desnormalizam quando os gradientes ficam
//! pequenos — exatamente no fim do treino, quando precisão importa mais.

use crate::num::Float;

#[derive(Clone, Debug)]
pub struct Adam {
    pub lr: f64,
    pub b1: f64,
    pub b2: f64,
    pub eps: f64,
    /// Weight decay desacoplado (AdamW). Zero por padrão.
    pub wd: f64,
    pub t: u64,
    /// Multiplicador de `lr` por tensor. Todos 1,0 por padrao.
    ///
    /// Serve para taxas discriminativas: um tronco pre-treinado precisa de uma taxa
    /// MUITO menor que cabecas recem-nascidas, senao os primeiros passos apagam o
    /// pre-treino antes que ele sirva pra alguma coisa. Foi exatamente o que
    /// aconteceu ao construir o agente sobre o cerebro treinado em portugues: com
    /// taxa unica, o pre-treino nao ajudava em nada.
    escalas: Vec<f64>,
    m: Vec<Vec<f64>>,
    v: Vec<Vec<f64>>,
}

impl Adam {
    pub fn novo(tamanhos: &[usize], lr: f64) -> Self {
        Self {
            lr,
            b1: 0.9,
            b2: 0.999,
            eps: 1e-8,
            wd: 0.0,
            t: 0,
            escalas: vec![1.0; tamanhos.len()],
            m: tamanhos.iter().map(|&n| vec![0.0; n]).collect(),
            v: tamanhos.iter().map(|&n| vec![0.0; n]).collect(),
        }
    }

    /// Define o multiplicador de taxa dos `n` primeiros tensores.
    pub fn escalar_primeiros(&mut self, n: usize, fator: f64) {
        for e in self.escalas.iter_mut().take(n) {
            *e = fator;
        }
    }

    pub fn n_estados(&self) -> usize {
        self.m.iter().map(|x| x.len()).sum::<usize>() * 2
    }

    /// Devolve a norma global do gradiente **antes** do recorte — vale a pena logar:
    /// ela subindo é o primeiro sinal de que a taxa de aprendizado está alta demais.
    pub fn passo<T: Float>(
        &mut self,
        params: &mut [&mut [T]],
        grads: &[&[T]],
        clip: f64,
    ) -> f64 {
        assert_eq!(params.len(), grads.len(), "params e grads desalinhados");
        assert_eq!(params.len(), self.m.len(), "otimizador criado com outra forma");

        let mut sq = 0.0f64;
        for g in grads {
            for &x in g.iter() {
                let x = x.to_f64();
                sq += x * x;
            }
        }
        let norma = sq.sqrt();
        let escala = if clip > 0.0 && norma > clip {
            clip / norma
        } else {
            1.0
        };

        self.t += 1;
        let bc1 = 1.0 - self.b1.powi(self.t as i32);
        let bc2 = 1.0 - self.b2.powi(self.t as i32);

        for (i, (p, g)) in params.iter_mut().zip(grads.iter()).enumerate() {
            debug_assert_eq!(p.len(), g.len());
            let lr_i = self.lr * self.escalas[i];
            let (m, v) = (&mut self.m[i], &mut self.v[i]);
            for j in 0..p.len() {
                let gj = g[j].to_f64() * escala;
                m[j] = self.b1 * m[j] + (1.0 - self.b1) * gj;
                v[j] = self.b2 * v[j] + (1.0 - self.b2) * gj * gj;
                let mh = m[j] / bc1;
                let vh = v[j] / bc2;
                let atual = p[j].to_f64();
                let novo = atual - lr_i * (mh / (vh.sqrt() + self.eps) + self.wd * atual);
                p[j] = T::from_f64(novo);
            }
        }
        norma
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanidade: Adam tem que achar o mínimo de uma quadrática simples.
    #[test]
    fn converge_numa_quadratica() {
        let alvo = [3.0f64, -1.5, 0.25];
        let mut p = vec![0.0f64; 3];
        let mut opt = Adam::novo(&[3], 0.1);
        for _ in 0..2000 {
            let g: Vec<f64> = (0..3).map(|i| 2.0 * (p[i] - alvo[i])).collect();
            let mut ps: Vec<&mut [f64]> = vec![&mut p[..]];
            let gs: Vec<&[f64]> = vec![&g[..]];
            opt.passo(&mut ps, &gs, 0.0);
        }
        for i in 0..3 {
            assert!((p[i] - alvo[i]).abs() < 1e-4, "i={i} p={} alvo={}", p[i], alvo[i]);
        }
    }

    #[test]
    fn recorte_limita_a_norma_e_preserva_a_direcao() {
        let mut p = vec![0.0f64; 2];
        let mut opt = Adam::novo(&[2], 1.0);
        // gradiente de norma 100, recorte em 1 → o passo tem que ser o mesmo que
        // o de um gradiente de norma 1 na mesma direção.
        let g = vec![60.0f64, 80.0];
        let mut ps: Vec<&mut [f64]> = vec![&mut p[..]];
        let gs: Vec<&[f64]> = vec![&g[..]];
        let norma = opt.passo(&mut ps, &gs, 1.0);
        assert!((norma - 100.0).abs() < 1e-9, "norma reportada = {norma}");
        // Adam normaliza por sqrt(v), então os dois componentes andam quase o mesmo
        // tanto; o que importa é o sinal e a finitude.
        assert!(p[0] < 0.0 && p[1] < 0.0);
        assert!(p[0].is_finite() && p[1].is_finite());
    }
}
