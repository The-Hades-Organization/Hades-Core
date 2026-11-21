# hades-parity

Roda a mesma matriz de casos no `hades <ferramenta>` em TS (Bun, da skill) e no binário Rust. Cada lado ganha um HOME temporário próprio, o ambiente é fechado e a saída normalizada é comparada em bytes.

```
hades-parity run --cases parity/cases.toml --skill <Hades-Skill> --rust-bin <binário> [--filter <substring do id>] [--keep]
```

## `cases.toml`

Cada `[[case]]` aceita (campo desconhecido é erro):

| Campo | Tipo | Efeito |
|---|---|---|
| `id` | texto | Nome do caso; `--filter` casa por substring. |
| `argv` | lista | Argumentos dos dois lados. Aceita `{skill}` e `{cases_dir}`. |
| `fixture` | texto | Pasta de `<pasta do cases.toml>/fixtures/` copiada para o HOME de cada lado. Ausente: HOME vazio. |
| `cwd` | texto | Diretório de execução, relativo ao HOME. `repo` ganha `git init` com remote `https://github.com/acme/demo.git`. |
| `env` | tabela | Variáveis do caso; entram por último e vencem as do sandbox. |
| `stdin` | texto | Entrada padrão. |
| `compare` | lista | `stdout`, `stderr`, `exit`, `json` (stdout por valor) e `files:<glob>` (relativo ao HOME). Padrão: `stdout` e `exit`. |
| `pending` | texto | Motivo. O caso roda e reporta, mas não falha a execução. |
| `ts_cmd` | lista | Entrada do lado TS no lugar de `<skill>/src/cli.ts`. |
| `setup` | texto | Script rodado nos dois lados antes do caso. |
| `setup_ts`, `setup_rust` | texto | Script só de um lado; roda depois do `setup`. |
| `normalize_dates` | bool | Troca `YYYY-MM-DD` por `<date>` na saída e nos `files:`. |

## Template da fixture

Em nome de arquivo, nome de pasta e conteúdo UTF-8 (binário passa intacto):

- `@HOME@`: o HOME do lado que está sendo montado.
- `@ENC:<caminho>@`: o caminho com todo caractere fora de `[A-Za-z0-9]` trocado por `-`, como a pasta de transcripts do Claude Code (`encodeCwd`, `moirai.ts:57`). O caminho pode conter `@HOME@`. Nome de pasta não aceita `/`, e como a codificação já troca `/` por `-`, escreva `|` no lugar: a pasta `.claude/projects/@ENC:@HOME@|wt|u-um@/` vira `.claude/projects/-tmp-hades-parity-x-ts-home-wt-u-um/`.

## `setup`

O caminho é relativo à pasta do `cases.toml`. O script roda com `sh`, no HOME do lado, depois da fixture e do `cwd`, com este ambiente e nenhum outro:

`HOME`, `SIDE` (`ts` ou `rust`), `TZ=UTC`, `PATH=/usr/bin:/bin`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`.

Saída diferente de zero aborta o caso com a stderr do script. Para um repo com commits em datas fixas, o script exporta `GIT_AUTHOR_DATE` e `GIT_COMMITTER_DATE` antes de cada `git commit`.

## Normalização

Sempre: HOME vira `~`, a skill vira `<skill>`, instante ISO vira `<ts>`, epoch em campo conhecido, UUID e hex de 12+ com letra viram `<id>`/`<n>`, duração vira `<dur>`. Com `normalize_dates`, a data `YYYY-MM-DD` vira `<date>`.
