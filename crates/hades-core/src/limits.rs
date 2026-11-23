//! Limites de uso do plano (janela de 5 h e semanal), só leitura: amostras gravadas, retenção e leitura atual de cada
//! janela (`src/lib/limites.ts:1-115`, `src/lib/limites-io.ts:24-55`).
//!
//! A gravação de `samples.jsonl` e a calibração ficam no Moirai.

use std::path::Path;

use serde_json::{Map, Number, Value};

use crate::config::config_value;
use crate::env::Env;
use crate::fsx::read_text;
use crate::output::warn;
use crate::paths::hades_home;

const MS_POR_DIA: f64 = 86_400_000.0;
const RETENCAO_PADRAO: u32 = 30;
const RETENCAO_MIN: u32 = 1;
const RETENCAO_MAX: u32 = 365;

/// `Janela` (`limites.ts:5`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Janela {
    Five,
    Seven,
}

/// `JANELAS` (`limites.ts:6`), na ordem de declaração.
pub const JANELAS: [Janela; 2] = [Janela::Five, Janela::Seven];

impl Janela {
    /// A chave do JSON (`five`, `seven`).
    pub fn key(self) -> &'static str {
        match self {
            Janela::Five => "five",
            Janela::Seven => "seven",
        }
    }

    /// `ROTULO` (`limites.ts:7`).
    pub fn rotulo(self) -> &'static str {
        match self {
            Janela::Five => "5 h",
            Janela::Seven => "semana",
        }
    }
}

/// `Leitura` (`limites.ts:10`): % usado (0–100) de uma janela e quando ela renova (epoch s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leitura {
    pub pct: f64,
    pub resets_at: f64,
}

/// `Amostra` (`limites.ts:15`).
#[derive(Clone, Debug, PartialEq)]
pub struct Amostra {
    /// Instante da leitura, UTC (ISO 8601 com Z).
    pub at: String,
    pub five: Option<Leitura>,
    pub seven: Option<Leitura>,
    pub version: Option<String>,
}

impl Amostra {
    pub fn janela(&self, janela: Janela) -> Option<Leitura> {
        match janela {
            Janela::Five => self.five,
            Janela::Seven => self.seven,
        }
    }
}

/// `leitura` (`limites.ts:38`).
fn leitura(pct: Option<&Value>, resets_at: Option<&Value>) -> Option<Leitura> {
    let pct = pct?.as_f64()?;
    let resets_at = resets_at?.as_f64()?;
    if !pct.is_finite() || !resets_at.is_finite() || pct < 0.0 {
        return None;
    }
    Some(Leitura { pct, resets_at })
}

/// A leitura gravada (`{pct, resetsAt}`) de `amostrasDe` (`limites.ts:62`).
fn leitura_gravada(value: Option<&Value>) -> Option<Leitura> {
    let map = value?.as_object()?;
    leitura(map.get("pct"), map.get("resetsAt"))
}

/// `amostrasDe` (`limites.ts:62`): a linha que não é uma amostra válida é ignorada, o resto segue em ordem de horário.
pub fn amostras_de(text: &str) -> Vec<Amostra> {
    let mut found = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            continue;
        }
        let Ok(doc) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        let Some(map) = doc.as_object() else {
            continue;
        };
        let Some(at) = map.get("at").and_then(Value::as_str) else {
            continue;
        };
        if parse_ts_ms(at).is_nan() {
            continue;
        }
        let five = leitura_gravada(map.get("five"));
        let seven = leitura_gravada(map.get("seven"));
        if five.is_none() && seven.is_none() {
            continue;
        }
        found.push(Amostra {
            at: at.to_owned(),
            five,
            seven,
            version: map
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    found.sort_by(|a, b| parse_ts_ms(&a.at).total_cmp(&parse_ts_ms(&b.at)));
    found
}

/// `dentroDaRetencao` (`limites.ts:90`).
pub fn dentro_da_retencao(amostras: &[Amostra], now_epoch_ms: f64, dias: f64) -> Vec<Amostra> {
    let corte = now_epoch_ms - dias * MS_POR_DIA;
    amostras
        .iter()
        .filter(|a| parse_ts_ms(&a.at) >= corte)
        .cloned()
        .collect()
}

/// `Atual` (`limites.ts:95`): a leitura e quando foi medida (ISO, UTC).
#[derive(Clone, Debug, PartialEq)]
pub struct Atual {
    pub pct: f64,
    pub resets_at: f64,
    pub at: String,
}

impl Atual {
    /// `{pct, resetsAt, at}`, a ordem do `{...leitura, at}` do TS.
    pub fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("pct".into(), num(self.pct));
        map.insert("resetsAt".into(), num(self.resets_at));
        map.insert("at".into(), Value::String(self.at.clone()));
        Value::Object(map)
    }
}

/// `Partial<Record<Janela, Atual>>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Atuais {
    pub five: Option<Atual>,
    pub seven: Option<Atual>,
}

impl Atuais {
    pub fn get(&self, janela: Janela) -> Option<&Atual> {
        match janela {
            Janela::Five => self.five.as_ref(),
            Janela::Seven => self.seven.as_ref(),
        }
    }

