//! Raízes e `$D` do Hades (`src/lib/paths.ts`, `kit.ts`, `instance.ts`, `work.ts`, `project.ts`, `profile.ts`).
//!
//! Tudo recebe o [`Env`], então os testes não tocam o `HOME` real. `load_work_env` fica para a PR do `work`.

use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::fsx::{first_line, is_file, list_dir, read_text, resolve};
use crate::proc::{RunOptions, run};

/// `$HOME/.claude/hades` (`src/lib/paths.ts:15`).
pub fn hades_home(env: &Env) -> PathBuf {
    env.home().join(".claude").join("hades")
}

/// `HADES_SKILL_DIR` > `~/.claude/skills/hades`. No TS é a pasta do código (`src/lib/paths.ts:4`); em Rust não há
/// pasta de código, então vale a instalação da skill (plano §2.1).
pub fn skill_root(env: &Env) -> PathBuf {
    match env.get("HADES_SKILL_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => env.home().join(".claude").join("skills").join("hades"),
    }
}

/// `env > <hh>/<arquivo> (1ª linha) > <hh>/<padrão>`. `trim` repete o `.trim()` do TS onde ele existe.
fn root_from(env: &Env, var: &str, file: &str, default: &str, trim: bool) -> PathBuf {
    if let Some(v) = env.get(var) {
        return PathBuf::from(v);
    }
    let hh = hades_home(env);
    let line = first_line(&hh.join(file));
    let line = if trim { line.trim().to_owned() } else { line };
    if line.is_empty() {
        hh.join(default)
    } else {
        PathBuf::from(line)
    }
}

/// `HADES_WORK_ROOT` > 1ª linha de `<hh>/work-root` > `<hh>/work`. Uma regra no lugar das três cópias do TS
/// (`src/lib/work.ts:43`, `journal.ts:80`, `cerberus.ts:146`); duas delas aparam a linha e a de `work.ts` não, e o
/// core apara.
pub fn work_root(env: &Env) -> PathBuf {
    root_from(env, "HADES_WORK_ROOT", "work-root", "work", true)
}

/// `HADES_PACKS_ROOT` > `<hh>/packs-root` > `<hh>/packs` (`src/lib/project.ts:22`, sem aparar a linha).
pub fn packs_root(env: &Env) -> PathBuf {
    root_from(env, "HADES_PACKS_ROOT", "packs-root", "packs", false)
}

/// `HADES_STACKS_ROOT` > `<hh>/stacks-root` > `<hh>/stacks` (`src/lib/profile.ts:72`).
pub fn stacks_root(env: &Env) -> PathBuf {
    root_from(env, "HADES_STACKS_ROOT", "stacks-root", "stacks", true)
}

/// `dono/nome` de uma URL de remote, https ou scp; vazio sem os dois campos (`src/lib/project.ts:30`).
pub fn slug_of(url: &str) -> String {
    let fields = remote_fields(url);
    if fields.len() < 2 {
        return String::new();
    }
    format!("{}/{}", fields[fields.len() - 2], fields[fields.len() - 1])
}

/// `owner__repo` de uma URL de remote, o slug do projeto no work root (`src/lib/work.ts:70`). Campo que falta vira
/// vazio, como o `?? ""` do TS.
pub fn slug_of_remote(url: &str) -> String {
    let fields = remote_fields(url);
    let at = |back: usize| fields.len().checked_sub(back).map_or("", |i| fields[i]);
    format!("{}__{}", at(2), at(1))
}

fn remote_fields(url: &str) -> Vec<&str> {
    url.strip_suffix(".git")
        .unwrap_or(url)
        .split(['/', ':'])
        .collect()
}

fn git_origin_url(dir: &Path) -> String {
    let args = [
        "-C",
        &dir.to_string_lossy(),
        "config",
        "--get",
        "remote.origin.url",
    ];
    run("git", &args, &RunOptions::default())
        .stdout
        .trim()
        .to_owned()
}

