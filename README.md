# Hades-Core

Contratos em disco compartilhados pelas ferramentas do Hades (work root, diário, registro de agentes, perfil, formato JSON).

## Crates

- `crates/hades-core`: lib sem gpui (work root, diário, registro de agentes, perfil, formato `--json`).
  - `jsonfmt`: `stringify`/`json_compact` (`JSON.stringify` compacto), `json_indent` e `py_dumps`.
  - `jsfmt`: `to_fixed` (`Number#toFixed`) e `to_js_string` (`Number#toString`).
  - `limits`: limites de uso só leitura (`amostras_de`, `ler_amostras`, `dentro_da_retencao`, `atuais`, `retention_days`); a gravação fica no Moirai.
- `crates/hades-parity`: runner de paridade TS × Rust; casos comuns, `keep = ["iso"]` e roundtrip `kind = "cross"`. Veja `crates/hades-parity/README.md`.

## Estado

F0, em construção. Só o esqueleto do workspace existe; nada da ferramenta foi portado ainda.

## Erros no stderr

`hades_core::errors` renderiza o bloco do catálogo `human-errors` do TS (`Por quê`, `O que fazer`, `Saiba mais`, `(código: …)`). O catálogo vai embutido no binário (`crates/hades-core/data/human-errors.json`); nada é lido da skill em runtime.

Uma ferramenta, no ponto em que hoje imprime só a primeira linha do erro:

```rust
use hades_core::errors::{render_cli, render_error};

// mensagem + código explícito (ou None para o código de classe de saída)
eprintln!("{}", render_cli("sem permissão", Some("cli_unknown_command"), "caronte"));
// HadesError já carrega mensagem e código
eprintln!("{}", render_error(&err, "caronte"));
```

`prefix` é o nome da ferramenta. Para regerar o catálogo depois de mudar o TS: `bun scripts/gen-human-errors.ts` e republicar.

## Padrões de configuração

Sem `config.tsv` no `$D`, `config::config_value` lê os padrões de `<skill>/kit/config.tsv`; `plugin/config.tsv` e `config.env` do `$D` valem por cima.

## Relação com o Hades-Skill

Faz parte da reescrita em Rust das ferramentas do [Hades-Skill](https://github.com/The-Hades-Organization/Hades-Skill). O Hades-Skill continua sendo a fonte até o corte, quando passa a chamar o binário.

## Licença

MIT. Veja `LICENSE`.
