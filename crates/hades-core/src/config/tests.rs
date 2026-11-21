use super::*;
use std::fs;
use tempfile::TempDir;

fn tsv(rows: &[&str]) -> String {
    rows.join("\n") + "\n"
}

const KIT: [&str; 5] = [
    "# comentário",
    "HADES_A\t1\tbool\tG\tRótulo A\thooks\tdesc",
    "HADES_B\tx\tpath\tG\tRótulo B\tpanel\tdesc",
    "HADES_SLICE_HIGH\t4G\tsize\tG\tAlto\tslice\tdesc",
    "HADES_SLICE_MAX\t6G\tsize\tG\tMax\tslice\tdesc",
];

fn store(dir: &TempDir) -> ConfigStore {
    let kit = dir.path().join("kit");
    let d = dir.path().join("d");
    fs::create_dir_all(&kit).unwrap();
    fs::create_dir_all(&d).unwrap();
    fs::write(kit.join("config.tsv"), tsv(&KIT)).unwrap();
    ConfigStore { dir: d, kit }
}

#[test]
fn is_key_segue_a_regex() {
    assert!(is_key("A"));
    assert!(is_key("HADES_X1"));
    assert!(!is_key(""));
    assert!(!is_key("a"));
    assert!(!is_key("1A"));
    assert!(!is_key("A-B"));
    assert!(!is_key("A\n"));
}

#[test]
fn split_lines_como_python() {
    assert_eq!(split_lines("a\r\nb\rc\u{2028}d\n"), ["a", "b", "c", "d"]);
    assert_eq!(split_lines("a\n\nb"), ["a", "", "b"]);
    assert!(split_lines("").is_empty());
}

#[test]
fn env_sh_primeira_atribuicao_vale() {
    let m = parse_env_sh("A=1\nB=\"x y\"\nA=2\nC=v #c\n C=nao\nD=\"aberto\nE=\"\n1X=z\n");
    assert_eq!(m.get("A"), Some("1"));
    assert_eq!(m.get("B"), Some("x y"));
    assert_eq!(m.get("C"), Some("v"));
    assert_eq!(m.get("D"), Some(""));
    assert_eq!(m.get("E"), Some(""));
    assert!(!m.has("1X"));
    assert_eq!(
        m.iter().map(|(k, _)| k).collect::<Vec<_>>(),
        ["A", "B", "C", "D", "E"]
    );
}

#[test]
fn instance_id_e_unit_prefix() {
    let d = TempDir::new().unwrap();
    let env = Env::empty();
    assert_eq!(instance_id(d.path()), "");
    assert_eq!(unit_prefix(&env, d.path()), "hds");
    fs::write(
        d.path().join("env.sh"),
        "X=1\nHADES_ID=\"dev_2\" # c\nHADES_ID=outro\n",
    )
    .unwrap();
    assert_eq!(instance_id(d.path()), "dev_2");
    assert_eq!(unit_prefix(&env, d.path()), "hds_dev_2");
    assert_eq!(
        unit_prefix(&env.clone().with("HADES_ID", "Dev-X"), d.path()),
        "hds_dev_x"
    );
    let longo = "a".repeat(30);
    assert_eq!(
        unit_prefix(&env.with("HADES_ID", longo), d.path()),
        format!("hds_{}", "a".repeat(24))
    );
}

#[test]
fn catalogo_brando_plugin_vence_na_posicao_do_kit() {
    let d = TempDir::new().unwrap();
    fs::create_dir_all(d.path().join("plugin")).unwrap();
    fs::write(d.path().join("config.tsv"), "lixo\nA\t1\nB\nC\tc\textra\n").unwrap();
    fs::write(d.path().join("plugin/config.tsv"), "A\t9\nZ\tz\n").unwrap();
    let got: Vec<_> = read_catalog(d.path())
        .into_iter()
        .map(|e| (e.key, e.default))
        .collect();
    assert_eq!(
        got,
        [
            ("A".into(), "9".into()),
            ("B".into(), "".into()),
            ("C".into(), "c".into()),
            ("Z".into(), "z".into())
        ]
    );
}

