//! Resultado de um lado e comparação em bytes (ou por valor JSON, ou por arquivos).

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub exit: i32,
    /// Caminho relativo ao HOME (`~/...`) → conteúdo normalizado, só dos globs pedidos.
    pub files: BTreeMap<String, String>,
}

/// Glob com `*` (dentro de um segmento) e `**` (zero ou mais segmentos).
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let segs: Vec<&str> = path.split('/').collect();
    match_segments(&pat, &segs)
}

fn match_segments(pat: &[&str], segs: &[&str]) -> bool {
    match pat.split_first() {
        None => segs.is_empty(),
        Some((&"**", rest)) => (0..=segs.len()).any(|i| match_segments(rest, &segs[i..])),
        Some((p, rest)) => segs.split_first().is_some_and(|(s, tail)| {
            wild(p.as_bytes(), s.as_bytes()) && match_segments(rest, tail)
        }),
    }
}

fn wild(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| wild(rest, &s[i..])),
        Some((c, rest)) => s.first() == Some(c) && wild(rest, &s[1..]),
    }
}

fn ignored(rel: &str) -> bool {
    rel.ends_with(".lock") || rel.ends_with(".tmp")
}

/// Lista a árvore do HOME depois do comando: arquivos que casam com algum glob, sem `.lock` e `*.tmp`.
pub fn list_files(home: &Path, globs: &[String]) -> Vec<(String, std::path::PathBuf)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push((rel.to_string_lossy().into_owned(), path.clone()));
            }
        }
    }
    let mut all = Vec::new();
    walk(home, home, &mut all);
    all.retain(|(rel, _)| {
        !ignored(rel)
            && globs
                .iter()
                .any(|g| glob_match(g.trim_start_matches("~/"), rel))
    });
    all.sort();
    all
}

/// Divergências entre os dois lados para a lista `compare` do caso; vazia = paridade.
pub fn compare(items: &[String], ts: &Outcome, rust: &Outcome) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        match item.as_str() {
            "stdout" => diff_text("stdout", &ts.stdout, &rust.stdout, &mut out),
            "stderr" => diff_text("stderr", &ts.stderr, &rust.stderr, &mut out),
            "exit" if ts.exit != rust.exit => {
                out.push(format!("exit: TS {} × Rust {}", ts.exit, rust.exit));
            }
            "exit" => {}
            "json" => diff_json(ts, rust, &mut out),
            other => match other.strip_prefix("files:") {
                Some(_) => diff_files(ts, rust, &mut out),
                None => out.push(format!("item de compare desconhecido: {other}")),
            },
        }
    }
    out.dedup();
    out
}

fn diff_text(name: &str, ts: &str, rust: &str, out: &mut Vec<String>) {
    if ts == rust {
        return;
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (ts.lines().collect(), rust.lines().collect());
    let at = (0..a.len().max(b.len()))
        .find(|&i| a.get(i) != b.get(i))
        .unwrap_or(0);
    out.push(format!(
        "{name}: difere na linha {}\n    TS:   {:?}\n    Rust: {:?}",
        at + 1,
        a.get(at).unwrap_or(&"<fim>"),
        b.get(at).unwrap_or(&"<fim>")
    ));
}

fn diff_json(ts: &Outcome, rust: &Outcome, out: &mut Vec<String>) {
    let parse = |s: &str| serde_json::from_str::<serde_json::Value>(s);
    match (parse(&ts.stdout), parse(&rust.stdout)) {
        (Ok(a), Ok(b)) if a == b => {}
        (Ok(_), Ok(_)) => out.push("json: valores diferentes".to_owned()),
        (a, b) => out.push(format!(
            "json: stdout não é JSON (TS {}, Rust {})",
            if a.is_ok() { "ok" } else { "inválido" },
            if b.is_ok() { "ok" } else { "inválido" }
        )),
    }
}

fn diff_files(ts: &Outcome, rust: &Outcome, out: &mut Vec<String>) {
    let names: std::collections::BTreeSet<&String> =
        ts.files.keys().chain(rust.files.keys()).collect();
    for name in names {
        match (ts.files.get(name), rust.files.get(name)) {
            (Some(a), Some(b)) => diff_text(&format!("arquivo {name}"), a, b, out),
            (Some(_), None) => out.push(format!("arquivo {name}: só no TS")),
            (None, Some(_)) => out.push(format!("arquivo {name}: só no Rust")),
            (None, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(stdout: &str, exit: i32) -> Outcome {
        Outcome {
            stdout: stdout.into(),
            exit,
            ..Outcome::default()
        }
    }

    #[test]
    fn glob() {
        assert!(glob_match("work/**", "work/a/b.json"));
        assert!(glob_match("work/**", "work/a.json"));
        assert!(glob_match(".claude/*/x.json", ".claude/loop/x.json"));
        assert!(!glob_match(".claude/*/x.json", ".claude/a/b/x.json"));
        assert!(glob_match("**/*.json", "a/b/c.json"));
        assert!(!glob_match("work/**", "other/a"));
    }

    #[test]
    fn bytes_e_exit() {
        let items = vec!["stdout".to_owned(), "exit".to_owned()];
        assert!(compare(&items, &out("a\n", 0), &out("a\n", 0)).is_empty());
        assert_eq!(compare(&items, &out("a\n", 0), &out("a\n\n", 1)).len(), 2);
    }

    #[test]
    fn json_ignora_ordem_de_chaves_mas_stdout_nao() {
        let (a, b) = (out(r#"{"a":1,"b":2}"#, 0), out(r#"{"b":2,"a":1}"#, 0));
        assert!(compare(&["json".to_owned()], &a, &b).is_empty());
        assert_eq!(compare(&["stdout".to_owned()], &a, &b).len(), 1);
    }

    #[test]
    fn arquivos() {
        let mut a = Outcome::default();
        let mut b = Outcome::default();
        a.files.insert("~/x".into(), "1".into());
        b.files.insert("~/y".into(), "1".into());
        let d = compare(&["files:**".to_owned()], &a, &b);
        assert_eq!(d, vec!["arquivo ~/x: só no TS", "arquivo ~/y: só no Rust"]);
    }
}
