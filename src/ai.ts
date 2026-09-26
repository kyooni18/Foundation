import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import type { Config } from "./config.js";
import type { ExtractionResult } from "./types.js";
import { normalizeTags, normalizeText } from "./utils.js";

export interface ExtractionContext {
  sourceTime?: string;
  timeZone?: string;
}

export type MemoryResolutionAction = "new" | "duplicate" | "update" | "supersede" | "conflict";

export interface MemoryResolutionCandidate {
  id: string;
  content: string;
  tags: string[];
  active: boolean;
  similarity: number;
  createdAt: string;
  updatedAt: string;
  metadata: Record<string, unknown>;
}

export interface MemoryResolutionInput {
  content: string;
  tags: string[];
  metadata: Record<string, unknown>;
  sourceTime: string;
  timeZone: string;
  candidates: MemoryResolutionCandidate[];
}

export interface MemoryResolution {
  action: MemoryResolutionAction;
  targetAtomId: string | null;
  canonicalContent: string;
  tags: string[];
  confidence: number;
  rationale: string;
}

export interface LLMProvider {
  extractNote(text: string, context?: ExtractionContext): Promise<ExtractionResult>;
  resolveMemory?(input: MemoryResolutionInput): Promise<MemoryResolution>;
}

export type EmbeddingPurpose = "document" | "query";

export interface EmbeddingResult {
  vector: number[];
  provider: "openai" | "gemini";
  model: string;
  dimensions: 3072;
}

export type EmbeddingIdentity = Omit<EmbeddingResult, "vector">;

export interface EmbeddingProvider {
  identity(): EmbeddingIdentity | null;
  embed(text: string, purpose?: EmbeddingPurpose): Promise<EmbeddingResult | null>;
}

export interface AIProvider extends LLMProvider, EmbeddingProvider {}

export class NoLLMProvider implements LLMProvider {
  async extractNote(): Promise<ExtractionResult> {
    return { subject: null, tags: [], atoms: [] };
  }
}

export class NoEmbeddingProvider implements EmbeddingProvider {
  identity(): null {
    return null;
  }

  async embed(): Promise<null> {
    return null;
  }
}

class OpenAIClient {
  constructor(private readonly config: Config) {}

  async request(path: string, body: Record<string, unknown>): Promise<any> {
    if (!this.config.openAIAPIKey) {
      throw new Error("OPENAI_API_KEY is required for the selected OpenAI provider");
    }

    const response = await fetch(`${this.config.openAIBaseURL}${path}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${this.config.openAIAPIKey}`
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(this.config.aiTimeoutMs)
    });

    if (!response.ok) {
      throw new Error(`OpenAI request failed (${response.status}): ${(await response.text()).slice(0, 500)}`);
    }
    return response.json();
  }
}

function parseExtraction(value: unknown): ExtractionResult {
  const parsed = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const subject = typeof parsed.subject === "string"
    ? normalizeText(parsed.subject).slice(0, 500) || null
    : null;
  const tags = normalizeTags(
    Array.isArray(parsed.tags)
      ? parsed.tags.filter((item): item is string => typeof item === "string")
      : []
  );

  const iso = (value: unknown): string | null | undefined => {
    if (value === null) return null;
    if (typeof value !== "string" || !value.trim()) return undefined;
    const date = new Date(value);
    return Number.isFinite(date.getTime()) ? date.toISOString() : undefined;
  };
  const memoryTypes = new Set(["Preference", "Fact", "Project", "Decision", "Context", "Instruction"]);
  const scopes = new Set(["Global", "Project", "Conversation"]);

  const atoms = Array.isArray(parsed.atoms)
    ? parsed.atoms.flatMap(item => {
        if (!item || typeof item !== "object") return [];
        const atom = item as Record<string, unknown>;
        if (typeof atom.content !== "string") return [];
        const content = normalizeText(atom.content);
        if (!content) return [];
        const atomTags = normalizeTags(
          Array.isArray(atom.tags)
            ? atom.tags.filter((tag): tag is string => typeof tag === "string")
            : []
        );
        const confidence = typeof atom.confidence === "number" && Number.isFinite(atom.confidence)
          ? Math.max(0, Math.min(1, atom.confidence))
          : undefined;
        return [{
          content,
          tags: atomTags,
          validFrom: iso(atom.validFrom),
          expires: iso(atom.expires),
          subject: typeof atom.subject === "string" ? normalizeText(atom.subject).slice(0, 500) || null : undefined,
          confidence,
          memoryType: typeof atom.memoryType === "string" && memoryTypes.has(atom.memoryType)
            ? atom.memoryType as "Preference" | "Fact" | "Project" | "Decision" | "Context" | "Instruction"
            : undefined,
          scope: typeof atom.scope === "string" && scopes.has(atom.scope)
            ? atom.scope as "Global" | "Project" | "Conversation"
            : undefined
        }];
      }).slice(0, 32)
    : [];

  return { subject, tags, atoms };
}

