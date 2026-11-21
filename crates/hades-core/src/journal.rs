//! Diário de eventos por agente (`src/lib/journal.ts`, `reference/journal.md`): JSONL em
//! `<raiz de trabalho>/<projeto>/journal/<agente>.jsonl`, uma linha por evento, versão `v: 1`; campo novo só se
//! acrescenta. Escrito pelo hook (guard), pelo watch, pelo gate-lock e pelo Cerberus; as unidades que consomem
//! (Argos, Thanatos) só leem.
//!
//! Gravar nunca falha nem atrasa o chamador: erro de disco ou de lock é engolido e contado.
//!
//! Diferenças deliberadas em relação ao TS:
//! - `parse` guarda em [`JournalEvent::extra`] os campos que não conhece (o TS os descarta), para quem regrava não
//!   perdê-los.
//! - O corte de resumo e o `slice` contam unidades UTF-16 como o JS, mas um corte no meio de um par substituto deixa o
//!   par inteiro de fora (o JS emitiria um `\ud83d` solto no JSON).
//! - A leitura de instante ISO sem fuso vale UTC, o JS usa o fuso local; o diário só grava `Z`.
//! - A escrita da retenção usa o tmp único de [`crate::atomic`], não o `<f>.tmp` fixo do TS.
//! - Não porta `eventOfHook`/`toolSummary` (`src/lib/journal.ts:119-180`): dependem de `agent-provider`, e ficam com o
//!   guard do Cerberus. [`mark_blocked`] e [`summarize_command`] estão aqui.

mod mask;
mod time;

#[cfg(test)]
mod tests;

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Map, Number, Value};

use crate::atomic::write_file_atomic;
use crate::env::Env;
use crate::fsx::{is_file, list_dir, read_text};
use crate::jsonfmt::json_compact;
use crate::lock::{try_lock, with_flock};
use crate::paths::{find_state_dir, skill_root, work_root};

pub use mask::{clip, mask_secrets, summarize_command};
pub use time::{iso_ms, now_ms, parse_iso, parse_since};

pub const JOURNAL_VERSION: u32 = 1;
/// `orch` / `agent.end`: o `agent done|park|skill-clean` encerrou a atividade do agente (`src/lib/journal.ts:16`).
pub const AGENT_END_KIND: &str = "agent.end";
pub const DEFAULT_DAYS: i64 = 14;
pub const DEFAULT_MAX_MB: i64 = 20;
pub const SESSIONS_DIR: &str = "_sessions";

const MIB: u64 = 1_048_576;
const SUMMARY_MAX: usize = 200;
const LINE_MAX_BYTES: usize = 3500;
const READ_TAIL_BYTES: u64 = 8 * MIB;
const LOCK_TRIES: u32 = 3;
const LOCK_RETRY: Duration = Duration::from_millis(1);
const DROPPED_FILE_MAX: u64 = 65536;
const MACHINE_DIR: &str = "_maquina";
const DECISION_FIELD_MAX: usize = 200;
const DECISION_ALTERNATIVES_MAX: usize = 5;
const DAY_MS: i64 = 86_400_000;

/// `JournalSource` (`src/lib/journal.ts:17`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Guard,
    Watch,
    Gate,
    Cerberus,
    Orch,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::Guard,
        Source::Watch,
        Source::Gate,
        Source::Cerberus,
        Source::Orch,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Source::Guard => "guard",
            Source::Watch => "watch",
            Source::Gate => "gate",
            Source::Cerberus => "cerberus",
            Source::Orch => "orch",
        }
    }

    pub fn parse(text: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// `DecisionFields` (`src/lib/journal.ts:37`): só em `kind: decision`.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub level: Number,
    pub needed: Number,
    pub question: String,
    pub chosen: String,
    pub alternatives: Vec<String>,
}

