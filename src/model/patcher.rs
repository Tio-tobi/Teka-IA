//! O patcher — **o compactador da Teka**.
//!
//! Um tokenizer BPE é um compactador de dicionário fixo, aprendido uma vez e
//! congelado. O patcher faz o mesmo trabalho de forma dinâmica: agrupa bytes em
//! unidades maiores para o backbone rodar menos vezes.
//!
//! A taxa de compressão **é** o multiplicador de velocidade do backbone: se o patch
//! médio tem 5 bytes, o modelo grande roda 5x menos. Por isso `bytes_por_patch` é
//! uma métrica de primeira classe, medida junto com bits/byte.
//!
//! `PorPalavra` é a v0: cada patch é "espaço + palavra". Custa zero e rende ~5
//! bytes/patch em português. A v1 (entropia do encoder local, estilo BLT) entra
//! atrás deste mesmo trait, sem tocar em nada do modelo.

/// Classe grosseira de um byte. Bytes ≥ 0x80 contam como letra: em UTF-8 são
/// continuação de caractere acentuado, e quebrar no meio de um "ã" seria pior que
/// inútil.
#[inline]
pub fn classe(b: u8) -> u8 {
    match b {
        b'\t' | b'\n' | b'\r' | b' ' => 0,
        b'0'..=b'9' => 2,
        b'a'..=b'z' | b'A'..=b'Z' => 1,
        0x80..=0xFF => 1,
        _ => 3,
    }
}

pub trait Patcher: Send + Sync {
    fn nome(&self) -> &'static str;
    /// Preenche `fim` com o índice **exclusivo** do fim de cada patch.
    /// Sempre cobre `bytes` inteiro: o último valor é `bytes.len()`.
    fn fronteiras(&self, bytes: &[u8], fim: &mut Vec<usize>);
}

/// Emprestar o patcher em vez de movê-lo.
///
/// Existe para o ponto de construção poder escolher o patcher em tempo de execução
/// (`Box<dyn Patcher>`) sem obrigar as dezenas de chamadas espalhadas — quase todas já
/// genéricas em `P: Patcher + ?Sized` — a mudar de forma. Com isto, `&dyn Patcher`
/// satisfaz o próprio trait e as chamadas seguem escritas do mesmo jeito.
impl<P: Patcher + ?Sized> Patcher for &P {
    fn nome(&self) -> &'static str {
        (**self).nome()
    }
    fn fronteiras(&self, bytes: &[u8], fim: &mut Vec<usize>) {
        (**self).fronteiras(bytes, fim)
    }
}

/// **A v0 recomendada.** Quebra no *início* de cada corrida de espaço em branco,
/// com teto de `max`. Cada patch fica sendo "espaço + palavra".
///
/// Esta regra nasceu de uma medição. A versão óbvia — quebrar sempre que a classe
/// do byte muda ([`PorClasse`]) — dá patches curtíssimos: o espaço vira um patch
/// só dele, a palavra vira outro, e a média cai para ~2,7 bytes/patch em português.
/// Como a taxa de compressão *é* o multiplicador de velocidade do backbone, isso
/// jogava fora metade do ganho da hierarquia.
///
/// Grudando o separador na palavra seguinte a média sobe para ~5 bytes/patch — o
/// mesmo regime em que o BLT opera. É o *space patching* feito direito.
#[derive(Clone, Copy, Debug)]
pub struct PorPalavra {
    pub max: usize,
}

impl Default for PorPalavra {
    fn default() -> Self {
        Self { max: 8 }
    }
}

impl Patcher for PorPalavra {
    fn nome(&self) -> &'static str {
        "por_palavra"
    }

    fn fronteiras(&self, bytes: &[u8], fim: &mut Vec<usize>) {
        fim.clear();
        if bytes.is_empty() {
            return;
        }
        let mut inicio = 0usize;
        for i in 1..bytes.len() {
            let comeca_espaco = classe(bytes[i]) == 0 && classe(bytes[i - 1]) != 0;
            if comeca_espaco || i - inicio >= self.max {
                fim.push(i);
                inicio = i;
            }
        }
        fim.push(bytes.len());
    }
}

