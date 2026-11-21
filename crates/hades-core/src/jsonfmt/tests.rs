use super::*;
use serde_json::{Value, json};

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn py_dumps_mistura_float_e_int_como_o_ts() {
    let v = parse(r#"{"a": 3.0, "b": 3, "c": 0.5}"#);
    assert_eq!(py_dumps(&v, None), r#"{"a": 3.0, "b": 3, "c": 0.5}"#);
}

#[test]
fn py_dumps_vazios_e_aninhados() {
    assert_eq!(py_dumps(&json!([]), None), "[]");
    assert_eq!(py_dumps(&json!({}), Some(2)), "{}");
    assert_eq!(
        py_dumps(&json!({"a": [1, 2], "b": {}}), None),
        r#"{"a": [1, 2], "b": {}}"#
    );
    assert_eq!(
        py_dumps(&json!({"a": [1, {"b": null}]}), Some(2)),
        "{\n  \"a\": [\n    1,\n    {\n      \"b\": null\n    }\n  ]\n}"
    );
}

#[test]
fn py_dumps_indent_zero_quebra_linha_sem_recuo() {
    assert_eq!(py_dumps(&json!([1, 2]), Some(0)), "[\n1,\n2\n]");
}

#[test]
fn py_float_expoente_e_zero_negativo() {
    let big = Value::Number(serde_json::Number::from_f64(1e21).unwrap());
    assert_eq!(py_dumps(&big, None), "1e+21");
    let neg0 = Value::Number(serde_json::Number::from_f64(-0.0).unwrap());
    assert_eq!(py_dumps(&neg0, None), "0.0");
    let f = Value::Number(serde_json::Number::from_f64(1e16).unwrap());
    assert_eq!(py_dumps(&f, None), "10000000000000000.0");
    let small = Value::Number(serde_json::Number::from_f64(1e-7).unwrap());
    assert_eq!(py_dumps(&small, None), "1e-7");
}

#[test]
fn py_dumps_int_exato() {
    assert_eq!(
        py_dumps(&json!(9007199254740993_i64), None),
        "9007199254740993"
    );
    assert_eq!(py_dumps(&json!(u64::MAX), None), "18446744073709551615");
    assert_eq!(py_dumps(&json!(-5), None), "-5");
}

#[test]
fn js_numeros() {
    let cases: &[(f64, &str)] = &[
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (-1.5, "-1.5"),
        (0.1, "0.1"),
        (0.000001, "0.000001"),
        (1e-7, "1e-7"),
        (1.5e-7, "1.5e-7"),
        (123456789012345680000.0, "123456789012345680000"),
        (1e21, "1e+21"),
        (1.2345e25, "1.2345e+25"),
        (9007199254740992.0, "9007199254740992"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (100.0, "100"),
        (0.30000000000000004, "0.30000000000000004"),
    ];
    for (n, want) in cases {
        let v = Value::Number(serde_json::Number::from_f64(*n).unwrap());
        assert_eq!(json_compact(&v), *want, "{n}");
    }
}

#[test]
fn js_inteiro_grande_passa_por_f64() {
    assert_eq!(
        json_compact(&json!(9007199254740993_i64)),
        "9007199254740992"
    );
    assert_eq!(json_compact(&json!(u64::MAX)), "18446744073709552000");
    assert_eq!(json_compact(&json!(i64::MIN)), "-9223372036854776000");
}

#[test]
fn compact_e_indent() {
    let v = json!({"a": [1, "x"], "b": {"c": null}, "e": [], "f": {}});
    assert_eq!(
        json_compact(&v),
        r#"{"a":[1,"x"],"b":{"c":null},"e":[],"f":{}}"#
    );
    assert_eq!(
        json_indent(&v, 2),
        "{\n  \"a\": [\n    1,\n    \"x\"\n  ],\n  \"b\": {\n    \"c\": null\n  },\n  \"e\": [],\n  \"f\": {}\n}"
    );
    assert_eq!(json_indent(&v, 0), json_compact(&v));
    assert_eq!(json_indent(&json!([1]), 99), "[\n          1\n]");
}

#[test]
fn ordem_de_chaves_preservada() {
    let v = parse(r#"{"z":1,"a":2,"m":{"y":1,"b":2}}"#);
    assert_eq!(json_compact(&v), r#"{"z":1,"a":2,"m":{"y":1,"b":2}}"#);
}

#[test]
fn strings_escapes() {
    let v = json!("a\"b\\c/d\n\r\t\u{8}\u{c}\u{1}\u{1f}\u{7f}\u{2028}é😀");
    assert_eq!(
        json_compact(&v),
        "\"a\\\"b\\\\c/d\\n\\r\\t\\b\\f\\u0001\\u001f\u{7f}\u{2028}é😀\""
    );
    assert_eq!(py_dumps(&v, None), json_compact(&v));
}

#[test]
fn ensure_ascii() {
    assert_eq!(py_dumps_ascii(&json!("ação"), None), r#""a\u00e7\u00e3o""#);
    assert_eq!(py_dumps_ascii(&json!("😀"), None), r#""\ud83d\ude00""#);
    assert_eq!(
        py_dumps_ascii(&json!("a/b\u{7f}\u{2028}"), None),
        r#""a/b\u007f\u2028""#
    );
    assert_eq!(
        py_dumps_ascii(&json!({"chavé": 1.0}), None),
        r#"{"chav\u00e9": 1.0}"#
    );
    assert_eq!(py_dumps_ascii(&json!("\n\u{1}"), None), r#""\n\u0001""#);
}

#[test]
fn goldens_cerberus_ida_e_volta_byte_a_byte() {
    for (name, raw) in [
        (
            "porta",
            include_str!("../../tests/fixtures/cerberus-porta-legado.json"),
        ),
        (
            "limiar",
            include_str!("../../tests/fixtures/cerberus-limiar-legado.json"),
        ),
        (
            "vigilia",
            include_str!("../../tests/fixtures/cerberus-vigilia-legado.json"),
        ),
    ] {
        let v = parse(raw);
        let want = raw.trim_end_matches('\n');
        assert_eq!(json_indent(&v, 2), want, "{name}: json_indent(2)");
        let compact = json_compact(&v);
        assert_eq!(parse(&compact), v, "{name}: compact preserva o valor");
    }
}

#[test]
fn goldens_py_dumps_com_indent_igual_ao_do_arquivo() {
    for raw in [
        include_str!("../../tests/fixtures/cerberus-porta-legado.json"),
        include_str!("../../tests/fixtures/cerberus-limiar-legado.json"),
        include_str!("../../tests/fixtures/cerberus-vigilia-legado.json"),
    ] {
        let v = parse(raw);
        assert_eq!(py_dumps(&v, Some(2)), raw.trim_end_matches('\n'));
    }
}
