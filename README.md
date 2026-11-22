# Hades-Core

Contratos em disco compartilhados pelas ferramentas do Hades (work root, diário, registro de agentes, perfil, formato JSON).

## Crates

- `crates/hades-core`: lib sem gpui (work root, diário, registro de agentes, perfil, formato `--json`).
  - `jsonfmt`: `stringify`/`json_compact` (`JSON.stringify` compacto), `json_indent` e `py_dumps`.
  - `jsfmt`: `to_fixed` (`Number#toFixed`) e `to_js_string` (`Number#toString`).
- `crates/hades-parity`: runner de paridade TS × Rust; casos comuns, `keep = ["iso"]` e roundtrip `kind = "cross"`. Veja `crates/hades-parity/README.md`.

## Estado

F0, em construção. Só o esqueleto do workspace existe; nada da ferramenta foi portado ainda.

## Relação com o Hades-Skill

Faz parte da reescrita em Rust das ferramentas do [Hades-Skill](https://github.com/The-Hades-Organization/Hades-Skill). O Hades-Skill continua sendo a fonte até o corte, quando passa a chamar o binário.

## Licença

MIT. Veja `LICENSE`.
