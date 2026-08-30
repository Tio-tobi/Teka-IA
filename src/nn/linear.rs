//! Camada linear: `y = x·W + b`.
//!
//! **Pesos guardados como `[n_in, n_out]`**, e essa escolha não é arbitrária — foi
//! decidida por medição na fase 0.
//!
//! Das três variantes de GEMM, duas (`nn`, `tn`) têm laço interno no formato
//! *axpy* (`c[j] += α·b[j]`), que o compilador vetoriza sozinho em AVX2. A terceira
//! (`nt`) tem laço interno de **redução** (`acc += a[p]·b[p]`), que não pode ser
//! vetorizada automaticamente porque soma de ponto flutuante não é associativa.
//! Medido neste Ryzen: ~26 GFLOP/s contra ~6 GFLOP/s. Um fator 4.
//!
//! Guardar `[n_out, n_in]` (o layout "natural", que a nila_mind usava) põe o
//! **forward** na variante lenta. Guardar `[n_in, n_out]` põe o forward na rápida e
//! empurra a lenta para o gradiente da entrada — que só existe no treino.
//!
//! Isso importa duas vezes:
//! - **inferência é só forward**, e é o que a Teka faz o dia inteiro;
//! - com `batch=1` (responder a um pedido) o GEMM vira GEMV, e aí o layout
//!   `[n_in, n_out]` mantém o laço interno contíguo sobre as saídas.
//!
//! | | fórmula | variante | vetoriza |
//! |---|---|---|---|
//! | forward | `y[r,o] = Σᵢ x[r,i]·W[i,o]` | `gemm_nn` | sim |
//! | grad. peso | `dW[i,o] += Σᵣ x[r,i]·dy[r,o]` | `gemm_tn` | sim |
//! | grad. entrada | `dx[r,i] = Σₒ dy[r,o]·W[i,o]` | `gemm_nn` (com `Wᵀ`) | sim |
//!
//! O gradiente da entrada seria a variante lenta; transpor `W` uma vez por chamada
//! custa 0,05% do trabalho e o traz para a rápida. Assim as três passam pelo
//! caminho vetorizado.
//!
//! A assimetria some quando o backend ganhar um microkernel com *packing* — aí
//! todas as variantes empacotam os operandos no layout que o kernel quer. Mas o
//! layout dos pesos fica gravado no formato do modelo, então é melhor acertar agora.

use crate::backend::Ops;
use crate::num::Float;
use crate::rng::Rng;

#[derive(Clone, Debug)]
pub struct Linear<T: Float> {
    pub n_in: usize,
    pub n_out: usize,
    /// `[n_in, n_out]` — ver a nota de layout no topo do módulo.
    pub w: Vec<T>,
    /// `[n_out]`
    pub b: Vec<T>,
}

#[derive(Clone, Debug)]
pub struct LinearGrad<T: Float> {
    pub dw: Vec<T>,
    pub db: Vec<T>,
    /// Buffer reaproveitado para `Wᵀ` no backward. Mora aqui, e não numa alocação
    /// por chamada, porque o backward é o laço quente do treino. Ver `backward`.
    wt: Vec<T>,
}

impl<T: Float> LinearGrad<T> {
    pub fn zeros(n_in: usize, n_out: usize) -> Self {
        Self {
            dw: vec![T::ZERO; n_in * n_out],
            db: vec![T::ZERO; n_out],
            wt: Vec::new(),
        }
    }

    /// Zera os gradientes. O scratch não é tocado — ele é reescrito inteiro a cada
    /// uso.
    pub fn clear(&mut self) {
        self.dw.fill(T::ZERO);
        self.db.fill(T::ZERO);
    }
}

impl<T: Float> Linear<T> {

    /// `(nome, forma)` para `w` e `b`, na ordem em que os donos os empilham.
    pub fn descritores(&self, p: &str) -> Vec<(String, Vec<usize>)> {
        vec![
            (format!("{p}.w"), vec![self.n_in, self.n_out]),
            (format!("{p}.b"), vec![self.n_out]),
        ]
    }
    /// Inicialização estilo Xavier: desvio `1/sqrt(n_in)`, viés zero. Mantém a
    /// variância da ativação estável ao empilhar camadas.
    pub fn new(n_in: usize, n_out: usize, rng: &mut Rng) -> Self {
        Self::nova_com_escala(n_in, n_out, 1.0, rng)
    }

