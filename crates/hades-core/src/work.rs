//! Ambiente de trabalho do loop (`src/lib/work.ts`): repositório → slug → `<work-root>/<slug>`, e a leitura dos
//! registros dos agentes.
//!
//! Divergência aceita (plano, "Decisões do dono" 4): `load_work_env` **não cria** as pastas. O TS cria `locks/`,
//! `state/`, `orchestration/agents/` e `orchestration/consolidations/` até em comando de leitura
//! (`src/lib/work.ts:61`); aqui quem grava chama [`ensure_dirs`]. `config_value` (`src/lib/work.ts:140`) fica para a
//! PR do `config`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::env::Env;
use crate::fsx::{is_file, list_dir, read_text};
use crate::jsonfmt::json_compact;
use crate::output::{HadesError, die, exit, warn};
use crate::paths::{skill_root, slug_of_remote, work_root};
use crate::proc::{RunOptions, run};

/// `src/lib/work.ts:12`.
pub const DEFAULT_REPORTS: &str = ".design-sync-reports";

/// Objeto JSON com a ordem das chaves do arquivo (`Json` em `src/lib/work.ts:117`).
pub type Json = Map<String, Value>;

/// `WorkEnv` (`src/lib/work.ts:14`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkEnv {
    pub skill_scope: bool,
    pub repo_top: PathBuf,
    pub main_clone: PathBuf,
    pub slug: String,
    pub repo_name: String,
    pub work: PathBuf,
    pub locks: PathBuf,
    pub state: PathBuf,
    pub orch: PathBuf,
    pub skill_orch: PathBuf,
    pub reports: String,
}

/// `WorkOptions` (`src/lib/work.ts:28`).
#[derive(Clone, Debug, Default)]
pub struct WorkOptions {
    /// `HADES_LIB_SKILL_OK`: o script aceita a raiz própria `_skill` quando estiver na skill.
    pub skill_ok: bool,
    /// `HADES_LIB_SKILL_FORCE`: usa `_skill` de qualquer diretório.
    pub skill_force: bool,
    /// `HADES_LIB_AGENT`: usa `_skill` se o agente está lá, e traz para lá a unidade skill de um skill-new antigo.
    pub agent: Option<String>,
    pub cwd: Option<PathBuf>,
}

/// `git -C <dir> <args>`, stdout aparado (`src/lib/work.ts:36`); vazio em qualquer falha.
fn git(dir: &Path, args: &[&str]) -> String {
    let dir = dir.to_string_lossy();
    let mut full = vec!["-C", dir.as_ref()];
    full.extend_from_slice(args);
    run("git", &full, &RunOptions::default())
        .stdout
        .trim()
        .to_owned()
}

