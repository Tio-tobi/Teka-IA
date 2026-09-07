//! Regras permanentes: ela vigia e age sozinha, enquanto ligada.
//!
//! ## O pedido que originou isto
//!
//! *"Teka, toda vez que a música parar, você retoma — por causa do Discord."*
//!
//! Até aqui a Teka era um tiro só: pedido → chamada → fim. O `pulso` roda em
//! segundo plano mas só **pensa** e **consolida**; não observa estado nem dispara
//! ação. O bite tem isso (`tasks.py`, `task_graph.py`, `autonomous_kernel.py`); a
//! Teka não tinha.
//!
//! ## As duas travas, e por que existem
//!
//! **1. O dono desliga com a voz, não com adivinhação.**
//!
//! Minha primeira versão tentava inferir quem pausou: "estava tocando, parou, e a
//! regra não mandou nada — logo foi o dono". O teste derrubou na hora, e a razão é
//! de fundo: **do estado do player, os dois casos são idênticos.** Não existe
//! diferença observável entre o Discord pausando e o John pausando.
//!
//! Então a regra retoma sempre, e para quando o dono manda parar —
//! [`Vigia::dono_pausou`], acionada quando ele pede pausa pela Teka, ou desligando
//! a regra. Foi o que ele respondeu quando eu levantei o medo de "ela vai brigar
//! comigo": *"não era só pedir para ela pausar?"*. Era.
//!
//! **2. Teto de disparos.** Se a ação não resolve — a ponte caiu, o Spotify fechou —
//! ela para de tentar. Uma regra que insiste contra a realidade queima CPU e enche
//! log sem nunca dar certo.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Quantas vezes seguidas a regra pode disparar sem o mundo mudar.
///
/// Três, e não uma: a primeira pode perder uma corrida com o app, a segunda cobre
/// o caso normal. A quarta seria teimosia.
const TETO_SEGUIDO: u32 = 3;

/// Espaço mínimo entre dois disparos da mesma regra.
const ESPACO: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condicao {
    /// O Spotify está pausado.
    MusicaParada,
}

