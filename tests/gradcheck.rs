//! Checagem de gradiente por diferenças finitas.
//!
//! Este é o alicerce da fase 0. Todo o BPTT da Teka é escrito à mão; sem uma prova
//! numérica de que ele está certo, todo resultado de treino depois vira fé. Um
//! gradiente sutilmente errado não quebra o programa — ele só faz o modelo aprender
//! um pouco pior, para sempre, sem nunca dar um erro.
//!
//! Roda tudo em `f64`. Em `f32` o erro de truncamento da diferença central (~1e-3)
//! é da mesma ordem do que se quer detectar, e o teste não provaria nada.
//!
//! Diferença central: `(f(x+h) − f(x−h)) / 2h`, erro O(h²).
//!
//! ```bash
//! cargo test --release -- --nocapture
//! ```

use teka::backend::{Ops, Scalar};
use teka::nn::{Linear, RmsNorm, RmsNormCache};
use teka::rng::Rng;
use teka::ssm::{RgLru, RgLruCache};

/// Compara gradiente analítico com diferenças finitas.
///
/// `aval(i, v)` avalia a perda com o parâmetro `i` trocado por `v` — sempre sobre
/// uma cópia, o que evita qualquer conflito de empréstimo e garante que uma
/// perturbação não vaze para a próxima.
fn checar<F: FnMut(usize, f64) -> f64>(
    nome: &str,
    valores: &[f64],
    analitico: &[f64],
    mut aval: F,
    tol: f64,
) {
    assert_eq!(
        valores.len(),
        analitico.len(),
        "{nome}: gradiente com tamanho diferente do parâmetro"
    );
    let mut pior = 0.0f64;
    let mut pior_i = 0usize;

    for i in 0..valores.len() {
        let v = valores[i];
        let num = derivada_richardson(&mut aval, i, v);
        let ana = analitico[i];

        let dif = (num - ana).abs();
        let escala = num.abs().max(ana.abs()).max(1e-8);
        let rel = dif / escala;
        // Criterio combinado -- ver a nota em tests/modelo.rs.
        let ok = rel < tol || dif < PISO_ABS;
        if ok && rel > pior && dif >= PISO_ABS {
            pior = rel;
            pior_i = i;
        }
        assert!(
            ok,
            "{nome}[{i}]: numérico={num:.12e} analítico={ana:.12e} rel={rel:.3e} dif={dif:.3e}"
        );
    }
    println!(
        "    {nome:<18} ok   {:>6} params   pior erro relativo {:.2e} (idx {})",
        valores.len(),
        pior,
        pior_i
    );
}

/// Piso absoluto do gradcheck: abaixo disso a diferenca central nao tem
/// precisao relativa a oferecer (ver a nota sobre o passo). Um gradiente
/// realmente errado erra por um FATOR, nao por 1e-8 -- entao este piso nao
/// esconde bug nenhum.
const PISO_ABS: f64 = 1e-8;


/// Derivada numerica por extrapolacao de Richardson sobre diferencas centrais.
///
/// A diferenca central simples tem erro `C*h^2 + |L|*eps/h`, e o `h` otimo depende
/// da curvatura `C` -- que varia varias ordens de grandeza ENTRE parametros da
/// mesma rede. Nao existe um `h` bom pra todos: medindo, `h=1e-5` erra por
/// truncamento no embedding e `h=1e-6` erra por arredondamento nos gradientes
/// minusculos.
///
/// Richardson resolve em vez de escolher. Combinando `D(h)` e `D(h/2)`, o termo em
/// `h^2` se cancela:
///
/// ```text
/// D(h)   = f'(x) + C*h^2   + O(h^4)
/// D(h/2) = f'(x) + C*h^2/4 + O(h^4)
/// (4*D(h/2) - D(h)) / 3 = f'(x) + O(h^4)
/// ```
///
/// Custa duas avaliacoes a mais e derruba o erro de truncamento de ~1e-5 para
/// abaixo do piso de arredondamento -- o que faz o teste medir o gradiente, e nao
/// a aritmetica.
fn derivada_richardson<F: FnMut(usize, f64) -> f64>(aval: &mut F, i: usize, v: f64) -> f64 {
    let h = 1e-5 * v.abs().max(1.0);
    let d1 = (aval(i, v + h) - aval(i, v - h)) / (2.0 * h);
    let d2 = (aval(i, v + h / 2.0) - aval(i, v - h / 2.0)) / h;
    (4.0 * d2 - d1) / 3.0
}