impl Decision {
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("level".into(), Value::Number(self.level.clone()));
        m.insert("needed".into(), Value::Number(self.needed.clone()));
        m.insert("question".into(), self.question.clone().into());
        m.insert("chosen".into(), self.chosen.clone().into());
        m.insert(
            "alternatives".into(),
            Value::Array(
                self.alternatives
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        Value::Object(m)
    }
}

/// Ordem das chaves na linha: o `JSON.stringify` do TS segue a ordem em que o objeto foi montado, e o hook, as outras
/// fontes e `parseEvent` montam em ordens diferentes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `eventOfHook` (`src/lib/journal.ts:154`): `v ts agent session source cwd kind [tool] summary`.
    Hook,
    /// `eventOfSource` (`src/lib/journal.ts:190`): `v ts agent session source kind summary cwd [decision]`.
    Source,
    /// `parseEvent` (`src/lib/journal.ts:411`): `v ts agent session source kind [tool] summary cwd [decision]`.
    Parsed,
}

/// Uma linha do diário (`JournalEvent`, `src/lib/journal.ts:21`).
#[derive(Clone, Debug, PartialEq)]
pub struct JournalEvent {
    pub v: Number,
    /// Instante em UTC, ISO 8601 com `Z`.
    pub ts: String,
    /// Nome do agente registrado; vazio quando a sessão não casa com nenhum.
    pub agent: String,
    pub session: String,
    pub source: Source,
    pub kind: String,
    pub tool: Option<String>,
    pub summary: String,
    pub cwd: String,
    pub decision: Option<Decision>,
    /// Campos que o Rust não conhece, lidos de uma linha (só na ordem `Parsed`).
    pub extra: Map<String, Value>,
    pub layout: Layout,
}

impl JournalEvent {
    /// O objeto na ordem de chaves do `layout`.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: Value| {
            m.insert(k.to_owned(), v);
        };
        put("v", Value::Number(self.v.clone()));
        put("ts", self.ts.clone().into());
        put("agent", self.agent.clone().into());
        put("session", self.session.clone().into());
        put("source", self.source.as_str().into());
        if self.layout == Layout::Hook {
            put("cwd", self.cwd.clone().into());
        }
        put("kind", self.kind.clone().into());
        if let Some(tool) = &self.tool {
            put("tool", tool.clone().into());
        }
        put("summary", self.summary.clone().into());
        if self.layout != Layout::Hook {
            put("cwd", self.cwd.clone().into());
        }
        if let Some(d) = &self.decision {
            put("decision", d.to_value());
        }
        for (k, v) in &self.extra {
            if !m.contains_key(k) {
                m.insert(k.clone(), v.clone());
            }
        }
        Value::Object(m)
    }

    /// `JSON.stringify(event)`.
    pub fn to_json(&self) -> String {
        json_compact(&self.to_value())
    }
}

/// `JournalSettings` (`src/lib/journal.ts:45`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub days: i64,
    pub max_bytes: u64,
}

