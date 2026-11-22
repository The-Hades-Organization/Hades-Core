//! Configuração da instância: `env.sh`, catálogo `config.tsv`, `config.env` e valor efetivo
//! (`src/lib/config.ts`, `src/lib/config-catalog.ts`, `configValue` de `src/lib/work.ts`).
//!
//! Há dois leitores de catálogo no TS: o brando de `config.ts` ([`read_catalog`], ignora linha ruim) e o estrito de
//! `config-catalog.ts` ([`catalog`], recusa linha fora do formato). Cada um segue a sua fonte.
//!
//! Fora deste módulo, por ora: `validate` por tipo, `exportText` e `plan` (`config-catalog.ts`). [`save`] grava o que
//! recebe (com `trim`) e quem chama valida.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic::write_file_atomic;
use crate::env::Env;
use crate::fsx::read_text;
use crate::lock::with_flock;
use crate::paths::{find_state_dir, skill_root};

const HEADER: &str = "# Configurações do Hades escolhidas pelo dono (painel → Configurações, ou config.sh set). Só o que difere\n# do padrão do catálogo (config.tsv). Gerado: edite pelo painel ou pelo config.sh, que validam os valores.\n";
const SECRET_TYPES: [&str; 1] = ["secret"];
const IDENTITY_TYPES: [&str; 6] = ["path", "login", "logins", "emails", "digits", "url"];
const SLICE_ERROR: &str = "Pressão de memória (MemoryHigh) não pode passar do teto (MemoryMax)";

/// `ConfigError` de `config-catalog.ts:18`; erro de disco entra na mesma mensagem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

/// `^[A-Z][A-Z0-9_]*$` (`config.ts:8`).
pub fn is_key(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('A'..='Z'))
        && chars.all(|c| matches!(c, 'A'..='Z' | '0'..='9' | '_'))
}

/// `str.splitlines()` do Python (`config-catalog.ts:48`): além de `\n`, também `\r`, `\v`, `\f`, `\x1c-\x1e`, NEL,
/// U+2028 e U+2029; `\r\n` conta uma vez; o último pedaço vazio cai.
pub fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(std::mem::take(&mut cur));
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                lines.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    lines.push(cur);
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// Mapa que guarda a ordem de inserção; regravar uma chave mantém a posição (como o `Map` do JS).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderedMap {
    entries: Vec<(String, String)>,
}

impl OrderedMap {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn set(&mut self, key: &str, value: &str) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.to_owned(),
            None => self.entries.push((key.to_owned(), value.to_owned())),
        }
    }

    pub fn delete(&mut self, key: &str) {
        self.entries.retain(|(k, _)| k != key);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `K="v"` / `K=v` no começo da linha (`ASSIGNMENT`, `config.ts:9`).
fn parse_assignment(line: &str) -> Option<(&str, String)> {
    let eq = line.find('=')?;
    let name = &line[..eq];
    let mut nc = name.chars();
    let first = nc.next()?;
    if !(first.is_ascii_alphabetic() || first == '_')
        || !nc.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = &line[eq + 1..];
    if let Some(quoted) = rest.strip_prefix('"')
        && let Some(end) = quoted.find('"')
    {
        return Some((name, quoted[..end].to_owned()));
    }
    let value: String = rest
        .chars()
        .take_while(|&c| !is_js_space(c) && c != '"' && c != '#')
        .collect();
    Some((name, value))
}

/// Atribuições do `env.sh`; a primeira de cada chave vale (`src/lib/config.ts:12`).
pub fn parse_env_sh(text: &str) -> OrderedMap {
    let mut values = OrderedMap::default();
    for line in text.split('\n') {
        if let Some((key, value)) = parse_assignment(line)
            && !values.has(key)
        {
            values.set(key, &value);
        }
    }
    values
}

/// `readEnvSh` (`src/lib/config.ts:22`).
pub fn read_env_sh(dir: &Path) -> OrderedMap {
    parse_env_sh(&read_text(&dir.join("env.sh")).unwrap_or_default())
}

/// `HADES_ID` do `env.sh`: a primeira linha `HADES_ID=`, vazio se não há (`src/lib/config.ts:27`).
pub fn instance_id(dir: &Path) -> String {
    let Some(text) = read_text(&dir.join("env.sh")) else {
        return String::new();
    };
    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("HADES_ID=") {
            let rest = rest.strip_prefix('"').unwrap_or(rest);
            return rest
                .chars()
                .take_while(|&c| c != '"' && c != ' ' && c != '#')
                .collect();
        }
    }
    String::new()
}

