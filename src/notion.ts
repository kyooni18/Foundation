import { spawn } from "node:child_process";
import type { Config } from "./config.js";
import type { Database } from "./db.js";
import type { Foundation } from "./foundation.js";
import type { Atom } from "./types.js";
import { contentHash, normalizeTags, record } from "./utils.js";

interface NotionPage {
  id: string;
  url?: string;
  last_edited_time: string;
  properties: Record<string, any>;
}

interface NotionQuery {
  results: NotionPage[];
  has_more: boolean;
  next_cursor?: string | null;
}

interface BindingRow {
  entity_id: string;
  external_id: string;
  external_url: string | null;
  external_updated_at: Date | null;
  foundation_updated_at: Date | null;
  last_synced_at: Date;
  content_hash: string | null;
  metadata: Record<string, unknown>;
}

interface ParsedMemory {
  pageId: string;
  url: string | null;
  updatedAt: Date;
  content: string;
  title: string;
  active: boolean;
  confidence: number | null;
  keywords: string[];
  scope: "Global" | "Project" | "Conversation";
  source: string | null;
  subject: string | null;
  memoryType: "Preference" | "Fact" | "Project" | "Decision" | "Context" | "Instruction";
  importance: "Critical" | "High" | "Medium" | "Low" | null;
  validFrom: string | null;
  expires: string | null;
}

function richText(value: unknown): string {
  if (!Array.isArray(value)) return "";
  return value.map(item => {
    if (!item || typeof item !== "object") return "";
    const plain = (item as Record<string, unknown>).plain_text;
    if (typeof plain === "string") return plain;
    const text = (item as any).text?.content;
    return typeof text === "string" ? text : "";
  }).join("");
}

function propertyText(prop: any): string {
  if (!prop || typeof prop !== "object") return "";
  if (Array.isArray(prop.rich_text)) return richText(prop.rich_text);
  if (Array.isArray(prop.title)) return richText(prop.title);
  return "";
}

function parsePage(page: NotionPage): ParsedMemory | null {
  const p = page.properties ?? {};
  const content = propertyText(p.Content).trim();
  if (!content) return null;
  const title = propertyText(p.Memory).trim() || content.slice(0, 120);
  const type = p.Type?.select?.name;
  const scope = p.Scope?.select?.name;
  const importance = p.Importance?.select?.name;

  return {
    pageId: page.id,
    url: typeof page.url === "string" ? page.url : null,
    updatedAt: new Date(page.last_edited_time),
    content,
    title,
    active: p.Active?.checkbox !== false,
    confidence: typeof p.Confidence?.number === "number" ? p.Confidence.number : null,
    keywords: normalizeTags(propertyText(p.Keywords).split(",").map((value: string) => value.trim())),
    scope: scope === "Project" || scope === "Conversation" ? scope : "Global",
    source: propertyText(p.Source).trim() || null,
    subject: propertyText(p.Subject).trim() || null,
    memoryType: ["Preference", "Fact", "Project", "Decision", "Context", "Instruction"].includes(type)
      ? type
      : "Context",
    importance: ["Critical", "High", "Medium", "Low"].includes(importance) ? importance : null,
    validFrom: typeof p["Valid From"]?.date?.start === "string" ? p["Valid From"].date.start : null,
    expires: typeof p.Expires?.date?.start === "string" ? p.Expires.date.start : null
  };
}

function textProperty(value: string | null | undefined): { rich_text: Array<{ type: "text"; text: { content: string } }> } {
  return {
    rich_text: value
      ? [{ type: "text", text: { content: value.slice(0, 2000) } }]
      : []
  };
}

function titleProperty(value: string): { title: Array<{ type: "text"; text: { content: string } }> } {
  return {
    title: [{ type: "text", text: { content: value.slice(0, 2000) } }]
  };
}

function dateProperty(value: unknown): { date: { start: string } | null } {
  if (typeof value !== "string" || !value.trim()) return { date: null };
  return { date: { start: value } };
}

function atomMetadata(atom: Atom): Record<string, unknown> {
  return record(atom.metadata);
}