    /// Xavier multiplicado por `escala`.
    ///
    /// Serve principalmente para a **cabeça de saída**, que quer nascer quase zerada.
    /// Com escala 1 os logits iniciais têm desvio ~1, e a perda de um modelo
    /// recém-nascido fica em ~9,4 bits/byte em vez dos 8,0 do chute uniforme — o
    /// modelo gasta os primeiros passos desfazendo o próprio ruído de inicialização.
    /// Com escala 0,05 ele nasce exatamente em `ln(256)`, e "8,0 no passo zero" vira
    /// um sinal de sanidade confiável.
    pub fn nova_com_escala(n_in: usize, n_out: usize, escala: f64, rng: &mut Rng) -> Self {
        let mut w = vec![T::ZERO; n_out * n_in];
        rng.fill_normal(&mut w, escala / (n_in as f64).sqrt());
        Self {
            n_in,
            n_out,
            w,
            b: vec![T::ZERO; n_out],
        }
    }

    pub fn grad(&self) -> LinearGrad<T> {
        LinearGrad::zeros(self.n_in, self.n_out)
    }

    pub fn n_params(&self) -> usize {
        self.w.len() + self.b.len()
    }

    /// `x: [rows, n_in]` → `y: [rows, n_out]`
    pub fn forward<O: Ops<T>>(&self, ops: &O, x: &[T], rows: usize, y: &mut [T]) {
        debug_assert_eq!(x.len(), rows * self.n_in);
        debug_assert_eq!(y.len(), rows * self.n_out);

        ops.gemm_nn(rows, self.n_in, self.n_out, x, &self.w, y, false);
        for r in 0..rows {
            let row = &mut y[r * self.n_out..(r + 1) * self.n_out];
            for o in 0..self.n_out {
                row[o] += self.b[o];
            }
        }
    }

    /// Acumula gradientes em `grad` e escreve `dx` (acumulando se `accum_dx`).
    ///
    /// `dx` é opcional porque na primeira camada da rede ninguém precisa dele.
    pub fn backward<O: Ops<T>>(
        &self,
        ops: &O,
        x: &[T],
        dy: &[T],
        rows: usize,
        dx: Option<&mut [T]>,
        accum_dx: bool,
        grad: &mut LinearGrad<T>,
    ) {
        debug_assert_eq!(x.len(), rows * self.n_in);
        debug_assert_eq!(dy.len(), rows * self.n_out);

        // dW[i,o] += Σ_r x[r,i] · dy[r,o]  — contração sobre o lote.
        ops.gemm_tn(self.n_in, rows, self.n_out, x, dy, &mut grad.dw, true);

        // db[o] += Σ_r dy[r,o]
        for r in 0..rows {
            let row = &dy[r * self.n_out..(r + 1) * self.n_out];
            for o in 0..self.n_out {
                grad.db[o] += row[o];
            }
        }

        if let Some(dx) = dx {
            debug_assert_eq!(dx.len(), rows * self.n_in);
            // dx[r,i] = Σₒ dy[r,o]·W[i,o]. Com W em `[n_in, n_out]` isso é a
            // variante `nt` — a que não vetoriza. Transpor W custa n_in·n_out
            // cópias contra rows·n_in·n_out multiplicações (para rows=2048, 0,05%
            // do trabalho) e transforma a conta na variante `nn`, ~5x mais rápida.
            //
            // Quando o backend ganhar um microkernel com packing isto vira
            // redundante — mas aí é uma linha a remover, não um layout a mudar.
            grad.wt.resize(self.n_in * self.n_out, T::ZERO);
            for i in 0..self.n_in {
                let wrow = &self.w[i * self.n_out..(i + 1) * self.n_out];
                for o in 0..self.n_out {
                    grad.wt[o * self.n_in + i] = wrow[o];
                }
            }
            ops.gemm_nn(rows, self.n_out, self.n_in, dy, &grad.wt, dx, accum_dx);
        }
    }
}