/// Fonte de entropia por byte, para [`PorEntropia`].
///
/// É um trait separado por dois motivos. O primeiro é de dependência: o modelo
/// depende do patcher (`hierarchy` usa `Plano`), então o patcher não pode depender do
/// modelo — mas pode depender de um trait que o modelo implemente. O segundo é de
/// medição: a regra de fronteira tem de ser testável sem modelo treinado, senão o
/// único jeito de saber se ela está certa é treinar quatro horas e olhar o resultado.
pub trait Entropia: Send + Sync {
    /// `saida[i]` = entropia, **em bits**, da distribuição do byte `i+1` dado
    /// `bytes[..=i]`. A última posição não tem sucessor e recebe zero.
    fn por_byte(&self, bytes: &[u8], saida: &mut Vec<f32>);
}

/// Emprestar a fonte em vez de movê-la. Serve para o patcher usar um n-grama que
/// vive em outro lugar — a tabela tem centenas de KB e não deve ser duplicada só
/// para varrer um limiar.
impl<E: Entropia + ?Sized> Entropia for &E {
    fn por_byte(&self, bytes: &[u8], saida: &mut Vec<f32>) {
        (**self).por_byte(bytes, saida)
    }
}

/// **A v1, estilo BLT.** Fronteira de palavra como piso, mais um corte onde o
/// próximo byte é surpreendente.
///
/// O BLT aloca patch por entropia: onde o texto é previsível o patch cresce, onde
/// surpreende o patch encolhe, e o modelo grande gasta compute onde é preciso. Aqui a
/// ideia entra pela metade, de propósito.
///
/// **Por que só metade.** [`Plano::limitar`] depende de o patcher quebrar no início de
/// cada corrida de espaço em branco — é isso que garante que a fronteira entre o
/// pedido e o preenchimento do lote caia numa fronteira de patch. Entropia pura
/// juntaria bytes por cima do espaço, a fronteira deixaria de existir, e a cabeça de
/// intenção passaria a ler o estado *depois de ler preenchimento*. Então a entropia
/// aqui só **acrescenta** fronteira, nunca remove.
///
/// **A hipótese que isto testa**, e é estreita a ponto de poder ser medida: nome de
/// arquivo inédito é justamente onde a entropia é alta, e foi justamente onde
/// `mover_arquivo` desabou (a cabeça de presença não via o segundo argumento). Se
/// resolução mais fina em vão de alta entropia serve para alguma coisa, é ali.
///
/// **O acerto experimental que isto exige.** Acrescentar fronteira encurta o patch, e
/// o patch médio *é* o multiplicador de velocidade do backbone. Comparar contra
/// [`PorPalavra`] com `limiar` baixo compararia duas coisas ao mesmo tempo — onde as
/// fronteiras estão e quantas são. `limiar` tem de ser calibrado para o
/// `bytes_por_patch` bater com o do `PorPalavra`; aí a única variável é o *lugar*.
#[derive(Clone, Copy, Debug)]
pub struct PorEntropia<E> {
    pub fonte: E,
    /// Teto de bytes por patch, igual ao do [`PorPalavra`].
    pub max: usize,
    /// Piso de bytes desde a última fronteira. Sem ele, uma corrida de entropia alta
    /// estilhaça o texto em patches de 1 byte e joga a compressão fora.
    pub min: usize,
    /// Entropia (bits) acima da qual o byte abre patch novo.
    pub limiar: f32,
}

impl<E> PorEntropia<E> {
    /// `limiar` alto o bastante para nunca disparar reproduz o [`PorPalavra`] —
    /// e é assim que o teste prova que esta é uma generalização estrita dele.
    pub fn nova(fonte: E, limiar: f32) -> Self {
        Self { fonte, max: 8, min: 2, limiar }
    }
}

impl<E: Entropia> Patcher for PorEntropia<E> {
    fn nome(&self) -> &'static str {
        "por_entropia"
    }

    fn fronteiras(&self, bytes: &[u8], fim: &mut Vec<usize>) {
        fim.clear();
        if bytes.is_empty() {
            return;
        }
        let mut h = Vec::with_capacity(bytes.len());
        self.fonte.por_byte(bytes, &mut h);
        let mut inicio = 0usize;
        for i in 1..bytes.len() {
            let comeca_espaco = classe(bytes[i]) == 0 && classe(bytes[i - 1]) != 0;
            let no_teto = i - inicio >= self.max;
            // `h[i-1]` é a entropia de prever o byte `i`. Quebrar ANTES de `i` é o
            // que dá "o byte surpreendeu, abre patch novo" — a regra de limiar
            // global do BLT.
            let surpreende = h.get(i - 1).copied().unwrap_or(0.0) > self.limiar
                && i - inicio >= self.min;
            if comeca_espaco || no_teto || surpreende {
                fim.push(i);
                inicio = i;
            }
        }
        fim.push(bytes.len());
    }
}

