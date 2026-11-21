//! Relógio injetável: `HADES_NOW` fixa o "agora" das janelas de dias e dos carimbos, para as fixtures de paridade não
//! envelhecerem. Sem a variável (ou vazia) vale o relógio do sistema; valor que não é data ISO é erro.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::env::Env;

/// `HADES_NOW` presente e não vazio que não é data ISO 8601.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockError(pub String);

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HADES_NOW={}: esperava data ISO 8601", self.0)
    }
}

impl std::error::Error for ClockError {}

impl From<ClockError> for std::io::Error {
    fn from(e: ClockError) -> Self {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, e)
    }
}

/// `clockNow` (`src/lib/clock.ts:2`): milissegundos desde a época. `HADES_NOW` (`YYYY-MM-DD` ou
/// `YYYY-MM-DDTHH:MM:SS[.mmm]Z`) ausente ou vazio usa o relógio do sistema; inválido é erro, como o `throw` do TS.
pub fn now_ms(env: &Env) -> Result<i64, ClockError> {
    match env.get("HADES_NOW") {
        None | Some("") => Ok(system_ms()),
        Some(raw) => parse_iso_ms(raw).ok_or_else(|| ClockError(raw.to_string())),
    }
}

/// Segundos desde a época, truncando os ms como o `isoNow`.
pub fn now_secs(env: &Env) -> Result<i64, ClockError> {
    now_ms(env).map(|ms| ms.div_euclid(1000))
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
        assert_eq!(now_ms(&env), Ok(1_795_176_000_000));
        assert_eq!(now_secs(&env), Ok(1_795_176_000));
    }

    #[test]
    fn aceita_ms_e_so_data() {
        let env = Env::empty().with("HADES_NOW", "1970-01-02T00:00:00.5Z");
        assert_eq!(now_ms(&env), Ok(86_400_500));
        let env = Env::empty().with("HADES_NOW", "2000-02-29");
        assert_eq!(now_ms(&env), Ok(951_782_400_000));
    }

    #[test]
    fn sem_variavel_ou_vazia_usa_o_sistema() {
        for env in [Env::empty(), Env::empty().with("HADES_NOW", "")] {
            assert!(now_ms(&env).unwrap() > 1_700_000_000_000);
        }
    }

    #[test]
    fn invalida_e_erro_com_a_mensagem_do_ts() {
        for v in ["ontem", "2026-13-01", "2026-11-20T12:00:00"] {
            let err = now_ms(&Env::empty().with("HADES_NOW", v)).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("HADES_NOW={v}: esperava data ISO 8601")
            );
        }
    }
}
