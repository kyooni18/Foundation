import pg, { type PoolClient, type QueryResult, type QueryResultRow } from "pg";
import type { Config } from "./config.js";
import { SCHEMA_VERSION, schemaStatements } from "./schema.js";

const { Pool } = pg;

export class Database {
  readonly pool: pg.Pool;

  constructor(config: Config) {
    this.pool = new Pool({ connectionString: config.databaseUrl, max: config.databasePoolSize, application_name: "foundation" });
  }

  async initialize(): Promise<void> {
    const client = await this.pool.connect();
    try {
      await client.query("BEGIN");
      await client.query("SELECT pg_advisory_xact_lock(hashtext('foundation-schema-v1'))");
      for (const statement of schemaStatements) await client.query(statement);
      await client.query("COMMIT");
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }

  query<T extends QueryResultRow = QueryResultRow>(text: string, values: unknown[] = []): Promise<QueryResult<T>> {
    return this.pool.query<T>(text, values);
  }

  async transaction<T>(work: (client: PoolClient) => Promise<T>): Promise<T> {
    const client = await this.pool.connect();
    try {
      await client.query("BEGIN");
      const result = await work(client);
      await client.query("COMMIT");
      return result;
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }
  }


  async health(): Promise<Record<string, unknown>> {
    const result = await this.query<{ database: string; atoms: string; files: string; vaults: string }>(`
      SELECT current_database() AS database,
        (SELECT count(*)::text FROM atoms) AS atoms,
        (SELECT count(*)::text FROM files) AS files,
        (SELECT count(*)::text FROM vaults) AS vaults
    `);
    return { ok: true, schemaVersion: SCHEMA_VERSION, ...result.rows[0] };
  }

  close(): Promise<void> { return this.pool.end(); }
}
