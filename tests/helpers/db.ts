import { randomBytes } from "node:crypto";
import pg from "pg";
import type { Config } from "../../src/config.js";
import { Database } from "../../src/db.js";

export const TEST_DATABASE_URL = process.env.TEST_DATABASE_URL ?? "";
export const hasTestDatabase = TEST_DATABASE_URL.length > 0;

function withDatabase(url: string, name: string): string {
  const parsed = new URL(url);
  parsed.pathname = "/" + name;
  return parsed.toString();
}

export interface TestDb {
  db: Database;
  url: string;
  name: string;
  drop(): Promise<void>;
}

/** Creates a throwaway database so destructive tests never touch real data. */
export async function createTestDb(options: { initialize?: boolean } = {}): Promise<TestDb> {
  const name = "foundation_test_" + randomBytes(5).toString("hex");
  const admin = new pg.Client({ connectionString: withDatabase(TEST_DATABASE_URL, "postgres") });
  await admin.connect();
  await admin.query(`CREATE DATABASE ${name}`);
  await admin.end();

  const url = withDatabase(TEST_DATABASE_URL, name);
  const db = new Database({ databaseUrl: url, databasePoolSize: 8 } as Config);
  if (options.initialize !== false) await db.initialize();

  return {
    db,
    url,
    name,
    async drop() {
      await db.close();
      const cleanup = new pg.Client({ connectionString: withDatabase(TEST_DATABASE_URL, "postgres") });
      await cleanup.connect();
      await cleanup.query(`DROP DATABASE IF EXISTS ${name} WITH (FORCE)`);
      await cleanup.end();
    }
  };
}