export class OpenAIEmbeddingProvider implements EmbeddingProvider {
  private readonly client: OpenAIClient;

  constructor(private readonly config: Config) {
    this.client = new OpenAIClient(config);
  }

  identity(): EmbeddingIdentity {
    return {
      provider: "openai",
      model: this.config.openAIEmbeddingModel,
      dimensions: this.config.embeddingDimensions
    };
  }

  async embed(text: string): Promise<EmbeddingResult> {
    const result = await this.client.request("/embeddings", {
      model: this.config.openAIEmbeddingModel,
      input: text,
      dimensions: this.config.embeddingDimensions,
      encoding_format: "float"
    });

    const vector = result?.data?.[0]?.embedding;
    if (
      !Array.isArray(vector) ||
      vector.length !== this.config.embeddingDimensions ||
      vector.some((item: unknown) => typeof item !== "number" || !Number.isFinite(item))
    ) {
      throw new Error(`OpenAI embedding provider did not return vector(${this.config.embeddingDimensions})`);
    }

    return {
      vector: vector as number[],
      provider: "openai",
      model: this.config.openAIEmbeddingModel,
      dimensions: this.config.embeddingDimensions
    };
  }
}


export class GeminiEmbeddingProvider implements EmbeddingProvider {
  constructor(private readonly config: Config) {}

  identity(): EmbeddingIdentity {
    return {
      provider: "gemini",
      model: this.config.geminiEmbeddingModel.replace(/^models\//, ""),
      dimensions: this.config.embeddingDimensions
    };
  }

  private prepareText(text: string, purpose: EmbeddingPurpose): {
    text: string;
    taskType?: string;
  } {
    if (this.config.geminiEmbeddingModel === "gemini-embedding-001") {
      return {
        text,
        taskType: purpose === "query" ? "RETRIEVAL_QUERY" : "RETRIEVAL_DOCUMENT"
      };
    }

    return {
      text: purpose === "query"
        ? `task: search result | query: ${text}`
        : `title: none | text: ${text}`
    };
  }

  async embed(text: string, purpose: EmbeddingPurpose = "document"): Promise<EmbeddingResult> {
    if (!this.config.geminiAPIKey) {
      throw new Error("GEMINI_API_KEY is required when EMBEDDING_PROVIDER=gemini");
    }

    const model = this.config.geminiEmbeddingModel.replace(/^models\//, "");
    const prepared = this.prepareText(text, purpose);
    const body: Record<string, unknown> = {
      model: `models/${model}`,
      content: {
        parts: [{ text: prepared.text }]
      },
      output_dimensionality: this.config.embeddingDimensions
    };
    if (prepared.taskType) body.taskType = prepared.taskType;

    const response = await fetch(
      `${this.config.geminiBaseURL}/models/${encodeURIComponent(model)}:embedContent`,
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-goog-api-key": this.config.geminiAPIKey
        },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(this.config.aiTimeoutMs)
      }
    );

    if (!response.ok) {
      throw new Error(`Gemini embedding request failed (${response.status}): ${(await response.text()).slice(0, 500)}`);
    }

    const result = await response.json() as any;
    const vector = result?.embeddings?.[0]?.values ?? result?.embedding?.values;
    if (
      !Array.isArray(vector) ||
      vector.length !== this.config.embeddingDimensions ||
      vector.some((item: unknown) => typeof item !== "number" || !Number.isFinite(item))
    ) {
      throw new Error(`Gemini embedding provider did not return vector(${this.config.embeddingDimensions})`);
    }

    return {
      vector: vector as number[],
      provider: "gemini",
      model,
      dimensions: this.config.embeddingDimensions
    };
  }
}

function parseMemoryResolution(value: unknown, input: MemoryResolutionInput): MemoryResolution {
  const parsed = value && typeof value === "object" ? value as Record<string, unknown> : {};
  const allowed = new Set<MemoryResolutionAction>(["new", "duplicate", "update", "supersede", "conflict"]);
  const action = allowed.has(parsed.action as MemoryResolutionAction)
    ? parsed.action as MemoryResolutionAction
    : "new";
  const candidateIds = new Set(input.candidates.map(candidate => candidate.id));
  const targetAtomId = typeof parsed.targetAtomId === "string" && candidateIds.has(parsed.targetAtomId)
    ? parsed.targetAtomId
    : null;
  const canonicalContent = typeof parsed.canonicalContent === "string"
    ? normalizeText(parsed.canonicalContent) || input.content
    : input.content;
  const tags = normalizeTags([
    ...input.tags,
    ...(Array.isArray(parsed.tags)
      ? parsed.tags.filter((tag): tag is string => typeof tag === "string")
      : [])
  ]);
  const confidence = typeof parsed.confidence === "number" && Number.isFinite(parsed.confidence)
    ? Math.max(0, Math.min(1, parsed.confidence))
    : 0.5;
  const rationale = typeof parsed.rationale === "string"
    ? normalizeText(parsed.rationale).slice(0, 1000)
    : "";

  const resolvedAction = action !== "new" && !targetAtomId ? "new" : action;
  return {
    action: resolvedAction,
    targetAtomId: resolvedAction === "new" ? null : targetAtomId,
    canonicalContent,
    tags,
    confidence,
    rationale
  };
}

