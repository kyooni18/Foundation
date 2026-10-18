import { describe, expect, it } from "vitest";
import { schemaStatements } from "../../src/schema.js";
import { createTestDb, hasTestDatabase } from "../helpers/db.js";

const repair = schemaStatements.find(s => s.startsWith("DO $foundation_v8_repair$"))!;
describe.skipIf(!hasTestDatabase)("v8 scope-key repair", () => {
  async function fixture() {
    const t = await createTestDb();
    await t.db.query("DELETE FROM foundation_schema WHERE version >= 8");
    await t.db.query("INSERT INTO foundation_schema(version) VALUES (7) ON CONFLICT DO NOTHING");
    const insert = async (raw: string, key: string, text = raw, kind = "project", status = "active") =>
      (await t.db.query(`INSERT INTO atoms(content,normalized_content,content_hash,type,metadata,scope_kind,scope_key,status,valid_from,expires_at)
        VALUES($1,lower($1),encode(sha256($1::bytea),'hex'),'imported',$2,$3,$4,$5,'2024-01-01','2030-01-01') RETURNING id`,
      [text, JSON.stringify(kind === "project" ? { project: raw } : { conversationId: raw }), kind, key, status])).rows[0]!.id;
    return { t, insert };
  }
  it("repairs only proven transforms, preserves every other column, and supports rollback and repeat initialization", async () => {
    const { t, insert } = await fixture();
    try {
      await insert("KSL", "k l");
      await insert("KSPShuttleLander", "k p huttlelander", "historical", "project", "superseded");
      await insert("StructToolingAB", " tructtoolingab", "forgotten", "project", "forgotten");
      await insert("Session", " e ion", "chat", "conversation");
      await insert("KSL", "legitimate-custom-scope", "custom");
      await insert("KSL", "ksl", "already correct");
      const before = (await t.db.query("SELECT id, to_jsonb(a) - 'scope_key' AS data, scope_key FROM atoms a ORDER BY id")).rows;
      await t.db.transaction(async c => {
        await c.query("SAVEPOINT repair");
        await c.query(repair);
        expect((await c.query("SELECT count(*)::int n FROM atom_scope_repairs_v8")).rows[0]!.n).toBe(4);
        await c.query("ROLLBACK TO repair");
      });
      expect((await t.db.query("SELECT id, to_jsonb(a) - 'scope_key' AS data, scope_key FROM atoms a ORDER BY id")).rows).toEqual(before);
      await t.db.initialize();
      await t.db.initialize();
      expect((await t.db.query("SELECT id, to_jsonb(a) - 'scope_key' AS data FROM atoms a ORDER BY id")).rows)
        .toEqual(before.map(({id,data}) => ({id,data})));
      const keys = (await t.db.query("SELECT scope_key FROM atoms ORDER BY content")).rows.map(r => r.scope_key);
      expect(keys).toContain("kspshuttlelander");
      expect(keys).toContain("structtoolingab");
      expect(keys).toContain("session");
      expect(keys).toContain("legitimate-custom-scope");
    } finally { await t.drop(); }
  });
  it("aborts atomically on a unique-index collision", async () => {
    const {t,insert} = await fixture();
    try {
      await insert("KSL", "k l", "twin"); await insert("KSL", "ksl", "twin");
      await expect(t.db.initialize()).rejects.toThrow(/collide/);
      expect((await t.db.query("SELECT count(*)::int n FROM atoms WHERE scope_key='k l'")).rows[0]!.n).toBe(1);
      expect((await t.db.query("SELECT count(*)::int n FROM foundation_schema WHERE version=8")).rows[0]!.n).toBe(0);
    } finally { await t.drop(); }
  });
  it("refuses ambiguous original metadata rather than guessing", async () => {
    const {t,insert} = await fixture();
    try {
      await insert("KSL", "k l");
      await t.db.query("INSERT INTO atoms_backup_v6(id,metadata) SELECT id, '{\"project\":\"Another\"}'::jsonb metadata FROM atoms");
      await expect(t.db.initialize()).rejects.toThrow(/metadata disagree/);
    } finally { await t.drop(); }
  });
  it("v7 SQL retains whitespace matching through JS evaluation, including conversation scope", () => {
    const v7 = schemaStatements.find(s => s.startsWith("DO $foundation_v7_backfill$"))!;
    expect(v7.match(/'\[\[:space:\]\]\+'/g)).toHaveLength(2);
    expect(v7).not.toContain("'s+'");
  });
});
