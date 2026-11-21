use clap::{Parser, Subcommand};
use hades_parity::{cases, runner};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "hades-parity", about = "Paridade TS × Rust do Hades")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Roda a matriz de casos nos dois lados.
    Run {
        #[arg(long)]
        cases: PathBuf,
        /// Checkout da skill Hades (TS).
        #[arg(long)]
        ts: PathBuf,
        /// Binário Rust da ferramenta.
        #[arg(long)]
        rust: PathBuf,
        /// Só os casos cujo id contém o texto.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value = "bun")]
        bun: PathBuf,
        /// Mantém os sandboxes para depurar.
        #[arg(long)]
        keep: bool,
    },
}

fn which(program: PathBuf) -> PathBuf {
    if program.components().count() > 1 {
        return program;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join(&program))
        .find(|p| p.is_file())
        .unwrap_or(program)
}

fn main() -> ExitCode {
    let Cmd::Run {
        cases: file,
        ts,
        rust,
        filter,
        bun,
        keep,
    } = Cli::parse().cmd;
    let parsed = match cases::load(&file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let abs = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
    let cfg = runner::Config {
        skill: abs(ts),
        rust_bin: abs(rust),
        bun: abs(which(bun)),
        cases_dir: abs(file.parent().map(PathBuf::from).unwrap_or_default()),
        keep,
    };
    if runner::run_all(&cfg, &parsed.case, filter.as_deref()) == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