export class OpenAILLMProvider implements LLMProvider {
  private readonly client: OpenAIClient;

  constructor(private readonly config: Config) {
    this.client = new OpenAIClient(config);
  }

  async extractNote(text: string, context: ExtractionContext = {}): Promise<ExtractionResult> {
    const sourceTime = context.sourceTime ?? new Date().toISOString();
    const timeZone = context.timeZone ?? "UTC";
    const result = await this.client.request("/chat/completions", {
      model: this.config.openAILLMModel,
      messages: [
        {
          role: "system",
          content: `Extract a short subject, note tags, and minimal independently useful facts from the raw note. Split distinct relations; do not paraphrase the whole note as one fact when it contains several. Preserve the user's viewpoint and uncertainty; never turn tentative wording into fact.
Captured ${sourceTime} (${timeZone}). Resolve relative dates against capture time; set validFrom to the supported ISO-8601 start and expires only for temporary facts, otherwise null.
Return JSON only: {subject,tags,atoms:[{content,tags,validFrom,expires,subject,confidence,memoryType,scope}]}. memoryType: Preference|Fact|Project|Decision|Context|Instruction; scope: Global|Project|Conversation. Use null for unknown optional values.`
        },
        { role: "user", content: text }
      ],
      response_format: { type: "json_object" },
      max_completion_tokens: 1800
    });

    const raw = result?.choices?.[0]?.message?.content;
    if (typeof raw !== "string") throw new Error("OpenAI LLM returned no JSON content");
    return parseExtraction(JSON.parse(raw));
  }
  async resolveMemory(input: MemoryResolutionInput): Promise<MemoryResolution> {
    const candidateSummary = input.candidates.map(candidate => ({
      id: candidate.id,
      content: candidate.content,
      tags: candidate.tags,
      active: candidate.active,
      similarity: Number(candidate.similarity.toFixed(4)),
      updatedAt: candidate.updatedAt,
      metadata: Object.fromEntries(
        ["validFrom", "expires", "memoryType", "scope", "subject"]
          .filter(key => candidate.metadata[key] !== undefined)
          .map(key => [key, candidate.metadata[key]])
      )
    }));

    const result = await this.client.request("/chat/completions", {
      model: this.config.openAILLMModel,
      messages: [
        {
          role: "system",
          content: `Reconcile one new durable-memory fact against candidate Atoms. Choose:
new = independent fact; duplicate = same fact, keep existing; update = compatible correction/refinement without meaningful history loss; supersede = newer value replaces a mutable historical state, keep old inactive and create current; conflict = incompatible facts with no clear recency/authority, keep both.
Judge recency by sourceTime and validFrom/updatedAt. Similarity alone is not identity; do not supersede timeless facts. Prefer duplicate if meaning is unchanged and supersede if history matters. When unsure, use conflict or new; never silently overwrite.
Return JSON only: {action,targetAtomId,canonicalContent,tags,confidence,rationale}. For every action except new, targetAtomId must be a supplied candidate ID.`
        },
        {
          role: "user",
          content: JSON.stringify({
            incoming: {
              content: input.content,
              tags: input.tags,
              metadata: Object.fromEntries(
                ["validFrom", "expires", "memoryType", "scope", "subject"]
                  .filter(key => input.metadata[key] !== undefined)
                  .map(key => [key, input.metadata[key]])
              ),
              sourceTime: input.sourceTime,
              timeZone: input.timeZone
            },
            candidates: candidateSummary
          })
        }
      ],
      response_format: { type: "json_object" },
      max_completion_tokens: 1200
    });

    const raw = result?.choices?.[0]?.message?.content;
    if (typeof raw !== "string") throw new Error("OpenAI LLM returned no memory resolution JSON");
    return parseMemoryResolution(JSON.parse(raw), input);
  }

}

export class AppleFoundationLLMProvider implements LLMProvider {
  constructor(private readonly config: Config) {}

