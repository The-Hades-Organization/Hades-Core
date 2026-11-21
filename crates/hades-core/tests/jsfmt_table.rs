//! Tabela de referência gerada no bun (`x.toFixed(d)`): `valor<TAB>digitos<TAB>esperado`.

use hades_core::jsfmt::to_fixed;

fn parse(text: &str) -> f64 {
    match text {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        other => other.parse().expect("número da tabela"),
    }
}

#[test]
fn bate_com_a_tabela_do_js() {
    let table = include_str!("data/to_fixed_js.tsv");
    let mut rows = 0;
    for line in table.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let (x, d, want) = (parse(cols[0]), cols[1].parse().unwrap(), cols[2]);
        assert_eq!(to_fixed(x, d), want, "{}.toFixed({d})", cols[0]);
        rows += 1;
    }
    assert_eq!(rows, 144);
}
