//! Gramática restrita: o registro de ferramentas compilado num autômato finito.
//!
//! **A ideia mais importante da fase 2.** A cada byte gerado o autômato diz quais
//! bytes são legais, e os logits dos ilegais viram −∞. Com isso:
//!
//! > É **impossível** a Teka emitir uma chamada malformada.
//!
//! Não é "raramente erra" — é estruturalmente impossível. A confiabilidade sintática
//! saiu do modelo e foi para a máquina de estados. O modelo só decide **o quê**,
//! nunca **como escrever**. É o que transforma um modelo de 11 M parâmetros num
//! chamador de ferramentas confiável.
//!
//! Byte-level torna isso quase gratuito: o vocabulário tem 256 símbolos, então a
//! máscara são **32 bytes**. Num modelo com vocabulário de 128 mil, a mesma máscara
//! teria 16 KB e precisaria ser reconstruída a cada passo.
//!
//! ## O formato
//!
//! ```text
//! {"acao":"listar_pasta","caminho":"C:\Users\User"}
//! ```
//!
//! Parecido com JSON, mas **não é JSON**: dentro de um valor, o único byte proibido
//! é `"` (mais os de controle). Isso é deliberado — caminho do Windows tem `\` e
//! escapar barra em cada argumento seria uma fonte de erro sem retorno nenhum.
//!
//! Parâmetros do tipo `Numero` só aceitam dígitos e sinal: a **tipagem também é
//! garantida pela gramática**, não checada depois.

use crate::num::Float;
use crate::tools::{Chamada, Registro, TipoParam};

const INVALIDO: u32 = u32::MAX;

/// Onde o decodificador está, para quem quiser intervir (a cabeça de ponteiro
/// preenche argumentos copiando do pedido em vez de gerar byte a byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posicao {
    /// Ainda escolhendo qual ferramenta.
    Escolhendo,
    /// Dentro do valor do parâmetro `param` da ferramenta `ferramenta`.
    Valor { ferramenta: usize, param: usize },
    /// Numa parte fixa do formato (chaves, aspas, nome de parâmetro).
    Estrutura,
    /// Chamada completa.
    Fim,
}

#[derive(Clone)]
pub struct Automato {
    transicoes: Vec<[u32; 256]>,
    mascaras: Vec<[u8; 32]>,
    aceita: Vec<bool>,
    posicao: Vec<Posicao>,
    inicial: usize,
}

impl Automato {
    pub fn n_estados(&self) -> usize {
        self.transicoes.len()
    }

    pub fn inicial(&self) -> usize {
        self.inicial
    }

    fn novo_estado(&mut self, pos: Posicao) -> usize {
        self.transicoes.push([INVALIDO; 256]);
        self.mascaras.push([0u8; 32]);
        self.aceita.push(false);
        self.posicao.push(pos);
        self.transicoes.len() - 1
    }

    fn ligar(&mut self, de: usize, byte: u8, para: usize) {
        self.transicoes[de][byte as usize] = para as u32;
        self.mascaras[de][byte as usize / 8] |= 1 << (byte % 8);
    }

    /// Encadeia estados para uma sequência literal e devolve o estado final.
    ///
    /// Reaproveita transições já existentes — é isso que faz os nomes das
    /// ferramentas virarem naturalmente um *trie*, sem código de trie nenhum.
    fn literal(&mut self, mut de: usize, bytes: &[u8], pos: Posicao) -> usize {
        for &b in bytes {
            let existente = self.transicoes[de][b as usize];
            de = if existente != INVALIDO {
                existente as usize
            } else {
                let novo = self.novo_estado(pos);
                self.ligar(de, b, novo);
                novo
            };
        }
        de
    }

    /// Compila o registro. Ver o formato no topo do módulo.
    pub fn compilar(reg: &Registro) -> Self {
        let mut a = Automato {
            transicoes: Vec::new(),
            mascaras: Vec::new(),
            aceita: Vec::new(),
            posicao: Vec::new(),
            inicial: 0,
        };
        let inicio = a.novo_estado(Posicao::Estrutura);
        a.inicial = inicio;

        let abertura = a.literal(inicio, b"{\"acao\":\"", Posicao::Estrutura);

        for (fi, f) in reg.ferramentas.iter().enumerate() {
            // O nome entra como literal a partir do MESMO estado para todas as
            // ferramentas: prefixos comuns compartilham caminho, e em cada ponto a
            // máscara só permite bytes que ainda levam a algum nome válido.
            let fim_nome = a.literal(abertura, f.nome.as_bytes(), Posicao::Escolhendo);
            let apos = a.literal(fim_nome, b"\"", Posicao::Estrutura);
            a.cadeia_de_params(apos, fi, f);
        }
        a
    }

