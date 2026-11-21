//! Binário de teste do Hades-Core: expõe funções do core por subcomando, para o runner de paridade comparar com o
//! `parity/ts-probe.ts`. Não é distribuído.

use std::process::ExitCode;

const USAGE: &str = "uso: hades-core-probe <pyjson <json> | slug <url>>";

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
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
