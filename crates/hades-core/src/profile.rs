//! Perfil do projeto e stacks: campo do perfil, referências, stacks instaladas, termos da guarda de nome e faixa
//! `core` (`src/lib/profile.ts`). O perfil é um `serde_json::Value`: ele é extensível por plugin.
//!
//! Fora desta PR: `detect_stacks` (glob de `detect.files`), que fica para quem precisar dele.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::env::Env;
use crate::fsx::{first_line, is_file, list_dir, read_text, resolve};
use crate::jsonfmt::json_compact;
use crate::output::exit;
use crate::paths::{packs_root, skill_root, stacks_root};
use crate::project::{Resolution, resolve_project};

/// `readJson` (`src/lib/profile.ts:17`): `None` se não lê ou não é JSON.
pub fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_str(&read_text(file)?).ok()
}

/// `readJsonObject` (`src/lib/profile.ts:27`): objeto do arquivo, `None` se ilegível ou de outro tipo.
pub fn read_json_object(file: &Path) -> Option<Map<String, Value>> {
    match read_json(file)? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// `loadProfile` (`src/lib/profile.ts:32`): objeto vazio sem perfil ou se ele não lê.
pub fn load_profile(resolution: &Resolution) -> Value {
    let map = resolution
        .profile
        .as_deref()
        .and_then(read_json_object)
        .unwrap_or_default();
    Value::Object(map)
}

/// `pathValue` (`src/lib/profile.ts:37`): valor do caminho `a.b.c`; `None` se o caminho não existe. Diferença do TS:
/// o `key in current` do JS também enxerga `toString` e afins do protótipo; aqui só chave própria.
pub fn path_value<'a>(doc: &'a Value, field: &str) -> Option<&'a Value> {
    field
        .split('.')
        .try_fold(doc, |current, key| current.as_object()?.get(key))
}

/// `jsonText` (`src/lib/profile.ts:46`): string crua, o resto em JSON compacto.
pub fn json_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => json_compact(other),
    }
}

/// `getField` (`src/lib/profile.ts:54`): como o jq, `null`, `false` e `""` valem como ausente. Devolve o texto a
/// imprimir, ou `None` quando a CLI sai 1.
pub fn get_field(profile: &Value, field: &str, fallback: Option<&str>) -> Option<String> {
    let present = path_value(profile, field)
        .filter(|v| !matches!(v, Value::Null | Value::Bool(false)) && v.as_str() != Some(""));
    match present {
        Some(value) => Some(json_text(value)),
        None => fallback.map(str::to_owned),
    }
}

/// `refPath` (`src/lib/profile.ts:61`): caminho de `refs.<nome>` relativo ao pacote, ou `None`.
pub fn ref_path(resolution: &Resolution, name: &str) -> Option<String> {
    let profile = resolution.profile.as_deref()?;
    let doc = read_json_object(profile)?;
    let rel = doc.get("refs")?.as_object()?.get(name)?.as_str()?;
    if rel.is_empty() {
        return None;
    }
    let pack = profile.parent().unwrap_or(Path::new("."));
    Some(format!("{}/{rel}", pack.display()))
}

// ------------------------------------------------------------------ stacks