/// Quebra a cada mudança de classe de byte, com teto de `max`.
///
/// Mantido como linha de base e para domínios onde as fronteiras de classe importam
/// mais que a compressão (por exemplo, gramática de ferramentas). Comprime menos
/// que [`PorPalavra`] — ver a nota lá.
#[derive(Clone, Copy, Debug)]
pub struct PorClasse {
    pub max: usize,
}

impl Default for PorClasse {
    fn default() -> Self {
        Self { max: 8 }
    }
}

impl Patcher for PorClasse {
    fn nome(&self) -> &'static str {
        "por_classe"
    }

    fn fronteiras(&self, bytes: &[u8], fim: &mut Vec<usize>) {
        fim.clear();
        if bytes.is_empty() {
            return;
        }
        let mut inicio = 0usize;
        let mut cl = classe(bytes[0]);
        for i in 1..bytes.len() {
            let c = classe(bytes[i]);
            if c != cl || i - inicio >= self.max {
                fim.push(i);
                inicio = i;
                cl = c;
            }
        }
        fim.push(bytes.len());
    }
}

/// Patches de tamanho fixo. Serve de linha de base para medir quanto o patching
/// por classe realmente ganha.
#[derive(Clone, Copy, Debug)]
pub struct Fixo {
    pub p: usize,
}

impl Patcher for Fixo {
    fn nome(&self) -> &'static str {
        "fixo"
    }

    fn fronteiras(&self, bytes: &[u8], fim: &mut Vec<usize>) {
        fim.clear();
        let mut i = self.p;
        while i < bytes.len() {
            fim.push(i);
            i += self.p;
        }
        if !bytes.is_empty() {
            fim.push(bytes.len());
        }
    }
}

/// Plano de patching de um lote inteiro.
///
/// Sequências diferentes rendem números de patches diferentes, então o lote é
/// **preenchido até `p_max`**. As posições de preenchimento recebem entrada zero no
/// backbone e nunca são lidas na saída — como o preenchimento fica sempre no fim de
/// cada sequência e cada sequência tem seu próprio canal de estado, ele não
/// contamina nada.
#[derive(Clone, Debug, Default)]
pub struct Plano {
    pub seq: usize,
    pub batch: usize,
    pub p_max: usize,
    /// `[p_max·batch]` — byte final de cada patch, `usize::MAX` se preenchimento.
    pub ultimo_byte: Vec<usize>,
    /// `[seq·batch]` — patch de contexto de cada byte (o último **completo**),
    /// `usize::MAX` quando ainda não há nenhum.
    pub ctx: Vec<usize>,
    /// `[seq·batch]` — a que patch cada byte pertence. É o que liga o nível
    /// grosseiro ao fino no ponteiro (ver `model::heads`).
    pub patch_de_byte: Vec<usize>,
    pub n_patches: Vec<usize>,
    /// `[batch]` — bytes reais de cada pedido (o resto do lote é preenchimento).
    pub comprimentos: Vec<usize>,
    /// `[seq·batch]` — este byte abre uma palavra?
    ///
    /// Foi calculado para alimentar um viés de fronteira no ponteiro fino, com a
    /// hipótese de que o nível de byte precisava do prior "argumentos começam e
    /// terminam em palavra". **A medição não sustentou**: o viés derrubou a acurácia
    /// de argumento de 94,3% para 88,4%, e o viés foi removido. As marcas ficam
    /// porque são baratas e corretas — mas ninguém as usa hoje.
    pub abre_palavra: Vec<bool>,
    /// `[seq·batch]` — este byte fecha uma palavra?
    pub fecha_palavra: Vec<bool>,
}

pub const SEM_PATCH: usize = usize::MAX;