function atomToProperties(atom: Atom): Record<string, unknown> {
  const metadata = atomMetadata(atom);
  const subject = typeof metadata.subject === "string" ? metadata.subject : "";
  const memoryType = typeof metadata.memoryType === "string" &&
    ["Preference", "Fact", "Project", "Decision", "Context", "Instruction"].includes(metadata.memoryType)
    ? metadata.memoryType
    : "Context";
  const scope = typeof metadata.scope === "string" && ["Global", "Project", "Conversation"].includes(metadata.scope)
    ? metadata.scope
    : "Global";
  const importance = typeof metadata.importance === "string" &&
    ["Critical", "High", "Medium", "Low"].includes(metadata.importance)
    ? metadata.importance
    : null;

  return {
    Memory: titleProperty(subject || atom.content.slice(0, 120)),
    Content: textProperty(atom.content),
    Active: { checkbox: metadata.active !== false },
    Confidence: { number: typeof metadata.confidence === "number" ? metadata.confidence : null },
    Keywords: textProperty(atom.tags.join(", ")),
    Scope: { select: { name: scope } },
    Source: textProperty(typeof metadata.source === "string" ? metadata.source : null),
    Subject: textProperty(subject),
    Type: { select: { name: memoryType } },
    Importance: { select: importance ? { name: importance } : null },
    "Valid From": dateProperty(metadata.validFrom),
    Expires: dateProperty(metadata.expires)
  };
}

function parsedToMetadata(memory: ParsedMemory): Record<string, unknown> {
  return {
    active: memory.active,
    confidence: memory.confidence,
    scope: memory.scope,
    source: memory.source,
    subject: memory.subject || memory.title,
    memoryType: memory.memoryType,
    importance: memory.importance,
    validFrom: memory.validFrom,
    expires: memory.expires,
    notionPageId: memory.pageId,
    notionUrl: memory.url
  };
}

export class NotionSync {
  constructor(
    private readonly config: Config,
    private readonly db: Database,
    private readonly foundation: Foundation
  ) {}

  configured(): boolean {
    return Boolean(this.config.notionDataSourceId);
  }

  private async command(args: string[], input?: unknown): Promise<any> {
    return new Promise((resolve, reject) => {
      const child = spawn(this.config.notionCli, args, { stdio: ["pipe", "pipe", "pipe"] });
      let stdout = "";
      let stderr = "";
      child.stdout.setEncoding("utf8");
      child.stderr.setEncoding("utf8");
      child.stdout.on("data", chunk => { stdout += chunk; });
      child.stderr.on("data", chunk => { stderr += chunk; });
      child.on("error", reject);
      child.on("close", code => {
        if (code !== 0) {
          reject(new Error(`Notion CLI failed (${code ?? "signal"}): ${stderr.trim() || stdout.trim()}`));
          return;
        }
        try {
          resolve(stdout.trim() ? JSON.parse(stdout) : {});
        } catch {
          reject(new Error(`Notion CLI returned invalid JSON: ${stdout.slice(0, 500)}`));
        }
      });
      if (input === undefined) child.stdin.end();
      else child.stdin.end(JSON.stringify(input));
    });
  }

  async status(): Promise<Record<string, unknown>> {
    const bindings = await this.db.query(
      "SELECT count(*)::int AS count, max(last_synced_at) AS last_sync FROM external_bindings WHERE system='notion' AND entity_kind='atom'"
    );
    let cliAuthenticated = false;
    try {
      await new Promise<void>((resolve, reject) => {
        const child = spawn(this.config.notionCli, ["whoami"], { stdio: ["ignore", "ignore", "ignore"] });
        child.on("error", reject);
        child.on("close", code => code === 0 ? resolve() : reject(new Error("not authenticated")));
      });
      cliAuthenticated = true;
    } catch {
      cliAuthenticated = false;
    }
    return {
      configured: this.config.notionDataSourceId !== null,
      dataSourceId: this.config.notionDataSourceId,
      cli: this.config.notionCli,
      cliAuthenticated,
      bindings: bindings.rows[0]?.count ?? 0,
      lastSync: bindings.rows[0]?.last_sync ?? null
    };
  }

  private async queryAll(limit = 10_000): Promise<NotionPage[]> {
    const dataSourceId = this.config.notionDataSourceId;
    if (!dataSourceId) throw new Error("NOTION_DATA_SOURCE_ID is not configured");

    const pages: NotionPage[] = [];
    let cursor: string | null = null;
    do {
      const args = ["datasources", "query", dataSourceId, "--limit", "100", "--json"];
      if (cursor) args.push("--start-cursor", cursor);
      const result = await this.command(args) as NotionQuery;
      pages.push(...(Array.isArray(result.results) ? result.results : []));
      cursor = result.has_more && typeof result.next_cursor === "string" ? result.next_cursor : null;
    } while (cursor && pages.length < limit);

    return pages.slice(0, limit);
  }

