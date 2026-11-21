//! Instantes em milissegundos desde a época Unix, sem dependência de calendário: `Date.toISOString`, `Date.parse`
//! (a parte ISO 8601) e a leitura de `--since` (`src/lib/journal.ts:498`).

use std::time::{SystemTime, UNIX_EPOCH};

const DAY_MS: i64 = 86_400_000;

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Dias desde 1970-01-01 de uma data do calendário gregoriano proléptico.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `new Date(ms).toISOString()`: `2026-03-10T12:00:00.000Z`.
pub fn iso_ms(ms: i64) -> String {
    let days = ms.div_euclid(DAY_MS);
    let rest = ms.rem_euclid(DAY_MS);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn digits(&mut self, n: usize) -> Option<i64> {
        let slice = self.bytes.get(self.at..self.at + n)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at += n;
        Some(
            slice
                .iter()
                .fold(0, |acc, b| acc * 10 + i64::from(b - b'0')),
        )
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            return true;
        }
        false
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// `Date.parse` para ISO 8601: `AAAA-MM-DD`, com `THH:mm[:ss[.f+]]` e fuso `Z` ou `±HH:mm`/`±HHmm`. Data sem hora
/// vale UTC (como no JS). Hora sem fuso vale UTC aqui; o JS a lê no fuso local, e o diário só grava `Z`.
pub fn parse_iso(text: &str) -> Option<i64> {
    let mut c = Cursor {
        bytes: text.as_bytes(),
        at: 0,
    };
    let y = c.digits(4)?;
    let (mut mo, mut d) = (1, 1);
    if c.eat(b'-') {
        mo = c.digits(2)?;
        if c.eat(b'-') {
            d = c.digits(2)?;
        }
    }
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) {
        return None;
    }
    let (mut h, mut mi, mut s, mut ms, mut offset) = (0, 0, 0, 0, 0);
    if c.eat(b'T') || c.eat(b' ') {
        h = c.digits(2)?;
        if !c.eat(b':') {
            return None;
        }
        mi = c.digits(2)?;
        if c.eat(b':') {
            s = c.digits(2)?;
            if c.eat(b'.') {
                let start = c.at;
                while c.bytes.get(c.at).is_some_and(u8::is_ascii_digit) {
                    c.at += 1;
                }
                if c.at == start {
                    return None;
                }
                let mut frac: String = text[start..c.at].chars().take(3).collect();
                while frac.len() < 3 {
                    frac.push('0');
                }
                ms = frac.parse().ok()?;
            }
        }
        if c.eat(b'Z') || c.eat(b'z') {
        } else if let Some(sign) = [b'+', b'-'].into_iter().find(|b| c.eat(*b)) {
            let oh = c.digits(2)?;
            c.eat(b':');
            let om = c.digits(2)?;
            if oh > 23 || om > 59 {
                return None;
            }
            offset = (oh * 60 + om) * if sign == b'+' { 1 } else { -1 };
        }
    }
    if !c.done() || h > 24 || mi > 59 || s > 59 || (h == 24 && (mi, s, ms) != (0, 0, 0)) {
        return None;
    }
    let day = days_from_civil(y, mo, d);
    Some(day * DAY_MS + h * 3_600_000 + mi * 60_000 + s * 1000 + ms - offset * 60_000)
}

/// Espaço em branco do `\s` e do `trim` do JS (inclui NBSP e BOM; não inclui U+0085).
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `--since`: duração (`30m`, `2h`, `1d`, `45s`) ou instante ISO com fuso; `None` se não entende (`src/lib/journal.ts:499`).
pub fn parse_since(text: &str, now_ms: i64) -> Option<i64> {
    let t = text.trim_matches(is_js_space);
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let rest = t[digits..].trim_start_matches(is_js_space);
        let unit = match rest {
            "s" => Some(1000.0),
            "m" => Some(60_000.0),
            "h" => Some(3_600_000.0),
            "d" => Some(86_400_000.0),
            _ => None,
        };
        if let Some(unit) = unit {
            let n: f64 = t[..digits].parse().ok()?;
            return Some(now_ms - (n * unit) as i64);
        }
    }
    if !has_zone(t) {
        return None;
    }
    parse_iso(t)
}

/// `/(Z|[+-]\d{2}:?\d{2})$/`.
fn has_zone(t: &str) -> bool {
    if t.ends_with('Z') {
        return true;
    }
    let b = t.as_bytes();
    let tail = |n: usize| b.len() >= n && b[b.len() - n..].iter().all(u8::is_ascii_digit);
    let signed = |at: usize| b.len() > at && matches!(b[b.len() - at - 1], b'+' | b'-');
    (b.len() >= 5 && tail(4) && signed(4))
        || (b.len() >= 6 && b[b.len() - 3] == b':' && tail(2) && {
            let hh = &b[b.len() - 5..b.len() - 3];
            hh.iter().all(u8::is_ascii_digit) && signed(5)
        })
}
