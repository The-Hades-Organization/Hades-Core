//! Contratos em disco compartilhados pelas ferramentas do Hades. Sem gpui.

pub mod atomic;
pub mod config;
pub mod env;
pub mod fsx;
pub mod jsonfmt;
pub mod lock;
pub mod output;
pub mod paths;
pub mod proc;
pub mod registry;
pub mod work;

pub use env::Env;