  private async ensureBridge(): Promise<void> {
    if (process.platform !== "darwin") {
      throw new Error("Apple Foundation Models provider requires macOS");
    }

    try {
      await access(this.config.appleFoundationBridge, fsConstants.X_OK);
    } catch {
      throw new Error(
        `Apple Foundation Models bridge is not executable at ${this.config.appleFoundationBridge}. Run npm run build:apple-model or set APPLE_FOUNDATION_BRIDGE.`
      );
    }
  }

  private async callBridge(payload: Record<string, unknown>): Promise<Record<string, unknown>> {
    await this.ensureBridge();

    const child = spawn(this.config.appleFoundationBridge, [], {
      stdio: ["pipe", "pipe", "pipe"]
    });

    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", chunk => { stdout += chunk; });
    child.stderr.on("data", chunk => { stderr += chunk; });

    const completion = new Promise<number | null>((resolve, reject) => {
      child.on("error", reject);
      child.on("close", resolve);
    });
    child.stdin.end(JSON.stringify(payload));

    const timeout = setTimeout(() => child.kill("SIGKILL"), this.config.aiTimeoutMs);
    let status: number | null;
    try {
      status = await completion;
    } finally {
      clearTimeout(timeout);
    }

    if (status !== 0) {
      throw new Error(`Apple Foundation Models bridge failed (${status ?? "signal"}): ${stderr.trim() || stdout.trim() || "unknown error"}`);
    }

    let decoded: unknown;
    try {
      decoded = JSON.parse(stdout);
    } catch {
      throw new Error(`Apple Foundation Models bridge returned invalid JSON: ${stdout.slice(0, 500)}`);
    }

    const envelope = decoded && typeof decoded === "object" ? decoded as Record<string, unknown> : {};
    if (envelope.ok !== true) {
      throw new Error(`Apple Foundation Models bridge error: ${String(envelope.error ?? "unknown error")}`);
    }
    return envelope;
  }

  async extractNote(text: string, context: ExtractionContext = {}): Promise<ExtractionResult> {
    const envelope = await this.callBridge({
      operation: "extractNote",
      text,
      sourceTime: context.sourceTime ?? new Date().toISOString(),
      timeZone: context.timeZone ?? "UTC"
    });
    return parseExtraction(envelope.extraction);
  }

  async resolveMemory(input: MemoryResolutionInput): Promise<MemoryResolution> {
    const envelope = await this.callBridge({
      operation: "resolveMemory",
      text: input.content,
      sourceTime: input.sourceTime,
      timeZone: input.timeZone,
      tags: input.tags,
      metadataJSON: JSON.stringify(input.metadata),
      candidates: input.candidates.map(candidate => ({
        id: candidate.id,
        content: candidate.content,
        tags: candidate.tags,
        active: candidate.active,
        similarity: candidate.similarity,
        createdAt: candidate.createdAt,
        updatedAt: candidate.updatedAt,
        metadataJSON: JSON.stringify(candidate.metadata)
      }))
    });
    return parseMemoryResolution(envelope.resolution, input);
  }
}

class CompositeAIProvider implements AIProvider {
  constructor(
    private readonly llm: LLMProvider,
    private readonly embeddings: EmbeddingProvider
  ) {}

  extractNote(text: string, context?: ExtractionContext): Promise<ExtractionResult> {
    return this.llm.extractNote(text, context);
  }

  resolveMemory(input: MemoryResolutionInput): Promise<MemoryResolution> {
    if (!this.llm.resolveMemory) {
      return Promise.resolve({
        action: "new",
        targetAtomId: null,
        canonicalContent: input.content,
        tags: input.tags,
        confidence: 0.5,
        rationale: "No memory resolver is available"
      });
    }
    return this.llm.resolveMemory(input);
  }

  identity(): EmbeddingIdentity | null {
    return this.embeddings.identity();
  }

  embed(text: string, purpose?: EmbeddingPurpose): Promise<EmbeddingResult | null> {
    return this.embeddings.embed(text, purpose);
  }
}

export function createAIProvider(config: Config): AIProvider {
  let llm: LLMProvider;
  switch (config.llmProvider) {
    case "apple":
      llm = new AppleFoundationLLMProvider(config);
      break;
    case "openai":
      llm = new OpenAILLMProvider(config);
      break;
    case "none":
      llm = new NoLLMProvider();
      break;
  }

  let embeddings: EmbeddingProvider;
  switch (config.embeddingProvider) {
    case "openai":
      embeddings = new OpenAIEmbeddingProvider(config);
      break;
    case "gemini":
      embeddings = new GeminiEmbeddingProvider(config);
      break;
    case "none":
      embeddings = new NoEmbeddingProvider();
      break;
  }

  return new CompositeAIProvider(llm, embeddings);
}
