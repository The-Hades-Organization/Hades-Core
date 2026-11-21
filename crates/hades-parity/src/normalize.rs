//! Normalização no padrão do golden legado (`<ts>`, `<id>`, `<n>`), aplicada aos dois lados antes de comparar.

use regex::{Captures, Regex};
use std::sync::LazyLock;

static ISO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
});
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{12,}\b").unwrap());
static EPOCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"("(?:abertaTs|expira|ts|epoch|mtime|atMs)"\s*:\s*)\d{10,13}\b"#).unwrap()
});
static DURATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d+(?:\.\d+)? ?(?:ms|s)\b").unwrap());

static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{4}-\d\d-\d\d\b").unwrap());

/// Normalização opcional (`normalize_dates`): data `YYYY-MM-DD` vira `<date>`. Roda depois de [`normalize`], que
/// já trocou os instantes ISO.
pub fn normalize_dates(text: &str) -> String {
    DATE.replace_all(text, "<date>").into_owned()
}

/// Raízes que viram `~` (HOME do sandbox, literal e realpath) e `<skill>`.
#[derive(Debug, Default, Clone)]
pub struct Roots {
    pub home: Vec<String>,
    pub skill: Vec<String>,
}

fn replace_prefixes(text: &str, roots: &[String], with: &str) -> String {
    let mut roots: Vec<&String> = roots.iter().filter(|r| !r.is_empty()).collect();
    roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
    roots
        .into_iter()
        .fold(text.to_owned(), |acc, r| acc.replace(r.as_str(), with))
}

/// Ordem: caminhos, instante ISO, epoch em campo conhecido, uuid, hex de 12+ com letra, duração.
pub fn normalize(text: &str, roots: &Roots) -> String {
    let text = replace_prefixes(text, &roots.skill, "<skill>");
    let text = replace_prefixes(&text, &roots.home, "~");
    let text = ISO.replace_all(&text, "<ts>");
    let text = EPOCH.replace_all(&text, "${1}<n>");
    let text = UUID.replace_all(&text, "<id>");
    let text = HEX.replace_all(&text, |c: &Captures| {
        if c[0].bytes().any(|b| b.is_ascii_lowercase()) {
            "<id>".to_owned()
        } else {
            c[0].to_owned()
        }
    });
    DURATION.replace_all(&text, "<dur>").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Roots {
        Roots {
            home: vec!["/tmp/x1/home".into(), "/private/tmp/x1/home".into()],
            skill: vec!["/sk".into()],
        }
    }

    #[test]
    fn datas_opcionais() {
        let n = normalize("em 2026-11-21 e 2026-11-21T10:20:30Z", &roots());
        assert_eq!(normalize_dates(&n), "em <date> e <ts>");
    }

    #[test]
    fn caminhos() {
        assert_eq!(
            normalize("/tmp/x1/home/work/acme__demo e /sk/src", &roots()),
            "~/work/acme__demo e <skill>/src"
        );
        assert_eq!(normalize("/private/tmp/x1/home/a", &roots()), "~/a");
    }

    #[test]
    fn instantes_ids_e_epoch() {
        assert_eq!(
            normalize(
                "em 2026-11-21T10:20:30Z e 2026-11-21T10:20:30.123Z",
                &roots()
            ),
            "em <ts> e <ts>"
        );
        assert_eq!(
            normalize(
                r#"{"abertaTs": 1763720000000, "expira":1763720000}"#,
                &roots()
            ),
            r#"{"abertaTs": <n>, "expira":<n>}"#
        );
        assert_eq!(
            normalize(
                "id 3f2a9c1b7d4e e 123e4567-e89b-12d3-a456-426614174000",
                &roots()
            ),
            "id <id> e <id>"
        );
    }

    #[test]
    fn numero_longo_sem_letra_nao_vira_id() {
        assert_eq!(
            normalize("n=123456789012345", &roots()),
            "n=123456789012345"
        );
    }

    #[test]
    fn duracao() {
        assert_eq!(
            normalize("levou 12ms, 3.5 s e 4s", &roots()),
            "levou <dur>, <dur> e <dur>"
        );
        assert_eq!(normalize("5 sources", &roots()), "5 sources");
    }
}