/// `Number.parseInt(text.trim(), 10)`: sinal opcional e dígitos iniciais; `None` é o `NaN`.
fn parse_int_prefix(text: &str) -> Option<f64> {
    let t = text.trim_matches(time::is_js_space);
    let (negative, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let n: f64 = rest[..digits].parse().ok()?;
    Some(if negative { -n } else { n })
}

/// `journalSettings` (`src/lib/journal.ts:68`): `HADES_JOURNAL_*` do config; valor fora do intervalo cai no padrão.
pub fn journal_settings(get: &dyn Fn(&str) -> Option<String>) -> Settings {
    let int = |key: &str, fallback: i64, min: i64, max: i64| -> i64 {
        match parse_int_prefix(&get(key).unwrap_or_default()) {
            Some(n) if n >= min as f64 && n <= max as f64 => n as i64,
            _ => fallback,
        }
    };
    Settings {
        enabled: get("HADES_JOURNAL")
            .unwrap_or_else(|| "1".into())
            .trim_matches(time::is_js_space)
            != "0",
        days: int("HADES_JOURNAL_DAYS", DEFAULT_DAYS, 1, 3650),
        max_bytes: int("HADES_JOURNAL_MAX_MB", DEFAULT_MAX_MB, 1, 1024) as u64 * MIB,
    }
}

/// `journalSettings` lendo a configuração efetiva do `$D` do `cwd` (ambiente > `config.env` > catálogo). Sem
/// instalação, o catálogo é o de `<skill>/kit`, como em [`crate::config::config_value`].
pub fn settings_for(env: &Env, cwd: &Path) -> Settings {
    let dir = find_state_dir(env, cwd).unwrap_or_else(|| skill_root(env).join("kit"));
    let effective = crate::config::effective_config(&dir, env);
    journal_settings(&|k| effective.get(k).map(str::to_owned))
}

/// `decision` com cada campo mascarado e cortado (`maskDecision`, `src/lib/journal.ts:200`).
fn mask_decision(d: &Decision) -> Decision {
    let field = |t: &str| clip(&mask_secrets(t), DECISION_FIELD_MAX);
    Decision {
        level: d.level.clone(),
        needed: d.needed.clone(),
        question: field(&d.question),
        chosen: field(&d.chosen),
        alternatives: d
            .alternatives
            .iter()
            .take(DECISION_ALTERNATIVES_MAX)
            .map(|a| field(a))
            .collect(),
    }
}

/// Campos opcionais de [`event_of_source`].
#[derive(Clone, Debug, Default)]
pub struct SourceFields {
    pub agent: Option<String>,
    pub session: Option<String>,
    pub cwd: Option<String>,
    pub decision: Option<Decision>,
}

/// `eventOfSource` (`src/lib/journal.ts:189`): evento das outras fontes; o resumo passa pelo mascaramento como o do
/// hook.
pub fn event_of_source(
    source: Source,
    kind: &str,
    summary: &str,
    fields: SourceFields,
    now_ms: i64,
) -> JournalEvent {
    JournalEvent {
        v: Number::from(JOURNAL_VERSION),
        ts: iso_ms(now_ms),
        agent: fields.agent.unwrap_or_default(),
        session: fields.session.unwrap_or_default(),
        source,
        kind: kind.to_owned(),
        tool: None,
        summary: clip(&mask_secrets(summary), SUMMARY_MAX),
        cwd: fields.cwd.unwrap_or_default(),
        decision: fields.decision.as_ref().map(mask_decision),
        extra: Map::new(),
        layout: Layout::Source,
    }
}

/// `markBlocked` (`src/lib/journal.ts:183`): o guard barrou a chamada; a razão (1ª linha, mascarada) vai antes do
/// comando.
pub fn mark_blocked(event: &JournalEvent, reason: &str) -> JournalEvent {
    let first = reason.split('\n').next().unwrap_or("");
    let why = clip(&mask_secrets(first), SUMMARY_MAX / 2);
    let mut out = event.clone();
    out.kind = "tool_blocked".into();
    out.summary = clip(&format!("{why} | {}", event.summary), SUMMARY_MAX);
    out
}

// ------------------------------------------------------------------ agente da sessão

/// `AGENT_NAME` (`src/lib/journal.ts:61`): `^[\w.-]{1,80}$`, ASCII.
pub fn is_agent_name(name: &str) -> bool {
    (1..=80).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// `AgentHome`: o agente e `<raiz>/<projeto>/journal`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentHome {
    pub agent: String,
    pub dir: PathBuf,
}

fn read_object(path: &Path) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(&read_text(path)?).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn str_of<'a>(m: &'a Map<String, Value>, key: &str) -> &'a str {
    m.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `agentOfCwd` (`src/lib/journal.ts:236`): agente registrado (não `done`) cuja worktree contém o `cwd`.
pub fn agent_of_cwd(cwd: &str, root: &Path) -> Option<AgentHome> {
    if cwd.is_empty() {
        return None;
    }
    for project in list_dir(root) {
        let agents = root.join(&project).join("orchestration").join("agents");
        for name in list_dir(&agents) {
            let Some(agent) = name.strip_suffix(".json") else {
                continue;
            };
            let Some(reg) = read_object(&agents.join(&name)) else {
                continue;
            };
            let worktree = str_of(&reg, "worktree");
            let inside = cwd == worktree
                || cwd
                    .strip_prefix(worktree)
                    .is_some_and(|rest| rest.starts_with('/'));
            if reg.get("state").and_then(Value::as_str) == Some("done")
                || worktree.is_empty()
                || !inside
            {
                continue;
            }
            if is_agent_name(agent) {
                return Some(AgentHome {
                    agent: agent.to_owned(),
                    dir: root.join(&project).join("journal"),
                });
            }
        }
    }
    None
}

