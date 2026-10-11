/**
 * Retrieval + memory-lifecycle benchmark on synthetic multi-project data.
 *
 *   TEST_DATABASE_URL=postgresql://postgres:test@127.0.0.1:55433/postgres npm run bench -- [--projects=8] [--per-project=60] [--no-embeddings]
 *
 * Runs only against a throwaway database created by tests/helpers/db.ts. The embedding provider is a
 * deterministic bag-of-words fake, so numbers measure Foundation's candidate generation, thresholds,
 * scoping and lifecycle handling, not a vendor model.
 */
import { writeFileSync, mkdirSync } from "node:fs";
import { Foundation } from "../src/foundation.js";
import { createTestDb } from "../tests/helpers/db.js";
import { FakeAI } from "../tests/helpers/fake-ai.js";

const arg = (name: string, fallback: number) => Number(process.argv.find(a => a.startsWith(`--${name}=`))?.split("=")[1] ?? fallback);
const flag = (name: string) => process.argv.includes(`--${name}`);
const PROJECTS = arg("projects", 8);
const PER_PROJECT = arg("per-project", 60);
const NO_EMBED = flag("no-embeddings");

let seed = 42;
const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const pick = <T>(items: T[]): T => items[Math.floor(rand() * items.length)]!;

const COMPONENTS = ["queue", "cache", "auth", "billing", "search", "deploy", "logging", "metrics", "storage", "scheduler", "gateway", "mailer", "backup", "tracing", "cdn"];
const TECH = ["redis", "kafka", "postgres", "nginx", "rabbitmq", "memcached", "clickhouse", "minio", "vault", "consul", "etcd", "envoy", "grafana", "loki", "temporal", "nats", "cassandra", "elastic", "sqlite", "duckdb"];
const PURPOSE = ["retries", "sharding", "failover", "rate limiting", "snapshots", "tenant isolation", "audit trails", "batching"];
const PROJECT_NAMES = ["alpha", "borealis", "cinder", "delphi", "ember", "fjord", "glacier", "harbor", "ivory", "jasper", "kestrel", "lumen"];

interface QueryCase {
  kind: string;
  query: string;
  project: string | null;
  relevant: Set<string>;
  /** Atoms that must never be returned (stale or other-project twins). */
  forbidden: Set<string>;
  expectEmpty?: boolean;
  fileRelevant?: Set<string>;
}

function percentile(values: number[], p: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))] ?? 0;
}

