//! Formatação numérica com o resultado do JS. O Rust arredonda empate exato para par; o `toFixed` escolhe o maior
//! `n` quando dois servem, então `0.5.toFixed(0)` é `"1"` e `2.5.toFixed(0)` é `"3"`.

/// Casas do `Number.prototype.toFixed`: o exato de um `f64` tem no máximo 1074 casas depois da vírgula.
const EXACT_DIGITS: usize = 1100;

/// `Number.prototype.toFixed(digits)` (ECMA-262 §21.1.3.3): `digits` limitado a 0..=100, `NaN`/`Infinity` como no JS,
/// `|x| >= 1e21` pela conversão padrão, `-0` sem sinal e negativo que arredonda a zero com sinal (`"-0.00"`).
pub fn to_fixed(x: f64, digits: usize) -> String {
    let digits = digits.min(100);
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_owned();
    }
    let negative = x < 0.0;
    let abs = x.abs();
    if abs >= 1e21 {
        let text = number_to_string(abs);
        return if negative { format!("-{text}") } else { text };
    }
    let exact = format!("{abs:.EXACT_DIGITS$}");
    let (int_part, frac_part) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut digits_vec: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes().take(digits))
        .collect();
    let round_up = frac_part.as_bytes().get(digits).is_some_and(|d| *d >= b'5');
    if round_up {
        let mut i = digits_vec.len();
        loop {
            if i == 0 {
                digits_vec.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits_vec[i] == b'9' {
                digits_vec[i] = b'0';
            } else {
                digits_vec[i] += 1;
                break;
            }
        }
    }
    let int_len = digits_vec.len() - digits;
    let int_text = String::from_utf8_lossy(&digits_vec[..int_len]).into_owned();
    let int_text = int_text.trim_start_matches('0');
    let int_text = if int_text.is_empty() { "0" } else { int_text };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(int_text);
    if digits > 0 {
        out.push('.');
        out.push_str(&String::from_utf8_lossy(&digits_vec[int_len..]));
    }
    out
}

/// `Number::toString` do JS para `x >= 1e21` finito e positivo: menor representação que volta ao mesmo número, com
/// expoente `e+N`.
fn number_to_string(x: f64) -> String {
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let sign = if exp < 0 { '-' } else { '+' };
    let head = &digits[..1];
    let tail = &digits[1..];
    if tail.is_empty() {
        format!("{head}e{sign}{}", exp.abs())
    } else {
        format!("{head}.{tail}e{sign}{}", exp.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empates_arredondam_para_cima_como_o_js() {
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(1.5, 0), "2");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(1.125, 2), "1.13");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(0.375, 2), "0.38");
    }

    #[test]
    fn nao_empates_seguem_o_valor_exato() {
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(1.45, 1), "1.4");
        assert_eq!(to_fixed(8.345, 2), "8.35");
        assert_eq!(to_fixed(10.235, 2), "10.23");
        assert_eq!(to_fixed(0.1 + 0.2, 2), "0.30");
    }

    #[test]
    fn propaga_o_vai_um() {
        assert_eq!(to_fixed(9.995, 2), "9.99");
        assert_eq!(to_fixed(9.5, 0), "10");
        assert_eq!(to_fixed(99.5, 0), "100");
        assert_eq!(to_fixed(0.9999, 2), "1.00");
    }

    #[test]
    fn zero_e_sinal() {
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.0001, 2), "-0.00");
        assert_eq!(to_fixed(-0.5, 0), "-1");
        assert_eq!(to_fixed(0.4, 0), "0");
    }

    #[test]
    fn nao_finitos_e_grandes() {
        assert_eq!(to_fixed(f64::NAN, 2), "NaN");
        assert_eq!(to_fixed(f64::INFINITY, 2), "Infinity");
        assert_eq!(to_fixed(f64::NEG_INFINITY, 2), "-Infinity");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
        assert_eq!(to_fixed(1.5e25, 1), "1.5e+25");
        assert_eq!(to_fixed(-1e21, 2), "-1e+21");
        assert_eq!(
            to_fixed(123456789012345680000.0, 2),
            "123456789012345683968.00"
        );
    }

    #[test]
    fn muitas_casas_e_inteiros() {
        assert_eq!(to_fixed(1.0, 0), "1");
        assert_eq!(to_fixed(0.1, 20), "0.10000000000000000555");
        assert_eq!(to_fixed(5e-324, 2), "0.00");
        assert_eq!(to_fixed(1.0, 100).len(), 102);
    }
}
