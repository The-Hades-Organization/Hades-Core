//! Normalização no padrão do golden legado (`<ts>`, `<id>`, `<n>`), aplicada aos dois lados antes de comparar.

use regex::{Captures, Regex};
use std::sync::LazyLock;

static ISO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
});
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{12,}\b").unwrap());
/// Id aleatório de 12 hex (`randomBytes(6).toString("hex")`) pelo nome da chave: pode sair só com dígitos, que o
/// [`HEX`] não troca.
static ID_FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"("(?:avaliacao|usada|id)"\s*:\s*")[0-9a-f]{12}""#).unwrap());
/// O mesmo id no texto da lista de exceções (`usada em <avaliação>`).
static ID_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(usada em )[0-9a-f]{12}\b").unwrap());
/// Pasta de preparo do Minos (`mkdtemp("minos-")`) como componente de caminho: o sufixo é aleatório (TS: 6
/// alfanuméricos; Rust: 6 hex).
static PREP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(/)minos-[A-Za-z0-9]{6}(/|$|[^\w.-])").unwrap());
static EPOCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"("(?:abertaTs|expira|ts|epoch|mtime|atMs)"\s*:\s*)\d{10,13}(?:\.\d+)?\b"#)
        .unwrap()
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
    /// Pasta de troca de um caso `cross`; vira `<xfer>`.
    pub xfer: Vec<String>,
}

fn replace_prefixes(text: &str, roots: &[String], with: &str) -> String {
    let mut roots: Vec<&String> = roots.iter().filter(|r| !r.is_empty()).collect();
    roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
    roots
        .into_iter()
        .fold(text.to_owned(), |acc, r| acc.replace(r.as_str(), with))
}

/// Ordem: caminhos, pasta de preparo `minos-XXXXXX`, instante ISO, epoch em campo conhecido, uuid, id aleatório por chave (`avaliacao`, `usada`, `id`)
/// e em `usada em`, hex de 12+ com letra, duração.
pub fn normalize(text: &str, roots: &Roots) -> String {
    normalize_keep(text, roots, false)
}

/// Como [`normalize`]; `keep_iso` mantém o instante ISO no texto (o caso compara a ordem cronológica).
pub fn normalize_keep(text: &str, roots: &Roots, keep_iso: bool) -> String {
    let text = replace_prefixes(text, &roots.xfer, "<xfer>");
    let text = replace_prefixes(&text, &roots.skill, "<skill>");
    let text = replace_prefixes(&text, &roots.home, "~");
    let text = PREP.replace_all(&text, "${1}<prep>${2}").into_owned();
    let text = if keep_iso {
        text
    } else {
        ISO.replace_all(&text, "<ts>").into_owned()
    };
    let text = EPOCH.replace_all(&text, "${1}<n>");
    let text = UUID.replace_all(&text, "<id>");
    let text = ID_FIELD.replace_all(&text, "${1}<id>\"");
    let text = ID_TEXT.replace_all(&text, "${1}<id>");
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
            ..Roots::default()
        }
    }

    #[test]
    fn datas_opcionais() {
        let n = normalize("em 2026-11-21 e 2026-11-21T10:20:30Z", &roots());
        assert_eq!(normalize_dates(&n), "em <date> e <ts>");
    }

    #[test]
    fn keep_iso_preserva_instante_e_normaliza_o_resto() {
        let text = "em 2026-11-21T10:20:30.123Z id 3f2a9c1b7d4e";
        assert_eq!(
            normalize_keep(text, &roots(), true),
            "em 2026-11-21T10:20:30.123Z id <id>"
        );
        assert_eq!(normalize_keep(text, &roots(), false), "em <ts> id <id>");
    }

    #[test]
    fn xfer_vira_marcador() {
        let r = Roots {
            xfer: vec!["/tmp/x1/xfer".into()],
            ..roots()
        };
        assert_eq!(normalize("/tmp/x1/xfer/p.caronte", &r), "<xfer>/p.caronte");
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
                r#"{"abertaTs": 1763720000.101, "expira": 1763723600.25}"#,
                &roots()
            ),
            r#"{"abertaTs": <n>, "expira": <n>}"#
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
    fn id_de_avaliacao_so_com_digitos_vira_id_pela_chave() {
        assert_eq!(
            normalize(
                r#"{"avaliacao": "073862443147", "usada":"073862443147", "id": "000000000001"}"#,
                &roots()
            ),
            r#"{"avaliacao": "<id>", "usada":"<id>", "id": "<id>"}"#
        );
        assert_eq!(
            normalize(r#"{"avaliacao":"283128447b66"}"#, &roots()),
            r#"{"avaliacao":"<id>"}"#
        );
        assert_eq!(
            normalize("pr-1  usada em 073862443147", &roots()),
            "pr-1  usada em <id>"
        );
    }

    #[test]
    fn numero_de_12_digitos_fora_das_chaves_do_id_segue_igual() {
        assert_eq!(
            normalize(
                r#"{"pr": "073862443147", "avaliacao": "0738624431470", "n": 073862443147}"#,
                &roots()
            ),
            r#"{"pr": "073862443147", "avaliacao": "0738624431470", "n": 073862443147}"#
        );
        assert_eq!(
            normalize("aberta em 073862443147", &roots()),
            "aberta em 073862443147"
        );
    }

    #[test]
    fn pasta_de_preparo_do_minos_vira_prep() {
        assert_eq!(
            normalize("token em /tmp/minos-jmRI1h/index.html:102", &roots()),
            "token em /tmp/<prep>/index.html:102"
        );
        assert_eq!(
            normalize("/tmp/minos-faf77f/index.html e /tmp/minos-faf77f", &roots()),
            "/tmp/<prep>/index.html e /tmp/<prep>"
        );
        assert_eq!(normalize("(/tmp/minos-A1b2C3)", &roots()), "(/tmp/<prep>)");
    }

    #[test]
    fn nome_parecido_com_a_pasta_de_preparo_segue_igual() {
        for text in [
            "crates/minos-core/src",
            "/x/minos-cli/a",
            "/x/minos-app",
            "/tmp/minos-abcde/a",
            "/tmp/minos-abcdefg/a",
            "/tmp/minos-abcdef.json",
            "/tmp/minos-abcdef-x/a",
            "minos-abcdef/a",
            "/tmp/xminos-abcdef/a",
        ] {
            assert_eq!(normalize(text, &roots()), text);
        }
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
