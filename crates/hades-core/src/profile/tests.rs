use super::*;
use serde_json::json;
use std::fs;

use crate::proc::{RunOptions, run};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn env_in(home: &Path) -> Env {
    Env::empty()
        .with("HOME", home.to_string_lossy())
        .with("HADES_PACKS_ROOT", home.join("packs").to_string_lossy())
        .with("HADES_STACKS_ROOT", home.join("stacks").to_string_lossy())
        .with("HADES_SKILL_DIR", home.join("skill").to_string_lossy())
}

fn s(text: &str) -> Value {
    Value::String(text.to_owned())
}

#[test]
fn get_field_trata_null_false_e_vazio_como_ausente() {
    let p = json!({"a":{"b":"x","n":null,"f":false,"e":"","z":0,"o":{"k":[1,2]},"t":true}});
    assert_eq!(get_field(&p, "a.b", None).as_deref(), Some("x"));
    assert_eq!(get_field(&p, "a.z", None).as_deref(), Some("0"));
    assert_eq!(get_field(&p, "a.t", None).as_deref(), Some("true"));
    assert_eq!(
        get_field(&p, "a.o", None).as_deref(),
        Some(r#"{"k":[1,2]}"#)
    );
    for ausente in ["a.n", "a.f", "a.e", "a.nada", "a.b.c", "x"] {
        assert_eq!(get_field(&p, ausente, None), None, "{ausente}");
        assert_eq!(get_field(&p, ausente, Some("fb")).as_deref(), Some("fb"));
    }
}

#[test]
fn ref_path_junta_pacote_e_relativo() {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("packs/demo/profile.json");
    write(
        &file,
        r#"{"refs":{"guia":"docs/guia.md","vazio":"","num":3}}"#,
    );
    let res = Resolution {
        project: None,
        profile: Some(file),
        repo: None,
        root: None,
        packs: t.path().join("packs"),
        code: 0,
        message: String::new(),
    };
    assert_eq!(
        ref_path(&res, "guia"),
        Some(format!("{}/packs/demo/docs/guia.md", t.path().display()))
    );
    for nome in ["vazio", "num", "nada"] {
        assert_eq!(ref_path(&res, nome), None, "{nome}");
    }
    let sem = Resolution {
        profile: None,
        ..res
    };
    assert_eq!(ref_path(&sem, "guia"), None);
}

#[test]
fn core_range_e_core_ok_seguem_a_sintaxe() {
    assert_eq!(core_ok(Some(&s(">=4.3")), "4.3.0"), Ok(true));
    assert_eq!(core_ok(Some(&s(">=4.3")), "4.2.9"), Ok(false));
    assert_eq!(core_ok(Some(&s(">=4.3 <5")), "4.9.9"), Ok(true));
    assert_eq!(core_ok(Some(&s(">=4.3 <5")), "5.0.0"), Ok(false));
    assert_eq!(core_ok(Some(&s("4.3.x")), "4.3.7"), Ok(true));
    assert_eq!(core_ok(Some(&s("4.3.x")), "4.4.0"), Ok(false));
    assert_eq!(core_ok(Some(&s("4.x")), "4.99"), Ok(true));
    assert_eq!(core_ok(Some(&s("4.x")), "5"), Ok(false));
    assert_eq!(core_ok(Some(&s("4.3")), "4.3.0"), Ok(true));
    assert_eq!(core_ok(Some(&s("=4.3.1")), "4.3.1"), Ok(true));
    assert_eq!(core_ok(Some(&s(">4")), "4.0.1"), Ok(true));
    assert_eq!(core_ok(Some(&s("<=4.3")), "4.3.0"), Ok(true));
    assert_eq!(
        core_range(Some(&s("4.3.x"))).unwrap(),
        vec![(CoreOp::Ge, [4.0, 3.0, 0.0]), (CoreOp::Lt, [4.0, 4.0, 0.0])]
    );
}

#[test]
fn core_range_recusa_com_a_mensagem_do_validador() {
    assert_eq!(core_range(Some(&s(""))), Err("faixa vazia".into()));
    assert_eq!(core_range(Some(&s("  "))), Err("faixa vazia".into()));
    assert_eq!(core_range(None), Err("faixa vazia".into()));
    assert_eq!(core_range(Some(&json!(5))), Err("faixa vazia".into()));
    assert_eq!(
        core_range(Some(&s(">=4 abc"))),
        Err("termo 'abc' fora da sintaxe (>=4.3, <5, 4.3.x, 4.3.0)".into())
    );
    assert_eq!(
        core_range(Some(&s("4.3.0.x"))),
        Err("termo '4.3.0.x': o .x vai no lugar de menor ou patch (4.x, 4.3.x)".into())
    );
    assert_eq!(
        core_range(Some(&s(">=4.x"))),
        Err("termo '>=4.x' fora da sintaxe (>=4.3, <5, 4.3.x, 4.3.0)".into())
    );
    assert!(core_range(Some(&s("1.2.3.4"))).is_err());
}

#[test]
fn legado_so_quando_a_faixa_aceita_nucleo_anterior_ao_5() {
    assert_eq!(admits_legacy_core(Some(&s(">=4"))), Ok(true));
    assert_eq!(admits_legacy_core(Some(&s(">=5"))), Ok(false));
    assert_eq!(admits_legacy_core(Some(&s(">=4 <5"))), Ok(true));
    assert!(legacy_core_message("p", ">=4").starts_with("o plugin p declara core >=4, que aceita"));
}

#[test]
fn py_repr_e_py_text_como_o_python() {
    assert_eq!(py_repr(None), "None");
    assert_eq!(py_repr(Some(&json!(null))), "None");
    assert_eq!(py_repr(Some(&json!(true))), "True");
    assert_eq!(py_repr(Some(&json!(1.5))), "1.5");
    assert_eq!(py_repr(Some(&s("a"))), "'a'");
    assert_eq!(py_repr(Some(&s("it's"))), "\"it's\"");
    assert_eq!(py_repr(Some(&s("it's \"q\""))), "'it\\'s \"q\"'");
    assert_eq!(py_repr(Some(&s("a\\b\nc\td\r"))), "'a\\\\b\\nc\\td\\r'");
    assert_eq!(
        py_repr(Some(&json!({"a":[1,"x",null],"b":{"c":false}}))),
        "{'a': [1, 'x', None], 'b': {'c': False}}"
    );
    assert_eq!(py_text(Some(&s("cru"))), "cru");
    assert_eq!(py_text(Some(&json!([1]))), "[1]");
    assert_eq!(py_text(None), "None");
}

fn plugin_repo(t: &Path, profile: &str) -> PathBuf {
    write(&t.join("packs/demo/profile.json"), profile);
    write(&t.join("skill/VERSION"), "5.2.0\n");
    let repo = t.join("wt");
    fs::create_dir_all(&repo).unwrap();
    let d = repo.to_string_lossy();
    run("git", &["-C", &d, "init", "-q"], &RunOptions::default());
    run(
        "git",
        &[
            "-C",
            &d,
            "remote",
            "add",
            "origin",
            "https://github.com/acme/demo.git",
        ],
        &RunOptions::default(),
    );
    repo
}

#[test]
fn core_check_cobre_os_cinco_estados() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path());
    let pack = |core: &str| format!(r#"{{"name":"demo","identity":{{"repo":"acme/demo"}}{core}}}"#);

    let repo = plugin_repo(t.path(), &pack(r#","core":">=5""#));
    let c = core_check(&env, &repo, None);
    assert_eq!(c.state, CoreState::Ok);
    assert_eq!(c.message, "plugin demo pede o núcleo >=5; núcleo 5.2.0");
    assert_eq!(c.version, "5.2.0");

    write(&t.path().join("packs/demo/profile.json"), &pack(""));
    let c = core_check(&env, &repo, Some("5.0.0"));
    assert_eq!(c.state.as_str(), "sem-core");
    assert_eq!(c.version, "5.0.0");

    write(
        &t.path().join("packs/demo/profile.json"),
        &pack(r#","core":">=9""#),
    );
    let c = core_check(&env, &repo, None);
    assert_eq!(c.state, CoreState::Fora);
    assert!(
        c.message
            .starts_with("o plugin demo pede o núcleo >=9, mas o núcleo instalado é o 5.2.0")
    );
    assert!(
        c.message
            .ends_with(&format!("(git -C {}/packs/demo pull)", t.path().display()))
    );

    write(
        &t.path().join("packs/demo/profile.json"),
        &pack(r#","core":">=4""#),
    );
    let c = core_check(&env, &repo, None);
    assert_eq!(c.state, CoreState::Fora);
    assert!(c.message.contains("aceita núcleo anterior à 5.0.0"));

    write(
        &t.path().join("packs/demo/profile.json"),
        &pack(r#","core":"zzz""#),
    );
    let c = core_check(&env, &repo, None);
    assert_eq!(c.state, CoreState::Invalido);
    assert_eq!(
        c.message,
        "core 'zzz' do plugin demo: termo 'zzz' fora da sintaxe (>=4.3, <5, 4.3.x, 4.3.0)"
    );

    write(
        &t.path().join("packs/demo/profile.json"),
        &pack(r#","core":null"#),
    );
    let c = core_check(&env, &repo, None);
    assert_eq!(c.state, CoreState::Invalido);
    assert_eq!(c.core, "None");
    assert_eq!(c.message, "core None do plugin demo: faixa vazia");

    let c = core_check(&env, t.path(), None);
    assert_eq!(c.state, CoreState::SemPerfil);
    assert!(c.message.contains("não está num repositório git"));
}

#[test]
fn stacks_usuario_vence_skill_e_perfil_filtra() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path());
    write(&t.path().join("stacks/nx/stack.json"), r#"{"who":"user"}"#);
    write(&t.path().join("stacks/ng/stack.json"), r#"[1]"#);
    write(
        &t.path().join("skill/stacks/nx/stack.json"),
        r#"{"who":"skill"}"#,
    );
    write(
        &t.path().join("skill/stacks/py/stack.json"),
        r#"{"steps":{"test":{"kind":"test","costGb":2}}}"#,
    );
    let names = |v: Vec<(String, Value)>| v.into_iter().map(|(n, _)| n).collect::<Vec<_>>();
    let all = installed_stacks(&env);
    assert_eq!(all[0], ("nx".to_owned(), json!({"who":"user"})));
    assert_eq!(names(all), ["nx", "py"]);

    let repo = plugin_repo(
        t.path(),
        r#"{"name":"demo","identity":{"repo":"acme/demo"},"stacks":["py","nada"]}"#,
    );
    assert_eq!(names(active_stacks(&env, &repo)), ["py"]);
    assert_eq!(names(active_stacks(&env, t.path())), ["nx", "py"]);

    let step = step_of(&env, "py:test").unwrap();
    assert_eq!(step.kind, json!("test"));
    assert_eq!(step.cost_gb, json!(2));
    assert_eq!(step.parallel, json!(0));
    assert_eq!(step.measured_at, json!(""));
    assert_eq!(step_of(&env, "py:nada"), None);
    assert_eq!(step_of(&env, "nada:test"), None);
}

#[test]
fn termos_da_guarda_de_nome_e_vocabulario() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path());
    write(
        &t.path().join("packs/a/profile.json"),
        r#"{"name":"Demo","identity":{"repo":"acme/demo","aliases":["dm","Demo","zeta"],"vocabulary":["b+","a"]},"domainTerms":"terms.txt"}"#,
    );
    write(
        &t.path().join("packs/a/terms.txt"),
        "# c\n  termo \n@allow src/**\nzzz\ntermo\n",
    );
    write(
        &t.path().join("packs/b/profile.json"),
        r#"{"name":"x","identity":{"vocabulary":["a","c"]},"domainTerms":"falta.txt"}"#,
    );
    write(&t.path().join("packs/c/profile.json"), "[]");

    let nt = name_terms(&env);
    assert_eq!(nt[0].0, "Demo");
    assert_eq!(nt[0].1, ["acme", "acme/demo", "Demo", "demo", "zeta"]);
    assert_eq!(nt[1], ("x".to_owned(), Vec::<String>::new()));
    assert_eq!(vocabulary(&env), ["a", "b+", "c"]);
    assert_eq!(
        domain_terms(&env),
        DomainTerms {
            terms: vec!["termo".into(), "zzz".into()],
            allow: vec!["src/**".into()],
        }
    );
}

#[test]
fn load_profile_sem_perfil_ou_ilegivel_e_objeto_vazio() {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("profile.json");
    write(&file, "[1]");
    let mut res = Resolution {
        project: None,
        profile: None,
        repo: None,
        root: None,
        packs: PathBuf::new(),
        code: 0,
        message: String::new(),
    };
    assert_eq!(load_profile(&res), json!({}));
    res.profile = Some(file);
    assert_eq!(load_profile(&res), json!({}));
}

#[test]
fn core_version_apara_a_linha() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path());
    assert_eq!(core_version(&env), "");
    write(&t.path().join("skill/VERSION"), " 5.2.0 \nresto");
    assert_eq!(core_version(&env), "5.2.0");
}
