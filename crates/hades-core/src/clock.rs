//! Relógio injetável: `HADES_NOW` fixa o "agora" das janelas de dias e dos carimbos, para as fixtures de paridade não
//! envelhecerem. Sem a variável (ou com valor que não é data ISO) vale o relógio do sistema.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::env::Env;

/// Milissegundos desde a época: `HADES_NOW` (`YYYY-MM-DD` ou `YYYY-MM-DDTHH:MM:SS[.mmm]Z`) ou o relógio do sistema.
pub fn now_ms(env: &Env) -> i64 {
    env.get("HADES_NOW")
        .and_then(parse_iso_ms)
        .unwrap_or_else(system_ms)
}

/// Segundos desde a época, truncando os ms como o `isoNow`.
pub fn now_secs(env: &Env) -> i64 {
    now_ms(env).div_euclid(1000)
}

fn system_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Dias desde 1970-01-01 de uma data civil do calendário gregoriano (inverso de `iso_from_epoch`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn num(s: &str, len: usize) -> Option<i64> {
    if s.len() != len || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn parse_iso_ms(text: &str) -> Option<i64> {
    let (date, time) = match text.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let (y, m, d) = (
        num(parts.next()?, 4)?,
        num(parts.next()?, 2)?,
        num(parts.next()?, 2)?,
    );
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, m, d) * 86_400_000;
    if let Some(time) = time {
        let time = time.strip_suffix('Z')?;
        let (hms, frac) = match time.split_once('.') {
            Some((h, f)) => (h, Some(f)),
            None => (time, None),
        };
        let mut t = hms.split(':');
        let (hh, mm, ss) = (num(t.next()?, 2)?, num(t.next()?, 2)?, num(t.next()?, 2)?);
        if t.next().is_some() || hh > 23 || mm > 59 || ss > 59 {
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
    }
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hades_now_fixa_o_relogio() {
        let env = Env::empty().with("HADES_NOW", "2026-11-20T12:00:00Z");
        assert_eq!(now_ms(&env), 1_795_176_000_000);
        assert_eq!(now_secs(&env), 1_795_176_000);
    }

    #[test]
    fn aceita_ms_e_so_data() {
        let env = Env::empty().with("HADES_NOW", "1970-01-02T00:00:00.5Z");
        assert_eq!(now_ms(&env), 86_400_500);
        let env = Env::empty().with("HADES_NOW", "2000-02-29");
        assert_eq!(now_ms(&env), 951_782_400_000);
    }

    #[test]
    fn sem_variavel_ou_invalida_usa_o_sistema() {
        for v in [
            None,
            Some(""),
            Some("ontem"),
            Some("2026-13-01"),
            Some("2026-11-20T12:00:00"),
        ] {
            let env = match v {
                Some(v) => Env::empty().with("HADES_NOW", v),
                None => Env::empty(),
            };
            assert!(now_ms(&env) > 1_700_000_000_000, "{v:?}");
        }
    }
}
