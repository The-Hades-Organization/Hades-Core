// Lado TS do hades-core-probe: importa as funções da skill pelo checkout (HADES_SKILL_DIR, fixado pelo sandbox).
const skill = process.env.HADES_SKILL_DIR;
if (!skill) {
  console.error("HADES_SKILL_DIR ausente");
  process.exit(2);
}
const [cmd, arg] = process.argv.slice(2);
const USAGE = "uso: hades-core-probe <pyjson <json> | slug <url>>";

if (cmd === "pyjson" && arg !== undefined) {
  const { pyDumps } = await import(`${skill}/src/lib/pyjson.ts`);
  try {
    console.log(pyDumps(JSON.parse(arg)));
  } catch (e) {
    console.error(`json inválido: ${(e as Error).message}`);
    process.exit(2);
  }
} else if (cmd === "slug" && arg !== undefined) {
  const { slugOfRemote } = await import(`${skill}/src/lib/work.ts`);
  console.log(slugOfRemote(arg));
} else {
  console.error(USAGE);
  process.exit(2);
}
