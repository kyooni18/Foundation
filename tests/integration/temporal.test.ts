import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Foundation } from "../../src/foundation.js";
import { createTestDb, hasTestDatabase, type TestDb } from "../helpers/db.js";
import { FakeAI } from "../helpers/fake-ai.js";

describe.skipIf(!hasTestDatabase)("temporal correctness", () => {
  let t: TestDb;
  let ai: FakeAI;
  let f: Foundation;

  beforeAll(async () => {
    t = await createTestDb();
    ai = new FakeAI();
    f = new Foundation(t.db, ai, "UTC");
  });
  afterAll(async () => { await t.drop(); });

  const texts = (items: Array<{ rawText: string | null }>) => items.map(item => item.rawText);

  it("supersedes with validity intervals: current, as-of and history reads differ", async () => {
    await f.createAtom({ content: "Deployments target Heroku dynos", scope: { kind: "project", key: "ops" }, validFrom: "2024-01-01T00:00:00Z", metadata: {} });
    ai.resolver = input => ({
      action: "supersede",
      targetAtomId: input.candidates[0]!.id,
      canonicalContent: input.content,
      tags: input.tags,
      confidence: 0.95,
      rationale: "migrated hosting"
    });
    const result = await f.remember({
      content: "Deployments target Fly machines",
      project: "ops",
      sourceTime: "2025-06-01T00:00:00Z",
      metadata: { validFrom: "2025-06-01T00:00:00Z" }
    });
    expect(result.results[0]!.action).toBe("supersede");
    ai.resolver = null;

    expect(texts(await f.search("deployments target", { project: "ops" }))).toEqual(["Deployments target Fly machines"]);

    const past = await f.search("deployments target", { project: "ops", asOf: "2024-09-01T00:00:00Z" });
    expect(texts(past)).toEqual(["Deployments target Heroku dynos"]);

    const history = await f.search("deployments target", { project: "ops", history: true });
    expect(texts(history)).toEqual(expect.arrayContaining(["Deployments target Fly machines", "Deployments target Heroku dynos"]));
    expect(history.find(item => item.rawText?.includes("Heroku"))?.state).toBe("superseded");
    // The current fact outranks the historical one.
    expect(history[0]!.rawText).toContain("Fly");
  });

  it("hides expired and not-yet-valid facts, and a recent edit does not revive an expired one", async () => {
    const expired = (await f.createAtom({ content: "Freeze window for okapi release", expiresAt: "2020-01-01T00:00:00Z", metadata: {} })).atom;
    await f.createAtom({ content: "Okapi release scheduled someday", validFrom: "2999-01-01T00:00:00Z", metadata: {} });
    await f.updateAtom(expired.id, { tags: ["touched-today"] });
    expect(await f.search("okapi release")).toEqual([]);
    const history = await f.search("okapi release", { history: true });
    expect(history.length).toBe(2);
    expect(history.map(item => item.state).sort()).toEqual(["expired", "future"]);
  });

  it("records an older late-arriving fact as history without replacing the current fact", async () => {
    await f.createAtom({ content: "Pager rotation is weekly", validFrom: "2025-03-01T00:00:00Z", metadata: {} });
    ai.resolver = input => ({ action: "supersede", targetAtomId: input.candidates[0]!.id, canonicalContent: input.content, tags: input.tags, confidence: 0.95, rationale: "older" });
    const result = await f.remember({ content: "Pager rotation is monthly", sourceTime: "2024-01-01T00:00:00Z" });
    ai.resolver = null;
    expect(result.results[0]!.atom.status).toBe("superseded");
    expect(texts(await f.search("pager rotation"))).toEqual(["Pager rotation is weekly"]);
    expect(texts(await f.search("pager rotation", { asOf: "2024-06-01T00:00:00Z" }))).toEqual(["Pager rotation is monthly"]);
  });

  it("surfaces conflicting facts together instead of silently picking one", async () => {
    await f.createAtom({ content: "Capybara service port is 8080", metadata: {} });
    ai.resolver = input => ({ action: "conflict", targetAtomId: input.candidates[0]!.id, canonicalContent: input.content, tags: input.tags, confidence: 0.9, rationale: "disagree" });
    await f.remember({ content: "Capybara service port is 9090" });
    ai.resolver = null;
    const hits = await f.search("capybara service port");
    expect(hits.length).toBe(2);
    for (const hit of hits) expect(hit.conflictsWith?.length).toBe(1);
  });
});

describe.skipIf(!hasTestDatabase)("validity mirrors and external sync safety", () => {
  let t: TestDb;
  let f: Foundation;
  beforeAll(async () => { t = await createTestDb(); f = new Foundation(t.db, new FakeAI(), "UTC"); });
  afterAll(async () => { await t.drop(); });

  it("keeps metadata.validFrom/expires in step with the validity columns", async () => {
    const atom = (await f.createAtom({ content: "Mirror check fact", metadata: {} })).atom;
    const updated = await f.updateAtom(atom.id, { expiresAt: "2031-01-01T00:00:00Z", validFrom: "2030-01-01T00:00:00Z" });
    expect(updated.metadata.expires).toBe("2031-01-01T00:00:00.000Z");
    expect(updated.metadata.validFrom).toBe("2030-01-01T00:00:00.000Z");
  });

  it("does not let the legacy active toggle revive or bury a superseded Atom", async () => {
    const atom = (await f.createAtom({ content: "Superseded legacy toggle fact", status: "superseded", expiresAt: "2025-01-01T00:00:00Z", metadata: {} })).atom;
    expect((await f.updateAtom(atom.id, { active: true })).status).toBe("superseded");
    expect((await f.updateAtom(atom.id, { active: false })).status).toBe("superseded");
    const active = (await f.createAtom({ content: "Toggle me active fact", metadata: {} })).atom;
    expect((await f.updateAtom(active.id, { active: false })).status).toBe("forgotten");
    expect((await f.updateAtom(active.id, { active: true })).status).toBe("active");
  });
});
