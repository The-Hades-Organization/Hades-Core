// Lado TS do hades-core-probe: importa as funções da skill pelo checkout (HADES_SKILL_DIR, fixado pelo sandbox).
const skill = process.env.HADES_SKILL_DIR;
if (!skill) {
  console.error("HADES_SKILL_DIR ausente");
  process.exit(2);
}
const [cmd, arg, arg2] = process.argv.slice(2);
const USAGE = "uso: hades-core-probe <pyjson <json> | slug <url> | coreok <faixa> <versao> | getfield <json> <caminho>>";

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
} else if (cmd === "coreok" && arg !== undefined && arg2 !== undefined) {
  const { coreOk } = await import(`${skill}/src/lib/profile.ts`);
  try {
    console.log(String(coreOk(arg, arg2)));
  } catch (e) {
    console.log((e as Error).message);
    process.exit(1);
  }
} else if (cmd === "getfield" && arg !== undefined && arg2 !== undefined) {
  const { getField } = await import(`${skill}/src/lib/profile.ts`);
  let doc: unknown;
  try {
    doc = JSON.parse(arg);
  } catch (e) {
    console.error(`json inválido: ${(e as Error).message}`);
    process.exit(2);
  }
  const out = getField(doc, arg2, undefined);
  if (out === undefined) process.exit(1);
  console.log(out);
} else {
  console.error(USAGE);
  process.exit(2);
}
