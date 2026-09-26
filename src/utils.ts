import { createHash } from "node:crypto";

export function normalizeText(value: string): string {
  return value.normalize("NFKC").replace(/\s+/g, " ").trim();
}

export function normalizeTags(tags: string[]): string[] {
  return [...new Set(tags.map(tag => normalizeText(tag).toLowerCase()).filter(Boolean))].slice(0, 100);
}

export function contentHash(value: string): string {
  return createHash("sha256").update(normalizeText(value)).digest("hex");
}

export function vectorLiteral(vector: number[] | null): string | null {
  if (!vector) return null;
  if (vector.length !== 3072) throw new Error(`Expected a 3072-dimensional vector, got ${vector.length}`);
  if (vector.some(value => !Number.isFinite(value))) throw new Error("Vector contains a non-finite value");
  return `[${vector.join(",")}]`;
}

export function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}
