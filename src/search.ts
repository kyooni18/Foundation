import type { QueryResultRow } from "pg";
import type { AIProvider, EmbeddingResult } from "./ai.js";
import type { Database } from "./db.js";
import {
  atomScopePredicate,
  atomTemporalPredicate,
  scopeFilterParams,
  temporalQuery,
  temporalState,
  type ScopeContext,
  type ScopeFilterParams,
  type TemporalQuery
} from "./scope.js";
import type { SearchItem, SearchSignals } from "./types.js";
import { contentHash, normalizeTags, normalizeText, record, vectorArray } from "./utils.js";

const METADATA_KEYS = new Map<string, string>([
  ["project", "project"],
  ["repo", "repo"],
  ["namespace", "namespace"],
  ["kind", "kind"],
  ["source", "source"],
  ["path", "path"],
  ["branch", "branch"],
  ["commit", "commit"],
  ["memorytype", "memoryType"],
  ["scope", "scope"],
  ["subject", "subject"]
]);

const STOPWORDS = new Set((
  "a an and are as at be but by for from has have how i in is it its of on or that the this to was were what when where which who why will with you your " +
  "do does did not no yes can could should would about into than then there their they them we our my me he she his her"
).split(" "));

export interface SearchQueryAnalysis {
  normalized: string;
  text: string;
  symbolText: string;
  tags: string[];
  metadataFilters: Record<string, string>;
  /** Meaningful lowercase query terms (stopwords removed). */
  tokens: string[];
  /** Identifier-like terms (snake_case, camelCase, paths, versions) matched with token boundaries. */
  identifiers: string[];
}

