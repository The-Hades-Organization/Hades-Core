//! Registro dos agentes e estado das entregas (`src/lib/agent-registry.ts`, `lib.sh reg_set`/`delivery_set`) e as
//! pequenas regras do `hades agent` que mais de um subcomando usa (`planSha`, `openQuestionHeads`).
//!
//! Diferença deliberada do TS (plano §3, decisão 1): todo read-modify-write roda sob `flock` em `<f>.lock` e grava por
//! [`write_json_atomic`] (tmp único), então Rust e TS (com a PR 8) não perdem campo um do outro. Os registros são
//! `serde_json::Map` com `preserve_order`: as chaves existentes ficam na posição e as novas entram no fim.
//! Gravar cria a pasta do arquivo; ler nunca cria.
//!
//! Fora desta PR: `startCommand`, `roleModelDefaults` (dependem de `config` e `provider`), `callerPane`,
//! `orchestratorField`, `journalAgentEnd` (`journal`) e `assertNoNestedWorktrees` (`worktree-scope`).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::atomic::write_json_atomic;
use crate::clock;
use crate::env::Env;
use crate::jsonfmt::json_compact;
use crate::lock::with_flock;
use crate::work::{Json, WorkEnv, agent_file, read_json};

/// `<f>.lock`, o arquivo de lock de um registro.
fn lock_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().map(|n| n.to_owned()).unwrap_or_default();
    name.push(".lock");
    file.with_file_name(name)
}

/// Read-modify-write de um JSON sob o lock do arquivo: lê (objeto vazio se não existe ou não é objeto), aplica `f` e
/// grava de forma atômica.
fn modify_json(file: &Path, f: impl FnOnce(Json) -> Json) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    with_flock(&lock_path(file), || {
        let next = f(read_json(file).unwrap_or_default());
        write_json_atomic(file, &Value::Object(next))
    })?
}

/// Segundos desde a época em `YYYY-MM-DDTHH:MM:SSZ`, sem ms (`isoNow`, `src/lib/agent-registry.ts:11`).
pub fn iso_from_epoch(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// `isoNow` (`src/lib/agent-registry.ts:11`). O diário usa outro formato, com ms.
pub fn iso_now() -> Result<String, clock::ClockError> {
    iso_now_env(&Env::from_process())
}

/// [`iso_now`] com o relógio do `env` (`HADES_NOW`); valor inválido é erro.
pub fn iso_now_env(env: &Env) -> Result<String, clock::ClockError> {
    clock::now_secs(env).map(iso_from_epoch)
}

/// `regReplace` (`src/lib/agent-registry.ts:14`): registro novo no lugar do anterior (o `reg_set` com objeto literal).
pub fn reg_replace(work: &WorkEnv, name: &str, record: Json) -> io::Result<()> {
    modify_json(&agent_file(work, name), |_| record)
}

/// `regUpdate` (`src/lib/agent-registry.ts:20`): os campos novos entram no fim, os existentes ficam na posição.
pub fn reg_update(work: &WorkEnv, name: &str, update: impl FnOnce(Json) -> Json) -> io::Result<()> {
    modify_json(&agent_file(work, name), update)
}

/// `regSet` (`src/lib/work.ts:148`): `{...atual, ...patch}`. Chave existente muda de valor sem mudar de posição.
pub fn reg_set(work: &WorkEnv, name: &str, patch: Json) -> io::Result<()> {
    modify_json(&agent_file(work, name), |mut current| {
        for (k, v) in patch {
            current.insert(k, v);
        }
        current
    })
}

/// `counter` (`src/lib/agent-registry.ts:25`): número finito e não negativo; senão zero. Inteiro sai inteiro.
fn counter(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(n)) => n
            .as_u64()
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.is_finite() && *f >= 0.0)
                    .map(|f| f as u64)
            })
            .unwrap_or(0),
        _ => 0,
    }
}

/// Qual subcomando abriu a sessão limpa (`afterFreshSession`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshSession {
    Phase,
    Resume,
}

