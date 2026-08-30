//! Testes do modelo hierárquico completo (fase 1).
//!
//! O gradcheck ponta a ponta é o que mais vale aqui: ele atravessa embedding →
//! encoder → pooling nos fins de patch → backbone → broadcast causal → decoder →
//! cabeça, e um erro em qualquer gather/scatter no meio aparece na hora.

use std::path::PathBuf;

use teka::backend::{Paralelo, Scalar};
use teka::learn::adam::Adam;
use teka::model::hierarchy::{Config, Teka, TekaCache};
use teka::model::patcher::{Fixo, Plano, PorClasse, PorPalavra};
use teka::rng::Rng;


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
/// Custa duas avaliacoes a mais e derruba o truncamento abaixo do piso de
/// arredondamento -- o que faz o teste medir o gradiente, e nao a aritmetica.
fn derivada_richardson<F: FnMut(usize, f64) -> f64>(aval: &mut F, i: usize, v: f64) -> f64 {
    let h = 1e-5 * v.abs().max(1.0);
    let d1 = (aval(i, v + h) - aval(i, v - h)) / (2.0 * h);
    let d2 = (aval(i, v + h / 2.0) - aval(i, v - h / 2.0)) / h;
    (4.0 * d2 - d1) / 3.0
}

/// Piso absoluto do gradcheck: abaixo disso a diferenca central nao tem
/// precisao relativa a oferecer (ver a nota sobre o passo). Um gradiente
/// realmente errado erra por um FATOR, nao por 1e-8 -- entao este piso nao
/// esconde bug nenhum.
const PISO_ABS: f64 = 1e-8;

const TOL: f64 = 1e-5;
const LN2: f64 = std::f64::consts::LN_2;

fn checar<F: FnMut(usize, f64) -> f64>(
    nome: &str,
    valores: &[f64],
    analitico: &[f64],
    mut aval: F,
    tol: f64,
) -> f64 {
    assert_eq!(valores.len(), analitico.len(), "{nome}: tamanhos diferentes");
    let mut pior = 0.0f64;
    for i in 0..valores.len() {
        let v = valores[i];
        let num = derivada_richardson(&mut aval, i, v);
        let ana = analitico[i];
        let dif = (num - ana).abs();
        let escala = num.abs().max(ana.abs()).max(1e-8);
        let rel = dif / escala;
        // Criterio combinado: passa por erro relativo OU por erro absoluto.
        // Gradientes muito pequenos comparados a perda nao tem precisao relativa
        // disponivel -- a diferenca central produz (lp-lm) da ordem de 1e-10, e o
        // f64 so garante ~1e-15 de ruido acumulado no forward. Exigir 1e-5 relativo
        // ali seria exigir precisao que a aritmetica nao tem. O piso absoluto de
        // 1e-9 continua estreitissimo para detectar bug de verdade.
        let ok = rel < tol || dif < PISO_ABS;
        if !ok {
            pior = pior.max(rel);
        }
        assert!(
            ok,
            "{nome}[{i}]: numerico={num:.10e} analitico={ana:.10e} rel={rel:.3e} dif={dif:.3e}"
        );
        pior = pior.max(if dif < PISO_ABS { 0.0 } else { rel });
    }
    pior
}

fn corpus_teste(n: usize) -> Vec<u8> {
    let base = "a teka pensa byte a byte. o patcher junta 4 ou 5 bytes num pedaco, \
                e o backbone roda uma vez por pedaco em vez de uma vez por byte. ";
    base.bytes().cycle().take(n).collect()
}

