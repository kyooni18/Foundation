import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import * as z from "zod/v4";
import type { Config } from "./config.js";
import type { Foundation } from "./foundation.js";
import type { NotionSync } from "./notion.js";

const metadata = z.record(z.string(), z.unknown());

type FacadeAction = {
  schema: z.ZodType;
  handler: (input: any) => Promise<any>;
  fields: string[];
  omitFileText?: boolean;
};

type FacadeAnnotations = {
  readOnlyHint: boolean;
  destructiveHint: boolean;
  idempotentHint: boolean;
  openWorldHint: boolean;
};

function compact(value: unknown, omitFileText = false): unknown {
  if (value instanceof Date) return value.toISOString();
  if (Array.isArray(value)) return value.map(item => compact(item, omitFileText));
  if (!value || typeof value !== "object") return value;

  const source = value as Record<string, unknown>;
  const result: Record<string, unknown> = {};
  for (const [key, item] of Object.entries(source)) {
    const provenance = item && typeof item === "object" && "embeddedAt" in item;
    if (key === "base64" || key === "normalizedContent" ||
        (key === "embedding" && provenance) ||
        key === "subjectEmbedding" || key === "contentEmbedding") continue;
    if (omitFileText && (key === "content" || key === "interpreted")) continue;
    if (key === "metadata" && item && typeof item === "object" && !Array.isArray(item) && Object.keys(item).length === 0) continue;
    result[key] = compact(item, omitFileText);
  }
  if ("base64" in source) result.hasBinary = Boolean(source.base64);
  if (typeof result.score === "number") result.score = Number(result.score.toFixed(3));
  return result;
}

function ok(value: unknown, omitFileText = false) {
  return { content: [{ type: "text" as const, text: JSON.stringify(compact(value, omitFileText)) }] };
}

function failure(error: unknown) {
  return {
    content: [{
      type: "text" as const,
      text: `Foundation error: ${error instanceof Error ? error.message : String(error)}`
    }],
    isError: true
  };
}

function tool<T>(handler: (input: T) => Promise<unknown>, options: { omitFileText?: boolean } = {}) {
  return async (input: T) => {
    try {
      return ok(await handler(input), options.omitFileText);
    } catch (error) {
      return failure(error);
    }
  };
}

function defineAction<T>(
  actions: Map<string, FacadeAction>,
  name: string,
  schema: z.ZodType<T>,
  handler: (input: T) => Promise<any>,
  options: { omitFileText?: boolean } = {}
): void {
  const shape = (schema as unknown as { shape?: Record<string, unknown> }).shape;
  actions.set(name, {
    schema,
    handler,
    fields: Object.keys(shape ?? {}),
    omitFileText: options.omitFileText
  });
}

function registerFacade(
  server: McpServer,
  name: string,
  title: string,
  description: string,
  actions: Map<string, FacadeAction>,
  annotations: FacadeAnnotations
): void {
  const names = [...actions.keys()];
  if (names.length === 0) return;
  const actionEnum = z.enum(names as [string, ...string[]]);
  const actionSummary = [...actions]
    .map(([action, config]) => `${action}(${config.fields.join(",")})`)
    .join("; ");

  server.registerTool(
    name,
    {
      title,
      description: `${description} Actions: ${actionSummary}.`,
      inputSchema: z.object({
        action: actionEnum,
        input: z.record(z.string(), z.unknown()).default({})
      }),
      annotations
    },
    async ({ action, input }) => {
      const selected = actions.get(action);
      if (!selected) return failure(`Unknown ${name} action: ${action}`);
      try {
        const parsed = selected.schema.parse(input);
        return ok(await selected.handler(parsed), selected.omitFileText);
      } catch (error) {
        return failure(error);
      }
    }
  );
}

export function mcpModelToolNames(config: Pick<Config,
  "mcpFileAccess" | "mcpAdminEnabled" | "mcpNotionEnabled"
>, notionAvailable = true): string[] {
  const names = ["recall", "remember", "forget"];
  if (config.mcpFileAccess !== "off") names.push("file");
  if (config.mcpAdminEnabled) names.push("admin");
  if (config.mcpNotionEnabled && notionAvailable) names.push("notion");
  return names;
}

export function mcpToolCallIsReadOnly(name: string, args: unknown): boolean {
  const input = args && typeof args === "object" ? args as Record<string, unknown> : {};
  const action = typeof input.action === "string" ? input.action : "";
  if (name === "recall") return true;
  if (name === "remember" || name === "forget") return false;
  if (name === "file") return ["get", "list"].includes(action);
  if (name === "admin") return action === "stats";
  if (name === "notion") return action === "status";
  return false;
}

