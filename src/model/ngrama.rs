//! Modelo de n-grama de bytes — **a fonte de entropia do patcher**.
//!
//! O BLT decide fronteira de patch pela entropia do próximo byte, e paga um
//! transformer de 100M de parâmetros só para produzir esse número. A Teka não pode:
//! uma passada neural por byte, na CPU, custaria mais que o backbone que o patching
//! existe para economizar.
//!
//! Um n-grama resolve o mesmo problema por consulta em tabela. É pior que uma rede
//! como modelo de linguagem — e irrelevante que seja, porque aqui ele não gera texto,
//! só responde "este byte surpreende?". Para essa pergunta, contagem basta.
//!
//! ## O mecanismo que faz isto valer a pena
//!
//! Contexto nunca visto devolve [`ENTROPIA_MAXIMA`], não zero. Isso não é detalhe de
//! implementação, é o ponto: **nome de arquivo inédito é contexto inédito**, então
//! ele recebe entropia alta e ganha fronteira de patch. Foi exatamente ali que
//! `mover_arquivo` desabou — a cabeça de presença não enxergava o segundo argumento
//! quando o nome era desconhecido.
//!
//! ## Por que o corpus maior importa aqui, e não importava no tronco
//!
//! Nove pares de sementes mostraram que pré-treinar o tronco não transfere para a
//! agente (+2,2 frases de 150, t=1,20). Aquilo media *inicialização de pesos*. Isto
//! aqui é outro uso do mesmo corpus: mais texto significa mais contexto de 4 bytes
//! com contagem suficiente, e portanto menos "não sei" falso. O resultado negativo do
//! tronco não se aplica, mas também não é licença para supor que este vai dar certo —
//! tem de ser medido igual.

use std::io::{Read, Write};
use std::path::Path;

use super::patcher::Entropia;

/// `log2(256)`. O que uma distribuição uniforme sobre bytes vale, e o que um
/// contexto sem observação suficiente recebe.
pub const ENTROPIA_MAXIMA: f32 = 8.0;

/// Quantas observações um contexto precisa antes de sua entropia ser levada a sério.
///
/// Sem piso, um contexto visto **uma** vez teria entropia zero — "perfeitamente
/// previsível" — quando o certo é "não sei nada sobre isto". O erro tem sinal
/// perigoso: diria "não quebre aqui" justamente nos trechos raros, que são os que
/// mais precisam de fronteira.
pub const MINIMO_OBS: u32 = 8;

pub struct NGrama {
    ordem: usize,
    mascara: usize,
    /// `[2^bits]` — entropia em bits do próximo byte, por casela de contexto.
    entropia: Vec<f32>,
}

/// FNV-1a sobre os bytes do contexto. Barato e espalha bem o bastante; a tabela é
/// endereçamento direto com colisão assumida, não um mapa exato.
#[inline]
fn embaralhar(ctx: &[u8]) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in ctx {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    (h ^ (h >> 32)) as usize
}

impl NGrama {
    /// Constrói contando o corpus inteiro numa passada.
    ///
    /// `bits` dá o tamanho da tabela final (`2^bits` caselas de `f32`).
    ///
    /// ## Por que a contagem é esparsa
    ///
    /// A primeira versão contava num vetor denso de `2^bits · 256 · u32`. Isso
    /// amarrava o tamanho da tabela à memória de construção — `bits = 17` já custava
    /// 134 MB, e `bits = 22` pediria 4 GB — então a tabela ficou pequena demais, e
    /// **a medição saiu ao contrário**: ordem 8 devolveu "0,0% sem dados", quando
    /// ordem mais alta tem obrigatoriamente de ser *mais* esparsa. O motivo era
    /// colisão: 7,1 milhões de posições em 131 mil caselas põem toda casela acima de
    /// [`MINIMO_OBS`] por acúmulo de contextos alheios, e aí ninguém diz "não sei" —
    /// a entropia guardada vira a média borrada de dezenas de contextos sem relação.
    ///
    /// Contando esparso, só contexto que aparece ocupa memória, e a tabela final pode
    /// ser grande o bastante (`2^22` = 16 MB) para a colisão voltar a ser rara.
    ///
    /// O custo é memória proporcional ao número de contextos distintos, que **cresce
    /// com a ordem**: ordem 4 num corpus de 7 MB é barato, ordem 8 num de 72 MB não é.
    pub fn treinar(corpus: &[u8], ordem: usize, bits: u32) -> Self {
        use std::collections::HashMap;

        let m = 1usize << bits;
        let mascara = m - 1;
        // Fan-out típico de um contexto de bytes é pequeno (poucos sucessores
        // distintos), então varredura linear num `Vec` curto bate mapa aninhado.
        let mut contextos: HashMap<usize, Vec<(u8, u32)>> = HashMap::new();

        if corpus.len() > ordem {
            for i in (ordem - 1)..(corpus.len() - 1) {
                let h = embaralhar(&corpus[i + 1 - ordem..=i]);
                let prox = corpus[i + 1];
                let sucessores = contextos.entry(h).or_default();
                match sucessores.iter_mut().find(|(b, _)| *b == prox) {
                    Some((_, c)) => *c += 1,
                    None => sucessores.push((prox, 1)),
                }
            }
        }

        let mut entropia = vec![ENTROPIA_MAXIMA; m];
        for (h, sucessores) in &contextos {
            let total: u32 = sucessores.iter().map(|(_, c)| *c).sum();
            if total < MINIMO_OBS {
                continue; // fica em ENTROPIA_MAXIMA: "nao sei"
            }
            let t = total as f32;
            let mut bits_h = 0.0f32;
            for (_, c) in sucessores {
                let p = *c as f32 / t;
                bits_h -= p * p.log2();
            }
            // Colisão na tabela final: fica a MENOR entropia. Entre "sei" e "não
            // sei" na mesma casela, guardar o "sei" erra para o lado barato — deixa
            // de criar uma fronteira. Guardar o "não sei" criaria fronteira em
            // contexto conhecido, que é o erro que estilhaça o patch.
            let casela = *h & mascara;
            if bits_h < entropia[casela] {
                entropia[casela] = bits_h;
            }
        }

        Self { ordem, mascara, entropia }
    }