#[test]
fn gradiente_do_modelo_inteiro_confere() {
    let (seq, batch) = (9usize, 2usize);
    let mut rng = Rng::new(4242);
    let modelo = Teka::<f64>::new(Config::minusculo(), &mut rng);

    // Texto de verdade, não bytes aleatórios: o patcher por classe precisa ver
    // fronteiras reais pra o teste exercitar o caminho que o treino usa.
    let texto = b"ab 12 cd! ok 7 zz";
    let bytes: Vec<u8> = (0..seq * batch).map(|k| texto[k % texto.len()]).collect();
    let alvos: Vec<u8> = (0..seq * batch).map(|k| texto[(k + 1) % texto.len()]).collect();

    let patcher = PorPalavra { max: 4 };
    let plano = Plano::novo(&patcher, &bytes, seq, batch);
    assert!(plano.p_max > 1, "o plano precisa ter mais de um patch");
    println!(
        "\n  plano: p_max={} bytes/patch={:.2}",
        plano.p_max,
        plano.bytes_por_patch()
    );

    let est = modelo.estado_zero(batch);
    let perda = |m: &Teka<f64>| -> f64 {
        let mut c = TekaCache::new();
        m.passo(&Scalar, &bytes, &alvos, &plano, &est, &mut c, None)
    };

    let mut cache = TekaCache::new();
    let mut grad = modelo.grad();
    let l = modelo.passo(
        &Scalar,
        &bytes,
        &alvos,
        &plano,
        &est,
        &mut cache,
        Some(&mut grad),
    );
    // Modelo recém-nascido prevê ~uniforme sobre 256 símbolos = 8 bits/byte.
    println!("  perda inicial: {:.4} bits/byte (esperado ~8)", l / LN2);
    assert!(
        (l / LN2 - 8.0).abs() < 1.0,
        "modelo recem-nascido deveria prever ~uniforme (8 bits/byte), deu {}",
        l / LN2
    );

    let fatias = grad.slices();
    let tamanhos: Vec<usize> = {
        let mut m = modelo.clone();
        m.params_mut().iter().map(|s| s.len()).collect()
    };
    assert_eq!(
        tamanhos.len(),
        fatias.len(),
        "params_mut() e slices() enumeram quantidades diferentes de tensores"
    );

    let mut pior_geral = 0.0f64;
    let mut total = 0usize;
    for (gi, g) in fatias.iter().enumerate() {
        assert_eq!(g.len(), tamanhos[gi], "tensor {gi} desalinhado");
        let vals: Vec<f64> = {
            let mut m = modelo.clone();
            m.params_mut()[gi].to_vec()
        };
        let pior = checar(
            &format!("t{gi}"),
            &vals,
            g,
            |i, v| {
                let mut m = modelo.clone();
                m.params_mut()[gi][i] = v;
                perda(&m)
            },
            TOL,
        );
        pior_geral = pior_geral.max(pior);
        total += vals.len();
    }
    println!(
        "  {} tensores, {total} parametros, pior erro relativo {:.2e}",
        fatias.len(),
        pior_geral
    );
}

#[test]
fn contexto_do_patch_e_causal() {
    // A prova de que o modelo não enxerga o futuro: mudar um byte na posição `t`
    // não pode alterar a previsão de nenhuma posição < t.
    let (seq, batch) = (16usize, 1usize);
    let mut rng = Rng::new(7);
    let modelo = Teka::<f64>::new(Config::minusculo(), &mut rng);
    let patcher = PorPalavra { max: 4 };

    let bytes: Vec<u8> = b"abc def ghi jklm".to_vec();
    let alvos: Vec<u8> = b"bc def ghi jklmn".to_vec();
    let _ = PorClasse::default();
    let est = modelo.estado_zero(batch);

    let logits = |bs: &[u8]| -> Vec<f64> {
        let plano = Plano::novo(&patcher, bs, seq, batch);
        let mut c = TekaCache::new();
        modelo.passo(&Scalar, bs, &alvos, &plano, &est, &mut c, None);
        c.logits.clone()
    };

    let base = logits(&bytes);
    for t_mudado in [5usize, 9, 12] {
        let mut b2 = bytes.clone();
        b2[t_mudado] = b'Z';
        let alt = logits(&b2);
        for t in 0..t_mudado {
            for j in 0..256 {
                let k = t * 256 + j;
                assert!(
                    (base[k] - alt[k]).abs() < 1e-12,
                    "mudar o byte {t_mudado} alterou o logit da posicao {t} — vazamento de futuro"
                );
            }
        }
    }
    println!("\n  causalidade: ok (nenhum vazamento do futuro)");
}

#[test]
fn paralelo_da_o_mesmo_resultado_que_o_oraculo() {
    let (seq, batch) = (64usize, 4usize);
    let mut rng = Rng::new(31);
    let modelo = Teka::<f32>::new(Config::pequeno(), &mut rng);
    let corpus = corpus_teste(seq * batch + 1);
    let bytes: Vec<u8> = corpus[..seq * batch].to_vec();
    let alvos: Vec<u8> = corpus[1..seq * batch + 1].to_vec();
    let plano = Plano::novo(&PorPalavra::default(), &bytes, seq, batch);
    let est = modelo.estado_zero(batch);

    let mut c1 = TekaCache::new();
    let mut g1 = modelo.grad();
    let l1 = modelo.passo(&Scalar, &bytes, &alvos, &plano, &est, &mut c1, Some(&mut g1));

    let mut c2 = TekaCache::new();
    let mut g2 = modelo.grad();
    let par = Paralelo::new(6);
    let l2 = modelo.passo(&par, &bytes, &alvos, &plano, &est, &mut c2, Some(&mut g2));

    assert_eq!(l1, l2, "perdas diferentes entre backends");
    for (a, b) in g1.slices().iter().zip(g2.slices().iter()) {
        assert_eq!(a, b, "gradiente diferente entre backends");
    }
    println!("\n  paralelo == escalar, bit a bit, inclusive nos gradientes");
}

