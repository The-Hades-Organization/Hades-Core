//! Subprocesso com tempo limite e busca no PATH (`src/lib/proc.ts:1-40`).

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// `RunResult` (`src/lib/proc.ts:4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Opções de `run` (`src/lib/proc.ts:11`).
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub cwd: Option<PathBuf>,
    pub timeout: Option<Duration>,
    /// Teto de bytes por saída; ao estourar o processo é morto e o status vira 1, como o `maxBuffer` do Node.
    pub max_buffer: Option<usize>,
    /// Variáveis extras sobre o ambiente herdado.
    pub env: Vec<(String, String)>,
    /// Bytes escritos no stdin do filho, que é fechado depois; `None` mantém o stdin nulo.
    pub stdin: Option<Vec<u8>>,
}

const POLL: Duration = Duration::from_millis(5);

fn drain(
    mut pipe: impl Read + Send + 'static,
    max: Option<usize>,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(m) = max.filter(|m| buf.len() > *m) {
                        buf.truncate(m);
                        exceeded.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
        buf
    })
}

/// `run` (`src/lib/proc.ts:11`): stdin fechado (ou alimentado por `options.stdin`), stdout e stderr capturados. Falha ao iniciar, morte por sinal e
/// estouro de `max_buffer` dão status 1 (o `r.status ?? 1` do TS); estouro de `timeout` dá `timed_out`.
pub fn run(command: &str, args: &[&str], options: &RunOptions) -> RunResult {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(if options.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(cwd) = &options.cwd {
        cmd.current_dir(cwd);
    }
    cmd.envs(options.env.iter().map(|(k, v)| (k, v)));
    let failed = RunResult {
        status: 1,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    };
    let Ok(mut child) = cmd.spawn() else {
        return failed;
    };
    if let (Some(data), Some(mut pipe)) = (options.stdin.clone(), child.stdin.take()) {
        thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let exceeded = Arc::new(AtomicBool::new(false));
    let out = drain(
        child.stdout.take().expect("stdout piped"),
        options.max_buffer,
        exceeded.clone(),
    );
    let err = drain(
        child.stderr.take().expect("stderr piped"),
        options.max_buffer,
        exceeded.clone(),
    );

    let started = Instant::now();
    let mut timed_out = false;
    let mut killed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code(),
            Ok(None) => {}
            Err(_) => break None,
        }
        if options.timeout.is_some_and(|t| started.elapsed() >= t) {
            timed_out = true;
            killed = true;
        } else if exceeded.load(Ordering::SeqCst) {
            killed = true;
        }
        if killed {
            kill_group(&child);
            let _ = child.wait();
            break None;
        }
        thread::sleep(POLL);
    };
    let status = if exceeded.load(Ordering::SeqCst) {
        None
    } else {
        status
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    RunResult {
        status: status.unwrap_or(1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        timed_out,
    }
}

/// Mata o grupo inteiro (`sh -c` e netos): o neto que herdou o pipe não segura a leitura depois do kill.
fn kill_group(child: &std::process::Child) {
    use rustix::process::{Pid, Signal, kill_process_group};
    if let Some(pgid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pgid, Signal::KILL);
    }
}

/// `which` (`src/lib/proc.ts:22`): o primeiro executável do `path_var` que casa, como `command -v`. Entradas vazias
/// são puladas. Executável aqui é arquivo regular com algum bit de execução.
pub fn which(command: &str, path_var: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .filter(|d| !d.is_empty())
        .find_map(|dir| {
            let candidate = Path::new(dir).join(command);
            let meta = std::fs::metadata(&candidate).ok()?;
            (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(candidate)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sh(script: &str, options: &RunOptions) -> RunResult {
        run("sh", &["-c", script], options)
    }

    #[test]
    fn captura_stdout_stderr_e_status() {
        let r = sh("echo oi; echo erro >&2; exit 3", &RunOptions::default());
        assert_eq!(
            r,
            RunResult {
                status: 3,
                stdout: "oi\n".into(),
                stderr: "erro\n".into(),
                timed_out: false
            }
        );
    }

    #[test]
    fn stdin_vem_fechado() {
        let r = sh("cat; echo fim", &RunOptions::default());
        assert_eq!((r.status, r.stdout.as_str()), (0, "fim\n"));
    }

    #[test]
    fn env_extra_visivel_no_filho() {
        let r = sh(
            "echo $HADES_X-$HOME",
            &RunOptions {
                env: vec![("HADES_X".into(), "v1".into())],
                ..Default::default()
            },
        );
        let home = std::env::var("HOME").unwrap_or_default();
        assert_eq!(r.stdout, format!("v1-{home}\n"));
    }

    #[test]
    fn stdin_lido_pelo_filho() {
        let r = run(
            "cat",
            &[],
            &RunOptions {
                stdin: Some(b"ola".to_vec()),
                ..Default::default()
            },
        );
        assert_eq!((r.status, r.stdout.as_str()), (0, "ola"));
    }

    #[test]
    fn stdin_grande_com_saida_grande_nao_trava() {
        let data = vec![b'a'; 3 * 1024 * 1024];
        let r = run(
            "cat",
            &[],
            &RunOptions {
                stdin: Some(data),
                timeout: Some(Duration::from_secs(20)),
                ..Default::default()
            },
        );
        assert!(!r.timed_out);
        assert_eq!(r.stdout.len(), 3 * 1024 * 1024);
    }

    #[test]
    fn timeout_com_stdin_aberto_sem_ler() {
        let t = Instant::now();
        let r = sh(
            "sleep 5",
            &RunOptions {
                stdin: Some(vec![b'a'; 2 * 1024 * 1024]),
                timeout: Some(Duration::from_millis(200)),
                ..Default::default()
            },
        );
        assert!(r.timed_out);
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn cwd_e_respeitado() {
        let t = tempfile::tempdir().unwrap();
        let r = sh(
            "pwd -P",
            &RunOptions {
                cwd: Some(t.path().into()),
                ..Default::default()
            },
        );
        assert_eq!(
            r.stdout.trim(),
            fs::canonicalize(t.path()).unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn comando_inexistente_da_status_1_sem_timeout() {
        let r = run("hades-nao-existe-xyz", &[], &RunOptions::default());
        assert_eq!(
            r,
            RunResult {
                status: 1,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: false
            }
        );
    }

    #[test]
    fn timeout_mata_e_marca_timed_out() {
        let t = Instant::now();
        let r = sh(
            "sleep 5",
            &RunOptions {
                timeout: Some(Duration::from_millis(100)),
                ..Default::default()
            },
        );
        assert!(r.timed_out);
        assert_eq!(r.status, 1);
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn dentro_do_timeout_nao_marca() {
        let r = sh(
            "echo ok",
            &RunOptions {
                timeout: Some(Duration::from_secs(5)),
                ..Default::default()
            },
        );
        assert!(!r.timed_out);
        assert_eq!(r.stdout, "ok\n");
    }

    #[test]
    fn max_buffer_estourado_mata_e_da_status_1() {
        let r = sh(
            "yes",
            &RunOptions {
                max_buffer: Some(1000),
                timeout: Some(Duration::from_secs(5)),
                ..Default::default()
            },
        );
        assert_eq!(r.status, 1);
        assert!(!r.timed_out);
        assert_eq!(r.stdout.len(), 1000);
    }

    #[test]
    fn saida_grande_sem_teto_nao_trava() {
        let r = sh(
            "head -c 300000 /dev/zero | tr '\\0' a",
            &RunOptions::default(),
        );
        assert_eq!(r.stdout.len(), 300_000);
    }

    #[test]
    fn which_acha_o_primeiro_executavel_do_path() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        for d in [&a, &b] {
            let f = d.join("tool");
            fs::write(&f, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = format!(":{}::{}", a.display(), b.display());
        assert_eq!(which("tool", &path), Some(a.join("tool")));
    }

    #[test]
    fn which_ignora_sem_exec_diretorio_e_ausente() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("plano"), "").unwrap();
        fs::set_permissions(t.path().join("plano"), fs::Permissions::from_mode(0o644)).unwrap();
        fs::create_dir(t.path().join("dir")).unwrap();
        fs::set_permissions(t.path().join("dir"), fs::Permissions::from_mode(0o755)).unwrap();
        let path = t.path().display().to_string();
        assert_eq!(which("plano", &path), None);
        assert_eq!(which("dir", &path), None);
        assert_eq!(which("nada", &path), None);
        assert_eq!(which("sh", ""), None);
    }
}