const TOL: f64 = 1e-6;

// ---------------------------------------------------------------------------
// Linear
// ---------------------------------------------------------------------------

#[test]
fn linear_gradiente_confere() {
    println!("\n  Linear");
    let mut rng = Rng::new(11);
    let (rows, n_in, n_out) = (4usize, 5usize, 3usize);

    let lin = Linear::<f64>::new(n_in, n_out, &mut rng);
    let mut x = vec![0.0; rows * n_in];
    rng.fill_normal(&mut x, 1.0);
    let mut c = vec![0.0; rows * n_out]; // L = Σ y·c  ⇒  dy = c
    rng.fill_normal(&mut c, 1.0);

    let perda = |lin: &Linear<f64>, x: &[f64]| -> f64 {
        let mut y = vec![0.0; rows * n_out];
        lin.forward(&Scalar, x, rows, &mut y);
        y.iter().zip(&c).map(|(a, b)| a * b).sum()
    };

    let mut g = lin.grad();
    let mut dx = vec![0.0; rows * n_in];
    lin.backward(&Scalar, &x, &c, rows, Some(&mut dx), false, &mut g);

    checar(
        "w",
        &lin.w,
        &g.dw,
        |i, v| {
            let mut l2 = lin.clone();
            l2.w[i] = v;
            perda(&l2, &x)
        },
        TOL,
    );
    checar(
        "b",
        &lin.b,
        &g.db,
        |i, v| {
            let mut l2 = lin.clone();
            l2.b[i] = v;
            perda(&l2, &x)
        },
        TOL,
    );
    checar(
        "x",
        &x,
        &dx,
        |i, v| {
            let mut x2 = x.clone();
            x2[i] = v;
            perda(&lin, &x2)
        },
        TOL,
    );
}

// ---------------------------------------------------------------------------
// RMSNorm
// ---------------------------------------------------------------------------

#[test]
fn rmsnorm_gradiente_confere() {
    println!("\n  RMSNorm");
    let mut rng = Rng::new(22);
    let (rows, dim) = (4usize, 6usize);

    let mut norm = RmsNorm::<f64>::new(dim);
    rng.fill_uniform(&mut norm.g, 0.5, 1.5); // ganho != 1 pra o teste morder
    let mut x = vec![0.0; rows * dim];
    rng.fill_normal(&mut x, 1.0);
    let mut c = vec![0.0; rows * dim];
    rng.fill_normal(&mut c, 1.0);

    let perda = |norm: &RmsNorm<f64>, x: &[f64]| -> f64 {
        let mut y = vec![0.0; rows * dim];
        let mut cache = RmsNormCache::default();
        norm.forward(x, rows, &mut y, &mut cache);
        y.iter().zip(&c).map(|(a, b)| a * b).sum()
    };

    let mut y = vec![0.0; rows * dim];
    let mut cache = RmsNormCache::default();
    norm.forward(&x, rows, &mut y, &mut cache);
    let mut g = norm.grad();
    let mut dx = vec![0.0; rows * dim];
    norm.backward(&x, &c, rows, &cache, &mut dx, false, &mut g);

    checar(
        "g",
        &norm.g,
        &g.dg,
        |i, v| {
            let mut n2 = norm.clone();
            n2.g[i] = v;
            perda(&n2, &x)
        },
        TOL,
    );
    checar(
        "x",
        &x,
        &dx,
        |i, v| {
            let mut x2 = x.clone();
            x2[i] = v;
            perda(&norm, &x2)
        },
        TOL,
    );
}

// ---------------------------------------------------------------------------
// RG-LRU — o núcleo
// ---------------------------------------------------------------------------

struct Cenario {
    lru: RgLru<f64>,
    x: Vec<f64>,
    h0: Vec<f64>,
    c: Vec<f64>,
    seq: usize,
    batch: usize,
    h: usize,
}

