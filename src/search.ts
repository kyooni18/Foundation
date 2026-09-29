import type { QueryResultRow } from "pg";
import type { AIProvider, EmbeddingResult } from "./ai.js";
import type { Database } from "./db.js";
import type { SearchItem } from "./types.js";
import { normalizeTags, normalizeText, record, vectorLiteral } from "./utils.js";

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

const FILE_SEARCH_SQL = [
  "SELECT *",
  "FROM (",
  "  SELECT id,content,interpreted,metadata,modified_at,",
  "    CASE WHEN",
  "      lower(coalesce(name,''))=lower($1) OR",
  "      lower(coalesce(content,''))=lower($1) OR",
  "      lower(coalesce(subject,''))=lower($1) OR",
  "      lower(coalesce(interpreted,''))=lower($1)",
  "    THEN 1 ELSE 0 END AS exact_score,",
  "    GREATEST(",
  "      similarity(coalesce(name,''),$1),",
  "      similarity(coalesce(content,''),$1),",
  "      similarity(coalesce(subject,''),$1),",
  "      similarity(coalesce(interpreted,''),$1),",
  "      ts_rank(",
  "        to_tsvector('simple',coalesce(name,'') || ' ' || coalesce(content,'') || ' ' || coalesce(interpreted,'') || ' ' || coalesce(subject,'')),",
  "        plainto_tsquery('simple',$1)",
  "      )",
  "    ) AS lexical_score,",
  "    GREATEST(",
  "      similarity(coalesce(name,''),$8),",
  "      similarity(coalesce(content,''),$8),",
  "      similarity(coalesce(subject,''),$8),",
  "      similarity(coalesce(interpreted,''),$8)",
  "    ) AS symbol_score,",
  "    CASE WHEN tags && $2::text[] THEN 1 ELSE 0 END AS tag_score,",
  "    GREATEST(",
  "      CASE WHEN metadata ?| $2::text[] THEN 1 ELSE 0 END,",
  "      similarity(metadata::text,$1),",
  "      ts_rank(jsonb_to_tsvector('simple',metadata,'[\"string\",\"key\",\"numeric\"]'::jsonb),plainto_tsquery('simple',$1))",
  "    ) AS metadata_score,",
  "    CASE WHEN $3::halfvec IS NULL THEN 0 ELSE GREATEST(",
  "      CASE WHEN",
  "        vector_content IS NULL OR",
  "        vector_content_provider IS DISTINCT FROM $5 OR",
  "        vector_content_model IS DISTINCT FROM $6 OR",
  "        vector_content_dimensions IS DISTINCT FROM $7",
  "      THEN 0 ELSE 1 - (vector_content <=> $3::halfvec) END,",
  "      CASE WHEN",
  "        vector_subject IS NULL OR",
  "        vector_subject_provider IS DISTINCT FROM $5 OR",
  "        vector_subject_model IS DISTINCT FROM $6 OR",
  "        vector_subject_dimensions IS DISTINCT FROM $7",
  "      THEN 0 ELSE 1 - (vector_subject <=> $3::halfvec) END",
  "    ) END AS semantic_score",
  "  FROM files",
  "  WHERE NOT EXISTS (",
  "    SELECT 1",
  "    FROM jsonb_each_text($9::jsonb) AS filter(key,value)",
  "    WHERE lower(coalesce(metadata ->> filter.key,'')) <> lower(filter.value)",
  "  )",
  ") AS ranked",
  "ORDER BY GREATEST(exact_score,lexical_score,symbol_score,tag_score,metadata_score,semantic_score) DESC",
  "LIMIT $4"
].join("\n");