export function createMcpServer(
  foundation: Foundation,
  config: Pick<Config,
    "mcpFileAccess" | "mcpAdminEnabled" | "mcpNotionEnabled"
  >,
  notion?: NotionSync
): McpServer {
  const modelTools = mcpModelToolNames(config, Boolean(notion));
  const server = new McpServer(
    { name: "foundation", version: "0.1.0" },
    {
      capabilities: { logging: {} },
      instructions: `Foundation exposes high-level memory facades: ${modelTools.join(", ")}. recall retrieves durable knowledge, remember reconciles durable knowledge, and forget removes a recalled memory from active memory by exact memory ID. File, admin, and Notion surfaces are opt-in server settings.`
    }
  );

  server.registerTool(
    "recall",
    {
      title: "Recall",
      description: "Retrieve durable knowledge using exact, lexical, tag, and semantic matching across Foundation memory. Returns only the recalled memory id and text.",
      inputSchema: z.object({
        query: z.string().min(1).max(20_000),
        limit: z.number().int().min(1).max(100).default(10)
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }
    },
    tool(async ({ query, limit }) => (await foundation.search(query, limit)).map(({ id, rawText }) => ({ id, text: rawText })))
  );

  server.registerTool(
    "remember",
    {
      title: "Remember",
      description: "Store durable knowledge through Foundation's reconciliation path. Compound input is split into Atoms and duplicate, update, supersede, and conflict handling is automatic and conservative. Returns only unique resulting memory IDs.",
      inputSchema: z.object({
        content: z.string().min(1).max(100_000),
        sourceTime: z.string().datetime({ offset: true }).optional(),
        requestId: z.string().min(1).max(500).optional(),
        tags: z.array(z.string().max(100)).max(100).default([]),
        metadata: metadata.default({})
      }),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: true }
    },
    tool(async input => ({ ids: [...new Set((await foundation.remember(input)).results.map(({ atom }) => atom.id))] }))
  );

  server.registerTool(
    "forget",
    {
      title: "Forget",
      description: "Remove one durable memory from active recall by exact memory ID. Use an id returned by recall. This does not expose or return low-level Atom data.",
      inputSchema: z.object({
        id: z.string().uuid()
      }),
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true, openWorldHint: false }
    },
    tool(({ id }) => foundation.forget(id))
  );


  if (config.mcpFileAccess !== "off") {
    const file = new Map<string, FacadeAction>();
    defineAction(file, "get", z.object({ id: z.string().uuid() }), ({ id }) => foundation.getFile(id));
    defineAction(
      file,
      "list",
      z.object({
        limit: z.number().int().min(1).max(500).default(20),
        offset: z.number().int().min(0).default(0)
      }),
      ({ limit, offset }) => foundation.listFiles(limit, offset),
      { omitFileText: true }
    );

    if (config.mcpFileAccess === "write") {
      defineAction(
        file,
        "ingest",
        z.object({
          rawText: z.string().min(1).max(1_000_000),
          name: z.string().max(500).optional(),
          vaultUid: z.string().max(1_000).nullable().optional(),
          sourceTime: z.string().datetime({ offset: true }).optional(),
          metadata: metadata.default({})
        }),
        input => foundation.ingestNote(input)
      );
      defineAction(
        file,
        "create",
        z.object({
          name: z.string().min(1).max(500),
          vaultUid: z.string().max(1_000).nullable().optional(),
          mimeType: z.string().max(200).nullable().optional(),
          base64: z.string().nullable().optional(),
          content: z.string().max(5_000_000).nullable().optional(),
          interpreted: z.string().max(1_000_000).nullable().optional(),
          subject: z.string().max(1_000).nullable().optional(),
          tags: z.array(z.string().max(100)).max(100).default([]),
          metadata: metadata.default({})
        }),
        input => foundation.createFile(input)
      );
      defineAction(
        file,
        "update",
        z.object({
          id: z.string().uuid(),
          name: z.string().min(1).max(500).optional(),
          vaultUid: z.string().max(1_000).nullable().optional(),
          mimeType: z.string().max(200).nullable().optional(),
          base64: z.string().nullable().optional(),
          content: z.string().max(5_000_000).nullable().optional(),
          interpreted: z.string().max(1_000_000).nullable().optional(),
          subject: z.string().max(1_000).nullable().optional(),
          tags: z.array(z.string().max(100)).max(100).optional(),
          metadata: metadata.optional()
        }),
        ({ id, ...input }) => foundation.updateFile(id, input)
      );
      defineAction(file, "interpret", z.object({ id: z.string().uuid() }), ({ id }) => foundation.interpretFile(id));
      defineAction(file, "delete", z.object({ id: z.string().uuid() }), ({ id }) => foundation.deleteFile(id));
    }

    registerFacade(
      server,
      "file",
      "File",
      config.mcpFileAccess === "write"
        ? "Inspect and manage Foundation source Files. Vault and FileLink plumbing stays internal."
        : "Inspect Foundation source Files. File mutation is disabled by server policy.",
      file,
      config.mcpFileAccess === "write"
        ? { readOnlyHint: false, destructiveHint: true, idempotentHint: false, openWorldHint: true }
        : { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }
    );
  }

  if (config.mcpAdminEnabled) {
    const admin = new Map<string, FacadeAction>();
    defineAction(admin, "stats", z.object({}), () => foundation.stats());
    defineAction(
      admin,
      "reindex",
      z.object({
        kind: z.enum(["all", "atoms", "files"]).default("all"),
        limit: z.number().int().min(1).max(10_000).default(1000),
        force: z.boolean().default(false),
        concurrency: z.number().int().min(1).max(16).default(4)
      }),
      input => foundation.reindex(input)
    );
    registerFacade(
      server,
      "admin",
      "Foundation Admin",
      "Optional operational facade for Foundation health and indexing maintenance.",
      admin,
      { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: true }
    );
  }

  if (config.mcpNotionEnabled && notion) {
    const notionFacade = new Map<string, FacadeAction>();
    defineAction(notionFacade, "status", z.object({}), () => notion.status());
    defineAction(
      notionFacade,
      "sync",
      z.object({
        dryRun: z.boolean().default(false),
        limit: z.number().int().min(1).max(10_000).default(10_000)
      }),
      input => notion.sync(input)
    );
    registerFacade(
      server,
      "notion",
      "Notion Sync",
      "Optional high-level Foundation/Notion synchronization facade.",
      notionFacade,
      { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: true }
    );
  }

  return server;
}