/// `agentHomeByName` (`src/lib/journal.ts:380`): pasta do diário do agente pelo nome (o watch conhece o agente, não o
/// cwd dele).
pub fn agent_home_by_name(agent: &str, root: &Path) -> Option<AgentHome> {
    list_dir(root).into_iter().find_map(|project| {
        let reg = root
            .join(&project)
            .join("orchestration")
            .join("agents")
            .join(format!("{agent}.json"));
        is_file(&reg).then(|| AgentHome {
            agent: agent.to_owned(),
            dir: root.join(&project).join("journal"),
        })
    })
}

/// `journalTarget` (`src/lib/journal.ts:253`): o arquivo do agente, ou o da sessão quando ela não casa com agente.
pub fn journal_target(event: &JournalEvent, home: Option<&AgentHome>, root: &Path) -> PathBuf {
    if let Some(h) = home {
        return h.dir.join(format!("{}.jsonl", h.agent));
    }
    let mut session: String = event
        .session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if session.is_empty() {
        session = "x".into();
    }
    root.join(MACHINE_DIR)
        .join("journal")
        .join(SESSIONS_DIR)
        .join(format!("{session}.jsonl"))
}

// ------------------------------------------------------------------ escrita

static DROPPED: AtomicU64 = AtomicU64::new(0);

/// `droppedCount` (`src/lib/journal.ts:309`): quantas linhas este processo deixou de gravar (disco, permissão, lock
/// ocupado).
pub fn dropped_count() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

