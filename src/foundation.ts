import type { PoolClient, QueryResult, QueryResultRow } from "pg";
import type { AIProvider, EmbeddingResult, MemoryResolution, MemoryResolutionCandidate } from "./ai.js";
import type { Database } from "./db.js";
import type { FileInterpreter } from "./interpreter.js";
import type { Atom, AtomStatus, AtomType, EmbeddingProvenance, FileLink, FileObject, SearchItem, Vault } from "./types.js";
import { resolveWriteScope, scopeLabel, type MemoryScope, type ScopeContext } from "./scope.js";
import { newSearchTrace, searchFoundation, type SearchOptions, type SearchTrace } from "./search.js";
import { contentHash, normalizeTags, normalizeText, record, vectorLiteral } from "./utils.js";

function embeddingProvenanceFromRow(row: QueryResultRow, prefix: string): EmbeddingProvenance | null {
  const provider = row[`${prefix}_provider`];
  const model = row[`${prefix}_model`];
  const dimensions = row[`${prefix}_dimensions`];
  const embeddedAt = row[`${prefix}_embedded_at`];

  if (
    provider === null || provider === undefined ||
    model === null || model === undefined ||
    dimensions === null || dimensions === undefined ||
    embeddedAt === null || embeddedAt === undefined
  ) {
    return null;
  }

  return {
    provider: String(provider),
    model: String(model),
    dimensions: Number(dimensions),
    embeddedAt: new Date(embeddedAt)
  };
}