/// Prefixo das units da instância: `hds`, ou `hds_<id>` com `HADES_ID` (ambiente > `env.sh`)
/// (`src/lib/config.ts:115`).
pub fn unit_prefix(env: &Env, dir: &Path) -> String {
    let mut id = env
        .get("HADES_ID")
        .map(str::to_owned)
        .unwrap_or_else(|| instance_id(dir));
    let valid = id.chars().count() <= 24
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        id = id
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(24)
            .collect();
    }
    if id.is_empty() {
        "hds".to_owned()
    } else {
        format!("hds_{id}")
    }
}

/// Entrada do catálogo brando (`CatalogEntry`, `src/lib/config.ts:37`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub key: String,
    pub default: String,
}

/// Catálogo do kit seguido do plugin; para a mesma chave o padrão do plugin vence, na posição da primeira
/// (`src/lib/config.ts:43`). Linha ruim é ignorada.
pub fn read_catalog(dir: &Path) -> Vec<CatalogEntry> {
    read_catalog_files(&[
        dir.join("config.tsv"),
        dir.join("plugin").join("config.tsv"),
    ])
}

fn read_catalog_files(files: &[PathBuf]) -> Vec<CatalogEntry> {
    let mut entries: Vec<CatalogEntry> = Vec::new();
    for file in files {
        let Some(text) = read_text(file) else {
            continue;
        };
        for line in text.split('\n') {
            let mut cols = line.split('\t');
            let key = cols.next().unwrap_or("");
            if !is_key(key) {
                continue;
            }
            let value = cols.next().unwrap_or("").to_owned();
            match entries.iter_mut().find(|e| e.key == key) {
                Some(e) => e.default = value,
                None => entries.push(CatalogEntry {
                    key: key.to_owned(),
                    default: value,
                }),
            }
        }
    }
    entries
}

/// Catálogo de um `$D` que pode não ter o `config.tsv` do núcleo (`HADES_DATA` exportado sem instalação): sem ele, os
/// padrões vêm do `config.tsv` do kit da skill, que é de onde o `configValue` do TS os lê. O do plugin do `$D` segue valendo.
pub fn read_catalog_or_kit(dir: &Path, kit: &Path) -> Vec<CatalogEntry> {
    if dir.join("config.tsv").is_file() {
        return read_catalog(dir);
    }
    read_catalog_files(&[
        kit.join("config.tsv"),
        dir.join("plugin").join("config.tsv"),
    ])
}

/// `readConfigEnv` (`src/lib/config.ts:57`): `CHAVE=valor` por linha, separando em `\n`.
pub fn read_config_env(dir: &Path) -> OrderedMap {
    let mut values = OrderedMap::default();
    let Some(text) = read_text(&dir.join("config.env")) else {
        return values;
    };
    for line in text.split('\n') {
        let Some(eq) = line.find('=') else { continue };
        if is_key(&line[..eq]) {
            values.set(&line[..eq], &line[eq + 1..]);
        }
    }
    values
}

/// `effectiveConfig` (`src/lib/config.ts:70`): ambiente > `config.env` > padrão do catálogo; só chaves do catálogo.
/// A variável de ambiente vazia vale (`??`).
pub fn effective_config(dir: &Path, env: &Env) -> OrderedMap {
    effective_from(dir, env, read_catalog(dir))
}

fn effective_from(dir: &Path, env: &Env, catalog: Vec<CatalogEntry>) -> OrderedMap {
    let own = read_config_env(dir);
    let mut effective = OrderedMap::default();
    for CatalogEntry { key, default } in catalog {
        let value = env
            .get_raw(&key)
            .or_else(|| own.get(&key))
            .unwrap_or(&default)
            .to_owned();
        effective.set(&key, &value);
    }
    effective
}

