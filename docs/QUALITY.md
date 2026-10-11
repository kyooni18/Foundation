# Quality, benchmarks and migration notes

## Running the quality suite

```sh
npm run test:db:up                      # disposable pgvector Postgres on :55433 (never real data)
npm run test:integration                # identity, scope, temporal, remember, migration, diagnostics
TEST_DATABASE_URL=postgresql://postgres:test@127.0.0.1:55433/postgres npm run bench -- --per-project=30
TEST_DATABASE_URL=... npx tsx bench/scale.ts --atoms=20000   # index-usage / latency at scale
npm run test:db:down
```

Integration tests create a fresh database per test file and drop it afterwards. They skip when `TEST_DATABASE_URL` is unset, so `npm test` stays dependency-free.
The embedding provider in tests/benchmarks is a deterministic bag-of-words fake: the numbers measure Foundation's candidate generation, thresholds, scoping and lifecycle handling, not a vendor model.

## Benchmark results (synthetic, 8 projects, 237 queries)

Query mix: project-specific facts, exact identifiers, identical-wording twins in other projects, outdated (superseded) decisions, contradictory memories, global fallback, unanswerable queries and Files with no Atoms.

| metric | before this work* | hybrid (embeddings) | lexical only (embeddings down) |
| --- | --- | --- | --- |
| Precision@5 | n/a | 1.00 | 1.00 |
| Recall@10 | n/a | 1.00 | 1.00 |
| irrelevant-result rate | n/a | 0.10 | 0.20 |
| cross-project leakage | structurally possible | 0 | 0 |
| stale (superseded) results | possible | 0 | 0 |
| unanswerable → empty | never (limit padded) | 3/3 | 3/3 |
| File-only recall | – | 8/8 | 8/8 |
| conflicts flagged | – | 8/8 | 8/8 |
| p50 / p95 latency (650 Atoms) | – | ~10 / ~13 ms | ~6 / ~11 ms |
| reconciliation correctness | – | 50/50 | |
| 12 concurrent identical writes | 1 row (global unique) | 1 Atom | |

\* The pre-change code had no scope or lifecycle model, so scope/stale metrics cannot be expressed there; the legacy ranking SQL is kept in `bench/legacy-sql.ts` as the latency baseline.

### Scale and index usage (20 000 Atoms, 20 projects, 3072-d halfvec)

| | p50 | p95 |
| --- | --- | --- |
| legacy `ORDER BY GREATEST(...)` over every row (seq scan) | ~780 ms | ~900–1000 ms |
| current pipeline, project-scoped | ~30 ms | ~75 ms |

Measured findings that shaped the SQL:

* The legacy ranking computed every signal for every row, so no index could be used (`Seq Scan on atoms`).
* A text-literal `halfvec` bind parameter costs ~0.75 ms per evaluated row on seq-scan paths (46 ms vs 3 ms for 60 rows); the query vector is now sent as `real[]` and cast once.
* An unconditional `NOT EXISTS (… jsonb_each_text($filters) …)` or `$all OR …` predicate forced a seq scan + sort (208 ms) even with HNSW available; with iterative scan and no such quals pgvector uses the HNSW index (3–27 ms). Predicates are now generated only when they apply.
* For a project scope covering ~5 % of rows the planner prefers the `(scope_kind, scope_key, status)` btree and computes exact distances, which is also more accurate than approximate HNSW.

Embedding quality with a real provider was **not** measured here (no live provider keys in the test environment); the semantic threshold (`0.60`, within `0.15` of the best vector hit) is conservative and should be re-tuned with `foundation explain` against production data.

## v6 → v7 migration

Applied automatically (and idempotently, under an advisory lock) by `Database.initialize()` at startup. **Take a backup first**: `sh scripts/backup-db.sh`.

What it does, in order:

1. adds `scope_kind`, `scope_key`, `status`, `valid_from`, `expires_at` to `atoms` (defaults keep every existing row valid);
2. copies metadata-only snapshots into `atoms_backup_v6` and `atom_relations_backup_v6` (one-time, guarded by the schema version);
3. backfills: `metadata.active=false` → `superseded` if it has `supersededBy` or an incoming `supersedes` relation, else `forgotten`; `validFrom`/`expires` parsed with a safe cast (garbage → NULL); superseded Atoms get `expires_at` = the superseding Atom's validity start; `scope=Project|null` with `project`/`projectId`/`repo` metadata → project scope (key lower-cased); `scope=Conversation` + `conversationId` → conversation scope; project/conversation claims without a key stay Global and get `metadata.scopeUnresolved=true`; `scope=Global` stays Global even if a project is mentioned;
4. replaces the global unique index on `content_hash` with unique `(scope_kind, scope_key, content_hash) WHERE status <> 'superseded'`.

Atom ids, relations, docks, external (Notion) bindings, files and vectors are untouched; nothing is dropped except the superseded unique index. The backfill is covered by `tests/integration/migration.test.ts`, which migrates a database built from the real v6 schema.

**Rollback caveat:** an old (v6) binary recreates the global `content_hash` unique index at startup, which fails once identical content exists in two scopes. Roll back by restoring the backup, not by running the old binary against a migrated database.

**Operator follow-ups after upgrading**

* `foundation diagnose` — check `scope_unresolved` and `content_shared_across_scopes`.
* Set `FOUNDATION_PROJECT` for per-workspace stdio MCP servers that should be isolated.
* Existing consumers that pass no scope keep the previous "see everything" behaviour.

## Known limitations

* Live runtime: the production database was not reachable from the build environment (tunnel down), so the migration was verified on a database built from the v6 schema with representative legacy rows, not on production data.
* Reconciliation quality still depends on the LLM resolver; Foundation guards its decisions (thresholds, same-scope targets, optimistic locking) but cannot judge a wrong high-confidence decision. Without a resolver, new text is stored as a new Atom (exact/case-only duplicates are still caught).
* Relative-time grounding ("next month") is delegated to the extraction model; Foundation only enforces the resulting `validFrom`/`expires`.
* `unscoped` recall intentionally keeps legacy "everything" semantics; use `project`/`FOUNDATION_PROJECT` for isolation.
* Semantic thresholds are untuned for real provider embeddings.
