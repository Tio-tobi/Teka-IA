//! O caminho por onde uma chamada vira efeito no mundo.
//!
//! Junta as três guardas, e a ordem entre elas é o desenho:
//!
//! ```text
//!   Chamada
//!      │
//!      ├─ sem efeito colateral ──────────────▶ executa direto
//!      │
//!      └─ com efeito colateral
//!             │
//!             ├─ [oficina aberta?] ──▶ redireciona a escrita para a cópia
//!             │                        (reversível: o diário não entra)
//!             │
//!             └─ [oficina fechada]  ──▶ [diário] ──▶ executa ──▶ [diário]
//!                                       iniciado              concluído
//! ```
//!
//! ## Por que a oficina dispensa o diário
//!
//! O diário existe para que um efeito **irreversível** não aconteça duas vezes.
//! Dentro da oficina nada é irreversível — `descartar` desfaz tudo. Journalizar ali
//! seria pior que inútil: a chave ficaria gravada, e depois de descartar a oficina a
//! Teka se recusaria a refazer o trabalho, alegando que "já fez". O diário guarda o
//! caminho sem volta; a oficina **é** a volta.
//!
//! ## Como a oficina redireciona
//!
//! Sem tocar nas primitivas: ela troca a **raiz da política**. `escrever_arquivo`
//! continua chamando `pol.checar_escrita`, que continua confinando — só que agora
//! confina na cópia. Uma linha de política em vez de um caminho especial dentro de
//! cada ferramenta, que é onde esse tipo de coisa costuma vazar.

use std::io;
use std::path::{Path, PathBuf};

use crate::tools::diario::{Diario, Veredito};
use crate::tools::oficina::{Mudanca, Oficina};
use crate::tools::seguranca::Politica;
use crate::tools::{Chamada, Registro};

pub struct Executor {
    diario: Diario,
    oficina: Option<Oficina>,
    politica: Politica,
    /// Pergunta antes de executar acao com efeito.
    ///
    /// Desligado por padrao porque o benchmark, o `--pedido` e o ambiente de treino
    /// nao tem ninguem para responder — e uma confirmacao sem gente do outro lado
    /// trava tudo. O REPL liga.
    confirmar: bool,
}

impl Executor {
    pub fn novo(caminho_diario: impl AsRef<Path>, politica: Politica) -> io::Result<Self> {
        Ok(Self {
            diario: Diario::abrir(caminho_diario.as_ref().to_path_buf())?,
            oficina: None,
            politica,
            confirmar: false,
        })
    }

    /// Liga a confirmacao. Chamar so onde ha gente para responder.
    pub fn com_confirmacao(mut self, sim: bool) -> Self {
        self.confirmar = sim;
        self
    }

    /// Pergunta, e devolve `false` se a pessoa recusou.
    ///
    /// Falha FECHADA: se a entrada acabou (pipe, EOF, terminal fechado), a resposta e
    /// nao. O contrario — assumir "sim" quando ninguem respondeu — seria transformar
    /// um script sem terminal em consentimento.
    fn pedir_permissao(&self, nome: &str, c: &Chamada) -> bool {
        use std::io::{BufRead, IsTerminal, Write};

        // SEM TERMINAL, SEM PERGUNTA — e sem permissao.
        //
        // Isto nao e detalhe: `read_line` num cano ABERTO e vazio nao devolve EOF,
        // ele BLOQUEIA. Um servico, um cron ou um `cargo test` ficariam pendurados
        // para sempre esperando uma resposta que nunca vem. Foi assim que o teste
        // desta funcao travou e mostrou o defeito.
        //
        // Recusar e a leitura certa: se nao ha terminal, nao ha ninguem para
        // autorizar, e agir seria transformar ausencia de gente em consentimento.
        if !io::stdin().is_terminal() {
            eprintln!("    {nome}: recusado — acao com efeito exige confirmacao, e nao ha terminal");
            return false;
        }
        let args: Vec<String> = c
            .args
            .iter()
            .map(|(k, v)| format!("{k}={v:?}"))
            .collect();
        print!("    {nome} {} — faz? [s/N] ", args.join(" "));
        let _ = io::stdout().flush();
        let mut linha = String::new();
        match io::stdin().lock().read_line(&mut linha) {
            Ok(0) | Err(_) => false,
            Ok(_) => {
                let r = linha.trim().to_lowercase();
                r == "s" || r == "sim" || r == "y"
            }
        }
    }

