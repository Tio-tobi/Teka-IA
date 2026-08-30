//! Memória semântica: o que a Teka **sabe**, separado do que ela **fez**.
//!
//! A episódica ([`super::MemoriaEpisodica`]) guarda decisões: este pedido, esta
//! ferramenta, deu certo ou não. Esta guarda fatos: "o projeto fica em
//! C:\Projetos", "o dono se chama João", "ele prefere resposta curta".
//!
//! ## O embedding sai de graça
//!
//! A Nyxara resolve busca semântica com Chroma mais um modelo de embedding externo.
//! São duas dependências, um banco vetorial e uma chamada por fato.
//!
//! A Teka não precisa de nada disso: o tronco já produz a **assinatura** — o estado
//! do backbone no último patch — na mesma passada que decide a ferramenta. Um
//! modelo, uma passada, dois usos. E é um embedding melhor alinhado à tarefa,
//! porque foi treinado para separar intenção, não para similaridade genérica.
//!
//! ## O preço disso, que é real
//!
//! O embedder da Nyxara é congelado; **o da Teka muda a cada treino**. Assinatura
//! guardada com um modelo não é comparável com a de outro — o espaço girou.
//!
//! Ignorar isso apodreceria a memória em silêncio: os cossenos continuariam saindo,
//! só que sem significado. Nenhum erro, só respostas piores.
//!
//! Por isso todo fato guarda **o texto** ao lado da assinatura, mais a marca do
//! modelo que a gerou ([`Fato::marca`]). Quando o modelo muda, [`MemoriaSemantica::
//! precisa_reindexar`] acusa, e as assinaturas são refeitas a partir do texto.
//!
//! ## Esquecer é uma função, não um bug
//!
//! Os tiers e a curva vêm do `memory_tiers.py` e do `ebbinghaus_forgetting.py` da
//! Nyxara, e a ideia é boa: o que foi confirmado muitas vezes decai devagar, o que
//! foi dito uma vez decai rápido, e o núcleo de identidade não decai nunca (`floor`).
//! Sem o piso por tier, uma memória que só decai vira amnésia com passos extras.

use crate::memory::cosseno;

/// Quanto tempo cada camada resiste.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Camada {
    /// Sessão atual. Morre rápido de propósito.
    Trabalho,
    /// Dito uma vez. Está provando a si mesmo.
    Provisorio,
    /// Confirmado várias vezes, sem contestação.
    Estavel,
    /// Identidade, dono, princípio. **Não decai.**
    Nucleo,
}

impl Camada {
    /// Força mínima: abaixo disto ela não cai, por mais tempo que passe.
    pub fn piso(&self) -> f32 {
        match self {
            Camada::Nucleo => 1.0,
            Camada::Estavel => 0.4,
            Camada::Provisorio => 0.0,
            Camada::Trabalho => 0.0,
        }
    }

    /// Em quantos dias a força cai pela metade.
    pub fn meia_vida_dias(&self) -> f32 {
        match self {
            Camada::Nucleo => 10_000.0,
            Camada::Estavel => 90.0,
            Camada::Provisorio => 7.0,
            Camada::Trabalho => 0.5,
        }
    }

    /// Quantas confirmações para subir de camada.
    pub fn promover_com(&self) -> u32 {
        match self {
            Camada::Nucleo => u32::MAX,
            Camada::Estavel => 10,
            Camada::Provisorio => 3,
            Camada::Trabalho => 2,
        }
    }

    pub fn acima(&self) -> Option<Camada> {
        match self {
            Camada::Trabalho => Some(Camada::Provisorio),
            Camada::Provisorio => Some(Camada::Estavel),
            Camada::Estavel => Some(Camada::Nucleo),
            Camada::Nucleo => None,
        }
    }

    pub fn codigo(&self) -> u8 {
        match self {
            Camada::Trabalho => 0,
            Camada::Provisorio => 1,
            Camada::Estavel => 2,
            Camada::Nucleo => 3,
        }
    }

    pub fn de_codigo(c: u8) -> Camada {
        match c {
            3 => Camada::Nucleo,
            2 => Camada::Estavel,
            1 => Camada::Provisorio,
            _ => Camada::Trabalho,
        }
    }
}