impl Cenario {
    fn novo(seed: u64, seq: usize, batch: usize, h: usize) -> Self {
        let mut rng = Rng::new(seed);
        let lru = RgLru::<f64>::new(h, &mut rng);
        let n = seq * batch * h;
        let mut x = vec![0.0; n];
        rng.fill_normal(&mut x, 1.0);
        let mut h0 = vec![0.0; batch * h];
        rng.fill_normal(&mut h0, 0.5);
        let mut c = vec![0.0; n];
        rng.fill_normal(&mut c, 1.0);
        Self {
            lru,
            x,
            h0,
            c,
            seq,
            batch,
            h,
        }
    }

    /// L = Σ_t Σ_b Σ_c y·c — soma ponderada de TODAS as saídas, não só da última.
    /// Isso força o gradiente a atravessar a recorrência inteira; um erro no carry
    /// temporal apareceria de imediato.
    fn perda(&self, lru: &RgLru<f64>, x: &[f64], h0: &[f64]) -> f64 {
        let mut y = vec![0.0; self.seq * self.batch * self.h];
        let mut cache = RgLruCache::new();
        lru.forward(&Scalar, x, h0, self.seq, self.batch, &mut y, &mut cache);
        y.iter().zip(&self.c).map(|(a, b)| a * b).sum()
    }
}