    /// Quantos contextos distintos o corpus rende nesta ordem. Serve para saber se a
    /// tabela é grande o bastante: se isto passa de `2^bits`, a colisão volta.
    pub fn contextos_distintos(corpus: &[u8], ordem: usize) -> usize {
        use std::collections::HashSet;
        let mut vistos: HashSet<usize> = HashSet::new();
        if corpus.len() > ordem {
            for i in (ordem - 1)..(corpus.len() - 1) {
                vistos.insert(embaralhar(&corpus[i + 1 - ordem..=i]));
            }
        }
        vistos.len()
    }

    pub fn ordem(&self) -> usize {
        self.ordem
    }

    pub fn caselas(&self) -> usize {
        self.entropia.len()
    }

    /// Fração da tabela que ficou sem observação suficiente. É o número que diz se o
    /// corpus é grande o bastante para a ordem escolhida: perto de 1 significa que o
    /// modelo responde "não sei" quase sempre e o patcher vira ruído.
    pub fn fracao_sem_dados(&self) -> f64 {
        let n = self
            .entropia
            .iter()
            .filter(|h| **h >= ENTROPIA_MAXIMA)
            .count();
        n as f64 / self.entropia.len().max(1) as f64
    }

    /// Fração das **posições de um texto** que caem em contexto sem dados.
    ///
    /// Esta é a métrica que interessa, e [`Self::fracao_sem_dados`] não é: aquela mede
    /// ocupação de tabela, que nem se move quando o corpus repete o mesmo texto mais
    /// vezes. O patcher não percorre a tabela, percorre um pedido — o que decide se o
    /// corpus é grande o bastante é quantas *posições reais* ainda respondem "não sei".
    ///
    /// Medir num texto que ficou fora do treino é o único jeito honesto de usar isto.
    pub fn fracao_sem_dados_em(&self, texto: &[u8]) -> f64 {
        if texto.is_empty() {
            return 0.0;
        }
        let n = (0..texto.len())
            .filter(|&i| self.em(texto, i) >= ENTROPIA_MAXIMA)
            .count();
        n as f64 / texto.len() as f64
    }

    /// Entropia do byte seguinte a `bytes[..=i]`.
    #[inline]
    pub fn em(&self, bytes: &[u8], i: usize) -> f32 {
        if i + 1 < self.ordem {
            return ENTROPIA_MAXIMA; // contexto ainda mais curto que a ordem
        }
        self.entropia[embaralhar(&bytes[i + 1 - self.ordem..=i]) & self.mascara]
    }

    pub fn salvar(&self, caminho: &Path) -> std::io::Result<usize> {
        let mut f = std::fs::File::create(caminho)?;
        f.write_all(b"TKNG")?;
        f.write_all(&(self.ordem as u32).to_le_bytes())?;
        f.write_all(&(self.entropia.len() as u64).to_le_bytes())?;
        let mut cru = Vec::with_capacity(self.entropia.len() * 4);
        for h in &self.entropia {
            cru.extend_from_slice(&h.to_le_bytes());
        }
        f.write_all(&cru)?;
        Ok(16 + cru.len())
    }

