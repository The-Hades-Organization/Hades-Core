// Gera crates/hades-core/data/human-errors.json a partir do catálogo da skill (src/lib/human-errors-catalog.ts).
// Uso: bun scripts/gen-human-errors.ts [<skill>]   (padrão: HADES_SKILL_DIR ou ~/.claude/skills/hades)
import { writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const skill = process.argv[2] ?? process.env.HADES_SKILL_DIR ?? join(homedir(), ".claude/skills/hades");
const { HUMAN_ERRORS } = await import(`${skill}/src/lib/human-errors-catalog.ts`);
const entries = Object.entries(HUMAN_ERRORS as Record<string, { title: string; cause: string; action: string; ref: string }>).map(([code, e]) => ({
  code,
  title: e.title,
  cause: e.cause,
  action: e.action,
  ref: e.ref,
}));
const out = join(dirname(fileURLToPath(import.meta.url)), "../crates/hades-core/data/human-errors.json");
writeFileSync(out, `${JSON.stringify(entries, null, 1)}\n`);
console.log(`${entries.length} entradas em ${out}`);