    fn cadeia_de_params(&mut self, apos_nome: usize, fi: usize, f: &crate::tools::Ferramenta) {
        let mut atual = apos_nome;
        for (pi, p) in f.params.iter().enumerate() {
            // Pode fechar aqui? Só se este parâmetro e todos os seguintes forem
            // opcionais — e o registro garante que opcionais vêm por último.
            if !p.obrigatorio {
                self.fechar(atual);
            }
            let cabeca = self.literal(
                atual,
                format!(",\"{}\":\"", p.nome).as_bytes(),
                Posicao::Estrutura,
            );
            let dentro = Posicao::Valor {
                ferramenta: fi,
                param: pi,
            };
            // Dois estados: o primeiro exige ao menos um byte (argumento vazio é
            // sempre um erro), o segundo aceita mais bytes ou fecha as aspas.
            let corpo = self.novo_estado(dentro);
            let fim_valor = self.novo_estado(Posicao::Estrutura);
            for b in bytes_de_valor(p.tipo) {
                self.ligar(cabeca, b, corpo);
                self.ligar(corpo, b, corpo);
            }
            self.ligar(corpo, b'"', fim_valor);
            atual = fim_valor;
        }
        self.fechar(atual);
    }

    fn fechar(&mut self, de: usize) {
        let existente = self.transicoes[de][b'}' as usize];
        let fim = if existente != INVALIDO {
            existente as usize
        } else {
            let novo = self.novo_estado(Posicao::Fim);
            self.ligar(de, b'}', novo);
            novo
        };
        self.aceita[fim] = true;
    }

    #[inline]
    pub fn mascara(&self, estado: usize) -> &[u8; 32] {
        &self.mascaras[estado]
    }

    #[inline]
    pub fn proximo(&self, estado: usize, b: u8) -> Option<usize> {
        let t = self.transicoes[estado][b as usize];
        (t != INVALIDO).then_some(t as usize)
    }

    #[inline]
    pub fn aceita(&self, estado: usize) -> bool {
        self.aceita[estado]
    }

    #[inline]
    pub fn posicao(&self, estado: usize) -> Posicao {
        self.posicao[estado]
    }

    /// Quantos bytes são legais neste estado. Uma medida direta de quanta liberdade
    /// o modelo tem — e de quanto trabalho a gramática está fazendo por ele.
    pub fn n_permitidos(&self, estado: usize) -> usize {
        self.mascaras[estado]
            .iter()
            .map(|b| b.count_ones() as usize)
            .sum()
    }
}

fn bytes_de_valor(tipo: TipoParam) -> Vec<u8> {
    match tipo {
        TipoParam::Numero => b"0123456789.,-+".to_vec(),
        // Tudo menos aspas e caracteres de controle. `\` fica permitido: caminho do
        // Windows é o argumento mais comum que existe.
        _ => (0x20u8..=0xFF).filter(|&b| b != b'"').collect(),
    }
}

// ---------------------------------------------------------------------------
// decodificador
// ---------------------------------------------------------------------------

pub struct Decodificador<'a> {
    aut: &'a Automato,
    estado: usize,
    buffer: Vec<u8>,
}

impl<'a> Decodificador<'a> {
    pub fn novo(aut: &'a Automato) -> Self {
        Self {
            aut,
            estado: aut.inicial(),
            buffer: Vec::new(),
        }
    }

    pub fn reiniciar(&mut self) {
        self.estado = self.aut.inicial();
        self.buffer.clear();
    }

    #[inline]
    pub fn permitido(&self, b: u8) -> bool {
        self.aut.mascaras[self.estado][b as usize / 8] & (1 << (b % 8)) != 0
    }

    pub fn posicao(&self) -> Posicao {
        self.aut.posicao(self.estado)
    }

    pub fn aceita(&self) -> bool {
        self.aut.aceita(self.estado)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buffer
    }

    pub fn texto(&self) -> String {
        String::from_utf8_lossy(&self.buffer).into_owned()
    }

    pub fn avancar(&mut self, b: u8) -> Result<Posicao, String> {
        match self.aut.proximo(self.estado, b) {
            Some(p) => {
                self.estado = p;
                self.buffer.push(b);
                Ok(self.aut.posicao(p))
            }
            None => Err(format!(
                "byte {b:#04x} ({:?}) nao e permitido apos {:?}",
                b as char,
                self.texto()
            )),
        }
    }