/// `configValue` (`src/lib/work.ts:156`) com a regra de instância unificada (decisão do dono, 2026-11-21): o `$D` do
/// `cwd` ([`find_state_dir`]); sem instalação, `<skill>/kit`. O TS lê sempre `<hh>/instance` e exige `config.sh`.
pub fn config_value(env: &Env, cwd: &Path, key: &str, fallback: &str) -> String {
    let dir = find_state_dir(env, cwd).unwrap_or_else(|| skill_root(env).join("kit"));
    if let Some(v) = env.get_raw(key) {
        return v.to_owned();
    }
    let kit = skill_root(env).join("kit");
    effective_from(&dir, env, read_catalog_or_kit(&dir, &kit))
        .get(key)
        .unwrap_or(fallback)
        .to_owned()
}

/// `ConfigStore` (`config-catalog.ts:55`).
#[derive(Debug, Clone)]
pub struct ConfigStore {
    /// `$D`: onde ficam `config.env` e `plugin/config.tsv`.
    pub dir: PathBuf,
    /// Pasta do kit: `config.tsv` do núcleo.
    pub kit: PathBuf,
}

/// `Item` (`config-catalog.ts:21`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub default: String,
    pub kind: String,
    pub group: String,
    pub label: String,
    pub apply: String,
    pub desc: String,
    pub plugin: bool,
}

/// `Effective` (`config-catalog.ts:32`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub item: Item,
    pub value: String,
    pub changed: bool,
}

/// `readCatalogFile` (`config-catalog.ts:67`): sete colunas TAB e chave em maiúsculas, senão erro com a linha.
fn read_catalog_file(path: &Path) -> Result<Vec<Item>, ConfigError> {
    let bytes = fs::read(path).map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut rows = Vec::new();
    for (index, line) in split_lines(&text).iter().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 7 || !is_key(cols[0]) {
            return Err(ConfigError(format!(
                "{}: linha {} fora do formato (7 colunas TAB, chave em MAIÚSCULAS)",
                path.display(),
                index + 1
            )));
        }
        rows.push(Item {
            key: cols[0].to_owned(),
            default: cols[1].to_owned(),
            kind: cols[2].to_owned(),
            group: cols[3].to_owned(),
            label: cols[4].to_owned(),
            apply: cols[5].to_owned(),
            desc: cols[6].to_owned(),
            plugin: false,
        });
    }
    Ok(rows)
}

/// `catalog` (`config-catalog.ts:81`): kit e depois plugin; chave do plugin já no kit só troca o padrão.
pub fn catalog(store: &ConfigStore) -> Result<Vec<Item>, ConfigError> {
    let mut items = read_catalog_file(&store.kit.join("config.tsv"))?;
    let plugin_file = store.dir.join("plugin").join("config.tsv");
    if read_text(&plugin_file).is_none() {
        return Ok(items);
    }
    for item in read_catalog_file(&plugin_file)? {
        match items.iter_mut().find(|i| i.key == item.key) {
            Some(known) => known.default = item.default,
            None => items.push(Item {
                plugin: true,
                ..item
            }),
        }
    }
    Ok(items)
}

/// Sensibilidade da chave (`config-catalog.ts:91`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    Secret,
    Identity,
    None,
}

/// `sensitivity` (`config-catalog.ts:91`): `secret` pelo tipo, `identity` para caminho, conta, e-mail, número e URL.
pub fn sensitivity(item: &Item) -> Sensitivity {
    if SECRET_TYPES.contains(&item.kind.as_str()) {
        Sensitivity::Secret
    } else if IDENTITY_TYPES.contains(&item.kind.as_str()) {
        Sensitivity::Identity
    } else {
        Sensitivity::None
    }
}

/// `loadValues` (`config-catalog.ts:106`): como [`read_config_env`], separando linhas à moda do Python.
pub fn load_values(store: &ConfigStore) -> OrderedMap {
    let mut values = OrderedMap::default();
    let Some(text) = read_text(&store.dir.join("config.env")) else {
        return values;
    };
    for line in split_lines(&text) {
        let Some(eq) = line.find('=') else { continue };
        if is_key(&line[..eq]) {
            values.set(&line[..eq], &line[eq + 1..]);
        }
    }
    values
}

