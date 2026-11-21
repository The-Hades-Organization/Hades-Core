//! Mascaramento de segredo e corte de resumo (`src/lib/journal.ts:86-115`).

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::time::is_js_space;

const MASK: &str = "***";

/// Classes do JS: `\w` e `\b` ASCII, `\s` com o conjunto do ECMAScript, `i` sem dobra Unicode.
const WS: &str = r"\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";
const WORD: &str = r"(?-u:\w)";
const BOUND: &str = r"(?-u:\b)";

enum Replace {
    All,
    Group1Then(&'static str),
    Groups12,
    Group1AtSign,
}

struct Rule {
    regex: Regex,
    replace: Replace,
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("regra de segredo válida")
}

/// As seis regras de `SECRET_RULES` (`src/lib/journal.ts:89-96`), na mesma ordem.
static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let keyword5 = "(?i-u:token|secret|passw(?:or)?d|passwd|senha|api-?key|credential)";
    let keyword6 =
        "(?i-u:token|secret|passw(?:or)?d|passwd|senha|api_?key|apikey|credential|private_?key)";
    vec![
        Rule {
            regex: compile(&format!(
                "{BOUND}(?:gh[pousr]_[A-Za-z0-9]{{20,}}|github_pat_[A-Za-z0-9_]{{20,}}|sk-[A-Za-z0-9_-]{{16,}}|xox[abprs]-[A-Za-z0-9-]{{10,}}|AKIA[0-9A-Z]{{16}}|AIza[0-9A-Za-z_-]{{30,}})"
            )),
            replace: Replace::All,
        },
        Rule {
            regex: compile(&format!(
                r"{BOUND}eyJ[A-Za-z0-9_-]{{8,}}\.[A-Za-z0-9_-]{{8,}}\.[A-Za-z0-9_-]{{4,}}"
            )),
            replace: Replace::All,
        },
        Rule {
            regex: compile(&format!(
                "{BOUND}((?i-u:Bearer|Basic))[{WS}]+(?i-u:[A-Za-z0-9._~+/=-]){{8,}}"
            )),
            replace: Replace::Group1Then(" "),
        },
        Rule {
            regex: compile(&format!(r"(://[^{WS}:/@]+:)[^{WS}@/]+@")),
            replace: Replace::Group1AtSign,
        },
        Rule {
            regex: compile(&format!(
                r#"(--?(?:{WORD}|-)*{keyword5}(?:{WORD}|-)*)(=|[{WS}]+)(?:"[^"]*"|'[^']*'|[^{WS}]+)"#
            )),
            replace: Replace::Groups12,
        },
        Rule {
            regex: compile(&format!(
                r#"{BOUND}((?:{WORD}|[.-])*{keyword6}(?:{WORD}|[.-])*)([{WS}]*[=:][{WS}]*)(?:"[^"]*"|'[^']*'|[^{WS}"';&|]+)"#
            )),
            replace: Replace::Groups12,
        },
    ]
});

/// `maskSecrets` (`src/lib/journal.ts:99`): troca por `***` o que parece segredo (prefixos conhecidos de token,
/// Bearer/Basic, `usuário:senha@` em URL, flags e variáveis com nome de segredo). Idempotente.
pub fn mask_secrets(text: &str) -> String {
    let mut masked = text.to_owned();
    for rule in RULES.iter() {
        masked = match &rule.replace {
            Replace::All => rule.regex.replace_all(&masked, MASK).into_owned(),
            Replace::Group1Then(sep) => rule
                .regex
                .replace_all(&masked, |c: &Captures| format!("{}{sep}{MASK}", &c[1]))
                .into_owned(),
            Replace::Groups12 => rule
                .regex
                .replace_all(&masked, |c: &Captures| format!("{}{}{MASK}", &c[1], &c[2]))
                .into_owned(),
            Replace::Group1AtSign => rule
                .regex
                .replace_all(&masked, |c: &Captures| format!("{}{MASK}@", &c[1]))
                .into_owned(),
        };
    }
    masked
}

/// Comprimento em unidades UTF-16, o `.length` do JS.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `text.slice(0, max)` do JS. Se o corte cairia no meio de um par substituto, o par inteiro fica de fora (o JS
/// deixaria um surrogate solto, que o `String` do Rust não representa).
pub fn slice_utf16(text: &str, max: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        used += c.len_utf16();
        if used > max {
            break;
        }
        out.push(c);
    }
    out
}

/// `clip` (`src/lib/journal.ts:105`): espaços colapsados em um, aparado, e mais que `max` unidades UTF-16 vira
/// `max-1` mais `…`.
pub fn clip(text: &str, max: usize) -> String {
    let mut flat = String::with_capacity(text.len());
    let mut pending = false;
    for c in text.chars() {
        if is_js_space(c) {
            pending = true;
            continue;
        }
        if pending && !flat.is_empty() {
            flat.push(' ');
        }
        pending = false;
        flat.push(c);
    }
    if utf16_len(&flat) > max {
        return format!("{}…", slice_utf16(&flat, max.saturating_sub(1)));
    }
    flat
}

/// `summarizeCommand` (`src/lib/journal.ts:111`): sem o corpo de heredoc, em uma linha, mascarado e só depois truncado.
pub fn summarize_command(command: &str) -> String {
    let head = match command.find("<<") {
        None => command.to_owned(),
        Some(i) => format!("{}<<…", &command[..i]),
    };
    clip(&mask_secrets(&head), super::SUMMARY_MAX)
}
