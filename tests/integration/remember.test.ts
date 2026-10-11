import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Foundation } from "../../src/foundation.js";
import { createTestDb, hasTestDatabase, type TestDb } from "../helpers/db.js";
import { FakeAI } from "../helpers/fake-ai.js";

describe.skipIf(!hasTestDatabase)("remember reconciliation", () => {
  let t: TestDb;
  let ai: FakeAI;
  let f: Foundation;

  beforeAll(async () => {
    t = await createTestDb();
    ai = new FakeAI();
    f = new Foundation(t.db, ai, "UTC");
  });
  afterAll(async () => { await t.drop(); });

  const count = async (sql: string, params: unknown[] = []) => Number((await t.db.query(sql, params)).rows[0]!.n);

  it("refreshes an exact duplicate without overwriting its validity", async () => {
    const first = (await f.remember({ content: "Lynx team standup is at 10:00", metadata: { validFrom: "2025-01-01T00:00:00Z" } })).results[0]!;
    const second = (await f.remember({ content: "lynx team standup is at 10:00" })).results[0]!;
    expect(second.action).toBe("duplicate");
    expect(second.atom.id).toBe(first.atom.id);
    expect(second.atom.validFrom?.toISOString()).toBe(first.atom.validFrom?.toISOString());
  });

  it("downgrades low-confidence destructive decisions to a conflict and keeps both Atoms", async () => {
    const base = (await f.remember({ content: "Marmot retention is 30 days" })).results[0]!.atom;
    ai.resolver = input => ({ action: "supersede", targetAtomId: input.candidates[0]!.id, canonicalContent: input.content, tags: input.tags, confidence: 0.4, rationale: "unsure" });
    const result = (await f.remember({ content: "Marmot retention is 90 days" })).results[0]!;
    ai.resolver = null;
    expect(result.action).toBe("conflict");
    const stillActive = await t.db.query("SELECT status FROM atoms WHERE id=$1", [base.id]);
    expect(stillActive.rows[0]!.status).toBe("active");
    expect(await count("SELECT count(*) n FROM atom_relations WHERE relation='conflicts_with' AND to_atom_id=$1", [base.id])).toBe(1);
  });

  it("ignores resolver targets that are not same-scope candidates", async () => {
    const other = (await f.remember({ content: "Ibex license is MIT", project: "one" })).results[0]!.atom;
    ai.resolver = () => ({ action: "update", targetAtomId: other.id, canonicalContent: "Ibex license is Apache", tags: [], confidence: 0.99, rationale: "hallucinated" });
    const result = (await f.remember({ content: "Ibex license is Apache", project: "two" })).results[0]!;
    ai.resolver = null;
    expect(result.action).toBe("new");
    expect(result.atom.id).not.toBe(other.id);
    expect((await f.getAtom(other.id)).content).toBe("Ibex license is MIT");
  });

  it("updates in place, preserving id and the previous wording as history", async () => {
    const base = (await f.remember({ content: "Gecko API timeout is 5s" })).results[0]!.atom;
    ai.resolver = input => ({ action: "update", targetAtomId: input.candidates[0]!.id, canonicalContent: "Gecko API timeout is 5s for reads, 30s for writes", tags: input.tags, confidence: 0.95, rationale: "refined" });
    const result = (await f.remember({ content: "Gecko API timeout is 30s for writes" })).results[0]!;
    ai.resolver = null;
    expect(result.action).toBe("update");
    expect(result.atom.id).toBe(base.id);
    expect(result.atom.content).toContain("30s for writes");
    expect(result.atom.metadata.previousContent).toBe("Gecko API timeout is 5s");
    expect(Array.isArray(result.atom.metadata.history)).toBe(true);
  });

  it("never overwrites a target that changed between planning and applying", async () => {
    const base = (await f.remember({ content: "Heron queue depth limit is 100" })).results[0]!.atom;
    ai.resolver = async input => {
      // Simulates a concurrent writer touching the target while the model is thinking.
      await f.updateAtom(input.candidates[0]!.id, { tags: ["concurrent-edit"] });
      return { action: "update", targetAtomId: input.candidates[0]!.id, canonicalContent: "Heron queue depth limit is 500", tags: input.tags, confidence: 0.95, rationale: "update" };
    };
    const result = (await f.remember({ content: "Heron queue depth limit is 500" })).results[0]!;
    ai.resolver = null;
    expect(result.action).toBe("conflict");
    expect((await f.getAtom(base.id)).content).toBe("Heron queue depth limit is 100");
  });

  it("stores an Atom with deferred embedding when the provider fails and stays searchable", async () => {
    ai.embeddingsFail = true;
    const result = (await f.remember({ content: "Kestrel exporter listens on port 9464" })).results[0]!;
    ai.embeddingsFail = false;
    expect(result.embeddingDeferred).toBe(true);
    expect(result.atom.embedding).toBeNull();
    // Search keeps working lexically and by exact identifier when embeddings are down.
    ai.embeddingsFail = true;
    const hits = await f.search("kestrel exporter 9464");
    ai.embeddingsFail = false;
    expect(hits.map(hit => hit.id)).toContain(result.atom.id);
  });

  it("commits multi-Atom requests all-or-nothing and can be retried safely", async () => {
    ai.splitOnSemicolon = true;
    await t.db.query(`
      CREATE OR REPLACE FUNCTION fail_on_boom() RETURNS trigger LANGUAGE plpgsql AS $$
      BEGIN IF NEW.content LIKE '%BOOM%' THEN RAISE EXCEPTION 'injected failure'; END IF; RETURN NEW; END $$`);
    await t.db.query("CREATE TRIGGER fail_boom BEFORE INSERT ON atoms FOR EACH ROW EXECUTE FUNCTION fail_on_boom()");

    const before = await count("SELECT count(*) n FROM atoms");
    const eventsBefore = await count("SELECT count(*) n FROM memory_events");
    await expect(f.remember({ content: "Osprey alpha fact; Osprey BOOM fact", requestId: "req-atomic" })).rejects.toThrow(/injected/);
    expect(await count("SELECT count(*) n FROM atoms")).toBe(before);
    expect(await count("SELECT count(*) n FROM memory_events")).toBe(eventsBefore);
    expect(await count("SELECT count(*) n FROM memory_requests WHERE request_id='req-atomic'")).toBe(0);

    await t.db.query("DROP TRIGGER fail_boom ON atoms");
    const ok = await f.remember({ content: "Osprey alpha fact; Osprey BOOM fact", requestId: "req-atomic" });
    expect(ok.results.length).toBe(2);
    expect(await count("SELECT count(*) n FROM atoms")).toBe(before + 2);
    ai.splitOnSemicolon = false;
  });

  it("is idempotent per requestId across restarts and rejects reuse with different content", async () => {
    const first = await f.remember({ content: "Wombat cache TTL is 60s", requestId: "req-idem" });
    const eventsAfterFirst = await count("SELECT count(*) n FROM memory_events WHERE request_id='req-idem'");

    const restarted = new Foundation(t.db, ai, "UTC"); // fresh in-process state
    const second = await restarted.remember({ content: "Wombat cache TTL is 60s", requestId: "req-idem" });
    expect(second).toEqual(JSON.parse(JSON.stringify(first)));
    expect(await count("SELECT count(*) n FROM memory_events WHERE request_id='req-idem'")).toBe(eventsAfterFirst);
    await expect(restarted.remember({ content: "Wombat cache TTL is 5s", requestId: "req-idem" })).rejects.toThrow(/different content/);
  });

  it("serializes concurrent instances: identical writes yield one Atom, same requestId one result", async () => {
    const a = new Foundation(t.db, ai, "UTC");
    const b = new Foundation(t.db, ai, "UTC");
    const writes = await Promise.all(
      Array.from({ length: 8 }, (_, i) => (i % 2 ? a : b).remember({ content: "Tapir race condition canary fact" }))
    );
    const ids = new Set(writes.map(result => result.results[0]!.atom.id));
    expect(ids.size).toBe(1);
    expect(await count("SELECT count(*) n FROM atoms WHERE content='Tapir race condition canary fact'")).toBe(1);

    const same = await Promise.all([
      a.remember({ content: "Tapir request canary fact", requestId: "req-race" }),
      b.remember({ content: "Tapir request canary fact", requestId: "req-race" })
    ]);
    expect(same[0]!.results[0]!.atom.id).toBe(same[1]!.results[0]!.atom.id);
    expect(await count("SELECT count(*) n FROM memory_events WHERE request_id='req-race'")).toBe(1);
  });

  it("revives a forgotten memory only when it is explicitly remembered again", async () => {
    const first = (await f.remember({ content: "Dingo backup runs nightly" })).results[0]!.atom;
    await f.forget(first.id);
    expect(await f.search("dingo backup nightly")).toEqual([]);
    const again = (await f.remember({ content: "Dingo backup runs nightly" })).results[0]!.atom;
    expect(again.id).toBe(first.id);
    expect((await f.search("dingo backup nightly")).map(hit => hit.id)).toContain(first.id);
  });
});