/// De onde veio o fato.
///
/// Vem do `VEM_DE` da Nyxara — 578 arestas, mais que o número de fatos dela. Hoje a
/// Teka só tem uma fonte de verdade (o usuário digitando), então isto parece
/// excesso. Está aqui porque **procedência não dá para retrofitar**: quando a
/// segunda fonte aparecer, o histórico já acumulado não tem como ganhar a
/// informação depois. Um campo agora, ou nada nunca.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fonte {
    /// O dono disse.
    Usuario,
    /// Saiu da execução de uma ferramenta.
    Ferramenta(String),
    /// Veio de outro sistema — a migração da Nyxara cai aqui.
    Importado(String),
    /// A própria Teka concluiu. **Não é evidência**: é o que ela achou.
    Inferido,
}

impl Fonte {
    /// Quanto se confia nisto por padrão.
    ///
    /// O que ela mesma inferiu vale menos que o que o dono falou — sem isso, uma
    /// inferência errada vira "fato" com o mesmo peso de uma afirmação direta, e a
    /// memória se autoconvence.
    pub fn peso(&self) -> f32 {
        match self {
            Fonte::Usuario => 1.0,
            Fonte::Ferramenta(_) => 0.9,
            Fonte::Importado(_) => 0.7,
            Fonte::Inferido => 0.5,
        }
    }

