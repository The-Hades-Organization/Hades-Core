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
                copy_tree_rendered(fx, &home, &home)?;
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

/// Nome de pasta do Claude Code para um caminho: todo caractere fora de `[A-Za-z0-9]` vira `-`
/// (`encodeCwd`, moirai.ts:57).
pub fn encode_path(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Aplica o template da fixture: `@HOME@` vira o HOME do lado; `@ENC:<caminho>@` vira o caminho codificado por
/// [`encode_path`] (o `<caminho>` pode conter `@HOME@`).
pub fn render_template(text: &str, home: &Path) -> String {
    static ENC: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"@ENC:([^@]*)@").unwrap());
    let text = text.replace("@HOME@", &home.display().to_string());
    ENC.replace_all(&text, |c: &regex::Captures| encode_path(&c[1]))
        .into_owned()
}

/// Copia a fixture aplicando o template ao nome de cada arquivo e pasta e ao conteúdo dos que são UTF-8;
/// binário passa intacto.
pub fn copy_tree_rendered(from: &Path, to: &Path, home: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = render_template(&entry.file_name().to_string_lossy(), home);
        let dest = to.join(name);
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_tree_rendered(&entry.path(), &dest, home)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let raw = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
            let out = match String::from_utf8(raw) {
                Ok(text) => render_template(&text, home).into_bytes(),
                Err(e) => e.into_bytes(),
            };
            std::fs::write(&dest, out).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
    }
    Ok(())
}

/// Roda um script de `setup` num lado: `sh <script>` com `cwd` = HOME do lado, `HOME`, `SIDE` (`ts`|`rust`),
/// `TZ=UTC` e git sem config global; falha de saída vira erro do caso.
pub fn run_setup(script: &Path, side: &Side, side_name: &str) -> Result<(), String> {
    let out = Command::new("sh")
        .arg(script)
        .current_dir(&side.home)
        .env_clear()
        .env("HOME", &side.home)
        .env("SIDE", side_name)
        .env("TZ", "UTC")
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|e| format!("setup {}: {e}", script.display()))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "setup {} ({side_name}) saiu {}: {}",
        script.display(),
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
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
    fn template_home_e_enc() {
        let home = Path::new("/tmp/x.y/home");
        assert_eq!(render_template("a @HOME@/b", home), "a /tmp/x.y/home/b");
        assert_eq!(
            render_template(".claude/projects/@ENC:@HOME@|wt|u-um@/t", home),
            ".claude/projects/-tmp-x-y-home-wt-u-um/t"
        );
        assert_eq!(render_template("sem marca", home), "sem marca");
    }

    #[test]
    fn copia_renderiza_nome_e_conteudo_e_poupa_binario() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("src"), dir.path().join("dst"));
        std::fs::create_dir_all(src.join("@ENC:@HOME@|wt@")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("@ENC:@HOME@|wt@/a.json"), "{\"w\":\"@HOME@\"}").unwrap();
        std::fs::write(src.join("bin"), [0xff, b'@', b'H', 0xfe]).unwrap();
        let home = dst.join("h");
        copy_tree_rendered(&src, &dst, &home).unwrap();
        let enc = encode_path(&format!("{}/wt", home.display()));
        let got = std::fs::read_to_string(dst.join(&enc).join("a.json")).unwrap();
        assert_eq!(got, format!("{{\"w\":\"{}\"}}", home.display()));
        assert_eq!(
            std::fs::read(dst.join("bin")).unwrap(),
            [0xff, b'@', b'H', 0xfe]
        );
    }

    #[test]
    fn setup_roda_por_lado_e_falha_vira_erro() {
        let dir = tempfile::tempdir().unwrap();
        let side = Side {
            home: dir.path().to_path_buf(),
        };
        let ok = dir.path().join("ok.sh");
        std::fs::write(&ok, "echo $SIDE > lado.txt; mkdir -p wt").unwrap();
        run_setup(&ok, &side, "rust").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lado.txt")).unwrap(),
            "rust\n"
        );
        let bad = dir.path().join("bad.sh");
        std::fs::write(&bad, "echo boom >&2; exit 3").unwrap();
        let err = run_setup(&bad, &side, "ts").unwrap_err();
        assert!(err.contains("saiu 3") && err.contains("boom"), "{err}");
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