const ATOM_SEARCH_SQL = [
  "SELECT *",
  "FROM (",
  "  SELECT a.id,a.content,a.metadata,a.updated_at,",
  "    CASE WHEN lower(a.content)=lower($1) THEN 1 ELSE 0 END AS exact_score,",
  "    GREATEST(",
  "      similarity(a.content,$1),",
  "      ts_rank(to_tsvector('simple',a.content || ' ' || array_to_string(a.tags,' ')),plainto_tsquery('simple',$1))",
  "    ) AS lexical_score,",
  "    similarity(a.content,$8) AS symbol_score,",
  "    CASE WHEN a.tags && $2::text[] THEN 1 ELSE 0 END AS tag_score,",
  "    GREATEST(",
  "      CASE WHEN a.metadata ?| $2::text[] THEN 1 ELSE 0 END,",
  "      similarity(a.metadata::text,$1),",
  "      ts_rank(jsonb_to_tsvector('simple',a.metadata,'[\"string\",\"key\",\"numeric\"]'::jsonb),plainto_tsquery('simple',$1))",
  "    ) AS metadata_score,",
  "    CASE WHEN",
  "      $3::halfvec IS NULL OR",
  "      a.vector IS NULL OR",
  "      a.vector_provider IS DISTINCT FROM $5 OR",
  "      a.vector_model IS DISTINCT FROM $6 OR",
  "      a.vector_dimensions IS DISTINCT FROM $7",
  "    THEN 0 ELSE 1 - (a.vector <=> $3::halfvec) END AS semantic_score,",
  "    COALESCE(",
  "      jsonb_agg(jsonb_build_object('id',f.id,'content',f.content)) FILTER (WHERE f.id IS NOT NULL),",
  "      '[]'::jsonb",
  "    ) AS docked_files",
  "  FROM atoms a",
  "  LEFT JOIN atom_file_docks d ON d.atom_id=a.id",
  "  LEFT JOIN files f ON f.id=d.file_id",
  "  WHERE COALESCE(a.metadata->>'active','true') <> 'false'",
  "    AND NOT EXISTS (",
  "      SELECT 1",
  "      FROM jsonb_each_text($9::jsonb) AS filter(key,value)",
  "      WHERE lower(coalesce(a.metadata ->> filter.key,'')) <> lower(filter.value)",
  "    )",
  "  GROUP BY a.id",
  ") AS ranked",
  "ORDER BY GREATEST(exact_score,lexical_score,symbol_score,tag_score,metadata_score,semantic_score) DESC",
  "LIMIT $4"
].join("\n");

const RELATED_ATOMS_SQL = [
  "SELECT related.id,related.content,related.metadata,related.updated_at,edge.relation,edge.seed_id,",
  "  COALESCE(",
  "    jsonb_agg(jsonb_build_object('id',f.id,'content',f.content)) FILTER (WHERE f.id IS NOT NULL),",
  "    '[]'::jsonb",
  "  ) AS docked_files",
  "FROM (",
  "  SELECT r.relation,",
  "    CASE WHEN r.from_atom_id = ANY($1::uuid[]) THEN r.from_atom_id ELSE r.to_atom_id END AS seed_id,",
  "    CASE WHEN r.from_atom_id = ANY($1::uuid[]) THEN r.to_atom_id ELSE r.from_atom_id END AS related_id",
  "  FROM atom_relations r",
  "  WHERE (r.from_atom_id = ANY($1::uuid[]) OR r.to_atom_id = ANY($1::uuid[]))",
  "    AND r.relation IN ('refines','supersedes')",
  ") AS edge",
  "JOIN atoms related ON related.id=edge.related_id",
  "LEFT JOIN atom_file_docks d ON d.atom_id=related.id",
  "LEFT JOIN files f ON f.id=d.file_id",
  "WHERE COALESCE(related.metadata->>'active','true') <> 'false'",
  "GROUP BY related.id,edge.relation,edge.seed_id"
].join("\n");

export interface SearchQueryAnalysis {
  normalized: string;
  text: string;
  symbolText: string;
  tags: string[];
  metadataFilters: Record<string, string>;
}

export interface SearchSignalCandidate {
  key: string;
  exact: number;
  lexical: number;
  symbol: number;
  semantic: number;
  tag: number;
  metadata: number;
  metadataObject?: Record<string, unknown> | null;
  changedAt?: Date | string | null;
}

export interface FusedSearchScore {
  key: string;
  score: number;
  matchedBy: string[];
}

