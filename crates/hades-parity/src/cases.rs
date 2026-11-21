//! `cases.toml`: a matriz de paridade de um repo.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct CasesFile {
    #[serde(default)]
    pub case: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    /// Pasta em `<pasta do cases.toml>/fixtures/` copiada para o HOME de cada lado; ausente = HOME vazio.
    /// Nome e conteúdo (UTF-8) aceitam `@HOME@` (HOME do lado) e `@ENC:<caminho>@` (caminho codificado como a
    /// pasta de transcripts do Claude Code; o `<caminho>` pode conter `@HOME@`).
    pub fixture: Option<String>,
    pub argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Relativo ao HOME. `repo` ganha `git init` com remote fixo.
    pub cwd: Option<String>,
    /// `stdout`, `stderr`, `exit`, `json` (stdout por valor) e `files:<glob>`.
    #[serde(default = "default_compare")]
    pub compare: Vec<String>,
    #[serde(default)]
    pub stdin: String,
    /// Motivo: o caso roda e reporta, mas não falha a execução.
    pub pending: Option<String>,
    /// Entrada do lado TS no lugar de `<skill>/src/cli.ts`; aceita `{skill}` e `{cases_dir}`.
    /// Script (relativo à pasta do `cases.toml`) rodado com `sh` nos dois lados, no HOME, depois da fixture e do
    /// `cwd`. Recebe `SIDE=ts|rust`.
    pub setup: Option<String>,
    /// Como `setup`, só no lado TS (roda depois do comum).
    pub setup_ts: Option<String>,
    /// Como `setup`, só no lado Rust (roda depois do comum).
    pub setup_rust: Option<String>,
    /// Troca toda data `YYYY-MM-DD` por `<date>` na saída e nos `files:` dos dois lados.
    #[serde(default)]
    pub normalize_dates: bool,
    pub ts_cmd: Option<Vec<String>>,
}

fn default_compare() -> Vec<String> {
    vec!["stdout".to_owned(), "exit".to_owned()]
}

pub fn load(path: &Path) -> Result<CasesFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