#[test]
fn rglru_gradiente_confere() {
    println!("\n  RG-LRU (seq=5, batch=2, h=4)");
    let cen = Cenario::novo(33, 5, 2, 4);
    let (seq, batch, h) = (cen.seq, cen.batch, cen.h);
    let n = seq * batch * h;

    // --- analítico ---
    let mut y = vec![0.0; n];
    let mut cache = RgLruCache::new();
    cen.lru
        .forward(&Scalar, &cen.x, &cen.h0, seq, batch, &mut y, &mut cache);

    let mut grad = cen.lru.grad();
    let mut dx = vec![0.0; n];
    let mut dh0 = vec![0.0; batch * h];
    cen.lru.backward(
        &Scalar,
        &cen.x,
        &y,
        &cen.h0,
        &cen.c,
        None,
        seq,
        batch,
        &mut cache,
        &mut dx,
        false,
        Some(&mut dh0),
        &mut grad,
    );

    // --- numérico ---
    checar(
        "lin_r.w",
        &cen.lru.lin_r.w,
        &grad.g_r.dw,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lin_r.w[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
    checar(
        "lin_r.b",
        &cen.lru.lin_r.b,
        &grad.g_r.db,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lin_r.b[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
    checar(
        "lin_i.w",
        &cen.lru.lin_i.w,
        &grad.g_i.dw,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lin_i.w[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
    checar(
        "lin_i.b",
        &cen.lru.lin_i.b,
        &grad.g_i.db,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lin_i.b[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
    checar(
        "lambda",
        &cen.lru.lambda,
        &grad.dlambda,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lambda[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
    checar(
        "x",
        &cen.x,
        &dx,
        |i, v| {
            let mut x2 = cen.x.clone();
            x2[i] = v;
            cen.perda(&cen.lru, &x2, &cen.h0)
        },
        TOL,
    );
    checar(
        "h0",
        &cen.h0,
        &dh0,
        |i, v| {
            let mut h2 = cen.h0.clone();
            h2[i] = v;
            cen.perda(&cen.lru, &cen.x, &h2)
        },
        TOL,
    );
}

#[test]
fn rglru_gradiente_confere_em_sequencia_longa() {
    // Sequência longa é onde um BPTT errado costuma se esconder: o erro só se
    // acumula depois de muitos passos.
    println!("\n  RG-LRU (seq=40, batch=1, h=3)");
    let cen = Cenario::novo(44, 40, 1, 3);
    let (seq, batch, h) = (cen.seq, cen.batch, cen.h);

    let mut y = vec![0.0; seq * batch * h];
    let mut cache = RgLruCache::new();
    cen.lru
        .forward(&Scalar, &cen.x, &cen.h0, seq, batch, &mut y, &mut cache);

    let mut grad = cen.lru.grad();
    let mut dx = vec![0.0; seq * batch * h];
    let mut dh0 = vec![0.0; batch * h];
    cen.lru.backward(
        &Scalar,
        &cen.x,
        &y,
        &cen.h0,
        &cen.c,
        None,
        seq,
        batch,
        &mut cache,
        &mut dx,
        false,
        Some(&mut dh0),
        &mut grad,
    );

    checar(
        "lambda",
        &cen.lru.lambda,
        &grad.dlambda,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lambda[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
    checar(
        "h0",
        &cen.h0,
        &dh0,
        |i, v| {
            let mut h2 = cen.h0.clone();
            h2[i] = v;
            cen.perda(&cen.lru, &cen.x, &h2)
        },
        TOL,
    );
    checar(
        "lin_r.w",
        &cen.lru.lin_r.w,
        &grad.g_r.dw,
        |i, v| {
            let mut m = cen.lru.clone();
            m.lin_r.w[i] = v;
            cen.perda(&m, &cen.x, &cen.h0)
        },
        TOL,
    );
}

// ---------------------------------------------------------------------------
// Propriedades estruturais
// ---------------------------------------------------------------------------

#[test]
fn processar_em_pedacos_da_o_mesmo_que_de_uma_vez() {
    // O treino vai processar sequências longas em janelas, carregando o estado
    // entre elas. Se isso não for idêntico a processar tudo de uma vez, o modelo
    // treinado e o modelo em inferência são objetos diferentes.
    let cen = Cenario::novo(55, 6, 3, 5);
    let (seq, batch, h) = (cen.seq, cen.batch, cen.h);
    let passo = batch * h;

    let mut inteiro = vec![0.0; seq * batch * h];
    let mut cache = RgLruCache::new();
    cen.lru.forward(
        &Scalar,
        &cen.x,
        &cen.h0,
        seq,
        batch,
        &mut inteiro,
        &mut cache,
    );

    let corte = 4;
    let mut y1 = vec![0.0; corte * passo];
    cen.lru.forward(
        &Scalar,
        &cen.x[..corte * passo],
        &cen.h0,
        corte,
        batch,
        &mut y1,
        &mut cache,
    );

    let h_meio = y1[(corte - 1) * passo..corte * passo].to_vec();
    let resto = seq - corte;
    let mut y2 = vec![0.0; resto * passo];
    cen.lru.forward(
        &Scalar,
        &cen.x[corte * passo..],
        &h_meio,
        resto,
        batch,
        &mut y2,
        &mut cache,
    );

    for k in 0..corte * passo {
        assert!((y1[k] - inteiro[k]).abs() < 1e-14, "primeiro pedaço, k={k}");
    }
    for k in 0..resto * passo {
        assert!(
            (y2[k] - inteiro[corte * passo + k]).abs() < 1e-14,
            "segundo pedaço, k={k}"
        );
    }
    println!("\n  continuidade de estado entre janelas: ok");
}

#[test]
fn estado_nao_diverge_em_sequencia_muito_longa() {
    // `a ∈ (0,1)` por construção deveria tornar divergência impossível para
    // QUALQUER valor dos pesos. Este teste força pesos grandes de propósito.
    let mut rng = Rng::new(66);
    let h = 8;
    let mut lru = RgLru::<f64>::new(h, &mut rng);
    rng.fill_normal(&mut lru.lin_r.w, 10.0); // pesos absurdos, de propósito
    rng.fill_normal(&mut lru.lin_i.w, 10.0);

    let (seq, batch) = (4000, 1);
    let mut x = vec![0.0; seq * batch * h];
    rng.fill_normal(&mut x, 3.0);
    let h0 = lru.zero_state(batch);
    let mut y = vec![0.0; seq * batch * h];
    let mut cache = RgLruCache::new();
    lru.forward(&Scalar, &x, &h0, seq, batch, &mut y, &mut cache);

    let maior = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    assert!(maior.is_finite(), "estado virou NaN/inf");
    assert!(maior < 1e3, "estado divergiu: máx |h| = {maior}");
    println!("\n  4000 passos com pesos ~N(0,10): máx |h| = {maior:.4}  (finito, ok)");
}

#[test]
fn decaimento_nasce_no_intervalo_projetado() {
    // Λ inicializado tal que o decaimento máximo por passo (r=1) fique em
    // [0,9 , 0,999]: memória longa desde o nascimento.
    let mut rng = Rng::new(77);
    let lru = RgLru::<f64>::new(512, &mut rng);
    for (c, &lam) in lru.lambda.iter().enumerate() {
        let a_max = (1.0 / (1.0 + (-lam).exp())).powf(teka::ssm::C_DECAY);
        assert!(
            (0.899..=0.9991).contains(&a_max),
            "canal {c}: a_max = {a_max}"
        );
    }
    println!("\n  decaimento inicial dentro de [0.9, 0.999]: ok");
}

#[test]
fn gemm_escalar_serve_de_oraculo() {
    // O contrato do backend: se uma implementação rápida divergir disto, ela é a
    // errada. Aqui só se confirma que o oráculo faz o que diz.
    let a = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0]; // [2,3]
    let b = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0]; // [3,2]
    let mut c = [0.0f64; 4];
    Scalar.gemm_nn(2, 3, 2, &a, &b, &mut c, false);
    assert_eq!(c, [22.0, 28.0, 49.0, 64.0]);
}

// ---------------------------------------------------------------------------
// Blocos compostos (fase 1)
// ---------------------------------------------------------------------------

use teka::nn::{BlocoMlp, BlocoMlpCache, Embed};
use teka::ssm::{BlocoRec, BlocoRecCache};

/// Checa todos os grupos de parametros de um modulo de uma vez.
///
/// `params` e `slices` devem enumerar os tensores na MESMA ordem -- e o primeiro
/// assert deste helper existe justamente pra provar isso. Uma troca silenciosa de
/// ordem entre parametro e gradiente e o tipo de bug que passa por todo teste de
/// forward e so aparece como "o modelo aprende meio devagar".
fn checar_modulo<M, P, L>(
    nome: &str,
    modelo: &M,
    grads: &[&[f64]],
    mut params: P,
    mut perda: L,
    tol: f64,
) where
    M: Clone,
    P: for<'a> FnMut(&'a mut M) -> Vec<&'a mut [f64]>,
    L: FnMut(&M) -> f64,
{
    let tamanhos: Vec<usize> = {
        let mut m = modelo.clone();
        params(&mut m).iter().map(|s| s.len()).collect()
    };
    assert_eq!(
        tamanhos.len(),
        grads.len(),
        "{nome}: params_mut() e slices() enumeram quantidades diferentes de tensores"
    );
    for (gi, g) in grads.iter().enumerate() {
        assert_eq!(
            g.len(),
            tamanhos[gi],
            "{nome}: tensor {gi} tem tamanho {} no parametro e {} no gradiente",
            tamanhos[gi],
            g.len()
        );
        let vals: Vec<f64> = {
            let mut m = modelo.clone();
            params(&mut m)[gi].to_vec()
        };
        checar(
            &format!("{nome}#{gi}"),
            &vals,
            g,
            |i, v| {
                let mut m = modelo.clone();
                params(&mut m)[gi][i] = v;
                perda(&m)
            },
            tol,
        );
    }
}

#[test]
fn embed_gradiente_confere() {
    println!("\n  Embed");
    let mut rng = Rng::new(101);
    let dim = 5;
    let emb = Embed::<f64>::new(dim, &mut rng);
    let ids: Vec<u8> = vec![7, 200, 7, 0, 255, 7]; // byte repetido: testa o scatter-add
    let mut c = vec![0.0; ids.len() * dim];
    rng.fill_normal(&mut c, 1.0);

    let perda = |e: &Embed<f64>| -> f64 {
        let mut y = vec![0.0; ids.len() * dim];
        e.forward(&ids, &mut y);
        y.iter().zip(&c).map(|(a, b)| a * b).sum()
    };

    let mut g = emb.grad();
    emb.backward(&ids, &c, &mut g);
    checar(
        "w",
        &emb.w,
        &g.dw,
        |i, v| {
            let mut e = emb.clone();
            e.w[i] = v;
            perda(&e)
        },
        TOL,
    );
}

#[test]
fn bloco_mlp_gradiente_confere() {
    println!("\n  BlocoMlp (d=6, f=8, rows=7)");
    let mut rng = Rng::new(202);
    let (d, f, rows) = (6usize, 8usize, 7usize);
    let bloco = BlocoMlp::<f64>::new(d, f, &mut rng);
    let mut x = vec![0.0; rows * d];
    rng.fill_normal(&mut x, 1.0);
    let mut c = vec![0.0; rows * d];
    rng.fill_normal(&mut c, 1.0);

    let perda = |b: &BlocoMlp<f64>, x: &[f64]| -> f64 {
        let mut out = vec![0.0; rows * d];
        let mut cache = BlocoMlpCache::new();
        b.forward(&Scalar, x, rows, &mut out, &mut cache);
        out.iter().zip(&c).map(|(a, b)| a * b).sum()
    };

    let mut cache = BlocoMlpCache::new();
    let mut out = vec![0.0; rows * d];
    bloco.forward(&Scalar, &x, rows, &mut out, &mut cache);
    let mut g = bloco.grad();
    let mut dx = vec![0.0; rows * d];
    bloco.backward(&Scalar, &x, &c, rows, &mut cache, &mut dx, false, &mut g);

    checar_modulo(
        "mlp",
        &bloco,
        &g.slices(),
        |m: &mut BlocoMlp<f64>| m.params_mut(),
        |m: &BlocoMlp<f64>| perda(m, &x),
        TOL,
    );
    checar(
        "mlp.x",
        &x,
        &dx,
        |i, v| {
            let mut x2 = x.clone();
            x2[i] = v;
            perda(&bloco, &x2)
        },
        TOL,
    );
}

#[test]
fn bloco_recorrente_gradiente_confere() {
    println!("\n  BlocoRec (d=6, h=5, seq=4, batch=2)");
    let mut rng = Rng::new(303);
    let (d, h, seq, batch) = (6usize, 5usize, 4usize, 2usize);
    let rows = seq * batch;
    let bloco = BlocoRec::<f64>::new(d, h, &mut rng);

    let mut x = vec![0.0; rows * d];
    rng.fill_normal(&mut x, 1.0);
    let mut h0 = vec![0.0; batch * h];
    rng.fill_normal(&mut h0, 0.5);
    let mut c = vec![0.0; rows * d];
    rng.fill_normal(&mut c, 1.0);

    let perda = |b: &BlocoRec<f64>, x: &[f64], h0: &[f64]| -> f64 {
        let mut out = vec![0.0; rows * d];
        let mut cache = BlocoRecCache::new();
        b.forward(&Scalar, x, h0, seq, batch, &mut out, &mut cache);
        out.iter().zip(&c).map(|(a, b)| a * b).sum()
    };

    let mut cache = BlocoRecCache::new();
    let mut out = vec![0.0; rows * d];
    bloco.forward(&Scalar, &x, &h0, seq, batch, &mut out, &mut cache);
    let mut g = bloco.grad();
    let mut dx = vec![0.0; rows * d];
    let mut dh0 = vec![0.0; batch * h];
    bloco.backward(
        &Scalar,
        &x,
        &h0,
        &c,
        None,
        seq,
        batch,
        &mut cache,
        &mut dx,
        false,
        Some(&mut dh0),
        &mut g,
    );

    checar_modulo(
        "rec",
        &bloco,
        &g.slices(),
        |m: &mut BlocoRec<f64>| m.params_mut(),
        |m: &BlocoRec<f64>| perda(m, &x, &h0),
        TOL,
    );
    checar(
        "rec.x",
        &x,
        &dx,
        |i, v| {
            let mut x2 = x.clone();
            x2[i] = v;
            perda(&bloco, &x2, &h0)
        },
        TOL,
    );
    checar(
        "rec.h0",
        &h0,
        &dh0,
        |i, v| {
            let mut h2 = h0.clone();
            h2[i] = v;
            perda(&bloco, &x, &h2)
        },
        TOL,
    );
}