function identifierParts(value: string): string[] {
  const expanded = value
    .replace(/([\p{Ll}\d])([\p{Lu}])/gu, "$1 $2")
    .replace(/[_.:/\\@#-]+/g, " ");
  return normalizeText(expanded).split(" ").filter(part => part.length > 1);
}

function looksLikeIdentifier(token: string): boolean {
  if (token.length < 3) return false;
  return /[\p{L}\p{N}][_./@#\\-][\p{L}\p{N}]/u.test(token) ||
    /\p{Ll}\p{Lu}/u.test(token) ||
    (/\p{N}/u.test(token) && /\p{L}/u.test(token));
}

/** Lowercase variants under which an identifier may appear in stored text. */
export function identifierVariants(token: string): string[] {
  const parts = identifierParts(token).map(part => part.toLowerCase());
  const variants = new Set<string>([token.toLowerCase()]);
  if (parts.length > 1) {
    variants.add(parts.join("_"));
    variants.add(parts.join("-"));
    variants.add(parts.join(" "));
    variants.add(parts.join(""));
  }
  return [...variants];
}

export function analyzeSearchQuery(query: string): SearchQueryAnalysis {
  const normalized = normalizeText(query);
  const metadataFilters: Record<string, string> = {};

  const stripped = normalized.replace(
    /(^|\s)([A-Za-z][A-Za-z0-9_-]*):(?:"([^"]+)"|'([^']+)'|([^\s]+))/g,
    (match, prefix: string, rawKey: string, doubleQuoted: string | undefined, singleQuoted: string | undefined, bare: string | undefined) => {
      const key = METADATA_KEYS.get(rawKey.toLowerCase());
      if (!key) return match;
      const value = normalizeText(doubleQuoted ?? singleQuoted ?? bare ?? "");
      if (value) metadataFilters[key] = value;
      return prefix;
    }
  );

  const freeText = normalizeText(stripped);
  const fallback = Object.values(metadataFilters).join(" ");
  const text = freeText || normalizeText(fallback) || normalized;
  const rawTokens = text.match(/[\p{L}\p{N}][\p{L}\p{N}._/@#:+-]*/gu) ?? [];

  const expandedTokens: string[] = [];
  for (const token of rawTokens) {
    expandedTokens.push(token);
    const parts = identifierParts(token);
    if (parts.length > 1) {
      expandedTokens.push(parts.join(" "));
      expandedTokens.push(...parts);
    }
  }

  const filterValues = Object.values(metadataFilters);
  const tags = normalizeTags([...rawTokens, ...expandedTokens.flatMap(identifierParts), ...filterValues]);
  const symbolText = normalizeText([...new Set(expandedTokens)].join(" ")) || text;

  const tokens = [...new Set(
    rawTokens
      .map(token => token.toLowerCase().replace(/[._/@#:+-]+$/g, ""))
      .filter(token => token.length >= 2 || /[^\u0000-\u007f]/.test(token))
      .filter(token => !STOPWORDS.has(token))
  )];
  const identifiers = [...new Set(rawTokens.filter(looksLikeIdentifier))];

  return { normalized, text, symbolText, tags, metadataFilters, tokens: tokens.length ? tokens : [text.toLowerCase()], identifiers };
}

// ---------------------------------------------------------------------------------------------
// Scoring and fusion
// ---------------------------------------------------------------------------------------------

export interface SearchSignalCandidate {
  key: string;
  exact: number;
  lexical: number;
  symbol: number;
  semantic: number;
  tag: number;
  metadata: number;
  fuzzy?: number;
  metadataObject?: Record<string, unknown> | null;
  /** When the *fact* became valid (never a database write time). */
  changedAt?: Date | string | null;
}

export interface FusedSearchScore {
  key: string;
  score: number;
  matchedBy: string[];
  eligible: boolean;
  dropReason: string | null;
}

export interface FuseOptions {
  rrfK?: number;
  now?: number;
  /** Number of meaningful query terms; drives how much lexical coverage counts as relevant. */
  queryTokenCount?: number;
  semanticMin?: number;
  /** Entity/identifier lookups need an identity anchor, not embedding proximity. */
  requireAnchor?: boolean;
  /** Calibrated separately from semantic corroboration. */
  semanticOnlyMin?: number;
}

function boundedUnit(value: unknown): number {
  const number = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(number)) return 0;
  return Math.max(0, Math.min(1, number));
}

function importanceScore(value: unknown): number {
  if (typeof value === "number") return boundedUnit(value);
  if (typeof value !== "string") return 0;
  switch (value.trim().toLowerCase()) {
    case "critical":
    case "highest":
      return 1;
    case "high":
      return 0.8;
    case "medium":
    case "normal":
      return 0.5;
    case "low":
      return 0.2;
    default:
      return boundedUnit(value);
  }
}

function freshnessScore(value: Date | string | null | undefined, now: number): number {
  if (!value) return 0;
  const timestamp = value instanceof Date ? value.getTime() : Date.parse(value);
  if (!Number.isFinite(timestamp)) return 0;
  const ageDays = Math.max(0, now - timestamp) / 86_400_000;
  return Math.exp(-ageDays / 730);
}

export function lexicalThreshold(tokenCount: number): number {
  if (tokenCount <= 1) return 1;
  if (tokenCount <= 5) return 0.5;
  return 0.34;
}

const SEMANTIC_MIN = 0.6;
const SEMANTIC_BAND = 0.15;
const FUZZY_MIN = 0.72;

/**
 * Fuse per-signal evidence into one relevance score in [0,1].
 *
 * A candidate is only *eligible* when at least one strong signal clears its own relevance threshold;
 * tag-free metadata overlap and sub-threshold similarity never qualify a result on their own. This is
 * what lets a query return nothing instead of padding the limit with unrelated memories.
 */
export function fuseSearchCandidates(
  candidates: SearchSignalCandidate[],
  options: FuseOptions = {}
): FusedSearchScore[] {
  const rrfK = Math.max(1, options.rrfK ?? 60);
  const now = options.now ?? Date.now();
  const lexMin = lexicalThreshold(options.queryTokenCount ?? 3);
  const semMin = options.semanticMin ?? SEMANTIC_MIN;

  const unique = new Map<string, SearchSignalCandidate>();
  for (const candidate of candidates) {
    const previous = unique.get(candidate.key);
    if (!previous) {
      unique.set(candidate.key, candidate);
      continue;
    }
    unique.set(candidate.key, {
      ...previous,
      exact: Math.max(previous.exact, candidate.exact),
      lexical: Math.max(previous.lexical, candidate.lexical),
      symbol: Math.max(previous.symbol, candidate.symbol),
      fuzzy: Math.max(previous.fuzzy ?? 0, candidate.fuzzy ?? 0),
      semantic: Math.max(previous.semantic, candidate.semantic),
      tag: Math.max(previous.tag, candidate.tag),
      metadata: Math.max(previous.metadata, candidate.metadata),
      metadataObject: previous.metadataObject ?? candidate.metadataObject,
      changedAt: previous.changedAt ?? candidate.changedAt
    });
  }

  const rows = [...unique.values()];
  const bestSemantic = rows.reduce((best, row) => Math.max(best, row.semantic), 0);
  const semanticFloor = Math.max(semMin, bestSemantic - SEMANTIC_BAND);
  const anchored = (row: SearchSignalCandidate): boolean =>
    row.exact > 0 || row.symbol >= 0.5 || row.lexical >= lexMin || row.tag >= lexMin || row.metadata >= lexMin;
  const semanticPass = (row: SearchSignalCandidate): boolean =>
    row.semantic >= semanticFloor && (anchored(row) ||
      (!options.requireAnchor && row.semantic >= (options.semanticOnlyMin ?? 0.68)));

  type Signal = { field: keyof SearchSignals; matchedBy: string; weight: number; strong: boolean; pass: (row: SearchSignalCandidate) => boolean };
  const signals: Signal[] = [
    { field: "exact", matchedBy: "exact", weight: 2.4, strong: true, pass: row => row.exact > 0 },
    { field: "symbol", matchedBy: "symbol", weight: 1.5, strong: true, pass: row => row.symbol >= 0.5 },
    { field: "lexical", matchedBy: "text", weight: 1.25, strong: true, pass: row => row.lexical >= lexMin },
    { field: "fuzzy", matchedBy: "fuzzy", weight: 0.8, strong: true, pass: row => (row.fuzzy ?? 0) >= FUZZY_MIN && (!options.requireAnchor || anchored(row)) },
    { field: "semantic", matchedBy: "vector", weight: 1, strong: true, pass: semanticPass },
    { field: "tag", matchedBy: "tag", weight: 1.1, strong: true, pass: row => row.tag >= lexMin },
    { field: "metadata", matchedBy: "metadata", weight: 0.5, strong: false, pass: row => row.metadata >= 0.5 }
  ];

  const value = (row: SearchSignalCandidate, field: keyof SearchSignals): number =>
    field === "fuzzy" ? row.fuzzy ?? 0 : row[field];

  const raw = new Map<string, number>();
  const matched = new Map<string, Set<string>>();
  const strongCount = new Map<string, number>();
  const maxRrf = signals.reduce((sum, signal) => sum + signal.weight / (rrfK + 1), 0);

  for (const signal of signals) {
    const ranked = rows
      .filter(row => signal.pass(row))
      .sort((a, b) => value(b, signal.field) - value(a, signal.field) || a.key.localeCompare(b.key));
    let rank = 0;
    ranked.forEach((row, index) => {
      // Equal evidence gets equal RRF credit; UUID order is only a deterministic final tie-break.
      if (index === 0 || value(row, signal.field) !== value(ranked[index - 1]!, signal.field)) rank = index;
      raw.set(row.key, (raw.get(row.key) ?? 0) + signal.weight / (rrfK + rank + 1));
      const set = matched.get(row.key) ?? new Set<string>();
      set.add(signal.matchedBy);
      matched.set(row.key, set);
      if (signal.strong) strongCount.set(row.key, (strongCount.get(row.key) ?? 0) + 1);
    });
  }

  return rows
    .map(row => {
      const strong = strongCount.get(row.key) ?? 0;
      const metadata = row.metadataObject ?? {};
      const quality = importanceScore(metadata.importance) * 0.6 + boundedUnit(metadata.confidence) * 0.4;
      const freshness = freshnessScore(row.changedAt, now);
      const semanticDirect = semanticPass(row) ? boundedUnit((row.semantic - 0.5) / 0.5) : 0;
      const direct = Math.max(
        row.exact > 0 ? 1 : 0,
        row.symbol >= 0.5 ? boundedUnit(row.symbol) * 0.95 : 0,
        row.lexical >= lexMin ? boundedUnit(row.lexical) * 0.9 : 0,
        (row.fuzzy ?? 0) >= FUZZY_MIN && (!options.requireAnchor || anchored(row)) ? boundedUnit(row.fuzzy) * 0.8 : 0,
        row.tag >= lexMin ? boundedUnit(row.tag) * 0.75 : 0,
        semanticDirect * 0.85,
        row.metadata >= 0.5 ? boundedUnit(row.metadata) * 0.4 : 0
      );
      const fused = maxRrf > 0 ? (raw.get(row.key) ?? 0) / maxRrf : 0;
      const corroboration = Math.min(0.12, Math.max(0, strong - 1) * 0.04);
      const score = strong > 0
        ? Math.min(1, direct * 0.62 + fused * 0.2 + corroboration + quality * 0.03 + freshness * 0.03)
        : 0;
      return {
        key: row.key,
        score,
        matchedBy: [...(matched.get(row.key) ?? new Set<string>())],
        eligible: strong > 0,
        dropReason: strong > 0 ? null : options.requireAnchor ? "entity lookup requires a token, identifier, or tag anchor" : "no signal cleared its relevance threshold (semantic-only evidence too weak)"
      };
    })
    .sort((a, b) => b.score - a.score || a.key.localeCompare(b.key));
}

// ---------------------------------------------------------------------------------------------
// Candidate generation. Every generator is a plain indexed predicate (GIN/HNSW/btree) with a LIMIT.
// Exact scores are computed afterwards by a primary-key rescoring query over the union of candidates.
// ---------------------------------------------------------------------------------------------

export interface SearchOptions extends ScopeContext {
  limit?: number;
  asOf?: string | Date | null;
  history?: boolean;
  minScore?: number;
  /** Collect per-generator timings and per-candidate signals (CLI diagnostics). */
  trace?: SearchTrace;
}

export interface SearchTrace {
  plans?: boolean;
  analysis?: SearchQueryAnalysis;
  scope?: { all: boolean; project: string | null; conversation: string | null };
  temporal?: { mode: string; at: string };
  embedding?: { provider: string; model: string } | null;
  embeddingError?: string | null;
  generators: Array<{ target: "atoms" | "files"; name: string; rows: number; ms: number; plan?: string[] }>;
  candidates: Array<{ key: string; kind: "atom" | "file"; signals: SearchSignals; score: number; eligible: boolean; dropReason: string | null; matchedBy: string[]; final: boolean }>;
  rescoreMs?: number;
  totalMs?: number;
}

type Row = QueryResultRow;

function tsQueryFromTokens(tokens: string[]): string | null {
  const safe = [...new Set(tokens
    .flatMap(token => token.split(/[^\p{L}\p{N}]+/u))
    .map(token => token.trim())
    .filter(token => token.length >= 2 && !STOPWORDS.has(token)))]
    .sort((a, b) => b.length - a.length)
    .slice(0, 12);
  if (safe.length === 0) return null;
  return safe.map(token => `'${token}'`).join(" | ");
}

function escapeLike(value: string): string {
  return value.replace(/[\\%_]/g, match => "\\" + match);
}

function symbolPatterns(identifiers: string[]): string[] {
  return [...new Set(identifiers.flatMap(identifierVariants))]
    .filter(variant => variant.length >= 3)
    .map(variant => `%${escapeLike(variant)}%`)
    .slice(0, 24);
}

/** Collects bind parameters while a statement is assembled, so unused predicates add neither SQL nor params. */
class Params {
  readonly values: unknown[] = [];
  add = (value: unknown): string => {
    this.values.push(value);
    return `$${this.values.length}`;
  };
}

interface BaseContext {
  scope: ScopeFilterParams;
  temporal: TemporalQuery;
  filters: Record<string, string>;
}

function metadataFilterPredicate(alias: string, filters: Record<string, string>, add: (v: unknown) => string): string {
  if (Object.keys(filters).length === 0) return "TRUE";
  return `NOT EXISTS (SELECT 1 FROM jsonb_each_text(${add(JSON.stringify(filters))}::jsonb) AS filter(key,value) WHERE lower(coalesce(${alias}.metadata ->> filter.key,'')) <> lower(filter.value))`;
}

function atomBase(alias: string, ctx: BaseContext, p: Params): string {
  return [
    atomScopePredicate(alias, ctx.scope, p.add),
    atomTemporalPredicate(alias, ctx.temporal, p.add),
    metadataFilterPredicate(alias, ctx.filters, p.add)
  ].filter(part => part !== "TRUE").join("\n  AND ") || "TRUE";
}

// Files carry scope through metadata, or through the Atoms they are docked to. A File whose only
// Atoms belong to another project is never shown; a File with no Atoms and no scope metadata stays shared.
function fileBase(alias: string, ctx: BaseContext, p: Params): string {
  const parts: string[] = [];
  if (!ctx.scope.all) {
    const project = p.add(ctx.scope.project);
    const conversation = p.add(ctx.scope.conversation);
    parts.push(`(${alias}.metadata->>'project' IS NULL OR lower(${alias}.metadata->>'project') = ${project}::text)`);
    parts.push(`(${alias}.metadata->>'conversationId' IS NULL OR lower(${alias}.metadata->>'conversationId') = ${conversation}::text)`);
    parts.push(`(NOT EXISTS (SELECT 1 FROM atom_file_docks d WHERE d.file_id = ${alias}.id)
    OR EXISTS (SELECT 1 FROM atom_file_docks d JOIN atoms da ON da.id = d.atom_id WHERE d.file_id = ${alias}.id AND ${atomScopePredicate("da", ctx.scope, p.add)}))`);
  }
  const filter = metadataFilterPredicate(alias, ctx.filters, p.add);
  if (filter !== "TRUE") parts.push(filter);
  return parts.join("\n  AND ") || "TRUE";
}

const FILE_FTS = "to_tsvector('simple', coalesce(f.content,'') || ' ' || coalesce(f.interpreted,'') || ' ' || coalesce(f.subject,''))";

interface Generator {
  target: "atoms" | "files";
  name: string;
  sql: string;
  params: unknown[];
  vector?: boolean;
}

function buildGenerators(
  analysis: SearchQueryAnalysis,
  ctx: BaseContext,
  hash: string,
  embedding: EmbeddingResult | null,
  candidateLimit: number
): Generator[] {
  const lowered = analysis.text.toLowerCase();
  const ts = tsQueryFromTokens(analysis.tokens);
  const patterns = symbolPatterns(analysis.identifiers);
  const out: Generator[] = [];
  const add = (target: "atoms" | "files", name: string, build: (p: Params, base: string) => string, vector = false) => {
    const p = new Params();
    const base = target === "atoms" ? atomBase("a", ctx, p) : fileBase("f", ctx, p);
    out.push({ target, name, sql: build(p, base), params: p.values, vector });
  };
  const atom = (name: string, build: (p: Params, base: string) => string, vector = false) => add("atoms", name, build, vector);
  const file = (name: string, build: (p: Params, base: string) => string, vector = false) => add("files", name, build, vector);

  atom("exact", (p, base) => `SELECT a.id FROM atoms a WHERE (a.content_hash=${p.add(hash)} OR a.normalized_content=${p.add(lowered)}) AND ${base} LIMIT 20`);
  if (ts) {
    atom("fts", (p, base) => {
      const q = p.add(ts);
      return `SELECT a.id FROM atoms a WHERE to_tsvector('simple', a.content) @@ to_tsquery('simple',${q}) AND ${base}
      ORDER BY ts_rank(to_tsvector('simple', a.content), to_tsquery('simple',${q})) DESC, a.id LIMIT ${p.add(candidateLimit)}`;
    });
    file("fts", (p, base) => {
      const q = p.add(ts);
      return `SELECT f.id FROM files f WHERE ${FILE_FTS} @@ to_tsquery('simple',${q}) AND ${base}
      ORDER BY ts_rank(${FILE_FTS}, to_tsquery('simple',${q})) DESC, f.id LIMIT ${p.add(candidateLimit)}`;
    });
  }
  atom("trigram", (p, base) => {
    const q = p.add(analysis.text);
    return `SELECT a.id FROM atoms a WHERE ${q}::text <% a.content AND ${base}
    ORDER BY word_similarity(${q}::text, a.content) DESC, a.id LIMIT ${p.add(candidateLimit)}`;
  });
  if (patterns.length) {
    atom("symbol", (p, base) => `SELECT a.id FROM atoms a WHERE a.content ILIKE ANY(${p.add(patterns)}::text[]) AND ${base} ORDER BY a.id LIMIT ${p.add(candidateLimit)}`);
    file("symbol", (p, base) => {
      const q = p.add(patterns);
      return `SELECT f.id FROM files f WHERE (f.content ILIKE ANY(${q}::text[]) OR f.subject ILIKE ANY(${q}::text[]) OR f.name ILIKE ANY(${q}::text[])) AND ${base} LIMIT ${p.add(candidateLimit)}`;
    });
  }
  atom("tags", (p, base) => {
    const q = p.add(analysis.tags);
    return `SELECT a.id FROM atoms a WHERE a.tags && ${q}::text[] AND ${base}
    ORDER BY (SELECT count(*) FROM unnest(a.tags) t WHERE t = ANY(${q}::text[])) DESC, a.id LIMIT ${p.add(candidateLimit)}`;
  });
  file("tags", (p, base) => `SELECT f.id FROM files f WHERE f.tags && ${p.add(analysis.tags)}::text[] AND ${base} LIMIT ${p.add(candidateLimit)}`);
  file("trigram", (p, base) => {
    const q = p.add(analysis.text);
    return `SELECT f.id FROM files f WHERE (${q}::text <% f.content OR ${q}::text <% f.subject OR ${q}::text <% f.name) AND ${base}
    ORDER BY GREATEST(word_similarity(${q}::text, f.subject), word_similarity(${q}::text, f.name)) DESC NULLS LAST LIMIT ${p.add(candidateLimit)}`;
  });

  if (embedding) {
    const vec = vectorArray(embedding.vector);
    const provenance = (p: Params, prefix: string, alias: string) =>
      `${alias}.${prefix} IS NOT NULL AND ${alias}.${prefix}_provider=${p.add(embedding.provider)} AND ${alias}.${prefix}_model=${p.add(embedding.model)} AND ${alias}.${prefix}_dimensions=${p.add(embedding.dimensions)}`;
    atom("vector", (p, base) =>
      `SELECT a.id FROM atoms a WHERE ${provenance(p, "vector", "a")} AND ${base}
      ORDER BY a.vector <=> ${p.add(vec)}::real[]::halfvec LIMIT ${p.add(candidateLimit)}`, true);
    file("vector_content", (p, base) =>
      `SELECT f.id FROM files f WHERE ${provenance(p, "vector_content", "f")} AND ${base}
      ORDER BY f.vector_content <=> ${p.add(vec)}::real[]::halfvec LIMIT ${p.add(candidateLimit)}`, true);
    file("vector_subject", (p, base) =>
      `SELECT f.id FROM files f WHERE ${provenance(p, "vector_subject", "f")} AND ${base}
      ORDER BY f.vector_subject <=> ${p.add(vec)}::real[]::halfvec LIMIT ${p.add(candidateLimit)}`, true);
  }
  return out;
}

async function runGenerator(db: Database, generator: Generator, trace: SearchTrace | undefined): Promise<string[]> {
  const started = performance.now();
  let rows: Row[];
  let plan: string[] | undefined;

  if (generator.vector) {
    rows = await db.transaction(async client => {
      // HNSW is approximate and applies scope/status filters after the graph walk; iterative scan
      // keeps walking until the limit is satisfied so filtered queries do not silently under-return.
      await client.query("SAVEPOINT hnsw_tuning");
      try {
        await client.query("SET LOCAL hnsw.iterative_scan = 'relaxed_order'");
        await client.query("SET LOCAL hnsw.ef_search = 100");
      } catch {
        await client.query("ROLLBACK TO SAVEPOINT hnsw_tuning");
      }
      if (trace?.plans) {
        plan = (await client.query("EXPLAIN " + generator.sql, generator.params)).rows.map(r => String(r["QUERY PLAN"]));
      }
      return (await client.query(generator.sql, generator.params)).rows;
    });
  } else {
    if (trace?.plans) {
      plan = (await db.query("EXPLAIN " + generator.sql, generator.params)).rows.map(r => String(r["QUERY PLAN"]));
    }
    rows = (await db.query(generator.sql, generator.params)).rows;
  }

  trace?.generators.push({ target: generator.target, name: generator.name, rows: rows.length, ms: performance.now() - started, ...(plan ? { plan } : {}) });
  return rows.map(row => String(row.id));
}

// The query vector travels as real[]: a 3072-float *text* halfvec parameter costs ~0.75ms per evaluated row on
// seq-scan plans (measured 46ms vs 3ms for 60 rows); the array cast is ~15x cheaper and still index-eligible.
const ATOM_RESCORE_SQL = `
WITH q AS (SELECT $5::real[]::halfvec AS v)
SELECT a.id, a.content, a.tags, a.metadata, a.scope_kind, a.scope_key, a.status, a.valid_from, a.expires_at, a.created_at,
  CASE WHEN a.content_hash=$2 OR a.normalized_content=$3 THEN 1 ELSE 0 END AS exact_score,
  word_similarity($4::text, a.content) AS fuzzy_score,
  CASE WHEN q.v IS NULL OR a.vector IS NULL OR a.vector_provider IS DISTINCT FROM $6
         OR a.vector_model IS DISTINCT FROM $7 OR a.vector_dimensions IS DISTINCT FROM $8
       THEN 0 ELSE 1 - (a.vector <=> q.v) END AS semantic_score,
  COALESCE((SELECT jsonb_agg(jsonb_build_object('id', f.id, 'name', f.name) ORDER BY f.created_at)
            FROM atom_file_docks d JOIN files f ON f.id = d.file_id WHERE d.atom_id = a.id), '[]'::jsonb) AS sources
FROM atoms a CROSS JOIN q WHERE a.id = ANY($1::uuid[])`;

const FILE_RESCORE_SQL = `
WITH q AS (SELECT $4::real[]::halfvec AS v)
SELECT f.id, f.name, left(coalesce(f.content,''), 50000) AS snippet, f.subject, f.tags, f.metadata, f.modified_at,
  CASE WHEN lower(coalesce(f.name,''))=$2 OR lower(coalesce(f.subject,''))=$2 OR lower(left(coalesce(f.content,''), 5000))=$2 THEN 1 ELSE 0 END AS exact_score,
  GREATEST(word_similarity($3::text, coalesce(f.name,'')), word_similarity($3::text, coalesce(f.subject,'')),
           word_similarity($3::text, left(coalesce(f.content,''), 20000))) AS fuzzy_score,
  CASE WHEN q.v IS NULL THEN 0 ELSE GREATEST(
    CASE WHEN f.vector_content IS NULL OR f.vector_content_provider IS DISTINCT FROM $5 OR f.vector_content_model IS DISTINCT FROM $6
              OR f.vector_content_dimensions IS DISTINCT FROM $7 THEN 0 ELSE 1 - (f.vector_content <=> q.v) END,
    CASE WHEN f.vector_subject IS NULL OR f.vector_subject_provider IS DISTINCT FROM $5 OR f.vector_subject_model IS DISTINCT FROM $6
              OR f.vector_subject_dimensions IS DISTINCT FROM $7 THEN 0 ELSE 1 - (f.vector_subject <=> q.v) END
  ) END AS semantic_score
FROM files f CROSS JOIN q WHERE f.id = ANY($1::uuid[])`;

function relatedAtomsQuery(seedIds: string[], ctx: BaseContext): { sql: string; params: unknown[] } {
  const p = new Params();
  const seeds = p.add(seedIds);
  const base = atomBase("a", ctx, p);
  return {
    params: p.values,
    sql: `SELECT edge.relation, edge.seed_id, edge.related_id
FROM (
  SELECT r.relation,
    CASE WHEN r.from_atom_id = ANY(${seeds}::uuid[]) THEN r.from_atom_id ELSE r.to_atom_id END AS seed_id,
    CASE WHEN r.from_atom_id = ANY(${seeds}::uuid[]) THEN r.to_atom_id ELSE r.from_atom_id END AS related_id
  FROM atom_relations r
  WHERE (r.from_atom_id = ANY(${seeds}::uuid[]) OR r.to_atom_id = ANY(${seeds}::uuid[]))
    AND r.relation IN ('refines','supersedes')
) edge
JOIN atoms a ON a.id = edge.related_id
WHERE ${base}`
  };
}

const CONFLICTS_SQL = `
SELECT r.from_atom_id, r.to_atom_id
FROM atom_relations r
JOIN atoms x ON x.id = r.from_atom_id AND x.status = 'active'
JOIN atoms y ON y.id = r.to_atom_id AND y.status = 'active'
WHERE r.relation = 'conflicts_with' AND (r.from_atom_id = ANY($1::uuid[]) OR r.to_atom_id = ANY($1::uuid[]))`;

const FILE_CONTENT_SQL = "SELECT id, content FROM files WHERE id = ANY($1::uuid[])";

function numberScore(row: Row, key: string): number {
  return Math.max(0, Number(row[key] ?? 0));
}

/** Latin/digit terms require word boundaries; Korean may carry grammatical particles. */
function containsTerm(text: string, term: string): boolean {
  const escaped = term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const suffix = /[가-힣]$/.test(term)
    ? "(?:은|는|이|가|을|를|의|에|에서|으로|로|와|과|도|만|입니다|이다)?"
    : "";
  return new RegExp(`(?<![\\p{L}\\p{N}])${escaped}${suffix}(?![\\p{L}\\p{N}])`, "u").test(text);
}

export function requiresIdentityAnchor(query: SearchQueryAnalysis): boolean {
  // A single Latin name/acronym (case insensitive), or a structured identifier, denotes
  // an identity. Natural-language sentences remain eligible for cross-language semantics.
  return (query.tokens.length <= 2 && query.identifiers.length > 0) ||
    (query.tokens.length === 1 && /^[a-z0-9._/@#+:-]+$/i.test(query.tokens[0]!));
}

function lowerText(value: string): string {
  return identifierParts(value).join(" ").toLowerCase() + " " + value.toLowerCase();
}

/**
 * Weight each query term by how discriminating it is inside the candidate pool (BM25-style idf). A term
 * that appears in nearly every candidate (a project name inside a project scope, "service") must not
 * make a weak partial match look relevant. Small pools fall back to uniform weights.
 */
export function tokenWeights(tokens: string[], texts: string[]): Map<string, number> {
  const weights = new Map<string, number>();
  if (texts.length < 8) {
    for (const token of tokens) weights.set(token, 1);
    return weights;
  }
  const haystacks = texts.map(lowerText);
  for (const token of tokens) {
    const df = haystacks.filter(text => containsTerm(text, token)).length;
    weights.set(token, Math.max(0.1, Math.log(1 + (texts.length - df + 0.5) / (df + 0.5))));
  }
  return weights;
}

/** Weighted fraction of query terms present in the text (identifier-aware, word-boundary safe with Korean particles). */
export function lexicalCoverage(tokens: string[], text: string, weights?: Map<string, number>): number {
  if (tokens.length === 0) return 0;
  const haystack = lowerText(text);
  let hit = 0;
  let total = 0;
  for (const token of tokens) {
    const weight = weights?.get(token) ?? 1;
    total += weight;
    if (containsTerm(haystack, token)) {
      hit += weight;
      continue;
    }
    if (identifierVariants(token).some(variant => containsTerm(text.toLowerCase(), variant))) hit += weight;
  }
  return total > 0 ? hit / total : 0;
}

function symbolCoverage(identifiers: string[], text: string): number {
  if (identifiers.length === 0) return 0;
  const haystack = text.toLowerCase();
  const hit = identifiers.filter(id => identifierVariants(id).some(variant => containsTerm(haystack, variant))).length;
  return hit / identifiers.length;
}

function tagCoverage(tokens: string[], tags: string[], weights?: Map<string, number>): number {
  if (tokens.length === 0 || tags.length === 0) return 0;
  const set = new Set(tags.map(tag => tag.toLowerCase()));
  let hit = 0;
  let total = 0;
  for (const token of tokens) {
    const weight = weights?.get(token) ?? 1;
    total += weight;
    if (set.has(token)) hit += weight;
  }
  return total > 0 ? hit / total : 0;
}

const METADATA_TEXT_FIELDS = ["subject", "project", "repo", "kind", "path", "source", "memoryType"];

function metadataCoverage(tokens: string[], metadata: Record<string, unknown>, weights?: Map<string, number>): number {
  const text = METADATA_TEXT_FIELDS.map(key => metadata[key]).filter((v): v is string => typeof v === "string").join(" ");
  return text ? lexicalCoverage(tokens, text, weights) : 0;
}

function contentTokens(text: string): Set<string> {
  return new Set((text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? []).filter(token => token.length > 1));
}

function jaccard(a: Set<string>, b: Set<string>): number {
  if (a.size === 0 || b.size === 0) return 0;
  let shared = 0;
  for (const token of a) if (b.has(token)) shared += 1;
  return shared / (a.size + b.size - shared);
}

const GLOBAL_FALLBACK_FACTOR = 0.92;
const HISTORICAL_FACTOR = 0.7;
const DEFAULT_MIN_SCORE = 0.22;
const DECISIVE_TAIL_RATIO = 0.6;

interface Scored {
  item: SearchItem;
  tokens: Set<string>;
  sourceIds: string[];
}

export async function searchFoundation(
  db: Database,
  ai: AIProvider,
  query: string,
  limitOrOptions: number | SearchOptions = 10
): Promise<SearchItem[]> {
  const options: SearchOptions = typeof limitOrOptions === "number" ? { limit: limitOrOptions } : limitOrOptions;
  const trace = options.trace;
  const started = performance.now();
  const normalizedQuery = normalizeText(query);
  if (!normalizedQuery) return [];

  const limit = Math.max(1, Math.min(options.limit ?? 10, 100));
  const analysis = analyzeSearchQuery(normalizedQuery);

  // An explicit `project:` query filter selects the project scope (with global fallback) rather than
  // requiring a literal metadata match, so it composes with the Atom scope column.
  const filters = { ...analysis.metadataFilters };
  const project = options.project ?? filters.project ?? null;
  delete filters.project;
  const scopeParams = scopeFilterParams({ project, conversationId: options.conversationId });
  const temporal = temporalQuery({ asOf: options.asOf, history: options.history });
  const minScore = options.minScore ?? DEFAULT_MIN_SCORE;

  let queryEmbedding: EmbeddingResult | null = null;
  try {
    queryEmbedding = await ai.embed(analysis.text, "query");
  } catch (error) {
    if (trace) trace.embeddingError = error instanceof Error ? error.message : String(error);
  }

  if (trace) {
    trace.analysis = analysis;
    trace.scope = scopeParams;
    trace.temporal = { mode: temporal.mode, at: temporal.at.toISOString() };
    trace.embedding = queryEmbedding ? { provider: queryEmbedding.provider, model: queryEmbedding.model } : null;
  }

  const baseContext: BaseContext = { scope: scopeParams, temporal, filters };
  const candidateLimit = Math.max(40, Math.min(limit * 8, 200));
  const generators = buildGenerators(analysis, baseContext, contentHash(analysis.text), queryEmbedding, candidateLimit);

  const idSets = await Promise.all(generators.map(generator => runGenerator(db, generator, trace)));
  const atomIds = new Set<string>();
  const fileIds = new Set<string>();
  generators.forEach((generator, index) => {
    for (const id of idSets[index]!) (generator.target === "atoms" ? atomIds : fileIds).add(id);
  });

  const rescoreStarted = performance.now();
  const lowered = analysis.text.toLowerCase();
  const vector = queryEmbedding ? vectorArray(queryEmbedding.vector) : null;
  const emb = [queryEmbedding?.provider ?? null, queryEmbedding?.model ?? null, queryEmbedding?.dimensions ?? null];
  const [atomRows, fileRows] = await Promise.all([
    atomIds.size
      ? db.query(ATOM_RESCORE_SQL, [[...atomIds], contentHash(analysis.text), lowered, analysis.text, vector, ...emb])
      : Promise.resolve({ rows: [] as Row[] }),
    fileIds.size
      ? db.query(FILE_RESCORE_SQL, [[...fileIds], lowered, analysis.text, vector, ...emb])
      : Promise.resolve({ rows: [] as Row[] })
  ]);
  if (trace) trace.rescoreMs = performance.now() - rescoreStarted;

  const weights = tokenWeights(analysis.tokens, [
    ...atomRows.rows.map(row => String(row.content)),
    ...fileRows.rows.map(row => [row.name, row.subject, row.snippet].filter(Boolean).join(" "))
  ]);
  const signalOf = (row: Row, text: string, tags: string[], metadata: Record<string, unknown>): SearchSignals => ({
    exact: numberScore(row, "exact_score"),
    lexical: lexicalCoverage(analysis.tokens, text, weights),
    symbol: symbolCoverage(analysis.identifiers, text),
    fuzzy: numberScore(row, "fuzzy_score"),
    semantic: numberScore(row, "semantic_score"),
    tag: tagCoverage(analysis.tokens, tags, weights),
    metadata: metadataCoverage(analysis.tokens, metadata, weights)
  });

  const kinds = new Map<string, "atom" | "file">();
  const signalMap = new Map<string, SearchSignals>();
  const candidates: SearchSignalCandidate[] = [];
  const push = (key: string, kind: "atom" | "file", signals: SearchSignals, metadata: Record<string, unknown>, changedAt: Date | null) => {
    kinds.set(key, kind);
    signalMap.set(key, signals);
    candidates.push({ key, ...signals, metadataObject: metadata, changedAt });
  };

  const atomData = new Map<string, Row>();
  for (const row of atomRows.rows) {
    const tags = Array.isArray(row.tags) ? row.tags.map(String) : [];
    const metadata = record(row.metadata);
    const key = "atom:" + String(row.id);
    atomData.set(key, row);
    // Freshness is the time the fact became true, never the time the row was last touched.
    push(key, "atom", signalOf(row, String(row.content), tags, metadata), metadata, row.valid_from ? new Date(row.valid_from) : null);
  }
  const fileData = new Map<string, Row>();
  for (const row of fileRows.rows) {
    const tags = Array.isArray(row.tags) ? row.tags.map(String) : [];
    const metadata = record(row.metadata);
    const text = [row.name, row.subject, row.snippet].filter(Boolean).join(" ");
    const key = "file:" + String(row.id);
    fileData.set(key, row);
    push(key, "file", signalOf(row, text, tags, metadata), metadata, null);
  }

  const fused = fuseSearchCandidates(candidates, {
    queryTokenCount: analysis.tokens.length,
    requireAnchor: requiresIdentityAnchor(analysis)
  });
  const fusedByKey = new Map(fused.map(item => [item.key, item]));

  const scored = new Map<string, Scored>();
  const addAtom = (row: Row, score: number, matchedBy: string[], signals: SearchSignals) => {
    const atomId = String(row.id);
    const scope = { kind: row.scope_kind as "global" | "project" | "conversation", key: String(row.scope_key) };
    const validFrom = row.valid_from ? new Date(row.valid_from) : null;
    const expiresAt = row.expires_at ? new Date(row.expires_at) : null;
    const state = temporalState({ status: String(row.status), validFrom, expiresAt }, temporal.at);
    let adjusted = score;
    if (scopeParams.project && scope.kind === "global") adjusted *= GLOBAL_FALLBACK_FACTOR;
    if (state !== "current") adjusted *= HISTORICAL_FACTOR;
    const sources = (Array.isArray(row.sources) ? row.sources : [])
      .map(source => record(source))
      .filter(source => typeof source.id === "string")
      .map(source => ({ fileId: String(source.id), name: typeof source.name === "string" ? source.name : null }));
    scored.set("atom:" + atomId, {
      item: {
        kind: "atom",
        id: atomId,
        atomId,
        score: adjusted,
        rawText: String(row.content),
        matchedBy,
        scope,
        state,
        validFrom: validFrom?.toISOString() ?? null,
        expires: expiresAt?.toISOString() ?? null,
        ...(sources.length ? { sources } : {}),
        ...(trace ? { signals } : {})
      },
      tokens: contentTokens(String(row.content)),
      sourceIds: sources.map(source => source.fileId)
    });
  };

  for (const [key, row] of atomData) {
    const ranked = fusedByKey.get(key)!;
    if (!ranked.eligible || ranked.score < minScore) continue;
    addAtom(row, ranked.score, ranked.matchedBy, signalMap.get(key)!);
  }
  for (const [key, row] of fileData) {
    const ranked = fusedByKey.get(key)!;
    if (!ranked.eligible || ranked.score < minScore) continue;
    const id = String(row.id);
    scored.set(key, {
      item: { kind: "file", id, fileId: id, score: ranked.score, rawText: null, matchedBy: ranked.matchedBy, ...(trace ? { signals: signalMap.get(key)! } : {}) },
      tokens: contentTokens(String(row.snippet ?? "").slice(0, 4000)),
      sourceIds: []
    });
  }

  // Relation expansion: pull in directly refined/superseding Atoms of strong seeds, discounted.
  const seeds = [...scored.values()]
    .filter(entry => entry.item.kind === "atom")
    .sort((a, b) => b.item.score - a.item.score)
    .slice(0, Math.min(16, Math.max(6, limit)));
  if (seeds.length) {
    const seedById = new Map(seeds.map(entry => [entry.item.id, entry.item]));
    const relatedQuery = relatedAtomsQuery(seeds.map(entry => entry.item.id), baseContext);
    const edges = await db.query(relatedQuery.sql, relatedQuery.params);
    const relatedIds = [...new Set(edges.rows.map(row => String(row.related_id)).filter(id => !scored.has("atom:" + id)))];
    if (relatedIds.length) {
      const related = await db.query(ATOM_RESCORE_SQL, [relatedIds, contentHash(analysis.text), lowered, analysis.text, vector, ...emb]);
      const relatedById = new Map(related.rows.map(row => [String(row.id), row]));
      const perSeed = new Map<string, number>();
      for (const edge of edges.rows) {
        const row = relatedById.get(String(edge.related_id));
        const seed = seedById.get(String(edge.seed_id));
        if (!row || !seed || scored.has("atom:" + String(row.id))) continue;
        const count = perSeed.get(seed.id) ?? 0;
        if (count >= 2) continue;
        const relation = String(edge.relation);
        const score = seed.score * (relation === "refines" ? 0.78 : 0.72);
        if (score < minScore) continue;
        perSeed.set(seed.id, count + 1);
        const tags = Array.isArray(row.tags) ? row.tags.map(String) : [];
        addAtom(row, score, [...new Set([...seed.matchedBy, "relation:" + relation])], signalOf(row, String(row.content), tags, record(row.metadata)));
      }
    }
  }

  // Greedy diversification: suppress near-duplicate Atoms and soften runs from a single source File.
  const pool = [...scored.values()].sort((a, b) => b.item.score - a.item.score);
  const selected: Scored[] = [];
  const perSource = new Map<string, number>();
  const remaining = [...pool];
  while (remaining.length && selected.length < limit * 2) {
    let bestIndex = -1;
    let bestScore = -1;
    for (let i = 0; i < remaining.length; i++) {
      const entry = remaining[i]!;
      const crowd = Math.max(0, ...entry.sourceIds.map(id => perSource.get(id) ?? 0));
      const adjusted = entry.item.score * (1 - 0.12 * Math.max(0, crowd - 2));
      if (adjusted > bestScore) {
        bestScore = adjusted;
        bestIndex = i;
      }
    }
    const [picked] = remaining.splice(bestIndex, 1);
    if (!picked) break;
    const duplicate = picked.item.kind === "atom" && selected.some(other => other.item.kind === "atom" && jaccard(other.tokens, picked.tokens) >= 0.85);
    if (duplicate) continue;
    for (const id of picked.sourceIds) perSource.set(id, (perSource.get(id) ?? 0) + 1);
    selected.push(picked);
  }

  // A File that is only the provenance of a returned Atom is folded into that Atom's `sources`;
  // it stays as its own result only when it matched better than the Atom(s) derived from it.
  const bestAtomForFile = new Map<string, number>();
  for (const entry of selected) {
    if (entry.item.kind !== "atom") continue;
    for (const id of entry.sourceIds) bestAtomForFile.set(id, Math.max(bestAtomForFile.get(id) ?? 0, entry.item.score));
  }
  let final = selected
    .filter(entry => entry.item.kind === "atom" || (bestAtomForFile.get(entry.item.id) ?? -1) < entry.item.score)
    .map(entry => entry.item);

  // When a decisive answer exists (exact match, or every term of a multi-term query present), weaker
  // partial matches are dropped instead of trailing it. Broad queries keep their longer tail.
  const topKey = final[0] ? `${final[0].kind}:${final[0].id}` : null;
  const topSignals = topKey ? signalMap.get(topKey) : undefined;
  if (topSignals && final[0] && (topSignals.exact > 0 || (analysis.tokens.length >= 2 && topSignals.lexical >= 0.99))) {
    const floor = final[0].score * DECISIVE_TAIL_RATIO;
    final = final.filter(item => {
      const signals = signalMap.get(`${item.kind}:${item.id}`)!;
      // A paraphrase may outrank the lexical answer semantically. Do not let the lexical
      // tail heuristic discard it merely because stopwords/inflections differ.
      return item.score >= floor || (!requiresIdentityAnchor(analysis) &&
        signals.semantic >= 0.68 && signals.semantic > topSignals.semantic);
    });
  }
  final = final.slice(0, limit);

  const finalAtomIds = final.filter(item => item.kind === "atom").map(item => item.id);
  const finalFileIds = final.filter(item => item.kind === "file").map(item => item.id);
  const [conflicts, contents] = await Promise.all([
    finalAtomIds.length ? db.query(CONFLICTS_SQL, [finalAtomIds]) : Promise.resolve({ rows: [] as Row[] }),
    finalFileIds.length ? db.query(FILE_CONTENT_SQL, [finalFileIds]) : Promise.resolve({ rows: [] as Row[] })
  ]);
  const conflictMap = new Map<string, Set<string>>();
  for (const row of conflicts.rows) {
    const a = String(row.from_atom_id);
    const b = String(row.to_atom_id);
    (conflictMap.get(a) ?? conflictMap.set(a, new Set()).get(a)!).add(b);
    (conflictMap.get(b) ?? conflictMap.set(b, new Set()).get(b)!).add(a);
  }
  const contentMap = new Map(contents.rows.map(row => [String(row.id), row.content === null ? null : String(row.content)]));
  for (const item of final) {
    if (item.kind === "file") item.rawText = contentMap.get(item.id) ?? null;
    const conflicting = conflictMap.get(item.id);
    if (conflicting?.size) item.conflictsWith = [...conflicting];
  }

  if (trace) {
    const finalKeys = new Set(final.map(item => `${item.kind}:${item.id}`));
    for (const item of fused) {
      trace.candidates.push({
        key: item.key,
        kind: kinds.get(item.key)!,
        signals: signalMap.get(item.key)!,
        score: item.score,
        eligible: item.eligible,
        dropReason: item.dropReason ?? (item.score < minScore ? `score below minScore ${minScore}` : null),
        matchedBy: item.matchedBy,
        final: finalKeys.has(item.key)
      });
    }
    trace.totalMs = performance.now() - started;
  }

  return final;
}

export function newSearchTrace(plans = false): SearchTrace {
  return { plans, generators: [], candidates: [] };
}