impl Plano {
    /// `bytes` em ordem *time-major*: `bytes[t·batch + b]`.
    pub fn novo<P: Patcher + ?Sized>(p: &P, bytes: &[u8], seq: usize, batch: usize) -> Self {
        debug_assert_eq!(bytes.len(), seq * batch);
        let mut n_patches = vec![0usize; batch];
        let mut fins: Vec<Vec<usize>> = Vec::with_capacity(batch);
        let mut linha = Vec::with_capacity(seq);
        let mut fim = Vec::new();

        for b in 0..batch {
            linha.clear();
            linha.extend((0..seq).map(|t| bytes[t * batch + b]));
            p.fronteiras(&linha, &mut fim);
            n_patches[b] = fim.len();
            fins.push(fim.clone());
        }
        let p_max = n_patches.iter().copied().max().unwrap_or(0);

        let (abre_palavra, fecha_palavra) =
            Self::marcar_palavras(bytes, seq, batch, &vec![seq; batch]);
        let mut ultimo_byte = vec![SEM_PATCH; p_max * batch];
        let mut ctx = vec![SEM_PATCH; seq * batch];
        let mut patch_de_byte = vec![0usize; seq * batch];

        for b in 0..batch {
            let mut inicio = 0usize;
            for (pi, &f) in fins[b].iter().enumerate() {
                ultimo_byte[pi * batch + b] = f - 1;
                for t in inicio..f {
                    patch_de_byte[t * batch + b] = pi;
                    // O patch `pi` só está completo depois do seu último byte. Antes
                    // disso, o contexto disponível é o patch anterior. É aqui que a
                    // causalidade da hierarquia é garantida.
                    ctx[t * batch + b] = if t == f - 1 {
                        pi
                    } else if pi == 0 {
                        SEM_PATCH
                    } else {
                        pi - 1
                    };
                }
                inicio = f;
            }
        }

        Self {
            seq,
            batch,
            p_max,
            ultimo_byte,
            ctx,
            patch_de_byte,
            n_patches,
            comprimentos: vec![seq; batch],
            abre_palavra,
            fecha_palavra,
        }
    }

    /// Recalcula as marcas de fronteira considerando só os bytes reais.
    fn marcar_palavras(bytes: &[u8], seq: usize, batch: usize, comprimentos: &[usize]) -> (Vec<bool>, Vec<bool>) {
        let branco = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r');
        let mut abre = vec![false; seq * batch];
        let mut fecha = vec![false; seq * batch];
        for b in 0..batch {
            let n = comprimentos[b].min(seq);
            for t in 0..n {
                let c = bytes[t * batch + b];
                if branco(c) {
                    continue;
                }
                abre[t * batch + b] = t == 0 || branco(bytes[(t - 1) * batch + b]);
                fecha[t * batch + b] = t + 1 == n || branco(bytes[(t + 1) * batch + b]);
            }
        }
        (abre, fecha)
    }

    /// Restringe cada sequência ao seu comprimento real, descartando os patches que
    /// caem no preenchimento do lote.
    ///
    /// Pedidos têm tamanhos diferentes e o lote precisa ser retangular, então eles
    /// são preenchidos com espaço até `seq`. Sem esta chamada, "o estado do último
    /// patch" — que é onde a cabeça de intenção lê — seria o estado depois de ler
    /// preenchimento, não depois de ler o pedido.
    ///
    /// Funciona porque o preenchimento fica sempre no fim e o patcher quebra no
    /// início de espaço em branco: a fronteira entre pedido e preenchimento é
    /// sempre uma fronteira de patch.
    pub fn limitar(&mut self, comprimentos: &[usize]) {
        debug_assert_eq!(comprimentos.len(), self.batch);
        self.comprimentos = comprimentos.to_vec();
        for b in 0..self.batch {
            let real = comprimentos[b];
            let n = (0..self.p_max)
                .filter(|&p| {
                    let ub = self.ultimo_byte[p * self.batch + b];
                    ub != SEM_PATCH && ub < real
                })
                .count();
            self.n_patches[b] = n.max(1);
        }
    }

