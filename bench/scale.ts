/**
 * Scale / index-usage benchmark: bulk-loads synthetic Atoms, then compares the legacy full-scan ranking SQL
 * with the current candidate-generation + rescoring pipeline, and prints which indexes the plans use.
 *   TEST_DATABASE_URL=... npx tsx bench/scale.ts --atoms=20000
 */
import { Foundation } from "../src/foundation.js";
import { newSearchTrace } from "../src/search.js";
import { createTestDb } from "../tests/helpers/db.js";
import { FakeAI, bagOfWordsVector } from "../tests/helpers/fake-ai.js";
import { LEGACY_ATOM_SEARCH_SQL } from "./legacy-sql.js";

const N = Number(process.argv.find(a => a.startsWith("--atoms="))?.split("=")[1] ?? 20000);
let seed = 7;
const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const vocab = Array.from({ length: 3000 }, (_, i) => `w${i.toString(36)}${"aeiou"[i % 5]}${(i * 7919) % 97}`);
const zipf = () => vocab[Math.floor(vocab.length * rand() ** 2.2)]!;
const pct = (v: number[], p: number) => [...v].sort((a, b) => a - b)[Math.min(v.length - 1, Math.floor((p / 100) * v.length))]!;

const t = await createTestDb();
const ai = new FakeAI();
const f = new Foundation(t.db, ai, "UTC");
await t.db.query("DROP INDEX atoms_vector_hnsw");

const projects = Array.from({ length: 20 }, (_, i) => `proj${i}`);
const texts: string[] = [];
const owners: string[] = [];
const t0 = performance.now();
const BATCH = 500;
for (let start = 0; start < N; start += BATCH) {
  const contents: string[] = [], hashes: string[] = [], kinds: string[] = [], keys: string[] = [], vectors: string[] = [];
  for (let i = start; i < Math.min(N, start + BATCH); i++) {
    const project = projects[i % projects.length]!;
    const content = `${project} ${Array.from({ length: 10 }, zipf).join(" ")} item${i}`;
    texts.push(content); owners.push(project);
    contents.push(content); keys.push(project); kinds.push("project");
    vectors.push("[" + bagOfWordsVector(content).join(",") + "]");
  }
  await t.db.query(
    `INSERT INTO atoms(content,normalized_content,content_hash,type,scope_kind,scope_key,vector,vector_provider,vector_model,vector_dimensions,vector_embedded_at)
     SELECT c, lower(c), encode(sha256(convert_to(c,'UTF8')),'hex'), 'usercreated', k, key, v::halfvec, 'gemini','fake-bow',3072,NOW()
     FROM unnest($1::text[],$2::text[],$3::text[],$4::text[]) AS u(c,k,key,v)`,
    [contents, kinds, keys, vectors]
  );
}
const loadMs = performance.now() - t0;
const t1 = performance.now();
await t.db.query("CREATE INDEX atoms_vector_hnsw ON atoms USING hnsw (vector halfvec_cosine_ops) WHERE vector IS NOT NULL");
const hnswMs = performance.now() - t1;
await t.db.query("ANALYZE atoms");

const queries = Array.from({ length: 40 }, (_, i) => {
  const idx = Math.floor(rand() * N);
  const words = texts[idx]!.split(" ").slice(1, 11);
  return { text: [words[0], words[3], words[7]].join(" "), project: owners[idx]!, expected: texts[idx]! };
});

const timeIt = async (fn: () => Promise<unknown>) => { const s = performance.now(); await fn(); return performance.now() - s; };
const legacy: number[] = [], current: number[] = [];
let hit = 0;
for (const q of queries) {
  const embedding = await ai.embed(q.text);
  const vec = "[" + embedding!.vector.join(",") + "]";
  legacy.push(await timeIt(() => t.db.query(LEGACY_ATOM_SEARCH_SQL, [q.text, [], vec, 100, "gemini", "fake-bow", 3072, q.text, "{}"])));
  let results: Awaited<ReturnType<Foundation["search"]>> = [];
  current.push(await timeIt(async () => { results = await f.search(q.text, { limit: 10, project: q.project }); }));
  if (results.some(r => r.rawText === q.expected)) hit += 1;
}

const trace = newSearchTrace(true);
await f.search(queries[0]!.text, { limit: 10, project: queries[0]!.project, trace } as never);
const unscopedTrace = newSearchTrace(true);
await f.search(queries[0]!.text, { limit: 10, trace: unscopedTrace } as never);
const used = trace.generators.filter(g => g.target === "atoms").map(g => ({
  generator: g.name, rows: g.rows, ms: Number(g.ms.toFixed(1)),
  index: (g.plan ?? []).join(" ").match(/(?:Bitmap Index Scan|Index Scan|Index Only Scan) on (\w+)|Seq Scan/g)?.slice(0, 2).join(", ") ?? "?"
}));
const legacyPlan = (await t.db.query("EXPLAIN " + LEGACY_ATOM_SEARCH_SQL, [queries[0]!.text, [], null, 100, null, null, null, queries[0]!.text, "{}"])).rows.map(r => r["QUERY PLAN"]).join(" ").match(/Seq Scan on \w+|Index Scan[^(]*/g)?.slice(0, 3);

console.log(JSON.stringify({
  atoms: N, loadMs: Math.round(loadMs), hnswBuildMs: Math.round(hnswMs),
  legacyFullScanMs: { p50: Number(pct(legacy, 50).toFixed(1)), p95: Number(pct(legacy, 95).toFixed(1)) },
  currentPipelineMs: { p50: Number(pct(current, 50).toFixed(1)), p95: Number(pct(current, 95).toFixed(1)) },
  breakdownMs: { total: Number(trace.totalMs?.toFixed(1)), rescore: Number(trace.rescoreMs?.toFixed(1)), unscopedTotal: Number(unscopedTrace.totalMs?.toFixed(1)) },
  unscopedVectorPlan: (unscopedTrace.generators.find(g => g.name === "vector" && g.target === "atoms")?.plan ?? []).join(" ").match(/Index Scan using \w+|Seq Scan on \w+|Bitmap Index Scan on \w+/g)?.slice(0, 3),
  currentTopHit: `${hit}/${queries.length}`,
  legacyPlan, currentGenerators: used
}, null, 2));
if (process.argv.includes("--keep")) console.error("kept database", t.name); else await t.drop();
