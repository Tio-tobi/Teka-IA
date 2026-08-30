//! Aprendizado: otimizador, laco de treino, e (nas fases seguintes) consolidacao,
//! reforco e afeto.

pub mod adam;
pub mod consolidacao;
pub mod reforco;
pub mod dados;
pub mod supervisionado;
pub mod train;

pub use adam::Adam;
pub use dados::{dividir, dividir_por_frase, gerar, Exemplo};
pub use reforco::{treinar_por_reforco, CfgReforco, Pesos};
pub use consolidacao::{consolidar, CfgConsolidacao};
pub use supervisionado::{treinar_agente, CfgSup};
pub use train::{avaliar, treinar, CfgTreino, Fluxo, Relatorio, LN2};
