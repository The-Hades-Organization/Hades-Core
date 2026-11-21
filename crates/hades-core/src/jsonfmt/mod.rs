//! Serialização de JSON byte a byte igual à do TS (`JSON.stringify`) e à do Python (`json.dumps`).
//!
//! A ordem das chaves é a do `Value` (`serde_json` com `preserve_order`).
//!
//! Números, o que não tem equivalente:
//! - `json_compact`/`json_indent` seguem o JS, em que todo número é `f64`: inteiros de `i64`/`u64` passam por `f64`
//!   (acima de 2^53 saem arredondados, como o JS imprimiria), `-0` sai `0` e expoente usa o formato do ECMAScript
//!   (`1e+21`, `1e-7`). `NaN` e infinito não existem em `serde_json::Number`.
//! - `py_dumps` imprime `i64`/`u64` exatos (int do Python) e trata todo `f64` como o `PyFloat` do TS: valor inteiro
//!   ganha `.0` (`3.0`), salvo se o texto tem expoente. Diferenças do TS para o Python de verdade, mantidas por paridade
//!   com o TS: `-0.0` sai `0.0` (Python: `-0.0`) e `1e16` sai `10000000000000000.0` (Python: `1e+16`).
//! - Strings: lone surrogate não existe em `&str`, então o `\udXXX` do `JSON.stringify` bem-formado não ocorre.

mod number;

use number::es_number;
use serde_json::{Number, Value};
use std::fmt::Write;

#[derive(Clone, Copy)]
enum NumberStyle {
    Js,
    Py,
}

struct Style {
    number: NumberStyle,
    ascii: bool,
    key_sep: &'static str,
    item_sep: &'static str,
    indent: Option<usize>,
}

/// `JSON.stringify(v)`: sem espaços.
pub fn json_compact(value: &Value) -> String {
    render(
        value,
        &Style {
            number: NumberStyle::Js,
            ascii: false,
            key_sep: ":",
            item_sep: ",",
            indent: None,
        },
    )
}

/// `JSON.stringify(v, null, n)`: `n` acima de 10 vale 10 e `0` equivale a `json_compact`.
pub fn json_indent(value: &Value, n: usize) -> String {
    let n = n.min(10);
    if n == 0 {
        return json_compact(value);
    }
    render(
        value,
        &Style {
            number: NumberStyle::Js,
            ascii: false,
            key_sep: ": ",
            item_sep: ",",
            indent: Some(n),
        },
    )
}

/// `pyDumps` de `src/lib/pyjson.ts:29-48`: `json.dumps(v, ensure_ascii=False[, indent])`, separadores `", "` e `": "`.
///
/// Com `indent`, os itens quebram em `,\n`; `Some(0)` quebra linha sem recuo (como o TS e o Python).
pub fn py_dumps(value: &Value, indent: Option<usize>) -> String {
    render(
        value,
        &Style {
            number: NumberStyle::Py,
            ascii: false,
            key_sep: ": ",
            item_sep: ", ",
            indent,
        },
    )
}

/// `json.dumps` com `ensure_ascii=True` (o padrão do Python): tudo fora de `' '..='~'` vira `\uXXXX`
/// (par substituto acima do BMP), inclusive `0x7f`. O TS não tem esta variante; o resto segue `py_dumps`.
pub fn py_dumps_ascii(value: &Value, indent: Option<usize>) -> String {
    render(
        value,
        &Style {
            number: NumberStyle::Py,
            ascii: true,
            key_sep: ": ",
            item_sep: ", ",
            indent,
        },
    )
}

fn render(value: &Value, style: &Style) -> String {
    let mut out = String::new();
    write_value(&mut out, value, style, 0);
    out
}

fn write_value(out: &mut String, value: &Value, style: &Style, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number_text(n, style.number)),
        Value::String(s) => write_string(out, s, style.ascii),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            write_container(out, '[', ']', items.len(), style, level, |out, i| {
                write_value(out, &items[i], style, level + 1);
            });
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let entries: Vec<(&String, &Value)> = map.iter().collect();
            write_container(out, '{', '}', entries.len(), style, level, |out, i| {
                let (key, item) = entries[i];
                write_string(out, key, style.ascii);
                out.push_str(style.key_sep);
                write_value(out, item, style, level + 1);
            });
        }
    }
}

fn write_container(
    out: &mut String,
    open: char,
    close: char,
    len: usize,
    style: &Style,
    level: usize,
    mut item: impl FnMut(&mut String, usize),
) {
    let pad = style
        .indent
        .map(|n| format!("\n{}", " ".repeat(n * (level + 1))));
    out.push(open);
    if let Some(pad) = &pad {
        out.push_str(pad);
    }
    for i in 0..len {
        if i > 0 {
            match &pad {
                Some(pad) => {
                    out.push(',');
                    out.push_str(pad);
                }
                None => out.push_str(style.item_sep),
            }
        }
        item(out, i);
    }
    if let Some(n) = style.indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * level));
    }
    out.push(close);
}

fn number_text(n: &Number, style: NumberStyle) -> String {
    match style {
        NumberStyle::Js => es_number(n.as_f64().unwrap_or(0.0)),
        NumberStyle::Py => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                py_float_text(n.as_f64().unwrap_or(0.0))
            }
        }
    }
}

/// `pyFloatText` de `src/lib/pyjson.ts:22-26`.
fn py_float_text(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_owned();
    }
    let text = es_number(value);
    if value.fract() == 0.0 && !text.contains('e') {
        format!("{text}.0")
    } else {
        text
    }
}

/// `JSON.stringify` de string; com `ascii`, o `ensure_ascii` do Python.
fn write_string(out: &mut String, text: &str, ascii: bool) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => push_u(out, c as u32),
            c if ascii && !(' '..='~').contains(&c) => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    push_u(out, u32::from(*unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_u(out: &mut String, unit: u32) {
    let _ = write!(out, "\\u{unit:04x}");
}

#[cfg(test)]
mod tests;
