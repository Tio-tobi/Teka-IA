//! Modelos de espaco de estados (SSM) -- o nucleo recorrente da Teka.

pub mod block;
pub mod rglru;

pub use block::{BlocoRec, BlocoRecCache, BlocoRecGrad};
pub use rglru::{RgLru, RgLruCache, RgLruGrad, C_DECAY};
