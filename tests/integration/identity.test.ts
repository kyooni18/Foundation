import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { Foundation } from "../../src/foundation.js";
import { createTestDb, hasTestDatabase, type TestDb } from "../helpers/db.js";
import { FakeAI } from "../helpers/fake-ai.js";

describe.skipIf(!hasTestDatabase)("recall/forget identity", () => {
  let t: TestDb;
  let ai: FakeAI;
  let f: Foundation;

  beforeAll(async () => {
    t = await createTestDb();
    ai = new FakeAI();
    f = new Foundation(t.db, ai, "UTC");
  });
  afterAll(async () => { await t.drop(); });

  it("returns the Atom id for a standalone Atom and forget removes exactly it", async () => {
    const { atom } = await f.createAtom({ content: "Standalone zebra migration checklist", metadata: {} });
    const [hit] = await f.search("zebra migration checklist");
    expect(hit?.kind).toBe("atom");
    expect(hit?.id).toBe(atom.id);
    expect(await f.forget(hit!.id)).toEqual({ id: atom.id, forgotten: true });
    expect(await f.search("zebra migration checklist")).toEqual([]);
  });

  it("returns Atom ids (never File ids) for docked Atoms and keeps File evidence", async () => {
    const file = await f.createFile({ name: "Meeting", content: "Raw meeting transcript about quokka budget", mimeType: "text/plain" });
    const { atom } = await f.createAtom({ content: "The quokka budget is 40k", metadata: {} });
    await f.dockAtom(atom.id, file.id);

    const hits = await f.search("quokka budget");
    const atomHit = hits.find(item => item.kind === "atom");
    expect(atomHit?.id).toBe(atom.id);
    expect(atomHit?.sources?.map(source => source.fileId)).toEqual([file.id]);
    // The File is folded into the Atom as provenance unless it matched strictly better.
    for (const item of hits.filter(item => item.kind === "file")) expect(item.id).not.toBe(atom.id);

    await f.forget(atomHit!.id);
    expect((await f.search("quokka budget")).filter(item => item.kind === "atom")).toEqual([]);
    // Source File is preserved and still independently searchable.
    const fileHits = await f.search("Raw meeting transcript quokka");
    expect(fileHits.some(item => item.kind === "file" && item.id === file.id)).toBe(true);
    expect((await f.getFile(file.id)).content).toContain("quokka");
  });

  it("keeps several Atoms of one File individually forgettable", async () => {
    const file = await f.createFile({ name: "Note", content: "Note about platypus hosting and platypus billing", mimeType: "text/plain" });
    const a = (await f.createAtom({ content: "Platypus hosting runs on Fly", metadata: {} })).atom;
    const b = (await f.createAtom({ content: "Platypus billing uses Stripe", metadata: {} })).atom;
    await f.dockAtom(a.id, file.id);
    await f.dockAtom(b.id, file.id);

    const hosting = (await f.search("platypus hosting Fly")).find(item => item.kind === "atom" && item.id === a.id);
    expect(hosting).toBeTruthy();
    await f.forget(a.id);

    const remaining = await f.search("platypus billing Stripe");
    expect(remaining.some(item => item.id === b.id)).toBe(true);
    expect((await f.search("platypus hosting Fly")).some(item => item.id === a.id)).toBe(false);
  });

  it("rejects a File id passed to forget instead of silently doing nothing", async () => {
    const file = await f.createFile({ name: "Doc", content: "kept as evidence", mimeType: "text/plain" });
    await expect(f.forget(file.id)).rejects.toThrow(/File/);
    expect(await f.forget("11111111-1111-4111-8111-111111111111")).toEqual({ id: "11111111-1111-4111-8111-111111111111", forgotten: false });
  });
});
