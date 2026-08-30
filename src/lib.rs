//! # Teka
//!
//! Agente cognitivo byte a byte, 100% local, CPU, Rust puro.
//!
//! Ver `ARQUITETURA.md` para o desenho completo. O estado atual e a **fase 0**:
//! backend numerico, recorrencia RG-LRU e a checagem de gradiente que valida tudo
//! que vier depois.

pub mod afeto;
pub mod ambiente;
pub mod backend;
pub mod coletor;
pub mod servidor;
pub mod gerador;
pub mod grammar;
pub mod learn;
pub mod memory;
pub mod model;
pub mod nn;
pub mod num;
pub mod pulso;
pub mod rng;
pub mod ssm;
pub mod tools;