function identifierParts(value: string): string[] {
  const expanded = value
    .replace(/([\p{Ll}\d])([\p{Lu}])/gu, "$1 $2")
    .replace(/[_.:/\\@#-]+/g, " ");
  return normalizeText(expanded).split(" ").filter(part => part.length > 1);
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

  return { normalized, text, symbolText, tags, metadataFilters };
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
  return Math.exp(-ageDays / 365);
}

export function fuseSearchCandidates(
  candidates: SearchSignalCandidate[],
  options: { rrfK?: number; now?: number } = {}
): FusedSearchScore[] {
  const rrfK = Math.max(1, options.rrfK ?? 60);
  const now = options.now ?? Date.now();
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
      semantic: Math.max(previous.semantic, candidate.semantic),
      tag: Math.max(previous.tag, candidate.tag),
      metadata: Math.max(previous.metadata, candidate.metadata),
      metadataObject: previous.metadataObject ?? candidate.metadataObject,
      changedAt: previous.changedAt ?? candidate.changedAt
    });
  }

  const rows = [...unique.values()];
  const signals = [
    { field: "exact", matchedBy: "exact", weight: 2.4, threshold: 0 },
    { field: "lexical", matchedBy: "text", weight: 1.25, threshold: 0.015 },
    { field: "symbol", matchedBy: "symbol", weight: 1.15, threshold: 0.03 },
    { field: "semantic", matchedBy: "vector", weight: 1, threshold: 0.12 },
    { field: "tag", matchedBy: "tag", weight: 1.45, threshold: 0 },
    { field: "metadata", matchedBy: "metadata", weight: 1.35, threshold: 0.015 }
  ] as const;

  const raw = new Map<string, number>();
  const matched = new Map<string, Set<string>>();
  const maxRrf = signals.reduce((sum, signal) => sum + signal.weight / (rrfK + 1), 0);

  for (const signal of signals) {
    const ranked = rows
      .filter(row => row[signal.field] > signal.threshold)
      .sort((a, b) => b[signal.field] - a[signal.field] || a.key.localeCompare(b.key));

    ranked.forEach((row, index) => {
      raw.set(row.key, (raw.get(row.key) ?? 0) + signal.weight / (rrfK + index + 1));
      const set = matched.get(row.key) ?? new Set<string>();
      set.add(signal.matchedBy);
      matched.set(row.key, set);
    });
  }

  return rows
    .filter(row => (raw.get(row.key) ?? 0) > 0)
    .map(row => {
      const metadata = row.metadataObject ?? {};
      const quality =
        importanceScore(metadata.importance) * 0.6 +
        boundedUnit(metadata.confidence) * 0.4;
      const freshness = freshnessScore(row.changedAt, now);
      const direct = Math.max(
        boundedUnit(row.exact),
        boundedUnit(row.tag) * 0.9,
        boundedUnit(row.metadata) * 0.85,
        boundedUnit(row.lexical) * 0.7,
        boundedUnit(row.symbol) * 0.75,
        boundedUnit(row.semantic) * 0.65
      );
      const fused = maxRrf > 0 ? (raw.get(row.key) ?? 0) / maxRrf : 0;
      const score = Math.min(1, fused * 0.58 + direct * 0.17 + boundedUnit(row.exact) * 0.20 + quality * 0.03 + freshness * 0.02);
      return {
        key: row.key,
        score,
        matchedBy: [...(matched.get(row.key) ?? new Set<string>())]
      };
    })
    .sort((a, b) => b.score - a.score || a.key.localeCompare(b.key));
}

function numberScore(row: QueryResultRow, key: string): number {
  return Math.max(0, Number(row[key] ?? 0));
}

function mergeAtomResult(
  merged: Map<string, SearchItem>,
  row: QueryResultRow,
  score: number,
  matchedBy: string[]
): void {
  const atomId = String(row.id);
  const dockedFiles = Array.isArray(row.docked_files) ? row.docked_files : [];

  if (dockedFiles.length === 0) {
    const key = "atom:" + atomId;
    const existing = merged.get(key);
    if (!existing || score > existing.score) {
      merged.set(key, {
        kind: "atom",
        id: atomId,
        atomId,
        score,
        rawText: String(row.content),
        matchedBy: [...matchedBy]
      });
    }
    return;
  }

  for (const docked of dockedFiles) {
    if (!docked || typeof docked !== "object") continue;
    const value = docked as Record<string, unknown>;
    if (typeof value.id !== "string") continue;
    const key = "file:" + value.id;
    const atomScore = score * 0.95;
    const existing = merged.get(key);
    if (existing) {
      existing.score = Math.max(existing.score, atomScore);
      for (const signal of [...matchedBy, "atom"]) {
        if (!existing.matchedBy.includes(signal)) existing.matchedBy.push(signal);
      }
    } else {
      merged.set(key, {
        kind: "file",
        id: value.id,
        fileId: value.id,
        score: atomScore,
        rawText: typeof value.content === "string" ? value.content : null,
        matchedBy: [...new Set([...matchedBy, "atom"])]
      });
    }
  }
}