#[test]
fn config_env_ignora_linha_sem_chave_valida() {
    let d = TempDir::new().unwrap();
    fs::write(
        d.path().join("config.env"),
        "# h\nA=1\nb=2\nC=\nA=3\nD=x=y\nsem igual\n",
    )
    .unwrap();
    let m = read_config_env(d.path());
    assert_eq!(m.get("A"), Some("3"));
    assert_eq!(m.get("C"), Some(""));
    assert_eq!(m.get("D"), Some("x=y"));
    assert_eq!(m.len(), 3);
}

#[test]
fn efetivo_ambiente_vence_config_env_vence_padrao() {
    let d = TempDir::new().unwrap();
    fs::write(d.path().join("config.tsv"), "A\t1\nB\t2\nC\t3\n").unwrap();
    fs::write(d.path().join("config.env"), "A=10\nB=20\nFORA=9\n").unwrap();
    let env = Env::empty().with("B", "").with("FORA", "7");
    let m = effective_config(d.path(), &env);
    assert_eq!(m.get("A"), Some("10"));
    assert_eq!(m.get("B"), Some(""));
    assert_eq!(m.get("C"), Some("3"));
    assert!(!m.has("FORA"));
}

#[test]
fn config_value_usa_o_d_do_cwd_e_cai_no_kit() {
    let home = TempDir::new().unwrap();
    let env = Env::empty().with("HOME", home.path().to_string_lossy());
    let kit = home.path().join(".claude/skills/hades/kit");
    fs::create_dir_all(&kit).unwrap();
    fs::write(kit.join("config.tsv"), "HADES_K\tkit\n").unwrap();
    let cwd = home.path().join("proj");
    fs::create_dir_all(&cwd).unwrap();
    assert_eq!(config_value(&env, &cwd, "HADES_K", "fb"), "kit");
    assert_eq!(config_value(&env, &cwd, "OUTRA", "fb"), "fb");

    let inst = home.path().join("inst");
    fs::create_dir_all(&inst).unwrap();
    fs::write(inst.join("env.sh"), "HADES_ID=a\n").unwrap();
    fs::write(inst.join("config.tsv"), "HADES_K\tpadrao\n").unwrap();
    fs::write(inst.join("config.env"), "HADES_K=dono\n").unwrap();
    let hh = home.path().join(".claude/hades");
    fs::create_dir_all(&hh).unwrap();
    fs::write(hh.join("instance"), inst.to_string_lossy().as_ref()).unwrap();
    assert_eq!(config_value(&env, &cwd, "HADES_K", "fb"), "dono");
    assert_eq!(
        config_value(&env.with("HADES_K", "amb"), &cwd, "HADES_K", "fb"),
        "amb"
    );
}

/// Divergência aceita do TS (decisão do dono): com `HADES_DATA` apontando outra instância, vale ela, não `<hh>/instance`.
#[test]
fn config_value_varias_instancias_segue_o_d_do_cwd() {
    let home = TempDir::new().unwrap();
    let a = home.path().join("a");
    let b = home.path().join("b");
    for (dir, v) in [(&a, "do-a"), (&b, "do-b")] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("env.sh"), "HADES_ID=x\n").unwrap();
        fs::write(dir.join("config.tsv"), "HADES_K\tpadrao\n").unwrap();
        fs::write(dir.join("config.env"), format!("HADES_K={v}\n")).unwrap();
    }
    let hh = home.path().join(".claude/hades");
    fs::create_dir_all(&hh).unwrap();
    fs::write(hh.join("instance"), a.to_string_lossy().as_ref()).unwrap();
    let env = Env::empty()
        .with("HOME", home.path().to_string_lossy())
        .with("HADES_DATA", b.to_string_lossy());
    assert_eq!(config_value(&env, home.path(), "HADES_K", "fb"), "do-b");
}

#[test]
fn catalogo_estrito_recusa_linha_ruim_e_marca_plugin() {
    let d = TempDir::new().unwrap();
    let s = store(&d);
    let items = catalog(&s).unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[1].kind, "path");
    assert!(!items[1].plugin);
    fs::create_dir_all(s.dir.join("plugin")).unwrap();
    fs::write(
        s.dir.join("plugin/config.tsv"),
        "HADES_A\t0\tbool\tG\tr\thooks\td\nHADES_P\tp\ttext\tP\tr\tnone\td\n",
    )
    .unwrap();
    let items = catalog(&s).unwrap();
    assert_eq!(items[0].default, "0");
    assert!(!items[0].plugin);
    assert_eq!(items[4].key, "HADES_P");
    assert!(items[4].plugin);
    fs::write(s.dir.join("plugin/config.tsv"), "ok\n").unwrap();
    let err = catalog(&s).unwrap_err();
    assert!(
        err.0
            .ends_with("linha 1 fora do formato (7 colunas TAB, chave em MAIÚSCULAS)"),
        "{err}"
    );
}

