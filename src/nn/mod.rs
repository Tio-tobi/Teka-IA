//! Blocos de rede neural: cada um com forward, backward e gradientes explicitos.
//!
//! Nada de autodiff. O BPTT e escrito a mao e verificado por diferencas finitas
//! (ver `tests/gradcheck.rs`). E mais trabalho na escrita e muito menos misterio
//! na depuracao -- e permite fundir operacoes que um autodiff generico separaria.

pub mod embed;
pub mod linear;
pub mod mlp;
pub mod quant;
pub mod rmsnorm;

pub use embed::{Embed, EmbedGrad, VOCAB};
pub use linear::{Linear, LinearGrad};
pub use mlp::{BlocoMlp, BlocoMlpCache, BlocoMlpGrad};
pub use rmsnorm::{RmsNorm, RmsNormCache, RmsNormGrad};