/// `effective` (`config-catalog.ts:223`): valor do dono ou padrão; `changed` se o dono difere do padrão.
pub fn effective(store: &ConfigStore) -> Result<Vec<Effective>, ConfigError> {
    let own = load_values(store);
    Ok(catalog(store)?
        .into_iter()
        .map(|item| {
            let value = own.get(&item.key).unwrap_or(&item.default).to_owned();
            let changed = own.has(&item.key) && value != item.default;
            Effective {
                item,
                value,
                changed,
            }
        })
        .collect())
}

/// `sizeBytes` (`config-catalog.ts:119`): `parseInt` do número sem o último caractere, vezes GiB (`G`) ou MiB.
/// `None` é o `NaN` do JS (toda comparação com ele dá falso).
pub fn size_bytes(value: &str) -> Option<f64> {
    let mut chars = value.chars();
    let last = chars.next_back()?;
    let head = chars.as_str().trim_start_matches(is_js_space);
    let (sign, digits) = match head.strip_prefix('-') {
        Some(r) => (-1.0, r),
        None => (1.0, head.strip_prefix('+').unwrap_or(head)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let n: f64 = digits[..end].parse().ok()?;
    let unit = if last == 'G' {
        1024f64.powi(3)
    } else {
        1024f64.powi(2)
    };
    Some(sign * n * unit)
}

/// `save` (`config-catalog.ts:233`): aplica `unset` e `updates`, grava `config.env` (chaves do catálogo na ordem do
/// catálogo, só as diferentes do padrão, depois as órfãs) e devolve os modos de aplicar das chaves que mudaram,
/// ordenados e sem repetição. O ciclo ler-alterar-gravar roda sob `flock` em `config.env.lock`, e a gravação usa tmp
/// único + `rename` (contrato da #147 da skill; o TS instalado ainda usa `<f>.tmp` fixo sem lock).
///
/// Os valores são gravados com `trim`, sem a validação por tipo do `validate` do TS.
pub fn save(
    store: &ConfigStore,
    updates: &[(&str, &str)],
    unset: &[&str],
) -> Result<Vec<String>, ConfigError> {
    let target = store.dir.join("config.env");
    let lock = store.dir.join("config.env.lock");
    with_flock(&lock, || save_locked(store, &target, updates, unset))
        .map_err(|e| ConfigError(e.to_string()))?
}

fn save_locked(
    store: &ConfigStore,
    target: &Path,
    updates: &[(&str, &str)],
    unset: &[&str],
) -> Result<Vec<String>, ConfigError> {
    let items = catalog(store)?;
    let find = |key: &str| items.iter().find(|i| i.key == key);
    let mut own = load_values(store);
    let value_of =
        |map: &OrderedMap, item: &Item| map.get(&item.key).unwrap_or(&item.default).to_owned();
    let before: Vec<String> = items.iter().map(|i| value_of(&own, i)).collect();
    for key in unset {
        if find(key).is_none() {
            return Err(ConfigError(format!("chave desconhecida: {key}")));
        }
        own.delete(key);
    }
    for (key, value) in updates {
        if find(key).is_none() {
            return Err(ConfigError(format!("chave desconhecida: {key}")));
        }
        own.set(key, value.trim_matches(is_js_space));
    }
    let after: Vec<String> = items.iter().map(|i| value_of(&own, i)).collect();
    let slice = |name: &str| {
        items
            .iter()
            .position(|i| i.key == name)
            .and_then(|at| size_bytes(&after[at]))
    };
    if let (Some(high), Some(max)) = (slice("HADES_SLICE_HIGH"), slice("HADES_SLICE_MAX"))
        && high > max
    {
        return Err(ConfigError(SLICE_ERROR.to_owned()));
    }
    let mut text = String::from(HEADER);
    for item in &items {
        if let Some(v) = own.get(&item.key)
            && v != item.default
        {
            text.push_str(&format!("{}={v}\n", item.key));
        }
    }
    for (key, value) in own.iter() {
        if find(key).is_none() {
            text.push_str(&format!("{key}={value}\n"));
        }
    }
    write_file_atomic(target, text.as_bytes()).map_err(|e| ConfigError(e.to_string()))?;
    let mut applies: Vec<String> = items
        .iter()
        .enumerate()
        .filter(|(at, _)| before[*at] != after[*at])
        .map(|(_, i)| i.apply.clone())
        .collect();
    applies.sort();
    applies.dedup();
    Ok(applies)
}

#[cfg(test)]
mod tests;