impl Condicao {
    pub fn ler(nome: &str) -> Option<Condicao> {
        match nome.trim() {
            "musica_parada" => Some(Condicao::MusicaParada),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Regra {
    pub nome: String,
    pub condicao: Condicao,
    pub acao: String,
    pub ligada: bool,
}

const TABELA: &str = include_str!("../../dados/regras.txt");

pub fn ler(texto: &str) -> Vec<Regra> {
    let mut fora = Vec::new();
    for l in texto.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let p: Vec<&str> = l.split('|').map(str::trim).collect();
        if p.len() < 3 {
            continue;
        }
        let Some(condicao) = Condicao::ler(p[1]) else { continue };
        fora.push(Regra {
            nome: p[0].to_string(),
            condicao,
            acao: p[2].to_string(),
            ligada: p.get(3).is_some_and(|v| {
                matches!(v.to_lowercase().as_str(), "sim" | "s" | "true" | "1")
            }),
        });
    }
    fora
}

pub fn tabela() -> Vec<Regra> {
    ler(TABELA)
}

/// O estado que separa "parou sozinho" de "o dono parou".
#[derive(Debug, Default)]
pub struct Vigia {
    /// A última vez que a regra mandou tocar. Serve para não contar como "pausa do
    /// dono" a janela entre o comando e o app obedecer.
    ultimo_disparo: Option<Instant>,
    /// Disparos seguidos sem o mundo mudar.
    seguidos: u32,
    /// Suspensa porque o dono pausou. Só volta quando ele tocar de novo.
    suspensa: bool,
    /// O estado visto na última olhada.
    tocava: bool,
}

/// O que fazer depois de olhar o mundo.
#[derive(Debug, PartialEq, Eq)]
pub enum Passo {
    /// Nada a fazer.
    Quieto,
    /// Dispara a ação.
    Age,
    /// Suspensa: o dono pausou de propósito.
    Suspensa,
    /// Desistiu: a ação não resolveu depois de `TETO_SEGUIDO` tentativas.
    Desistiu,
}

impl Vigia {
    /// Decide o que fazer, dado o estado atual do mundo.
    ///
    /// `tocando` é o que a ponte reportou agora. `agora` entra como parâmetro para
    /// o teste poder controlar o tempo em vez de dormir.
    pub fn olhar(&mut self, tocando: bool, agora: Instant) -> Passo {
        // Voltou a tocar: o mundo esta como se quer. Zera tudo, inclusive a
        // suspensao — se o dono deu play, ele quer musica de novo.
        if tocando {
            self.suspensa = false;
            self.seguidos = 0;
            self.tocava = true;
            return Passo::Quieto;
        }

        // Parou. NAO tenta adivinhar quem parou — do estado do player os dois
        // casos sao identicos, e chutar erra metade das vezes. Quem suspende e o
        // dono, explicitamente.
        let recem_disparou = self
            .ultimo_disparo
            .is_some_and(|t| agora.duration_since(t) < ESPACO);
        self.tocava = false;

        if self.suspensa {
            return Passo::Suspensa;
        }
        if self.seguidos >= TETO_SEGUIDO {
            return Passo::Desistiu;
        }
        if recem_disparou {
            return Passo::Quieto; // da tempo do app obedecer
        }
        self.seguidos += 1;
        self.ultimo_disparo = Some(agora);
        Passo::Age
    }

    /// O dono mandou pausar pela Teka. Suspende sem esperar a próxima olhada.
    pub fn dono_pausou(&mut self) {
        self.suspensa = true;
        self.tocava = false;
    }
}

/// Regras LIGADAS agora, nesta execucao.
///
/// O arquivo `dados/regras.txt` diz o **padrao**; isto diz o estado de agora. A
/// diferenca importa porque ligar uma regra e um PEDIDO, nao uma configuracao:
/// o John fala "toda vez que a musica parar, retoma" e ela passa a fazer; fala
/// "para de retomar" e ela para. Editar arquivo para isso seria pedir que ele
/// abrisse um editor no meio de uma partida.
fn ligadas() -> &'static Mutex<Vec<String>> {
    static L: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    L.get_or_init(|| {
        Mutex::new(tabela().into_iter().filter(|r| r.ligada).map(|r| r.nome).collect())
    })
}

/// Liga uma regra pelo nome. Devolve `false` se ela nao existe na tabela.
pub fn ligar(nome: &str) -> bool {
    if !tabela().iter().any(|r| r.nome == nome) {
        return false;
    }
    let mut l = ligadas().lock().unwrap();
    if !l.iter().any(|n| n == nome) {
        l.push(nome.to_string());
    }
    // Comeca limpa: uma regra recem-ligada nao herda a suspensao nem o teto de
    // disparos de quando esteve ligada antes.
    *vigia().lock().unwrap() = Vigia::default();
    true
}

/// Desliga uma regra. Devolve `false` se ela nao estava ligada.
pub fn desligar(nome: &str) -> bool {
    let mut l = ligadas().lock().unwrap();
    let antes = l.len();
    l.retain(|n| n != nome);
    antes != l.len()
}

pub fn esta_ligada(nome: &str) -> bool {
    ligadas().lock().unwrap().iter().any(|n| n == nome)
}

/// As regras que valem agora — tabela filtrada pelo estado de execucao.
pub fn ativas() -> Vec<Regra> {
    let l = ligadas().lock().unwrap().clone();
    tabela()
        .into_iter()
        .filter(|r| l.iter().any(|n| *n == r.nome))
        .collect()
}

/// O vigia do processo. Uma regra só, por enquanto — a de retomar a música.
pub fn vigia() -> &'static Mutex<Vigia> {
    static V: Mutex<Vigia> = Mutex::new(Vigia {
        ultimo_disparo: None,
        seguidos: 0,
        suspensa: false,
        tocava: false,
    });
    &V
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn le_a_tabela() {
        let r = tabela();
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].nome, "retomar_musica");
        assert_eq!(r[0].condicao, Condicao::MusicaParada);
        assert_eq!(r[0].acao, "alternar_musica");
        assert!(!r[0].ligada, "regra tem de vir DESLIGADA por padrao");
    }

    /// Toda ação da tabela tem de existir como atalho, senão a regra dispara e ela
    /// responde "nao conheco o atalho" para sempre, sozinha, sem ninguém vendo.
    #[test]
    fn toda_acao_e_um_atalho_que_existe() {
        for r in tabela() {
            assert!(
                super::super::teclado::como_de(&r.acao).is_some(),
                "a regra {:?} aciona {:?}, que nao existe",
                r.nome,
                r.acao
            );
        }
    }

    /// O caso do John: parou sem ele mexer, ela retoma.
    #[test]
    fn parou_sozinha_e_ela_retoma() {
        let mut v = Vigia::default();
        let t = t0();
        // Nunca viu tocar: primeira olhada com musica parada nao e "pausa do dono".
        assert_eq!(v.olhar(false, t), Passo::Age);
    }

    /// **A trava que importa.** Pedir pausa A ELA suspende a regra.
    #[test]
    fn o_dono_pede_pausa_e_ela_obedece() {
        let mut v = Vigia::default();
        let t = t0();
        assert_eq!(v.olhar(true, t), Passo::Quieto, "tocando, nada a fazer");
        v.dono_pausou();
        let t2 = t + Duration::from_secs(30);
        assert_eq!(v.olhar(false, t2), Passo::Suspensa);
        assert_eq!(v.olhar(false, t2 + Duration::from_secs(5)), Passo::Suspensa);
    }

    /// E volta a valer quando ele der play de novo.
    #[test]
    fn dar_play_de_novo_reativa_a_regra() {
        let mut v = Vigia::default();
        let t = t0();
        v.olhar(true, t);
        v.dono_pausou();
        assert_eq!(v.olhar(false, t + Duration::from_secs(30)), Passo::Suspensa);
        assert_eq!(v.olhar(true, t + Duration::from_secs(60)), Passo::Quieto);
        // Parou de novo, sem ele pedir: ela age.
        assert_eq!(v.olhar(false, t + Duration::from_secs(90)), Passo::Age);
    }

    /// O caso do Discord: tocava, parou sem ninguem pedir, ela retoma.
    #[test]
    fn parou_no_meio_e_ela_retoma() {
        let mut v = Vigia::default();
        let t = t0();
        assert_eq!(v.olhar(true, t), Passo::Quieto);
        assert_eq!(
            v.olhar(false, t + Duration::from_secs(30)),
            Passo::Age,
            "o Discord parou a musica; ela tem de retomar"
        );
    }

    /// Pausa pedida À TEKA também suspende — o caminho explícito.
    #[test]
    fn pausa_pedida_a_ela_tambem_suspende() {
        let mut v = Vigia::default();
        let t = t0();
        v.olhar(true, t);
        v.dono_pausou();
        assert_eq!(v.olhar(false, t + Duration::from_secs(1)), Passo::Suspensa);
    }

    /// Se a ação não resolve, ela desiste em vez de girar para sempre.
    #[test]
    fn desiste_depois_do_teto() {
        let mut v = Vigia::default();
        let mut t = t0();
        for i in 0..TETO_SEGUIDO {
            assert_eq!(v.olhar(false, t), Passo::Age, "disparo {i}");
            t += ESPACO + Duration::from_secs(1);
        }
        assert_eq!(v.olhar(false, t), Passo::Desistiu);
        assert_eq!(v.olhar(false, t + Duration::from_secs(60)), Passo::Desistiu);
    }

    /// Entre mandar e o app obedecer há uma janela. Nela ela espera, não repete.
    #[test]
    fn nao_repete_antes_do_app_obedecer() {
        let mut v = Vigia::default();
        let t = t0();
        assert_eq!(v.olhar(false, t), Passo::Age);
        assert_eq!(
            v.olhar(false, t + Duration::from_millis(500)),
            Passo::Quieto,
            "repetir antes do app reagir vira metralhadora"
        );
    }
}
