//! Plugins, só leitura (`src/lib/plugins.ts`, `src/lib/install-copy.ts`): as constantes que o Cerberus e o Thanatos
//! consultam, os arquivos de estado no `$D` e a pasta `env/` do pacote. Instalar, atualizar e prévia ficam no Hades.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::split_lines;
use crate::env::Env;
use crate::fsx::{is_dir, read_text, realpath_missing_ok, resolve};

/// O que executa: o `install.sh` roda o `post-install.sh`, o painel carrega o `api.ts` e os scripts da UI, o doctor
/// lê o `doctor.sh` e o onboarding mede os checks (`src/lib/plugins.ts:40`).
pub const RUNS: [&str; 6] = [
    "post-install.sh",
    "panel/api.ts",
    "panel/ui/*.js",
    "doctor.sh",
    "checks/*.sh",
    "commands/*",
];

/// Entradas do `env/` que não viram script do ambiente (`src/lib/plugins.ts:41`).
pub const ENV_SKIP: [&str; 3] = ["assets", "reference", "__pycache__"];

/// Arquivos de plugin que a cópia da instalação separa do núcleo (`src/lib/install-copy.ts:17`).
pub const PLUGIN_FILES: [&str; 5] = [
    "config.tsv",
    "onboard.tsv",
    "doctor.sh",
    "plugin.json",
    "panel",
];

/// `NAME` (`src/lib/plugins.ts:34`): `^[a-z0-9][a-z0-9._-]{0,63}$`, o `name` do `profile.schema.json`.
pub fn is_valid_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    let tail_ok = |b: &u8| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-');
    (1..=64).contains(&bytes.len())
        && matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9')
        && bytes[1..].iter().all(tail_ok)
}

/// `PluginsCtx` (`src/lib/plugins.ts:56`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginsCtx {
    /// O `$D` desta instalação (`HADES_DATA`).
    pub data: PathBuf,
    /// O worktree (`HADES_WT`); vazio fora de uma instalação.
    pub worktree: PathBuf,
    /// A skill que o `install.sh` e o Cerberus usam (`HADES_SKILL_DIR`).
    pub skill: PathBuf,
}

/// `ctxFromEnv` (`src/lib/plugins.ts:63`): `data` cai no `kit` sem `HADES_DATA`; `skill` cai em
/// `~/.claude/skills/hades`.
pub fn ctx_from_env(env: &Env, kit: &Path) -> PluginsCtx {
    PluginsCtx {
        data: env
            .get("HADES_DATA")
            .map_or_else(|| kit.to_path_buf(), PathBuf::from),
        worktree: env.get("HADES_WT").map(PathBuf::from).unwrap_or_default(),
        skill: env.get("HADES_SKILL_DIR").map_or_else(
            || env.home().join(".claude").join("skills").join("hades"),
            PathBuf::from,
        ),
    }
}

/// `<$D>/plugins.disabled` (`src/lib/plugins.ts:72`).
pub fn disabled_file(c: &PluginsCtx) -> PathBuf {
    c.data.join("plugins.disabled")
}

/// `<$D>/plugins-staging`, fora da pasta de pacotes para o resolvedor nunca achar um clone em prévia
/// (`src/lib/plugins.ts:73`).
pub fn staging_dir(c: &PluginsCtx) -> PathBuf {
    c.data.join("plugins-staging")
}

/// `<$D>/plugins.lock` (`src/lib/plugins.ts:74`).
pub fn lock_file(c: &PluginsCtx) -> PathBuf {
    c.data.join("plugins.lock")
}

/// `<$D>/logs/plugins.log` (`src/lib/plugins.ts:75`).
pub fn audit_file(c: &PluginsCtx) -> PathBuf {
    c.data.join("logs").join("plugins.log")
}

/// `disabled` (`src/lib/plugins.ts:105`): nomes válidos de `plugins.disabled`, um por linha, aparados; sem arquivo,
/// vazio.
pub fn disabled(c: &PluginsCtx) -> BTreeSet<String> {
    read_text(&disabled_file(c))
        .map(|text| {
            split_lines(&text)
                .iter()
                .map(|line| line.trim().to_owned())
                .filter(|line| is_valid_name(line))
                .collect()
        })
        .unwrap_or_default()
}

