//! Erros que se explicam (`src/lib/human-errors.ts`): do código estável ao bloco de texto para humano.
//!
//! O catálogo (`src/lib/human-errors-catalog.ts`) vai embutido no binário: `data/human-errors.json` é gerado por
//! `scripts/gen-human-errors.ts` a partir do TS, sem leitura em runtime da skill.

use std::collections::HashMap;
use std::sync::OnceLock;

const CATALOG_JSON: &str = include_str!("../data/human-errors.json");

pub const ERRORS_DOC: &str = "reference/erros.md";

/// `CLI_ERRORS` (`src/lib/human-errors-catalog.ts:26`).
pub mod cli_codes {
    pub const UNKNOWN_COMMAND: &str = "cli_unknown_command";
    pub const USAGE: &str = "cli_usage";
    pub const FAILED: &str = "cli_failed";
    pub const NO_PROFILE: &str = "cli_no_profile";
    pub const OUTSIDE_REPO: &str = "cli_outside_repo";
    pub const MISSING_DEPENDENCY: &str = "cli_missing_dependency";
    pub const UNEXPECTED: &str = "cli_unexpected";
    pub const GATE_NOT_ADMITTED: &str = "gate_not_admitted";
}

/// `HumanError` + `code` e `known` (`Explanation`, `src/lib/human-errors.ts:13`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub code: String,
    pub known: bool,
    pub title: String,
    pub cause: String,
    pub action: String,
    pub reference: String,
}

struct Entry {
    title: String,
    cause: String,
    action: String,
    reference: String,
}

fn catalog() -> &'static HashMap<String, Entry> {
    static CATALOG: OnceLock<HashMap<String, Entry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(CATALOG_JSON).expect("data/human-errors.json inválido");
        let text =
            |row: &serde_json::Value, k: &str| row[k].as_str().unwrap_or_default().to_owned();
        rows.iter()
            .map(|row| {
                (
                    text(row, "code"),
                    Entry {
                        title: text(row, "title"),
                        cause: text(row, "cause"),
                        action: text(row, "action"),
                        reference: text(row, "ref"),
                    },
                )
            })
            .collect()
    })
}

/// `explain` (`src/lib/human-errors.ts:35`): entrada do catálogo ou o texto de fallback (nunca falha).
pub fn explain(code: Option<&str>) -> Explanation {
    let key = code.unwrap_or(cli_codes::FAILED);
    match catalog().get(key) {
        Some(e) => Explanation {
            code: key.to_owned(),
            known: true,
            title: e.title.clone(),
            cause: e.cause.clone(),
            action: e.action.clone(),
            reference: e.reference.clone(),
        },
        None => Explanation {
            code: key.to_owned(),
            known: false,
            title: "Esse erro ainda não tem explicação própria".to_owned(),
            cause: format!(
                "A Hades registrou a falha com o código `{key}`, mas ninguém escreveu ainda o que ele significa. A mensagem original (acima) é o que se sabe."
            ),
            action: "Rode `hades doctor` para ver se falta algo na máquina e `hades explain <código>` para procurar outro erro parecido.".to_owned(),
            reference: format!("{ERRORS_DOC} §Como acrescentar uma entrada"),
        },
    }
}

/// `knownCodes` (`src/lib/human-errors.ts:42`): códigos do catálogo em ordem alfabética.
pub fn known_codes() -> Vec<String> {
    let mut codes: Vec<String> = catalog().keys().cloned().collect();
    codes.sort();
    codes
}

/// `renderCli` (`src/lib/human-errors.ts:51`): a mensagem original na 1ª linha e o bloco do catálogo depois, sem `\n`
/// final. `prefix` é `hades` no TS; cada ferramenta passa o próprio nome.
pub fn render_cli(message: &str, code: Option<&str>, prefix: &str) -> String {
    let e = explain(code);
    [
        format!("{prefix}: {message}"),
        format!("  Por quê:      {}", e.cause),
        format!("  O que fazer:  {}", e.action),
        format!("  Saiba mais:   {}", e.reference),
        format!("  (código: {})", e.code),
    ]
    .join("\n")
}

