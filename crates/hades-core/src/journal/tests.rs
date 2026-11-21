use super::*;
use serde_json::json;
use std::fs;
use tempfile::TempDir;

const NOW: i64 = 1_773_144_000_000; // 2026-03-10T12:00:00.000Z

fn settings() -> Settings {
    Settings {
        enabled: true,
        days: 14,
        max_bytes: 20 * MIB,
    }
}

fn ev(summary: &str, at: i64) -> JournalEvent {
    event_of_source(
        Source::Gate,
        "gate.done",
        summary,
        SourceFields {
            agent: Some("a1".into()),
            cwd: Some("/w".into()),
            ..Default::default()
        },
        at,
    )
}

fn register(root: &Path, project: &str, name: &str, worktree: &str, state: &str) {
    let dir = root.join(project).join("orchestration").join("agents");
    fs::create_dir_all(&dir).unwrap();
    let body = json!({"name": name, "worktree": worktree, "state": state});
    fs::write(dir.join(format!("{name}.json")), body.to_string()).unwrap();
}

fn lines(file: &Path) -> Vec<Value> {
    fs::read_to_string(file)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn iso_ms_e_parse_iso_sao_inversos() {
    assert_eq!(iso_ms(NOW), "2026-03-10T12:00:00.000Z");
    assert_eq!(parse_iso("2026-03-10T12:00:00.000Z"), Some(NOW));
    assert_eq!(parse_iso("2026-03-10T09:00:00-03:00"), Some(NOW));
    assert_eq!(parse_iso("2026-03-10"), Some(NOW - 12 * 3_600_000));
    assert_eq!(iso_ms(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(parse_iso("2024-02-30"), None);
    assert_eq!(parse_iso("x"), None);
    assert_eq!(parse_iso("2026-03-10T12:0"), None);
}

#[test]
fn parse_since_duracao_iso_com_fuso_e_recusas() {
    assert_eq!(parse_since("2h", NOW), Some(NOW - 7_200_000));
    assert_eq!(parse_since("30 m", NOW), Some(NOW - 1_800_000));
    assert_eq!(
        parse_since("2026-03-10T00:00:00Z", NOW),
        parse_iso("2026-03-10T00:00:00Z")
    );
    assert_eq!(parse_since("2026-03-10T00:00:00", NOW), None);
    assert_eq!(parse_since("ontem", NOW), None);
}

#[test]
fn config_padroes_e_fora_do_intervalo() {
    assert_eq!(journal_settings(&|_| None), settings());
    let s = journal_settings(&|k| {
        Some(
            match k {
                "HADES_JOURNAL" => "0",
                "HADES_JOURNAL_DAYS" => "3",
                "HADES_JOURNAL_MAX_MB" => "5",
                _ => return None,
            }
            .into(),
        )
    });
    assert_eq!(
        s,
        Settings {
            enabled: false,
            days: 3,
            max_bytes: 5 * MIB
        }
    );
    let s = journal_settings(&|k| match k {
        "HADES_JOURNAL_DAYS" => Some("0".into()),
        "HADES_JOURNAL_MAX_MB" => Some("abc".into()),
        _ => None,
    });
    assert_eq!((s.days, s.max_bytes), (14, 20 * MIB));
}

fn secret_pieces() -> [&'static str; 8] {
    [
        "ghp_",
        "abcdef0123456789abcdef",
        "s3nh4forte",
        "hunter2",
        "um valor com",
        "AKIA",
        "abc123",
        "Jhbgcioijiuzi1ni",
    ]
}

#[test]
fn mascara_cada_forma_de_segredo() {
    let a = "A1b2".repeat(8);
    let z = "Z".repeat(16);
    let cmds = [
        format!("gh auth login --with-token ghp_{a}"),
        r#"curl -H "Authorization: Bearer abcdef0123456789abcdef" https://x"#.to_owned(),
        "psql postgres://admin:s3nh4forte@db:5432/app".to_owned(),
        "deploy --password hunter2 --force".to_owned(),
        "DB_PASSWORD=hunter2 ./run.sh".to_owned(),
        "export API_TOKEN='um valor com espaço' && ls".to_owned(),
        format!("aws configure set key AKIA{z}"),
        "curl --api-key=abc123 https://x".to_owned(),
        "echo eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTYifQ.abcdefghijk".to_owned(),
    ];
    for cmd in cmds {
        let out = summarize_command(&cmd);
        for piece in secret_pieces() {
            assert!(!out.contains(piece), "{cmd} -> {out}");
        }
        assert!(out.contains("***"), "{cmd} -> {out}");
    }
}

#[test]
fn mascara_saidas_exatas() {
    assert_eq!(
        mask_secrets("DB_PASSWORD=hunter2 ./run.sh"),
        "DB_PASSWORD=*** ./run.sh"
    );
    assert_eq!(
        mask_secrets("deploy --password hunter2 --force"),
        "deploy --password *** --force"
    );
    assert_eq!(
        mask_secrets("Authorization: bearer abcdefgh12"),
        "Authorization: bearer ***"
    );
    assert_eq!(
        mask_secrets("postgres://admin:segredo@db"),
        "postgres://admin:***@db"
    );
    assert_eq!(mask_secrets("--api-key=abc"), "--api-key=***");
    assert_eq!(mask_secrets("senha: 'a b'"), "senha: ***");
    assert_eq!(mask_secrets("ação TOKEN=x"), "ação TOKEN=***");
}

#[test]
fn comando_sem_segredo_passa_e_mascarar_e_idempotente() {
    assert_eq!(
        summarize_command("git status --short"),
        "git status --short"
    );
    let once = mask_secrets("TOKEN=abc");
    assert_eq!(mask_secrets(&once), once);
}

#[test]
fn mascara_antes_do_corte() {
    let cmd = format!("{} --token supersecret123456", "a ".repeat(95));
    assert!(!summarize_command(&cmd).contains("supersecret"));
}

#[test]
fn heredoc_perde_o_corpo_e_clip_usa_utf16() {
    assert_eq!(summarize_command("cat <<EOF\nsegredo\nEOF"), "cat <<…");
    assert_eq!(clip("  a \n\t b  ", 10), "a b");
    assert_eq!(clip("abcdef", 4), "abc…");
    assert_eq!(clip("😀😀😀", 4), "😀…");
}

#[test]
fn linha_na_ordem_de_chaves_de_cada_origem() {
    let e = ev("ok", NOW);
    assert_eq!(
        e.to_json(),
        r#"{"v":1,"ts":"2026-03-10T12:00:00.000Z","agent":"a1","session":"","source":"gate","kind":"gate.done","summary":"ok","cwd":"/w"}"#
    );
    let mut hook = e.clone();
    hook.layout = Layout::Hook;
    hook.tool = Some("Bash".into());
    assert_eq!(
        hook.to_json(),
        r#"{"v":1,"ts":"2026-03-10T12:00:00.000Z","agent":"a1","session":"","source":"gate","cwd":"/w","kind":"gate.done","tool":"Bash","summary":"ok"}"#
    );
    let blocked = mark_blocked(&hook, "barrado\nsegunda");
    assert_eq!(blocked.kind, "tool_blocked");
    assert_eq!(blocked.summary, "barrado | ok");
}

#[test]
fn decisao_mascara_e_corta_alternativas() {
    let d = Decision {
        level: Number::from(2),
        needed: Number::from(3),
        question: "usar TOKEN=abc".into(),
        chosen: "sim".into(),
        alternatives: (0..7).map(|i| format!("alt{i}")).collect(),
    };
    let e = event_of_source(
        Source::Orch,
        "decision",
        "d",
        SourceFields {
            decision: Some(d),
            ..Default::default()
        },
        NOW,
    );
    let json = e.to_json();
    assert!(json.contains(r#""decision":{"level":2,"needed":3,"question":"usar TOKEN=***","chosen":"sim","alternatives":["alt0","alt1","alt2","alt3","alt4"]}"#), "{json}");
}

#[test]
fn parse_recusa_linha_cortada_e_fora_do_formato() {
    assert!(parse(r#"{"v":1,"ts":"2026-03-10T12:0"#).is_none());
    let bad_ts = json!({"v":1,"ts":"x","source":"guard","kind":"k","summary":"s"});
    assert!(parse(&bad_ts.to_string()).is_none());
    let bad_source = json!({"v":1,"ts":iso_ms(NOW),"source":"outra","kind":"k","summary":"s"});
    assert!(parse(&bad_source.to_string()).is_none());
}

#[test]
fn parse_guarda_campo_desconhecido_e_reserializa() {
    let line = json!({"v":1,"ts":iso_ms(NOW),"agent":"a","session":"","source":"watch","kind":"k","tool":"T","summary":"s","cwd":"/c","novo":{"x":1}});
    let e = parse(&line.to_string()).unwrap();
    assert_eq!(e.extra["novo"], json!({"x": 1}));
    assert_eq!(e.to_json(), line.to_string());
}

#[test]
fn uma_linha_json_por_evento_em_append() {
    let t = TempDir::new().unwrap();
    let target = t.path().join("p/journal/a1.jsonl");
    assert!(append(&ev("um", NOW), &target, &settings(), NOW));
    assert!(append(&ev("dois", NOW + 1), &target, &settings(), NOW));
    let got = lines(&target);
    assert_eq!(got.len(), 2);
    assert_eq!(got[1]["summary"], "dois");
}

#[test]
fn diario_desligado_nao_grava_nem_cria_pasta() {
    let t = TempDir::new().unwrap();
    let target = t.path().join("p/journal/a1.jsonl");
    let off = Settings {
        enabled: false,
        ..settings()
    };
    assert!(!append(&ev("x", NOW), &target, &off, NOW));
    assert!(!t.path().join("p").exists());
}

#[test]
fn no_teto_gira_para_o_arquivo_1() {
    let t = TempDir::new().unwrap();
    let target = t.path().join("j/a1.jsonl");
    let small = Settings {
        max_bytes: 200,
        ..settings()
    };
    for i in 0..6 {
        assert!(append(
            &ev(&format!("evento {i} com algum texto"), NOW),
            &target,
            &small,
            NOW
        ));
    }
    assert!(t.path().join("j/a1.1.jsonl").exists());
    assert!(fs::metadata(&target).unwrap().len() < 400);
    assert_eq!(
        rotated_name(Path::new("/x/a.jsonl")),
        Path::new("/x/a.1.jsonl")
    );
}

#[test]
fn disco_com_erro_nao_falha_e_conta_a_perda() {
    let t = TempDir::new().unwrap();
    let file = t.path().join("arquivo");
    fs::write(&file, "nao e pasta").unwrap();
    let before = dropped_count();
    assert!(!append(
        &ev("x", NOW),
        &file.join("j/a1.jsonl"),
        &settings(),
        NOW
    ));
    assert!(dropped_count() > before);
}

#[test]
fn lock_ocupado_por_outro_nao_espera_e_anota_o_drop() {
    let t = TempDir::new().unwrap();
    let dir = t.path().join("j");
    fs::create_dir_all(&dir).unwrap();
    let held = crate::lock::lock(&dir.join(".lock")).unwrap();
    let started = std::time::Instant::now();
    assert!(!append(
        &ev("x", NOW),
        &dir.join("a1.jsonl"),
        &settings(),
        NOW
    ));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!dir.join("a1.jsonl").exists());
    let dropped = fs::read_to_string(dir.join(".dropped")).unwrap();
    assert!(dropped.ends_with("\tlock ocupado\n"), "{dropped}");
    drop(held);
    assert!(append(
        &ev("y", NOW),
        &dir.join("a1.jsonl"),
        &settings(),
        NOW
    ));
}

#[test]
fn gravacoes_concorrentes_nao_misturam_linhas() {
    let t = TempDir::new().unwrap();
    let target = t.path().join("j/a1.jsonl");
    let threads: Vec<_> = (0..4)
        .map(|n| {
            let target = target.clone();
            thread::spawn(move || {
                let mut ok = 0;
                for i in 0..40 {
                    if append(&ev(&format!("t{n}-{i}"), NOW), &target, &settings(), NOW) {
                        ok += 1;
                    }
                }
                ok
            })
        })
        .collect();
    let wrote: usize = threads.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(lines(&target).len(), wrote);
}

#[test]
fn linha_enorme_e_encurtada() {
    let t = TempDir::new().unwrap();
    let target = t.path().join("j/a1.jsonl");
    let mut e = ev("x", NOW);
    e.summary = "a".repeat(4000);
    assert!(append(&e, &target, &settings(), NOW));
    let got = lines(&target);
    assert_eq!(got[0]["summary"].as_str().unwrap().chars().count(), 80);
    assert!(fs::read(&target).unwrap().len() <= LINE_MAX_BYTES);
}

#[test]
fn agente_pelo_cwd_dentro_da_worktree() {
    let t = TempDir::new().unwrap();
    let root = t.path();
    let wt = |s: &str| t.path().join("wt").join(s).to_string_lossy().into_owned();
    register(
        root,
        "example__monorepo",
        "ag-ativo",
        &wt("ativo"),
        "working",
    );
    register(root, "example__monorepo", "ag-done", &wt("done"), "done");
    let home = agent_of_cwd(&format!("{}/src/x", wt("ativo")), root).unwrap();
    assert_eq!(home.agent, "ag-ativo");
    assert_eq!(home.dir, root.join("example__monorepo/journal"));
    assert_eq!(agent_of_cwd(&wt("ativo"), root).unwrap().agent, "ag-ativo");
    assert!(agent_of_cwd(&wt("ativo-outro"), root).is_none());
    assert!(agent_of_cwd(&wt("done"), root).is_none());
    assert!(agent_of_cwd("", root).is_none());
}

#[test]
fn alvo_da_unidade_skill_e_da_sessao_sem_agente() {
    let t = TempDir::new().unwrap();
    let root = t.path();
    register(root, "_skill", "un-skill", "/wt/skill", "working");
    let home = agent_of_cwd("/wt/skill", root);
    let e = event_of_source(Source::Guard, "tool_pre", "x", SourceFields::default(), NOW);
    assert_eq!(
        journal_target(&e, home.as_ref(), root),
        root.join("_skill/journal/un-skill.jsonl")
    );
    let orphan = event_of_source(
        Source::Guard,
        "tool_pre",
        "x",
        SourceFields {
            session: Some("sess/../1".into()),
            ..Default::default()
        },
        NOW,
    );
    assert_eq!(
        journal_target(&orphan, None, root),
        root.join("_maquina/journal/_sessions/sess1.jsonl")
    );
    assert!(agent_home_by_name("un-skill", root).is_some());
    assert!(agent_home_by_name("nada", root).is_none());
}

#[test]
fn emit_grava_no_agente_nomeado_ou_na_sessao_da_fonte() {
    let t = TempDir::new().unwrap();
    let root = t.path().to_path_buf();
    register(&root, "p", "ag1", "/wt/ag1", "working");
    let opts = |agent: Option<&str>, cwd: &str| EmitOptions {
        agent: agent.map(str::to_owned),
        cwd: Some(cwd.into()),
        now_ms: Some(NOW),
        root: Some(root.clone()),
        ..Default::default()
    };
    assert!(emit(
        Source::Watch,
        "watch.parked",
        "PARKED TOKEN=zzz",
        opts(Some("ag1"), "/qualquer")
    ));
    let got = lines(&root.join("p/journal/ag1.jsonl"));
    assert_eq!(got[0]["agent"], "ag1");
    assert_eq!(got[0]["summary"], "PARKED TOKEN=***");
    assert!(emit(
        Source::Gate,
        "gate.done",
        "x",
        opts(None, "/wt/ag1/sub")
    ));
    assert_eq!(lines(&root.join("p/journal/ag1.jsonl")).len(), 2);
    assert!(emit(Source::Cerberus, "c", "x", opts(None, "/fora")));
    let orphan = lines(&root.join("_maquina/journal/_sessions/_cerberus.jsonl"));
    assert_eq!(orphan[0]["agent"], "");
    assert_eq!(orphan[0]["session"], "_cerberus");
    let off = EmitOptions {
        settings: Some(Settings {
            enabled: false,
            ..settings()
        }),
        ..opts(None, "/fora")
    };
    assert!(!emit(Source::Orch, "o", "x", off));
}

#[test]
fn leitura_filtra_por_since_kind_limit_e_pula_linha_quebrada() {
    let t = TempDir::new().unwrap();
    let dir = t.path().join("p/journal");
    fs::create_dir_all(&dir).unwrap();
    let at = |m: i64| NOW - m * 60_000;
    let e = |kind: &str, m: i64| {
        event_of_source(
            Source::Gate,
            kind,
            kind,
            SourceFields {
                agent: Some("a1".into()),
                ..Default::default()
            },
            at(m),
        )
        .to_json()
    };
    fs::write(
        dir.join("a1.jsonl"),
        format!(
            "{}\n{{\"quebrada\"\n{}\n{}\n{}\n",
            e("gate.admit", 50),
            e("tool_pre", 20),
            e("tool_post", 10),
            e("gate.done", 5)
        ),
    )
    .unwrap();
    fs::write(dir.join("outro.jsonl"), format!("{}\n", e("tool_pre", 1))).unwrap();
    let q = |extra: &dyn Fn(&mut Query)| {
        let mut query = Query {
            agent: Some("a1".into()),
            limit: 100,
            ..Default::default()
        };
        extra(&mut query);
        read_events(&query, t.path())
            .into_iter()
            .map(|x| x.kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        q(&|_| {}),
        ["gate.admit", "tool_pre", "tool_post", "gate.done"]
    );
    assert_eq!(
        q(&|x| x.since_ms = parse_since("30m", NOW)),
        ["tool_pre", "tool_post", "gate.done"]
    );
    assert_eq!(
        q(&|x| x.kind = Some("tool".into())),
        ["tool_pre", "tool_post"]
    );
    assert_eq!(
        q(&|x| x.kind = Some("gate".into())),
        ["gate.admit", "gate.done"]
    );
    assert_eq!(q(&|x| x.limit = 2), ["tool_post", "gate.done"]);
    let all = Query {
        limit: 100,
        ..Default::default()
    };
    assert_eq!(read_events(&all, t.path()).len(), 5);
}

#[test]
fn read_tail_corta_a_primeira_linha_parcial() {
    let t = TempDir::new().unwrap();
    let f = t.path().join("a.jsonl");
    fs::write(&f, "linha-um\nlinha-dois\nlinha-tres\n").unwrap();
    assert_eq!(read_tail(&f, 1000), "linha-um\nlinha-dois\nlinha-tres\n");
    assert_eq!(read_tail(&f, 14), "linha-tres\n");
    assert_eq!(read_tail(&t.path().join("nao"), 10), "");
}

#[test]
fn files_e_list_journals_somam_a_geracao_1_e_as_sessoes() {
    let t = TempDir::new().unwrap();
    let dir = t.path().join("p/journal");
    fs::create_dir_all(dir.join("_sessions")).unwrap();
    let a = ev("a", NOW).to_json();
    let b = ev("b", NOW + 60_000).to_json();
    fs::write(dir.join("a1.1.jsonl"), format!("{a}\n")).unwrap();
    fs::write(dir.join("a1.jsonl"), format!("{b}\n")).unwrap();
    fs::write(dir.join("_sessions/s1.jsonl"), format!("{a}\n")).unwrap();
    fs::write(dir.join("ruim nome.jsonl"), "x\n").unwrap();
    let names: Vec<_> = files(t.path(), true).into_iter().map(|f| f.agent).collect();
    assert_eq!(names, ["a1", "a1", "_sessions/s1"]);
    assert_eq!(files(t.path(), false).len(), 2);
    let list = list_journals(t.path());
    assert_eq!(list[0].agent, "a1");
    assert_eq!(
        (list[0].events, list[0].last.as_str()),
        (2, "2026-03-10T12:01:00.000Z")
    );
    assert_eq!(list[1].agent, "_sessions/s1");
}

fn set_mtime(path: &Path, ms: i64) {
    let t = UNIX_EPOCH + Duration::from_millis(ms as u64);
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn retencao_apaga_parado_reescreve_vivo_e_e_idempotente() {
    let t = TempDir::new().unwrap();
    let dir = t.path().join("p/journal");
    fs::create_dir_all(dir.join("_sessions")).unwrap();
    let old = parse_iso("2026-02-01T00:00:00Z").unwrap();
    let fresh = parse_iso("2026-03-09T00:00:00Z").unwrap();
    let mk = |s: &str, at: i64| ev(s, at).to_json();
    fs::write(dir.join("parado.jsonl"), format!("{}\n", mk("velho", old))).unwrap();
    set_mtime(&dir.join("parado.jsonl"), old);
    fs::write(
        dir.join("_sessions/s9.jsonl"),
        format!("{}\n", mk("sessao velha", old)),
    )
    .unwrap();
    set_mtime(&dir.join("_sessions/s9.jsonl"), old);
    fs::write(
        dir.join("vivo.jsonl"),
        format!(
            "{}\n{}\nlinha cortada\n",
            mk("velho", old),
            mk("novo", fresh)
        ),
    )
    .unwrap();
    let plan = prune(14, NOW, t.path(), true);
    assert_eq!((plan.removed, plan.rewritten), (2, 1));
    assert!(dir.join("parado.jsonl").exists());
    let done = prune(14, NOW, t.path(), false);
    assert_eq!((done.removed, done.rewritten), (2, 1));
    assert!(!dir.join("parado.jsonl").exists());
    assert!(!dir.join("_sessions/s9.jsonl").exists());
    let vivo = fs::read_to_string(dir.join("vivo.jsonl")).unwrap();
    assert!(!vivo.contains("velho") && vivo.contains("novo") && vivo.contains("linha cortada"));
    let again = prune(14, NOW, t.path(), false);
    assert_eq!((again.removed, again.rewritten), (0, 0));
}
