//! Leituras de arquivo que nunca falham para o chamador (`src/lib/fs.ts`).

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

/// `statSync(path).isFile()` (`src/lib/fs.ts:4`); segue links.
pub fn is_file(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

/// `statSync(path).isDirectory()` (`src/lib/fs.ts:13`); segue links.
pub fn is_dir(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

/// Texto do arquivo, ou `None` se não existe ou não abre (`src/lib/fs.ts:22`). Bytes inválidos viram U+FFFD, como o
/// `readFileSync(_, "utf8")`.
pub fn read_text(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Primeira linha, sem o `\n` final; vazio se não lê (`src/lib/fs.ts:31`).
pub fn first_line(path: &Path) -> String {
    match read_text(path) {
        None => String::new(),
        Some(text) => match text.find('\n') {
            Some(i) => text[..i].to_owned(),
            None => text,
        },
    }
}

/// Nomes das entradas em ordem de byte; vazio se a pasta não existe (`src/lib/fs.ts:40`). O TS ordena por unidade
/// UTF-16, que só difere da ordem de byte UTF-8 em nomes com caractere fora do plano básico.
pub fn list_dir(path: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    let mut names: Vec<OsString> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
    names
        .into_iter()
        .map(|n| String::from_utf8_lossy(&n.into_vec()).into_owned())
        .collect()
}

/// `path.resolve` do Node: absoluto (a partir do cwd) e normalizado de forma léxica, sem tocar o disco.
pub fn resolve(path: &Path) -> PathBuf {
    let base = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    };
    let mut out = base;
    for comp in path.components() {
        match comp {
            Component::RootDir => out = PathBuf::from("/"),
            Component::CurDir | Component::Prefix(_) => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
        }
    }
    out
}

/// `realpath -m`: resolve os links do trecho que existe e mantém o resto como veio (`src/lib/fs.ts:50`).
pub fn realpath_missing_ok(path: &Path) -> PathBuf {
    let mut current = resolve(path);
    let mut rest: Vec<OsString> = Vec::new();
    while !current.exists() {
        let Some(parent) = current.parent().map(Path::to_path_buf) else {
            break;
        };
        match current.file_name() {
            Some(name) => rest.push(name.to_owned()),
            None => break,
        }
        current = parent;
    }
    let mut real = fs::canonicalize(&current).unwrap_or(current);
    for name in rest.into_iter().rev() {
        real.push(name);
    }
    real
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn is_file_e_is_dir_distinguem_e_seguem_links() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a.txt");
        fs::write(&f, "x").unwrap();
        let l = t.path().join("link");
        symlink(&f, &l).unwrap();
        assert!(is_file(&f) && is_file(&l) && !is_dir(&f));
        assert!(is_dir(t.path()) && !is_file(t.path()));
        assert!(!is_file(&t.path().join("nada")) && !is_dir(&t.path().join("nada")));
    }

    #[test]
    fn read_text_ausente_e_none() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(read_text(&t.path().join("x")), None);
        fs::write(t.path().join("x"), "oi\n").unwrap();
        assert_eq!(read_text(&t.path().join("x")).as_deref(), Some("oi\n"));
    }

    #[test]
    fn first_line_como_head_1() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("f");
        assert_eq!(first_line(&p), "");
        fs::write(&p, "um\ndois\n").unwrap();
        assert_eq!(first_line(&p), "um");
        fs::write(&p, "sem quebra").unwrap();
        assert_eq!(first_line(&p), "sem quebra");
        fs::write(&p, "\nresto").unwrap();
        assert_eq!(first_line(&p), "");
        fs::write(&p, "a\r\nb").unwrap();
        assert_eq!(first_line(&p), "a\r");
    }

    #[test]
    fn list_dir_ordena_por_byte_e_pasta_ausente_e_vazia() {
        let t = tempfile::tempdir().unwrap();
        for n in ["b", "a", "B", "_z", "á"] {
            fs::write(t.path().join(n), "").unwrap();
        }
        assert_eq!(list_dir(t.path()), ["B", "_z", "a", "b", "á"]);
        assert!(list_dir(&t.path().join("nada")).is_empty());
    }

    #[test]
    fn resolve_normaliza_sem_tocar_o_disco() {
        assert_eq!(resolve(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(resolve(Path::new("/a//b/")), PathBuf::from("/a/b"));
        assert_eq!(resolve(Path::new("/..")), PathBuf::from("/"));
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(resolve(Path::new("x/../y")), cwd.join("y"));
    }

    #[test]
    fn realpath_missing_ok_resolve_link_e_preserva_resto() {
        let t = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(t.path()).unwrap();
        fs::create_dir(real.join("d")).unwrap();
        symlink(real.join("d"), real.join("ln")).unwrap();
        assert_eq!(
            realpath_missing_ok(&real.join("ln/novo/fundo")),
            real.join("d/novo/fundo")
        );
        assert_eq!(realpath_missing_ok(&real.join("d")), real.join("d"));
        assert_eq!(
            realpath_missing_ok(&real.join("d/../d/x")),
            real.join("d/x")
        );
    }
}
