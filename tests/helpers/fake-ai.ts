import { createHash } from "node:crypto";
import type { AIProvider, EmbeddingResult, MemoryResolution, MemoryResolutionInput } from "../../src/ai.js";
import type { ExtractionResult } from "../../src/types.js";

const DIMENSIONS = 3072;
const STOP = new Set("a an the is are of to in on for and or with this that it be use uses using".split(" "));

export function tokenize(text: string): string[] {
  return (text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [])
    .map(token => (token.length > 3 && token.endsWith("s") ? token.slice(0, -1) : token))
    .filter(token => !STOP.has(token));
}

/** Deterministic bag-of-words embedding: overlapping vocabulary => high cosine, disjoint => ~0. */
export function bagOfWordsVector(text: string): number[] {
  const vector = new Array<number>(DIMENSIONS).fill(0);
  for (const token of tokenize(text)) {
    const digest = createHash("sha256").update(token).digest();
    for (let i = 0; i < 4; i++) {
      const index = digest.readUInt16BE(i * 2) % DIMENSIONS;
      vector[index]! += digest[8 + i]! % 2 === 0 ? 1 : -1;
    }
  }
  const norm = Math.sqrt(vector.reduce((sum, value) => sum + value * value, 0)) || 1;
  return vector.map(value => value / norm);
}

export type Resolver = (input: MemoryResolutionInput) => MemoryResolution | Promise<MemoryResolution>;

export class FakeAI implements AIProvider {
  embeddingsFail = false;
  embedCalls = 0;
  resolver: Resolver | null = null;
  /** Split extraction input on ";" into several Atoms when enabled. */
  splitOnSemicolon = false;
  scopeForAtoms: "Global" | "Project" | "Conversation" | null = null;

  identity() {
    return { provider: "gemini" as const, model: "fake-bow", dimensions: 3072 as const };
  }

  async embed(text: string): Promise<EmbeddingResult | null> {
    this.embedCalls += 1;
    if (this.embeddingsFail) throw new Error("embedding provider unavailable");
    return { vector: bagOfWordsVector(text), provider: "gemini", model: "fake-bow", dimensions: 3072 };
  }

  async extractNote(text: string): Promise<ExtractionResult> {
    if (!this.splitOnSemicolon) return { subject: null, tags: [], atoms: [] };
    const parts = text.split(";").map(part => part.trim()).filter(Boolean);
    return {
      subject: null,
      tags: [],
      atoms: parts.map(content => ({ content, tags: [], scope: this.scopeForAtoms }))
    };
  }

  async resolveMemory(input: MemoryResolutionInput): Promise<MemoryResolution> {
    if (!this.resolver) {
      return { action: "new", targetAtomId: null, canonicalContent: input.content, tags: input.tags, confidence: 0.9, rationale: "default" };
    }
    return this.resolver(input);
  }
}
