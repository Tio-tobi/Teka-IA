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
}

impl Executor {
    pub fn novo(caminho_diario: impl AsRef<Path>, politica: Politica) -> io::Result<Self> {
        Ok(Self {
            diario: Diario::abrir(caminho_diario.as_ref().to_path_buf())?,
            oficina: None,
            politica,
        })
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

    /// Executa uma chamada, com as guardas que ela merecer.
    pub fn executar(&mut self, reg: &Registro, c: &Chamada) -> Result<String, String> {
        let f = reg
            .ferramentas
            .get(c.ferramenta)
            .ok_or_else(|| format!("ferramenta {} nao existe", c.ferramenta))?;
        let pol = self.politica_efetiva();

        // Sem efeito no mundo, ou dentro da oficina (que é reversível): direto.
        if !f.prim.efeito_colateral() || self.oficina.is_some() {
            return reg.executar(c, &pol);
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
            Veredito::Executar => {
                // A ordem é a garantia: registrar e sincronizar ANTES de agir.
                self.diario
                    .iniciar(&chave)
                    .map_err(|e| format!("nao consegui registrar no diario: {e}"))?;
                let saida = reg.executar(c, &pol);
                if let Ok(texto) = &saida {
                    // Falha em concluir não invalida o efeito, que já aconteceu; só
                    // deixa a entrada como `Incerto`, que é o lado seguro.
                    let _ = self.diario.concluir(&chave, texto);
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
}
