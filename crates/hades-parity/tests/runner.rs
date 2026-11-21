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