  private async createPage(atom: Atom): Promise<NotionPage> {
    const dataSourceId = this.config.notionDataSourceId;
    if (!dataSourceId) throw new Error("NOTION_DATA_SOURCE_ID is not configured");
    return this.command(
      ["api", "/v1/pages", "-X", "POST", "-d", "@-"],
      { parent: { data_source_id: dataSourceId }, properties: atomToProperties(atom) }
    );
  }

  private async updatePage(pageId: string, atom: Atom): Promise<NotionPage> {
    return this.command(
      ["api", `/v1/pages/${pageId}`, "-X", "PATCH", "-d", "@-"],
      { properties: atomToProperties(atom) }
    );
  }

  private async bind(atom: Atom, page: NotionPage): Promise<void> {
    await this.db.transaction(async client => {
      await client.query(
        `DELETE FROM external_bindings
         WHERE system='notion'
           AND entity_kind='atom'
           AND entity_id=$1
           AND external_id<>$2`,
        [atom.id, page.id]
      );
      await client.query(
        `INSERT INTO external_bindings(
           system,entity_kind,entity_id,external_id,external_url,
           external_updated_at,foundation_updated_at,content_hash,last_synced_at,metadata
         )
         VALUES('notion','atom',$1,$2,$3,$4,$5,$6,NOW(),'{}'::jsonb)
         ON CONFLICT(system,external_id) DO UPDATE SET
           entity_kind='atom',
           entity_id=EXCLUDED.entity_id,
           external_url=EXCLUDED.external_url,
           external_updated_at=EXCLUDED.external_updated_at,
           foundation_updated_at=EXCLUDED.foundation_updated_at,
           content_hash=EXCLUDED.content_hash,
           metadata=external_bindings.metadata - 'notionMissing',
           last_synced_at=NOW(),
           updated_at=NOW()`,
        [
          atom.id,
          page.id,
          typeof page.url === "string" ? page.url : null,
          page.last_edited_time ? new Date(page.last_edited_time) : new Date(),
          atom.updatedAt,
          contentHash(atom.content)
        ]
      );
    });
  }

  private async bindingByExternal(pageId: string): Promise<BindingRow | null> {
    const result = await this.db.query(
      "SELECT * FROM external_bindings WHERE system='notion' AND entity_kind='atom' AND external_id=$1 LIMIT 1",
      [pageId]
    );
    return result.rows[0] ? {
      ...result.rows[0],
      entity_id: String(result.rows[0].entity_id),
      external_id: String(result.rows[0].external_id),
      metadata: record(result.rows[0].metadata)
    } as BindingRow : null;
  }

  private async atomRow(id: string): Promise<Atom | null> {
    try {
      return await this.foundation.getAtom(id);
    } catch {
      return null;
    }
  }

  private async pull(memory: ParsedMemory, atom?: Atom | null): Promise<Atom> {
    if (!atom) {
      const existing = await this.db.query("SELECT id::text FROM atoms WHERE content_hash=$1 LIMIT 1", [contentHash(memory.content)]);
      if (existing.rows[0]?.id) {
        atom = await this.foundation.getAtom(String(existing.rows[0].id));
      }
    }

    const metadata = parsedToMetadata(memory);
    if (!atom) {
      const created = await this.foundation.createAtom({
        content: memory.content,
        type: "imported",
        tags: memory.keywords,
        metadata,
        vector: null
      });
      return created.atom;
    }

    return this.foundation.updateAtom(atom.id, {
      content: memory.content,
      tags: memory.keywords,
      metadata,
      active: memory.active
    });
  }

