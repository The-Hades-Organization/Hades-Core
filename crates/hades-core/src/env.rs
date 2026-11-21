//! Ambiente injetável: os testes não dependem do `HOME` nem das variáveis reais do processo.

use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Env {
    vars: BTreeMap<String, String>,
}

impl Env {
    /// Ambiente vazio: nenhuma variável, nem `HOME`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Cópia das variáveis do processo.
    pub fn from_process() -> Self {
        Self {
            vars: std::env::vars_os()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
        }
    }

    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.vars.insert(key.to_owned(), value.into());
        self
    }

    /// Valor da variável; vazia conta como ausente, como o `||` do TS (`process.env.X || …`).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// Valor da variável, vazia inclusive: o `env[key] ?? …` do TS só pula a variável ausente.
    pub fn get_raw(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    /// `process.env.HOME || homedir()` (`src/lib/paths.ts:11`).
    pub fn home(&self) -> PathBuf {
        if let Some(home) = self.get("HOME") {
            return PathBuf::from(home);
        }
        std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    }
}
