# Retrieval and v8 scope repair: implementation and local verification

## Root causes and changes

* Two v7 SQL expressions (project **and conversation**) used `\s+` in ordinary JavaScript template literals. JavaScript consumed the backslash, so PostgreSQL replaced runs of the letter `s`, not whitespace. Both now use PostgreSQL `[[:space:]]+`. Audited the rest of `schema.ts` and the v6 SQL fixture: no additional escaped regular expressions of this kind.
* v8 changes **only `scope_key`**, under the existing schema transaction/advisory lock and an Atom write lock. It uses the existing metadata precedence `project`, `projectId`, `repo` (or `conversationId`), NFKC/lowercase/whitespace normalization. A row must have exactly the demonstrable buggy v7 value, and a different corrected value. Original `atoms_backup_v6` metadata, when available, must corroborate that identity. Disagreement or an empty identity aborts the transaction. It checks the *complete prospective identity set*, including unrepaired rows and forgotten records, against the existing non-superseded unique-index predicate before any update. It never merges, deletes, or rewrites content. `atom_scope_repairs_v8` records IDs and old/new scopes. Schema version 8 makes initialization idempotent.
* Standalone vector evidence previously qualified at 0.6; RRF contributed relevance even for the best of an entirely irrelevant candidate pool. Direct vector score plus metadata quality/fact freshness could carry cosine 0.65–0.68 over default score 0.22. `foundation explain Comet` reproduced five unrelated results: zero exact/lexical/tag evidence, fuzzy at most 0.5, vector-only acceptance, top cosine 0.67944 and score 0.25418.
* Short Latin names/acronyms and short structured-identifier lookups now need token/identifier/tag evidence (metadata can corroborate the identity); fuzzy spelling similarity and vectors cannot invent an entity. This applies case-insensitively, not to a hardcoded list. Natural-language and mixed-language sentences still support semantic-only matches.
* Semantic corroboration remains 0.6 with the existing 0.15 relative band. Standalone semantics has a **separate 0.68 floor**. Global minScore, signal weights, lifecycle behavior and scope predicates are unchanged. The calibration sweep retained the same positive acceptance through 0.68; 0.70 lost a positive and 0.72/0.75 sacrificed more recall. Thus 0.68 is the conservative end of the recall-preserving plateau, not a globally raised cutoff. The explicit identity gate is the primary protection for entity queries. This is an empirical operating point, not a model-independent probability of relevance.
* Latin/digit lexical and identifier matching now uses Unicode word boundaries instead of `includes`. Camel-case expansion is actually lowercased; Korean noun particles are supported without accepting arbitrary Korean substrings. Tag matching remains exact. Indexed fuzzy/substring generators may still propose candidates, but rescoring no longer treats an incidental substring as lexical evidence.
* Tied signal values receive tied RRF credit instead of UUID-dependent artificial rank differences. Lexical generator ties have deterministic ID ordering. The existing decisive-tail rule now handles two-term queries (e.g. `KSL telemetry`) and no longer throws away a stronger semantic answer just because a lexical answer exists (e.g. `What database does Foundation use?`). Provenance folding, source diversification, historical penalties and conflict annotations remain intact.

**Unscoped retrieval is unchanged:** no default project, inferred project isolation, mandatory filter, or restriction on global searches was added. Only an explicitly requested scope limits the search.

## Production migration (AppleDocker, 2,620 Atoms)

The obsolete `dockerd-machine` SSH target was unreachable. Used the existing Apple Container `apple-dockerd` and its Docker service directly, including the actual service's CLI and Gemini embedding configuration. A temporary TCP relay allowed local integration tests to use PostgreSQL; it is not required for normal service operation.

1. Created `.local/repair-v8/before.dump` with `pg_dump -Fc`. SHA-256:
   `8d24a1587fb21351b88c68c3c397cd8d9e3d77b2c630cecafae5d309f008be6f`.
2. Read the archive manifest, restored it into **foundation_repair_test**, and verified all table counts/fingerprints against production before mutation. They matched. (Both `container exec` and `docker exec` need `-i` to forward restore input.)
3. Tested repair + transaction rollback, full initialization, repeated initialization, reverse scope rollback, and reapplication on the restored database. Also tested the checked-in guarded rollback SQL on that copy. Unit/integration fixtures independently test collisions and ambiguous original metadata.
4. Applied v8 to the live database. **857 proven corrupted keys repaired, 762 active**. All 2,620 Atom IDs and all 394 relations remain. Full row fingerprints excluding only `atoms.scope_key` match before/after for every existing table, including metadata, embeddings, validity intervals, statuses, timestamps, events, and backups.
5. The preliminary **883 / 788 active** counts included **26 legitimate metadata/scope differences**: 13 normalized `ksl` scopes whose metadata repo is a full path, and 13 CruiseMissile path scopes whose metadata project is a short name. These are not outputs of the buggy v7 transformation; they were deliberately **not changed**. There are zero remaining demonstrable v7 transforms and no collisions.
6. KSL active scope `ksl`: **153 → 265**; corrupted `k l`: **112 → 0**. KSL now also has 31 superseded rows under the correct key; no history was dropped. Database versions 6, 7, 8 are retained.
7. Built successfully. Deployed only compiled `search`/`schema` modules into the running service, restarted it and verified `/health` reports schema 8 and 2,620 Atoms. Patched `foundation-foundation:latest` equivalently for subsequent container recreation. Retained `foundation-foundation:pre-scope-v8` and `.local/repair-v8/runtime-before.tar` for runtime rollback. Unrelated uncommitted application changes were not deployed.