/// Stacks instaladas, na ordem em que o TS as insere; a do usuário vence a da skill (`src/lib/profile.ts:79`).
pub fn installed_stacks(env: &Env) -> Vec<(String, Value)> {
    let mut stacks: Vec<(String, Value)> = Vec::new();
    for root in [stacks_root(env), skill_root(env).join("stacks")] {
        for name in list_dir(&root) {
            let Some(doc) = read_json_object(&root.join(&name).join("stack.json")) else {
                continue;
            };
            if !stacks.iter().any(|(n, _)| *n == name) {
                stacks.push((name, Value::Object(doc)));
            }
        }
    }
    stacks
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// `activeStacks` (`src/lib/profile.ts:97`): stacks do perfil do diretório; sem perfil, ou sem a lista, todas.
pub fn active_stacks(env: &Env, dir: &Path) -> Vec<(String, Value)> {
    let all = installed_stacks(env);
    let resolution = resolve_project(env, dir);
    let wanted = if resolution.code == exit::OK {
        string_list(load_profile(&resolution).get("stacks"))
    } else {
        Vec::new()
    };
    if wanted.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .collect()
}

/// `StepLine` (`src/lib/profile.ts:109`).
#[derive(Clone, Debug, PartialEq)]
pub struct StepLine {
    pub kind: Value,
    pub cost_gb: Value,
    pub parallel: Value,
    pub measured_at: Value,
}

/// `stepOf` (`src/lib/profile.ts:117`): etapa `<stack>:<etapa>`; `None` se não existe.
pub fn step_of(env: &Env, reference: &str) -> Option<StepLine> {
    let (name, step_name) = match reference.split_once(':') {
        Some((name, step)) => (name, step),
        None => (reference, ""),
    };
    let stacks = installed_stacks(env);
    let doc = &stacks.iter().find(|(n, _)| n == name)?.1;
    let step = path_value(doc, &format!("steps.{step_name}"))?.as_object()?;
    let field = |key: &str, default: Value| step.get(key).cloned().unwrap_or(default);
    Some(StepLine {
        kind: field("kind", Value::Null),
        cost_gb: field("costGb", Value::Null),
        parallel: field("parallel", Value::from(0)),
        measured_at: field("measuredAt", Value::String(String::new())),
    })
}

/// `repoRoot` (`src/lib/profile.ts:125`): `git rev-parse --show-toplevel`, ou o próprio diretório.
pub fn repo_root(dir: &Path) -> PathBuf {
    let d = dir.to_string_lossy();
    let top = crate::proc::run(
        "git",
        &["-C", d.as_ref(), "rev-parse", "--show-toplevel"],
        &crate::proc::RunOptions::default(),
    );
    let text = top.stdout.trim();
    if top.status == 0 && !text.is_empty() {
        PathBuf::from(text)
    } else {
        dir.to_path_buf()
    }
}

// ------------------------------------------------------------------ guarda de nome

fn profiles_in(root: &Path) -> Vec<(PathBuf, Map<String, Value>)> {
    list_dir(root)
        .into_iter()
        .filter_map(|name| {
            let file = root.join(name).join("profile.json");
            let doc = if is_file(&file) {
                read_json_object(&file)
            } else {
                None
            }?;
            Some((file, doc))
        })
        .collect()
}

fn by_lower_then_raw(a: &String, b: &String) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn identity_of(doc: &Map<String, Value>) -> Map<String, Value> {
    doc.get("identity")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn str_or_empty(map: &Map<String, Value>, key: &str) -> String {
    map.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// `nameTerms` (`src/lib/profile.ts:158`): por perfil, o nome e os termos que a guarda de nome procura no núcleo.
/// Comprimento em unidades UTF-16, como o `.length` do JS.
pub fn name_terms(env: &Env) -> Vec<(String, Vec<String>)> {
    profiles_in(&packs_root(env))
        .into_iter()
        .map(|(_, doc)| {
            let identity = identity_of(&doc);
            let repo = str_or_empty(&identity, "repo");
            let name = str_or_empty(&doc, "name");
            let mut raw = vec![name.clone(), repo.clone()];
            raw.extend(repo.split('/').map(str::to_owned));
            raw.extend(string_list(identity.get("aliases")));
            let mut seen = BTreeSet::new();
            let mut terms: Vec<String> = raw
                .into_iter()
                .filter(|t| t.encode_utf16().count() >= 3 && seen.insert(t.clone()))
                .collect();
            terms.sort_by(by_lower_then_raw);
            (name, terms)
        })
        .collect()
}

/// `vocabulary` (`src/lib/profile.ts:171`): vocabulário (`identity.vocabulary`) de todos os perfis, sem repetição,
/// em ordem de ponto de código.
pub fn vocabulary(env: &Env) -> Vec<String> {
    let mut out = BTreeSet::new();
    for (_, doc) in profiles_in(&packs_root(env)) {
        if let Some(identity) = doc.get("identity").and_then(Value::as_object) {
            out.extend(string_list(identity.get("vocabulary")));
        }
    }
    out.into_iter().collect()
}

/// Termos de domínio e exceções `@allow <glob>` (`src/lib/profile.ts:183`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainTerms {
    pub terms: Vec<String>,
    pub allow: Vec<String>,
}

/// `domainTerms` (`src/lib/profile.ts:183`): lê o arquivo `domainTerms` de cada pacote; pacote cujo arquivo não lê
/// fica de fora.
pub fn domain_terms(env: &Env) -> DomainTerms {
    let mut terms = BTreeSet::new();
    let mut allow = BTreeSet::new();
    for (file, doc) in profiles_in(&packs_root(env)) {
        let rel = str_or_empty(&doc, "domainTerms");
        let text = if rel.is_empty() {
            String::new()
        } else {
            let pack = file.parent().unwrap_or(Path::new("."));
            match read_text(&resolve(&pack.join(&rel))) {
                Some(text) => text,
                None => continue,
            }
        };
        for line in text.split('\n').map(str::trim) {
            if let Some(glob) = line.strip_prefix("@allow ") {
                allow.insert(glob.trim().to_owned());
            } else if !line.is_empty() && !line.starts_with('#') {
                terms.insert(line.to_owned());
            }
        }
    }
    DomainTerms {
        terms: terms.into_iter().collect(),
        allow: allow.into_iter().collect(),
    }
}

// ------------------------------------------------------------------ faixa do núcleo (profile.core)

/// Operador de um termo da faixa `core`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreOp {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
}

/// `[maior, menor, patch]`. `f64` como o `Number` do JS: parte não numérica vira `NaN`.
pub type Version = [f64; 3];

/// `Number(texto)` do JS para as partes de uma versão: vazio é 0, ilegível é `NaN`.
fn js_number(text: &str) -> f64 {
    let t = text.trim();
    if t.is_empty() {
        0.0
    } else {
        t.parse().unwrap_or(f64::NAN)
    }
}

/// `toVersion` (`src/lib/profile.ts:207`).
pub fn to_version(text: &str) -> Version {
    let mut parts = text.split('.').map(js_number);
    let mut next = || parts.next().unwrap_or(0.0);
    [next(), next(), next()]
}

/// `compare` (`src/lib/profile.ts:212`).
fn compare(a: &Version, b: &Version) -> i32 {
    for i in 0..3 {
        #[allow(clippy::float_cmp)]
        if a[i] != b[i] {
            return if a[i] < b[i] { -1 } else { 1 };
        }
    }
    0
}

/// `\d+(?:\.\d+){0,2}`.
fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    (1..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `repr()` do Python para valores JSON: aspas simples, `True`/`False`/`None` (`src/lib/profile.ts:314`).
pub fn py_repr(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_owned(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.to_owned(),
        Some(v @ Value::Number(_)) => json_compact(v),
        Some(Value::String(s)) => {
            let quote = if s.contains('\'') && !s.contains('"') {
                '"'
            } else {
                '\''
            };
            let mut body = String::new();
            for c in s.chars() {
                match c {
                    '\\' => body.push_str("\\\\"),
                    '\n' => body.push_str("\\n"),
                    '\r' => body.push_str("\\r"),
                    '\t' => body.push_str("\\t"),
                    c if c == quote => {
                        body.push('\\');
                        body.push(c);
                    }
                    c => body.push(c),
                }
            }
            format!("{quote}{body}{quote}")
        }
        Some(Value::Array(items)) => {
            let inner: Vec<String> = items.iter().map(|v| py_repr(Some(v))).collect();
            format!("[{}]", inner.join(", "))
        }
        Some(Value::Object(map)) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}: {}",
                        py_repr(Some(&Value::String(k.clone()))),
                        py_repr(Some(v))
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// `str()` do Python: texto cru para string, `repr` para o resto (`src/lib/profile.ts:337`).
pub fn py_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        other => py_repr(other),
    }
}

/// `coreRange` (`src/lib/profile.ts:224`): faixa simples (`>=4.3`, `>=4.3 <5`, `4.3.x`) em termos. O erro é a mensagem
/// do validador.
pub fn core_range(text: Option<&Value>) -> Result<Vec<(CoreOp, Version)>, String> {
    let terms: Vec<&str> = text
        .and_then(Value::as_str)
        .map(|t| {
            t.split(|c: char| c.is_whitespace() || c == '\u{feff}')
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if terms.is_empty() {
        return Err("faixa vazia".to_owned());
    }
    let mut out = Vec::new();
    for term in terms {
        let repr = || py_repr(Some(&Value::String(term.to_owned())));
        if let Some(wild) = term.strip_suffix(".x").filter(|w| is_version(w)) {
            let depth = wild.split('.').count();
            if depth == 3 {
                return Err(format!(
                    "termo {}: o .x vai no lugar de menor ou patch (4.x, 4.3.x)",
                    repr()
                ));
            }
            let base = to_version(wild);
            let top = if depth == 1 {
                [base[0] + 1.0, 0.0, 0.0]
            } else {
                [base[0], base[1] + 1.0, 0.0]
            };
            out.push((CoreOp::Ge, base));
            out.push((CoreOp::Lt, top));
            continue;
        }
        let (op, rest) = [
            (">=", CoreOp::Ge),
            ("<=", CoreOp::Le),
            (">", CoreOp::Gt),
            ("<", CoreOp::Lt),
            ("=", CoreOp::Eq),
        ]
        .iter()
        .find_map(|(prefix, op)| term.strip_prefix(prefix).map(|rest| (*op, rest)))
        .unwrap_or((CoreOp::Eq, term));
        if !is_version(rest) {
            return Err(format!(
                "termo {} fora da sintaxe (>=4.3, <5, 4.3.x, 4.3.0)",
                repr()
            ));
        }
        out.push((op, to_version(rest)));
    }
    Ok(out)
}

/// `coreOk` (`src/lib/profile.ts:249`): a versão cabe na faixa? Erro igual ao de [`core_range`].
pub fn core_ok(range: Option<&Value>, version: &str) -> Result<bool, String> {
    let v = to_version(version);
    Ok(core_range(range)?.iter().all(|(op, reference)| {
        let c = compare(&v, reference);
        match op {
            CoreOp::Ge => c >= 0,
            CoreOp::Le => c <= 0,
            CoreOp::Gt => c > 0,
            CoreOp::Lt => c < 0,
            CoreOp::Eq => c == 0,
        }
    }))
}

const PLUGIN_CONTRACT_MIN: &str = "5.0.0";
const LAST_LEGACY_CORE: &str = "4.999.999";

/// `admitsLegacyCore` (`src/lib/profile.ts:268`): a faixa aceita núcleo anterior ao contrato atual de plugin?
pub fn admits_legacy_core(range: Option<&Value>) -> Result<bool, String> {
    core_ok(range, LAST_LEGACY_CORE)
}

/// `legacyCoreMessage` (`src/lib/profile.ts:272`).
pub fn legacy_core_message(project: &str, core: &str) -> String {
    format!(
        "o plugin {project} declara core {core}, que aceita núcleo anterior à {PLUGIN_CONTRACT_MIN}: o contrato atual só carrega plugin com \"core\": \">=5\" e o painel em panel/api.ts (api.py não é mais lido); porte o api.py para api.ts, declare core >=5 no profile.json e atualize o plugin (reference/projects.md §Plugin)"
    )
}

/// `coreVersion` (`src/lib/profile.ts:276`): 1ª linha de `<skill>/VERSION`, aparada.
pub fn core_version(env: &Env) -> String {
    first_line(&skill_root(env).join("VERSION"))
        .trim()
        .to_owned()
}

/// Estado do `core_check` (`src/lib/profile.ts:280`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreState {
    Ok,
    SemCore,
    Fora,
    Invalido,
    SemPerfil,
}

impl CoreState {
    /// O texto que o TS usa (`"sem-core"`, `"fora"`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::SemCore => "sem-core",
            Self::Fora => "fora",
            Self::Invalido => "invalido",
            Self::SemPerfil => "sem-perfil",
        }
    }
}

