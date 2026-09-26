import "dotenv/config";
import pg from "pg";
import { loadConfig } from "../dist/config.js";
import { Database } from "../dist/db.js";
import { Foundation } from "../dist/foundation.js";

class DeterministicAIProvider {
  identity() {
    return { provider: "openai", model: "smoke-3072", dimensions: 3072 };
  }

  async embed(text) {
    const vector = Array(3072).fill(0);
    if (text.includes("파스타") || text.includes("파이어볼")) vector[0] = 1;
    if (text.includes("도쿄")) vector[1] = 0.5;
    if (vector[0] === 0 && vector[1] === 0) vector[2] = 1;
    return {
      vector,
      provider: "openai",
      model: "smoke-3072",
      dimensions: 3072
    };
  }

  async extractNote() {
    return {
      subject: "도쿄의 맛있는 파스타집",
      tags: ["도쿄", "파스타", "맛집"],
      atoms: [
        { content: "사용자는 오늘 도쿄에 있었다", tags: ["도쿄", "위치"] },
        { content: "도쿄에는 파이어볼 파스타집이 있다", tags: ["도쿄", "파스타"] },
        { content: "사용자는 파이어볼 파스타집에서 식사했다", tags: ["식사", "파스타"] },
        { content: "사용자는 파이어볼 파스타집을 맛있다고 평가했다", tags: ["맛집", "파스타"] }
      ]
    };
  }
}

const base = new URL(process.env.DATABASE_URL ?? "");
if (!base.protocol.startsWith("postgres")) {
  throw new Error("DATABASE_URL must point to PostgreSQL");
}

const testDb = `foundation_smoke_${process.pid}`.replace(/[^a-zA-Z0-9_]/g, "_");
const admin = new pg.Client({ connectionString: base.toString() });
await admin.connect();
await admin.query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname=$1 AND pid<>pg_backend_pid()", [testDb]);
await admin.query(`DROP DATABASE IF EXISTS "${testDb}"`);
await admin.query(`CREATE DATABASE "${testDb}"`);
await admin.end();

const testUrl = new URL(base.toString());
testUrl.pathname = "/" + testDb;
process.env.DATABASE_URL = testUrl.toString();

let db;
try {
  db = new Database(loadConfig());
  await db.initialize();
  const foundation = new Foundation(db, new DeterministicAIProvider(), "Asia/Seoul");

  const ingested = await foundation.ingestNote({
    rawText: "도쿄 파이어볼 파스타집 맛있는듯",
    name: "tokyo-note",
    vaultUid: "obsidian:test",
    sourceTime: "2026-09-23T12:00:00+09:00"
  });
  const first = ingested.file;
  const second = await foundation.createFile({
    name: "trip-note",
    vaultUid: "obsidian:test",
    mimeType: "text/plain",
    content: "도쿄 여행 기록"
  });

  await foundation.updateFile(first.id, {
    content: "도쿄 파이어볼 파스타집 진짜 맛있는듯",
    subject: "도쿄 파스타 맛집",
    metadata: { edited: true }
  });
  await foundation.linkFiles({
    aFileId: first.id,
    bFileId: second.id,
    subject: "같은 도쿄 여행",
    absolute: false,
    atomIds: [ingested.atomIds[0]]
  });

  const loadedFile = await foundation.getFile(first.id);
  const loadedVault = await foundation.getVault("obsidian:test");
  const search = await foundation.search("파이어볼 파스타", 5);
  const actions = loadedFile.history.map(item => item.action);
  const stats = await foundation.stats();

  const verdict = {
    isolatedDatabase: testDb,
    extractedAtomCount: ingested.atomIds.length,
    fileAtomCount: loadedFile.atoms.length,
    hasCreated: actions.includes("created"),
    hasDocked: actions.filter(action => action === "atom_docked").length === 4,
    hasUpdated: actions.includes("updated"),
    hasLinked: actions.includes("file_linked"),
    vaultFileCount: loadedVault.files.length,
    searchTopRawText: search[0]?.rawText ?? null,
    searchMatchedBy: search[0]?.matchedBy ?? [],
    statsAtoms: stats.atoms,
    embedding: stats.embedding
  };

  console.log(JSON.stringify(verdict, null, 2));

  if (verdict.extractedAtomCount !== 4 || verdict.fileAtomCount !== 4) {
    throw new Error("Raw note extraction/docking did not produce four top-level Atoms");
  }
  if (!verdict.hasCreated || !verdict.hasDocked || !verdict.hasUpdated || !verdict.hasLinked) {
    throw new Error("Modification history verification failed");
  }
  if (verdict.vaultFileCount !== 2) throw new Error("Vault did not hydrate both Files");
  if (verdict.searchTopRawText !== "도쿄 파이어볼 파스타집 진짜 맛있는듯") {
    throw new Error("Search did not resolve derived evidence to the updated raw File text");
  }
  if (!verdict.searchMatchedBy.includes("vector") || !verdict.searchMatchedBy.includes("atom")) {
    throw new Error("Search did not exercise pgvector and Atom-to-File evidence");
  }
} finally {
  if (db) await db.close().catch(() => undefined);
  const cleanup = new pg.Client({ connectionString: base.toString() });
  await cleanup.connect();
  await cleanup.query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname=$1 AND pid<>pg_backend_pid()", [testDb]);
  await cleanup.query(`DROP DATABASE IF EXISTS "${testDb}"`);
  await cleanup.end();
}
