//! O runner de ponta a ponta com `sh` nos dois lados (sem Bun nem skill).

use hades_parity::cases::Case;
use hades_parity::runner::{Config, Verdict, run_case};
use std::path::PathBuf;

fn cfg(cases_dir: PathBuf) -> Config {
    Config {
        skill: PathBuf::from("/nonexistent-skill"),
        rust_bin: PathBuf::from("/bin/sh"),
        bun: PathBuf::from("/bin/sh"),
        cases_dir,
        keep: false,
    }
}

fn case(toml: &str) -> Case {
    toml::from_str(toml).unwrap()
}

#[test]
fn passa_normaliza_home_e_nao_herda_env() {
    let dir = tempfile::tempdir().unwrap();
    let c = case(
        r#"
id = "ok"
argv = ["-c", 'echo "$HOME ${HADES_VAZA:-vazio} $TZ"']
ts_cmd = ["-c", 'echo "$HOME ${HADES_VAZA:-vazio} $TZ"']
"#,
    );
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    assert_eq!(v, Verdict::Pass);
}

#[test]
fn diverge_vira_fail_e_pending_nao() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = case(
        r#"
id = "div"
argv = ["-c", 'echo "$0"', "lado"]
ts_cmd = ["-c", 'echo ts']
"#,
    );
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    assert!(
        matches!(v, Verdict::Fail(ref d) if d.len() == 1 && d[0].starts_with("stdout")),
        "{v:?}"
    );
    c.pending = Some("motivo".into());
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    assert!(
        matches!(v, Verdict::Pending { diffs: Some(_), .. }),
        "{v:?}"
    );
}

#[test]
fn estado_em_arquivo_e_fixture_por_lado() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("fixtures/base")).unwrap();
    std::fs::write(dir.path().join("fixtures/base/in.txt"), "dado\n").unwrap();
    let c = case(
        r#"
id = "files"
fixture = "base"
argv = ["-c", 'cat in.txt >> out.txt; echo "$HOME" >> out.txt; touch x.lock']
ts_cmd = ["-c", 'cat in.txt >> out.txt; echo "$HOME" >> out.txt; touch x.lock']
compare = ["exit", "files:out.txt", "files:*.lock"]
"#,
    );
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    assert_eq!(v, Verdict::Pass);
}

#[test]
fn template_na_fixture_e_setup_por_lado() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("fixtures/base/@ENC:@HOME@|wt@")).unwrap();
    std::fs::write(
        dir.path().join("fixtures/base/@ENC:@HOME@|wt@/r.json"),
        r#"{"worktree":"@HOME@/wt","data":"2026-11-20"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("comum.sh"),
        "mkdir -p wt; echo $SIDE >> marca.txt",
    )
    .unwrap();
    std::fs::write(dir.path().join("so-rust.sh"), "echo extra >> marca.txt").unwrap();
    let c = case(
        r#"
id = "tpl"
fixture = "base"
setup = "comum.sh"
setup_rust = "so-rust.sh"
normalize_dates = true
argv = ["-c", 'sed "s/^rust$/ts/" marca.txt | grep -v extra; cat -- */r.json; test -d wt']
ts_cmd = ["-c", 'cat marca.txt; cat -- */r.json; test -d wt']
compare = ["stdout", "exit", "files:marca.txt"]
"#,
    );
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    assert!(
        matches!(v, Verdict::Fail(ref d) if d.len() == 1 && d[0].starts_with("arquivo")),
        "marca.txt difere (extra só no Rust): {v:?}"
    );
    let mut ok = c;
    ok.compare = vec!["stdout".into(), "exit".into()];
    assert_eq!(
        run_case(&cfg(dir.path().into()), &ok).unwrap(),
        Verdict::Pass
    );
}

#[test]
fn setup_que_falha_aborta_o_caso() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ruim.sh"), "exit 7").unwrap();
    let c = case(
        r#"
id = "ruim"
setup = "ruim.sh"
argv = ["-c", "true"]
ts_cmd = ["-c", "true"]
"#,
    );
    let err = run_case(&cfg(dir.path().into()), &c).unwrap_err();
    assert!(err.contains("saiu 7"), "{err}");
}