export async function searchFoundation(
  db: Database,
  ai: AIProvider,
  query: string,
  limit = 10
): Promise<SearchItem[]> {
  const normalizedQuery = normalizeText(query);
  if (!normalizedQuery) return [];

  const analysis = analyzeSearchQuery(normalizedQuery);
  let queryEmbedding: EmbeddingResult | null = null;
  try {
    queryEmbedding = await ai.embed(analysis.text, "query");
  } catch {
    queryEmbedding = null;
  }

  const vector = vectorLiteral(queryEmbedding?.vector ?? null);
  const candidateLimit = Math.max(40, Math.min(limit * 10, 300));
  const params = [
    analysis.text,
    analysis.tags,
    vector,
    candidateLimit,
    queryEmbedding?.provider ?? null,
    queryEmbedding?.model ?? null,
    queryEmbedding?.dimensions ?? null,
    analysis.symbolText,
    JSON.stringify(analysis.metadataFilters)
  ];

  const [files, atoms] = await Promise.all([
    db.query(FILE_SEARCH_SQL, params),
    db.query(ATOM_SEARCH_SQL, params)
  ]);

  const signalCandidates: SearchSignalCandidate[] = [
    ...files.rows.map(row => ({
      key: "file:" + String(row.id),
      exact: numberScore(row, "exact_score"),
      lexical: numberScore(row, "lexical_score"),
      symbol: numberScore(row, "symbol_score"),
      semantic: numberScore(row, "semantic_score"),
      tag: numberScore(row, "tag_score"),
      metadata: numberScore(row, "metadata_score"),
      metadataObject: record(row.metadata),
      changedAt: row.modified_at ? new Date(row.modified_at) : null
    })),
    ...atoms.rows.map(row => ({
      key: "atom:" + String(row.id),
      exact: numberScore(row, "exact_score"),
      lexical: numberScore(row, "lexical_score"),
      symbol: numberScore(row, "symbol_score"),
      semantic: numberScore(row, "semantic_score"),
      tag: numberScore(row, "tag_score"),
      metadata: numberScore(row, "metadata_score"),
      metadataObject: record(row.metadata),
      changedAt: row.updated_at ? new Date(row.updated_at) : null
    }))
  ];

  const fused = new Map(
    fuseSearchCandidates(signalCandidates).map(item => [item.key, item])
  );
  const merged = new Map<string, SearchItem>();

  for (const row of files.rows) {
    const id = String(row.id);
    const ranked = fused.get("file:" + id);
    if (!ranked) continue;
    merged.set("file:" + id, {
      kind: "file",
      id,
      fileId: id,
      score: ranked.score,
      rawText: row.content === null ? null : String(row.content),
      matchedBy: ranked.matchedBy
    });
  }

  for (const row of atoms.rows) {
    const atomId = String(row.id);
    const ranked = fused.get("atom:" + atomId);
    if (!ranked) continue;
    mergeAtomResult(merged, row, ranked.score, ranked.matchedBy);
  }

  const seedAtoms = atoms.rows
    .map(row => {
      const id = String(row.id);
      return { id, ranked: fused.get("atom:" + id) };
    })
    .filter((item): item is { id: string; ranked: FusedSearchScore } => Boolean(item.ranked))
    .sort((a, b) => b.ranked.score - a.ranked.score)
    .slice(0, Math.min(24, Math.max(8, limit * 2)));

  if (seedAtoms.length > 0) {
    const seedScores = new Map(seedAtoms.map(item => [item.id, item.ranked]));
    const related = await db.query(RELATED_ATOMS_SQL, [seedAtoms.map(item => item.id)]);
    for (const row of related.rows) {
      const seed = seedScores.get(String(row.seed_id));
      if (!seed) continue;
      const relation = String(row.relation);
      const relationScore = seed.score * (relation === "refines" ? 0.78 : 0.72);
      mergeAtomResult(
        merged,
        row,
        relationScore,
        [...new Set([...seed.matchedBy, "relation:" + relation])]
      );
    }
  }

  return [...merged.values()]
    .sort((a, b) => b.score - a.score)
    .slice(0, Math.max(1, Math.min(limit, 100)));
}
