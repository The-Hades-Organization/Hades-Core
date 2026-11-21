//! `Number::prototype.toString` do ECMAScript para `f64` (ECMA-262 §6.1.6.1.20).

/// Texto que o JS imprime para um número finito. `-0` sai `0`, como no `JSON.stringify`.
///
/// Os dígitos são os mais curtos que fazem ida e volta (`{:e}` do Rust), os mesmos do V8.
pub(crate) fn es_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let sci = format!("{:e}", value.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    let sign = if value < 0.0 { "-" } else { "" };
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let esign = if e < 0 { '-' } else { '+' };
        let head = &digits[..1];
        let tail = &digits[1..];
        let frac = if tail.is_empty() {
            String::new()
        } else {
            format!(".{tail}")
        };
        format!("{head}{frac}e{esign}{}", e.abs())
    };
    format!("{sign}{body}")
}