    fn set(&mut self, janela: Janela, atual: Atual) {
        match janela {
            Janela::Five => self.five = Some(atual),
            Janela::Seven => self.seven = Some(atual),
        }
    }
}

fn num(x: f64) -> Value {
    Number::from_f64(x).map_or(Value::Null, Value::Number)
}

/// `atuais` (`limites.ts:101`): a última leitura de cada janela ainda aberta (a renovação vem depois de `now`).
pub fn atuais(amostras: &[Amostra], now_epoch_ms: f64) -> Atuais {
    let agora = now_epoch_ms / 1000.0;
    let mut out = Atuais::default();
    for amostra in amostras {
        for janela in JANELAS {
            if let Some(l) = amostra.janela(janela)
                && l.resets_at > agora
            {
                out.set(
                    janela,
                    Atual {
                        pct: l.pct,
                        resets_at: l.resets_at,
                        at: amostra.at.clone(),
                    },
                );
            }
        }
    }
    out
}

/// `HADES_LIMITS_RETENTION_DAYS` de `chaves` (`limites-io.ts:24`, `limites-io.ts:35`): inteiro de 1 a 365; fora da
/// faixa só avisa e usa 30.
pub fn retention_days(env: &Env, cwd: &Path) -> f64 {
    let key = "HADES_LIMITS_RETENTION_DAYS";
    let raw = config_value(env, cwd, key, &RETENCAO_PADRAO.to_string());
    if let Some(value) = js_number(&raw)
        && value.is_finite()
        && value.fract() == 0.0
        && value >= f64::from(RETENCAO_MIN)
        && value <= f64::from(RETENCAO_MAX)
    {
        return value;
    }
    warn(&format!(
        "limites: {key}={raw}: esperava um inteiro de {RETENCAO_MIN} a {RETENCAO_MAX}; uso {RETENCAO_PADRAO}"
    ));
    f64::from(RETENCAO_PADRAO)
}

/// `lerAmostras` (`limites-io.ts:53`): `<hades_home>/limits/samples.jsonl`; sem o arquivo, nenhuma amostra e nenhuma
/// pasta criada.
pub fn ler_amostras(env: &Env) -> Vec<Amostra> {
    let file = hades_home(env).join("limits").join("samples.jsonl");
    amostras_de(&read_text(&file).unwrap_or_default())
}

/// `Number(s)` do JS (ECMA-262 §7.1.4.1.1): vazio e só espaço valem 0; `0x`/`0o`/`0b` e `Infinity` são aceitos;
/// `inf`, `nan` e `_` do Rust não. `None` é `NaN`.
fn js_number(text: &str) -> Option<f64> {
    let s = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if s.is_empty() {
        return Some(0.0);
    }
    let radix = match s.get(..2) {
        Some("0x" | "0X") => Some(16),
        Some("0o" | "0O") => Some(8),
        Some("0b" | "0B") => Some(2),
        _ => None,
    };
    if let Some(radix) = radix {
        let digits = &s[2..];
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return None;
        }
        return Some(digits.chars().fold(0.0, |acc, c| {
            acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
        }));
    }
    match s {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let numeric = body
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && body
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_digit() || b == b'.');
    if !numeric {
        return None;
    }
    s.parse::<f64>().ok()
}

fn civil_days(y: i64, m: i64, d: i64) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(s: &str, len: usize) -> Option<i64> {
    if s.len() == len && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

/// `Date.parse` dos horários ISO das amostras, em ms; `NaN` quando não é data. Sem fuso vale UTC.
fn parse_ts_ms(ts: &str) -> f64 {
    parse_ts(ts).map_or(f64::NAN, |ms| ms as f64)
}

fn parse_ts(ts: &str) -> Option<i64> {
    let (date, time) = match ts.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (ts, None),
    };
    let mut parts = date.split('-');
    let (y, m, d) = (
        digits(parts.next()?, 4)?,
        digits(parts.next()?, 2)?,
        digits(parts.next()?, 2)?,
    );
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = civil_days(y, m, d) * 86_400_000;
    let Some(time) = time else {
        return Some(ms);
    };
    let (clock, offset_min) = if let Some(c) = time.strip_suffix('Z') {
        (c, 0)
    } else if let Some(i) = time.rfind(['+', '-']) {
        let (c, off) = time.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let (oh, om) = off[1..].split_once(':')?;
        (c, sign * (digits(oh, 2)? * 60 + digits(om, 2)?))
    } else {
        (time, 0)
    };
    let (hms, frac) = match clock.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (clock, None),
    };
    let mut t = hms.split(':');
    let hh = digits(t.next()?, 2)?;
    let mm = digits(t.next()?, 2)?;
    let ss = t.next().map_or(Some(0), |s| digits(s, 2))?;
    if t.next().is_some() || hh > 24 || mm > 59 || ss > 59 {
        return None;
    }
    ms += (hh * 3600 + mm * 60 + ss) * 1000;
    if let Some(f) = frac {
        if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let padded = format!("{:0<3}", &f[..f.len().min(3)]);
        ms += padded.parse::<i64>().ok()?;
    }
    Some(ms - offset_min * 60_000)
}

#[cfg(test)]
mod tests;