/// "Estou na skill": diretório sem origin (skill, unidade ou fora de git) ou o mesmo git da skill
/// (`src/lib/work.ts:45`).
pub fn in_skill_scope(env: &Env, cwd: &Path) -> bool {
    let top = git(cwd, &["rev-parse", "--show-toplevel"]);
    if top.is_empty() {
        return true;
    }
    let top = Path::new(&top);
    if git(top, &["config", "--get", "remote.origin.url"]).is_empty() {
        return true;
    }
    let common = git(
        top,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    let here = git(
        &skill_root(env),
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    !common.is_empty() && common == here
}

/// Pastas e `reports` de um slug, sem criá-las (`workDirs`, `src/lib/work.ts:56`).
fn work_dirs(slug: &str, env: &Env) -> (PathBuf, PathBuf, PathBuf, PathBuf, String) {
    let work = work_root(env).join(slug);
    let orch = work.join("orchestration");
    let reports = env
        .get("HADES_ORCH_REPORTS_DIR")
        .unwrap_or(DEFAULT_REPORTS)
        .to_owned();
    (work.join("locks"), work.join("state"), orch, work, reports)
}

/// Cria as pastas que `workDirs` cria no TS (`src/lib/work.ts:61`); para quem grava.
pub fn ensure_dirs(work: &WorkEnv) -> io::Result<()> {
    for dir in [
        work.locks.clone(),
        work.state.clone(),
        work.orch.join("agents"),
        work.orch.join("consolidations"),
    ] {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn skill_env(env: &Env) -> WorkEnv {
    let root = skill_root(env);
    let (locks, state, orch, work, reports) = work_dirs("_skill", env);
    WorkEnv {
        skill_scope: true,
        repo_top: root.clone(),
        main_clone: root,
        slug: "_skill".to_owned(),
        repo_name: "_skill".to_owned(),
        work,
        locks,
        state,
        orch,
        skill_orch: work_root(env).join("_skill").join("orchestration"),
        reports,
    }
}

/// `loadWorkEnv` (`src/lib/work.ts:84`). Fora de repositório e sem `origin` a recusa é a do TS (código `USAGE`).
pub fn load_work_env(options: &WorkOptions, env: &Env) -> Result<WorkEnv, HadesError> {
    let cwd = options
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let skill_orch = work_root(env).join("_skill").join("orchestration");
    let agent_in_skill = options
        .agent
        .as_deref()
        .is_some_and(|a| is_file(&skill_orch.join("agents").join(format!("{a}.json"))));
    if options.skill_ok && (options.skill_force || agent_in_skill || in_skill_scope(env, &cwd)) {
        return Ok(skill_env(env));
    }

    let repo_top = git(&cwd, &["rev-parse", "--show-toplevel"]);
    if repo_top.is_empty() {
        return die(
            "rode dentro de um clone ou worktree do repositório",
            exit::USAGE,
            None,
        );
    }
    let repo_top = PathBuf::from(repo_top);
    let listing = git(&repo_top, &["worktree", "list", "--porcelain"]);
    let first = listing.split('\n').next().unwrap_or("");
    let main_clone = PathBuf::from(first.strip_prefix("worktree ").unwrap_or(first));
    let url = git(&repo_top, &["config", "--get", "remote.origin.url"]);
    if url.is_empty() {
        return die("clone sem remote.origin.url", exit::USAGE, None);
    }
    let slug = slug_of_remote(&url);
    let repo_name = slug
        .find("__")
        .map_or(slug.as_str(), |i| &slug[i + 2..])
        .to_owned();
    let (locks, state, orch, work, reports) = work_dirs(&slug, env);
    let found = WorkEnv {
        skill_scope: false,
        repo_top,
        main_clone,
        slug,
        repo_name,
        work,
        locks,
        state,
        orch,
        skill_orch,
        reports,
    };
    Ok(move_skill_unit(found, options, env))
}

/// Unidade skill que um skill-new antigo gravou no registro do projeto passa para o registro único `_skill`
/// (`src/lib/work.ts:107`).
fn move_skill_unit(found: WorkEnv, options: &WorkOptions, env: &Env) -> WorkEnv {
    let Some(agent) = options.agent.as_deref().filter(|_| options.skill_ok) else {
        return found;
    };
    let file = found.orch.join("agents").join(format!("{agent}.json"));
    let is_skill = read_json(&file)
        .and_then(|j| j.get("mode").and_then(Value::as_str).map(|m| m == "skill"))
        .unwrap_or(false);
    if !is_skill {
        return found;
    }
    let dest_dir = found.skill_orch.join("agents");
    let moved = fs::create_dir_all(&dest_dir)
        .and_then(|()| fs::rename(&file, dest_dir.join(format!("{agent}.json"))));
    if moved.is_err() {
        return found;
    }
    warn(&format!(
        "hades: unidade {agent} movida do registro do projeto para o registro único _skill"
    ));
    skill_env(env)
}

/// `readJson` (`src/lib/work.ts:119`): objeto JSON do arquivo; `None` se não lê, não parseia ou não é objeto.
pub fn read_json(file: &Path) -> Option<Json> {
    match serde_json::from_str::<Value>(&read_text(file)?) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// `agentFile` (`src/lib/work.ts:134`).
pub fn agent_file(work: &WorkEnv, name: &str) -> PathBuf {
    work.orch.join("agents").join(format!("{name}.json"))
}

/// `regGet` (`src/lib/work.ts:139`): campo do registro de um agente; vazio se o registro ou o campo não existe, ou se
/// é `null`/`false`. String sai crua, o resto como `JSON.stringify`.
pub fn reg_get(work: &WorkEnv, name: &str, field: &str) -> String {
    match read_json(&agent_file(work, name)).and_then(|mut j| j.remove(field)) {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s,
        Some(other) => json_compact(&other),
    }
}

/// `openQuestions` (`src/lib/work.ts:154`): perguntas abertas em `<worktree>/<reports>/questions.md`: `## Qn` sem
/// "(respondida)" nem `## Resposta do dono` depois.
pub fn open_questions(worktree: &Path, reports: &str) -> usize {
    let Some(text) = read_text(&worktree.join(reports).join("questions.md")) else {
        return 0;
    };
    let mut open: Vec<bool> = Vec::new();
    for line in text.split('\n') {
        if is_question_head(line) {
            open.push(!line.contains("(respondida)"));
        } else if line.starts_with("## Resposta do dono")
            && let Some(last) = open.last_mut()
        {
            *last = false;
        }
    }
    open.into_iter().filter(|o| *o).count()
}

/// `/^## Q[0-9]+/` (`src/lib/work.ts:156`).
fn is_question_head(line: &str) -> bool {
    line.strip_prefix("## Q")
        .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
}

/// `countQuestions` (`src/lib/work.ts:169`): todas as perguntas, abertas ou não.
pub fn count_questions(worktree: &Path, reports: &str) -> usize {
    read_text(&worktree.join(reports).join("questions.md"))
        .unwrap_or_default()
        .split('\n')
        .filter(|l| is_question_head(l))
        .count()
}

/// `agentFiles` (`src/lib/work.ts:177`): registros do ambiente e, fora da skill, as unidades do registro único `_skill`.
pub fn agent_files(work: &WorkEnv) -> Vec<PathBuf> {
    let mut dirs = vec![work.orch.join("agents")];
    if !work.skill_scope {
        dirs.push(work.skill_orch.join("agents"));
    }
    dirs.into_iter()
        .flat_map(|dir| {
            list_dir(&dir)
                .into_iter()
                .filter(|n| n.ends_with(".json"))
                .map(|n| dir.join(n))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo(parent: &Path, name: &str, origin: Option<&str>) -> PathBuf {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q"]);
        if let Some(url) = origin {
            sh(&dir, &["remote", "add", "origin", url]);
        }
        fs::canonicalize(&dir).unwrap()
    }

    fn env_in(home: &Path) -> Env {
        Env::empty()
            .with("HOME", home.to_string_lossy())
            .with("HADES_SKILL_DIR", home.join("skill").to_string_lossy())
    }

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
            reports: DEFAULT_REPORTS.into(),
        }
    }

    #[test]
    fn projeto_deriva_slug_e_pastas_sem_criar_nada() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let r = repo(t.path(), "clone", Some("git@github.com:acme/demo.git"));
        let opts = WorkOptions {
            cwd: Some(r.clone()),
            ..Default::default()
        };
        let w = load_work_env(&opts, &env_in(&home)).unwrap();
        let base = home.join(".claude/hades/work/acme__demo");
        assert_eq!(
            (w.skill_scope, w.slug.as_str(), w.repo_name.as_str()),
            (false, "acme__demo", "demo")
        );
        assert_eq!(w.repo_top, r);
        assert_eq!(w.main_clone, r);
        assert_eq!(w.work, base);
        assert_eq!(w.locks, base.join("locks"));
        assert_eq!(w.state, base.join("state"));
        assert_eq!(w.orch, base.join("orchestration"));
        assert_eq!(
            w.skill_orch,
            home.join(".claude/hades/work/_skill/orchestration")
        );
        assert_eq!(w.reports, DEFAULT_REPORTS);
        assert!(!base.exists(), "leitura não cria pasta");
        ensure_dirs(&w).unwrap();
        for d in [
            "locks",
            "state",
            "orchestration/agents",
            "orchestration/consolidations",
        ] {
            assert!(base.join(d).is_dir(), "{d}");
        }
    }

    #[test]
    fn work_root_e_reports_vem_do_ambiente() {
        let t = tempfile::tempdir().unwrap();
        let r = repo(t.path(), "c", Some("https://github.com/o/r.git"));
        let env = env_in(t.path())
            .with("HADES_WORK_ROOT", t.path().join("wr").to_string_lossy())
            .with("HADES_ORCH_REPORTS_DIR", ".rep");
        let opts = WorkOptions {
            cwd: Some(r),
            ..Default::default()
        };
        let w = load_work_env(&opts, &env).unwrap();
        assert_eq!(w.work, t.path().join("wr/o__r"));
        assert_eq!(w.reports, ".rep");
    }

    #[test]
    fn worktree_aponta_o_clone_principal() {
        let t = tempfile::tempdir().unwrap();
        let main = repo(t.path(), "main", Some("https://github.com/o/r"));
        sh(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        );
        let wt = t.path().join("wt");
        sh(
            &main,
            &["worktree", "add", "-q", "-b", "b", &wt.to_string_lossy()],
        );
        let opts = WorkOptions {
            cwd: Some(wt.clone()),
            ..Default::default()
        };
        let w = load_work_env(&opts, &env_in(t.path())).unwrap();
        assert_eq!(w.main_clone, main);
        assert_eq!(w.repo_top, fs::canonicalize(&wt).unwrap());
    }

    #[test]
    fn fora_de_repo_e_sem_origin_recusam_com_a_mensagem_do_ts() {
        let t = tempfile::tempdir().unwrap();
        let plain = t.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let env = env_in(t.path());
        let at = |cwd: PathBuf| {
            load_work_env(
                &WorkOptions {
                    cwd: Some(cwd),
                    ..Default::default()
                },
                &env,
            )
        };
        let e = at(plain).unwrap_err();
        assert_eq!(
            e.message,
            "rode dentro de um clone ou worktree do repositório"
        );
        assert_eq!(e.code, exit::USAGE);
        let e = at(repo(t.path(), "semorigin", None)).unwrap_err();
        assert_eq!(e.message, "clone sem remote.origin.url");
    }

    #[test]
    fn escopo_skill_so_com_skill_ok_e_um_dos_gatilhos() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        let sem_origin = repo(t.path(), "unidade", None);
        let com_origin = repo(t.path(), "proj", Some("https://github.com/o/r"));
        let opts = |cwd: &Path, f: fn(&mut WorkOptions)| {
            let mut o = WorkOptions {
                cwd: Some(cwd.to_owned()),
                ..Default::default()
            };
            f(&mut o);
            o
        };
        let skill = load_work_env(&opts(&sem_origin, |o| o.skill_ok = true), &env).unwrap();
        assert!(skill.skill_scope);
        assert_eq!(skill.slug, "_skill");
        assert_eq!(skill.repo_name, "_skill");
        assert_eq!(skill.repo_top, t.path().join("skill"));
        assert_eq!(skill.work, t.path().join(".claude/hades/work/_skill"));
        assert!(load_work_env(&opts(&sem_origin, |_| {}), &env).is_err());
        assert!(
            !load_work_env(&opts(&com_origin, |o| o.skill_ok = true), &env)
                .unwrap()
                .skill_scope
        );
        let forced = |o: &mut WorkOptions| {
            o.skill_ok = true;
            o.skill_force = true;
        };
        assert!(
            load_work_env(&opts(&com_origin, forced), &env)
                .unwrap()
                .skill_scope
        );
    }

    #[test]
    fn agente_ja_registrado_na_skill_usa_o_registro_skill() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        let com_origin = repo(t.path(), "proj", Some("https://github.com/o/r"));
        let agents = t
            .path()
            .join(".claude/hades/work/_skill/orchestration/agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(agents.join("u1.json"), "{}").unwrap();
        let opts = |agent: &str| WorkOptions {
            skill_ok: true,
            agent: Some(agent.into()),
            cwd: Some(com_origin.clone()),
            ..Default::default()
        };
        assert!(load_work_env(&opts("u1"), &env).unwrap().skill_scope);
        assert!(!load_work_env(&opts("outro"), &env).unwrap().skill_scope);
    }

    #[test]
    fn unidade_skill_antiga_no_projeto_passa_para_o_registro_unico() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        let r = repo(t.path(), "proj", Some("https://github.com/o/r"));
        let proj_agents = t
            .path()
            .join(".claude/hades/work/o__r/orchestration/agents");
        fs::create_dir_all(&proj_agents).unwrap();
        fs::write(proj_agents.join("u.json"), r#"{"name":"u","mode":"skill"}"#).unwrap();
        fs::write(
            proj_agents.join("d.json"),
            r#"{"name":"d","mode":"deliver"}"#,
        )
        .unwrap();
        let opts = |agent: &str| WorkOptions {
            skill_ok: true,
            agent: Some(agent.into()),
            cwd: Some(r.clone()),
            ..Default::default()
        };
        assert!(!load_work_env(&opts("d"), &env).unwrap().skill_scope);
        assert!(proj_agents.join("d.json").exists());
        let w = load_work_env(&opts("u"), &env).unwrap();
        assert!(w.skill_scope);
        assert!(!proj_agents.join("u.json").exists());
        assert!(w.skill_orch.join("agents/u.json").is_file());
    }

    #[test]
    fn read_json_so_aceita_objeto_e_preserva_a_ordem() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.json");
        assert!(read_json(&p).is_none());
        for ruim in ["[1]", "3", "null", "{", ""] {
            fs::write(&p, ruim).unwrap();
            assert!(read_json(&p).is_none(), "{ruim}");
        }
        fs::write(&p, r#"{"z":1,"a":2,"m":{"y":1,"b":2}}"#).unwrap();
        let j = read_json(&p).unwrap();
        assert_eq!(j.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
    }

    #[test]
    fn reg_get_devolve_vazio_para_ausente_null_e_false() {
        let t = tempfile::tempdir().unwrap();
        let w = work_env(t.path());
        assert_eq!(reg_get(&w, "x", "state"), "");
        fs::create_dir_all(w.orch.join("agents")).unwrap();
        fs::write(
            agent_file(&w, "x"),
            r#"{"state":"parked","pr":12,"n":null,"f":false,"t":true,"o":{"a":[1,"b"]},"z":0,"e":""}"#,
        )
        .unwrap();
        let g = |f: &str| reg_get(&w, "x", f);
        assert_eq!(g("state"), "parked");
        assert_eq!(g("pr"), "12");
        assert_eq!(g("n"), "");
        assert_eq!(g("f"), "");
        assert_eq!(g("t"), "true");
        assert_eq!(g("o"), r#"{"a":[1,"b"]}"#);
        assert_eq!(g("z"), "0");
        assert_eq!(g("e"), "");
        assert_eq!(g("nao-existe"), "");
    }

    fn questions(t: &Path, text: &str) -> usize {
        fs::create_dir_all(t.join(DEFAULT_REPORTS)).unwrap();
        fs::write(t.join(DEFAULT_REPORTS).join("questions.md"), text).unwrap();
        open_questions(t, DEFAULT_REPORTS)
    }

    #[test]
    fn open_questions_sem_arquivo_e_zero() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(open_questions(t.path(), DEFAULT_REPORTS), 0);
        assert_eq!(count_questions(t.path(), DEFAULT_REPORTS), 0);
    }

    #[test]
    fn open_questions_respeita_respondida_e_resposta_do_dono() {
        let t = tempfile::tempdir().unwrap();
        let texto = "# Perguntas\n## Q1 a\ntexto\n## Q2 b (respondida)\n## Q3 c\n## Resposta do dono (Q3)\nsim\n## Q4 d\n## Qx nao\n## Q10 e\n";
        assert_eq!(questions(t.path(), texto), 3);
        assert_eq!(count_questions(t.path(), DEFAULT_REPORTS), 5);
    }

    #[test]
    fn resposta_do_dono_sem_pergunta_antes_e_ignorada() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(questions(t.path(), "## Resposta do dono\n## Q1 a\n"), 1);
    }

    #[test]
    fn resposta_fecha_so_a_pergunta_anterior() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(
            questions(t.path(), "## Q1\n## Q2\n## Resposta do dono\n"),
            1
        );
    }

    #[test]
    fn agent_files_lista_projeto_e_skill_so_fora_do_escopo_skill() {
        let t = tempfile::tempdir().unwrap();
        let mut w = work_env(t.path());
        let (a, b) = (w.orch.join("agents"), w.skill_orch.join("agents"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        for f in ["b.json", "a.json", "x.lock", "c.txt"] {
            fs::write(a.join(f), "{}").unwrap();
        }
        fs::write(b.join("s.json"), "{}").unwrap();
        assert_eq!(
            agent_files(&w),
            [a.join("a.json"), a.join("b.json"), b.join("s.json")]
        );
        w.skill_scope = true;
        assert_eq!(agent_files(&w), [a.join("a.json"), a.join("b.json")]);
    }
}