### Rollback

Stop Foundation before rollback; take/verify a new backup first if anything has changed. Use `scripts/rollback-scope-v8.sql` with `psql -v ON_ERROR_STOP=1` on the intended database. It takes the schema advisory lock, rejects post-repair scope drift or reverse uniqueness collisions, changes only repaired keys, archives the audit table, and removes schema version 8. It is a no-op without version 8. Deploy the retained pre-v8 runtime before starting the service, otherwise initialization correctly reapplies v8. A second rollback cycle needs a different audit archive name. Full database restore is a last-resort recovery option, **not** the normal scope-only rollback: restoring an old dump could lose subsequent writes.

## Reproducible evaluation

* `bench/retrieval-quality.json`: 24 hand-authored documents and 28 labeled queries with complete fixture relevance sets. Includes weak topic decoys, Comet/Commit, independent nonexistent names, exact identifiers, AppleDocker paraphrases, Korean/English and mixed-language queries, corrupted scopes, unscoped cross-project PostgreSQL search, no-answer queries, and current/as-of/history facts.
* `npm run bench:quality` uses **a newly created disposable database only**, the configured embedding provider, cached embeddings, deterministic document IDs and per-generator EXPLAIN/latency plus per-candidate scoring diagnostics. Requires `TEST_DATABASE_URL` with permission to create disposable databases. Use the same Gemini model as production for model-quality numbers; no-embedding/fake-provider runs are not cross-language quality evidence.
* For a paired old implementation, export the old `src/search.ts` beside the current source as `src/.search-baseline.ts`, then run `QUALITY_SEARCH_MODULE=../src/.search-baseline.js QUALITY_PHASE=before npm run bench:quality`. Remove that temporary file afterward. `QUALITY_PHASE=before` keeps the fixture's v7-corrupted scopes; the default runs v8. The baseline source came from the unchanged-at-start HEAD search file. A common `QUALITY_EMBED_CACHE` makes comparisons deterministic and excludes provider latency after warm-up.
* `npm run bench:quality:calibrate` sweeps semantic-only gates using only calibration-labeled cases and the saved candidate signals. It is an offline **acceptance** diagnostic, not a substitute for the complete end-to-end benchmark. The separate regression split includes independent negative entities; it is a regression suite, not a statistically blinded test set.
* Compact measured results and per-query returned keys: `bench/retrieval-quality-results.json`. Full traces: `bench/results/quality-{before,after}.json` (ignored generated artifacts). Existing lifecycle benchmark: `npm run bench -- --projects=8 --per-project=60`.

### Paired Gemini embedding-2 results

| Metric | Before | After |
|---|---:|---:|
| Precision@5, positive queries, five fixed slots | 0.290 | 0.330 |
| Recall@5 | 0.750 | 0.925 |
| MRR | 0.850 | 1.000 |
| nDCG@5 | 0.773 | 0.942 |
| Precision among actually returned results | 87.9% | 94.3% |
| False-positive fraction of returned results | 12.1% | 5.7% |
| No-answer accuracy (8 negative queries) | 100% | 100% |
| Cached end-to-end p50 / p95 | 166.8 / 178.1 ms | 173.1 / 196.0 ms |

Fixed-slot Precision@5 is intentionally low when there are only 1–3 relevant fixture documents: empty slots count zero. The false-positive measure above is **FP / returned**, not population false-positive rate over every document in the corpus. The regression split's Recall@5 is 0.767 → 1.000, nDCG 0.774 → 1.000 and false-positive fraction 12.5% → 3.8%. The small fixture did not reproduce the real Comet hubness by itself; the production diagnostics below are necessary evidence, not an omitted failure.

### Real-corpus checks

On the unchanged live 2,620-Atom corpus, a paired cached-embedding run of 22 diagnostic queries measured p50 **124.6 → 141.3 ms**, p95 **148.8 → 165.6 ms**. Six labeled no-answer queries (Comet, comet, Neptune, Pine, Mars tomatoes, Korean banana/space recipe) improved **2/6 → 6/6 empty** and **15 → 0 false-positive results**. Separate fresh `foundation explain` calls inside the deployed service confirm:

* Comet, Neptune, Pine: empty; Comet candidates explicitly rejected for missing an identity anchor, regardless of vector proximity.
* AppleDocker and KSL: five results each; `landing guidance --project=KSL` retrieves correct scoped rows plus intentional global fallback.
* Korean `착륙 유도 제어`: useful English landing/control memories across KSL and KSPShuttleLander as well as globals.
* `How are containers hosted on this Mac?`: four semantic-only AppleDocker memories with **zero lexical overlap** survive.
* Unscoped `git commit --limit=10`: results span global scope, Yeet, StructToolingAB, KSPFlightComputer and CruiseMissile, proving global retrieval remains open.

