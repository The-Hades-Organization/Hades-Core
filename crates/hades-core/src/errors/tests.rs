use super::*;
use crate::output::HadesError;

/// Saída real do `renderCli` do TS (`hades: subcomando desconhecido: foo.`, `cli_unknown_command`).
const TS_UNKNOWN: &str = "hades: subcomando desconhecido: foo.\n  Por quê:      O nome depois de `hades` não bate com nenhum subcomando. Em geral é erro de digitação ou um comando de uma versão diferente da skill.\n  O que fazer:  Veja a lista com `hades --help`. Se a skill está desatualizada, rode `hades version-check status`.\n  Saiba mais:   reference/cli.md §install\n  (código: cli_unknown_command)";

/// Saída real do TS com prefixo `caronte`.
const TS_USAGE_CARONTE: &str = "caronte: recusado\n  Por quê:      O comando foi chamado sem algo que ele precisa (um nome, um número, uma opção) ou com algo que ele não conhece.\n  O que fazer:  Confira o uso do comando em `hades --help` e, para o pedido em linguagem comum, em `reference/pedidos.md`.\n  Saiba mais:   reference/pedidos.md §Pedidos típicos do dono\n  (código: cli_usage)";

/// Saída real do TS para código fora do catálogo.
const TS_FALLBACK: &str = "hades: algo\n  Por quê:      A Hades registrou a falha com o código `codigo_que_nao_existe`, mas ninguém escreveu ainda o que ele significa. A mensagem original (acima) é o que se sabe.\n  O que fazer:  Rode `hades doctor` para ver se falta algo na máquina e `hades explain <código>` para procurar outro erro parecido.\n  Saiba mais:   reference/erros.md §Como acrescentar uma entrada\n  (código: codigo_que_nao_existe)";

#[test]
fn render_cli_igual_ao_ts() {
    assert_eq!(
        render_cli(
            "subcomando desconhecido: foo.",
            Some(cli_codes::UNKNOWN_COMMAND),
            "hades"
        ),
        TS_UNKNOWN
    );
    assert_eq!(
        render_cli("recusado", Some("cli_usage"), "caronte"),
        TS_USAGE_CARONTE
    );
    assert_eq!(
        render_cli("algo", Some("codigo_que_nao_existe"), "hades"),
        TS_FALLBACK
    );
}

#[test]
fn sem_codigo_vale_cli_failed() {
    let out = render_cli("falhou", None, "hades");
    assert!(out.ends_with("(código: cli_failed)"));
    assert!(explain(None).known);
}

#[test]
fn catalogo_embutido_tem_os_codigos_da_cli() {
    for code in [
        "cli_unknown_command",
        "cli_usage",
        "cli_failed",
        "cli_no_profile",
        "cli_outside_repo",
        "cli_missing_dependency",
        "cli_unexpected",
        "gate_not_admitted",
    ] {
        assert!(explain(Some(code)).known, "{code}");
    }
    let codes = known_codes();
    assert!(codes.len() > 100);
    assert!(codes.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn cli_code_of_exit_segue_o_ts() {
    assert_eq!(cli_code_of_exit(2), "cli_usage");
    assert_eq!(cli_code_of_exit(3), "cli_no_profile");
    assert_eq!(cli_code_of_exit(4), "cli_outside_repo");
    assert_eq!(cli_code_of_exit(69), "cli_missing_dependency");
    assert_eq!(cli_code_of_exit(75), "gate_not_admitted");
    assert_eq!(cli_code_of_exit(1), "cli_failed");
    assert_eq!(cli_code_of_exit(141), "cli_failed");
}

#[test]
fn render_error_usa_o_codigo_proprio_ou_o_da_saida() {
    let e = HadesError::with_code("faltou", 2);
    assert!(render_error(&e, "hades").ends_with("(código: cli_usage)"));
    let mut e = HadesError::with_code("sem perfil", 3);
    e.error_code = Some("cli_outside_repo".into());
    assert!(render_error(&e, "hades").ends_with("(código: cli_outside_repo)"));
}

#[test]
fn message_of_cli_failure_segue_o_ts() {
    assert_eq!(
        message_of_cli_failure(&format!("{TS_UNKNOWN}\n")),
        "hades: subcomando desconhecido: foo."
    );
    assert_eq!(message_of_cli_failure("a\nb\n"), "b");
    assert_eq!(message_of_cli_failure("\n \n"), "");
    assert_eq!(message_of_cli_failure("x\n  (código: )\n"), "  (código: )");
}

#[test]
fn render_hook_e_entry_tem_o_formato_do_ts() {
    let hook = render_hook("barrado", "cli_usage", Some("faça isto"));
    assert!(hook.starts_with("barrado\n  Por quê:                "));
    assert!(hook.contains("\n  O que fazer no lugar:   faça isto\n"));
    assert!(hook.ends_with("  (código: cli_usage)\n"));
    let entry = render_entry("cli_usage");
    assert!(
        entry.starts_with("Faltou ou sobrou argumento no comando\n\n  Código:       cli_usage\n")
    );
}

#[test]
fn suggest_name_segue_o_ts() {
    let names = ["status", "start", "gate-lock"];
    assert_eq!(suggest_name("statuz", &names), Some("status"));
    assert_eq!(suggest_name("STATUS", &names), Some("status"));
    assert_eq!(suggest_name("zzzzzz", &names), None);
    assert_eq!(suggest_name("x", &["a", "xy"]), Some("a"));
    assert_eq!(suggest_name("x", &["xyz"]), None);
}