    /// Empurra uma sequência inteira — usado quando a cabeça de ponteiro já decidiu
    /// o argumento e ele só precisa ser escrito.
    pub fn avancar_todos(&mut self, bytes: &[u8]) -> Result<(), String> {
        for &b in bytes {
            self.avancar(b)?;
        }
        Ok(())
    }

    /// Zera a probabilidade dos bytes ilegais. Custa 256 comparações de bit.
    pub fn mascarar<T: Float>(&self, logits: &mut [T]) {
        debug_assert_eq!(logits.len(), 256);
        let menos_inf = T::from_f64(-1e30);
        let m = self.aut.mascara(self.estado);
        for (b, l) in logits.iter_mut().enumerate() {
            if m[b / 8] & (1 << (b % 8)) == 0 {
                *l = menos_inf;
            }
        }
    }

    /// O byte legal de maior logit. `None` só se nada for permitido (estado final).
    pub fn melhor<T: Float>(&self, logits: &[T]) -> Option<u8> {
        debug_assert_eq!(logits.len(), 256);
        let m = self.aut.mascara(self.estado);
        let mut melhor: Option<(u8, T)> = None;
        for b in 0..256usize {
            if m[b / 8] & (1 << (b % 8)) == 0 {
                continue;
            }
            let l = logits[b];
            if melhor.is_none_or(|(_, best)| l > best) {
                melhor = Some((b as u8, l));
            }
        }
        melhor.map(|(b, _)| b)
    }