#[test]
fn sensibilidade_por_tipo() {
    let mk = |kind: &str| Item {
        key: "K".into(),
        default: String::new(),
        kind: kind.into(),
        group: String::new(),
        label: String::new(),
        apply: String::new(),
        desc: String::new(),
        plugin: false,
    };
    assert_eq!(sensitivity(&mk("secret")), Sensitivity::Secret);
    for t in ["path", "login", "logins", "emails", "digits", "url"] {
        assert_eq!(sensitivity(&mk(t)), Sensitivity::Identity, "{t}");
    }
    assert_eq!(sensitivity(&mk("bool")), Sensitivity::None);
}

#[test]
fn size_bytes_como_parse_int() {
    assert_eq!(size_bytes("4G"), Some(4.0 * 1024f64.powi(3)));
    assert_eq!(size_bytes("512M"), Some(512.0 * 1024f64.powi(2)));
    assert_eq!(size_bytes("x"), None);
    assert_eq!(size_bytes(""), None);
    assert_eq!(size_bytes("12abcG"), Some(12.0 * 1024f64.powi(3)));
}

#[test]
fn efetivo_do_store_marca_changed() {
    let d = TempDir::new().unwrap();
    let s = store(&d);
    fs::write(s.dir.join("config.env"), "HADES_A=1\nHADES_B=y\n").unwrap();
    let e = effective(&s).unwrap();
    assert_eq!((e[0].value.as_str(), e[0].changed), ("1", false));
    assert_eq!((e[1].value.as_str(), e[1].changed), ("y", true));
    assert_eq!((e[2].value.as_str(), e[2].changed), ("4G", false));
}

#[test]
fn save_grava_so_o_que_difere_ordem_do_catalogo_e_orfas() {
    let d = TempDir::new().unwrap();
    let s = store(&d);
    fs::write(s.dir.join("config.env"), "ORFA=o\nHADES_B=velho\n").unwrap();
    let applies = save(
        &s,
        &[
            ("HADES_B", " novo "),
            ("HADES_A", "0"),
            ("HADES_SLICE_HIGH", "4G"),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(applies, ["hooks", "panel"]);
    let text = fs::read_to_string(s.dir.join("config.env")).unwrap();
    assert_eq!(text, format!("{HEADER}HADES_A=0\nHADES_B=novo\nORFA=o\n"));
    assert!(s.dir.join("config.env.lock").exists());
    let applies = save(&s, &[], &["HADES_A", "HADES_B"]).unwrap();
    assert_eq!(applies, ["hooks", "panel"]);
    assert_eq!(
        fs::read_to_string(s.dir.join("config.env")).unwrap(),
        format!("{HEADER}ORFA=o\n")
    );
}

#[test]
fn save_recusa_chave_desconhecida_e_slice_invertido() {
    let d = TempDir::new().unwrap();
    let s = store(&d);
    assert_eq!(
        save(&s, &[("NADA", "1")], &[]).unwrap_err().0,
        "chave desconhecida: NADA"
    );
    assert_eq!(
        save(&s, &[], &["NADA"]).unwrap_err().0,
        "chave desconhecida: NADA"
    );
    assert_eq!(
        save(&s, &[("HADES_SLICE_HIGH", "7G")], &[]).unwrap_err().0,
        SLICE_ERROR
    );
    assert!(!s.dir.join("config.env").exists());
}

#[test]
fn save_concorrente_nao_perde_chave() {
    let d = TempDir::new().unwrap();
    let s = store(&d);
    std::thread::scope(|scope| {
        for (k, v) in [("HADES_A", "0"), ("HADES_B", "z")] {
            let s = s.clone();
            scope.spawn(move || {
                save(&s, &[(k, v)], &[]).unwrap();
            });
        }
    });
    let own = load_values(&s);
    assert_eq!(own.get("HADES_A"), Some("0"));
    assert_eq!(own.get("HADES_B"), Some("z"));
}