/// `noteDrop` (`src/lib/journal.ts:312`): anota a perda ao lado do diário, se ainda der; nunca falha.
fn note_drop(dir: &Path, reason: &str, now_ms: i64) {
    DROPPED.fetch_add(1, Ordering::Relaxed);
    let file = dir.join(".dropped");
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if size >= DROPPED_FILE_MAX {
        return;
    }
    let flat = clip_flat(reason);
    let line = format!("{}\t{}\n", iso_ms(now_ms), mask::slice_utf16(&flat, 120));
    let _ = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// `reason.replace(/\s+/g, " ")` (sem aparar).
fn clip_flat(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if time::is_js_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `rotatedName` (`src/lib/journal.ts:328`): `a.jsonl` vira `a.1.jsonl`.
pub fn rotated_name(file: &Path) -> PathBuf {
    let text = file.to_string_lossy();
    match text.strip_suffix(".jsonl") {
        Some(stem) => PathBuf::from(format!("{stem}.1.jsonl")),
        None => file.to_path_buf(),
    }
}

fn dir_of(target: &Path) -> PathBuf {
    target.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// Lock exclusivo sem esperar além de `LOCK_TRIES` × 1 ms; `Ok(false)` se outro processo o segurou o tempo todo
/// (`withLockBounded`, `src/lib/journal.ts:269`).
fn with_lock_bounded(
    path: &Path,
    f: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<bool> {
    for i in 0..LOCK_TRIES {
        if let Some(_guard) = try_lock(path)? {
            f()?;
            return Ok(true);
        }
        if i + 1 < LOCK_TRIES {
            thread::sleep(LOCK_RETRY);
        }
    }
    Ok(false)
}

fn write_line(target: &Path, dir: &Path, line: &str, max_bytes: u64) -> std::io::Result<bool> {
    fs::create_dir_all(dir)?;
    with_lock_bounded(&dir.join(".lock"), || {
        let size = fs::metadata(target).map(|m| m.len()).unwrap_or(0);
        if size >= max_bytes {
            fs::rename(target, rotated_name(target))?;
        }
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(target)?
            .write_all(line.as_bytes())
    })
}

/// `appendEvent` (`src/lib/journal.ts:333`): uma linha no arquivo do alvo, com rotação no teto. Devolve `false` (e
/// conta) se não gravou; nunca falha. Linha acima de 3500 bytes sai com o `summary` cortado em 80, para caber numa
/// escrita atômica.
pub fn append(event: &JournalEvent, target: &Path, settings: &Settings, now_ms: i64) -> bool {
    if !settings.enabled {
        return false;
    }
    let dir = dir_of(target);
    let mut line = format!("{}\n", event.to_json());
    if line.len() > LINE_MAX_BYTES {
        let mut short = event.clone();
        short.summary = clip(&event.summary, 80);
        line = format!("{}\n", short.to_json());
    }
    match write_line(target, &dir, &line, settings.max_bytes) {
        Ok(true) => true,
        Ok(false) => {
            note_drop(&dir, "lock ocupado", now_ms);
            false
        }
        Err(e) => {
            note_drop(&dir, &e.to_string(), now_ms);
            false
        }
    }
}

/// Opções de [`emit`].
#[derive(Default)]
pub struct EmitOptions<'a> {
    pub agent: Option<String>,
    pub cwd: Option<String>,
    pub now_ms: Option<i64>,
    pub root: Option<PathBuf>,
    pub decision: Option<Decision>,
    /// `get` da configuração; ausente, vale o padrão do catálogo ([`journal_settings`] com tudo vazio).
    pub settings: Option<Settings>,
    /// Ambiente para a raiz de trabalho quando `root` não vem.
    pub env: Option<&'a Env>,
}

/// `emitJournal` (`src/lib/journal.ts:359`): grava um evento de fonte que não é o hook (watch, gate, cerberus, orch).
/// O agente vem dado ou do `cwd`; sem agente, a sessão é `_<fonte>`. Nunca falha.
pub fn emit(source: Source, kind: &str, summary: &str, opts: EmitOptions<'_>) -> bool {
    let settings = opts.settings.unwrap_or_else(|| journal_settings(&|_| None));
    if !settings.enabled {
        return false;
    }
    let now = opts.now_ms.unwrap_or_else(now_ms);
    let root = opts
        .root
        .clone()
        .unwrap_or_else(|| work_root(&opts.env.cloned().unwrap_or_else(Env::from_process)));
    let cwd = opts.cwd.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let named = opts.agent.as_deref().filter(|a| is_agent_name(a));
    let home = match named {
        Some(name) => agent_home_by_name(name, &root),
        None => agent_of_cwd(&cwd, &root),
    };
    let event = event_of_source(
        source,
        kind,
        summary,
        SourceFields {
            agent: Some(home.as_ref().map(|h| h.agent.clone()).unwrap_or_default()),
            session: Some(if home.is_some() {
                String::new()
            } else {
                format!("_{}", source.as_str())
            }),
            cwd: Some(cwd),
            decision: opts.decision,
        },
        now,
    );
    append(
        &event,
        &journal_target(&event, home.as_ref(), &root),
        &settings,
        now,
    )
}

// ------------------------------------------------------------------ leitura

fn parse_decision(value: Option<&Value>) -> Option<Decision> {
    let obj = value?.as_object()?;
    let level = obj.get("level")?.as_number()?.clone();
    let needed = obj.get("needed")?.as_number()?.clone();
    let alternatives = obj
        .get("alternatives")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(Decision {
        level,
        needed,
        question: str_of(obj, "question").to_owned(),
        chosen: str_of(obj, "chosen").to_owned(),
        alternatives,
    })
}

const KNOWN: [&str; 10] = [
    "v", "ts", "agent", "session", "source", "kind", "tool", "summary", "cwd", "decision",
];

/// `parseEvent` (`src/lib/journal.ts:402`): linha validada na fronteira; `None` se cortada (disco cheio) ou fora do
/// formato.
pub fn parse(line: &str) -> Option<JournalEvent> {
    let Value::Object(row) = serde_json::from_str::<Value>(line).ok()? else {
        return None;
    };
    let v = row.get("v")?.as_number()?.clone();
    let ts = row.get("ts")?.as_str()?.to_owned();
    let source = Source::parse(row.get("source")?.as_str()?)?;
    let kind = row.get("kind")?.as_str()?.to_owned();
    let summary = row.get("summary")?.as_str()?.to_owned();
    parse_iso(&ts)?;
    Some(JournalEvent {
        v,
        ts,
        agent: str_of(&row, "agent").to_owned(),
        session: str_of(&row, "session").to_owned(),
        source,
        kind,
        tool: row.get("tool").and_then(Value::as_str).map(str::to_owned),
        summary,
        cwd: str_of(&row, "cwd").to_owned(),
        decision: parse_decision(row.get("decision")),
        extra: row
            .iter()
            .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        layout: Layout::Parsed,
    })
}

/// `readTailText` (`src/lib/journal.ts:419`): os últimos `max_bytes` do arquivo, sem a primeira linha se o corte caiu
/// no meio dela. Vazio se não abre.
pub fn read_tail(file: &Path, max_bytes: u64) -> String {
    let read = || -> std::io::Result<String> {
        let mut f = fs::File::open(file)?;
        let size = f.metadata()?.len();
        let start = size.saturating_sub(max_bytes);
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::with_capacity((size - start) as usize);
        f.read_to_end(&mut buf)?;
        let text = String::from_utf8_lossy(&buf).into_owned();
        Ok(match (start > 0, text.find('\n')) {
            (true, Some(i)) => text[i + 1..].to_owned(),
            _ => text,
        })
    };
    read().unwrap_or_default()
}

/// [`read_tail`] com o teto padrão de 8 MiB.
pub fn read_tail_default(file: &Path) -> String {
    read_tail(file, READ_TAIL_BYTES)
}

/// `JournalFile`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalFile {
    pub agent: String,
    pub project: String,
    pub file: PathBuf,
}

/// `journalFiles` (`src/lib/journal.ts:447`): diários existentes, `<raiz>/<projeto>/journal/<agente>.jsonl` (as
/// gerações `.1` vêm junto) e, com `include_sessions`, as sessões sem agente (`_sessions/<id>`).
pub fn files(root: &Path, include_sessions: bool) -> Vec<JournalFile> {
    let mut found = Vec::new();
    for project in list_dir(root) {
        let dir = root.join(&project).join("journal");
        let mut dirs = vec![(dir.clone(), false)];
        if include_sessions {
            dirs.push((dir.join(SESSIONS_DIR), true));
        }
        for (d, is_sessions) in dirs {
            for name in list_dir(&d) {
                let Some(stem) = name.strip_suffix(".jsonl") else {
                    continue;
                };
                let agent = stem.strip_suffix(".1").unwrap_or(stem);
                if is_agent_name(agent) {
                    found.push(JournalFile {
                        agent: if is_sessions {
                            format!("{SESSIONS_DIR}/{agent}")
                        } else {
                            agent.to_owned()
                        },
                        project: project.clone(),
                        file: d.join(&name),
                    });
                }
            }
        }
    }
    found
}

/// `JournalQuery`.
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub agent: Option<String>,
    pub since_ms: Option<i64>,
    pub kind: Option<String>,
    pub limit: usize,
}

/// `kindMatches` (`src/lib/journal.ts:471`): `tool` casa `tool_pre`/`tool_post`, `gate` casa `gate.done`; o valor
/// exato também.
pub fn kind_matches(kind: &str, wanted: &str) -> bool {
    kind == wanted
        || kind
            .strip_prefix(wanted)
            .is_some_and(|r| r.starts_with('.') || r.starts_with('_'))
}

/// `readEvents` (`src/lib/journal.ts:476`): os `limit` eventos mais recentes que casam, em ordem de tempo. Lê só o fim
/// de cada arquivo. `limit` 0 devolve todos (o `slice(-0)` do JS).
pub fn read_events(query: &Query, root: &Path) -> Vec<JournalEvent> {
    let sessions = query
        .agent
        .as_deref()
        .is_some_and(|a| a.starts_with(&format!("{SESSIONS_DIR}/")));
    let mut events = Vec::new();
    for entry in files(root, sessions) {
        if query.agent.as_deref().is_some_and(|a| a != entry.agent) {
            continue;
        }
        for line in read_tail_default(&entry.file).split('\n') {
            if line.is_empty() {
                continue;
            }
            let Some(event) = parse(line) else { continue };
            if let Some(since) = query.since_ms
                && parse_iso(&event.ts).is_some_and(|at| at < since)
            {
                continue;
            }
            if query
                .kind
                .as_deref()
                .is_some_and(|k| !kind_matches(&event.kind, k))
            {
                continue;
            }
            events.push(event);
        }
    }
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    if query.limit > 0 && events.len() > query.limit {
        events.drain(..events.len() - query.limit);
    }
    events
}

/// `JournalSummary`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub agent: String,
    pub project: String,
    pub events: usize,
    pub bytes: u64,
    pub last: String,
}

