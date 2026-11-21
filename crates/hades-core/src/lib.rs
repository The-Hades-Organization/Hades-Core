//! Contratos em disco compartilhados pelas ferramentas do Hades. Sem gpui.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compila() {
        assert_eq!(env!("CARGO_PKG_NAME"), "hades-core");
    }
}
