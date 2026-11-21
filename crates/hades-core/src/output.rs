//! Códigos de saída e erro da CLI (`src/lib/output.ts`).

use std::fmt;
use std::io::{self, ErrorKind, Write};

/// `EXIT` (`src/lib/output.ts:2`): os códigos dos scripts em bash que a CLI substitui.
pub mod exit {
    pub const OK: i32 = 0;
    pub const FAIL: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const NO_PROFILE: i32 = 3;
    pub const OUTSIDE_REPO: i32 = 4;
    pub const MISSING_DEPENDENCY: i32 = 69;
    /// Leitor fechou o pipe (`| head`): `128 + SIGPIPE` (`src/lib/output.ts:34`).
    pub const SIGPIPE: i32 = 141;
}

/// `HadesError` (`src/lib/output.ts:15`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HadesError {
    pub message: String,
    /// Código de saída do processo.
    pub code: i32,
    /// Código estável do catálogo de erros para humano; sem ele vale o da classe do código de saída.
    pub error_code: Option<String>,
}

impl HadesError {
    /// Padrão do TS: `code = EXIT.usage`.
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_code(message, exit::USAGE)
    }

    pub fn with_code(message: impl Into<String>, code: i32) -> Self {
        Self {
            message: message.into(),
            code,
            error_code: None,
        }
    }

    pub fn error_code(mut self, error_code: impl Into<String>) -> Self {
        self.error_code = Some(error_code.into());
        self
    }
}

impl fmt::Display for HadesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HadesError {}

/// `die` (`src/lib/output.ts:28`): erro pronto para propagar com `?`.
pub fn die<T>(
    message: impl Into<String>,
    code: i32,
    error_code: Option<&str>,
) -> Result<T, HadesError> {
    let mut e = HadesError::with_code(message, code);
    e.error_code = error_code.map(str::to_owned);
    Err(e)
}

/// Pipe quebrado sai calado com 141; qualquer outra falha de escrita sai com 1 (`src/lib/output.ts:36-41`).
pub fn exit_code_for_write_error(error: &io::Error) -> i32 {
    if error.kind() == ErrorKind::BrokenPipe {
        exit::SIGPIPE
    } else {
        exit::FAIL
    }
}

fn write_line(mut w: impl Write, text: &str) -> io::Result<()> {
    w.write_all(text.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

fn finish(result: io::Result<()>) {
    if let Err(e) = result {
        std::process::exit(exit_code_for_write_error(&e));
    }
}

/// `out` (`src/lib/output.ts:46`): linha no stdout. Pipe fechado encerra o processo com 141, sem mensagem.
pub fn out(text: &str) {
    finish(write_line(io::stdout().lock(), text));
}

/// `warn` (`src/lib/output.ts:50`): linha no stderr, com a mesma regra do SIGPIPE.
pub fn warn(text: &str) {
    finish(write_line(io::stderr().lock(), text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigos_iguais_aos_do_ts() {
        assert_eq!(
            [
                exit::OK,
                exit::FAIL,
                exit::USAGE,
                exit::NO_PROFILE,
                exit::OUTSIDE_REPO,
                exit::MISSING_DEPENDENCY,
                exit::SIGPIPE
            ],
            [0, 1, 2, 3, 4, 69, 141]
        );
    }

    #[test]
    fn hades_error_padrao_e_uso() {
        let e = HadesError::new("falhou");
        assert_eq!(
            (e.code, e.error_code.as_deref(), e.to_string().as_str()),
            (2, None, "falhou")
        );
        let e = HadesError::with_code("x", exit::FAIL).error_code("E1");
        assert_eq!((e.code, e.error_code.as_deref()), (1, Some("E1")));
    }

    #[test]
    fn die_devolve_err_com_codigo() {
        let r: Result<(), _> = die("sem instalação", exit::FAIL, Some("NO_INSTALL"));
        let e = r.unwrap_err();
        assert_eq!((e.code, e.error_code.as_deref()), (1, Some("NO_INSTALL")));
    }

    #[test]
    fn pipe_quebrado_vira_141_e_outro_erro_1() {
        assert_eq!(
            exit_code_for_write_error(&io::Error::from(ErrorKind::BrokenPipe)),
            141
        );
        assert_eq!(
            exit_code_for_write_error(&io::Error::from(ErrorKind::PermissionDenied)),
            1
        );
    }

    struct Quebrado;
    impl Write for Quebrado {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn escrita_em_pipe_quebrado_propaga_broken_pipe() {
        let e = write_line(Quebrado, "x").unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BrokenPipe);
    }

    #[test]
    fn write_line_acrescenta_quebra() {
        let mut buf = Vec::new();
        write_line(&mut buf, "oi").unwrap();
        assert_eq!(buf, b"oi\n");
    }
}