/// `afterFreshSession` (`src/lib/agent-registry.ts:34`): sessão limpa de `phase` ou `resume`; os dois contam em
/// `clears`, só o `resume` incrementa `resumes`. `label` vazio não mexe em `phase`. Chaves novas entram no fim, na
/// ordem `phase`?, `state`, `promptedAt`, `clears`, `resumes`?.
pub fn after_fresh_session(
    mut record: Json,
    sub: FreshSession,
    label: &str,
    prompted_at: &str,
) -> Json {
    let clears = counter(record.get("clears")) + 1;
    let resumes = counter(record.get("resumes")) + 1;
    if !label.is_empty() {
        record.insert("phase".into(), Value::String(label.to_owned()));
    }
    record.insert("state".into(), "working".into());
    record.insert("promptedAt".into(), prompted_at.into());
    record.insert("clears".into(), clears.into());
    if sub == FreshSession::Resume {
        record.insert("resumes".into(), resumes.into());
    }
    record
}

/// `jqText` (`src/lib/agent-registry.ts:42`): o texto de um campo como o `jq -r`; ausente vira `"null"`.
pub fn jq_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "null".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => json_compact(other),
    }
}

fn object_of(value: Option<Value>) -> Json {
    match value {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// `deliverySet` (`src/lib/agent-registry.ts:48`): `deliveries[<chave>]` em `<state>/global.json`
/// (`{version: 2, deliveries: {}}` se não existe). Entrega e `deliveries` que não são objeto contam como vazios.
pub fn delivery_set(
    work: &WorkEnv,
    key: &str,
    update: impl FnOnce(Json) -> Json,
) -> io::Result<()> {
    let file = work.state.join("global.json");
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    with_flock(&lock_path(&file), || {
        let mut doc = read_json(&file).unwrap_or_else(|| {
            let mut m = Map::new();
            m.insert("version".into(), 2.into());
            m.insert("deliveries".into(), Value::Object(Map::new()));
            m
        });
        let mut deliveries = object_of(doc.get("deliveries").cloned());
        let delivery = object_of(deliveries.get(key).cloned());
        deliveries.insert(key.to_owned(), Value::Object(update(delivery)));
        doc.insert("deliveries".into(), Value::Object(deliveries));
        write_json_atomic(&file, &Value::Object(doc))
    })?
}

/// `awkLines` (`src/commands/agent.ts:28`): linhas como o awk as vê, sem o último `\n`.
fn awk_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// `/^## Estado atual([^A-Za-z0-9_]|$)/` (`src/commands/agent.ts:42`).
fn is_estado_atual(line: &str) -> bool {
    line.strip_prefix("## Estado atual")
        .is_some_and(|rest| match rest.chars().next() {
            None => true,
            Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
        })
}

/// `planSha` (`src/commands/agent.ts:36`): SHA-256 do plano sem a seção "Estado atual", que é reescrita a cada evento
/// e não pede nova aprovação. Cercas de código (```) suspendem o reconhecimento de `## `.
pub fn plan_sha(plan_text: &str) -> String {
    let text = plan_text.replace('\r', "");
    let mut in_fence = false;
    let mut skip = false;
    let mut hasher = Sha256::new();
    for line in awk_lines(&text) {
        if line.starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && line.starts_with("## ") {
            skip = is_estado_atual(line);
        }
        if !skip {
            hasher.update(line.as_bytes());
            hasher.update(b"\n");
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub use crate::work::{count_questions, open_questions};

const SUMMARY_MAX: usize = 200;

/// Terminadores de linha do `^`/`$` multilinha do JS: `\n`, `\r`, ` `, ` `.
fn js_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\n', '\r', '\u{2028}', '\u{2029}'])
}

/// `\s` do JS (difere do `char::is_whitespace` em U+0085 e U+FEFF).
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `clip` (`src/lib/argos.ts:184`): corta em `max` unidades UTF-16, com `…` no lugar da última.
fn clip(text: &str, max: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= max {
        return text.to_owned();
    }
    let mut cut = String::from_utf16_lossy(&units[..max - 1]);
    cut.push('…');
    cut
}

/// `^## (Q\d+.*)$` → o texto a partir do `Q`.
fn question_head(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("## ")?;
    let digits = rest.strip_prefix('Q')?;
    digits
        .starts_with(|c: char| c.is_ascii_digit())
        .then_some(rest)
}

/// `^## Resposta do dono \((Q\d+)\)` → `Qn`.
fn answered_id(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("## Resposta do dono (")?;
    let digits = rest.strip_prefix('Q')?;
    let n = digits.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0 && digits[n..].starts_with(')')).then(|| &rest[..n + 1])
}

/// `openQuestionHeads` (`src/lib/argos.ts:192`): títulos `## Qn` sem "(respondida)" e sem `## Resposta do dono (Qn)`
/// em qualquer ponto do texto, cortados em 200 unidades. É a regra do `hades agent status`; difere de
/// [`open_questions`], que casa a resposta pela posição.
pub fn open_question_heads(text: &str) -> Vec<String> {
    let answered: HashSet<&str> = js_lines(text).filter_map(answered_id).collect();
    js_lines(text)
        .filter_map(question_head)
        .filter(|h| {
            let id = h.split(is_js_space).next().unwrap_or("");
            !h.contains("(respondida)") && !answered.contains(id)
        })
        .map(|h| clip(h, SUMMARY_MAX))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::thread;

    fn work_env(root: &Path) -> WorkEnv {
        let work = root.join("p");
        WorkEnv {
            skill_scope: false,
            repo_top: root.to_owned(),
            main_clone: root.to_owned(),
            slug: "p".into(),
            repo_name: "p".into(),
            locks: work.join("locks"),
            state: work.join("state"),
            orch: work.join("orchestration"),
            skill_orch: root.join("_skill").join("orchestration"),
            work,
            reports: ".r".into(),
        }
    }

    fn obj(v: Value) -> Json {
        v.as_object().unwrap().clone()
    }

    fn keys(m: &Json) -> Vec<&str> {
        m.keys().map(String::as_str).collect()
    }

    #[test]
    fn iso_from_epoch_bate_com_to_iso_string() {
        assert_eq!(iso_from_epoch(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_epoch(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_from_epoch(1_793_000_000 + 3), "2026-10-26T07:33:23Z");
        assert_eq!(iso_from_epoch(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(iso_from_epoch(-1), "1969-12-31T23:59:59Z");
        let now = iso_now().unwrap();
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z') && now.as_bytes()[10] == b'T');
    }

    #[test]
    fn reg_replace_cria_a_pasta_e_grava_no_formato_do_ts() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        reg_replace(
            &w,
            "a",
            obj(json!({"name":"a","state":"parked","pr":null,"clears":0})),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(agent_file(&w, "a")).unwrap(),
            "{\n  \"name\": \"a\",\n  \"state\": \"parked\",\n  \"pr\": null,\n  \"clears\": 0\n}\n"
        );
        reg_replace(&w, "a", obj(json!({"name":"a"}))).unwrap();
        assert_eq!(read_json(&agent_file(&w, "a")).unwrap().len(), 1);
    }

    #[test]
    fn reg_update_mantem_a_posicao_e_poe_o_novo_no_fim() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        reg_replace(
            &w,
            "a",
            obj(json!({"name":"a","state":"parked","createdAt":"x"})),
        )
        .unwrap();
        reg_update(&w, "a", |mut r| {
            r.insert("pane".into(), "p1".into());
            r.insert("state".into(), "working".into());
            r
        })
        .unwrap();
        let r = read_json(&agent_file(&w, "a")).unwrap();
        assert_eq!(keys(&r), ["name", "state", "createdAt", "pane"]);
        assert_eq!(r["state"], "working");
    }

    #[test]
    fn reg_update_de_registro_ausente_ou_invalido_parte_de_objeto_vazio() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        reg_update(&w, "novo", |r| {
            assert!(r.is_empty());
            obj(json!({"name":"novo"}))
        })
        .unwrap();
        fs::write(agent_file(&w, "lixo"), "[1]").unwrap();
        reg_update(&w, "lixo", |r| {
            assert!(r.is_empty());
            r
        })
        .unwrap();
        assert_eq!(fs::read_to_string(agent_file(&w, "lixo")).unwrap(), "{}\n");
    }

    #[test]
    fn reg_set_mescla_como_spread_do_ts() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        reg_set(&w, "a", obj(json!({"b":1,"a":2}))).unwrap();
        reg_set(&w, "a", obj(json!({"a":9,"c":3}))).unwrap();
        let r = read_json(&agent_file(&w, "a")).unwrap();
        assert_eq!(keys(&r), ["b", "a", "c"]);
        assert_eq!(r["a"], 9);
    }

    #[test]
    fn nao_deixa_tmp_nem_lixo_alem_do_lock() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        reg_set(&w, "a", obj(json!({"x":1}))).unwrap();
        let mut names = crate::fsx::list_dir(&w.orch.join("agents"));
        names.sort();
        assert_eq!(names, ["a.json", "a.json.lock"]);
    }

    #[test]
    fn escritores_concorrentes_nao_perdem_campo() {
        let t = tempfile::tempdir().unwrap();
        let w = Arc::new(work_env(t.path()));
        reg_replace(&w, "a", obj(json!({"name":"a"}))).unwrap();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let w = Arc::clone(&w);
                thread::spawn(move || {
                    for j in 0..10 {
                        reg_update(&w, "a", |mut r| {
                            r.insert(format!("k{i}_{j}"), (i * 10 + j).into());
                            r
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(read_json(&agent_file(&w, "a")).unwrap().len(), 81);
    }

    #[test]
    fn after_fresh_session_phase_e_resume() {
        let rec = obj(json!({"name":"a","state":"parked","clears":2}));
        let phase = after_fresh_session(rec.clone(), FreshSession::Phase, "impl", "T");
        assert_eq!(
            keys(&phase),
            ["name", "state", "clears", "phase", "promptedAt"]
        );
        assert_eq!(
            (
                phase["state"].clone(),
                phase["clears"].clone(),
                phase.get("resumes").cloned()
            ),
            (json!("working"), json!(3), None)
        );
        let resume = after_fresh_session(rec, FreshSession::Resume, "", "T2");
        assert!(!resume.contains_key("phase"));
        assert_eq!(
            (resume["clears"].clone(), resume["resumes"].clone()),
            (json!(3), json!(1))
        );
        assert_eq!(resume["promptedAt"], "T2");
    }

    #[test]
    fn counter_ignora_o_que_nao_e_numero_valido() {
        let c = |v: Value| counter(Some(&v));
        assert_eq!(counter(None), 0);
        assert_eq!(
            (c(json!(4)), c(json!(-1)), c(json!("3")), c(json!(null))),
            (4, 0, 0, 0)
        );
        assert_eq!(c(json!(2.0)), 2);
        let rec = obj(json!({"clears":"x","resumes":-5}));
        let r = after_fresh_session(rec, FreshSession::Resume, "", "T");
        assert_eq!(
            (r["clears"].clone(), r["resumes"].clone()),
            (json!(1), json!(1))
        );
    }

    #[test]
    fn jq_text_como_o_jq_r() {
        assert_eq!(jq_text(None), "null");
        assert_eq!(jq_text(Some(&json!(null))), "null");
        assert_eq!(jq_text(Some(&json!("x"))), "x");
        assert_eq!(jq_text(Some(&json!(7))), "7");
        assert_eq!(jq_text(Some(&json!(false))), "false");
        assert_eq!(jq_text(Some(&json!({"a":[1]}))), r#"{"a":[1]}"#);
    }

    #[test]
    fn delivery_set_cria_o_documento_v2_e_mescla_por_chave() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        delivery_set(&w, "K-1", |mut d| {
            assert!(d.is_empty());
            d.insert("state".into(), "open".into());
            d
        })
        .unwrap();
        delivery_set(&w, "K-2", |_| obj(json!({"n":1}))).unwrap();
        delivery_set(&w, "K-1", |mut d| {
            d.insert("pr".into(), 5.into());
            d
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(w.state.join("global.json")).unwrap(),
            "{\n  \"version\": 2,\n  \"deliveries\": {\n    \"K-1\": {\n      \"state\": \"open\",\n      \"pr\": 5\n    },\n    \"K-2\": {\n      \"n\": 1\n    }\n  }\n}\n"
        );
    }

    #[test]
    fn delivery_set_preserva_campos_do_documento_e_tolera_tipos_errados() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        fs::create_dir_all(&w.state).unwrap();
        fs::write(
            w.state.join("global.json"),
            r#"{"version":3,"extra":true,"deliveries":[1]}"#,
        )
        .unwrap();
        delivery_set(&w, "k", |_| obj(json!({"a":1}))).unwrap();
        let doc = read_json(&w.state.join("global.json")).unwrap();
        assert_eq!(keys(&doc), ["version", "extra", "deliveries"]);
        assert_eq!(doc["version"], 3);
        assert_eq!(doc["deliveries"], json!({"k":{"a":1}}));
        fs::write(
            w.state.join("global.json"),
            r#"{"deliveries":{"k":"texto"}}"#,
        )
        .unwrap();
        delivery_set(&w, "k", |d| {
            assert!(d.is_empty());
            d
        })
        .unwrap();
    }

    fn sha(s: &str) -> String {
        Sha256::digest(s.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn plan_sha_ignora_estado_atual_e_normaliza_cr_e_fim_de_arquivo() {
        let base = "# Plano\n\n## Escopo\nA\n";
        assert_eq!(plan_sha(base), sha(base));
        let com_estado =
            "# Plano\n\n## Escopo\nA\n## Estado atual\nmuda\n## Estado atual (v2)\nmuda\n";
        assert_eq!(plan_sha(com_estado), sha(base));
        assert_eq!(plan_sha(&base.replace('\n', "\r\n")), sha(base));
        assert_eq!(plan_sha("# Plano\n\n## Escopo\nA"), sha(base));
        assert_eq!(plan_sha(""), sha(""));
    }

    #[test]
    fn plan_sha_estado_atual_termina_no_proximo_h2_e_nome_parecido_nao_conta() {
        let t = "## A\n1\n## Estado atual\nx\n## B\n2\n";
        assert_eq!(plan_sha(t), sha("## A\n1\n## B\n2\n"));
        let parecido = "## Estado atualizado\nx\n## Estado atual_2\ny\n";
        assert_eq!(plan_sha(parecido), sha(parecido));
        assert_eq!(plan_sha("## Estado atual: x\nz\n"), sha(""));
    }

    #[test]
    fn plan_sha_cerca_de_codigo_esconde_o_h2() {
        let t = "## A\n```\n## Estado atual\ndentro\n```\nfora\n";
        assert_eq!(plan_sha(t), sha(t));
        let aberta = "## Estado atual\nx\n```\n## B\ny\n";
        assert_eq!(plan_sha(aberta), sha(""));
    }

    #[test]
    fn plan_sha_vetor_fixo() {
        assert_eq!(
            plan_sha("a\n"),
            "87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7"
        );
    }

    #[test]
    fn open_question_heads_respostas_valem_em_qualquer_ponto() {
        let t = "## Resposta do dono (Q2)\nok\n## Q1 primeira\n## Q2 segunda\n## Q3 terceira (respondida)\n## Q4\n## Qx fora\n";
        assert_eq!(open_question_heads(t), ["Q1 primeira", "Q4"]);
    }

    #[test]
    fn open_question_heads_casa_o_id_inteiro_e_corta_em_200() {
        let t = "## Resposta do dono (Q1)\n## Q10 aberta\n## Q1 fechada\n";
        assert_eq!(open_question_heads(t), ["Q10 aberta"]);
        let longa = format!("## Q5 {}\n", "x".repeat(300));
        let heads = open_question_heads(&longa);
        assert_eq!(heads[0].encode_utf16().count(), 200);
        assert!(heads[0].ends_with('…'));
        assert!(heads[0].starts_with("Q5 xxx"));
    }

    #[test]
    fn open_question_heads_trata_cr_como_fim_de_linha() {
        assert_eq!(
            open_question_heads("## Q1 a\r\n## Q2 b\r\n"),
            ["Q1 a", "Q2 b"]
        );
        assert!(open_question_heads("").is_empty());
    }
}