#[test]
fn o_modelo_aprende() {
    // Teste de ponta a ponta: modelo + Adam + laço. Num corpus curto e repetitivo,
    // bits/byte tem que despencar de ~8 (chute uniforme) para algo baixo. Se este
    // teste passar, a cadeia inteira está viva.
    let (seq, batch) = (128usize, 4usize);
    let mut rng = Rng::new(2026);
    let mut modelo = Teka::<f32>::new(Config::pequeno(), &mut rng);
    let corpus = corpus_teste(20_000);
    let patcher = PorPalavra::default();
    let ops = Paralelo::auto();

    let tamanhos: Vec<usize> = modelo.params_mut().iter().map(|p| p.len()).collect();
    let mut adam = Adam::novo(&tamanhos, 2e-3);
    let mut grad = modelo.grad();
    let mut cache = TekaCache::new();
    let mut est = modelo.estado_zero(batch);

    let mut primeiro = f64::NAN;
    let mut ultimo = f64::NAN;
    let passos = 120;

    for passo in 0..passos {
        let inicio = (passo * seq) % (corpus.len() - seq - 1);
        let mut bytes = vec![0u8; seq * batch];
        let mut alvos = vec![0u8; seq * batch];
        for b in 0..batch {
            let base = (inicio + b * 997) % (corpus.len() - seq - 1);
            for t in 0..seq {
                bytes[t * batch + b] = corpus[base + t];
                alvos[t * batch + b] = corpus[base + t + 1];
            }
        }
        let plano = Plano::novo(&patcher, &bytes, seq, batch);
        grad.clear();
        let perda = modelo.passo(
            &ops,
            &bytes,
            &alvos,
            &plano,
            &est,
            &mut cache,
            Some(&mut grad),
        );
        est = modelo.estados_finais(&cache, &plano);
        {
            let fatias = grad.slices();
            let mut params = modelo.params_mut();
            adam.passo(&mut params, &fatias, 1.0);
        }
        let bpb = perda / LN2;
        if passo == 0 {
            primeiro = bpb;
        }
        ultimo = bpb;
        if passo % 30 == 0 {
            println!("    passo {passo:>4}: {bpb:.4} bits/byte");
        }
    }
    println!("    passo {passos:>4}: {ultimo:.4} bits/byte");
    assert!(primeiro > 7.0, "inicio deveria ser ~8 bits/byte, foi {primeiro}");
    assert!(
        ultimo < 3.0,
        "depois de {passos} passos deveria estar bem abaixo de 3 bits/byte, ficou {ultimo}"
    );
    println!("\n  aprendeu: {primeiro:.2} → {ultimo:.2} bits/byte em {passos} passos");
}

#[test]
fn salvar_e_carregar_preserva_o_cerebro() {
    let mut rng = Rng::new(555);
    let mut modelo = Teka::<f32>::new(Config::minusculo(), &mut rng);
    let caminho: PathBuf = std::env::temp_dir().join("teka_teste_cerebro.bin");

    let n = modelo.salvar(&caminho).expect("salvar falhou");
    assert!(n > 0);
    let mut carregado = Teka::<f32>::carregar(&caminho).expect("carregar falhou");

    let a: Vec<Vec<f32>> = modelo.params_mut().iter().map(|s| s.to_vec()).collect();
    let b: Vec<Vec<f32>> = carregado.params_mut().iter().map(|s| s.to_vec()).collect();
    assert_eq!(a, b, "os pesos mudaram na ida e volta");
    assert_eq!(modelo.cfg.d_bb, carregado.cfg.d_bb);
    assert_eq!(modelo.cfg.n_bb, carregado.cfg.n_bb);
    let _ = std::fs::remove_file(&caminho);
    println!("\n  ida e volta em disco: ok ({n} bytes)");
}

#[test]
fn compressao_do_patcher() {
    // A taxa de compressão do patcher É o multiplicador de velocidade do backbone,
    // então vale medir e não só assumir. Foi medindo isto que apareceu que quebrar
    // na mudança de classe (o óbvio) rende metade de grudar o separador na palavra.
    let corpus = corpus_teste(8192);
    let (seq, batch) = (512usize, 8usize);
    let bytes: Vec<u8> = (0..seq * batch).map(|k| corpus[k % corpus.len()]).collect();

    let palavra = Plano::novo(&PorPalavra { max: 8 }, &bytes, seq, batch);
    let classe = Plano::novo(&PorClasse { max: 8 }, &bytes, seq, batch);
    let fixo = Plano::novo(&Fixo { p: 4 }, &bytes, seq, batch);
    println!(
        "\n  bytes/patch — por_palavra: {:.2}   por_classe: {:.2}   fixo(4): {:.2}",
        palavra.bytes_por_patch(),
        classe.bytes_por_patch(),
        fixo.bytes_por_patch()
    );
    assert!(
        palavra.bytes_por_patch() > 4.0,
        "por_palavra comprimindo pouco: {:.2}",
        palavra.bytes_por_patch()
    );
    assert!(
        palavra.bytes_por_patch() > classe.bytes_por_patch(),
        "por_palavra deveria comprimir mais que por_classe"
    );
}
