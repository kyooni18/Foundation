import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Foundation } from "../../src/foundation.js";
import { createTestDb, hasTestDatabase, type TestDb } from "../helpers/db.js";
import { FakeAI } from "../helpers/fake-ai.js";

describe.skipIf(!hasTestDatabase)("CLI diagnostics", () => {
  let t: TestDb;
  let f: Foundation;
  beforeAll(async () => { t = await createTestDb(); f = new Foundation(t.db, new FakeAI(), "UTC"); });
  afterAll(async () => { await t.drop(); });

  it("explains ranking with per-signal evidence and index plans", async () => {
    const atom = (await f.remember({ content: "Quasar ingest pipeline uses NATS jetstream", project: "p" })).results[0]!.atom;
    const { trace, results } = await f.explainSearch("quasar jetstream", { project: "p", plans: true });
    expect(results.map(r => r.id)).toContain(atom.id);
    expect(trace.generators.map(g => g.name)).toEqual(expect.arrayContaining(["fts", "trigram", "vector"]));
    const candidate = trace.candidates.find(c => c.key === `atom:${atom.id}`)!;
    expect(candidate.final).toBe(true);
    expect(candidate.signals.lexical).toBeGreaterThan(0.9);
    const plan = trace.generators.find(g => g.name === "fts" && g.target === "atoms")!.plan!.join("\n");
    expect(plan).not.toMatch(/\[[-0-9.,]{200,}\]/); // vector literals are redacted from plans
  });

  it("reports relations, history and health without leaking text in diagnose()", async () => {
    await f.remember({ content: "Nebula cache TTL is 60s", project: "p" });
    const rel = await f.atomRelations((await f.search("nebula cache TTL", { project: "p" }))[0]!.id);
    expect(rel.status).toBe("active");
    const health = await f.diagnose();
    expect(JSON.stringify(health)).not.toMatch(/Nebula|Quasar/);
    expect(health.scopes).toEqual(expect.arrayContaining([expect.objectContaining({ scope_kind: "project" })]));
  });
});
