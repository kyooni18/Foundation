import { fileURLToPath } from "node:url";

export type LLMProviderName = "none" | "apple" | "openai";
export type EmbeddingProviderName = "none" | "openai" | "gemini";
export type McpAccessLevel = "off" | "read" | "write";

export interface Config {
  databaseUrl: string;
  databasePoolSize: number;
  host: string;
  port: number;
  mcpPath: string;
  restPath: string;
  mcpFileAccess: McpAccessLevel;
  mcpAdminEnabled: boolean;
  mcpNotionEnabled: boolean;
  apiKey: string | null;
  timeZone: string;
  notionCli: string;
  notionDataSourceId: string | null;
  publicBaseUrl: string;
  oauthEnabled: boolean;
  oauthApprovalSecret: string | null;
  oauthAccessTtlSeconds: number;
  oauthRefreshTtlSeconds: number;

  llmProvider: LLMProviderName;
  embeddingProvider: EmbeddingProviderName;
  embeddingDimensions: 3072;

  openAIAPIKey: string | null;
  openAIBaseURL: string;
  openAILLMModel: string;
  openAIEmbeddingModel: string;
  openAIInterpretationModel: string;
  openAITranscriptionModel: string;

  geminiAPIKey: string | null;
  geminiBaseURL: string;
  geminiEmbeddingModel: string;

  appleFoundationBridge: string;
  aiTimeoutMs: number;
}

function int(name: string, fallback: number): number {
  const value = process.env[name];
  if (!value) return fallback;
  const parsed = Number(value);
  if (!Number.isInteger(parsed)) throw new Error(`${name} must be an integer`);
  return parsed;
}

function bool(name: string, fallback: boolean): boolean {
  const value = process.env[name]?.trim().toLowerCase();
  if (!value) return fallback;
  if (["1", "true", "yes", "on"].includes(value)) return true;
  if (["0", "false", "no", "off"].includes(value)) return false;
  throw new Error(`${name} must be true or false`);
}

function url(name: string, fallback: string): string {
  const value = (process.env[name] ?? fallback).replace(/\/+$/, "");
  const parsed = new URL(value);
  if (!["http:", "https:"].includes(parsed.protocol)) throw new Error(`${name} must use http or https`);
  return value;
}

function oneOf<T extends string>(name: string, value: string, allowed: readonly T[]): T {
  if (!allowed.includes(value as T)) {
    throw new Error(`${name} must be one of: ${allowed.join(", ")}`);
  }
  return value as T;
}

export function loadConfig(): Config {
  const legacyProvider = process.env.AI_PROVIDER?.trim().toLowerCase();
  const llmProvider = oneOf(
    "LLM_PROVIDER",
    (process.env.LLM_PROVIDER ?? legacyProvider ?? "none").trim().toLowerCase(),
    ["none", "apple", "openai"] as const
  );

  const defaultEmbeddingProvider = "none";
  const embeddingProvider = oneOf(
    "EMBEDDING_PROVIDER",
    (process.env.EMBEDDING_PROVIDER ?? defaultEmbeddingProvider).trim().toLowerCase(),
    ["none", "openai", "gemini"] as const
  );

  const embeddingDimensions = int("EMBEDDING_DIMENSIONS", 3072);
  if (embeddingDimensions !== 3072) {
    throw new Error("Foundation stores maximum-size embeddings as halfvec(3072); EMBEDDING_DIMENSIONS must be 3072");
  }

  const mcpPath = process.env.MCP_PATH?.trim() || "/mcp";
  if (!mcpPath.startsWith("/")) throw new Error("MCP_PATH must start with /");
  const restPath = process.env.REST_PATH?.trim() || "/api/v1";
  if (!restPath.startsWith("/")) throw new Error("REST_PATH must start with /");

  const defaultAppleBridge = fileURLToPath(
    new URL("../native/apple-foundation-model/.build/release/foundation-apple-bridge", import.meta.url)
  );

  const genericEmbeddingModel = process.env.EMBEDDING_MODEL?.trim();

  const host = process.env.HOST ?? "127.0.0.1";
  const port = int("PORT", 8787);
  const publicBaseUrl = url("FOUNDATION_PUBLIC_BASE_URL", `http://${host}:${port}`);
  const apiKey = process.env.FOUNDATION_API_KEY?.trim() || null;
  const oauthApprovalSecret = process.env.OAUTH_APPROVAL_SECRET?.trim() || apiKey;
  const oauthEnabled = (process.env.OAUTH_ENABLED ?? (oauthApprovalSecret ? "true" : "false")).toLowerCase() === "true";

  return {
    databaseUrl: process.env.DATABASE_URL ?? "postgresql://foundation:foundation@127.0.0.1:5432/foundation",
    databasePoolSize: Math.max(1, int("DATABASE_POOL_SIZE", 10)),
    host,
    port,
    mcpPath,
    restPath,
    mcpFileAccess: oneOf("FOUNDATION_MCP_FILE_ACCESS", (process.env.FOUNDATION_MCP_FILE_ACCESS ?? "off").trim().toLowerCase(), ["off", "read", "write"] as const),
    mcpAdminEnabled: bool("FOUNDATION_MCP_ADMIN", false),
    mcpNotionEnabled: bool("FOUNDATION_MCP_NOTION", false),
    apiKey,
    timeZone: process.env.FOUNDATION_TIME_ZONE?.trim() || Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",

    notionCli: process.env.NOTION_CLI?.trim() || "ntn",
    notionDataSourceId: process.env.NOTION_DATA_SOURCE_ID?.trim() || null,
    publicBaseUrl,
    oauthEnabled,
    oauthApprovalSecret,
    oauthAccessTtlSeconds: Math.max(300, int("OAUTH_ACCESS_TTL_SECONDS", 3600)),
    oauthRefreshTtlSeconds: Math.max(3600, int("OAUTH_REFRESH_TTL_SECONDS", 30 * 24 * 3600)),
    llmProvider,
    embeddingProvider,
    embeddingDimensions: 3072,

    openAIAPIKey: process.env.OPENAI_API_KEY?.trim() || null,
    openAIBaseURL: url("OPENAI_BASE_URL", "https://api.openai.com/v1"),
    openAILLMModel: process.env.OPENAI_LLM_MODEL?.trim() || process.env.EXTRACTION_MODEL?.trim() || "gpt-5.6-luna",
    openAIEmbeddingModel: process.env.OPENAI_EMBEDDING_MODEL?.trim() || genericEmbeddingModel || "text-embedding-3-large",
    openAIInterpretationModel: process.env.OPENAI_INTERPRETATION_MODEL?.trim() || process.env.OPENAI_LLM_MODEL?.trim() || process.env.EXTRACTION_MODEL?.trim() || "gpt-5.6-luna",
    openAITranscriptionModel: process.env.OPENAI_TRANSCRIPTION_MODEL?.trim() || "gpt-4o-mini-transcribe",

    geminiAPIKey: process.env.GEMINI_API_KEY?.trim() || null,
    geminiBaseURL: url("GEMINI_BASE_URL", "https://generativelanguage.googleapis.com/v1beta"),
    geminiEmbeddingModel: process.env.GEMINI_EMBEDDING_MODEL?.trim() ||
      (embeddingProvider === "gemini" ? genericEmbeddingModel : null) ||
      "gemini-embedding-2",

    appleFoundationBridge: process.env.APPLE_FOUNDATION_BRIDGE?.trim() || defaultAppleBridge,
    aiTimeoutMs: Math.max(1_000, Math.min(int("AI_TIMEOUT_MS", 30_000), 120_000))
  };
}