    pub fn politica(&self) -> &Politica {
        &self.politica
    }

    pub fn tem_oficina(&self) -> bool {
        self.oficina.is_some()
    }

    /// Abre uma oficina sobre `origem`. Enquanto estiver aberta, toda escrita cai
    /// na cópia.
    pub fn abrir_oficina(
        &mut self,
        origem: impl AsRef<Path>,
        destino: impl AsRef<Path>,
    ) -> io::Result<PathBuf> {
        if self.oficina.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "já existe uma oficina aberta; aplique ou descarte antes",
            ));
        }
        let of = Oficina::abrir(origem, destino)?;
        let raiz = of.raiz().to_path_buf();
        self.oficina = Some(of);
        Ok(raiz)
    }

    pub fn diff(&self) -> io::Result<Vec<Mudanca>> {
        match &self.oficina {
            Some(of) => of.diff(),
            None => Ok(Vec::new()),
        }
    }

    /// Integra as mudanças na pasta real e fecha a oficina.
    pub fn aplicar_oficina(&mut self, backup: &Path) -> io::Result<Vec<Mudanca>> {
        match self.oficina.take() {
            Some(mut of) => {
                let m = of.aplicar(backup)?;
                let _ = of.descartar();
                Ok(m)
            }
            None => Ok(Vec::new()),
        }
    }

    /// Joga a cópia fora. A pasta real nunca foi tocada.
    pub fn descartar_oficina(&mut self) -> io::Result<()> {
        match self.oficina.take() {
            Some(of) => of.descartar(),
            None => Ok(()),
        }
    }

    /// A política que vale agora: dentro da oficina, a raiz é a cópia.
    fn politica_efetiva(&self) -> Politica {
        match &self.oficina {
            Some(of) => Politica {
                raiz: Some(of.raiz().to_path_buf()),
                modo: crate::tools::seguranca::Modo::Real,
                ..self.politica.clone()
            },
            None => self.politica.clone(),
        }
    }

    /// Esta chamada vai parar para pedir permissão?
    ///
    /// Espelha exatamente a condição de [`Executor::executar`] — e existe porque
    /// quem não tem terminal precisa saber disso **antes** de executar, para poder
    /// perguntar por outro canal. Ver [`crate::servidor`].
    ///
    /// Se as duas condições saírem de sincronia, o servidor passa a mostrar um
    /// "faz?" para uma ação que executaria direto, ou pior, executa direto uma que
    /// deveria perguntar. O teste `precisa_confirmar_espelha_o_executar` prende as
    /// duas juntas.
    pub fn precisa_confirmar(&self, reg: &Registro, c: &Chamada) -> bool {
        let Some(f) = reg.ferramentas.get(c.ferramenta) else {
            return false;
        };
        self.confirmar && f.prim.pede_cerimonia() && self.oficina.is_none()
    }

    /// Executa uma chamada, com as guardas que ela merecer.
    pub fn executar(&mut self, reg: &Registro, c: &Chamada) -> Result<String, String> {
        self.executar_com_permissao(reg, c, false)
    }

    /// A mesma coisa, mas quem chama pode afirmar que a permissão **já foi dada**.
    ///
    /// `ja_autorizado` só é verdade quando um humano respondeu "sim" a esta chamada
    /// específica, identificada, por um canal que não é o terminal. É assim que a
    /// interface web consegue agir sem que `IsTerminal` seja afrouxado.
    ///
    /// **O que isto NÃO afrouxa:** sandbox, lista negra, raiz, diário e oficina
    /// continuam todos valendo depois daqui. A permissão pula a pergunta, não as
    /// guardas — quem responde "sim" para `apagar_arquivo` fora da raiz continua
    /// recebendo uma recusa.
    pub fn executar_com_permissao(
        &mut self,
        reg: &Registro,
        c: &Chamada,
        ja_autorizado: bool,
    ) -> Result<String, String> {
        let f = reg
            .ferramentas
            .get(c.ferramenta)
            .ok_or_else(|| format!("ferramenta {} nao existe", c.ferramenta))?;
        let pol = self.politica_efetiva();

        // Sem cerimonia devida, ou dentro da oficina (que é reversível): direto.
        //
        // `pede_cerimonia` e nao `efeito_colateral` por causa do `Reversivel`: um
        // atalho de teclado muda o mundo, mas nao pode passar pelo diario — ele
        // deduplica, e o segundo "proxima musica" viraria "ja tinha sido feito".
        if !f.prim.pede_cerimonia() || self.oficina.is_some() {
            return reg.executar(c, &pol);
        }

        // Daqui para baixo a acao deixa marca. As outras guardas do projeto —
        // sandbox, raiz, oficina, diario — sao todas tudo-ou-nada: passado o
        // `--real`, ela agia sem perguntar. Esta e a unica que dá a chance de dizer
        // nao no caso concreto, olhando o argumento que ela escolheu.
        if self.confirmar && !ja_autorizado && !self.pedir_permissao(&f.nome, c) {
            return Err("nao autorizado".into());
        }

        let chave = Diario::chave(&f.nome, &c.args);
        match self.diario.consultar(&chave) {
            Veredito::Repetir(recibo) => {
                // Não reexecuta. Devolver o recibo é o comportamento correto: o
                // efeito pedido já está no mundo.
                Ok(format!("{recibo}\n  (ja tinha sido feito; nao repeti)"))
            }
            Veredito::Incerto => Err(format!(
                "'{}' comecou antes e nao se sabe se terminou. \
                 Nao vou repetir as cegas — confira e, se precisar, apague a linha \
                 correspondente do diario.",
                f.nome
            )),
            // Falhou antes: deixa repetir, mas conta o que houve. Bloquear aqui
            // transformaria rede fora ou consulta sem resultado em travamento
            // permanente, e o unico recurso do dono seria editar o log a mao.
            Veredito::FalhouAntes(erro) => {
                eprintln!("    (a tentativa anterior falhou: {erro})");
                self.diario
                    .iniciar(&chave)
                    .map_err(|e| format!("nao consegui registrar no diario: {e}"))?;
                let saida = reg.executar(c, &pol);
                match &saida {
                    Ok(texto) => {
                        let _ = self.diario.concluir(&chave, texto);
                    }
                    Err(e) => {
                        let _ = self.diario.falhar(&chave, e);
                    }
                }
                saida
            }
            Veredito::Executar => {
                // A ordem é a garantia: registrar e sincronizar ANTES de agir.
                self.diario
                    .iniciar(&chave)
                    .map_err(|e| format!("nao consegui registrar no diario: {e}"))?;
                let saida = reg.executar(c, &pol);
                match &saida {
                    // Falha em GRAVAR nao invalida o efeito, que ja aconteceu; so
                    // deixa a entrada como `Incerto`, que e o lado seguro.
                    Ok(texto) => {
                        let _ = self.diario.concluir(&chave, texto);
                    }
                    // A OPERACAO falhou: o efeito nao esta no mundo. Registrar isso
                    // e o que separa "falhou, tente de novo" de "travou para sempre".
                    Err(e) => {
                        let _ = self.diario.falhar(&chave, e);
                    }
                }
                saida
            }
        }
    }

    pub fn quantas_no_diario(&self) -> usize {
        self.diario.quantas()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::seguranca::Modo;
    use std::fs;

    fn temporaria(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("teka_exec_{nome}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn chamada(reg: &Registro, nome: &str, args: &[(&str, &str)]) -> Chamada {
        Chamada {
            ferramenta: reg.indice(nome).unwrap(),
            args: args
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    #[test]
    fn escrever_duas_vezes_so_escreve_uma() {
        let base = temporaria("idempotente");
        let reg = Registro::padrao();
        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(base.clone()),
            ..Default::default()
        };
        let mut ex = Executor::novo(base.join("diario.log"), pol).unwrap();
        let c = chamada(&reg, "escrever_arquivo", &[("caminho", "a.txt"), ("texto", "oi")]);

        let primeira = ex.executar(&reg, &c).unwrap();
        assert!(!primeira.contains("nao repeti"));
        let segunda = ex.executar(&reg, &c).unwrap();
        assert!(
            segunda.contains("nao repeti"),
            "a segunda tinha de ser recusada: {segunda}"
        );
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn ler_pode_repetir_a_vontade_e_nao_entra_no_diario() {
        let base = temporaria("leitura");
        let reg = Registro::padrao();
        let mut ex = Executor::novo(base.join("d.log"), Politica::default()).unwrap();
        let c = chamada(&reg, "hora", &[]);
        for _ in 0..3 {
            let r = ex.executar(&reg, &c).unwrap();
            assert!(!r.contains("nao repeti"));
        }
        assert_eq!(
            ex.quantas_no_diario(),
            0,
            "ferramenta sem efeito colateral nao pode custar fsync"
        );
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn dentro_da_oficina_a_escrita_vai_para_a_copia() {
        let base = temporaria("oficina");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("existente.txt"), "original").unwrap();

        let reg = Registro::padrao();
        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(real.clone()),
            ..Default::default()
        };
        let mut ex = Executor::novo(base.join("d.log"), pol).unwrap();
        ex.abrir_oficina(&real, base.join("of")).unwrap();

        ex.executar(
            &reg,
            &chamada(&reg, "escrever_arquivo", &[("caminho", "novo.txt"), ("texto", "x")]),
        )
        .unwrap();

        // A pasta real continua intacta.
        assert!(!real.join("novo.txt").exists(), "a real nao podia ser tocada");
        let d = ex.diff().unwrap();
        assert_eq!(d.len(), 1, "{d:?}");

        // Aplicar leva para a real, com backup.
        ex.aplicar_oficina(&base.join("bkp")).unwrap();
        assert!(real.join("novo.txt").exists());
        assert!(!ex.tem_oficina());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn descartar_desfaz_e_permite_refazer() {
        let base = temporaria("descarte");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        let reg = Registro::padrao();
        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(real.clone()),
            ..Default::default()
        };
        let mut ex = Executor::novo(base.join("d.log"), pol).unwrap();
        let c = chamada(&reg, "escrever_arquivo", &[("caminho", "t.txt"), ("texto", "y")]);

        ex.abrir_oficina(&real, base.join("of1")).unwrap();
        ex.executar(&reg, &c).unwrap();
        ex.descartar_oficina().unwrap();
        assert!(!real.join("t.txt").exists());

        // O ponto: o diario NAO pode ter guardado a tentativa descartada, senao
        // refazer seria recusado por "ja fiz" — e nada foi feito.
        assert_eq!(ex.quantas_no_diario(), 0);
        let r = ex.executar(&reg, &c).unwrap();
        assert!(!r.contains("nao repeti"), "refazer depois de descartar tem de valer");
        assert!(real.join("t.txt").exists());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn nao_abre_duas_oficinas() {
        let base = temporaria("duas");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        let mut ex = Executor::novo(base.join("d.log"), Politica::default()).unwrap();
        ex.abrir_oficina(&real, base.join("a")).unwrap();
        assert!(ex.abrir_oficina(&real, base.join("b")).is_err());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_denylist_continua_valendo_dentro_da_oficina() {
        let base = temporaria("deny");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        let reg = Registro::padrao();
        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(real.clone()),
            ..Default::default()
        };
        let mut ex = Executor::novo(base.join("d.log"), pol).unwrap();
        ex.abrir_oficina(&real, base.join("of")).unwrap();
        // A oficina protege contra ERRO, nao contra malicia — as outras guardas
        // continuam sendo as que impedem isto.
        let r = ex.executar(
            &reg,
            &chamada(&reg, "executar_comando", &[("comando", "format c:")]),
        );
        assert!(r.is_err(), "denylist tem de valer dentro da oficina tambem");
        fs::remove_dir_all(&base).ok();
    }

    /// `precisa_confirmar` e `executar` têm de concordar sobre o que pede permissão.
    ///
    /// Se saírem de sincronia, o servidor mostra um "faz?" para algo que executaria
    /// direto — ou, muito pior, executa direto algo que deveria perguntar. Percorre
    /// o registro inteiro em vez de amostrar: é barato e não deixa ferramenta nova
    /// entrar sem passar por aqui.
    #[test]
    fn precisa_confirmar_espelha_o_executar() {
        let reg = Registro::padrao();
        let base = temporaria("espelho");
        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(base.clone()),
            ..Default::default()
        };
        let ex = Executor::novo(base.join("d.log"), pol)
            .unwrap()
            .com_confirmacao(true);

        for (i, f) in reg.ferramentas.iter().enumerate() {
            let c = Chamada { ferramenta: i, args: Vec::new() };
            assert_eq!(
                ex.precisa_confirmar(&reg, &c),
                f.prim.pede_cerimonia(),
                "{}: precisa_confirmar discorda do efeito colateral",
                f.nome
            );
        }

        // Com a confirmacao desligada, ninguem pergunta nada.
        let sem = Executor::novo(base.join("d2.log"), Politica::default())
            .unwrap()
            .com_confirmacao(false);
        for i in 0..reg.n() {
            let c = Chamada { ferramenta: i, args: Vec::new() };
            assert!(!sem.precisa_confirmar(&reg, &c));
        }
        fs::remove_dir_all(&base).ok();
    }

    /// A permissao pula a PERGUNTA, nao as guardas.
    ///
    /// Um "sim" na pagina nao pode virar passe livre: sandbox, lista negra e raiz
    /// continuam valendo depois dele.
    #[test]
    fn autorizado_nao_fura_as_outras_guardas() {
        let reg = Registro::padrao();
        let base = temporaria("autorizado");
        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(base.clone()),
            ..Default::default()
        };
        let mut ex = Executor::novo(base.join("d.log"), pol)
            .unwrap()
            .com_confirmacao(true);

        // Fora da raiz, mesmo autorizado.
        let fuga = chamada(&reg, "escrever_arquivo", &[("caminho", "..\\..\\fuga.txt"), ("texto", "x")]);
        assert!(ex.executar_com_permissao(&reg, &fuga, true).is_err(), "escapou da raiz");

        // Na lista negra, mesmo autorizado.
        let destrutivo = chamada(&reg, "executar_comando", &[("comando", "format c:")]);
        assert!(
            ex.executar_com_permissao(&reg, &destrutivo, true).is_err(),
            "a lista negra parou de valer com autorizacao"
        );
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn sem_ninguem_para_responder_nao_e_consentimento() {
        // A confirmacao falha FECHADA. Em `cargo test` a entrada padrao nao e
        // terminal, entao `read_line` devolve EOF — e EOF tem de virar "nao".
        //
        // O contrario seria o pior tipo de defeito de seguranca: um script sem
        // terminal, um servico, um cron, todos passariam a apagar sem que ninguem
        // tivesse dito sim uma vez.
        let base = temporaria("confirma_eof");
        let reg = Registro::padrao();
        let alvo = base.join("nao_apague.txt");
        std::fs::write(&alvo, b"fica").unwrap();

        let pol = Politica {
            modo: Modo::Real,
            raiz: Some(base.clone()),
            ..Default::default()
        };
        let mut ex = Executor::novo(base.join("d.log"), pol)
            .unwrap()
            .com_confirmacao(true);
        let c = chamada(&reg, "apagar_arquivo", &[("caminho", "nao_apague.txt")]);
        let r = ex.executar(&reg, &c);

        assert!(r.is_err(), "sem resposta deveria recusar, veio {r:?}");
        assert!(alvo.exists(), "o arquivo foi apagado sem ninguem autorizar");
    }

    #[test]
    fn ferramenta_sem_efeito_nao_pergunta() {
        // Se `hora` pedisse confirmacao, a pessoa aprenderia a apertar "s" sem ler —
        // que e como toda confirmacao morre. Aqui isso e testado, nao prometido: com
        // a confirmacao LIGADA e sem ninguem para responder, `hora` tem de passar.
        let base = temporaria("confirma_sem_efeito");
        let reg = Registro::padrao();
        let mut ex = Executor::novo(base.join("d.log"), Politica::default())
            .unwrap()
            .com_confirmacao(true);
        let c = chamada(&reg, "hora", &[]);
        assert!(ex.executar(&reg, &c).is_ok(), "hora nao deveria pedir permissao");
    }
}