    /// Taxa de compressão: quantos bytes, em média, cada patch representa.
    pub fn bytes_por_patch(&self) -> f64 {
        let total: usize = self.n_patches.iter().sum();
        if total == 0 {
            0.0
        } else {
            (self.seq * self.batch) as f64 / total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fronteiras_de(p: &dyn Patcher, s: &str) -> Vec<usize> {
        let mut f = Vec::new();
        p.fronteiras(s.as_bytes(), &mut f);
        f
    }

    #[test]
    fn por_palavra_gruda_o_separador_na_palavra() {
        let p = PorPalavra { max: 8 };
        let f = fronteiras_de(&p, "ola mundo 42!");
        // "ola" | " mundo" | " 42!"
        assert_eq!(f, vec![3, 9, 13]);
    }

    #[test]
    fn por_palavra_comprime_mais_que_por_classe() {
        let texto = "a teka pensa byte a byte, e o patcher junta os bytes em pedacos                      maiores pra o backbone rodar bem menos vezes por texto lido.";
        let n = texto.len() as f64;
        let pal = fronteiras_de(&PorPalavra { max: 8 }, texto).len() as f64;
        let cla = fronteiras_de(&PorClasse { max: 8 }, texto).len() as f64;
        println!("  por_palavra {:.2} b/patch | por_classe {:.2} b/patch", n/pal, n/cla);
        assert!(n/pal > 4.0, "por_palavra comprimindo pouco: {:.2}", n/pal);
        assert!(n/pal > n/cla, "por_palavra deveria comprimir mais que por_classe");
    }

    #[test]
    fn quebra_na_mudanca_de_classe() {
        let p = PorClasse { max: 8 };
        let f = fronteiras_de(&p, "ola mundo 42!");
        // "ola" | " " | "mundo" | " " | "42" | "!"
        assert_eq!(f, vec![3, 4, 9, 10, 12, 13]);
    }

    #[test]
    fn respeita_o_teto() {
        let p = PorClasse { max: 4 };
        let f = fronteiras_de(&p, "abcdefghij");
        assert_eq!(f, vec![4, 8, 10]);
    }

    #[test]
    fn acentuado_nao_quebra_no_meio() {
        // "coração" em UTF-8: 'ç' e 'ã' são dois bytes cada. Se o patcher tratasse
        // 0x80..0xFF como pontuação, quebraria dentro do caractere.
        let p = PorClasse { max: 32 };
        let f = fronteiras_de(&p, "coração");
        assert_eq!(f, vec!["coração".len()], "deveria ser um patch só");
    }

    #[test]
    fn cobre_a_entrada_inteira() {
        for texto in ["", "a", "ola mundo", "  \n\t  ", "12345678901234567890"] {
            for p in [
                &PorPalavra { max: 8 } as &dyn Patcher,
                &PorClasse { max: 8 } as &dyn Patcher,
                &Fixo { p: 6 } as &dyn Patcher,
            ] {
                let f = fronteiras_de(p, texto);
                if texto.is_empty() {
                    assert!(f.is_empty());
                } else {
                    assert_eq!(*f.last().unwrap(), texto.len(), "{}", p.nome());
                    // estritamente crescente
                    assert!(f.windows(2).all(|w| w[0] < w[1]), "{}", p.nome());
                }
            }
        }
    }

    #[test]
    fn plano_marca_contexto_causal() {
        // batch=1 pra facilitar a leitura: "ab cd" com quebra por classe dá
        // patches "ab"(0..2), " "(2..3), "cd"(3..5).
        let bytes = b"ab cd";
        let plano = Plano::novo(&PorClasse { max: 8 }, bytes, 5, 1);
        assert_eq!(plano.p_max, 3);
        assert_eq!(plano.ultimo_byte, vec![1, 2, 4]);
        // t=0 ('a'): dentro do patch 0, ainda não há patch completo
        // t=1 ('b'): último byte do patch 0 → contexto = patch 0
        // t=2 (' '): último byte do patch 1 → contexto = patch 1
        // t=3 ('c'): dentro do patch 2 → contexto = patch 1
        // t=4 ('d'): último byte do patch 2 → contexto = patch 2
        assert_eq!(plano.ctx, vec![SEM_PATCH, 0, 1, 1, 2]);
    }

    #[test]
    fn preenchimento_do_lote_nao_perde_patch() {
        // Duas sequências com contagens de patch diferentes.
        let seq = 6;
        let batch = 2;
        let mut bytes = vec![0u8; seq * batch];
        for (t, c) in b"aaaaaa".iter().enumerate() {
            bytes[t * batch] = *c; // b=0: um patch só (mesma classe, cabe no teto)
        }
        for (t, c) in b"a b c ".iter().enumerate() {
            bytes[t * batch + 1] = *c; // b=1: seis patches
        }
        let plano = Plano::novo(&PorClasse { max: 8 }, &bytes, seq, batch);
        assert_eq!(plano.n_patches, vec![1, 6]);
        assert_eq!(plano.p_max, 6);
        // As posições de preenchimento de b=0 ficam marcadas.
        for p in 1..6 {
            assert_eq!(plano.ultimo_byte[p * batch], SEM_PATCH);
        }
        assert_ne!(plano.ultimo_byte[1], SEM_PATCH); // b=1 tem patch 0 válido
    }

    /// Fonte de entropia de mentira: devolve o vetor que lhe deram, zero fora dele.
    /// É o que permite testar a regra de fronteira sem modelo treinado — sem isto o
    /// único jeito de saber se ela está certa seria treinar e olhar o resultado.
    struct EntropiaFixa(Vec<f32>);

    impl Entropia for EntropiaFixa {
        fn por_byte(&self, bytes: &[u8], saida: &mut Vec<f32>) {
            saida.clear();
            saida.extend((0..bytes.len()).map(|i| self.0.get(i).copied().unwrap_or(0.0)));
        }
    }

    #[test]
    fn sem_surpresa_reproduz_por_palavra() {
        // O teste que dá direito de chamar o `PorEntropia` de generalização: se nada
        // passa do limiar, ele TEM de dar exatamente as fronteiras do `PorPalavra`.
        // Sem isto, qualquer diferença de acurácia na medição poderia vir de um bug
        // na regra em vez de vir da entropia.
        for texto in [
            "ola mundo 42!",
            "move notas.md para backup.md",
            "a",
            "  espaco  ",
            "coração",
            "12345678901234567890",
        ] {
            let e = PorEntropia {
                fonte: EntropiaFixa(vec![0.0; texto.len()]),
                max: 8,
                min: 2,
                limiar: 1.0,
            };
            assert_eq!(
                fronteiras_de(&e, texto),
                fronteiras_de(&PorPalavra { max: 8 }, texto),
                "divergiu em {texto:?}"
            );
        }
    }

    #[test]
    fn surpresa_abre_patch() {
        let texto = "backupxy"; // 8 bytes, uma palavra só: sem entropia é um patch
        let plano = PorEntropia {
            fonte: EntropiaFixa(vec![0.0; 8]),
            max: 8,
            min: 2,
            limiar: 1.0,
        };
        assert_eq!(fronteiras_de(&plano, texto), vec![8]);

        // Pico em h[3] = "prever o byte 4 é difícil" → fronteira ANTES do byte 4.
        let mut h = vec![0.0f32; 8];
        h[3] = 5.0;
        let pico = PorEntropia {
            fonte: EntropiaFixa(h),
            max: 8,
            min: 2,
            limiar: 1.0,
        };
        assert_eq!(fronteiras_de(&pico, texto), vec![4, 8]);
    }

    #[test]
    fn o_piso_impede_estilhaco() {
        // Entropia alta em toda posição. Sem `min`, isto viraria oito patches de um
        // byte e jogaria fora o multiplicador de velocidade inteiro do backbone.
        let e = PorEntropia {
            fonte: EntropiaFixa(vec![9.0; 8]),
            max: 8,
            min: 3,
            limiar: 1.0,
        };
        assert_eq!(fronteiras_de(&e, "abcdefgh"), vec![3, 6, 8]);
    }

    #[test]
    fn entropia_nunca_remove_fronteira_de_palavra() {
        // A invariante de que `Plano::limitar` depende: a quebra no início de espaço
        // em branco sobrevive a qualquer entropia. Se cair, o preenchimento do lote
        // passa a contaminar o estado que a cabeça de intenção lê.
        let texto = "move notas.md para backup.md";
        let alta = PorEntropia {
            fonte: EntropiaFixa(vec![9.0; texto.len()]),
            max: 8,
            min: 2,
            limiar: 1.0,
        };
        let f = fronteiras_de(&alta, texto);
        for (i, b) in texto.bytes().enumerate().skip(1) {
            if classe(b) == 0 && classe(texto.as_bytes()[i - 1]) != 0 {
                assert!(f.contains(&i), "perdeu a fronteira de espaco em {i}");
            }
        }
        assert_eq!(*f.last().unwrap(), texto.len());
        assert!(f.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn entropia_comprime_menos_e_isso_precisa_ser_calibrado() {
        // Registra o efeito colateral em número, porque é ele que obriga a calibrar
        // o limiar antes de comparar acurácia: acrescentar fronteira encurta o patch,
        // e o patch médio É o multiplicador de velocidade do backbone. Comparar com
        // limiar baixo mediria "onde as fronteiras estão" e "quantas são" de uma vez.
        let texto = "move notas.md para backup.md e apaga rascunho.txt depois disso";
        let n = texto.len() as f64;
        let pal = fronteiras_de(&PorPalavra { max: 8 }, texto).len() as f64;
        let ent = fronteiras_de(
            &PorEntropia {
                fonte: EntropiaFixa(vec![2.0; texto.len()]),
                max: 8,
                min: 2,
                limiar: 1.0,
            },
            texto,
        )
        .len() as f64;
        println!("  por_palavra {:.2} b/patch | por_entropia {:.2} b/patch", n / pal, n / ent);
        assert!(ent >= pal, "entropia so pode acrescentar fronteira");
    }
}
