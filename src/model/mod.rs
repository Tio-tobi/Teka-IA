//! O modelo hierarquico: encoder local, patcher, backbone, decoder local, cabecas.

pub mod agente;
pub mod confianca;
pub mod heads;
pub mod hierarchy;
pub mod io;
pub mod ngrama;
pub mod patcher;
pub mod safetensors;
pub mod stack;

pub use agente::{Agente, AgenteCache, AgenteGrad};
pub use heads::{Alvo, Cabecas, CabecasCache, Placar, MAX_SLOTS};
pub use hierarchy::{Config, Estado, Teka, TekaCache, TekaGrad};
pub use patcher::{Fixo, Patcher, Plano, PorClasse, PorPalavra, SEM_PATCH};
pub use stack::{Pilha, PilhaCache, PilhaGrad};