Fresh service calls, including Gemini/network and explain overhead, were roughly 0.7–1.1 seconds. PostgreSQL still uses FTS/tag GIN and scope btree indexes; plans are captured in the traces. The planner chooses sequential scans for some vector/trigram/exact queries at this small corpus size; no forced index hints, vector-order changes, or mandatory scope predicates were introduced.

## Verification and limitations

* Typecheck, build and the isolated pgvector smoke test pass. The smoke assertion was updated from the obsolete `matchedBy: atom` flag to checking actual Atom/File provenance under the existing Atom-first API. **44 unit tests + 36 PostgreSQL integration tests pass.** The default unit command skips the DB-dependent tests; they were separately run against the real PostgreSQL/pgvector instance using disposable databases.
* Existing lifecycle benchmark: 643 Atoms, 8 Files, 237 queries, 8 projects. Hybrid reported Precision@5 / Recall@10 1.0 / 1.0, zero stale results/project leaks in explicitly scoped checks, 3/3 empty no-answer cases, 8/8 file recall/conflict annotations, 50/50 reconciliation checks, and one identity for 12 concurrent identical writes. Its older precision convention differs from the fixed-slot metric above; its hybrid returned-irrelevance fraction is still 14.3%, so this is not a claim of perfect retrieval.
* Semantic similarity is not entailment or calibrated relevance probability. Two weak fixture results remain and some Korean paraphrase recall is below 100%; the production Korean AppleDocker paraphrase still favors binary/volume facts over the lower-similarity relay-service fact. Broader annotated real-corpus evaluation would be needed before provider-specific recalibration. Conservative single-Latin-term lookup can miss an alias with no identity evidence; phrase the intent as a natural-language question for semantic search. No translation service, reranking service, ingestion change, or embedding rewrite was added.
* Verified backup, migration fingerprints, CLI before/after traces, test logs and live-query diagnostics are retained privately in `.local/repair-v8/`. That directory is git-excluded and the database backup is not committed.

## Additional verification receipts

The backup was subsequently restored **again** into a newly created `foundation_v8_evidence` database, rather than relying on the earlier command logs. Assertions confirmed all 16 existing table fingerprints (excluding only Atom scope keys) matched live production; the restored snapshot was migrated, scope-rolled back with the checked-in SQL, reapplied and initialized a second time. Full fingerprints **including scope keys** matched the original restored snapshot after rollback and matched the first repaired snapshot after both reapplications. The temporary database was then dropped. Machine-readable assertion outputs, actual key groups, and metadata recovery counts are in `bench/migration-v8-verification.json`.

Separate disposable guard probes produced the actual errors `v8: scope repair would collide with an existing identity; no scopes repaired` and `v8: original and current metadata disagree; no scopes repaired`; in each case schema 8 was not recorded and the input Atom rows/scopes remained unchanged. The 28-query paired Gemini benchmark was also rerun: every reported quality metric reproduced exactly (warm p95 178.2 → 193.8 ms on this rerun). Actual CLI result excerpts and rejection signals are retained in `.local/repair-v8/observable-search-verification.json`.

### Chronology, preservation, and planner evidence

The original, unmodified logs establish ordering: full archive creation **13:44:03**, disposable validation completion **13:48:09** (`rollbackVerified: true`, `idempotent: true`), then production migration **13:51:58** (local time). Strict comparison of all 16 table/count/hash tuples in those two original logs confirms `restored-before == disposable-after == production-before == production-after`, excluding only Atom scope keys and migration bookkeeping. This includes 3,698 memory events, 113 memory requests, 394 relations, the 2,005-row original Atom backup, and the 309-row relation backup. `bench/verification-receipts.json` records these original-log fingerprints and timestamps; these are separate from the later repeat verification.

Post-deployment `foundation explain --plans` shows actual FTS/tag GIN, scope/status btree and trigram GIN bitmap index scans. Broad global queries also use FTS/tag/trigram indexes with no project predicate. HNSW remains valid/ready (21 MB); PostgreSQL normally prefers a sequential scan for this small corpus. A **diagnostic-only**, transaction-local disabling of sequential/bitmap scans demonstrates an `Index Scan using atoms_vector_hnsw` ordered by cosine distance. Those settings were rolled back and are not in application code.

For working-tree preservation, the eight initially dirty files are `.env.example`, `README.md`, `src/{ai,config,interpreter,mcp}.ts`, and `tests/{config,mcp}.test.ts`. Their final diff statistics match the earlier observed statistics, and all last-modification timestamps precede this repair (13:29–13:39). No edit/write command in this job targeted these files. Initial byte hashes were not taken; the final hash receipts must not be misrepresented as an independently captured initial/final byte-hash comparison. The new work is confined to previously clean files and new benchmark/test/report files.

Final combined verification: with `TEST_DATABASE_URL` set, `npm test` completed with **17/17 test files and 80/80 tests passing, no skips** (44 unit + 36 PostgreSQL integration). `npm run typecheck` and `npm run build` both exited successfully. The temporary database relay was stopped afterward.
