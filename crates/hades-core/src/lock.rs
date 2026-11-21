//! `flock(2)` do sistema, o mesmo lock que o Python (`fcntl.flock`) e o TS tomam (`src/lib/flock.ts`). Nunca `fcntl`
//! POSIX lock, que não enxerga o `flock`.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};

/// Intervalo entre tentativas de `with_flock_timeout` (`RETRY_MS`, `src/lib/flock.ts:63`).
const RETRY: Duration = Duration::from_millis(25);

/// O prazo de `with_flock_timeout` estourou com o lock ainda de outro processo (`FlockTimeoutError`,
/// `src/lib/flock.ts:55`).
#[derive(Clone, Debug, PartialEq)]
pub struct FlockTimeoutError {
    pub path: PathBuf,
    pub wait_seconds: f64,
}

impl fmt::Display for FlockTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "flock de {} nao veio em {} s",
            self.path.display(),
            self.wait_seconds
        )
    }
}

impl std::error::Error for FlockTimeoutError {}

#[derive(Debug)]
pub enum LockError {
    Io(io::Error),
    Timeout(FlockTimeoutError),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Io(e) => e.fmt(f),
            LockError::Timeout(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LockError {}

impl From<io::Error> for LockError {
    fn from(e: io::Error) -> Self {
        LockError::Io(e)
    }
}

/// Lock exclusivo tomado; solta (e fecha o descritor) ao sair de escopo.
#[derive(Debug)]
pub struct FlockGuard {
    file: File,
}

impl Drop for FlockGuard {
    fn drop(&mut self) {
        let _ = flock(&self.file, FlockOperation::Unlock);
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().append(true).create(true).open(path)
}

fn flock_failed(path: &Path, e: rustix::io::Errno) -> io::Error {
    io::Error::other(format!("flock falhou em {}: {e}", path.display()))
}

/// Toma o lock exclusivo, esperando (`flock` bloqueante), sobre o arquivo criado se não existe.
pub fn lock(path: &Path) -> io::Result<FlockGuard> {
    let file = open_append(path)?;
    flock(&file, FlockOperation::LockExclusive).map_err(|e| flock_failed(path, e))?;
    Ok(FlockGuard { file })
}

/// Como `lock`, sem esperar: `None` se outro processo (ou outro descritor) tem o lock.
pub fn try_lock(path: &Path) -> io::Result<Option<FlockGuard>> {
    let file = open_append(path)?;
    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(FlockGuard { file })),
        Err(e) if e == rustix::io::Errno::WOULDBLOCK => Ok(None),
        Err(e) => Err(flock_failed(path, e)),
    }
}

/// `withFlock` (`src/lib/flock.ts:32`): roda `f` com o lock exclusivo do arquivo (criado se não existe); solta o lock e
/// fecha o descritor ao fim, inclusive se `f` entrar em pânico.
pub fn with_flock<T>(path: &Path, f: impl FnOnce() -> T) -> io::Result<T> {
    let _guard = lock(path)?;
    Ok(f())
}

/// `tryFlockForProcess` (`src/lib/flock.ts:46`), a base do `flock -n`: toma o lock exclusivo sem esperar e o mantém até
/// o processo sair; `false` se outro o tem. Como o TS, abre com truncamento (`"w"`), inclusive quando não consegue o
/// lock.
pub fn try_flock_for_process(path: &Path) -> io::Result<bool> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => {
            std::mem::forget(file);
            Ok(true)
        }
        Err(e) if e == rustix::io::Errno::WOULDBLOCK => Ok(false),
        Err(e) => Err(flock_failed(path, e)),
    }
}

/// `withFlockAsync` (`src/lib/flock.ts:100`) sem a fila do laço de eventos, que em Rust não existe: tenta com
/// `LOCK_NB` a cada 25 ms. `wait_seconds` `None` espera sem prazo; com prazo, estourar devolve `FlockTimeoutError`.
pub fn with_flock_timeout<T>(
    path: &Path,
    wait_seconds: Option<f64>,
    f: impl FnOnce() -> T,
) -> Result<T, LockError> {
    let file = open_append(path)?;
    let deadline = wait_seconds.map(|s| Instant::now() + Duration::from_secs_f64(s.max(0.0)));
    loop {
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => break,
            Err(e) if e == rustix::io::Errno::WOULDBLOCK => {}
            Err(e) => return Err(flock_failed(path, e).into()),
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let wait_seconds = wait_seconds.unwrap_or(0.0);
            return Err(LockError::Timeout(FlockTimeoutError {
                path: path.to_path_buf(),
                wait_seconds,
            }));
        }
        thread::sleep(RETRY);
    }
    let _guard = FlockGuard { file };
    Ok(f())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.lock");
        (t, p)
    }

    #[test]
    fn with_flock_cria_o_arquivo_e_devolve_o_valor() {
        let (_t, p) = tmp();
        assert_eq!(with_flock(&p, || 7).unwrap(), 7);
        assert!(p.exists());
    }

    #[test]
    fn with_flock_em_pasta_ausente_falha() {
        let (t, _) = tmp();
        assert!(with_flock(&t.path().join("x/y.lock"), || ()).is_err());
    }

    #[test]
    fn lock_segurado_impede_try_lock_e_soltar_libera() {
        let (_t, p) = tmp();
        let g = lock(&p).unwrap();
        assert!(try_lock(&p).unwrap().is_none());
        drop(g);
        assert!(try_lock(&p).unwrap().is_some());
    }

    #[test]
    fn panico_dentro_da_secao_solta_o_lock() {
        let (_t, p) = tmp();
        let r = catch_unwind(AssertUnwindSafe(|| with_flock(&p, || panic!("boom"))));
        assert!(r.is_err());
        assert!(try_lock(&p).unwrap().is_some());
    }

    #[test]
    fn timeout_com_lock_de_outro_descritor_estoura_com_a_mensagem_do_ts() {
        let (_t, p) = tmp();
        let _g = lock(&p).unwrap();
        let t0 = Instant::now();
        let e = with_flock_timeout(&p, Some(0.2), || ()).unwrap_err();
        assert!(t0.elapsed() >= Duration::from_millis(150));
        let LockError::Timeout(e) = e else {
            panic!("esperava Timeout")
        };
        assert_eq!(
            e.to_string(),
            format!("flock de {} nao veio em 0.2 s", p.display())
        );
        let e = with_flock_timeout(&p, Some(1.0), || ()).unwrap_err();
        assert_eq!(
            e.to_string(),
            format!("flock de {} nao veio em 1 s", p.display())
        );
    }

    #[test]
    fn timeout_entra_quando_o_lock_solta_dentro_do_prazo() {
        let (_t, p) = tmp();
        let g = lock(&p).unwrap();
        let h = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            drop(g);
        });
        assert_eq!(
            with_flock_timeout(&p, Some(5.0), || "entrei").unwrap(),
            "entrei"
        );
        h.join().unwrap();
    }

    #[test]
    fn timeout_solta_o_lock_ao_fim() {
        let (_t, p) = tmp();
        with_flock_timeout(&p, Some(1.0), || ()).unwrap();
        assert!(try_lock(&p).unwrap().is_some());
    }

    #[test]
    fn try_flock_for_process_segura_ate_o_fim_do_processo_e_trunca() {
        let (_t, p) = tmp();
        std::fs::write(&p, "velho").unwrap();
        assert!(try_flock_for_process(&p).unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
        assert!(!try_flock_for_process(&p).unwrap());
        assert!(try_lock(&p).unwrap().is_none());
    }
}