/// `listJournals` (`src/lib/journal.ts:516`): um item por agente (arquivo vivo e geração `.1` somados), o mais recente
/// primeiro.
pub fn list_journals(root: &Path) -> Vec<Summary> {
    let mut order: Vec<String> = Vec::new();
    let mut by_agent: std::collections::HashMap<String, Summary> = Default::default();
    for entry in files(root, true) {
        let key = format!("{}/{}", entry.project, entry.agent);
        if !by_agent.contains_key(&key) {
            order.push(key.clone());
        }
        let item = by_agent.entry(key).or_insert_with(|| Summary {
            agent: entry.agent.clone(),
            project: entry.project.clone(),
            events: 0,
            bytes: 0,
            last: String::new(),
        });
        let text = read_tail(&entry.file, MIB);
        let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
        item.events += lines.len();
        item.bytes += fs::metadata(&entry.file).map(|m| m.len()).unwrap_or(0);
        let live = !entry
            .file
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(".1.jsonl"));
        if live && let Some(last) = lines.iter().rev().find_map(|l| parse(l)) {
            item.last = last.ts;
        }
    }
    let mut out: Vec<Summary> = order
        .into_iter()
        .filter_map(|k| by_agent.remove(&k))
        .collect();
    out.sort_by(|a, b| b.last.cmp(&a.last));
    out
}

