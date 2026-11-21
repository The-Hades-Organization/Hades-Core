//! Binário de teste do Hades-Core: expõe funções do core por subcomando, para o runner de paridade comparar com o
//! `parity/ts-probe.ts`. Não é distribuído.

use std::process::ExitCode;

const USAGE: &str = "uso: hades-core-probe <pyjson <json> | slug <url> | coreok <faixa> <versao> | getfield <json> <caminho> | mask <texto> | jline <kind> <resumo>>";

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
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
