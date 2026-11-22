// Lado TS do hades-core-probe: importa as funções da skill pelo checkout (HADES_SKILL_DIR, fixado pelo sandbox).
const skill = process.env.HADES_SKILL_DIR;
if (!skill) {
  console.error("HADES_SKILL_DIR ausente");
  process.exit(2);
}
const [cmd, arg, arg2, arg3] = process.argv.slice(2);
const USAGE = "uso: hades-core-probe <pyjson <json> | slug <url> | coreok <faixa> <versao> | getfield <json> <caminho> | mask <texto> | jline <kind> <resumo> | config-value <chave> <padrao> | work-root | state-dir | config-effective <dir> | config-save <dir> <json> <json> | reg-set <nome> <json> | reg-fresh <nome> <phase|resume> <rotulo> | delivery-set <chave> <json> | work-env | project | tofixed <numero> <digitos> | clock | render-cli <msg> <codigo|-> [prefixo] | message-of-failure <stderr> | suggest <digitado> <nome>... | cli-code-of-exit <n>>";

process.on("uncaughtException", (e: Error & { code?: unknown }) => {
  console.error(e.message);
  process.exit(e.name === "HadesError" && typeof e.code === "number" ? e.code : 1);
});

if (cmd === "pyjson" && arg !== undefined) {
  const { pyDumps } = await import(`${skill}/src/lib/pyjson.ts`);
  try {
    console.log(pyDumps(JSON.parse(arg)));
  } catch (e) {
    console.error(`json inválido: ${(e as Error).message}`);
    process.exit(2);
  }
} else if (cmd === "tofixed" && arg !== undefined && arg2 !== undefined) {
  console.log(Number(arg).toFixed(Number(arg2)));
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
} else if (cmd === "mask" && arg !== undefined) {
  const { maskSecrets } = await import(`${skill}/src/lib/journal.ts`);
  console.log(maskSecrets(arg));
} else if (cmd === "jline" && arg !== undefined && arg2 !== undefined) {
  const { eventOfSource } = await import(`${skill}/src/lib/journal.ts`);
  const now = new Date(1_773_144_000_000);
  console.log(JSON.stringify(eventOfSource("gate", arg, arg2, { agent: "a1", cwd: "/w", now })));
} else if (cmd === "config-value" && arg !== undefined && arg2 !== undefined) {
  const { configValue } = await import(`${skill}/src/lib/work.ts`);
  console.log(configValue(arg, arg2));
} else if (cmd === "work-root") {
  const { loadWorkEnv } = await import(`${skill}/src/lib/work.ts`);
  const { dirname } = await import("node:path");
  console.log(dirname(loadWorkEnv({ skillOk: true, skillForce: true }).work));
} else if (cmd === "state-dir") {
  const { findStateDir } = await import(`${skill}/src/lib/kit.ts`);
  console.log(findStateDir() ?? "");
} else if (cmd === "config-effective" && arg !== undefined) {
  const { effectiveConfig } = await import(`${skill}/src/lib/config.ts`);
  for (const [k, v] of effectiveConfig(arg, process.env)) console.log(`${k}=${v}`);
} else if (cmd === "config-save" && arg !== undefined && arg2 !== undefined && arg3 !== undefined) {
  const { save } = await import(`${skill}/src/lib/config-catalog.ts`);
  console.log(save({ dir: arg, kit: arg }, JSON.parse(arg2), JSON.parse(arg3)).join(","));
} else if (cmd === "reg-set" && arg !== undefined && arg2 !== undefined) {
  const { loadWorkEnv, regSet } = await import(`${skill}/src/lib/work.ts`);
  regSet(loadWorkEnv({ cwd: process.cwd() }), arg, JSON.parse(arg2));
} else if (cmd === "reg-fresh" && arg !== undefined && (arg2 === "phase" || arg2 === "resume") && arg3 !== undefined) {
  const { loadWorkEnv } = await import(`${skill}/src/lib/work.ts`);
  const { regUpdate, afterFreshSession } = await import(`${skill}/src/lib/agent-registry.ts`);
  regUpdate(loadWorkEnv({ cwd: process.cwd() }), arg, (r: Record<string, unknown>) => afterFreshSession(r, arg2, arg3, "2026-01-01T00:00:00Z"));
} else if (cmd === "delivery-set" && arg !== undefined && arg2 !== undefined) {
  const { loadWorkEnv } = await import(`${skill}/src/lib/work.ts`);
  const { deliverySet } = await import(`${skill}/src/lib/agent-registry.ts`);
  const patch = JSON.parse(arg2);
  deliverySet(loadWorkEnv({ cwd: process.cwd() }), arg, (d: Record<string, unknown>) => ({ ...d, ...patch }));
} else if (cmd === "work-env") {
  const { loadWorkEnv } = await import(`${skill}/src/lib/work.ts`);
  const w = loadWorkEnv({ cwd: process.cwd() });
  console.log(JSON.stringify({ skillScope: w.skillScope, repoTop: w.repoTop, mainClone: w.mainClone, slug: w.slug, repoName: w.repoName, work: w.work, locks: w.locks, state: w.state, orch: w.orch, skillOrch: w.skillOrch, reports: w.reports }));
} else if (cmd === "render-cli" && arg !== undefined && arg2 !== undefined) {
  const { renderCli } = await import(`${skill}/src/lib/human-errors.ts`);
  console.log(renderCli(arg, arg2 === "-" ? undefined : arg2, arg3));
} else if (cmd === "message-of-failure" && arg !== undefined) {
  const { messageOfCliFailure } = await import(`${skill}/src/lib/human-errors.ts`);
  console.log(messageOfCliFailure(arg));
} else if (cmd === "suggest" && arg !== undefined) {
  const { suggestName } = await import(`${skill}/src/lib/human-errors.ts`);
  console.log(suggestName(arg, process.argv.slice(4)) ?? "");
} else if (cmd === "cli-code-of-exit" && arg !== undefined) {
  const { cliCodeOfExit } = await import(`${skill}/src/lib/human-errors.ts`);
  console.log(cliCodeOfExit(Number(arg)));
} else if (cmd === "clock") {
  const { clockNow } = await import(`${skill}/src/lib/clock.ts`);
  console.log(String(clockNow().getTime()));
} else if (cmd === "project") {
  const { resolveProject, resolutionJson } = await import(`${skill}/src/lib/project.ts`);
  const r = resolveProject(process.cwd());
  console.log(resolutionJson(r));
  if (r.message) console.error(r.message);
  process.exit(r.code);
} else {
  console.error(USAGE);
  process.exit(2);
}
