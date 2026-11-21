//! Escrita atômica: arquivo temporário único na mesma pasta e `rename`. Dois escritores nunca dividem o tmp, então um
//! `rename` não leva o conteúdo do outro (o `<f>.tmp` fixo de `writeJsonAtomic`, `src/lib/work.ts:126`, é o defeito
//! que a convivência TS e Rust corrige; plano §3).

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::jsonfmt::json_indent;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// `<f>.<pid>.<rand>.tmp`, ao lado do destino. O sufixo mistura nanossegundos, um contador do processo e o endereço de
/// uma pilha, e a criação com `create_new` repete se ainda assim colidir.
fn tmp_path(path: &Path) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0);
    let marker = &n as *const u64 as u64;
    let rand =
        (nanos.rotate_left(17) ^ marker.rotate_left(33) ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            & 0xFFFF_FFFF_FFFF;
    let mut name: OsString = path.file_name().map(OsString::from).unwrap_or_default();
    name.push(format!(".{}.{rand:x}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// Grava `content` em `path` de uma vez: tmp único, `fsync`, `rename`. Em qualquer falha o tmp é apagado e o destino
/// fica como estava.
pub fn write_file_atomic(path: &Path, content: &[u8]) -> io::Result<()> {
    let (tmp, mut file) = loop {
        let tmp = tmp_path(path);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = file
        .write_all(content)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// JSON no formato do TS: indentação 2 e `\n` final (`src/lib/work.ts:127`), gravado de forma atômica.
pub fn write_json_atomic(path: &Path, value: &Value) -> io::Result<()> {
    let mut text = json_indent(value, 2);
    text.push('\n');
    write_file_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(dir: &Path) -> Vec<String> {
        crate::fsx::list_dir(dir)
    }

    #[test]
    fn grava_novo_e_substitui_sem_deixar_tmp() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.json");
        write_file_atomic(&p, b"um").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"um");
        write_file_atomic(&p, b"dois").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"dois");
        assert_eq!(names(t.path()), ["a.json"]);
    }

    #[test]
    fn tmp_tem_o_formato_pid_rand_e_difere_a_cada_chamada() {
        let p = Path::new("/x/agente.json");
        let (a, b) = (tmp_path(p), tmp_path(p));
        assert_ne!(a, b);
        let pid = std::process::id();
        let re = |s: &str| {
            let s = s
                .strip_prefix(&format!("agente.json.{pid}."))
                .and_then(|r| r.strip_suffix(".tmp"))
                .unwrap_or("");
            !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
        };
        assert!(re(a.file_name().unwrap().to_str().unwrap()));
        assert_eq!(a.parent(), Some(Path::new("/x")));
    }

    #[test]
    fn falha_no_rename_apaga_o_tmp_e_preserva_o_destino() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("alvo");
        fs::create_dir(&p).unwrap();
        fs::write(p.join("dentro"), "x").unwrap();
        assert!(write_file_atomic(&p, b"conteudo").is_err());
        assert_eq!(names(t.path()), ["alvo"]);
        assert!(p.join("dentro").exists());
    }

    #[test]
    fn pasta_ausente_falha_sem_criar_nada() {
        let t = tempfile::tempdir().unwrap();
        assert!(write_file_atomic(&t.path().join("x/y.json"), b"1").is_err());
        assert!(names(t.path()).is_empty());
    }

    #[test]
    fn write_json_atomic_indenta_com_2_e_termina_em_quebra() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.json");
        write_json_atomic(&p, &json!({"b": 1, "a": [1, 2], "c": {}})).unwrap();
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ],\n  \"c\": {}\n}\n"
        );
    }
}