/// `envDir` (`src/lib/plugins.ts:155`): a pasta `profile.env.pack` do pacote, se existe e está dentro dele (links
/// resolvidos); `None` caso contrário.
pub fn env_dir(pack: &Path, profile: &Value) -> Option<PathBuf> {
    let rel = profile.get("env")?.as_object()?.get("pack")?.as_str()?;
    if rel.is_empty() {
        return None;
    }
    let dir = realpath_missing_ok(&resolve(&pack.join(rel)));
    let root = realpath_missing_ok(pack);
    ((dir == root || dir.starts_with(&root)) && is_dir(&dir)).then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn nome_valido_segue_a_regex_do_schema() {
        let longo = "a".repeat(64);
        for ok in ["a", "0x", "demo-plugin_1.2", longo.as_str()] {
            assert!(is_valid_name(ok), "{ok}");
        }
        let longo = "a".repeat(65);
        for ruim in [
            "", "-a", ".a", "_a", "A", "aB", "a b", "a/b", "a\n", "ação", &longo,
        ] {
            assert!(!is_valid_name(ruim), "{ruim:?}");
        }
    }

    #[test]
    fn ctx_e_arquivos_de_estado_no_d() {
        let env = Env::empty().with("HOME", "/h");
        let c = ctx_from_env(&env, Path::new("/kit"));
        assert_eq!(
            c,
            PluginsCtx {
                data: "/kit".into(),
                worktree: PathBuf::new(),
                skill: "/h/.claude/skills/hades".into()
            }
        );
        let env = env
            .with("HADES_DATA", "/d")
            .with("HADES_WT", "/w")
            .with("HADES_SKILL_DIR", "/s");
        let c = ctx_from_env(&env, Path::new("/kit"));
        assert_eq!(
            (c.data.as_path(), c.worktree.as_path()),
            ("/d".as_ref(), "/w".as_ref())
        );
        assert_eq!(c.skill, PathBuf::from("/s"));
        assert_eq!(disabled_file(&c), PathBuf::from("/d/plugins.disabled"));
        assert_eq!(staging_dir(&c), PathBuf::from("/d/plugins-staging"));
        assert_eq!(lock_file(&c), PathBuf::from("/d/plugins.lock"));
        assert_eq!(audit_file(&c), PathBuf::from("/d/logs/plugins.log"));
    }

    #[test]
    fn disabled_filtra_nomes_invalidos_e_apara() {
        let t = tempfile::tempdir().unwrap();
        let c = PluginsCtx {
            data: t.path().to_path_buf(),
            worktree: PathBuf::new(),
            skill: PathBuf::new(),
        };
        assert!(disabled(&c).is_empty());
        fs::write(
            disabled_file(&c),
            "  b-plug \r\nA-ruim\n\n../x\na.plug\nb-plug\n",
        )
        .unwrap();
        assert_eq!(
            disabled(&c).into_iter().collect::<Vec<_>>(),
            ["a.plug", "b-plug"]
        );
    }

    #[test]
    fn env_dir_fica_dentro_do_pacote() {
        let t = tempfile::tempdir().unwrap();
        let pack = t.path().join("pack");
        fs::create_dir_all(pack.join("env/sub")).unwrap();
        fs::create_dir_all(t.path().join("fora")).unwrap();
        fs::write(pack.join("arquivo"), "x").unwrap();
        symlink(t.path().join("fora"), pack.join("escape")).unwrap();
        let real = realpath_missing_ok(&pack);
        let with = |rel: Value| json!({"env": {"pack": rel}});

        assert_eq!(env_dir(&pack, &with(json!("env"))), Some(real.join("env")));
        assert_eq!(
            env_dir(&pack, &with(json!("env/sub/.."))),
            Some(real.join("env"))
        );
        assert_eq!(env_dir(&pack, &with(json!("."))), Some(real.clone()));
        for ruim in [
            json!("../fora"),
            json!("escape"),
            json!("arquivo"),
            json!("nada"),
            json!(""),
            json!(3),
        ] {
            assert_eq!(env_dir(&pack, &with(ruim.clone())), None, "{ruim}");
        }
        assert_eq!(env_dir(&pack, &json!({})), None);
        assert_eq!(env_dir(&pack, &json!({"env": "env"})), None);
    }
}