/// `renderHook` (`src/lib/human-errors.ts:62`), com `\n` final. `action` troca a ação do catálogo.
pub fn render_hook(message: &str, code: &str, action: Option<&str>) -> String {
    let e = explain(Some(code));
    format!(
        "{message}\n  Por quê:                {}\n  O que fazer no lugar:   {}\n  Saiba mais:             {}\n  (código: {})\n",
        e.cause,
        action.unwrap_or(&e.action),
        e.reference,
        e.code
    )
}

/// `renderEntry` (`src/lib/human-errors.ts:86`): saída de `hades explain <código>`.
pub fn render_entry(code: &str) -> String {
    let e = explain(Some(code));
    format!(
        "{}\n\n  Código:       {}\n  Por quê:      {}\n  O que fazer:  {}\n  Saiba mais:   {}",
        e.title, e.code, e.cause, e.action, e.reference
    )
}

/// `messageOfCliFailure` (`src/lib/human-errors.ts:74`): a mensagem de um comando que falhou, dado o stderr dele.
pub fn message_of_cli_failure(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();
    let Some(last) = lines.len().checked_sub(1) else {
        return String::new();
    };
    if !is_code_line(lines[last]) {
        return lines[last].to_owned();
    }
    let mut i = last;
    while i > 0 && lines[i].starts_with(char::is_whitespace) {
        i -= 1;
    }
    lines[i].to_owned()
}

/// `CODE_LINE` (`src/lib/human-errors.ts:70`): `^\s*\(código: [^)]+\)\s*$`.
fn is_code_line(line: &str) -> bool {
    let t = line.trim();
    t.strip_prefix("(código: ")
        .and_then(|r| r.strip_suffix(')'))
        .is_some_and(|inner| !inner.is_empty() && !inner.contains(')'))
}

/// `cliCodeOfExit` (`src/lib/human-errors.ts:92`): código padrão de uma saída não-zero sem código próprio.
pub fn cli_code_of_exit(exit: i32) -> &'static str {
    match exit {
        2 => cli_codes::USAGE,
        3 => cli_codes::NO_PROFILE,
        4 => cli_codes::OUTSIDE_REPO,
        69 => cli_codes::MISSING_DEPENDENCY,
        75 => cli_codes::GATE_NOT_ADMITTED,
        _ => cli_codes::FAILED,
    }
}

/// `HadesError` → bloco de stderr, como o `catch` do `cli.ts:146`: código próprio, senão o da classe da saída.
pub fn render_error(error: &crate::output::HadesError, prefix: &str) -> String {
    let code = error
        .error_code
        .as_deref()
        .unwrap_or_else(|| cli_code_of_exit(error.code));
    render_cli(&error.message, Some(code), prefix)
}

/// `distance` (`src/lib/human-errors.ts:107`): Levenshtein com duas linhas.
fn distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut row = vec![i];
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            row.push((prev[j] + 1).min(row[j - 1] + 1).min(sub));
        }
        prev = row;
    }
    prev[b.len()]
}

/// `suggestName` (`src/lib/human-errors.ts:122`): o subcomando mais parecido (até 3 edições e menos que o tamanho
/// digitado, com mínimo 2), o 1º em caso de empate.
pub fn suggest_name<'a>(typed: &str, names: &[&'a str]) -> Option<&'a str> {
    let typed_chars: Vec<char> = typed.to_lowercase().chars().collect();
    let length = typed.encode_utf16().count().max(2);
    let mut best: Option<(&str, usize)> = None;
    for name in names {
        let name_chars: Vec<char> = name.chars().collect();
        let d = distance(&typed_chars, &name_chars);
        if d <= 3 && d < length && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((name, d));
        }
    }
    best.map(|(n, _)| n)
}

#[cfg(test)]
mod tests;