    pub fn carregar(caminho: &Path) -> Result<Self, String> {
        let mut f = std::fs::File::open(caminho).map_err(|e| e.to_string())?;
        let mut cab = [0u8; 16];
        f.read_exact(&mut cab).map_err(|e| e.to_string())?;
        if &cab[..4] != b"TKNG" {
            return Err("nao e um arquivo de n-grama da Teka".into());
        }
        let ordem = u32::from_le_bytes(cab[4..8].try_into().unwrap()) as usize;
        let n = u64::from_le_bytes(cab[8..16].try_into().unwrap()) as usize;
        if !n.is_power_of_two() {
            return Err(format!("tamanho de tabela {n} nao e potencia de dois"));
        }
        let mut cru = vec![0u8; n * 4];
        f.read_exact(&mut cru).map_err(|e| e.to_string())?;
        let entropia = cru
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        Ok(Self { ordem, mascara: n - 1, entropia })
    }
}

impl Entropia for NGrama {
    fn por_byte(&self, bytes: &[u8], saida: &mut Vec<f32>) {
        saida.clear();
        saida.reserve(bytes.len());
        for i in 0..bytes.len() {
            saida.push(self.em(bytes, i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texto_repetido_fica_previsivel() {
        // "abcabcabc..." é o caso mais previsível que existe: depois de "abc" vem
        // sempre 'a'. A entropia ali tem de ir a zero.
        let corpus: Vec<u8> = "abc".repeat(4000).into_bytes();
        let ng = NGrama::treinar(&corpus, 3, 12);
        let amostra = b"abcabcabc";
        assert!(
            ng.em(amostra, 5) < 0.1,
            "contexto repetido deveria ser previsivel, deu {}",
            ng.em(amostra, 5)
        );
    }

    #[test]
    fn contexto_inedito_devolve_nao_sei() {
        // O mecanismo inteiro do patcher depende disto: contexto que nunca apareceu
        // recebe entropia MAXIMA, nao zero. É o que faz nome de arquivo inedito
        // ganhar fronteira.
        let corpus: Vec<u8> = "abc".repeat(4000).into_bytes();
        let ng = NGrama::treinar(&corpus, 3, 12);
        assert_eq!(ng.em(b"zqxjvw", 4), ENTROPIA_MAXIMA);
    }

    #[test]
    fn uma_observacao_nao_vira_certeza() {
        // Sem o piso de MINIMO_OBS, um contexto visto uma vez daria entropia zero —
        // "perfeitamente previsivel" — e o erro tem o sinal perigoso: mandaria NAO
        // quebrar justamente no trecho raro.
        let mut corpus = "xxxx".repeat(3000).into_bytes();
        corpus.extend_from_slice(b"raroQ");
        let ng = NGrama::treinar(&corpus, 4, 14);
        assert_eq!(ng.em(b"raroQ", 3), ENTROPIA_MAXIMA);
    }

    #[test]
    fn duas_continuacoes_iguais_dao_um_bit() {
        // Contexto seguido de dois bytes diferentes em proporcao igual: 1 bit exato.
        let mut corpus = Vec::new();
        for _ in 0..1000 {
            corpus.extend_from_slice(b"qa");
            corpus.extend_from_slice(b"qb");
        }
        let ng = NGrama::treinar(&corpus, 1, 12);
        let h = ng.em(b"q", 0);
        assert!((h - 1.0).abs() < 0.05, "esperava ~1 bit, deu {h}");
    }

    #[test]
    fn por_byte_cobre_a_entrada() {
        let corpus: Vec<u8> = "o rato roeu a roupa do rei ".repeat(500).into_bytes();
        let ng = NGrama::treinar(&corpus, 4, 14);
        let mut h = Vec::new();
        ng.por_byte(b"o rato roeu", &mut h);
        assert_eq!(h.len(), "o rato roeu".len());
        assert!(h.iter().all(|x| (0.0..=ENTROPIA_MAXIMA).contains(x)));
    }

    #[test]
    fn salvar_e_carregar_preserva_a_tabela() {
        let corpus: Vec<u8> = "o rato roeu a roupa do rei ".repeat(500).into_bytes();
        let ng = NGrama::treinar(&corpus, 4, 12);
        let tmp = std::env::temp_dir().join("teka_ngrama_teste.bin");
        ng.salvar(&tmp).expect("salvar");
        let volta = NGrama::carregar(&tmp).expect("carregar");
        assert_eq!(volta.ordem(), ng.ordem());
        assert_eq!(volta.caselas(), ng.caselas());
        let amostra = b"o rato roeu a roupa";
        for i in 0..amostra.len() {
            assert_eq!(volta.em(amostra, i), ng.em(amostra, i), "divergiu em {i}");
        }
        let _ = std::fs::remove_file(&tmp);
    }

    /// Texto sintético com vocabulário *variado*, controlado por semente.
    ///
    /// Repetir a mesma frase mais vezes não serve para este teste: não cria contexto
    /// novo, só aumenta a contagem dos mesmos poucos. Foi assim que a primeira versão
    /// deste teste deu 0,9969 nos dois lados e me mostrou que eu estava medindo
    /// ocupação de tabela em vez de cobertura de texto.
    fn corpus_sintetico(n_palavras: usize, semente: u64) -> Vec<u8> {
        const SILABAS: &[&str] = &[
            "ba", "ca", "da", "fa", "ga", "la", "ma", "na", "pa", "ra", "sa", "ta", "be", "ce",
            "de", "fe", "ge", "le", "me", "ne", "pe", "re", "se", "te", "bi", "ci", "di", "fi",
            "gi", "li", "mi", "ni", "pi", "ri", "si", "ti", "bo", "co", "do", "fo", "go", "lo",
        ];
        let mut s = String::new();
        let mut x = semente;
        let proximo = |x: &mut u64| {
            *x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (*x >> 33) as usize
        };
        for _ in 0..n_palavras {
            for _ in 0..(2 + proximo(&mut x) % 3) {
                s.push_str(SILABAS[proximo(&mut x) % SILABAS.len()]);
            }
            s.push(' ');
        }
        s.into_bytes()
    }

    #[test]
    fn corpus_maior_reduz_o_nao_sei() {
        // O elo que justifica o corpus 10x: mais texto significa mais contexto de 4
        // bytes com contagem suficiente, e portanto menos "nao sei" falso. Se este
        // numero nao cair com mais dados, o corpus maior nao serve ao patcher — e
        // entao a fase 1 inteira do plano estaria errada.
        //
        // A sonda usa OUTRA semente: medir cobertura no proprio texto de treino
        // responderia "decorei", nao "generalizo".
        let sonda = corpus_sintetico(200, 999);
        let pequeno = corpus_sintetico(200, 1);
        let grande = corpus_sintetico(20_000, 1);
        let a = NGrama::treinar(&pequeno, 4, 16).fracao_sem_dados_em(&sonda);
        let b = NGrama::treinar(&grande, 4, 16).fracao_sem_dados_em(&sonda);
        println!("  posicoes sem dados: corpus pequeno {a:.3} | corpus 100x {b:.3}");
        assert!(b < a, "corpus maior deveria cobrir mais posicoes: {a} -> {b}");
    }

    #[test]
    fn ordem_maior_nunca_cobre_mais() {
        // A invariante que a tabela densa violava sem avisar: contexto mais longo é
        // obrigatoriamente MAIS raro, então "sem dados" só pode subir com a ordem.
        // A primeira versão devolvia 0,0% na ordem 8 — não por cobrir tudo, mas
        // porque 7 milhões de posições em 131 mil caselas põem toda casela acima de
        // MINIMO_OBS por colisão, e aí ninguém mais diz "não sei".
        //
        // Se isto inverter de novo, a tabela ficou pequena para o corpus.
        let corpus = corpus_sintetico(20_000, 1);
        let sonda = corpus_sintetico(200, 999);
        let mut anterior = -1.0f64;
        for ordem in [2usize, 4, 6, 8] {
            let f = NGrama::treinar(&corpus, ordem, 22).fracao_sem_dados_em(&sonda);
            println!("  ordem {ordem}: {:.1}% sem dados", f * 100.0);
            assert!(
                f >= anterior - 1e-9,
                "ordem {ordem} cobriu MAIS que a anterior ({f:.4} < {anterior:.4}) — colisao saturando"
            );
            anterior = f;
        }
    }

    #[test]
    fn ocupacao_de_tabela_nao_e_cobertura() {
        // Registra a confusao que derrubou a primeira versao do teste acima, para
        // ninguem (eu, daqui a duas semanas) trocar uma metrica pela outra de novo.
        let base = "o rato roeu a roupa do rei ";
        let pouco = NGrama::treinar(&base.repeat(20).into_bytes(), 4, 14);
        let muito = NGrama::treinar(&base.repeat(2000).into_bytes(), 4, 14);
        assert_eq!(
            pouco.fracao_sem_dados(),
            muito.fracao_sem_dados(),
            "repetir texto nao cria contexto novo — ocupacao tem de ficar igual"
        );
    }
}
