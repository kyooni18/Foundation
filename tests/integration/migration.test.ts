import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { schemaStatements } from "../../src/schema.js";
import { schemaStatements as v6Statements } from "../fixtures/schema-v6.js";
import { createTestDb, hasTestDatabase, type TestDb } from "../helpers/db.js";

describe.skipIf(!hasTestDatabase)("schema v6 -> v7 migration", () => {
  let t: TestDb;
  const ids = {
    global: "00000000-0000-4000-8000-000000000001",
    project: "00000000-0000-4000-8000-000000000002",
    superseded: "00000000-0000-4000-8000-000000000003",
    superseder: "00000000-0000-4000-8000-000000000004",
    forgotten: "00000000-0000-4000-8000-000000000005",
    expiring: "00000000-0000-4000-8000-000000000006",
    unresolved: "00000000-0000-4000-8000-000000000007",
    file: "00000000-0000-4000-8000-0000000000f1"
  };

  beforeAll(async () => {
    t = await createTestDb({ initialize: false });
    for (const statement of v6Statements) await t.db.query(statement);
    const insert = (id: string, content: string, metadata: object) => t.db.query(
      `INSERT INTO atoms(id,content,normalized_content,content_hash,type,metadata)
       VALUES($1,$2,lower($2),encode(sha256($2::bytea),'hex'),'usercreated',$3::jsonb)`,
      [id, content, JSON.stringify(metadata)]
    );
    await insert(ids.global, "Global preference", { active: true, scope: "Global" });
    await insert(ids.project, "Project scoped decision", { active: true, scope: "Project", project: "Mun" });
    await insert(ids.superseded, "Old decision", { active: false, supersededBy: ids.superseder, validFrom: "2024-01-01T00:00:00Z" });
    await insert(ids.superseder, "New decision", { active: true, validFrom: "2025-01-01T00:00:00Z" });
    await insert(ids.forgotten, "Forgotten thing", { active: false });
    await insert(ids.expiring, "Time boxed", { active: true, expires: "2020-01-01T00:00:00Z", validFrom: "garbage" });
    await insert(ids.unresolved, "Project claim w/o project", { active: true, scope: "Project" });
    await t.db.query("INSERT INTO atom_relations(from_atom_id,to_atom_id,relation) VALUES($1,$2,'supersedes')", [ids.superseder, ids.superseded]);
    await t.db.query("INSERT INTO files(id,name,content) VALUES($1,'note','body')", [ids.file]);
    await t.db.query("INSERT INTO atom_file_docks(atom_id,file_id) VALUES($1,$2)", [ids.project, ids.file]);
    await t.db.query("INSERT INTO external_bindings(system,entity_kind,entity_id,external_id) VALUES('notion','atom',$1,'page-1')", [ids.global]);
    await t.db.query("INSERT INTO foundation_schema(version) VALUES (6) ON CONFLICT DO NOTHING");
  });
  afterAll(async () => { await t.drop(); });

  it("preserves ids, relations, docks and bindings while backfilling scope, status and validity", async () => {
    await t.db.initialize();
    const rows = Object.fromEntries((await t.db.query("SELECT id,scope_kind,scope_key,status,valid_from,expires_at,metadata FROM atoms")).rows.map(row => [row.id, row]));

    expect(Object.keys(rows).sort()).toEqual(Object.values(ids).filter(id => id !== ids.file).sort());
    expect(rows[ids.global]).toMatchObject({ scope_kind: "global", scope_key: "", status: "active" });
    expect(rows[ids.project]).toMatchObject({ scope_kind: "project", scope_key: "mun", status: "active" });
    expect(rows[ids.superseded]).toMatchObject({ status: "superseded" });
    expect(rows[ids.superseded].expires_at.toISOString()).toBe("2025-01-01T00:00:00.000Z");
    expect(rows[ids.forgotten]).toMatchObject({ status: "forgotten" });
    expect(rows[ids.expiring].expires_at.toISOString()).toBe("2020-01-01T00:00:00.000Z");
    expect(rows[ids.expiring].valid_from).toBeNull();
    expect(rows[ids.unresolved]).toMatchObject({ scope_kind: "global" });
    expect(rows[ids.unresolved].metadata.scopeUnresolved).toBe(true);

    const count = async (sql: string) => Number((await t.db.query(sql)).rows[0]!.n);
    expect(await count("SELECT count(*) n FROM atom_relations")).toBe(1);
    expect(await count("SELECT count(*) n FROM atom_file_docks")).toBe(1);
    expect(await count("SELECT count(*) n FROM external_bindings WHERE external_id='page-1'")).toBe(1);
    expect(await count("SELECT count(*) n FROM atoms_backup_v6")).toBe(7);
  });

  it("allows identical content in different scopes after migration, but not within one", async () => {
    const insert = (scopeKind: string, scopeKey: string) => t.db.query(
      `INSERT INTO atoms(content,normalized_content,content_hash,type,scope_kind,scope_key)
       VALUES('Global preference','global preference',encode(sha256('Global preference'::bytea),'hex'),'usercreated',$1,$2)`,
      [scopeKind, scopeKey]
    );
    await insert("project", "other");
    await expect(insert("project", "other")).rejects.toThrow(/duplicate key/);
  });

  it("is idempotent when run again and keeps migrated data", async () => {
    await t.db.initialize();
    await t.db.initialize();
    const n = Number((await t.db.query("SELECT count(*) n FROM atoms_backup_v6")).rows[0]!.n);
    expect(n).toBe(7);
    expect(schemaStatements.length).toBeGreaterThan(0);
  });
});