// ------------------------------------------------------------------ retenção

/// O que a limpeza faz (ou faria, no `dry_run`) em um arquivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PruneAction {
    Apaga,
    Reescreve,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneItem {
    pub file: PathBuf,
    pub action: PruneAction,
    pub bytes: u64,
}

/// `PruneResult`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PruneResult {
    pub removed: usize,
    pub rewritten: usize,
    pub bytes: u64,
    pub items: Vec<PruneItem>,
}

struct Split {
    kept: Vec<String>,
    dropped: usize,
    bytes: u64,
}

/// `recentSplit` (`src/lib/journal.ts:577`): linhas que ficam (recentes ou ilegíveis) e as que saem, com os bytes
/// destas.
fn recent_split(text: &str, cutoff_ms: i64) -> Split {
    let mut split = Split {
        kept: Vec::new(),
        dropped: 0,
        bytes: 0,
    };
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        match parse(line).and_then(|e| parse_iso(&e.ts)) {
            Some(at) if at < cutoff_ms => {
                split.dropped += 1;
                split.bytes += line.len() as u64 + 1;
            }
            _ => split.kept.push(line.to_owned()),
        }
    }
    split
}

fn mtime_ms(meta: &fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .map(|t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs_f64() * 1000.0,
            Err(e) => -e.duration().as_secs_f64() * 1000.0,
        })
        .unwrap_or(0.0)
}

/// `rewriteLocked` (`src/lib/journal.ts:602`): relê e filtra dentro do lock do diretório; linha gravada entre a
/// primeira leitura e o lock não se perde.
fn rewrite_locked(file: &Path, cutoff_ms: i64) {
    let dir = dir_of(file);
    let _ = with_flock(&dir.join(".lock"), || {
        let Split { kept, .. } = recent_split(&read_text(file).unwrap_or_default(), cutoff_ms);
        let body = if kept.is_empty() {
            String::new()
        } else {
            format!("{}\n", kept.join("\n"))
        };
        write_file_atomic(file, body.as_bytes())
    });
}

/// `pruneJournals` (`src/lib/journal.ts:549`): apaga o arquivo parado há mais que `days` e tira de dentro dos vivos as
/// linhas mais velhas que o corte. Idempotente.
pub fn prune(days: i64, now_ms: i64, root: &Path, dry_run: bool) -> PruneResult {
    let cutoff = now_ms - days * DAY_MS;
    let mut result = PruneResult::default();
    for entry in files(root, true) {
        let Ok(meta) = fs::metadata(&entry.file) else {
            continue;
        };
        if mtime_ms(&meta) < cutoff as f64 {
            result.removed += 1;
            result.bytes += meta.len();
            result.items.push(PruneItem {
                file: entry.file.clone(),
                action: PruneAction::Apaga,
                bytes: meta.len(),
            });
            if !dry_run {
                let _ = fs::remove_file(&entry.file);
            }
            continue;
        }
        let split = recent_split(&read_text(&entry.file).unwrap_or_default(), cutoff);
        if split.dropped == 0 {
            continue;
        }
        result.rewritten += 1;
        result.bytes += split.bytes;
        result.items.push(PruneItem {
            file: entry.file.clone(),
            action: PruneAction::Reescreve,
            bytes: split.bytes,
        });
        if !dry_run {
            rewrite_locked(&entry.file, cutoff);
        }
    }
    result
}