  async sync(input: { dryRun?: boolean; limit?: number } = {}): Promise<Record<string, unknown>> {
    if (!this.config.notionDataSourceId) throw new Error("NOTION_DATA_SOURCE_ID is not configured");
    const dryRun = input.dryRun ?? false;
    const limit = Math.max(1, Math.min(input.limit ?? 10_000, 10_000));
    const pages = await this.queryAll(limit);
    const parsed = pages.map(parsePage).filter((value): value is ParsedMemory => value !== null);

    let imported = 0;
    let pulled = 0;
    let pushed = 0;
    let bound = 0;
    let unchanged = 0;
    let deactivatedMissing = 0;
    const conflicts: Array<{ atomId: string; pageId: string; winner: "foundation" | "notion" }> = [];

    for (const memory of parsed) {
      const binding = await this.bindingByExternal(memory.pageId);
      let atom = binding ? await this.atomRow(binding.entity_id) : null;

      if (!binding) {
        if (dryRun) {
          imported += 1;
          continue;
        }
        atom = await this.pull(memory, atom);
        const page = pages.find(value => value.id === memory.pageId)!;
        await this.bind(atom, page);
        imported += 1;
        bound += 1;
        continue;
      }
      if (!atom) {
        if (dryRun) {
          imported += 1;
          continue;
        }
        atom = await this.pull(memory, null);
        const page = pages.find(value => value.id === memory.pageId)!;
        await this.bind(atom, page);
        imported += 1;
        continue;
      }

      const externalBaseline = binding.external_updated_at?.getTime() ?? 0;
      const foundationBaseline = binding.foundation_updated_at?.getTime() ?? 0;
      const notionChanged = memory.updatedAt.getTime() > externalBaseline + 1000;
      const foundationChanged = atom.updatedAt.getTime() > foundationBaseline + 1000;

      if (!notionChanged && !foundationChanged) {
        unchanged += 1;
        continue;
      }

      if (notionChanged && (!foundationChanged || memory.updatedAt.getTime() > atom.updatedAt.getTime())) {
        if (foundationChanged) conflicts.push({ atomId: atom.id, pageId: memory.pageId, winner: "notion" });
        if (!dryRun) {
          atom = await this.pull(memory, atom);
          const page = pages.find(value => value.id === memory.pageId)!;
          await this.bind(atom, page);
        }
        pulled += 1;
      } else {
        if (notionChanged) conflicts.push({ atomId: atom.id, pageId: memory.pageId, winner: "foundation" });
        if (!dryRun) {
          const page = await this.updatePage(memory.pageId, atom);
          await this.bind(atom, page);
        }
        pushed += 1;
      }
    }

    // Missing external pages are reconciled only on a full sync. A partial
    // limit must never be interpreted as deletion of the pages not fetched.
    if (limit === 10_000 && pages.length < 10_000) {
      const pageIds = pages.map(page => page.id);
      const missing = await this.db.query(
        `SELECT entity_id::text,external_id
         FROM external_bindings
         WHERE system='notion'
           AND entity_kind='atom'
           AND NOT (external_id = ANY($1::text[]))`,
        [pageIds]
      );

      for (const row of missing.rows) {
        const atom = await this.atomRow(String(row.entity_id));
        if (!atom || record(atom.metadata).active === false) continue;
        deactivatedMissing += 1;
        if (!dryRun) {
          const updated = await this.foundation.updateAtom(atom.id, {
            active: false,
            metadata: {
              notionMissing: true,
              notionMissingSince: new Date().toISOString()
            }
          });
          await this.db.query(
            `UPDATE external_bindings
             SET foundation_updated_at=$2,
                 last_synced_at=NOW(),
                 metadata=metadata || $3::jsonb,
                 updated_at=NOW()
             WHERE system='notion' AND external_id=$1`,
            [
              String(row.external_id),
              updated.updatedAt,
              JSON.stringify({ notionMissing: true })
            ]
          );
        }
      }
    }

    const unbound = await this.db.query(
      `SELECT a.*
       FROM atoms a
       LEFT JOIN external_bindings b
         ON b.system='notion' AND b.entity_kind='atom' AND b.entity_id=a.id
       WHERE b.id IS NULL
       ORDER BY a.updated_at,a.id
       LIMIT $1`,
      [limit]
    );

    for (const row of unbound.rows) {
      const atom = {
        ...(await this.foundation.getAtom(String(row.id)))
      };
      if (!dryRun) {
        const page = await this.createPage(atom);
        await this.bind(atom, page);
      }
      pushed += 1;
      bound += dryRun ? 0 : 1;
    }

    return {
      dryRun,
      scannedNotion: parsed.length,
      imported,
      pulled,
      deactivatedMissing,
      pushed,
      bound,
      unchanged,
      conflicts
    };
  }
}
