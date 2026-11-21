//! Sandbox por caso: um HOME temporário por lado, com a árvore da fixture e o ambiente fechado.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO_REMOTE: &str = "https://github.com/acme/demo.git";

pub struct Side {
    pub home: PathBuf,
}

pub struct Sandbox {
    root: PathBuf,
    keep: bool,
    pub ts: Side,
    pub rust: Side,
}

impl Sandbox {
    /// Cria `T/ts/home` e `T/rust/home`, cada um uma cópia separada da fixture.
    pub fn create(fixture: Option<&Path>, cwd: Option<&str>, keep: bool) -> Result<Self, String> {
        let root = tempfile::Builder::new()
            .prefix("hades-parity-")
            .tempdir()
            .map_err(|e| e.to_string())?
            .keep();
        let side = |name: &str| -> Result<Side, String> {
            let home = root.join(name).join("home");
            std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
            if let Some(fx) = fixture {
                copy_tree(fx, &home)?;
            }
            if let Some(cwd) = cwd {
                let dir = home.join(cwd);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                if cwd == "repo" {
                    init_repo(&dir)?;
                }
            }
            Ok(Side { home })
        };
        Ok(Self {
            ts: side("ts")?,
            rust: side("rust")?,
            root,
            keep,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn init_repo(dir: &Path) -> Result<(), String> {
    for args in [
        vec!["init", "-q"],
        vec!["remote", "add", "origin", REPO_REMOTE],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(&args)
            .status()
            .map_err(|e| format!("git: {e}"))?;
        if !status.success() {
            return Err(format!("git {} falhou", args.join(" ")));
        }
    }
    Ok(())
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dest = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Ambiente fechado de um lado: nada herdado do processo (`HADES_*`, `HERDR_*`, `CLAUDE*`, `GH_*` não vazam, e o
/// `~/.claude/hades/instance` real fica fora porque `HOME` é o sandbox). O `env` do caso entra por último.
pub fn build_env(
    home: &Path,
    skill: &Path,
    stubs: &Path,
    bun_dir: &Path,
    case_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let h = home.display();
    let mut env = BTreeMap::new();
    for (k, v) in [
        ("HOME", h.to_string()),
        ("HADES_WORK_ROOT", format!("{h}/work")),
        ("HADES_PACKS_ROOT", format!("{h}/packs")),
        ("HADES_DATA", format!("{h}/data")),
        ("HADES_SKILL_DIR", skill.display().to_string()),
        ("TZ", "UTC".to_owned()),
        ("LANG", "C.UTF-8".to_owned()),
        ("NO_COLOR", "1".to_owned()),
        (
            "PATH",
            format!("{}:/usr/bin:/bin:{}", stubs.display(), bun_dir.display()),
        ),
    ] {
        env.insert(k.to_owned(), v);
    }
    env.extend(case_env.clone());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_fechado_com_caso_por_ultimo() {
        let case = BTreeMap::from([("HADES_JOURNAL".to_owned(), "1".to_owned())]);
        let env = build_env(
            Path::new("/h"),
            Path::new("/s"),
            Path::new("/s/stubs"),
            Path::new("/b"),
            &case,
        );
        assert_eq!(env["HOME"], "/h");
        assert_eq!(env["HADES_WORK_ROOT"], "/h/work");
        assert_eq!(env["PATH"], "/s/stubs:/usr/bin:/bin:/b");
        assert_eq!(env["HADES_JOURNAL"], "1");
        assert!(!env.contains_key("HERDR_ENV") && !env.contains_key("GH_TOKEN"));
    }

    #[test]
    fn lados_separados_e_limpeza() {
        let root = {
            let sb = Sandbox::create(None, Some("repo"), false).unwrap();
            assert_ne!(sb.ts.home, sb.rust.home);
            assert!(sb.ts.home.join("repo/.git").is_dir());
            sb.root().to_path_buf()
        };
        assert!(!root.exists());
    }
}