/// `CoreCheck` (`src/lib/profile.ts:280`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreCheck {
    pub state: CoreState,
    pub project: String,
    pub core: String,
    pub version: String,
    pub message: String,
}

/// `coreCheck` (`src/lib/profile.ts:288`): o `core` do perfil do diretório contra a versão do núcleo (a instalada, se
/// `version` é vazia).
pub fn core_check(env: &Env, dir: &Path, version: Option<&str>) -> CoreCheck {
    let resolution = resolve_project(env, dir);
    let make = |state, project: &str, core: &str, ver: &str, message: String| CoreCheck {
        state,
        project: project.to_owned(),
        core: core.to_owned(),
        version: ver.to_owned(),
        message,
    };
    if resolution.code != exit::OK {
        return make(CoreState::SemPerfil, "", "", "", resolution.message);
    }
    let profile = load_profile(&resolution);
    let project = resolution.project.clone().unwrap_or_default();
    let ver = match version {
        Some(v) if !v.is_empty() => v.to_owned(),
        _ => core_version(env),
    };
    let pack = resolution
        .profile
        .as_deref()
        .and_then(Path::parent)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let range = profile.get("core");
    let Some(range_value) = range else {
        return make(
            CoreState::SemCore,
            &project,
            "",
            &ver,
            format!(
                "o plugin {project} não declara `core` (faixa de versões do núcleo); acrescente \"core\" ao profile.json"
            ),
        );
    };
    let ok = match core_ok(range, &ver) {
        Ok(ok) => ok,
        Err(reason) => {
            return make(
                CoreState::Invalido,
                &project,
                &py_text(range),
                &ver,
                format!(
                    "core {} do plugin {project}: {reason}",
                    py_repr(Some(range_value))
                ),
            );
        }
    };
    let core = py_text(range);
    if ok && admits_legacy_core(range).unwrap_or(false) {
        let message = legacy_core_message(&project, &core);
        return make(CoreState::Fora, &project, &core, &ver, message);
    }
    if ok {
        let message = format!("plugin {project} pede o núcleo {core}; núcleo {ver}");
        return make(CoreState::Ok, &project, &core, &ver, message);
    }
    let message = format!(
        "o plugin {project} pede o núcleo {core}, mas o núcleo instalado é o {ver}: atualize o núcleo (skill Hades) ou o plugin (git -C {pack} pull)"
    );
    make(CoreState::Fora, &project, &core, &ver, message)
}

#[cfg(test)]
mod tests;
