//! Projeto ativo: repo do diretório → `<raiz dos pacotes>/<projeto>/profile.json` (`src/lib/project.ts`).
//!
//! Leitura não cria pasta (divergência aceita do plano, sem efeito em saída).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::env::Env;
use crate::fsx::{is_file, list_dir, read_text};
use crate::output::exit;
use crate::paths::{packs_root, slug_of};
use crate::proc::{RunOptions, run};

/// `Resolution` (`src/lib/project.ts:10`). `code`: 0 com perfil, 3 repo sem perfil, 4 fora de repo com
/// `remote.origin.url`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub project: Option<String>,
    pub profile: Option<PathBuf>,
    pub repo: Option<String>,
    pub root: Option<String>,
    pub packs: PathBuf,
    pub code: i32,
    pub message: String,
}

impl Resolution {
    fn new(packs: PathBuf, repo: Option<String>) -> Self {
        Self {
            project: None,
            profile: None,
            repo,
            root: None,
            packs,
            code: exit::OK,
            message: String::new(),
        }
    }
}

/// `{repo, name}` de um `profile.json`: `identity.repo` e `name`, vazios se ausentes, ilegíveis ou de outro tipo
/// (`src/lib/project.ts:34`).
pub fn json_field(file: &Path) -> (String, String) {
    let Some(text) = read_text(file) else {
        return (String::new(), String::new());
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return (String::new(), String::new());
    };
    let text_at = |pointer: &str| {
        doc.pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    (text_at("/identity/repo"), text_at("/name"))
}

/// Caminho do `profile.json` cujo `identity.repo` casa com o slug, sem caixa (`src/lib/project.ts:46`).
pub fn find_profile(slug: &str, root: &Path) -> Option<PathBuf> {
    let wanted = slug.to_lowercase();
    if wanted.is_empty() {
        return None;
    }
    list_dir(root).into_iter().find_map(|name| {
        let file = root.join(name).join("profile.json");
        (is_file(&file) && json_field(&file).0.to_lowercase() == wanted).then_some(file)
    })
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let dir = dir.to_string_lossy();
    let mut argv = vec!["-C", dir.as_ref()];
    argv.extend_from_slice(args);
    run("git", &argv, &RunOptions::default())
        .stdout
        .trim()
        .to_owned()
}

fn no_profile_message(repo: &str, packs: &Path) -> String {
    format!(
        "hades: {repo} sem perfil em {}/ — rode /hades project init para criar o perfil do projeto (reference/projects.md)",
        packs.display()
    )
}

/// `resolveProject` (`src/lib/project.ts:72`).
pub fn resolve_project(env: &Env, dir: &Path) -> Resolution {
    let packs = packs_root(env);
    let mut result = Resolution::new(packs.clone(), None);
    let url = git_out(dir, &["config", "--get", "remote.origin.url"]);
    if url.is_empty() {
        result.code = exit::OUTSIDE_REPO;
        result.message = format!(
            "hades: {} não está num repositório git com remote.origin.url — o projeto ativo vem do repo do diretório atual",
            dir.display()
        );
        return result;
    }
    let top = git_out(dir, &["rev-parse", "--show-toplevel"]);
    result.root = (!top.is_empty()).then_some(top);
    let repo = slug_of(&url);
    result.repo = (!repo.is_empty()).then(|| repo.clone());
    let Some(profile) = find_profile(&repo, &packs) else {
        result.code = exit::NO_PROFILE;
        result.message = no_profile_message(&repo, &packs);
        return result;
    };
    let name = json_field(&profile).1;
    result.project = (!name.is_empty()).then_some(name);
    result.profile = Some(profile);
    result
}

/// Formato JSON dos resolvedores, compacto, nesta ordem de chaves (`src/lib/project.ts:93`).
pub fn resolution_json(r: &Resolution) -> Value {
    json!({
        "project": r.project,
        "profile": r.profile.as_ref().map(|p| p.to_string_lossy()),
        "repo": r.repo,
        "root": r.root,
        "packs": r.packs.to_string_lossy(),
    })
}

/// Pasta do pacote do perfil resolvido (`src/lib/project.ts:99`).
pub fn pack_of(r: &Resolution) -> Option<PathBuf> {
    r.profile.as_ref().map(|p| {
        p.parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    })
}

/// Resolve pelo slug `dono/nome`, sem olhar o git; `root` fica vazio (`src/lib/project.ts:104`).
pub fn resolve_slug(env: &Env, slug: &str) -> Resolution {
    let packs = packs_root(env);
    let mut result = Resolution::new(packs.clone(), (!slug.is_empty()).then(|| slug.to_owned()));
    let Some(profile) = find_profile(slug, &packs) else {
        result.code = exit::NO_PROFILE;
        result.message = no_profile_message(slug, &packs);
        return result;
    };
    let name = json_field(&profile).1;
    result.project = (!name.is_empty()).then_some(name);
    result.profile = Some(profile);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn env_in(home: &Path) -> Env {
        Env::empty()
            .with("HOME", home.to_string_lossy())
            .with("HADES_PACKS_ROOT", home.join("packs").to_string_lossy())
    }

    fn git_repo(dir: &Path, origin: Option<&str>) {
        fs::create_dir_all(dir).unwrap();
        let d = dir.to_string_lossy();
        run("git", &["-C", &d, "init", "-q"], &RunOptions::default());
        if let Some(url) = origin {
            run(
                "git",
                &["-C", &d, "remote", "add", "origin", url],
                &RunOptions::default(),
            );
        }
    }

    #[test]
    fn json_field_ignora_tipo_errado_e_json_invalido() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("p.json");
        write(&f, r#"{"name":"demo","identity":{"repo":"Acme/Demo"}}"#);
        assert_eq!(json_field(&f), ("Acme/Demo".into(), "demo".into()));
        write(&f, r#"{"name":3,"identity":{"repo":false}}"#);
        assert_eq!(json_field(&f), (String::new(), String::new()));
        write(&f, "[1]");
        assert_eq!(json_field(&f), (String::new(), String::new()));
        write(&f, "{nope");
        assert_eq!(json_field(&f), (String::new(), String::new()));
        assert_eq!(
            json_field(&t.path().join("nada")),
            (String::new(), String::new())
        );
    }

    #[test]
    fn find_profile_casa_sem_caixa_e_pega_o_primeiro_em_ordem() {
        let t = tempfile::tempdir().unwrap();
        let packs = t.path().join("packs");
        write(
            &packs.join("b/profile.json"),
            r#"{"identity":{"repo":"acme/demo"}}"#,
        );
        write(
            &packs.join("a/profile.json"),
            r#"{"identity":{"repo":"ACME/Demo"}}"#,
        );
        write(&packs.join("c/outro.json"), "{}");
        assert_eq!(
            find_profile("acme/DEMO", &packs),
            Some(packs.join("a/profile.json"))
        );
        assert_eq!(find_profile("acme/nada", &packs), None);
        assert_eq!(find_profile("", &packs), None);
        assert_eq!(find_profile("acme/demo", &t.path().join("sem")), None);
    }

    #[test]
    fn resolve_project_com_perfil_devolve_codigo_0() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        write(
            &t.path().join("packs/demo/profile.json"),
            r#"{"name":"demo","identity":{"repo":"acme/demo"}}"#,
        );
        let repo = t.path().join("wt");
        git_repo(&repo, Some("git@github.com:acme/demo.git"));
        let r = resolve_project(&env, &repo);
        assert_eq!(r.code, 0);
        assert_eq!(r.project.as_deref(), Some("demo"));
        assert_eq!(r.repo.as_deref(), Some("acme/demo"));
        assert_eq!(r.profile, Some(t.path().join("packs/demo/profile.json")));
        assert!(r.root.is_some());
        assert_eq!(pack_of(&r), Some(t.path().join("packs/demo")));
        assert_eq!(
            resolution_json(&r)
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["project", "profile", "repo", "root", "packs"]
        );
    }

    #[test]
    fn resolve_project_sem_perfil_devolve_3_e_mensagem() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        let repo = t.path().join("wt");
        git_repo(&repo, Some("https://github.com/other/plain.git"));
        let r = resolve_project(&env, &repo);
        assert_eq!(r.code, exit::NO_PROFILE);
        assert_eq!(r.repo.as_deref(), Some("other/plain"));
        assert_eq!(r.profile, None);
        assert_eq!(
            r.message,
            format!(
                "hades: other/plain sem perfil em {}/packs/ — rode /hades project init para criar o perfil do projeto (reference/projects.md)",
                t.path().display()
            )
        );
    }

    #[test]
    fn resolve_project_fora_de_repo_ou_sem_origin_devolve_4() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        let plain = t.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        let sem_origin = t.path().join("sem-origin");
        git_repo(&sem_origin, None);
        for dir in [&plain, &sem_origin] {
            let r = resolve_project(&env, dir);
            assert_eq!(r.code, exit::OUTSIDE_REPO);
            assert_eq!((r.repo, r.root, r.profile), (None, None, None));
            assert!(r.message.contains("não está num repositório git"));
        }
    }

    #[test]
    fn resolve_slug_nao_olha_o_git_e_deixa_root_vazio() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path());
        write(
            &t.path().join("packs/demo/profile.json"),
            r#"{"name":"demo","identity":{"repo":"acme/demo"}}"#,
        );
        let r = resolve_slug(&env, "Acme/Demo");
        assert_eq!((r.code, r.root), (0, None));
        assert_eq!(r.project.as_deref(), Some("demo"));
        let r = resolve_slug(&env, "acme/nada");
        assert_eq!(r.code, exit::NO_PROFILE);
        assert!(r.message.starts_with("hades: acme/nada sem perfil em "));
        assert_eq!(resolve_slug(&env, "").repo, None);
    }
}
