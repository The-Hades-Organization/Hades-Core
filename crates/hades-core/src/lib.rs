//! Contratos em disco compartilhados pelas ferramentas do Hades. Sem gpui.

pub mod atomic;
pub mod config;
pub mod env;
pub mod fsx;
pub mod journal;
pub mod jsonfmt;
pub mod lock;
pub mod output;
pub mod paths;
pub mod plugins;
pub mod proc;
pub mod profile;
pub mod project;
pub mod registry;
pub mod work;

pub use env::Env;