function stringOrNull(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function parseTimestamp(value: unknown): Date | null {
  if (value === null || value === undefined || value === "") return null;
  const date = value instanceof Date ? value : new Date(String(value));
  return Number.isFinite(date.getTime()) ? date : null;
}

interface Queryable {
  query(text: string, values?: unknown[]): Promise<QueryResult<any>>;
}

function atomFromRow(row: QueryResultRow): Atom {
  return {
    id: String(row.id),
    scope: { kind: (row.scope_kind ?? "global") as MemoryScope["kind"], key: String(row.scope_key ?? "") },
    status: (row.status ?? "active") as AtomStatus,
    validFrom: row.valid_from ? new Date(row.valid_from) : null,
    expiresAt: row.expires_at ? new Date(row.expires_at) : null,
    content: String(row.content),
    normalizedContent: String(row.normalized_content),
    tags: Array.isArray(row.tags) ? row.tags.map(String) : [],
    type: row.type as AtomType,
    metadata: record(row.metadata),
    embedding: embeddingProvenanceFromRow(row, "vector"),
    createdAt: new Date(row.created_at),
    updatedAt: new Date(row.updated_at)
  };
}

function fileFromRow(row: QueryResultRow): FileObject {
  const data = row.data;
  return {
    id: String(row.id),
    vaultId: row.vault_id ? String(row.vault_id) : null,
    name: String(row.name),
    mimeType: row.mime_type ? String(row.mime_type) : null,
    base64: Buffer.isBuffer(data) ? data.toString("base64") : null,
    content: row.content === null ? null : String(row.content),
    interpreted: row.interpreted === null ? null : String(row.interpreted),
    subject: row.subject === null ? null : String(row.subject),
    tags: Array.isArray(row.tags) ? row.tags.map(String) : [],
    subjectEmbedding: embeddingProvenanceFromRow(row, "vector_subject"),
    contentEmbedding: embeddingProvenanceFromRow(row, "vector_content"),
    metadata: record(row.metadata),
    createdAt: new Date(row.created_at),
    modifiedAt: new Date(row.modified_at)
  };
}

function vaultFromRow(row: QueryResultRow): Vault {
  return {
    id: String(row.id),
    vaultUid: String(row.vault_uid),
    name: row.name === null ? null : String(row.name),
    metadata: record(row.metadata),
    createdAt: new Date(row.created_at),
    updatedAt: new Date(row.updated_at)
  };
}

function fileLinkFromRow(row: QueryResultRow): FileLink {
  return {
    id: String(row.id),
    aFileId: String(row.a_file_id),
    bFileId: String(row.b_file_id),
    subject: row.subject === null ? null : String(row.subject),
    absolute: Boolean(row.absolute),
    content: row.content === null ? null : String(row.content),
    createdAt: new Date(row.created_at),
    modifiedAt: new Date(row.modified_at)
  };
}

export interface RememberInput {
  content: string;
  sourceTime?: string;
  requestId?: string;
  tags?: string[];
  metadata?: Record<string, unknown>;
  /** Explicit storage scope. Project/conversation scopes need `project` / `conversationId`. */
  scope?: "global" | "project" | "conversation";
  project?: string;
  conversationId?: string;
}

interface AtomPlan {
  content: string;
  storedContent: string;
  tags: string[];
  metadata: Record<string, unknown>;
  scope: MemoryScope;
  scopeUnresolved: string | null;
  candidates: MemoryResolutionCandidate[];
  resolution: MemoryResolution;
  target: MemoryResolutionCandidate | null;
  embedding: EmbeddingResult | null;
  embeddingError: string | null;
}

export interface RememberAtomicResult {
  action: MemoryResolution["action"];
  targetAtomId: string | null;
  atom: Atom;
  confidence: number;
  rationale: string;
  embeddingDeferred: boolean;
  candidates: Array<{ id: string; similarity: number }>;
}

export interface RememberResult {
  requestId: string | null;
  sourceTime: string;
  decomposed: boolean;
  results: RememberAtomicResult[];
}

export class Foundation {
  constructor(
    private readonly db: Database,
    private readonly ai: AIProvider,
    private readonly timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
    private readonly interpreter?: FileInterpreter,
    private readonly defaults: { project?: string | null } = {}
  ) {}
  private readonly rememberInFlight = new Map<string, { content: string; promise: Promise<RememberResult> }>();
  async createVault(input: {
    vaultUid: string;
    name?: string | null;
    metadata?: Record<string, unknown>;
  }): Promise<Vault> {
    const vaultUid = normalizeText(input.vaultUid);
    if (!vaultUid) throw new Error("vaultUid must not be empty");
    const result = await this.db.query(
      `INSERT INTO vaults (vault_uid,name,metadata)
       VALUES ($1,$2,$3::jsonb)
       ON CONFLICT (vault_uid) DO UPDATE SET
         name=COALESCE(EXCLUDED.name,vaults.name),
         metadata=vaults.metadata || EXCLUDED.metadata,
         updated_at=NOW()
       RETURNING *`,
      [vaultUid, input.name ?? null, JSON.stringify(input.metadata ?? {})]
    );
    return vaultFromRow(result.rows[0]!);
  }

  async getVault(idOrUid: string): Promise<Vault & {
    files: FileObject[];
    modifications: Array<Record<string, unknown>>;
  }> {
    const result = await this.db.query(
      "SELECT * FROM vaults WHERE id::text=$1 OR vault_uid=$1 LIMIT 1",
      [idOrUid]
    );
    if (!result.rows[0]) throw new Error("Vault not found");
    const vault = vaultFromRow(result.rows[0]);
    const [files, modifications] = await Promise.all([
      this.db.query("SELECT * FROM files WHERE vault_id=$1 ORDER BY modified_at DESC", [vault.id]),
      this.db.query("SELECT id,file_id,action,payload,time FROM modifications WHERE vault_id=$1 ORDER BY time DESC", [vault.id])
    ]);
    return {
      ...vault,
      files: files.rows.map(fileFromRow),
      modifications: modifications.rows.map(row => ({
        id: String(row.id),
        fileId: row.file_id ? String(row.file_id) : null,
        action: String(row.action),
        payload: record(row.payload),
        time: new Date(row.time).toISOString()
      }))
    };
  }

  private async resolveVaultId(vaultUid?: string | null): Promise<string | null> {
    if (!vaultUid) return null;
    return (await this.createVault({ vaultUid })).id;
  }

  async createAtom(input: {
    content: string;
    type?: AtomType;
    tags?: string[];
    metadata?: Record<string, unknown>;
    vector?: number[] | null;
    embedding?: EmbeddingResult | null;
    scope?: MemoryScope;
    status?: AtomStatus;
    validFrom?: Date | string | null;
    expiresAt?: Date | string | null;
    /** Explicit user intent to bring a previously forgotten identical memory back. */
    reviveForgotten?: boolean;
  }, client?: Queryable): Promise<{ atom: Atom; created: boolean }> {
    const content = normalizeText(input.content);
    if (!content) throw new Error("Atom content must not be empty");
    const normalized = content.toLowerCase();
    const hash = contentHash(content);
    const tags = normalizeTags(input.tags ?? []);
    const metadata = input.metadata ?? {};
    const scope = input.scope ?? resolveWriteScope({ metadata, extractedScope: stringOrNull(metadata.scope) }).scope;
    const status = input.status ?? "active";
    const validFrom = parseTimestamp(input.validFrom !== undefined ? input.validFrom : metadata.validFrom);
    const expiresAt = parseTimestamp(input.expiresAt !== undefined ? input.expiresAt : metadata.expires);

    const generated = input.embedding !== undefined
      ? input.embedding
      : input.vector === undefined
        ? await this.ai.embed(content, "document")
        : input.vector === null
          ? null
          : {
              vector: input.vector,
              provider: "manual" as const,
              model: "manual",
              dimensions: 3072 as const
            };

    const result = await (client ?? this.db).query(
      `INSERT INTO atoms (
         content,normalized_content,content_hash,
         vector,vector_provider,vector_model,vector_dimensions,vector_embedded_at,
         type,tags,metadata,scope_kind,scope_key,status,valid_from,expires_at
       )
       VALUES (
         $1,$2,$3,
         $4::halfvec,$5,$6,$7,CASE WHEN $4::halfvec IS NULL THEN NULL ELSE NOW() END,
         $8,$9::text[],($10::jsonb || jsonb_build_object('active', $13::text = 'active')),$11,$12,$13,$14::timestamptz,$15::timestamptz
       )
       ON CONFLICT (scope_kind, scope_key, content_hash) WHERE status <> 'superseded' DO UPDATE SET
         tags=ARRAY(SELECT DISTINCT value FROM unnest(atoms.tags || EXCLUDED.tags) AS value ORDER BY value),
         vector=COALESCE(EXCLUDED.vector,atoms.vector),
         vector_provider=CASE WHEN EXCLUDED.vector IS NULL THEN atoms.vector_provider ELSE EXCLUDED.vector_provider END,
         vector_model=CASE WHEN EXCLUDED.vector IS NULL THEN atoms.vector_model ELSE EXCLUDED.vector_model END,
         vector_dimensions=CASE WHEN EXCLUDED.vector IS NULL THEN atoms.vector_dimensions ELSE EXCLUDED.vector_dimensions END,
         vector_embedded_at=CASE WHEN EXCLUDED.vector IS NULL THEN atoms.vector_embedded_at ELSE EXCLUDED.vector_embedded_at END,
         status=CASE WHEN atoms.status='forgotten' AND $16::boolean THEN 'active' ELSE atoms.status END,
         valid_from=CASE WHEN atoms.status='forgotten' AND $16::boolean THEN EXCLUDED.valid_from ELSE COALESCE(atoms.valid_from,EXCLUDED.valid_from) END,
         expires_at=CASE WHEN atoms.status='forgotten' AND $16::boolean THEN EXCLUDED.expires_at ELSE atoms.expires_at END,
         metadata=(atoms.metadata || (EXCLUDED.metadata - 'active')) || jsonb_build_object(
           'active', CASE WHEN atoms.status='forgotten' AND $16::boolean THEN TRUE ELSE atoms.status='active' END),
         updated_at=NOW()
       RETURNING atoms.*, (xmax = 0) AS inserted`,
      [
        content,
        normalized,
        hash,
        vectorLiteral(generated?.vector ?? null),
        generated?.provider ?? null,
        generated?.model ?? null,
        generated?.dimensions ?? null,
        input.type ?? "usercreated",
        tags,
        JSON.stringify(metadata),
        scope.kind,
        scope.key,
        status,
        validFrom,
        expiresAt,
        input.reviveForgotten ?? false
      ]
    );
    return { atom: atomFromRow(result.rows[0]!), created: Boolean(result.rows[0]!.inserted) };
  }

  async updateAtom(id: string, input: {
    content?: string;
    type?: AtomType;
    tags?: string[];
    metadata?: Record<string, unknown>;
    active?: boolean;
    status?: AtomStatus;
    validFrom?: Date | string | null;
    expiresAt?: Date | string | null;
    embedding?: EmbeddingResult | null;
  }, client?: Queryable): Promise<Atom> {
    const q = client ?? this.db;
    const currentResult = await q.query(`SELECT * FROM atoms WHERE id=$1${client ? " FOR UPDATE" : ""}`, [id]);
    if (!currentResult.rows[0]) throw new Error("Atom not found");
    const current = atomFromRow(currentResult.rows[0]);

    const contentChanged = input.content !== undefined && normalizeText(input.content) !== current.content;
    const nextContent = input.content === undefined ? current.content : normalizeText(input.content);
    if (!nextContent) throw new Error("Atom content must not be empty");

    const embedding = contentChanged
      ? (input.embedding !== undefined ? input.embedding : await this.ai.embed(nextContent, "document"))
      : null;
    const sets = ["updated_at=NOW()"];
    const values: unknown[] = [id];
    let index = 2;
    const add = (column: string, value: unknown) => {
      sets.push(`${column}=$${index++}`);
      values.push(value);
    };

    if (contentChanged) {
      add("content", nextContent);
      add("normalized_content", nextContent.toLowerCase());
      add("content_hash", contentHash(nextContent));
      sets.push(`vector=$${index++}::halfvec`);
      values.push(vectorLiteral(embedding?.vector ?? null));
      add("vector_provider", embedding?.provider ?? null);
      add("vector_model", embedding?.model ?? null);
      add("vector_dimensions", embedding?.dimensions ?? null);
      sets.push(`vector_embedded_at=${embedding ? "NOW()" : "NULL"}`);
    }
    if (input.type !== undefined) add("type", input.type);
    if (input.tags !== undefined) {
      sets.push(`tags=$${index++}::text[]`);
      values.push(normalizeTags(input.tags));
    }

    // `active` is the legacy toggle. It never resurrects or destroys history on its own: a superseded
    // Atom stays superseded, only active <-> forgotten flips.
    let status: AtomStatus = input.status ?? current.status;
    if (input.status === undefined && input.active !== undefined && current.status !== "superseded") {
      status = input.active ? "active" : "forgotten";
    }
    if (status !== current.status) add("status", status);

    const incoming = input.metadata ?? {};
    // Columns are authoritative; metadata mirrors them (ISO strings) so Notion export and legacy readers agree.
    const temporalMirror: Record<string, unknown> = {};
    if (input.validFrom !== undefined || "validFrom" in incoming) {
      const validFrom = parseTimestamp(input.validFrom !== undefined ? input.validFrom : incoming.validFrom);
      add("valid_from", validFrom);
      temporalMirror.validFrom = validFrom?.toISOString() ?? null;
    }
    if (input.expiresAt !== undefined || "expires" in incoming) {
      const expiresAt = parseTimestamp(input.expiresAt !== undefined ? input.expiresAt : incoming.expires);
      add("expires_at", expiresAt);
      temporalMirror.expires = expiresAt?.toISOString() ?? null;
    }

    const { active: _ignored, scope: _scopeLabel, ...metadataPatch } = incoming;
    const patch = {
      ...metadataPatch,
      ...temporalMirror,
      active: status === "active",
      ...(status === "forgotten" && current.status !== "forgotten" ? { forgottenAt: new Date().toISOString() } : {})
    };
    sets.push(`metadata=metadata || $${index++}::jsonb`);
    values.push(JSON.stringify(patch));

    try {
      const result = await q.query(
        `UPDATE atoms SET ${sets.join(", ")} WHERE id=$1 RETURNING *`,
        values
      );
      return atomFromRow(result.rows[0]!);
    } catch (error) {
      if (error && typeof error === "object" && "code" in error && error.code === "23505") {
        throw new Error("An Atom with the same normalized content already exists in this scope; use atom_merge for explicit consolidation");
      }
      throw error;
    }
  }

  async getAtom(id: string): Promise<Atom & { dockedTo: string[] }> {
    const result = await this.db.query(
      `SELECT a.*, COALESCE(array_agg(d.file_id::text) FILTER (WHERE d.file_id IS NOT NULL), ARRAY[]::text[]) AS docked_to
       FROM atoms a
       LEFT JOIN atom_file_docks d ON d.atom_id=a.id
       WHERE a.id=$1
       GROUP BY a.id`,
      [id]
    );
    if (!result.rows[0]) throw new Error("Atom not found");
    return { ...atomFromRow(result.rows[0]), dockedTo: result.rows[0].docked_to as string[] };
  }

  async dockAtom(atomId: string, fileId: string, metadata: Record<string, unknown> = {}): Promise<void> {
    await this.db.transaction(async client => {
      await client.query(
        `INSERT INTO atom_file_docks (atom_id,file_id,metadata)
         VALUES ($1,$2,$3::jsonb)
         ON CONFLICT (atom_id,file_id) DO UPDATE SET metadata=atom_file_docks.metadata || EXCLUDED.metadata`,
        [atomId, fileId, JSON.stringify(metadata)]
      );
      const file = await client.query<{ vault_id: string | null }>("SELECT vault_id::text FROM files WHERE id=$1", [fileId]);
      await client.query(
        "INSERT INTO modifications (vault_id,file_id,action,payload) VALUES ($1,$2,'atom_docked',$3::jsonb)",
        [file.rows[0]?.vault_id ?? null, fileId, JSON.stringify({ atomId })]
      );
    });
  }

  async undockAtom(atomId: string, fileId: string): Promise<void> {
    await this.db.transaction(async client => {
      const removed = await client.query(
        "DELETE FROM atom_file_docks WHERE atom_id=$1 AND file_id=$2 RETURNING file_id",
        [atomId, fileId]
      );
      if (!removed.rowCount) return;
      const file = await client.query<{ vault_id: string | null }>("SELECT vault_id::text FROM files WHERE id=$1", [fileId]);
      await client.query(
        "INSERT INTO modifications (vault_id,file_id,action,payload) VALUES ($1,$2,'atom_undocked',$3::jsonb)",
        [file.rows[0]?.vault_id ?? null, fileId, JSON.stringify({ atomId })]
      );
    });
  }

  private async insertFile(
    client: PoolClient,
    input: {
      vaultId: string | null;
      name: string;
      mimeType?: string | null;
      base64?: string | null;
      content?: string | null;
      interpreted?: string | null;
      subject?: string | null;
      tags?: string[];
      metadata?: Record<string, unknown>;
      contentEmbedding?: EmbeddingResult | null;
      subjectEmbedding?: EmbeddingResult | null;
      action?: string;
    }
  ): Promise<FileObject> {
    const bytes = input.base64 ? Buffer.from(input.base64, "base64") : null;
    const subjectEmbedding = input.subjectEmbedding ?? null;
    const contentEmbedding = input.contentEmbedding ?? null;

    const result = await client.query(
      `INSERT INTO files (
         vault_id,name,mime_type,data,content,interpreted,subject,
         vector_subject,vector_subject_provider,vector_subject_model,vector_subject_dimensions,vector_subject_embedded_at,
         vector_content,vector_content_provider,vector_content_model,vector_content_dimensions,vector_content_embedded_at,
         tags,metadata
       )
       VALUES (
         $1,$2,$3,$4,$5,$6,$7,
         $8::halfvec,$9,$10,$11,CASE WHEN $8::halfvec IS NULL THEN NULL ELSE NOW() END,
         $12::halfvec,$13,$14,$15,CASE WHEN $12::halfvec IS NULL THEN NULL ELSE NOW() END,
         $16::text[],$17::jsonb
       )
       RETURNING *`,
      [
        input.vaultId,
        normalizeText(input.name) || "Untitled",
        input.mimeType ?? null,
        bytes,
        input.content ?? null,
        input.interpreted ?? null,
        input.subject ?? null,
        vectorLiteral(subjectEmbedding?.vector ?? null),
        subjectEmbedding?.provider ?? null,
        subjectEmbedding?.model ?? null,
        subjectEmbedding?.dimensions ?? null,
        vectorLiteral(contentEmbedding?.vector ?? null),
        contentEmbedding?.provider ?? null,
        contentEmbedding?.model ?? null,
        contentEmbedding?.dimensions ?? null,
        normalizeTags(input.tags ?? []),
        JSON.stringify(input.metadata ?? {})
      ]
    );
    const file = fileFromRow(result.rows[0]!);
    await client.query(
      "INSERT INTO modifications (vault_id,file_id,action,payload) VALUES ($1,$2,$3,$4::jsonb)",
      [file.vaultId, file.id, input.action ?? "created", JSON.stringify({ name: file.name, mimeType: file.mimeType })]
    );
    return file;
  }

  async createFile(input: {
    name: string;
    vaultUid?: string | null;
    mimeType?: string | null;
    base64?: string | null;
    content?: string | null;
    interpreted?: string | null;
    subject?: string | null;
    tags?: string[];
    metadata?: Record<string, unknown>;
  }): Promise<FileObject> {
    if (!input.content && !input.base64 && !input.interpreted) {
      throw new Error("A File needs content, interpreted text, or base64 data");
    }
    const vaultId = await this.resolveVaultId(input.vaultUid);
    const semanticContent = input.content ?? input.interpreted ?? null;
    const [contentEmbedding, subjectEmbedding] = await Promise.all([
      semanticContent ? this.ai.embed(semanticContent, "document") : Promise.resolve(null),
      input.subject ? this.ai.embed(input.subject, "document") : Promise.resolve(null)
    ]);
    return this.db.transaction(client => this.insertFile(client, {
      ...input,
      vaultId,
      contentEmbedding,
      subjectEmbedding
    }));
  }


  async updateFile(id: string, input: {
    name?: string;
    vaultUid?: string | null;
    mimeType?: string | null;
    base64?: string | null;
    content?: string | null;
    interpreted?: string | null;
    subject?: string | null;
    tags?: string[];
    metadata?: Record<string, unknown>;
  }): Promise<FileObject> {
    const currentResult = await this.db.query("SELECT * FROM files WHERE id=$1", [id]);
    if (!currentResult.rows[0]) throw new Error("File not found");
    const current = fileFromRow(currentResult.rows[0]);

    const semanticChanged = input.content !== undefined || input.interpreted !== undefined;
    const nextContent = input.content !== undefined ? input.content : current.content;
    const nextInterpreted = input.interpreted !== undefined ? input.interpreted : current.interpreted;
    const semanticText = nextContent ?? nextInterpreted;
    const contentEmbedding = semanticChanged && semanticText ? await this.ai.embed(semanticText, "document") : null;
    const subjectChanged = input.subject !== undefined;
    const subjectEmbedding = subjectChanged && input.subject ? await this.ai.embed(input.subject, "document") : null;
    const vaultId = input.vaultUid !== undefined
      ? await this.resolveVaultId(input.vaultUid)
      : current.vaultId;

    return this.db.transaction(async client => {
      const sets = ["modified_at=NOW()"];
      const values: unknown[] = [id];
      let index = 2;
      const add = (column: string, value: unknown) => {
        sets.push(`${column}=$${index++}`);
        values.push(value);
      };

      if (input.name !== undefined) add("name", normalizeText(input.name) || "Untitled");
      if (input.vaultUid !== undefined) add("vault_id", vaultId);
      if (input.mimeType !== undefined) add("mime_type", input.mimeType);
      if (input.base64 !== undefined) add("data", input.base64 ? Buffer.from(input.base64, "base64") : null);
      if (input.content !== undefined) add("content", input.content);
      if (input.interpreted !== undefined) add("interpreted", input.interpreted);
      if (input.subject !== undefined) add("subject", input.subject);
      if (semanticChanged) {
        sets.push(`vector_content=$${index++}::halfvec`);
        values.push(vectorLiteral(contentEmbedding?.vector ?? null));
        add("vector_content_provider", contentEmbedding?.provider ?? null);
        add("vector_content_model", contentEmbedding?.model ?? null);
        add("vector_content_dimensions", contentEmbedding?.dimensions ?? null);
        sets.push(`vector_content_embedded_at=${contentEmbedding ? "NOW()" : "NULL"}`);
      }
      if (subjectChanged) {
        sets.push(`vector_subject=$${index++}::halfvec`);
        values.push(vectorLiteral(subjectEmbedding?.vector ?? null));
        add("vector_subject_provider", subjectEmbedding?.provider ?? null);
        add("vector_subject_model", subjectEmbedding?.model ?? null);
        add("vector_subject_dimensions", subjectEmbedding?.dimensions ?? null);
        sets.push(`vector_subject_embedded_at=${subjectEmbedding ? "NOW()" : "NULL"}`);
      }
      if (input.tags !== undefined) {
        sets.push(`tags=$${index++}::text[]`);
        values.push(normalizeTags(input.tags));
      }
      if (input.metadata !== undefined) {
        sets.push(`metadata=metadata || $${index++}::jsonb`);
        values.push(JSON.stringify(input.metadata));
      }

      const result = await client.query(
        `UPDATE files SET ${sets.join(", ")} WHERE id=$1 RETURNING *`,
        values
      );
      const file = fileFromRow(result.rows[0]!);
      await client.query(
        "INSERT INTO modifications (vault_id,file_id,action,payload) VALUES ($1,$2,'updated',$3::jsonb)",
        [file.vaultId, file.id, JSON.stringify({ fields: Object.keys(input) })]
      );
      return file;
    });
  }


  async interpretFile(id: string): Promise<{
    file: FileObject;
    interpretation: {
      provider: string;
      model: string;
      method: string;
      metadata: Record<string, unknown>;
    };
  }> {
    if (!this.interpreter?.available()) {
      throw new Error("No File interpreter is available. Configure OPENAI_API_KEY.");
    }

    const file = await this.getFile(id);
    if (!file.base64) throw new Error("File has no binary data to interpret");

    const result = await this.interpreter.interpret({
      name: file.name,
      mimeType: file.mimeType,
      base64: file.base64
    });

    const updated = await this.updateFile(id, {
      interpreted: result.text,
      metadata: {
        interpretation: {
          provider: result.provider,
          model: result.model,
          method: result.method,
          interpretedAt: new Date().toISOString(),
          ...result.metadata
        }
      }
    });

    return {
      file: updated,
      interpretation: {
        provider: result.provider,
        model: result.model,
        method: result.method,
        metadata: result.metadata
      }
    };
  }

  async getFile(id: string): Promise<FileObject & {
    atoms: Atom[];
    atomIds: string[];
    history: Array<Record<string, unknown>>;
    links: FileLink[];
  }> {
    const fileResult = await this.db.query("SELECT * FROM files WHERE id=$1", [id]);
    if (!fileResult.rows[0]) throw new Error("File not found");
    const [atoms, history, links] = await Promise.all([
      this.db.query("SELECT a.* FROM atoms a JOIN atom_file_docks d ON d.atom_id=a.id WHERE d.file_id=$1 ORDER BY d.created_at", [id]),
      this.db.query("SELECT id,action,payload,time FROM modifications WHERE file_id=$1 ORDER BY time DESC", [id]),
      this.db.query("SELECT * FROM file_links WHERE a_file_id=$1 OR b_file_id=$1 ORDER BY modified_at DESC", [id])
    ]);
    const atomValues = atoms.rows.map(atomFromRow);
    return {
      ...fileFromRow(fileResult.rows[0]),
      atoms: atomValues,
      atomIds: atomValues.map(atom => atom.id),
      history: history.rows.map(row => ({
        id: String(row.id),
        action: String(row.action),
        payload: record(row.payload),
        time: new Date(row.time).toISOString()
      })),
      links: links.rows.map(fileLinkFromRow)
    };
  }

  async linkFiles(input: {
    aFileId: string;
    bFileId: string;
    subject?: string | null;
    absolute?: boolean;
    content?: string | null;
    atomIds?: string[];
  }): Promise<FileLink> {
    const link = await this.db.transaction(async client => {
      const result = await client.query(
        `INSERT INTO file_links (a_file_id,b_file_id,subject,absolute,content)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (a_file_id,b_file_id) DO UPDATE SET
           subject=EXCLUDED.subject,
           absolute=EXCLUDED.absolute,
           content=EXCLUDED.content,
           modified_at=NOW()
         RETURNING *`,
        [input.aFileId, input.bFileId, input.subject ?? null, input.absolute ?? false, input.content ?? null]
      );
      const value = fileLinkFromRow(result.rows[0]!);
      for (const atomId of input.atomIds ?? []) {
        await client.query(
          "INSERT INTO file_link_atoms (link_id,atom_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
          [value.id, atomId]
        );
      }
      const relatedFiles = await client.query<{ id: string; vault_id: string | null }>(
        "SELECT id::text,vault_id::text FROM files WHERE id=ANY($1::uuid[])",
        [[input.aFileId, input.bFileId]]
      );
      for (const file of relatedFiles.rows) {
        const peerFileId = file.id === input.aFileId ? input.bFileId : input.aFileId;
        await client.query(
          "INSERT INTO modifications (vault_id,file_id,action,payload) VALUES ($1,$2,'file_linked',$3::jsonb)",
          [file.vault_id, file.id, JSON.stringify({ linkId: value.id, peerFileId, absolute: value.absolute })]
        );
      }
      return value;
    });
    return link;
  }

  async ingestNote(input: {
    rawText: string;
    name?: string;
    vaultUid?: string | null;
    metadata?: Record<string, unknown>;
    sourceTime?: string;
    project?: string;
    conversationId?: string;
  }): Promise<{
    file: FileObject;
    atomIds: string[];
    extraction: { subject: string | null; tags: string[]; atomCount: number };
  }> {
    const rawText = input.rawText.trim();
    if (!rawText) throw new Error("rawText must not be empty");

    const sourceTime = input.sourceTime ?? new Date().toISOString();
    const extraction = await this.ai.extractNote(rawText, {
      sourceTime,
      timeZone: this.timeZone
    });
    const file = await this.createFile({
      name: input.name ?? "Note",
      vaultUid: input.vaultUid,
      mimeType: "text/plain",
      content: rawText,
      subject: extraction.subject,
      tags: extraction.tags,
      metadata: {
        ...(input.metadata ?? {}),
        ...(input.project ?? this.defaults.project ? { project: input.project ?? this.defaults.project } : {}),
        ...(input.conversationId ? { conversationId: input.conversationId } : {}),
        ingestion: "note",
        sourceTime,
        timeZone: this.timeZone
      }
    });

    const atomIds: string[] = [];
    for (const extracted of extraction.atoms) {
      const { scope, unresolved } = resolveWriteScope({
        project: input.project ?? this.defaults.project,
        conversationId: input.conversationId,
        extractedScope: extracted.scope ?? null,
        metadata: input.metadata ?? {}
      });
      const created = await this.createAtom({
        content: extracted.content,
        scope,
        type: "aicreated",
        tags: extracted.tags,
        metadata: {
          derivedBy: "foundation.ingestNote",
          active: true,
          validFrom: extracted.validFrom ?? null,
          expires: extracted.expires ?? null,
          subject: extracted.subject ?? extraction.subject,
          confidence: extracted.confidence ?? null,
          memoryType: extracted.memoryType ?? "Context",
          scope: scopeLabel(scope),
          ...(unresolved ? { scopeUnresolved: unresolved } : {}),
          source: file.id,
          sourceTime,
          timeZone: this.timeZone
        }
      });
      await this.dockAtom(created.atom.id, file.id, { origin: "extracted" });
      atomIds.push(created.atom.id);
    }

    return {
      file,
      atomIds,
      extraction: { subject: extraction.subject, tags: extraction.tags, atomCount: atomIds.length }
    };
  }

  private async tryEmbedDocument(content: string): Promise<{ embedding: EmbeddingResult | null; error: string | null }> {
    try {
      return { embedding: await this.ai.embed(content, "document"), error: null };
    } catch (error) {
      return {
        embedding: null,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  }

  /** Same-scope neighbours of `content`. Never crosses scopes, never includes forgotten/superseded Atoms. */
  private async memoryCandidates(
    content: string,
    tags: string[],
    scope: MemoryScope,
    limit = 12
  ): Promise<MemoryResolutionCandidate[]> {
    let embedding: EmbeddingResult | null = null;
    try {
      embedding = await this.ai.embed(content, "query");
    } catch {
      embedding = null;
    }

    const vector = vectorLiteral(embedding?.vector ?? null);
    const result = await this.db.query(
      `SELECT *,
         GREATEST(
           similarity(content,$1),
           CASE WHEN
             $2::halfvec IS NULL OR
             vector IS NULL OR
             vector_provider IS DISTINCT FROM $4 OR
             vector_model IS DISTINCT FROM $5 OR
             vector_dimensions IS DISTINCT FROM $6
           THEN 0 ELSE 1 - (vector <=> $2::halfvec) END
         ) AS candidate_score
       FROM atoms
       WHERE scope_kind=$8 AND scope_key=$9 AND status='active'
         AND (
              lower(content)=lower($1)
           OR similarity(content,$1) >= 0.20
           OR tags && $3::text[]
           OR (
             $2::halfvec IS NOT NULL
             AND vector IS NOT NULL
             AND vector_provider IS NOT DISTINCT FROM $4
             AND vector_model IS NOT DISTINCT FROM $5
             AND vector_dimensions IS NOT DISTINCT FROM $6
             AND 1 - (vector <=> $2::halfvec) >= 0.55
           )
         )
       ORDER BY candidate_score DESC, updated_at DESC
       LIMIT $7`,
      [
        content,
        vector,
        tags,
        embedding?.provider ?? null,
        embedding?.model ?? null,
        embedding?.dimensions ?? null,
        Math.max(1, Math.min(limit, 32)),
        scope.kind,
        scope.key
      ]
    );

    return result.rows.map(row => {
      const metadata = record(row.metadata);
      return {
        id: String(row.id),
        content: String(row.content),
        tags: Array.isArray(row.tags) ? row.tags.map(String) : [],
        active: true,
        similarity: Number(row.candidate_score ?? 0),
        createdAt: new Date(row.created_at).toISOString(),
        updatedAt: new Date(row.updated_at).toISOString(),
        metadata: {
          ...metadata,
          validFrom: row.valid_from ? new Date(row.valid_from).toISOString() : null,
          expires: row.expires_at ? new Date(row.expires_at).toISOString() : null
        }
      };
    });
  }

  /**
   * Phase 1 of remember for one Atom: all model inference (embedding, candidate recall, reconciliation)
   * happens here, outside any database transaction. The returned plan is validated again under row
   * locks in applyPlan, so a stale plan degrades to a conservative write instead of an overwrite.
   */
  private async planAtomic(input: {
    content: string;
    tags: string[];
    metadata: Record<string, unknown>;
    sourceTime: string;
    scope: MemoryScope;
    scopeUnresolved: string | null;
  }): Promise<AtomPlan> {
    const normalized = normalizeText(input.content);
    if (!normalized) throw new Error("Remembered content must not be empty");
    const incomingTags = normalizeTags(input.tags);

    const candidates = await this.memoryCandidates(normalized, incomingTags, input.scope);
    let resolution: MemoryResolution = {
      action: "new",
      targetAtomId: null,
      canonicalContent: normalized,
      tags: incomingTags,
      confidence: candidates.length ? 0.5 : 1,
      rationale: candidates.length ? "No resolver decision available" : "No related candidate"
    };

    if (candidates.length && this.ai.resolveMemory) {
      try {
        resolution = await this.ai.resolveMemory({
          content: normalized,
          tags: incomingTags,
          metadata: input.metadata,
          sourceTime: input.sourceTime,
          timeZone: this.timeZone,
          candidates
        });
      } catch (error) {
        resolution = {
          ...resolution,
          rationale: `Resolver unavailable: ${error instanceof Error ? error.message : String(error)}`
        };
      }
    }

    let target = resolution.targetAtomId
      ? candidates.find(candidate => candidate.id === resolution.targetAtomId) ?? null
      : null;

    if (resolution.action !== "new" && !target) {
      resolution = {
        ...resolution,
        action: "new",
        targetAtomId: null,
        rationale: `Resolver named no valid same-scope target; stored as new. ${resolution.rationale}`
      };
    }

    const confidence = Number.isFinite(resolution.confidence) ? Math.max(0, Math.min(1, resolution.confidence)) : 0;
    if (
      (resolution.action === "duplicate" && confidence < 0.7) ||
      (resolution.action === "supersede" && confidence < 0.75) ||
      (resolution.action === "update" && confidence < 0.8)
    ) {
      resolution = {
        ...resolution,
        action: target ? "conflict" : "new",
        rationale: `Low-confidence destructive reconciliation avoided. ${resolution.rationale}`
      };
    }
    resolution = { ...resolution, confidence };

    // Only `update` may rewrite wording; everything else keeps the extracted Atom text verbatim.
    const canonical = resolution.action === "update"
      ? normalizeText(resolution.canonicalContent) || normalized
      : normalized;
    const storedContent = resolution.action === "update" ? canonical : normalized;
    const embedded = await this.tryEmbedDocument(storedContent);

    return {
      content: normalized,
      storedContent,
      tags: normalizeTags([...incomingTags, ...resolution.tags]),
      metadata: input.metadata,
      scope: input.scope,
      scopeUnresolved: input.scopeUnresolved,
      candidates,
      resolution,
      target,
      embedding: embedded.embedding,
      embeddingError: embedded.error
    };
  }

  private async recordMemoryRelation(
    client: Queryable,
    fromAtomId: string,
    toAtomId: string,
    relation: "supersedes" | "conflicts_with" | "refines" | "duplicate_of",
    metadata: Record<string, unknown>
  ): Promise<void> {
    if (fromAtomId === toAtomId) return;
    await client.query(
      `INSERT INTO atom_relations(from_atom_id,to_atom_id,relation,metadata)
       VALUES($1,$2,$3,$4::jsonb)
       ON CONFLICT(from_atom_id,to_atom_id,relation) DO UPDATE SET
         metadata=atom_relations.metadata || EXCLUDED.metadata`,
      [fromAtomId, toAtomId, relation, JSON.stringify(metadata)]
    );
  }

  /**
   * Phase 2: apply one planned Atom inside the request transaction. Every row it depends on is locked
   * and re-checked; a plan that went stale falls back to "new + conflicts_with" rather than overwriting.
   */
  private async applyPlan(
    client: PoolClient,
    plan: AtomPlan,
    ctx: { sourceTime: string; requestId?: string; touched: Set<string> }
  ): Promise<RememberAtomicResult> {
    const rememberedAt = new Date().toISOString();
    const hash = contentHash(plan.content);
    const sourceTime = new Date(ctx.sourceTime);
    const declaredFrom = parseTimestamp(plan.metadata.validFrom);
    const newValidFrom = declaredFrom ?? sourceTime;
    const embeddingDeferred = plan.embedding === null && plan.embeddingError !== null;
    const baseMetadata: Record<string, unknown> = {
      ...plan.metadata,
      scope: scopeLabel(plan.scope),
      ...(plan.scopeUnresolved ? { scopeUnresolved: plan.scopeUnresolved } : {}),
      rememberedAt,
      sourceTime: ctx.sourceTime,
      timeZone: this.timeZone,
      resolverConfidence: plan.resolution.confidence,
      embeddingDeferred,
      embeddingError: plan.embeddingError?.slice(0, 1000) ?? null
    };
    delete baseMetadata.active;

    let action: MemoryResolution["action"] = plan.resolution.action;
    let rationale = plan.resolution.rationale;
    let target: Atom | null = null;
    let atom: Atom;
    let deferred = embeddingDeferred;

    const event = async (resultAtomId: string, targetId: string | null) => {
      await client.query(
        `INSERT INTO memory_events(
           request_id,input_content,source_time,action,target_atom_id,result_atom_id,
           confidence,rationale,candidates,metadata
         ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9::jsonb,$10::jsonb)`,
        [
          ctx.requestId ?? null,
          plan.content,
          ctx.sourceTime,
          action,
          targetId,
          resultAtomId,
          plan.resolution.confidence,
          rationale,
          JSON.stringify(plan.candidates.map(c => ({ id: c.id, similarity: c.similarity, active: c.active }))),
          JSON.stringify({ ...plan.metadata, scope: plan.scope })
        ]
      );
    };
    const finish = (): RememberAtomicResult => ({
      action,
      targetAtomId: target?.id ?? null,
      atom,
      confidence: plan.resolution.confidence,
      rationale,
      embeddingDeferred: deferred,
      candidates: plan.candidates.map(c => ({ id: c.id, similarity: c.similarity }))
    });
    const lockTarget = async (): Promise<{ atom: Atom; stable: boolean } | null> => {
      if (!plan.target) return null;
      const row = (await client.query("SELECT * FROM atoms WHERE id=$1 FOR UPDATE", [plan.target.id])).rows[0];
      if (!row) return null;
      const locked = atomFromRow(row);
      const stable = locked.status === "active" &&
        locked.updatedAt.toISOString() === plan.target.updatedAt &&
        !ctx.touched.has(locked.id);
      return { atom: locked, stable };
    };

    // 1. Identical content in the same scope: refresh instead of duplicating. Never overwrites
    //    validity or provenance of the existing Atom with the new extraction's guesses.
    const exact = (await client.query(
      `SELECT * FROM atoms
       WHERE scope_kind=$1 AND scope_key=$2 AND status<>'superseded' AND (content_hash=$3 OR normalized_content=$4)
       ORDER BY (status='active') DESC LIMIT 1 FOR UPDATE`,
      [plan.scope.kind, plan.scope.key, hash, plan.content.toLowerCase()]
    )).rows[0];
    if (exact) {
      const existing = atomFromRow(exact);
      if (existing.status === "active") {
        target = existing;
        action = "duplicate";
        rationale = "Exact active content match in the same scope";
        atom = await this.updateAtom(existing.id, {
          tags: normalizeTags([...existing.tags, ...plan.tags]),
          metadata: { lastRememberedAt: rememberedAt, lastSourceTime: ctx.sourceTime }
        }, client);
        ctx.touched.add(atom.id);
        await event(atom.id, existing.id);
        return finish();
      }
      // The user deliberately remembered something they had forgotten: bring it back.
      target = null;
      action = "new";
      rationale = "Re-remembered previously forgotten memory";
      atom = await this.updateAtom(existing.id, {
        status: "active",
        tags: normalizeTags([...existing.tags, ...plan.tags]),
        validFrom: newValidFrom,
        expiresAt: parseTimestamp(plan.metadata.expires),
        metadata: { ...baseMetadata, revivedAt: rememberedAt },
        embedding: plan.embedding ?? undefined
      }, client);
      ctx.touched.add(atom.id);
      await event(atom.id, null);
      return finish();
    }

    // 2. Resolver decisions, each re-validated against the locked target.
    let stable = false;
    if (plan.resolution.action !== "new") {
      const locked = await lockTarget();
      target = locked?.atom ?? null;
      stable = locked?.stable ?? false;
      if (!stable && plan.resolution.action !== "conflict") {
        const stillActive = target !== null && target.status === "active";
        action = stillActive ? "conflict" : "new";
        rationale = `Target changed or was unavailable after planning; kept both. ${rationale}`;
        if (!stillActive) target = null;
      } else if (plan.resolution.action === "conflict" && (target === null || target.status !== "active")) {
        action = "new";
        target = null;
      }
    }

    if (action === "duplicate" && target) {
      atom = await this.updateAtom(target.id, {
        tags: normalizeTags([...target.tags, ...plan.tags]),
        metadata: { lastRememberedAt: rememberedAt, lastSourceTime: ctx.sourceTime, duplicateRefresh: true }
      }, client);
      ctx.touched.add(atom.id);
      await event(atom.id, target.id);
      return finish();
    }

    if (action === "update" && target && stable) {
      const clash = (await client.query(
        `SELECT id FROM atoms WHERE scope_kind=$1 AND scope_key=$2 AND status<>'superseded' AND content_hash=$3 AND id<>$4 LIMIT 1`,
        [plan.scope.kind, plan.scope.key, contentHash(plan.storedContent), target.id]
      )).rows[0];
      if (clash) {
        // The rewritten wording already exists as another Atom; refresh that one, leave the target alone.
        action = "duplicate";
        rationale = `Updated wording already exists as another Atom. ${rationale}`;
        const existing = atomFromRow((await client.query("SELECT * FROM atoms WHERE id=$1 FOR UPDATE", [clash.id])).rows[0]);
        target = existing;
        atom = await this.updateAtom(existing.id, {
          tags: normalizeTags([...existing.tags, ...plan.tags]),
          metadata: { lastRememberedAt: rememberedAt, lastSourceTime: ctx.sourceTime }
        }, client);
        ctx.touched.add(atom.id);
        await event(atom.id, existing.id);
        return finish();
      }
      const history = Array.isArray(target.metadata.history) ? target.metadata.history.slice(-19) : [];
      const { validFrom: _vf, expires: _ex, ...updateMetadata } = baseMetadata;
      atom = await this.updateAtom(target.id, {
        content: plan.storedContent,
        tags: normalizeTags([...target.tags, ...plan.tags]),
        embedding: plan.embedding,
        metadata: {
          ...updateMetadata,
          previousContent: target.content,
          history: [...history, { content: target.content, replacedAt: rememberedAt, sourceTime: ctx.sourceTime }]
        }
      }, client);
      ctx.touched.add(atom.id);
      await event(atom.id, target.id);
      return finish();
    }

    // 3. Everything else creates a new Atom.
    if (action === "update" || action === "duplicate") {
      action = target ? "conflict" : "new";
      rationale = `Target was not safely updatable; stored as a separate Atom. ${rationale}`;
    }

    const historical = action === "supersede" && target !== null && stable &&
      target.validFrom !== null && newValidFrom < target.validFrom;
    const created = await this.createAtom({
      content: plan.storedContent,
      type: "usercreated",
      tags: plan.tags,
      metadata: baseMetadata,
      embedding: plan.embedding,
      scope: plan.scope,
      status: historical ? "superseded" : "active",
      validFrom: newValidFrom,
      expiresAt: historical ? target!.validFrom : parseTimestamp(plan.metadata.expires)
    }, client);
    atom = created.atom;
    ctx.touched.add(atom.id);

    if (!created.created && action === "new") {
      action = "duplicate";
      rationale = "Concurrent identical write merged into the existing Atom";
    }

    if (action === "supersede" && target && stable) {
      if (historical) {
        // Out-of-order arrival: the incoming fact is older than the current one. Keep it as history only.
        await this.recordMemoryRelation(client, target.id, atom.id, "supersedes", {
          sourceTime: ctx.sourceTime,
          confidence: plan.resolution.confidence,
          rationale: "Older fact received after a newer one"
        });
        rationale = `Older fact recorded as history; current fact unchanged. ${rationale}`;
      } else {
        const takesEffectNow = newValidFrom.getTime() <= Date.now();
        const expires = target.expiresAt && target.expiresAt < newValidFrom ? target.expiresAt : newValidFrom;
        await this.updateAtom(target.id, {
          ...(takesEffectNow ? { status: "superseded" as const } : {}),
          expiresAt: expires,
          metadata: {
            supersededBy: atom.id,
            supersededAt: rememberedAt,
            supersedeReason: rationale
          }
        }, client);
        ctx.touched.add(target.id);
        await this.recordMemoryRelation(client, atom.id, target.id, "supersedes", {
          sourceTime: ctx.sourceTime,
          confidence: plan.resolution.confidence,
          rationale
        });
      }
    } else if (action === "supersede") {
      action = "new";
    } else if (action === "conflict" && target) {
      await this.recordMemoryRelation(client, atom.id, target.id, "conflicts_with", {
        sourceTime: ctx.sourceTime,
        confidence: plan.resolution.confidence,
        rationale
      });
    }

    await event(atom.id, target?.id ?? null);
    return finish();
  }

  async remember(input: RememberInput): Promise<RememberResult> {
    if (!input.requestId) return this.rememberUnlocked(input);

    const content = normalizeText(input.content);
    const existing = this.rememberInFlight.get(input.requestId);
    if (existing) {
      if (existing.content !== content) throw new Error("requestId is already in use for different content");
      return existing.promise;
    }

    const promise = this.rememberUnlocked(input)
      .finally(() => this.rememberInFlight.delete(input.requestId!));
    this.rememberInFlight.set(input.requestId, { content, promise });
    return promise;
  }

  private async priorRequest(requestId: string, content: string, client?: Queryable): Promise<RememberResult | null> {
    const prior = await (client ?? this.db).query(
      "SELECT input_content,result FROM memory_requests WHERE request_id=$1 LIMIT 1",
      [requestId]
    );
    const row = prior.rows[0];
    if (!row) return null;
    if (normalizeText(String(row.input_content)) !== content) {
      throw new Error("requestId is already in use for different content");
    }
    return row.result as RememberResult;
  }

  private async rememberUnlocked(input: RememberInput): Promise<RememberResult> {
    const content = normalizeText(input.content);
    if (!content) throw new Error("content must not be empty");
    const sourceTime = input.sourceTime ?? new Date().toISOString();
    if (!Number.isFinite(new Date(sourceTime).getTime())) throw new Error("sourceTime must be valid ISO-8601");

    if (input.requestId) {
      const prior = await this.priorRequest(input.requestId, content);
      if (prior) return prior;
    }

    // ---- Phase 1: inference only, no transaction held open. ----
    let extracted: Awaited<ReturnType<AIProvider["extractNote"]>> | null = null;
    try {
      extracted = await this.ai.extractNote(content, { sourceTime, timeZone: this.timeZone });
    } catch {
      extracted = null;
    }

    const atomic = extracted?.atoms.length
      ? extracted.atoms
      : [{
          content,
          tags: normalizeTags(input.tags ?? []),
          validFrom: sourceTime,
          expires: null,
          subject: null,
          confidence: null,
          memoryType: null,
          scope: null
        }];

    const callerMetadata = input.metadata ?? {};
    const plans: AtomPlan[] = [];
    for (const item of atomic) {
      const { scope, unresolved } = resolveWriteScope({
        scope: input.scope,
        project: input.project ?? this.defaults.project,
        conversationId: input.conversationId,
        extractedScope: item.scope ?? null,
        metadata: callerMetadata
      });
      plans.push(await this.planAtomic({
        content: item.content,
        tags: normalizeTags([...(input.tags ?? []), ...item.tags]),
        sourceTime,
        scope,
        scopeUnresolved: unresolved,
        metadata: {
          ...callerMetadata,
          validFrom: item.validFrom ?? null,
          expires: item.expires ?? null,
          subject: item.subject ?? extracted?.subject ?? null,
          confidence: item.confidence ?? null,
          memoryType: item.memoryType ?? "Context",
          rememberSource: "foundation.remember"
        }
      }));
    }

    // ---- Phase 2: one transaction for every Atom, relation, event and the request record. ----
    return this.db.transaction(async client => {
      if (input.requestId) {
        await client.query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))", [input.requestId]);
        const prior = await this.priorRequest(input.requestId, content, client);
        if (prior) return prior;
      }

      const touched = new Set<string>();
      const results: RememberAtomicResult[] = [];
      for (const plan of plans) {
        results.push(await this.applyPlan(client, plan, { sourceTime, requestId: input.requestId, touched }));
      }

      const result: RememberResult = {
        requestId: input.requestId ?? null,
        sourceTime,
        decomposed: atomic.length > 1,
        results
      };
      if (input.requestId) {
        await client.query(
          `INSERT INTO memory_requests(request_id,input_content,source_time,result)
           VALUES($1,$2,$3,$4::jsonb)`,
          [input.requestId, content, sourceTime, JSON.stringify(result)]
        );
      }
      return result;
    });
  }

  async search(query: string, limitOrOptions: number | SearchOptions = 10): Promise<SearchItem[]> {
    const options: SearchOptions = typeof limitOrOptions === "number" ? { limit: limitOrOptions } : { ...limitOrOptions };
    // A server-configured default project scopes callers that did not name one.
    if (!options.project && !options.conversationId && this.defaults.project) options.project = this.defaults.project;
    return searchFoundation(this.db, this.ai, query, options);
  }

  /** Ranking diagnostics for the CLI: what each generator found, every candidate's signals, why it was cut. */
  async explainSearch(query: string, options: SearchOptions & { plans?: boolean } = {}): Promise<{ trace: SearchTrace; results: SearchItem[] }> {
    const trace = newSearchTrace(options.plans ?? false);
    const results = await this.search(query, { ...options, trace });
    for (const generator of trace.generators) {
      if (generator.plan) generator.plan = generator.plan.map(line => line.replace(/\[[-0-9.,eE+]{200,}\]/g, "[vector]"));
    }
    trace.candidates.sort((a, b) => b.score - a.score);
    return { trace, results };
  }

  /** An Atom with its relations, docks, lifecycle and reconciliation history, for diagnosing memory identity. */
  async atomRelations(id: string): Promise<Record<string, unknown>> {
    const atom = await this.getAtom(id);
    const [relations, events, files] = await Promise.all([
      this.db.query(
        `SELECT r.relation, r.from_atom_id, r.to_atom_id, r.created_at, r.metadata,
                CASE WHEN r.from_atom_id=$1 THEN 'out' ELSE 'in' END AS direction,
                o.content AS other_content, o.status AS other_status
         FROM atom_relations r
         JOIN atoms o ON o.id = CASE WHEN r.from_atom_id=$1 THEN r.to_atom_id ELSE r.from_atom_id END
         WHERE r.from_atom_id=$1 OR r.to_atom_id=$1 ORDER BY r.created_at`, [id]),
      this.db.query(
        `SELECT action, confidence, rationale, request_id, created_at, target_atom_id, result_atom_id
         FROM memory_events WHERE result_atom_id=$1 OR target_atom_id=$1 ORDER BY created_at`, [id]),
      this.db.query(
        `SELECT f.id, f.name FROM atom_file_docks d JOIN files f ON f.id=d.file_id WHERE d.atom_id=$1`, [id])
    ]);
    return {
      id: atom.id,
      content: atom.content,
      scope: atom.scope,
      status: atom.status,
      validFrom: atom.validFrom,
      expiresAt: atom.expiresAt,
      sources: files.rows,
      relations: relations.rows.map(row => ({
        relation: row.relation,
        direction: row.direction,
        otherAtomId: row.direction === "out" ? row.to_atom_id : row.from_atom_id,
        otherStatus: row.other_status,
        otherContent: String(row.other_content).slice(0, 160),
        at: row.created_at
      })),
      events: events.rows,
      history: Array.isArray(atom.metadata.history) ? atom.metadata.history : []
    };
  }

  /** Scope/lifecycle/embedding health summary. Counts only; never returns memory text. */
  async diagnose(): Promise<Record<string, unknown>> {
    const [byScope, byStatus, health] = await Promise.all([
      this.db.query("SELECT scope_kind, count(*)::int AS atoms, count(DISTINCT scope_key)::int AS keys FROM atoms GROUP BY scope_kind ORDER BY scope_kind"),
      this.db.query("SELECT status, count(*)::int AS atoms FROM atoms GROUP BY status ORDER BY status"),
      this.db.query(`SELECT
        (SELECT count(*)::int FROM atoms WHERE metadata ? 'scopeUnresolved') AS scope_unresolved,
        (SELECT count(*)::int FROM atoms WHERE status='active' AND expires_at IS NOT NULL AND expires_at <= NOW()) AS expired_but_active,
        (SELECT count(*)::int FROM atoms WHERE status='active' AND valid_from > NOW()) AS not_yet_valid,
        (SELECT count(*)::int FROM atoms WHERE status='active' AND vector IS NULL) AS active_without_embedding,
        (SELECT count(*)::int FROM atom_relations WHERE relation='conflicts_with') AS conflicts,
        (SELECT count(*)::int FROM (SELECT 1 FROM atoms WHERE status='active' GROUP BY lower(content) HAVING count(DISTINCT scope_kind || ':' || scope_key) > 1) x) AS content_shared_across_scopes,
        (SELECT count(*)::int FROM memory_requests) AS idempotent_requests`)
    ]);
    return { scopes: byScope.rows, status: byStatus.rows, ...health.rows[0] };
  }

  async stats(): Promise<Record<string, unknown>> {
    const identity = this.ai.identity();
    const result = await this.db.query(`
      SELECT
        (SELECT count(*)::int FROM atoms) AS atoms,
        (SELECT count(*)::int FROM files) AS files,
        (SELECT count(*)::int FROM vaults) AS vaults,
        (SELECT count(*)::int FROM atom_file_docks) AS docks,
        (SELECT count(*)::int FROM file_links) AS file_links,
        (SELECT count(*)::int FROM atoms WHERE vector IS NULL) AS atoms_without_embedding,
        (SELECT count(*)::int FROM files WHERE coalesce(content,interpreted) IS NOT NULL AND vector_content IS NULL) AS files_without_content_embedding,
        (SELECT count(*)::int FROM files WHERE subject IS NOT NULL AND vector_subject IS NULL) AS files_without_subject_embedding
    `);
    return {
      ...result.rows[0],
      embedding: identity
    };
  }

  async listAtoms(limit = 50, offset = 0): Promise<Atom[]> {
    const result = await this.db.query(
      "SELECT * FROM atoms ORDER BY updated_at DESC, id LIMIT $1 OFFSET $2",
      [Math.max(1, Math.min(limit, 500)), Math.max(0, offset)]
    );
    return result.rows.map(atomFromRow);
  }

  async listFiles(limit = 50, offset = 0): Promise<FileObject[]> {
    const result = await this.db.query(
      "SELECT * FROM files ORDER BY modified_at DESC, id LIMIT $1 OFFSET $2",
      [Math.max(1, Math.min(limit, 500)), Math.max(0, offset)]
    );
    return result.rows.map(fileFromRow);
  }

  async listVaults(limit = 50, offset = 0): Promise<Vault[]> {
    const result = await this.db.query(
      "SELECT * FROM vaults ORDER BY updated_at DESC, id LIMIT $1 OFFSET $2",
      [Math.max(1, Math.min(limit, 500)), Math.max(0, offset)]
    );
    return result.rows.map(vaultFromRow);
  }

  /**
   * Remove one memory from recall by exact Atom id. Source Files and relations are kept as evidence.
   * A File id is rejected explicitly: a File can back several Atoms, so it never identifies one memory.
   */
  async forget(id: string): Promise<{ id: string; forgotten: boolean }> {
    const result = await this.db.query(
      `UPDATE atoms
       SET status='forgotten',
           metadata=metadata || jsonb_build_object('active', FALSE, 'forgottenAt', to_jsonb(NOW()::text)),
           updated_at=NOW()
       WHERE id=$1 AND status <> 'forgotten'
       RETURNING id`,
      [id]
    );
    if (result.rowCount) return { id, forgotten: true };

    const existing = await this.db.query("SELECT 1 FROM atoms WHERE id=$1", [id]);
    if (existing.rowCount) return { id, forgotten: true };

    const file = await this.db.query("SELECT 1 FROM files WHERE id=$1", [id]);
    if (file.rowCount) {
      throw new Error("The id refers to a source File, not a memory. Use the memory id returned by recall; a File may back several memories.");
    }
    return { id, forgotten: false };
  }

  async deleteAtom(id: string): Promise<{ deleted: boolean }> {
    const result = await this.db.query("DELETE FROM atoms WHERE id=$1 RETURNING id", [id]);
    return { deleted: Boolean(result.rowCount) };
  }

  async deleteFile(id: string): Promise<{ deleted: boolean }> {
    return this.db.transaction(async client => {
      const current = await client.query<{ vault_id: string | null; name: string }>(
        "SELECT vault_id::text,name FROM files WHERE id=$1 FOR UPDATE",
        [id]
      );
      if (!current.rows[0]) return { deleted: false };
      await client.query(
        "INSERT INTO modifications (vault_id,file_id,action,payload) VALUES ($1,$2,'deleted',$3::jsonb)",
        [current.rows[0].vault_id, id, JSON.stringify({ name: current.rows[0].name, fileId: id })]
      );
      await client.query("DELETE FROM files WHERE id=$1", [id]);
      return { deleted: true };
    });
  }

  async deleteVault(idOrUid: string, deleteFiles = false): Promise<{ deleted: boolean; deletedFiles: number }> {
    return this.db.transaction(async client => {
      const vault = await client.query<{ id: string }>(
        "SELECT id::text FROM vaults WHERE id::text=$1 OR vault_uid=$1 LIMIT 1 FOR UPDATE",
        [idOrUid]
      );
      const id = vault.rows[0]?.id;
      if (!id) return { deleted: false, deletedFiles: 0 };

      let deletedFiles = 0;
      if (deleteFiles) {
        const files = await client.query("DELETE FROM files WHERE vault_id=$1 RETURNING id", [id]);
        deletedFiles = files.rowCount ?? 0;
      }
      await client.query("DELETE FROM vaults WHERE id=$1", [id]);
      return { deleted: true, deletedFiles };
    });
  }

  async findSimilarAtoms(input: {
    id?: string;
    content?: string;
    limit?: number;
    minScore?: number;
  }): Promise<Array<{ atom: Atom; score: number }>> {
    let content = normalizeText(input.content ?? "");
    let excludeId: string | null = null;

    if (input.id) {
      const current = await this.db.query("SELECT id::text,content FROM atoms WHERE id=$1", [input.id]);
      if (!current.rows[0]) throw new Error("Atom not found");
      content = String(current.rows[0].content);
      excludeId = String(current.rows[0].id);
    }
    if (!content) throw new Error("id or content is required");

    const embedding = await this.ai.embed(content, "query");
    if (!embedding) return [];

    const limit = Math.max(1, Math.min(input.limit ?? 10, 100));
    const minScore = Math.max(-1, Math.min(input.minScore ?? 0.85, 1));
    const result = await this.db.query(
      `SELECT *, 1 - (vector <=> $1::halfvec) AS similarity_score
       FROM atoms
       WHERE vector IS NOT NULL
         AND status='active'
         AND vector_provider=$2
         AND vector_model=$3
         AND vector_dimensions=$4
         AND ($5::uuid IS NULL OR id<>$5::uuid)
         AND 1 - (vector <=> $1::halfvec) >= $6
       ORDER BY vector <=> $1::halfvec
       LIMIT $7`,
      [
        vectorLiteral(embedding.vector),
        embedding.provider,
        embedding.model,
        embedding.dimensions,
        excludeId,
        minScore,
        limit
      ]
    );
    return result.rows.map(row => ({
      atom: atomFromRow(row),
      score: Number(row.similarity_score)
    }));
  }

  async mergeAtoms(sourceId: string, targetId: string): Promise<Atom & { dockedTo: string[] }> {
    if (sourceId === targetId) throw new Error("sourceId and targetId must differ");

    await this.db.transaction(async client => {
      const pair = await client.query(
        "SELECT * FROM atoms WHERE id=ANY($1::uuid[]) ORDER BY id FOR UPDATE",
        [[sourceId, targetId]]
      );
      if (pair.rows.length !== 2) throw new Error("Both source and target Atoms must exist");
      const source = pair.rows.find(row => String(row.id) === sourceId)!;
      const targetRow = pair.rows.find(row => String(row.id) === targetId)!;
      if (source.scope_kind !== targetRow.scope_kind || source.scope_key !== targetRow.scope_key) {
        throw new Error("Atoms from different scopes cannot be merged");
      }

      await client.query(
        `INSERT INTO atom_file_docks(atom_id,file_id,metadata,created_at)
         SELECT $2,file_id,metadata || $3::jsonb,created_at
         FROM atom_file_docks WHERE atom_id=$1
         ON CONFLICT(atom_id,file_id) DO UPDATE
           SET metadata=atom_file_docks.metadata || EXCLUDED.metadata`,
        [sourceId, targetId, JSON.stringify({ mergedFrom: sourceId })]
      );
      await client.query(
        `INSERT INTO file_link_atoms(link_id,atom_id)
         SELECT link_id,$2 FROM file_link_atoms WHERE atom_id=$1
         ON CONFLICT DO NOTHING`,
        [sourceId, targetId]
      );
      await client.query(
        `UPDATE atoms
         SET tags=ARRAY(
           SELECT DISTINCT value
           FROM unnest(atoms.tags || $2::text[]) value
           ORDER BY value
         ),
         metadata=atoms.metadata || $3::jsonb,
         updated_at=NOW()
         WHERE id=$1`,
        [
          targetId,
          Array.isArray(source.tags) ? source.tags : [],
          JSON.stringify({
            mergedFrom: sourceId,
            mergedContent: source.content,
            mergedAt: new Date().toISOString()
          })
        ]
      );
      await client.query("DELETE FROM atoms WHERE id=$1", [sourceId]);
    });

    return this.getAtom(targetId);
  }

  async reindex(input: {
    kind?: "all" | "atoms" | "files";
    limit?: number;
    force?: boolean;
    concurrency?: number;
  } = {}): Promise<{
    embedding: ReturnType<AIProvider["identity"]>;
    concurrency: number;
    atoms: { scanned: number; updated: number; failed: number };
    files: { scanned: number; contentUpdated: number; subjectUpdated: number; failed: number };
    errors: Array<{ kind: "atom" | "file"; id: string; error: string }>;
  }> {
    const identity = this.ai.identity();
    const concurrency = Math.max(1, Math.min(input.concurrency ?? 4, 16));
    if (!identity) {
      return {
        embedding: null,
        concurrency,
        atoms: { scanned: 0, updated: 0, failed: 0 },
        files: { scanned: 0, contentUpdated: 0, subjectUpdated: 0, failed: 0 },
        errors: []
      };
    }

    const runPool = async <T>(items: T[], worker: (item: T) => Promise<void>): Promise<void> => {
      let cursor = 0;
      const workers = Array.from(
        { length: Math.min(concurrency, Math.max(1, items.length)) },
        async () => {
          while (true) {
            const index = cursor++;
            if (index >= items.length) return;
            await worker(items[index]!);
          }
        }
      );
      await Promise.all(workers);
    };

    const kind = input.kind ?? "all";
    const limit = Math.max(1, Math.min(input.limit ?? 1000, 10_000));
    const force = input.force ?? false;
    const errors: Array<{ kind: "atom" | "file"; id: string; error: string }> = [];
    const atomStats = { scanned: 0, updated: 0, failed: 0 };
    const fileStats = { scanned: 0, contentUpdated: 0, subjectUpdated: 0, failed: 0 };

    if (kind === "all" || kind === "atoms") {
      const atoms = await this.db.query(
        `SELECT id::text,content
         FROM atoms
         WHERE $1::boolean
            OR vector IS NULL
            OR vector_provider IS DISTINCT FROM $2
            OR vector_model IS DISTINCT FROM $3
            OR vector_dimensions IS DISTINCT FROM $4
         ORDER BY updated_at,id
         LIMIT $5`,
        [force, identity.provider, identity.model, identity.dimensions, limit]
      );
      atomStats.scanned = atoms.rows.length;

      await runPool(atoms.rows, async row => {
        const id = String(row.id);
        try {
          const embedding = await this.ai.embed(String(row.content), "document");
          if (!embedding) return;
          await this.db.query(
            `UPDATE atoms SET
               vector=$2::halfvec,
               vector_provider=$3,
               vector_model=$4,
               vector_dimensions=$5,
               vector_embedded_at=NOW()
             WHERE id=$1`,
            [id, vectorLiteral(embedding.vector), embedding.provider, embedding.model, embedding.dimensions]
          );
          atomStats.updated += 1;
        } catch (error) {
          atomStats.failed += 1;
          errors.push({ kind: "atom", id, error: error instanceof Error ? error.message : String(error) });
        }
      });
    }

    if (kind === "all" || kind === "files") {
      const files = await this.db.query(
        `SELECT id::text,content,interpreted,subject,
           vector_content_provider,vector_content_model,vector_content_dimensions,
           vector_subject_provider,vector_subject_model,vector_subject_dimensions,
           vector_content IS NULL AS content_missing,
           vector_subject IS NULL AS subject_missing
         FROM files
         WHERE $1::boolean
            OR (
              coalesce(content,interpreted) IS NOT NULL
              AND (
                vector_content IS NULL
                OR vector_content_provider IS DISTINCT FROM $2
                OR vector_content_model IS DISTINCT FROM $3
                OR vector_content_dimensions IS DISTINCT FROM $4
              )
            )
            OR (
              subject IS NOT NULL
              AND (
                vector_subject IS NULL
                OR vector_subject_provider IS DISTINCT FROM $2
                OR vector_subject_model IS DISTINCT FROM $3
                OR vector_subject_dimensions IS DISTINCT FROM $4
              )
            )
         ORDER BY modified_at,id
         LIMIT $5`,
        [force, identity.provider, identity.model, identity.dimensions, limit]
      );
      fileStats.scanned = files.rows.length;

      await runPool(files.rows, async row => {
        const id = String(row.id);
        try {
          const semanticText = row.content ?? row.interpreted;
          const contentStale = force ||
            Boolean(row.content_missing) ||
            row.vector_content_provider !== identity.provider ||
            row.vector_content_model !== identity.model ||
            Number(row.vector_content_dimensions ?? 0) !== identity.dimensions;
          const subjectStale = force ||
            Boolean(row.subject_missing) ||
            row.vector_subject_provider !== identity.provider ||
            row.vector_subject_model !== identity.model ||
            Number(row.vector_subject_dimensions ?? 0) !== identity.dimensions;

          if (semanticText && contentStale) {
            const embedding = await this.ai.embed(String(semanticText), "document");
            if (embedding) {
              await this.db.query(
                `UPDATE files SET
                   vector_content=$2::halfvec,
                   vector_content_provider=$3,
                   vector_content_model=$4,
                   vector_content_dimensions=$5,
                   vector_content_embedded_at=NOW()
                 WHERE id=$1`,
                [id, vectorLiteral(embedding.vector), embedding.provider, embedding.model, embedding.dimensions]
              );
              fileStats.contentUpdated += 1;
            }
          }

          if (row.subject && subjectStale) {
            const embedding = await this.ai.embed(String(row.subject), "document");
            if (embedding) {
              await this.db.query(
                `UPDATE files SET
                   vector_subject=$2::halfvec,
                   vector_subject_provider=$3,
                   vector_subject_model=$4,
                   vector_subject_dimensions=$5,
                   vector_subject_embedded_at=NOW()
                 WHERE id=$1`,
                [id, vectorLiteral(embedding.vector), embedding.provider, embedding.model, embedding.dimensions]
              );
              fileStats.subjectUpdated += 1;
            }
          }
        } catch (error) {
          fileStats.failed += 1;
          errors.push({ kind: "file", id, error: error instanceof Error ? error.message : String(error) });
        }
      });
    }

    return {
      embedding: identity,
      concurrency,
      atoms: atomStats,
      files: fileStats,
      errors
    };
  }
}
