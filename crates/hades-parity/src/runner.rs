//! Execução de um caso nos dois lados e relatório.

use crate::cases::Case;
use crate::compare::{self, Outcome};
use crate::normalize::{Roots, normalize, normalize_dates};
use crate::sandbox::{Sandbox, Side, build_env, run_setup};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(120);

pub struct Config {
    pub skill: PathBuf,
    pub rust_bin: PathBuf,
    pub bun: PathBuf,
    pub cases_dir: PathBuf,
    pub keep: bool,
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Pass,
    Fail(Vec<String>),
    /// Caso `pending`: roda e reporta, não falha. `Some` = divergências; `None` = já bate.
    Pending {
        reason: String,
        diffs: Option<Vec<String>>,
    },
}

fn subst(arg: &str, cfg: &Config) -> String {
    arg.replace("{skill}", &cfg.skill.display().to_string())
        .replace("{cases_dir}", &cfg.cases_dir.display().to_string())
}

fn spawn_capture(
    program: &Path,
    args: &[String],
    cwd: &Path,
    env: &std::collections::BTreeMap<String, String>,
    stdin: &str,
) -> Result<(String, String, i32), String> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.as_bytes());
    }
    let mut out = child.stdout.take().ok_or("sem stdout")?;
    let mut err = child.stderr.take().ok_or("sem stderr")?;
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{}: estourou {}s",
                program.display(),
                TIMEOUT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let text = |h: std::thread::JoinHandle<Vec<u8>>| {
        String::from_utf8_lossy(&h.join().unwrap_or_default()).into_owned()
    };
    Ok((text(t_out), text(t_err), status.code().unwrap_or(-1)))
}

fn roots_of(home: &Path, skill: &Path) -> Roots {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    Roots {
        home: vec![
            home.display().to_string(),
            canon(home).display().to_string(),
        ],
        skill: vec![
            skill.display().to_string(),
            canon(skill).display().to_string(),
        ],
    }
}

fn run_side(cfg: &Config, case: &Case, side: &Side, is_ts: bool) -> Result<Outcome, String> {
    let stubs = cfg.skill.join("loop/test/stubs");
    let bun_dir = cfg.bun.parent().unwrap_or(Path::new("/"));
    let env = build_env(&side.home, &cfg.skill, &stubs, bun_dir, &case.env);
    let cwd = side.home.join(case.cwd.as_deref().unwrap_or(""));
    let argv: Vec<String> = case.argv.iter().map(|a| subst(a, cfg)).collect();
    let (program, args) = if is_ts {
        let entry = match &case.ts_cmd {
            Some(cmd) => cmd.iter().map(|a| subst(a, cfg)).collect(),
            None => vec![format!("{}/src/cli.ts", cfg.skill.display())],
        };
        (cfg.bun.clone(), [entry, argv].concat())
    } else {
        (cfg.rust_bin.clone(), argv)
    };
    let (stdout, stderr, exit) = spawn_capture(&program, &args, &cwd, &env, &case.stdin)?;
    let roots = roots_of(&side.home, &cfg.skill);
    let norm = |text: &str| {
        let text = normalize(text, &roots);
        if case.normalize_dates {
            normalize_dates(&text)
        } else {
            text
        }
    };
    let globs: Vec<String> = case
        .compare
        .iter()
        .filter_map(|c| c.strip_prefix("files:").map(str::to_owned))
        .collect();
    let mut files = std::collections::BTreeMap::new();
    for (rel, path) in compare::list_files(&side.home, &globs) {
        let raw = std::fs::read(&path).map_err(|e| e.to_string())?;
        files.insert(format!("~/{rel}"), norm(&String::from_utf8_lossy(&raw)));
    }
    Ok(Outcome {
        stdout: norm(&stdout),
        stderr: norm(&stderr),
        exit,
        files,
    })
}

pub fn run_case(cfg: &Config, case: &Case) -> Result<Verdict, String> {
    let fixture = case
        .fixture
        .as_ref()
        .map(|f| cfg.cases_dir.join("fixtures").join(f));
    let sandbox = Sandbox::create(fixture.as_deref(), case.cwd.as_deref(), cfg.keep)?;
    if cfg.keep {
        eprintln!("{}: sandbox em {}", case.id, sandbox.root().display());
    }
    for (side, name, own) in [
        (&sandbox.ts, "ts", &case.setup_ts),
        (&sandbox.rust, "rust", &case.setup_rust),
    ] {
        for script in [&case.setup, own].into_iter().flatten() {
            run_setup(&cfg.cases_dir.join(script), side, name)?;
        }
    }
    let ts = run_side(cfg, case, &sandbox.ts, true)?;
    let rust = run_side(cfg, case, &sandbox.rust, false)?;
    let diffs = compare::compare(&case.compare, &ts, &rust);
    Ok(match (&case.pending, diffs.is_empty()) {
        (None, true) => Verdict::Pass,
        (None, false) => Verdict::Fail(diffs),
        (Some(reason), true) => Verdict::Pending {
            reason: reason.clone(),
            diffs: None,
        },
        (Some(reason), false) => Verdict::Pending {
            reason: reason.clone(),
            diffs: Some(diffs),
        },
    })
}

/// Executa os casos (filtrados por substring do id) e imprime o relatório; devolve o número de falhas.
pub fn run_all(cfg: &Config, cases: &[Case], filter: Option<&str>) -> usize {
    let (mut pass, mut fail, mut pending) = (0, 0, 0);
    for case in cases
        .iter()
        .filter(|c| filter.is_none_or(|f| c.id.contains(f)))
    {
        match run_case(cfg, case) {
            Ok(Verdict::Pass) => {
                pass += 1;
                println!("PASS    {}", case.id);
            }
            Ok(Verdict::Fail(diffs)) => {
                fail += 1;
                println!("FAIL    {}", case.id);
                diffs.iter().for_each(|d| println!("  {d}"));
            }
            Ok(Verdict::Pending { reason, diffs }) => {
                pending += 1;
                let note = if diffs.is_some() {
                    "diverge"
                } else {
                    "já bate; remova o pending"
                };
                println!("PENDING {} — {reason} ({note})", case.id);
            }
            Err(e) if case.pending.is_some() => {
                pending += 1;
                println!(
                    "PENDING {} — {} (erro: {e})",
                    case.id,
                    case.pending.as_deref().unwrap_or("")
                );
            }
            Err(e) => {
                fail += 1;
                println!("FAIL    {}\n  erro: {e}", case.id);
            }
        }
    }
    println!("{pass} passaram, {fail} falharam, {pending} pendentes");
    fail
}
