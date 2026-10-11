import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Foundation } from "../../src/foundation.js";
import { createTestDb, hasTestDatabase, type TestDb } from "../helpers/db.js";
import { FakeAI } from "../helpers/fake-ai.js";

describe.skipIf(!hasTestDatabase)("memory scope isolation", () => {
  let t: TestDb;
  let f: Foundation;

  beforeAll(async () => {
    t = await createTestDb();
    f = new Foundation(t.db, new FakeAI(), "UTC");
  });
  afterAll(async () => { await t.drop(); });

  const ids = (items: Array<{ id: string }>) => items.map(item => item.id);

  it("keeps identical facts in independent projects as independent identities", async () => {
    const a = (await f.remember({ content: "The build uses pnpm workspaces", project: "alpha" })).results[0]!.atom;
    const b = (await f.remember({ content: "The build uses pnpm workspaces", project: "beta" })).results[0]!.atom;
    expect(a.id).not.toBe(b.id);
    expect(a.scope).toEqual({ kind: "project", key: "alpha" });
    expect(b.scope).toEqual({ kind: "project", key: "beta" });

    // Re-remembering inside a scope refreshes that scope's Atom only.
    const again = (await f.remember({ content: "The build uses pnpm workspaces", project: "alpha" })).results[0]!;
    expect(again.action).toBe("duplicate");
    expect(again.atom.id).toBe(a.id);

    await f.forget(a.id);
    expect(ids(await f.search("pnpm workspaces build", { project: "beta" }))).toContain(b.id);
    expect(ids(await f.search("pnpm workspaces build", { project: "alpha" }))).not.toContain(a.id);
  });

  it("never leaks project memories into other projects, but falls back to global", async () => {
    const secret = (await f.remember({ content: "Gamma launch codename is Heron", project: "gamma" })).results[0]!.atom;
    const shared = (await f.remember({ content: "Everyone prefers Heron-free naming conventions", scope: "global" })).results[0]!.atom;

    const fromDelta = await f.search("Heron", { project: "delta" });
    expect(ids(fromDelta)).not.toContain(secret.id);
    expect(ids(fromDelta)).toContain(shared.id);

    const fromGamma = await f.search("Heron codename", { project: "gamma" });
    expect(ids(fromGamma)).toContain(secret.id);
    expect(ids(await f.search("project:gamma Heron codename"))).toContain(secret.id);
    expect(ids(await f.search("project:delta Heron codename"))).not.toContain(secret.id);
  });

  it("hides conversation memories from every other conversation", async () => {
    const mine = (await f.remember({ content: "Scratch idea: try ocelot caching", scope: "conversation", conversationId: "c1" })).results[0]!.atom;
    expect(mine.scope).toEqual({ kind: "conversation", key: "c1" });
    expect(ids(await f.search("ocelot caching", { conversationId: "c1" }))).toContain(mine.id);
    expect(ids(await f.search("ocelot caching", { conversationId: "c2" }))).not.toContain(mine.id);
    expect(ids(await f.search("ocelot caching", { project: "gamma" }))).not.toContain(mine.id);
  });

  it("falls back to Global (flagged) when a narrower scope has no key", async () => {
    const atom = (await f.remember({ content: "Orphan project claim about narwhals", scope: "project" })).results[0]!.atom;
    expect(atom.scope.kind).toBe("global");
    expect(atom.metadata.scopeUnresolved).toMatch(/project/);
  });

  it("scopes Files by metadata and by the Atoms they are docked to", async () => {
    const open = await f.createFile({ name: "open", content: "walrus handbook general", mimeType: "text/plain" });
    const tagged = await f.createFile({ name: "tagged", content: "walrus handbook for epsilon", mimeType: "text/plain", metadata: { project: "epsilon" } });
    const docked = await f.createFile({ name: "docked", content: "walrus handbook from zeta note", mimeType: "text/plain" });
    const atom = (await f.remember({ content: "Zeta walrus rule", project: "zeta" })).results[0]!.atom;
    await f.dockAtom(atom.id, docked.id);

    const inEta = ids(await f.search("walrus handbook", { project: "eta", limit: 20 }));
    expect(inEta).toContain(open.id);
    expect(inEta).not.toContain(tagged.id);
    expect(inEta).not.toContain(docked.id);

    const inZeta = ids(await f.search("walrus handbook", { project: "zeta", limit: 20 }));
    expect(inZeta).toContain(docked.id);
    expect(inZeta).not.toContain(tagged.id);
  });

  it("refuses to merge Atoms across scopes", async () => {
    const a = (await f.createAtom({ content: "merge me please", scope: { kind: "project", key: "p1" } })).atom;
    const b = (await f.createAtom({ content: "merge me please now", scope: { kind: "project", key: "p2" } })).atom;
    await expect(f.mergeAtoms(a.id, b.id)).rejects.toThrow(/scopes/);
  });

  it("returns an empty result instead of padding with unrelated memories", async () => {
    expect(await f.search("quantum chromodynamics lattice gauge", { project: "alpha" })).toEqual([]);
  });
});