    /// A chamada pronta. Só existe se o autômato aceitou.
    pub fn chamada(&self, reg: &Registro) -> Result<Chamada, String> {
        if !self.aceita() {
            return Err("chamada incompleta".into());
        }
        Chamada::parse(&self.texto(), reg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::tools::Registro;

    #[test]
    fn aceita_chamadas_validas() {
        let reg = Registro::padrao();
        let aut = Automato::compilar(&reg);
        for txt in [
            "{\"acao\":\"hora\"}",
            "{\"acao\":\"memoria\"}",
            "{\"acao\":\"listar_pasta\",\"caminho\":\"C:\\Users\\User\"}",
            "{\"acao\":\"calcular\",\"expressao\":\"(15/100)*340\"}",
            "{\"acao\":\"disco\"}",                              // opcional omitido
            "{\"acao\":\"disco\",\"caminho\":\"D:\\\"}",         // opcional presente
            "{\"acao\":\"procurar_arquivo\",\"nome\":\"nota\"}", // só o obrigatório
            "{\"acao\":\"procurar_arquivo\",\"nome\":\"nota\",\"raiz\":\"C:\\tmp\"}",
        ] {
            let mut d = Decodificador::novo(&aut);
            d.avancar_todos(txt.as_bytes())
                .unwrap_or_else(|e| panic!("{txt}: {e}"));
            assert!(d.aceita(), "nao aceitou: {txt}");
            assert!(d.chamada(&reg).is_ok(), "nao parseou: {txt}");
        }
    }

    #[test]
    fn recusa_malformadas() {
        let reg = Registro::padrao();
        let aut = Automato::compilar(&reg);
        for txt in [
            "{\"acao\":\"horas\"}",                  // ferramenta inexistente
            "{\"acao\":\"listar_pasta\"}",           // falta parametro obrigatorio
            "{\"acao\":\"hora\",\"caminho\":\"x\"}", // parametro que a ferramenta nao tem
            "{\"acao\":\"listar_pasta\",\"caminho\":\"\"}", // argumento vazio
            "{\"acao\":\"calcular\",\"expressao\":\"1\",\"extra\":\"2\"}",
            "[\"acao\":\"hora\"]",
        ] {
            let mut d = Decodificador::novo(&aut);
            let r = d.avancar_todos(txt.as_bytes());
            assert!(
                r.is_err() || !d.aceita(),
                "deveria recusar mas aceitou: {txt}"
            );
        }
    }

    #[test]
    fn tipo_numero_e_garantido_pela_gramatica() {
        // Uma ferramenta com parâmetro numérico: a gramática recusa letras sem que
        // ninguém precise validar depois.
        use crate::tools::{Ferramenta, Param, Primitiva, TipoParam};
        let reg = Registro {
            ferramentas: vec![Ferramenta {
                nome: "esperar".into(),
                descricao: "espera n segundos".into(),
                params: vec![Param::obrigatorio("segundos", TipoParam::Numero)],
                prim: Primitiva::Hora,
            }],
        };
        let aut = Automato::compilar(&reg);
        let mut d = Decodificador::novo(&aut);
        d.avancar_todos(b"{\"acao\":\"esperar\",\"segundos\":\"1").unwrap();
        assert!(d.permitido(b'2'));
        assert!(d.permitido(b'.'));
        assert!(!d.permitido(b'a'), "letra nao deveria ser permitida num Numero");
    }

    /// A prova principal: **qualquer** caminho pelo autômato produz uma chamada
    /// válida. Não importa o que o modelo queira, ele não consegue errar a sintaxe.
    #[test]
    fn qualquer_caminho_aceito_e_uma_chamada_valida() {
        let reg = Registro::padrao();
        let aut = Automato::compilar(&reg);
        let mut rng = Rng::new(20260823);
        let mut vistas = std::collections::HashSet::new();

        for _ in 0..4000 {
            let mut d = Decodificador::novo(&aut);
            let mut passos = 0;
            while !d.aceita() && passos < 300 {
                // Escolhe uniformemente entre os bytes permitidos, com uma
                // preferência por fechar quando possível (senão o passeio aleatório
                // fica gerando valores enormes pra sempre).
                let permitidos: Vec<u8> = (0..=255u8).filter(|&b| d.permitido(b)).collect();
                assert!(!permitidos.is_empty(), "estado sem saida: {:?}", d.texto());
                let b = if permitidos.contains(&b'"') && rng.uniform01() < 0.3 {
                    b'"'
                } else if permitidos.contains(&b'}') && rng.uniform01() < 0.3 {
                    b'}'
                } else {
                    permitidos[(rng.uniform01() * permitidos.len() as f64) as usize
                        % permitidos.len()]
                };
                d.avancar(b).unwrap();
                passos += 1;
            }
            if !d.aceita() {
                continue; // passeio longo demais; não é falha do autômato
            }
            let c = d
                .chamada(&reg)
                .unwrap_or_else(|e| panic!("aceitou mas nao parseou: {:?} — {e}", d.texto()));
            // Todo parâmetro obrigatório presente, nenhum parâmetro estranho.
            let f = &reg.ferramentas[c.ferramenta];
            for p in f.params.iter().filter(|p| p.obrigatorio) {
                assert!(
                    c.args.iter().any(|(k, _)| *k == p.nome),
                    "faltou {} em {:?}",
                    p.nome,
                    d.texto()
                );
            }
            for (k, v) in &c.args {
                assert!(f.params.iter().any(|p| p.nome == *k), "parametro estranho {k}");
                assert!(!v.is_empty(), "argumento vazio em {:?}", d.texto());
            }
            vistas.insert(c.ferramenta);
        }
        assert_eq!(
            vistas.len(),
            reg.n(),
            "o passeio aleatorio nao alcancou todas as ferramentas"
        );
        println!("\n  4000 passeios aleatorios: toda chamada aceita e valida");
    }

    #[test]
    fn a_mascara_estreita_o_espaco_drasticamente() {
        let reg = Registro::padrao();
        let aut = Automato::compilar(&reg);
        let mut d = Decodificador::novo(&aut);
        // No início só `{` é legal: 1 byte de 256.
        assert_eq!(aut.n_permitidos(d.estado), 1);
        d.avancar_todos(b"{\"acao\":\"").unwrap();
        // Aqui só as primeiras letras dos nomes de ferramenta. A janela fixa que
        // estava aqui (`n <= 9`) morreu quando o registro foi de 10 para 19
        // ferramentas: passou a haver 11 iniciais distintas e o teste quebrou sem
        // nada estar errado. O que ele quer afirmar não é um número, são dois
        // limites que valem para qualquer registro.
        let n = aut.n_permitidos(d.estado);
        assert!(
            n <= reg.n(),
            "nao pode haver mais iniciais que ferramentas: {n} > {}",
            reg.n()
        );
        assert!(n * 8 < 256, "a mascara deveria estreitar drasticamente, deu {n} de 256");
        println!(
            "\n  {} estados | inicio: 1 byte legal | apos '\\\"acao\\\":\\\"': {} bytes legais",
            aut.n_estados(),
            n
        );
    }

    #[test]
    fn mascarar_logits_zera_os_ilegais() {
        let reg = Registro::padrao();
        let aut = Automato::compilar(&reg);
        let d = Decodificador::novo(&aut);
        let mut logits = vec![1.0f32; 256];
        logits[b'X' as usize] = 99.0; // o modelo adoraria emitir 'X'
        d.mascarar(&mut logits);
        assert!(logits[b'X' as usize] < -1e29, "ilegal nao foi mascarado");
        assert_eq!(logits[b'{' as usize], 1.0);
        assert_eq!(d.melhor(&logits), Some(b'{'), "escolheu byte ilegal");
    }
}
