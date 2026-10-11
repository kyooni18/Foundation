import { normalizeText } from "./utils.js";

export const SCOPE_KINDS = ["global", "project", "conversation"] as const;
export type ScopeKind = (typeof SCOPE_KINDS)[number];

export interface MemoryScope {
  kind: ScopeKind;
  /** Normalized project name / conversation id. Always "" for global scope. */
  key: string;
}

/** Query-side view of which scopes are visible. Neither field set means "unscoped" (legacy: everything). */
export interface ScopeContext {
  project?: string | null;
  conversationId?: string | null;
}

export const GLOBAL_SCOPE: MemoryScope = { kind: "global", key: "" };

export function normalizeScopeKey(value: unknown): string {
  return typeof value === "string" ? normalizeText(value).toLowerCase() : "";
}

export function projectKeyFromMetadata(metadata: Record<string, unknown>): string {
  for (const field of ["project", "projectId", "repo"]) {
    const key = normalizeScopeKey(metadata[field]);
    if (key) return key;
  }
  return "";
}

export function scopeLabel(scope: MemoryScope): "Global" | "Project" | "Conversation" {
  return scope.kind === "project" ? "Project" : scope.kind === "conversation" ? "Conversation" : "Global";
}

function kindFromLabel(value: unknown): ScopeKind | null {
  if (typeof value !== "string") return null;
  const lowered = value.trim().toLowerCase();
  return (SCOPE_KINDS as readonly string[]).includes(lowered) ? lowered as ScopeKind : null;
}

export interface WriteScopeInput {
  /** Explicit caller override (wins over the model's guess). */
  scope?: string | null;
  project?: string | null;
  conversationId?: string | null;
  /** Scope guessed by the extraction model. */
  extractedScope?: string | null;
  metadata?: Record<string, unknown>;
}

export interface ResolvedScope {
  scope: MemoryScope;
  /** Set when the requested scope could not be honoured and the memory fell back to Global. */
  unresolved: string | null;
}

/**
 * Decide the storage scope for a new memory. A scope is only ever narrower than Global when a concrete
 * key is available, so project/conversation memories can never be stored with an empty identity.
 */
export function resolveWriteScope(input: WriteScopeInput): ResolvedScope {
  const metadata = input.metadata ?? {};
  const project = normalizeScopeKey(input.project) || projectKeyFromMetadata(metadata);
  const conversation = normalizeScopeKey(input.conversationId) || normalizeScopeKey(metadata.conversationId);
  const explicit = kindFromLabel(input.scope);
  const extracted = kindFromLabel(input.extractedScope);
  const wanted: ScopeKind | null = explicit ?? extracted;

  if (explicit === "global" || extracted === "global" && !explicit) {
    return { scope: GLOBAL_SCOPE, unresolved: null };
  }
  if (wanted === "conversation") {
    if (conversation) return { scope: { kind: "conversation", key: conversation }, unresolved: null };
    return { scope: GLOBAL_SCOPE, unresolved: "conversation scope requested without a conversationId" };
  }
  if (wanted === "project" || (wanted === null && project)) {
    if (project) return { scope: { kind: "project", key: project }, unresolved: null };
    return { scope: GLOBAL_SCOPE, unresolved: "project scope requested without a project" };
  }
  if (project && wanted === null) return { scope: { kind: "project", key: project }, unresolved: null };
  return { scope: GLOBAL_SCOPE, unresolved: null };
}

export interface ScopeFilterParams {
  all: boolean;
  project: string | null;
  conversation: string | null;
}

export function scopeFilterParams(context: ScopeContext): ScopeFilterParams {
  const project = normalizeScopeKey(context.project) || null;
  const conversation = normalizeScopeKey(context.conversationId) || null;
  return { all: !project && !conversation, project, conversation };
}

/** Whether an Atom stored in `scope` is visible from `context`. Mirrors the SQL filter exactly. */
export function scopeVisible(scope: MemoryScope, context: ScopeContext): boolean {
  const filter = scopeFilterParams(context);
  if (filter.all || scope.kind === "global") return true;
  if (scope.kind === "project") return scope.key === filter.project;
  return scope.key === filter.conversation;
}

