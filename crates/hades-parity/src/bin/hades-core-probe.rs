//! Binário de teste do Hades-Core: expõe funções do core por subcomando, para o runner de paridade comparar com o
//! `parity/ts-probe.ts`. Não é distribuído.

use hades_core::env::Env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn json_object(text: &str) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    serde_json::from_str(text).map_err(|e| format!("json inválido: {e}"))
}

fn with_work(f: impl FnOnce(&hades_core::work::WorkEnv) -> Result<(), String>) -> ExitCode {
    let options = hades_core::work::WorkOptions {
        cwd: std::env::current_dir().ok(),
        ..Default::default()
    };
    let work = match hades_core::work::load_work_env(&options, &Env::from_process()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{}", e.message);
            return ExitCode::from(e.code as u8);
        }
    };
    if let Err(e) = hades_core::work::ensure_dirs(&work) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }
    match f(&work) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "uso: hades-core-probe <pyjson <json> | slug <url> | coreok <faixa> <versao> | getfield <json> <caminho> | mask <texto> | jline <kind> <resumo> | config-value <chave> <padrao> | work-root | state-dir | config-effective <dir> | config-save <dir> <json> <json> | reg-set <nome> <json> | reg-fresh <nome> <phase|resume> <rotulo> | delivery-set <chave> <json> | work-env | project | tofixed <numero> <digitos> | clock | render-cli <msg> <codigo|-> [prefixo] | message-of-failure <stderr> | suggest <digitado> <nome>... | cli-code-of-exit <n>>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["pyjson", text] => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v) => {
                println!("{}", hades_core::jsonfmt::py_dumps(&v, None));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("json inválido: {e}");
                ExitCode::from(2)
            }
        },
        ["tofixed", number, digits] => match (number.parse::<f64>(), digits.parse::<usize>()) {
            (Ok(x), Ok(d)) if d <= 100 => {
                println!("{}", hades_core::jsfmt::to_fixed(x, d));
                ExitCode::SUCCESS
            }
            _ => {
                eprintln!("número ou casas inválidos");
                ExitCode::from(2)
            }
        },
        ["slug", url] => {
            println!("{}", hades_core::paths::slug_of_remote(url));
            ExitCode::SUCCESS
        }
        ["coreok", range, version] => {
            let range = serde_json::Value::String((*range).to_owned());
            match hades_core::profile::core_ok(Some(&range), version) {
                Ok(ok) => {
                    println!("{ok}");
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    println!("{reason}");
                    ExitCode::from(1)
                }
            }
        }
        ["getfield", text, field] => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(doc) => match hades_core::profile::get_field(&doc, field, None) {
                Some(out) => {
                    println!("{out}");
                    ExitCode::SUCCESS
                }
                None => ExitCode::from(1),
            },
            Err(e) => {
                eprintln!("json inválido: {e}");
                ExitCode::from(2)
            }
        },
        ["mask", text] => {
            println!("{}", hades_core::journal::mask_secrets(text));
            ExitCode::SUCCESS
        }
        ["jline", kind, summary] => {
            use hades_core::journal::{Source, SourceFields, event_of_source};
            let fields = SourceFields {
                agent: Some("a1".into()),
                cwd: Some("/w".into()),
                ..Default::default()
            };
            let event = event_of_source(Source::Gate, kind, summary, fields, 1_773_144_000_000);
            println!("{}", event.to_json());
            ExitCode::SUCCESS
        }
        ["config-value", key, fallback] => {
            let cwd = std::env::current_dir().unwrap_or_default();
            println!(
                "{}",
                hades_core::config::config_value(&Env::from_process(), &cwd, key, fallback)
            );
            ExitCode::SUCCESS
        }
        ["render-cli", msg, code, rest @ ..] => {
            let code = (*code != "-").then_some(*code);
            let prefix = rest.first().map_or("hades", |p| *p);
            println!("{}", hades_core::errors::render_cli(msg, code, prefix));
            ExitCode::SUCCESS
        }
        ["message-of-failure", stderr] => {
            println!("{}", hades_core::errors::message_of_cli_failure(stderr));
            ExitCode::SUCCESS
        }
        ["suggest", typed, names @ ..] => {
            let names: Vec<&str> = names.to_vec();
            println!(
                "{}",
                hades_core::errors::suggest_name(typed, &names).unwrap_or("")
            );
            ExitCode::SUCCESS
        }
        ["cli-code-of-exit", n] => {
            let n: i32 = n.parse().unwrap_or(1);
            println!("{}", hades_core::errors::cli_code_of_exit(n));
            ExitCode::SUCCESS
        }
        ["work-root"] => {
            println!(
                "{}",
                hades_core::paths::work_root(&Env::from_process()).display()
            );
            ExitCode::SUCCESS
        }
        ["state-dir"] => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let found = hades_core::paths::find_state_dir(&Env::from_process(), &cwd);
            println!(
                "{}",
                found.map(|p| p.display().to_string()).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        ["config-effective", dir] => {
            let eff = hades_core::config::effective_config(Path::new(dir), &Env::from_process());
            for (k, v) in eff.iter() {
                println!("{k}={v}");
            }
            ExitCode::SUCCESS
        }
        ["config-save", dir, updates, unset] => {
            let (Some(updates), Some(unset)) = (
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(updates).ok(),
                serde_json::from_str::<Vec<String>>(unset).ok(),
            ) else {
                eprintln!("json inválido");
                return ExitCode::from(2);
            };
            let pairs: Vec<(String, String)> = updates
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect();
            let pairs: Vec<(&str, &str)> = pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let unset: Vec<&str> = unset.iter().map(String::as_str).collect();
            let store = hades_core::config::ConfigStore {
                dir: PathBuf::from(dir),
                kit: PathBuf::from(dir),
            };
            match hades_core::config::save(&store, &pairs, &unset) {
                Ok(applied) => {
                    println!("{}", applied.join(","));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.0);
                    ExitCode::from(1)
                }
            }
        }
        ["reg-set", name, patch] => with_work(|work| {
            let patch = json_object(patch)?;
            hades_core::registry::reg_set(work, name, patch).map_err(|e| e.to_string())
        }),
        ["reg-fresh", name, sub, label] => with_work(|work| {
            use hades_core::registry::FreshSession;
            let sub = if *sub == "phase" {
                FreshSession::Phase
            } else {
                FreshSession::Resume
            };
            hades_core::registry::reg_update(work, name, |r| {
                hades_core::registry::after_fresh_session(r, sub, label, "2026-01-01T00:00:00Z")
            })
            .map_err(|e| e.to_string())
        }),
        ["delivery-set", key, patch] => with_work(|work| {
            let patch = json_object(patch)?;
            hades_core::registry::delivery_set(work, key, |mut d| {
                for (k, v) in patch {
                    d.insert(k, v);
                }
                d
            })
            .map_err(|e| e.to_string())
        }),
        ["work-env"] => {
            let options = hades_core::work::WorkOptions {
                cwd: std::env::current_dir().ok(),
                ..Default::default()
            };
            match hades_core::work::load_work_env(&options, &Env::from_process()) {
                Ok(w) => {
                    let p = |x: &Path| x.display().to_string();
                    println!(
                        "{}",
                        serde_json::json!({
                            "skillScope": w.skill_scope, "repoTop": p(&w.repo_top), "mainClone": p(&w.main_clone),
                            "slug": w.slug, "repoName": w.repo_name, "work": p(&w.work), "locks": p(&w.locks),
                            "state": p(&w.state), "orch": p(&w.orch), "skillOrch": p(&w.skill_orch),
                            "reports": w.reports,
                        })
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.message);
                    ExitCode::from(e.code as u8)
                }
            }
        }
        ["clock"] => match hades_core::clock::now_ms(&Env::from_process()) {
            Ok(ms) => {
                println!("{ms}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        ["project"] => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let r = hades_core::project::resolve_project(&Env::from_process(), &cwd);
            println!("{}", hades_core::project::resolution_json(&r));
            if !r.message.is_empty() {
                eprintln!("{}", r.message);
            }
            ExitCode::from(r.code as u8)
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
