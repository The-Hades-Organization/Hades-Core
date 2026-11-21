//! Runner de paridade TS × Rust: a mesma fixture roda no `hades <tool>` (Bun, da skill) e no binário Rust, cada lado
//! num sandbox próprio, e a saída normalizada é comparada em bytes.

pub mod cases;
pub mod compare;
pub mod normalize;
pub mod runner;
pub mod sandbox;