    pub fn codigo(&self) -> u8 {
        match self {
            Fonte::Usuario => 0,
            Fonte::Ferramenta(_) => 1,
            Fonte::Importado(_) => 2,
            Fonte::Inferido => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Fato {
    /// O fato em si. É a **fonte da verdade**: a assinatura é derivada dele e pode
    /// ser refeita, ele não.
    pub texto: String,
    /// Embedding do tronco. Vazio quando ainda não foi indexado.
    pub assinatura: Vec<f32>,
    /// Qual modelo produziu a assinatura. Ver [`MemoriaSemantica::precisa_reindexar`].
    pub marca: u64,
    pub camada: Camada,
    /// Quantas vezes foi confirmado. Sobe a camada.
    pub confirmacoes: u32,
    /// Quantas vezes foi lembrado. Estende a meia-vida.
    pub acessos: u32,
    /// Dias (fracionários) desde a criação, no relógio do chamador.
    pub criado_em: f64,
    pub ultimo_acesso_em: f64,
    /// 0..1. Multiplica a força — o que importa resiste mais.
    pub importancia: f32,
    /// De onde veio.
    pub fonte: Fonte,
    /// Índice do fato que tornou este obsoleto.
    ///
    /// Vem do `SUPERSEDED_BY` do grafo da Nyxara, e resolve um buraco real: sem
    /// isto, "prefiro resposta curta" e "prefiro resposta longa" coexistem e a busca
    /// devolve os dois. A memória fica **errada**, não só grande.
    ///
    /// Superar não apaga. O fato velho sai da busca mas continua no arquivo, porque
    /// "o que ela achava antes" é a única forma de responder "desde quando" e de
    /// desfazer uma correção equivocada.
    pub superado_por: Option<usize>,
    /// Conceitos a que este fato está ligado. Índices em [`MemoriaSemantica::conceitos`].
    pub conceitos: Vec<usize>,
}

/// Um nó do grafo associativo: `Conceito` + `RELACIONADO_A` da Nyxara.
///
/// ## Ressalva honesta
///
/// Isto é a estrutura, não o mecanismo. Na Nyxara **um LLM extraía** os conceitos de
/// cada fato; a Teka não tem LLM no runtime. Enquanto ninguém alimentar
/// [`MemoriaSemantica::ligar`], este grafo fica vazio e a expansão não faz nada.
///
/// Está aqui porque é barato e porque a travessia precisa existir antes do
/// extrator para que ele tenha onde entregar. E os números da Nyxara moderam a
/// expectativa: 122 conceitos, 147 arestas, grau médio 2,4 — grafo raso, onde um
/// salto acrescenta pouco sobre o cosseno puro.
#[derive(Clone, Debug)]
pub struct Conceito {
    pub nome: String,
    /// Fatos que mencionam este conceito.
    pub fatos: Vec<usize>,
    /// Conceitos relacionados.
    pub vizinhos: Vec<usize>,
}

impl Fato {
    pub fn novo(texto: impl Into<String>, agora: f64) -> Self {
        Self {
            texto: texto.into(),
            assinatura: Vec::new(),
            marca: 0,
            camada: Camada::Provisorio,
            confirmacoes: 1,
            acessos: 0,
            criado_em: agora,
            ultimo_acesso_em: agora,
            importancia: 0.5,
            fonte: Fonte::Usuario,
            superado_por: None,
            conceitos: Vec::new(),
        }
    }

    /// Ainda vale? Fato superado sai da busca, mas não do arquivo.
    pub fn vigente(&self) -> bool {
        self.superado_por.is_none()
    }

    /// Quão vivo este fato está, entre 0 e 1.
    ///
    /// ```text
    /// meia_vida_efetiva = meia_vida(camada) · 2^min(acessos, 6)
    /// força = 0,5^(dias desde o último reforço / meia_vida_efetiva) · importância
    /// ```
    ///
    /// ## Por que não é a fórmula da Nyxara
    ///
    /// O `compute_memory_strength` dela faz
    /// `retencao(idade) · boost(desde_acesso) · emocao · importancia`, onde
    /// `boost = ebbinghaus_retention(desde_acesso, meia_estendida)`.
    ///
    /// O nome engana: esse `boost` é `0,5^algo`, então é **sempre ≤ 1**. Nunca
    /// aumenta nada — é um segundo fator de decaimento. E como um fato nunca
    /// lembrado recebe `boost = 1,0` exato (o `if retrievals <= 0`), **lembrar de um
    /// fato o deixa mais fraco do que nunca tê-lo lembrado.** O contrário do que
    /// repetição espaçada significa.
    ///
    /// Medido no teste `lembrar_estende_a_meia_vida`, copiando a fórmula dela:
    /// 0,062 depois de quatro acessos contra 0,104 sem acesso nenhum.
    ///
    /// Repetição espaçada **estende a meia-vida** da curva, não multiplica uma
    /// segunda curva por cima. E o relógio corre desde o **último reforço**, não
    /// desde a criação — é isso que a torna espaçada.
    pub fn forca(&self, agora: f64) -> f32 {
        // 2^acessos, saturado em 6: sem o teto, um fato lembrado 30 vezes viraria
        // imortal por acidente e furaria as camadas.
        let fator = 2f32.powi(self.acessos.min(6) as i32);
        let efetiva = self.camada.meia_vida_dias() * fator;
        let desde_reforco = (agora - self.ultimo_acesso_em).max(0.0) as f32;
        let f = meia_vida(desde_reforco, efetiva) * (0.5 + self.importancia * 0.5);
        f.clamp(self.camada.piso(), 1.0)
    }

    /// Confirma o fato: sobe confirmações e promove de camada quando merece.
    pub fn confirmar(&mut self, agora: f64) {
        self.confirmacoes = self.confirmacoes.saturating_add(1);
        self.ultimo_acesso_em = agora;
        if self.confirmacoes >= self.camada.promover_com() {
            if let Some(acima) = self.camada.acima() {
                self.camada = acima;
                // Zera o contador ao promover: subir de novo tem de custar o preço
                // da camada nova, não sobrar troco da anterior.
                self.confirmacoes = 1;
            }
        }
    }

    fn lembrado(&mut self, agora: f64) {
        self.acessos = self.acessos.saturating_add(1);
        self.ultimo_acesso_em = agora;
    }
}

fn meia_vida(t: f32, meia: f32) -> f32 {
    if t <= 0.0 || meia <= 0.0 {
        return 1.0;
    }
    0.5f32.powf(t / meia)
}

#[derive(Default)]
pub struct MemoriaSemantica {
    pub fatos: Vec<Fato>,
    /// Grafo associativo. Vazio até alguém chamar [`Self::ligar`].
    pub conceitos: Vec<Conceito>,
    /// Marca do modelo que indexou esta memória.
    pub marca_atual: u64,
}

impl MemoriaSemantica {
    pub fn nova(marca: u64) -> Self {
        Self {
            fatos: Vec::new(),
            conceitos: Vec::new(),
            marca_atual: marca,
        }
    }

    /// Substitui um fato por outro. O velho sai da busca, não do arquivo.
    ///
    /// Devolve o índice do novo. O novo herda camada, confirmações e conceitos: uma
    /// correção de um fato consolidado não deve renascer provisória — quem trocou
    /// "resposta curta" por "resposta longa" está confirmando que o assunto importa,
    /// não começando do zero.
    pub fn substituir(&mut self, velho: usize, texto_novo: &str, agora: f64) -> usize {
        let (camada, confirmacoes, conceitos, importancia, fonte) = match self.fatos.get(velho) {
            Some(f) => (
                f.camada,
                f.confirmacoes,
                f.conceitos.clone(),
                f.importancia,
                f.fonte.clone(),
            ),
            None => return self.gravar(texto_novo, agora),
        };
        let novo = self.gravar(texto_novo, agora);
        if novo == velho {
            // Mesmo texto normalizado: foi confirmação, não substituição.
            return novo;
        }
        self.fatos[novo].camada = camada;
        self.fatos[novo].confirmacoes = confirmacoes;
        self.fatos[novo].conceitos = conceitos;
        self.fatos[novo].importancia = importancia;
        self.fatos[novo].fonte = fonte;
        self.fatos[velho].superado_por = Some(novo);
        // Reaponta o resto da corrente para o mais novo, senão `historico` teria de
        // percorrer uma lista ligada que pode virar cíclica depois de duas correções.
        for i in 0..self.fatos.len() {
            if i != velho && self.fatos[i].superado_por == Some(velho) {
                self.fatos[i].superado_por = Some(novo);
            }
        }
        novo
    }

    /// A corrente de versões que levou até `i`, da mais velha para a mais nova.
    pub fn historico(&self, i: usize) -> Vec<&Fato> {
        let mut v: Vec<&Fato> = self
            .fatos
            .iter()
            .filter(|f| f.superado_por == Some(i))
            .collect();
        v.sort_by(|a, b| a.criado_em.total_cmp(&b.criado_em));
        v
    }

    /// Índice do conceito, criando se não existir.
    pub fn conceito(&mut self, nome: &str) -> usize {
        let chave = normalizar(nome);
        if let Some(i) = self.conceitos.iter().position(|c| normalizar(&c.nome) == chave) {
            return i;
        }
        self.conceitos.push(Conceito {
            nome: nome.to_string(),
            fatos: Vec::new(),
            vizinhos: Vec::new(),
        });
        self.conceitos.len() - 1
    }

    /// Liga um fato a um conceito. Dois fatos no mesmo conceito viram vizinhos.
    pub fn ligar(&mut self, fato: usize, nome_conceito: &str) {
        if fato >= self.fatos.len() {
            return;
        }
        let c = self.conceito(nome_conceito);
        if !self.conceitos[c].fatos.contains(&fato) {
            self.conceitos[c].fatos.push(fato);
        }
        if !self.fatos[fato].conceitos.contains(&c) {
            self.fatos[fato].conceitos.push(c);
        }
    }

    /// Liga dois conceitos entre si (`RELACIONADO_A`).
    pub fn relacionar(&mut self, a: &str, b: &str) {
        let (ia, ib) = (self.conceito(a), self.conceito(b));
        if ia == ib {
            return;
        }
        if !self.conceitos[ia].vizinhos.contains(&ib) {
            self.conceitos[ia].vizinhos.push(ib);
        }
        if !self.conceitos[ib].vizinhos.contains(&ia) {
            self.conceitos[ib].vizinhos.push(ia);
        }
    }

    /// Fatos alcançáveis a partir de `semente`, num salto pelo grafo.
    ///
    /// É o que o cosseno não faz: "me fala do projeto X" alcança um fato que não tem
    /// nenhuma palavra em comum com "projeto X", porque os dois passam pelo mesmo
    /// conceito. Sem conceitos alimentados, devolve vazio — e isso é o esperado.
    fn vizinhanca(&self, semente: &[(usize, f32)]) -> Vec<(usize, f32)> {
        let so_indices: Vec<usize> = semente.iter().map(|(i, _)| *i).collect();
        let mut vistos: Vec<(usize, f32)> = Vec::new();
        for &(f, herdado) in semente {
            let Some(fato) = self.fatos.get(f) else { continue };
            for &c in &fato.conceitos {
                let Some(con) = self.conceitos.get(c) else { continue };
                // O conceito em si, e os conceitos vizinhos dele: um salto.
                let alcance = std::iter::once(&c).chain(con.vizinhos.iter());
                for &cc in alcance {
                    let Some(outro) = self.conceitos.get(cc) else { continue };
                    for &g in &outro.fatos {
                        if g == f || so_indices.contains(&g) {
                            continue;
                        }
                        if vistos.iter().any(|(v, _)| *v == g) {
                            continue;
                        }
                        // O vizinho HERDA a relevância da semente. Pontuá-lo pelo
                        // cosseno com a busca anularia a expansão: ele chegou aqui
                        // justamente porque o cosseno dele é ~0 — é esse o ponto.
                        vistos.push((g, herdado));
                    }
                }
            }
        }
        vistos
    }

    /// Grava um fato, ou confirma o que já existir com o mesmo texto.
    ///
    /// A comparação é por texto normalizado, não por assinatura: dois jeitos de
    /// dizer a mesma coisa são fatos diferentes até que alguém prove o contrário, e
    /// fundir por cosseno aqui juntaria "gosto de café" com "não gosto de café".
    pub fn gravar(&mut self, texto: &str, agora: f64) -> usize {
        let chave = normalizar(texto);
        if let Some(i) = self.fatos.iter().position(|f| normalizar(&f.texto) == chave) {
            self.fatos[i].confirmar(agora);
            return i;
        }
        self.fatos.push(Fato::novo(texto, agora));
        self.fatos.len() - 1
    }

    /// Indexa um fato com a assinatura vinda do tronco.
    pub fn indexar(&mut self, i: usize, assinatura: Vec<f32>) {
        if let Some(f) = self.fatos.get_mut(i) {
            f.assinatura = assinatura;
            f.marca = self.marca_atual;
        }
    }

    /// Quais fatos estão com assinatura de outro modelo (ou sem assinatura).
    ///
    /// O chamador reprocessa o texto pelo tronco e chama [`Self::indexar`]. Sem esta
    /// checagem a memória apodreceria calada a cada retreino.
    pub fn precisa_reindexar(&self) -> Vec<usize> {
        self.fatos
            .iter()
            .enumerate()
            .filter(|(_, f)| f.assinatura.is_empty() || f.marca != self.marca_atual)
            .map(|(i, _)| i)
            .collect()
    }

    /// Os `k` fatos mais relevantes para uma assinatura.
    ///
    /// Relevância é `cosseno · força`: um fato parecidíssimo mas quase esquecido
    /// perde para um fato parecido e vivo. É o que faz a memória **envelhecer** em
    /// vez de só crescer.
    ///
    /// Marca os devolvidos como lembrados, o que estende a meia-vida deles — usar
    /// uma lembrança é o que a mantém.
    pub fn lembrar(&mut self, assinatura: &[f32], k: usize, agora: f64) -> Vec<(String, f32)> {
        let pontuar = |f: &Fato| cosseno(&f.assinatura, assinatura) * f.forca(agora) * f.fonte.peso();
        let utilizavel =
            |f: &Fato| !f.assinatura.is_empty() && f.marca == self.marca_atual && f.vigente();

        let mut pontuados: Vec<(usize, f32)> = self
            .fatos
            .iter()
            .enumerate()
            .filter(|(_, f)| utilizavel(f))
            .map(|(i, f)| (i, pontuar(f)))
            .filter(|(_, s)| *s > 0.0)
            .collect();
        pontuados.sort_by(|a, b| b.1.total_cmp(&a.1));
        pontuados.truncate(k);

        // -- expansão por conceito, um salto --
        //
        // Entra DEPOIS do corte por cosseno e com penalidade: o vizinho chegou aqui
        // por associação, não por parecença, e não pode passar na frente de quem
        // casou direto. Sem a penalidade, um conceito muito conectado inundaria toda
        // busca com os fatos dele.
        // O vizinho herda a nota da semente e leva a penalidade, multiplicada pela
        // própria força e fonte. Como a penalidade é 0,5 e os outros dois fatores
        // são ≤ 1, o salto **nunca** passa na frente de quem o trouxe.
        const PENALIDADE_SALTO: f32 = 0.5;
        let semente: Vec<(usize, f32)> = pontuados.clone();
        for (g, herdado) in self.vizinhanca(&semente) {
            let Some(f) = self.fatos.get(g) else { continue };
            if !utilizavel(f) {
                continue;
            }
            let s = herdado * PENALIDADE_SALTO * f.forca(agora) * f.fonte.peso();
            if s > 0.0 {
                pontuados.push((g, s));
            }
        }
        pontuados.sort_by(|a, b| b.1.total_cmp(&a.1));
        pontuados.truncate(k);

        let mut saida = Vec::with_capacity(pontuados.len());
        for (i, s) in pontuados {
            self.fatos[i].lembrado(agora);
            saida.push((self.fatos[i].texto.clone(), s));
        }
        saida
    }

    /// Descarta o que já não está vivo. Devolve quantos saíram.
    ///
    /// Núcleo e estável têm piso acima de zero, então nunca caem aqui por idade —
    /// é isso que separa esquecer de ter Alzheimer.
    pub fn esquecer(&mut self, limiar: f32, agora: f64) -> usize {
        let antes = self.fatos.len();
        self.fatos.retain(|f| f.forca(agora) >= limiar);
        antes - self.fatos.len()
    }

    /// Encerra a sessão: o que era de trabalho e não foi confirmado, some.
    pub fn fechar_sessao(&mut self) -> usize {
        let antes = self.fatos.len();
        self.fatos
            .retain(|f| f.camada != Camada::Trabalho || f.confirmacoes > 1);
        antes - self.fatos.len()
    }

    pub fn quantos_por_camada(&self) -> [usize; 4] {
        let mut c = [0usize; 4];
        for f in &self.fatos {
            c[f.camada.codigo() as usize] += 1;
        }
        c
    }
}

/// Minúsculas, sem acento, sem pontuação, espaços colapsados.
fn normalizar(s: &str) -> String {
    let mut saida = String::with_capacity(s.len());
    let mut espaco = false;
    for c in s.chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ô' | 'õ' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            outro => outro,
        };
        if c.is_alphanumeric() {
            for m in c.to_lowercase() {
                saida.push(m);
            }
            espaco = false;
        } else if !espaco && !saida.is_empty() {
            saida.push(' ');
            espaco = true;
        }
    }
    saida.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn o_nucleo_nao_decai_e_o_de_trabalho_evapora() {
        let mut nucleo = Fato::novo("o dono se chama joao", 0.0);
        nucleo.camada = Camada::Nucleo;
        let mut trabalho = Fato::novo("ele esta olhando o arquivo x", 0.0);
        trabalho.camada = Camada::Trabalho;

        // Dez anos depois.
        assert!(
            nucleo.forca(3650.0) >= 1.0,
            "nucleo tem de ser imune ao tempo"
        );
        assert!(
            trabalho.forca(3.0) < 0.05,
            "memoria de trabalho tem de sumir em dias"
        );
    }

    #[test]
    fn lembrar_estende_a_meia_vida() {
        let mut f = Fato::novo("o projeto fica em c:\\projetos", 0.0);
        let sozinho = f.forca(20.0);
        for d in [1.0, 2.0, 4.0, 8.0] {
            f.lembrado(d);
        }
        assert!(
            f.forca(20.0) > sozinho,
            "repeticao espacada tem de segurar a lembranca: {} vs {sozinho}",
            f.forca(20.0)
        );
    }

    #[test]
    fn confirmar_promove_de_camada_e_zera_o_troco() {
        let mut f = Fato::novo("ele prefere resposta curta", 0.0);
        assert_eq!(f.camada, Camada::Provisorio);
        f.confirmar(1.0); // 2
        assert_eq!(f.camada, Camada::Provisorio);
        f.confirmar(2.0); // 3 → promove
        assert_eq!(f.camada, Camada::Estavel);
        assert_eq!(f.confirmacoes, 1, "promover nao pode deixar troco");
    }

    #[test]
    fn gravar_o_mesmo_texto_confirma_em_vez_de_duplicar() {
        let mut m = MemoriaSemantica::nova(7);
        let i = m.gravar("O projeto fica em C:\\Projetos.", 0.0);
        // Mesmo fato, escrito diferente: acento, caixa, pontuacao.
        let j = m.gravar("o projeto fica em c:\\projetos", 1.0);
        assert_eq!(i, j, "normalizacao devia ter casado os dois");
        assert_eq!(m.fatos.len(), 1);
        assert_eq!(m.fatos[0].confirmacoes, 2);
    }

    #[test]
    fn a_lembranca_pesa_forca_junto_com_parecenca() {
        let mut m = MemoriaSemantica::nova(1);
        let i = m.gravar("fato vivo", 0.0);
        m.indexar(i, vec![1.0, 0.0]);
        let j = m.gravar("fato quase esquecido", 0.0);
        m.indexar(j, vec![1.0, 0.0]); // parecenca IDENTICA
        m.fatos[j].camada = Camada::Trabalho; // mas quase morto
        m.fatos[i].camada = Camada::Estavel;

        let r = m.lembrar(&[1.0, 0.0], 2, 5.0);
        assert_eq!(r[0].0, "fato vivo", "entre iguais, o vivo vem primeiro");
        assert!(r[0].1 > r[1].1);
    }

    #[test]
    fn trocar_de_modelo_marca_tudo_para_reindexar() {
        let mut m = MemoriaSemantica::nova(100);
        let i = m.gravar("um fato", 0.0);
        m.indexar(i, vec![0.5, 0.5]);
        assert!(m.precisa_reindexar().is_empty());

        // A Teka treinou: o espaco girou.
        m.marca_atual = 101;
        assert_eq!(
            m.precisa_reindexar(),
            vec![0],
            "assinatura de outro modelo tem de ser recusada, nao usada calada"
        );
        // E enquanto nao reindexar, ela nao entra na busca.
        assert!(m.lembrar(&[0.5, 0.5], 5, 0.0).is_empty());
    }

    #[test]
    fn esquecer_poupa_o_que_tem_piso() {
        let mut m = MemoriaSemantica::nova(1);
        let i = m.gravar("identidade", 0.0);
        m.fatos[i].camada = Camada::Nucleo;
        let j = m.gravar("bobagem do momento", 0.0);
        m.fatos[j].camada = Camada::Trabalho;

        assert_eq!(m.esquecer(0.2, 10.0), 1);
        assert_eq!(m.fatos.len(), 1);
        assert_eq!(m.fatos[0].texto, "identidade");
    }

    #[test]
    fn fechar_sessao_leva_o_trabalho_nao_confirmado() {
        let mut m = MemoriaSemantica::nova(1);
        let a = m.gravar("visto uma vez", 0.0);
        m.fatos[a].camada = Camada::Trabalho;
        let b = m.gravar("visto e repetido", 0.0);
        m.fatos[b].camada = Camada::Trabalho;
        m.gravar("visto e repetido", 1.0); // confirma
        m.gravar("fato normal", 0.0);

        assert_eq!(m.fechar_sessao(), 1);
        assert!(m.fatos.iter().all(|f| f.texto != "visto uma vez"));
    }

    #[test]
    fn substituir_tira_o_velho_da_busca_mas_nao_do_arquivo() {
        let mut m = MemoriaSemantica::nova(1);
        let velho = m.gravar("ele prefere resposta curta", 0.0);
        m.fatos[velho].camada = Camada::Estavel;
        m.fatos[velho].confirmacoes = 5;
        m.indexar(velho, vec![1.0, 0.0]);

        let novo = m.substituir(velho, "ele prefere resposta longa", 10.0);
        m.indexar(novo, vec![1.0, 0.0]);

        // Os dois existem, mas so um responde.
        assert_eq!(m.fatos.len(), 2);
        let r = m.lembrar(&[1.0, 0.0], 5, 10.0);
        assert_eq!(r.len(), 1, "o superado nao pode voltar na busca: {r:?}");
        assert_eq!(r[0].0, "ele prefere resposta longa");

        // Correcao de fato consolidado nao renasce provisoria.
        assert_eq!(m.fatos[novo].camada, Camada::Estavel);
        assert_eq!(m.fatos[novo].confirmacoes, 5);
        // E da para contar a historia.
        let h = m.historico(novo);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].texto, "ele prefere resposta curta");
    }

    #[test]
    fn duas_correcoes_seguidas_nao_perdem_a_corrente() {
        let mut m = MemoriaSemantica::nova(1);
        let a = m.gravar("mora em sao paulo", 0.0);
        let b = m.substituir(a, "mora no rio", 1.0);
        let c = m.substituir(b, "mora em curitiba", 2.0);

        let h = m.historico(c);
        assert_eq!(h.len(), 2, "as duas versoes velhas tem de apontar para a atual");
        assert_eq!(h[0].texto, "mora em sao paulo");
        assert_eq!(h[1].texto, "mora no rio");
        assert!(m.fatos[c].vigente());
    }

    #[test]
    fn a_fonte_pesa_na_busca() {
        let mut m = MemoriaSemantica::nova(1);
        let dito = m.gravar("o projeto fica em c", 0.0);
        m.indexar(dito, vec![1.0, 0.0]);
        let achado = m.gravar("o projeto fica em d", 0.0);
        m.indexar(achado, vec![1.0, 0.0]);
        m.fatos[achado].fonte = Fonte::Inferido;

        let r = m.lembrar(&[1.0, 0.0], 2, 0.0);
        assert_eq!(
            r[0].0, "o projeto fica em c",
            "o que o dono disse ganha do que ela inferiu"
        );
        assert!(r[0].1 > r[1].1);
    }

    #[test]
    fn o_grafo_alcanca_o_que_o_cosseno_nao_alcancaria() {
        let mut m = MemoriaSemantica::nova(1);
        let a = m.gravar("a teka roda em rust", 0.0);
        m.indexar(a, vec![1.0, 0.0]);
        let b = m.gravar("o prazo e sexta", 0.0);
        // Assinatura ORTOGONAL: por cosseno, este fato nunca apareceria.
        m.indexar(b, vec![0.0, 1.0]);

        assert_eq!(m.lembrar(&[1.0, 0.0], 5, 0.0).len(), 1);

        m.ligar(a, "projeto teka");
        m.ligar(b, "projeto teka");
        let r = m.lembrar(&[1.0, 0.0], 5, 0.0);
        assert_eq!(r.len(), 2, "um salto pelo conceito devia trazer o outro");
        assert_eq!(r[0].0, "a teka roda em rust", "quem casou direto vem primeiro");
        assert_eq!(r[1].0, "o prazo e sexta");
    }

    #[test]
    fn conceitos_vizinhos_alcancam_dois_assuntos() {
        let mut m = MemoriaSemantica::nova(1);
        let a = m.gravar("uso rust no trabalho", 0.0);
        m.indexar(a, vec![1.0, 0.0]);
        let b = m.gravar("cargo baixa as dependencias", 0.0);
        m.indexar(b, vec![0.0, 1.0]);
        m.ligar(a, "rust");
        m.ligar(b, "cargo");
        // Sem a aresta entre os conceitos, um salto nao chega la.
        assert_eq!(m.lembrar(&[1.0, 0.0], 5, 0.0).len(), 1);
        m.relacionar("rust", "cargo");
        assert_eq!(m.lembrar(&[1.0, 0.0], 5, 0.0).len(), 2);
    }

    #[test]
    fn o_grafo_vazio_nao_muda_nada() {
        // Sem extrator de conceitos, tudo tem de continuar funcionando igual.
        let mut m = MemoriaSemantica::nova(1);
        let a = m.gravar("um fato", 0.0);
        m.indexar(a, vec![1.0, 0.0]);
        assert_eq!(m.lembrar(&[1.0, 0.0], 5, 0.0).len(), 1);
        assert!(m.conceitos.is_empty());
    }

    #[test]
    fn normalizar_junta_o_que_e_o_mesmo_e_separa_o_que_nao_e() {
        assert_eq!(normalizar("Memória  do  João!"), "memoria do joao");
        assert_eq!(normalizar("ação, é isso."), "acao e isso");
        assert_ne!(normalizar("gosto de cafe"), normalizar("nao gosto de cafe"));
    }
}