/**
 * SQL predicate for atom visibility. `$all`, `$project`, `$conversation` are the 1-based parameter
 * indexes of ScopeFilterParams.
 */
export function atomScopeSql(alias: string, all: number, project: number, conversation: number): string {
  return `(
    $${all}::boolean
    OR ${alias}.scope_kind = 'global'
    OR (${alias}.scope_kind = 'project' AND ${alias}.scope_key = $${project}::text)
    OR (${alias}.scope_kind = 'conversation' AND ${alias}.scope_key = $${conversation}::text)
  )`;
}

export type TemporalMode = "current" | "asof" | "history";

export interface TemporalQuery {
  mode: TemporalMode;
  at: Date;
}

export function temporalQuery(input: { asOf?: string | Date | null; history?: boolean }, now = new Date()): TemporalQuery {
  if (input.asOf) {
    const at = input.asOf instanceof Date ? input.asOf : new Date(input.asOf);
    if (!Number.isFinite(at.getTime())) throw new Error("asOf must be a valid ISO-8601 timestamp");
    return { mode: "asof", at };
  }
  return { mode: input.history ? "history" : "current", at: now };
}

/**
 * SQL predicate for temporal validity. `$mode` is text ('current'|'asof'|'history'), `$at` timestamptz.
 * Forgotten memories are never returned. Superseded memories are only visible for as-of/history reads,
 * and only inside the interval in which they were true.
 */
export function atomTemporalSql(alias: string, mode: number, at: number): string {
  return `(
    ${alias}.status <> 'forgotten'
    AND (
      $${mode}::text = 'history'
      OR (
        ($${mode}::text = 'current' AND ${alias}.status = 'active')
        OR $${mode}::text = 'asof'
      )
      AND (${alias}.valid_from IS NULL OR ${alias}.valid_from <= $${at}::timestamptz)
      AND (${alias}.expires_at IS NULL OR ${alias}.expires_at > $${at}::timestamptz)
    )
  )`;
}

export type TemporalState = "current" | "superseded" | "expired" | "future";

export function temporalState(
  row: { status: string; validFrom: Date | null; expiresAt: Date | null },
  at: Date
): TemporalState {
  if (row.status === "superseded") return row.expiresAt && row.expiresAt > at ? "current" : "superseded";
  if (row.validFrom && row.validFrom > at) return "future";
  if (row.expiresAt && row.expiresAt <= at) return "expired";
  return "current";
}

export type AddParam = (value: unknown) => string;

/** Atom visibility predicate. Emits `TRUE` for unscoped reads so the planner sees no extra quals. */
export function atomScopePredicate(alias: string, filter: ScopeFilterParams, add: AddParam): string {
  if (filter.all) return "TRUE";
  const parts = [`${alias}.scope_kind = 'global'`];
  if (filter.project) parts.push(`(${alias}.scope_kind = 'project' AND ${alias}.scope_key = ${add(filter.project)}::text)`);
  if (filter.conversation) parts.push(`(${alias}.scope_kind = 'conversation' AND ${alias}.scope_key = ${add(filter.conversation)}::text)`);
  return `(${parts.join(" OR ")})`;
}

/** Temporal predicate, specialised per mode so index-friendly plans stay possible. */
export function atomTemporalPredicate(alias: string, query: TemporalQuery, add: AddParam): string {
  if (query.mode === "history") return `${alias}.status <> 'forgotten'`;
  const at = add(query.at.toISOString());
  const status = query.mode === "current" ? `${alias}.status = 'active'` : `${alias}.status <> 'forgotten'`;
  return `(${status} AND (${alias}.valid_from IS NULL OR ${alias}.valid_from <= ${at}::timestamptz) AND (${alias}.expires_at IS NULL OR ${alias}.expires_at > ${at}::timestamptz))`;
}
