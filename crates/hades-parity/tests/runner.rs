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

#[test]
fn keep_iso_mantem_instante_e_sem_keep_vira_ts() {
    let dir = tempfile::tempdir().unwrap();
    let base = r#"
id = "iso"
argv = ["-c", 'echo "em 2026-11-21T10:20:30Z"']
ts_cmd = []
"#;
    let normal = case(base);
    let kept = case(&format!(
        "{base}keep = ['iso']\ncompare = ['stdout','files:out']\n"
    ));
    assert_eq!(
        run_case(&cfg(dir.path().into()), &normal).unwrap(),
        Verdict::Pass
    );
    assert_eq!(
        run_case(&cfg(dir.path().into()), &kept).unwrap(),
        Verdict::Pass
    );

    let mut diverge = case(
        r#"
id = "iso-div"
argv = ["-c", 'echo "$0"', "2026-11-21T10:20:30Z"]
ts_cmd = ["-c", 'echo 2026-11-21T10:20:31Z']
"#,
    );
    assert_eq!(
        run_case(&cfg(dir.path().into()), &diverge).unwrap(),
        Verdict::Pass
    );
    diverge.keep = vec!["iso".into()];
    let v = run_case(&cfg(dir.path().into()), &diverge).unwrap();
    assert!(matches!(v, Verdict::Fail(ref d) if d.len() == 1), "{v:?}");
}

const CROSS_OK: &str = r#"
id = "x"
kind = "cross"
ts_cmd = []
produce = ["-c", 'echo "payload" > "{xfer}/p" && echo marca > "$HOME/so-do-produtor"']
consume = ["-c", 'test ! -e "$HOME/so-do-produtor" && cat "{xfer}/p"']
compare = ["stdout", "exit"]
"#;

#[test]
fn cross_passa_e_consumidor_nao_ve_o_home_do_produtor() {
    let dir = tempfile::tempdir().unwrap();
    let v = run_case(&cfg(dir.path().into()), &case(CROSS_OK)).unwrap();
    assert_eq!(v, Verdict::Pass);
}

#[test]
fn cross_diverge_aponta_a_perna() {
    let dir = tempfile::tempdir().unwrap();
    let c = case(&CROSS_OK.replace(
        r#"cat "{xfer}/p""#,
        r#"case "$HOME" in */rust/home) echo r;; *) echo t;; esac"#,
    ));
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    match v {
        Verdict::Fail(d) => {
            assert_eq!(d.len(), 1, "{d:?}");
            assert!(d[0].starts_with("TS→Rust: stdout"), "{d:?}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn cross_produce_falho_e_erro_do_caso() {
    let dir = tempfile::tempdir().unwrap();
    let c = case(&CROSS_OK.replace(r#"echo "payload""#, "echo erro >&2; exit 3; echo x"));
    let e = run_case(&cfg(dir.path().into()), &c).unwrap_err();
    assert!(e.contains("produce (TS) saiu 3"), "{e}");
}

#[test]
fn cross_normaliza_pasta_de_troca() {
    let dir = tempfile::tempdir().unwrap();
    let c = case(&CROSS_OK.replace(r#"cat "{xfer}/p""#, r#"echo "{xfer}/p""#));
    let v = run_case(&cfg(dir.path().into()), &c).unwrap();
    assert_eq!(v, Verdict::Pass);
}

#[test]
fn cross_compara_arvore_do_consumidor() {
    let dir = tempfile::tempdir().unwrap();
    let ok = CROSS_OK.replace(r#"cat "{xfer}/p""#, r#"cp "{xfer}/p" "$HOME/restaurado""#)
        + "\ncompare = ['files:restaurado']\n";
    let ok = ok.replace("compare = [\"stdout\", \"exit\"]\n", "");
    let v = run_case(&cfg(dir.path().into()), &case(&ok)).unwrap();
    assert_eq!(v, Verdict::Pass);
}
