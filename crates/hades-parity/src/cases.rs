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
    /// Ausente = caso comum (mesmo `argv` nos dois lados). `cross` = roundtrip entre os lados (`produce`/`consume`).
    pub kind: Option<String>,
    /// Pasta em `<pasta do cases.toml>/fixtures/` copiada para o HOME de cada lado; ausente = HOME vazio.
    /// Nome e conteúdo (UTF-8) aceitam `@HOME@` (HOME do lado) e `@ENC:<caminho>@` (caminho codificado como a
    /// pasta de transcripts do Claude Code; o `<caminho>` pode conter `@HOME@`).
    pub fixture: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    /// `kind = "cross"`: argv do passo que grava o artefato (aceita `{xfer}`, a pasta compartilhada entre os lados).
    #[serde(default)]
    pub produce: Vec<String>,
    /// `kind = "cross"`: argv do passo que lê o artefato (aceita `{xfer}`), rodado num HOME limpo.
    #[serde(default)]
    pub consume: Vec<String>,
    /// `kind = "cross"`: fixture do HOME do consumidor; ausente = HOME vazio.
    pub consume_fixture: Option<String>,
    /// Normalizações padrão a desligar neste caso. Aceita: `iso` (mantém o instante ISO em vez de `<ts>`).
    #[serde(default)]
    pub keep: Vec<String>,
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

pub const KEEP_ISO: &str = "iso";

impl Case {
    pub fn is_cross(&self) -> bool {
        self.kind.as_deref() == Some("cross")
    }

    pub fn keeps(&self, what: &str) -> bool {
        self.keep.iter().any(|k| k == what)
    }

    fn validate(&self) -> Result<(), String> {
        match self.kind.as_deref() {
            None | Some("cross") => {}
            Some(other) => return Err(format!("{}: kind desconhecido: {other}", self.id)),
        }
        if let Some(k) = self.keep.iter().find(|k| *k != KEEP_ISO) {
            return Err(format!("{}: keep desconhecido: {k}", self.id));
        }
        if self.is_cross() {
            if self.produce.is_empty() || self.consume.is_empty() {
                return Err(format!(
                    "{}: kind = \"cross\" exige produce e consume",
                    self.id
                ));
            }
            if !self.argv.is_empty() {
                return Err(format!("{}: kind = \"cross\" não usa argv", self.id));
            }
        } else if !self.produce.is_empty()
            || !self.consume.is_empty()
            || self.consume_fixture.is_some()
        {
            return Err(format!(
                "{}: produce, consume e consume_fixture só valem com kind = \"cross\"",
                self.id
            ));
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<CasesFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: CasesFile = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    file.case.iter().try_for_each(Case::validate)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<CasesFile, String> {
        let file: CasesFile = toml::from_str(text).map_err(|e| e.to_string())?;
        file.case.iter().try_for_each(Case::validate)?;
        Ok(file)
    }

    #[test]
    fn keep_iso_aceito() {
        let f = parse("[[case]]\nid='a'\nargv=['x']\nkeep=['iso']").unwrap();
        assert!(f.case[0].keeps("iso"));
        assert!(!f.case[0].keeps("outro"));
    }

    #[test]
    fn keep_desconhecido_e_erro() {
        let e = parse("[[case]]\nid='a'\nargv=['x']\nkeep=['uuid']").unwrap_err();
        assert!(e.contains("keep desconhecido: uuid"), "{e}");
    }

    #[test]
    fn cross_valido() {
        let f = parse("[[case]]\nid='c'\nkind='cross'\nproduce=['p']\nconsume=['c']").unwrap();
        assert!(f.case[0].is_cross());
    }

    #[test]
    fn cross_exige_produce_e_consume() {
        let e = parse("[[case]]\nid='c'\nkind='cross'\nproduce=['p']").unwrap_err();
        assert!(e.contains("exige produce e consume"), "{e}");
    }

    #[test]
    fn cross_nao_aceita_argv() {
        let e = parse("[[case]]\nid='c'\nkind='cross'\nargv=['x']\nproduce=['p']\nconsume=['c']")
            .unwrap_err();
        assert!(e.contains("não usa argv"), "{e}");
    }

    #[test]
    fn produce_fora_de_cross_e_erro() {
        let e = parse("[[case]]\nid='a'\nargv=['x']\nproduce=['p']").unwrap_err();
        assert!(e.contains("só valem com kind"), "{e}");
    }

    #[test]
    fn kind_desconhecido_e_erro() {
        let e = parse("[[case]]\nid='a'\nkind='zzz'").unwrap_err();
        assert!(e.contains("kind desconhecido"), "{e}");
    }
}