async function main() {
  const t = await createTestDb();
  const ai = new FakeAI();
  const f = new Foundation(t.db, ai, "UTC");
  const cases: QueryCase[] = [];
  const projectAtoms = new Map<string, Map<string, string>>(); // project -> component -> atom id
  const forbiddenByKey = new Map<string, Set<string>>();

  const names = PROJECT_NAMES.slice(0, PROJECTS);
  const started = performance.now();

  // Global preferences shared by everyone.
  const globals: string[] = [];
  for (const text of [
    "Prefer small reviewable commits with descriptive messages",
    "Use UTC timestamps in every log line",
    "Never commit secrets or tokens to the repository"
  ]) globals.push((await f.createAtom({ content: text, scope: { kind: "global", key: "" }, metadata: {} })).atom.id);

  for (const project of names) {
    const map = new Map<string, string>();
    projectAtoms.set(project, map);
    const facts: Array<{ component: string; tech: string; ident: string; id: string }> = [];
    for (const component of COMPONENTS.slice(0, Math.min(COMPONENTS.length, PER_PROJECT))) {
      const tech = pick(TECH);
      const ident = `${project[0]!.toUpperCase()}${project.slice(1)}${component[0]!.toUpperCase()}${component.slice(1)}Svc`;
      // Identical wording across projects except the project-specific identifier => twin decoys.
      const content = `The ${component} service uses ${tech} for ${pick(PURPOSE)} via ${ident}`;
      const atom = (await f.createAtom({
        content,
        scope: { kind: "project", key: project },
        validFrom: "2024-01-01T00:00:00Z",
        metadata: { project },
        tags: [component, project]
      })).atom;
      map.set(component, atom.id);
      facts.push({ component, tech, ident, id: atom.id });
    }
    // Outdated decisions: superseded atoms must not surface in current reads.
    for (const component of COMPONENTS.slice(0, 3)) {
      const oldId = map.get(component)!;
      const newTech = pick(TECH);
      const next = (await f.createAtom({
        content: `Decision: ${project} ${component} moved to ${newTech} replacing the previous backend`,
        scope: { kind: "project", key: project },
        validFrom: "2025-06-01T00:00:00Z",
        metadata: { project }
      })).atom;
      await t.db.query("UPDATE atoms SET status='superseded', expires_at='2025-06-01' WHERE id=$1", [oldId]);
      await t.db.query("INSERT INTO atom_relations(from_atom_id,to_atom_id,relation) VALUES($1,$2,'supersedes')", [next.id, oldId]);
      map.set(component, next.id);
      facts.splice(facts.findIndex(fact => fact.id === oldId), 1);
      cases.push({ kind: "outdated-decision", query: `${project} ${component} moved decision`, project, relevant: new Set([next.id]), forbidden: new Set([oldId]) });
    }
    for (const fact of facts) {
      cases.push({ kind: "project-fact", query: `${fact.component} service ${fact.tech}`, project, relevant: new Set([fact.id]), forbidden: new Set() });
      cases.push({ kind: "identifier", query: fact.ident, project, relevant: new Set([fact.id]), forbidden: new Set() });
    }
    // Contradictory memories: both must be returned and flagged.
    const a = (await f.createAtom({ content: `${project} deploy window is Tuesday morning`, scope: { kind: "project", key: project }, metadata: {} })).atom;
    const b = (await f.createAtom({ content: `${project} deploy window is Friday evening`, scope: { kind: "project", key: project }, metadata: {} })).atom;
    await t.db.query("INSERT INTO atom_relations(from_atom_id,to_atom_id,relation) VALUES($1,$2,'conflicts_with')", [b.id, a.id]);
    cases.push({ kind: "contradiction", query: `${project} deploy window`, project, relevant: new Set([a.id, b.id]), forbidden: new Set() });

    // Filler memories (noise) to give the index something to wade through.
    for (let i = 0; i < PER_PROJECT; i++) {
      await f.createAtom({
        content: `${project} note ${i}: ${pick(COMPONENTS)} ${pick(PURPOSE)} experiment ${pick(TECH)} observation ${Math.floor(rand() * 1e6)}`,
        scope: { kind: "project", key: project },
        metadata: { project }
      });
    }
    // Disconnected File (no Atoms): found only by direct File search.
    const file = await f.createFile({
      name: `${project}-runbook`,
      content: `${project} runbook: restart the ${pick(COMPONENTS)} pods then verify ${pick(TECH)} health endpoint ${project}-probe-${Math.floor(rand() * 1e5)}`,
      mimeType: "text/plain",
      metadata: { project }
    });
    cases.push({ kind: "disconnected-file", query: `${project} runbook restart pods`, project, relevant: new Set(), fileRelevant: new Set([file.id]), forbidden: new Set() });
  }

  for (const text of ["Use UTC timestamps log line", "commits descriptive messages small"]) {
    cases.push({ kind: "global-fallback", query: text, project: names[0]!, relevant: new Set(globals.slice(text.includes("UTC") ? 1 : 0, text.includes("UTC") ? 2 : 1)), forbidden: new Set() });
  }
  for (const q of ["quantum chromodynamics lattice gauge", "recipe for sourdough starter hydration", "zebra migration patterns savanna"]) {
    cases.push({ kind: "unanswerable", query: q, project: names[0]!, relevant: new Set(), forbidden: new Set(), expectEmpty: true });
  }

  // Cross-project twin decoys: every project's atom for a component is forbidden for other projects' queries.
  for (const c of cases) {
    if (!c.project) continue;
    for (const [other, map] of projectAtoms) {
      if (other === c.project) continue;
      for (const id of map.values()) c.forbidden.add(id);
    }
  }
  const seedMs = performance.now() - started;

  const totals = await t.db.query("SELECT (SELECT count(*) FROM atoms) atoms, (SELECT count(*) FROM files) files");
  const run = async (label: string, embeddings: boolean) => {
    ai.embeddingsFail = !embeddings;
    const latencies: number[] = [];
    const byKind = new Map<string, { n: number; hit5: number; hit10: number; returned: number; irrelevant: number }>();
    let p5 = 0, r10 = 0, scored = 0, irrelevant = 0, returned = 0, leaks = 0, stale = 0, emptyOk = 0, emptyTotal = 0, fileHit = 0, fileTotal = 0, conflictFlagged = 0, conflictTotal = 0;
    for (const c of cases) {
      const t0 = performance.now();
      const results = await f.search(c.query, { limit: 10, project: c.project ?? undefined });
      latencies.push(performance.now() - t0);
      const atomIds = results.filter(r => r.kind === "atom").map(r => r.id);
      if (c.expectEmpty) {
        emptyTotal += 1;
        if (results.length === 0) emptyOk += 1;
        irrelevant += results.length; returned += results.length;
        continue;
      }
      if (c.fileRelevant) {
        fileTotal += 1;
        if (results.some(r => r.kind === "file" && c.fileRelevant!.has(r.id))) fileHit += 1;
      }
      for (const id of atomIds) {
        if (c.forbidden.has(id)) leaks += 1;
        if (c.kind === "outdated-decision" && c.forbidden.has(id)) stale += 1;
      }
      if (c.relevant.size) {
        const k = byKind.get(c.kind) ?? { n: 0, hit5: 0, hit10: 0, returned: 0, irrelevant: 0 };
        k.n += 1;
        k.hit5 += atomIds.slice(0, 5).filter(id => c.relevant.has(id)).length / Math.max(1, Math.min(5, c.relevant.size));
        k.hit10 += atomIds.slice(0, 10).filter(id => c.relevant.has(id)).length / c.relevant.size;
        k.returned += atomIds.length;
        k.irrelevant += atomIds.filter(id => !c.relevant.has(id)).length;
        byKind.set(c.kind, k);
        scored += 1;
        const top5 = atomIds.slice(0, 5);
        p5 += top5.filter(id => c.relevant.has(id)).length / Math.max(1, Math.min(5, c.relevant.size));
        r10 += atomIds.slice(0, 10).filter(id => c.relevant.has(id)).length / c.relevant.size;
        irrelevant += atomIds.filter(id => !c.relevant.has(id)).length;
        returned += atomIds.length;
      }
      if (c.kind === "contradiction") {
        conflictTotal += 1;
        if (results.filter(r => r.conflictsWith?.length).length === 2) conflictFlagged += 1;
      }
    }
    return {
      label,
      queries: cases.length,
      precisionAt5: p5 / scored,
      recallAt10: r10 / scored,
      irrelevantResultRate: returned ? irrelevant / returned : 0,
      crossProjectLeaks: leaks,
      staleResults: stale,
      unanswerableReturnedEmpty: `${emptyOk}/${emptyTotal}`,
      fileRecall: `${fileHit}/${fileTotal}`,
      conflictsFlagged: `${conflictFlagged}/${conflictTotal}`,
      p50ms: Number(percentile(latencies, 50).toFixed(2)),
      p95ms: Number(percentile(latencies, 95).toFixed(2)),
      byKind: Object.fromEntries([...byKind].map(([k, v]) => [k, { queries: v.n, precisionAt5: Number((v.hit5 / v.n).toFixed(3)), recallAt10: Number((v.hit10 / v.n).toFixed(3)), irrelevantRate: Number((v.returned ? v.irrelevant / v.returned : 0).toFixed(3)) }]))
    };
  };

  const retrieval = [await run("hybrid (embeddings)", true), await run("lexical only (embeddings down)", false)];
  ai.embeddingsFail = false;

  // ---- Reconciliation correctness against a scripted oracle resolver ----
  let ok = 0, total = 0;
  const check = (name: string, condition: boolean) => { total += 1; if (condition) ok += 1; else console.error("reconciliation check failed:", name); };
  const scenario = new Foundation(t.db, ai, "UTC");
  for (let i = 0; i < 10; i++) {
    const text = `Reconcile subject ${i} stores data in bucket-${i}`;
    const first = (await scenario.remember({ content: text, project: "recon", sourceTime: "2025-01-01T00:00:00Z" })).results[0]!;
    const dup = (await scenario.remember({ content: text, project: "recon" })).results[0]!;
    check("duplicate", dup.action === "duplicate" && dup.atom.id === first.atom.id);
    const twin = (await scenario.remember({ content: text, project: "recon-twin" })).results[0]!;
    check("cross-scope-independent", twin.atom.id !== first.atom.id && twin.action === "new");

    const subjectCandidate = (input: { candidates: Array<{ id: string; content: string }> }) => input.candidates.find(c => c.content.includes(`subject ${i} `))?.id ?? null;
    ai.resolver = input => ({ action: "supersede", targetAtomId: subjectCandidate(input), canonicalContent: input.content, tags: input.tags, confidence: 0.92, rationale: "oracle" });
    const next = (await scenario.remember({ content: `Reconcile subject ${i} stores data in vault-${i}`, project: "recon", sourceTime: "2026-01-01T00:00:00Z" })).results[0]!;
    check("supersede", next.action === "supersede" && (await scenario.getAtom(first.atom.id)).status === "superseded");
    check("history-preserved", (await t.db.query("SELECT 1 FROM atom_relations WHERE from_atom_id=$1 AND to_atom_id=$2 AND relation='supersedes'", [next.atom.id, first.atom.id])).rowCount === 1);

    ai.resolver = input => ({ action: "supersede", targetAtomId: subjectCandidate(input), canonicalContent: input.content, tags: input.tags, confidence: 0.3, rationale: "unsure" });
    const unsure = (await scenario.remember({ content: `Reconcile subject ${i} stores data in tape-${i}`, project: "recon", sourceTime: "2026-02-01T00:00:00Z" })).results[0]!;
    check("low-confidence-conservative", unsure.action === "conflict" && (await scenario.getAtom(next.atom.id)).status === "active");
    ai.resolver = null;
  }
  const reconciliation = { checks: total, correct: ok, accuracy: ok / total };

  const concurrent = await Promise.all(Array.from({ length: 12 }, () => new Foundation(t.db, ai, "UTC").remember({ content: "Concurrent canary memory for bench" })));
  const concurrentAtoms = new Set(concurrent.map(r => r.results[0]!.atom.id)).size;

  let sampleTrace: unknown = null;
  const explain = await (async () => {
    const trace = (await import("../src/search.js")).newSearchTrace(true);
    await f.search(cases[0]!.query, { limit: 10, project: cases[0]!.project ?? undefined, trace } as never);
    sampleTrace = { totalMs: trace.totalMs, rescoreMs: trace.rescoreMs, embeddingError: trace.embeddingError ?? null };
    return trace.generators.map(g => ({ target: g.target, name: g.name, rows: g.rows, ms: Number(g.ms.toFixed(2)), plan: (g.plan ?? []).slice(0, 4).join(" | ").replace(/\[[-0-9.,e]+\]/g, "[vector]").slice(0, 400) }));
  })();

  const report = {
    dataset: { projects: names.length, atoms: Number(totals.rows[0]!.atoms), files: Number(totals.rows[0]!.files), queries: cases.length, seedMs: Math.round(seedMs) },
    retrieval,
    reconciliation,
    concurrentIdenticalWrites: { writers: 12, distinctAtoms: concurrentAtoms },
    generatorPlans: explain,
    sampleTrace: sampleTrace
  };
  mkdirSync("bench/results", { recursive: true });
  writeFileSync(`bench/results/${NO_EMBED ? "no-embeddings" : "latest"}.json`, JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ ...report, generatorPlans: `${explain.length} generators (see bench/results)` }, null, 2));
  await t.drop();
}

main().catch(error => { console.error(error); process.exit(1); });