/// `identity.repo` do `profile.json`; vazio se ausente, ilegível ou de tipo inesperado (`src/lib/project.ts:34`).
fn profile_repo(file: &Path) -> String {
    let Some(text) = read_text(file) else {
        return String::new();
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return String::new();
    };
    doc.pointer("/identity/repo")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// Pasta do pacote cujo `profile.json` tem `identity.repo` igual (sem caixa) ao slug do `origin` do diretório. Só o
/// que `instance_for` precisa de `resolveProject`/`packOf` (`src/lib/project.ts:46,72,99`); a resolução completa é
/// da PR do `project`.
fn pack_for(env: &Env, dir: &Path) -> Option<PathBuf> {
    let url = git_origin_url(dir);
    if url.is_empty() {
        return None;
    }
    let wanted = slug_of(&url).to_lowercase();
    if wanted.is_empty() {
        return None;
    }
    let root = packs_root(env);
    list_dir(&root).into_iter().find_map(|name| {
        let file = root.join(&name).join("profile.json");
        (is_file(&file) && profile_repo(&file).to_lowercase() == wanted).then(|| root.join(name))
    })
}

fn is_valid_instance(dir: &str) -> bool {
    !dir.is_empty() && is_file(&Path::new(dir).join("env.sh"))
}

/// `$D` da instância do projeto do diretório (`<hh>/instances/<pacote>`, 1ª linha, válida se tem `env.sh`); sem
/// registro do projeto, a padrão `<hh>/instance`; `None` se nenhuma tem `env.sh` (`src/lib/instance.ts:26`).
pub fn instance_for(env: &Env, cwd: &Path) -> Option<PathBuf> {
    let hh = hades_home(env);
    if let Some(pack) = pack_for(env, cwd).and_then(|p| p.file_name().map(|n| n.to_owned())) {
        let registered = first_line(&hh.join("instances").join(pack));
        if is_valid_instance(&registered) {
            return Some(PathBuf::from(registered));
        }
    }
    let fallback = first_line(&hh.join("instance"));
    is_valid_instance(&fallback).then(|| PathBuf::from(fallback))
}

/// O `$D` (`src/lib/kit.ts:20`): `HADES_KIT_DIR` se difere de `<skill>/kit`; senão `HADES_DATA`; senão a instância
/// do `cwd`. `None` quando não há instalação.
///
/// Decisão do dono (2026-11-21): este é o único "qual instância vale" do core. O `configValue` do TS lê sempre
/// `<hh>/instance`; aqui ele passa por esta função. Diverge do TS só com várias instâncias na máquina.
pub fn find_state_dir(env: &Env, cwd: &Path) -> Option<PathBuf> {
    let skill_kit = skill_root(env).join("kit");
    if let Some(exported) = env.get("HADES_KIT_DIR")
        && resolve(Path::new(exported)) != resolve(&skill_kit)
    {
        return Some(PathBuf::from(exported));
    }
    if let Some(data) = env.get("HADES_DATA") {
        return Some(PathBuf::from(data));
    }
    instance_for(env, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn env_in(home: &Path) -> Env {
        Env::empty().with("HOME", home.to_string_lossy())
    }

    fn hh(home: &Path) -> PathBuf {
        home.join(".claude/hades")
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn git_repo(dir: &Path, origin: &str) {
        fs::create_dir_all(dir).unwrap();
        let d = dir.to_string_lossy();
        run("git", &["-C", &d, "init", "-q"], &RunOptions::default());
        run(
            "git",
            &["-C", &d, "remote", "add", "origin", origin],
            &RunOptions::default(),
        );
    }

    #[test]
    fn hades_home_e_skill_root_saem_do_home_injetado() {
        let e = env_in(Path::new("/h"));
        assert_eq!(hades_home(&e), PathBuf::from("/h/.claude/hades"));
        assert_eq!(skill_root(&e), PathBuf::from("/h/.claude/skills/hades"));
        assert_eq!(
            skill_root(&e.with("HADES_SKILL_DIR", "/s")),
            PathBuf::from("/s")
        );
    }

    #[test]
    fn home_vazio_cai_no_homedir() {
        let e = Env::empty().with("HOME", "");
        assert!(e.home().is_absolute());
    }

    #[test]
    fn work_root_ordem_env_arquivo_padrao() {
        let t = tempfile::tempdir().unwrap();
        let e = env_in(t.path());
        assert_eq!(work_root(&e), hh(t.path()).join("work"));
        write(
            &hh(t.path()).join("work-root"),
            "  /dados/work  \nignorada\n",
        );
        assert_eq!(work_root(&e), PathBuf::from("/dados/work"));
        assert_eq!(
            work_root(&e.with("HADES_WORK_ROOT", "/env")),
            PathBuf::from("/env")
        );
        write(&hh(t.path()).join("work-root"), "   \n");
        assert_eq!(work_root(&env_in(t.path())), hh(t.path()).join("work"));
    }

    #[test]
    fn packs_root_nao_apara_e_stacks_root_apara() {
        let t = tempfile::tempdir().unwrap();
        let e = env_in(t.path());
        assert_eq!(packs_root(&e), hh(t.path()).join("packs"));
        assert_eq!(stacks_root(&e), hh(t.path()).join("stacks"));
        write(&hh(t.path()).join("packs-root"), " /p \n");
        write(&hh(t.path()).join("stacks-root"), " /s \n");
        assert_eq!(packs_root(&e), PathBuf::from(" /p "));
        assert_eq!(stacks_root(&e), PathBuf::from("/s"));
        assert_eq!(
            packs_root(&e.clone().with("HADES_PACKS_ROOT", "/x")),
            PathBuf::from("/x")
        );
        assert_eq!(
            stacks_root(&e.with("HADES_STACKS_ROOT", "/y")),
            PathBuf::from("/y")
        );
    }

    #[test]
    fn slug_de_remote_https_e_scp() {
        assert_eq!(
            slug_of_remote("https://github.com/acme/demo.git"),
            "acme__demo"
        );
        assert_eq!(
            slug_of_remote("git@github.com:acme/demo"),
            "acme__demo"
        );
        assert_eq!(slug_of_remote("solto"), "__solto");
        assert_eq!(slug_of_remote(""), "__");
        assert_eq!(slug_of("git@github.com:Dono/Nome.git"), "Dono/Nome");
        assert_eq!(slug_of("solto"), "");
    }

    #[test]
    fn instance_for_registro_do_projeto_vence_a_padrao() {
        let t = tempfile::tempdir().unwrap();
        let (e, home) = (env_in(t.path()), t.path());
        let packs = home.join("packs");
        write(
            &packs.join("demo/profile.json"),
            r#"{"name":"demo","identity":{"repo":"acme/demo"}}"#,
        );
        write(
            &packs.join("outro/profile.json"),
            r#"{"identity":{"repo":"x/y"}}"#,
        );
        write(&packs.join("quebrado/profile.json"), "{não é json");
        let e = e.with("HADES_PACKS_ROOT", packs.to_string_lossy());
        let (d_proj, d_pad) = (home.join("d-proj"), home.join("d-pad"));
        write(&d_proj.join("env.sh"), "");
        write(&d_pad.join("env.sh"), "");
        write(
            &hh(home).join("instances/demo"),
            &format!("{}\n", d_proj.display()),
        );
        write(
            &hh(home).join("instance"),
            &format!("{}\n", d_pad.display()),
        );
        let repo = home.join("repo");
        git_repo(&repo, "https://github.com/acme/demo.git");
        assert_eq!(instance_for(&e, &repo), Some(d_proj));
        let alheio = home.join("alheio");
        git_repo(&alheio, "https://github.com/x/sem-perfil.git");
        assert_eq!(instance_for(&e, &alheio), Some(d_pad.clone()));
        assert_eq!(instance_for(&e, &home.join("fora-de-git")), Some(d_pad));
    }

    #[test]
    fn instance_for_registro_sem_env_sh_cai_na_padrao_e_none_sem_nenhuma() {
        let t = tempfile::tempdir().unwrap();
        let (e, home) = (env_in(t.path()), t.path());
        let packs = home.join("packs");
        write(
            &packs.join("p/profile.json"),
            r#"{"identity":{"repo":"o/r"}}"#,
        );
        let e = e.with("HADES_PACKS_ROOT", packs.to_string_lossy());
        let repo = home.join("repo");
        git_repo(&repo, "git@github.com:o/r.git");
        assert_eq!(instance_for(&e, &repo), None);
        let sem_env_sh = home.join("sem-env-sh");
        fs::create_dir_all(&sem_env_sh).unwrap();
        write(&hh(home).join("instances/p"), &sem_env_sh.to_string_lossy());
        write(&hh(home).join("instance"), &sem_env_sh.to_string_lossy());
        assert_eq!(instance_for(&e, &repo), None);
        let boa = home.join("boa");
        write(&boa.join("env.sh"), "");
        write(&hh(home).join("instance"), &boa.to_string_lossy());
        assert_eq!(instance_for(&e, &repo), Some(boa));
    }

    #[test]
    fn find_state_dir_ordem_kit_dir_data_instancia() {
        let t = tempfile::tempdir().unwrap();
        let (e, home) = (env_in(t.path()), t.path());
        let cwd = home.join("sem-git");
        fs::create_dir_all(&cwd).unwrap();
        assert_eq!(find_state_dir(&e, &cwd), None);

        let inst = home.join("inst");
        write(&inst.join("env.sh"), "");
        write(&hh(home).join("instance"), &inst.to_string_lossy());
        assert_eq!(find_state_dir(&e, &cwd), Some(inst.clone()));

        let e_data = e.clone().with("HADES_DATA", "/dados");
        assert_eq!(find_state_dir(&e_data, &cwd), Some(PathBuf::from("/dados")));

        let e_kit = e_data.clone().with("HADES_KIT_DIR", "/kit-exportado");
        assert_eq!(
            find_state_dir(&e_kit, &cwd),
            Some(PathBuf::from("/kit-exportado"))
        );
    }

    #[test]
    fn find_state_dir_ignora_kit_dir_igual_ao_kit_da_skill() {
        let t = tempfile::tempdir().unwrap();
        let skill_kit = skill_root(&env_in(t.path())).join("kit");
        let e = env_in(t.path()).with("HADES_DATA", "/dados");
        let com_kit = e
            .clone()
            .with("HADES_KIT_DIR", format!("{}/./x/..", skill_kit.display()));
        assert_eq!(
            find_state_dir(&com_kit, t.path()),
            Some(PathBuf::from("/dados"))
        );
        let vazio = e.with("HADES_KIT_DIR", "");
        assert_eq!(
            find_state_dir(&vazio, t.path()),
            Some(PathBuf::from("/dados"))
        );
    }

    #[test]
    fn instancia_unificada_no_cwd_diverge_do_ts_com_varias_instancias() {
        // Divergência aceita (decisão do dono, 2026-11-21): com duas instâncias, o `$D` segue o projeto do cwd, e não
        // a `<hh>/instance` que o `configValue` do TS lê sempre.
        let t = tempfile::tempdir().unwrap();
        let (e, home) = (env_in(t.path()), t.path());
        let packs = home.join("packs");
        write(
            &packs.join("proj-a/profile.json"),
            r#"{"identity":{"repo":"o/a"}}"#,
        );
        write(
            &packs.join("proj-b/profile.json"),
            r#"{"identity":{"repo":"o/b"}}"#,
        );
        let e = e.with("HADES_PACKS_ROOT", packs.to_string_lossy());
        let (da, db, dpad) = (home.join("da"), home.join("db"), home.join("dpad"));
        for d in [&da, &db, &dpad] {
            write(&d.join("env.sh"), "");
        }
        write(&hh(home).join("instances/proj-a"), &da.to_string_lossy());
        write(&hh(home).join("instances/proj-b"), &db.to_string_lossy());
        write(&hh(home).join("instance"), &dpad.to_string_lossy());
        let (ra, rb) = (home.join("ra"), home.join("rb"));
        git_repo(&ra, "https://github.com/o/a");
        git_repo(&rb, "https://github.com/o/b");
        assert_eq!(find_state_dir(&e, &ra), Some(da));
        assert_eq!(find_state_dir(&e, &rb), Some(db));
    }
}
