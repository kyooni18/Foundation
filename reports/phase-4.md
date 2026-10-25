# Phase 4 — Knowledge Integrity: validation report

Date: 2026-10-25. Scope: IMPLEMENTATION_PLAN Phase 4. Decisions: D-92–D-104 in
`DECISIONS.md`. No paid model or network-dependent test was run. Every model-facing
path uses fake providers or saved fixtures.

## 1. Implemented features and architecture

- **Dependency semantics** for `premise`, `restates`, `alternative` and `informed_by`
  (DATA_CONTRACTS §6). Dependencies can be declared at any time, by the user, by
  `system` during extraction, or by agents/imports for Claims their own role recorded
  (D-94). Self-dependencies are rejected, and so are cycles, including cycles through
  retracted edges. Dependencies are retracted and restored with `integrity_lifecycle`.
- **Materialized three-state validity** in `claim_validity`: valid, needs_revalidation
  or invalid, with reason codes and `deps_unknown`. Validity is a pure function of the
  current authoritative state (D-92).
  - Propagation is incremental over the downward closure and runs in the same writer
    transaction as the triggering change (D-93).
  - An independent oracle replays `status_event` and recomputes every Claim.
- **Provenance**:
  - `source_citation`, with cycle and scope triggers.
  - `session_read` for authenticated agent sessions, which drives `deps_unknown` (D-96).
    The session is carried in the `x-primeradiant-session` header (D-97).
  - `claim_support`, which counts distinct origins inside the caller's projection (D-98).
- **Identity**:
  - Thin `entity` rows plus append-only `identity_assertion` rows
    (`refers_to`, `same_as`, `distinct_from`, `alias`).
  - Tiered union-find resolution, user > agent > model.
  - Merges are reversible. Model suggestions are off by default (`IdentityPolicy`).
  - Re-extraction retracts the previous run's model suggestions; user and agent
    corrections survive (D-99).
- **Temporal**:
  - `relation` (`supersedes`, `exception`, `backfill`) and immutable
    `model_judgment(task=temporal)` rows.
  - `claims_as_of` ports the R2 history rule.
  - Freshness thresholds are configurable (`FreshnessPolicy`) (D-100, D-101).
- **Revalidation**:
  - Durable `job(kind='revalidate')` rows, coalesced to one pending job per Claim.
  - Leases are fenced and completion is idempotent.
  - Outcomes are `resolved`, `repaired` or `semantic_pending`. No model is ever called (D-102).
  - The `serve` worker and the `revalidate-work` CLI run the jobs.
- **Projections**:
  - Claim views carry real validity, reasons, `deps_unknown`, support and mentions.
  - The user projection hides invalid Claims. The D-38 heuristic is gone (D-103).
- **Transports**:
  - All new operations go through the shared `Service::dispatch`, so HTTP and MCP use
    identical core code paths. There are now 24 MCP tools.
  - New CLI commands: `integrity-rebuild` and `revalidate-work`.

Module layout and write path: `ARCHITECTURE.md` §6. Source: `src/integrity/{mod,validity,revalidation,provenance,identity,temporal}.rs`.

## 2. Database migration

`src/store/migrations/0003_knowledge_integrity.sql` (schema 2 → 3) follows SQLite's
12-step table-rebuild procedure (D-95).

**Rebuilt tables:**
- `status_event` gains the new target kinds. Every `seq` is preserved.
- `claim_dependency` gains the user/agent/import declarers, the declarer-scope trigger,
  and a cycle check that covers retracted edges.
- `job` gains `kind ∈ {extract, revalidate}` and target/identity triggers.

**Added tables:**
- Authoritative: `revalidation_request`, `revalidation_result`, `source_citation`,
  `session_read`, `model_judgment`, `relation`, `entity`, `identity_assertion`.
- Derived: `claim_validity`, `entity_resolution`, `mention_resolution`, `entity_alias`,
  `identity_blocked`.

**Procedure:**
- The migration runs with `foreign_keys=OFF` and `legacy_alter_table=ON` inside one transaction.
- `pragma_foreign_key_check` runs before commit, then enforcement is restored.
- A `<db>.pre-v3.<ULID>.bak` backup is written first.
- A newer schema is refused by older binaries.

**Evidence:**
- `phase3_database_migrates_losslessly_with_validity_and_backup`: a v2 database built
  from the original 0001/0002 SQL, with Claims, dependencies and lifecycle history,
  migrates with identical rows. Validity equals the oracle, the backup is v2, and
  integrity is ok.
- `rollback::failed_or_dangling_migration_rolls_back_and_restores_enforcement` (unit
  test): a failing or FK-violating migration rolls back completely and re-enables
  foreign keys.
- **Real phase-3 binary** (`reports/evidence/phase-4/real-phase3-binary-migration.txt`):
  - A database written by the binary built from tag `phase-3` contained 30 Sources
    (10 per recorder) with agent sessions, 5 lifecycle events and 30 backfilled jobs.
  - The phase-4 `migrate` left the row snapshot hash unchanged (`c7768eda1b1c55fa`
    before and after). `check` returned ok with the new derived items, and lifecycle
    commands still work.
  - The phase-3 binary then refuses the database ("schema too new: database 3, binary 2").
  - Claims cannot be produced by that binary without a paid model call. Claim and
    dependency migration is therefore covered by the SQL-built fixture above.
- SIGKILL during migration: §7.

## 3. Validity semantics and test evidence

Reason codes:
- **Invalid:** `retracted`, `forgotten`, `source_retracted`, `source_forgotten`,
  `premise_invalid`, `restates_invalid`, `alternatives_invalid`.
- **Needs revalidation:** `premise_uncertain`, `alternatives_uncertain`,
  `advisory_invalid`, `session_read_invalid`.

Tests in `tests/integrity.rs`:
- `dependency_semantics_lifecycles_and_revalidation_jobs` covers:
  - every dependency kind and Source versus Claim retraction;
  - forget and restore, and a new dependency on a retracted parent (immediately invalid);
  - an uncertain alternative, which keeps its dependent uncertain and never invalid;
  - self-dependency and cycle rejection;
  - dependency retraction and restore;
  - job coalescing, lease expiry with a fenced late completion, duplicate delivery
    returning the recorded result, and draining.
- `dependency_insertion_order_does_not_change_validity`: every insertion order and
  late-discovered edge gives the same states.
- `randomized_graph_operations_match_full_recomputation_oracle`: §6.
- `reextraction_keeps_retractions_validity_and_manual_identity`: re-extraction keeps
  lifecycle, validity and user identity corrections.
- `tests/extraction.rs::pagination_dependencies_and_stale_publication`: now asserts
  real validity. Retracting a premise makes the dependent invalid and hidden, and
  restoring it returns the dependent. This replaces the D-38 placeholder expectation.

Every reason code is asserted to occur at least once across the property-test seeds.

## 4. Identity and temporal behaviour

**Identity** (`identity_resolution_is_precedence_ordered_reversible_and_durable`):
- A shared alias never merges entities.
- Transitive `distinct_from` protection holds: a user merge plus a user `distinct_from`
  blocks an agent bridge.
- An agent `distinct_from` cannot split a user merge.
- Retracting a merge reverses it, and restoring it reapplies it.
- Agents cannot retract user assertions, and the database refuses `system` attempts.
- Model suggestions apply only under a permissive policy, and reopening with the
  default policy splits them again.
- Corrections survive restart, `integrity-rebuild`, and backup/restore.
- The user view resolves only user assertions over user-visible mentions.

**Temporal** (`as_of_reproduces_r2_history_examples_and_preserves_history`):
- The R2 C-case corpus gives 22/22 status answers for both user and agent views.
- The only freshness-flag difference is `q_meeting_now`, the limitation inherited from
  R2. The test asserts it explicitly instead of hiding it.
- Superseded Claims stay present as history.

**Temporal** (`temporal_unknowns_judgments_and_relation_authority`):
- An unknown `authored_at` uses `captured_at` only as an upper bound.
- `stale`, `age_unknown` and `unexecuted_decision` flags are asserted directly.
  `uncertain` is exercised by the R2 corpus test.
- Freshness thresholds are configurable: reopening with a 400-day window clears `stale`
  on the same Claims, and age stays unknown where only capture time is known.
- Model judgments are immutable, appear only as provisional `possibly_superseded_by`,
  and can be blocked per model.
- Agent relations are provisional and user relations are confirmed.
- `contradiction`/`not_conflicting` are rejected (Phase 7).
- `supersedes` never invalidates.

## 5. Authorization and projection verification

`tests/transports.rs::integrity_operations_are_equivalent_and_never_leak_across_projections`
runs over the real HTTP server and the stdio MCP bridge.

The fixture contains:
- a user Source with a premise chain;
- an agent-only Claim depending on a user Claim, carrying an agent mention and alias;
- a forgotten user Source whose dependent Claim is invalid;
- an agent citation, agent and user entities, an agent relation, and a model temporal judgment.

What the test checks:
- **Hidden versus absent:** for the user, every read operation (`claim_get` with and
  without `include_retracted`, `claim_dependencies`, `claim_support`, `claim_relations`,
  `entity_get`) on 7 hidden ids returns the same `not_found` error as an absent id.
  HTTP equals MCP for every probe.
- **Non-user roles:** for both independent agent credentials, the same holds for
  forgotten records on every read and every write (`identity_assert`, `relation_add`,
  `dependency_add`, `citation_add`, `claim_retract`).
- **No leakage in user reads:** the visible user reads (list, `as_of`, get,
  dependencies, support, relations, entity) are equal over HTTP and MCP. Their combined
  output contains none of the hidden ids, the mention id, the judgment id, the agent
  text or alias, the forgotten text, `declared_by` or `judgments`. This still holds
  after the user authors edges onto hidden Claims (D-30/D-103).
- **Counts:** user count is 2, support is `{sources:1, independence_unknown:0, claims:1}`,
  dependencies are 1 and relations are empty.
- **Model judgments stay non-authoritative:** the audit view lists the judgment but no
  relation created from it.
- **Agents cannot override user authority:**
  - an agent retracting a user dependency or a user Claim gets `forbidden`;
  - an agent declaring a dependency for a user child or citing from a user Source gets `forbidden`;
  - import identity, entity and relation writes get `forbidden`;
  - a `contradiction` relation is rejected, and so is a smuggled `actor` field.
- **Cursors:** a cursor issued to one agent credential is rejected for the user and
  for another agent, identically to garbage, for both `claim_list` and `claims_as_of`.
- **Mutation check:** removing the visibility filter on the other end of a dependency
  makes this test fail with "user output leaks a0".

Existing Phase 1–3 authorization, leakage and transport tests all pass. The tool count
expectation is now 24 and the schema expectation is 3.

## 6. Randomized property-test coverage and seeds

Each seed builds 8 Sources (random recorder, agent sessions) with a mixed random DAG
across all four kinds. It then runs 120 operations:
- Claim lifecycle (retract, forget, restore);
- Source lifecycle;
- dependency insertion in either direction, including cycle attempts;
- dependency retract/restore;
- agent session reads;
- historical imports through the approval boundary.

After every operation the materialized state must equal the oracle, and every cycle
attempt must be rejected. A failing seed is greedily minimized to a short reproducing
operation list and printed.

| Run | Seeds | Operations | deps_unknown rows seen | cycles rejected | imports |
|---|---|---|---|---|---|
| default (`cargo test`) | `0x9a4e0001`–`0x9a4e000c` (12) | 1,440 | 2,097 | 49 | 146 |
| extended (`PRIMERADIANT_PROPERTY_SEEDS=1000..1199`, release) | 212 | 25,440 | 45,842 | 934 | 2,493 |

Both runs passed with zero mismatches.

**Sensitivity:** two injected bugs were detected and minimized:
1. the closure skips session-read children;
2. wrong alternative sufficiency.

## 7. Crash-recovery evidence

**`tests/integrity_crash.rs`** (SIGKILL against a real subprocess):

`sigkill_during_migration_is_atomic`:
- Setup: a 6,000-Claim v2 fixture. Each round kills the migrating process after
  0/5/20/80/200/500 ms.
- Before any recovery the schema is wholly v2 (5 rounds) or wholly v3 (1 round), and
  every authoritative row count is unchanged.
- After reopening, integrity is ok, there are no validity mismatches, and counts are unchanged.

`sigkill_during_propagation_keeps_validity_consistent`:
- Setup: a 5,000-Claim premise chain whose root is retracted and restored in a loop.
  The process is killed at 6 jittered delays.
- Each time, a raw connection opened *before* `App::open` (no repair) finds
  `validity_mismatches_on` empty. Every committed lifecycle event already has its
  validity, so event and validity are atomic.
- Kill timing is not controlled precisely. Each round observed one acknowledged
  propagation, so most kills landed during an uncommitted propagation.

**Other recovery and concurrency coverage:**
- `concurrent_readers_never_observe_lifecycle_without_its_validity`: one writer
  toggles a hub with 400 premise children while 3 readers take 900 raw snapshots plus
  900 projection reads. In every snapshot, the hub being retracted implies that all
  children are invalid, and the user list shows the children only alongside the hub.
  Both states were observed.
- Lease expiry, fencing and duplicate delivery: §3.
- Restart, backup/restore durability of identity: §4.
- The existing Phase 1/3 crash tests (`tests/crash.rs`, the extraction SIGKILL worker)
  still pass.

## 8. 100,000-Claim benchmark

`cargo test --release --offline --test integrity_benchmark -- --ignored --nocapture` →
`reports/evidence/phase-4/benchmark.json`.

**Shape:**
- 100,000 Claims in 10,000 Sources (half user, half agent).
- 99,986 dependency edges: premise 31,304, restates 26,221, alternative 21,401,
  informed_by 21,060.
- A 5,000-child premise hub, 100 alternating premise/restates chains of 100, and a
  random DAG (0–2 parents within the previous 5,000 Claims, seed `0x9a4ebe0c`).

**Environment:**
- Apple M4 Pro (Mac16,8), 24 GiB, Darwin 27.2.0 arm64.
- rustc 1.96.0 release build, SQLite 3.50.2.
- WAL with `synchronous=FULL`, tempdir on local disk.

| Operation (median of 5) | Retract | Restore | Affected Claims |
|---|---|---|---|
| Claim: hub root | 59.8 ms | 59.1 ms | 5,001 |
| Claim: chain root | 1.5 ms | 1.5 ms | 100 |
| Claim: leaf | 0.3 ms | 0.3 ms | 1 |
| Source: hub's Source | 60.4 ms | 60.2 ms | 5,001 |
| Source: mid-graph | 1.4 ms | 1.3 ms | 22 |

| Full operations | Time |
|---|---|
| Full oracle recompute (100k) | 260 ms |
| Oracle compare with materialized table | 306 ms |
| `rebuild_derived` (validity + identity) | 603 ms |
| `App::open` with derived repair | 2.62 s |
| Synthetic generation (not product code) | 19.6 s |

**Correctness:** `validity_mismatches()` was empty after every measured operation, and
every restore returned the exact baseline.

**Storage:** the database plus WAL was 84.9 MB. Nine revalidation jobs were created,
all coalesced.

**Comparable before/after:** the first run of this same benchmark took 7.1 s per hub
toggle (about 1.4 ms per affected Claim). Sampling showed that `children()` scanned
every agent Source per closure node. Pinning the session-read join order (D-104) gave
the numbers above, an improvement of roughly 120× on the identical shape and machine.

**Limitation:** this graph has no `session_read` rows, so the session-read path is not
measured at scale.

## 9. Verification command outcomes

Run on the final tree:

| Command | Outcome |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --offline --all-targets -- -D warnings` | clean |
| `cargo test --offline --all-targets` | 62 passed, 0 failed, 5 ignored (see below) |
| `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'` | 40 tests OK |
| `cargo test --release --offline --test integrity_benchmark -- --ignored` | 1 passed (§8) |
| Extended property seeds (§6) | 1 passed |

**Passed tests by suite:** lib 7, authorization 5, crash 1, extraction 20, integrity 10,
integrity_crash 2, persistence 10, transports 7. The pre-Phase-4 baseline was 48
passing tests.

**The 5 ignored tests are not skipped tests:**
- 4 are subprocess workers driven by their parent crash tests: `tests/crash.rs`,
  `tests/extraction.rs`, and 2 in `tests/integrity_crash.rs`.
- 1 is the benchmark, which is run explicitly as shown above.

**Changes to existing tests:**
- Schema-version and tool-count expectations were updated for the new schema.
- One D-38 expectation was replaced with real validity assertions.
- A latent ordering flake in the identity test was fixed (entity ids created in the
  same millisecond were compared in creation order rather than id order); the
  assertions themselves are unchanged.

## 10. Remaining limitations

- **No semantic revalidation.** `semantic_pending` outcomes need a model and are left
  pending. Phase 4 calls no model.
- **Owner reach is administrative (D-30/D-103).** The user can author dependencies and
  relations onto, and run lifecycle commands against, records outside the user
  projection. This lets the local owner learn that an id exists, by design. User reads
  never reveal such records.
- **Real-binary migration evidence covers Sources, events and jobs only.** Claims are
  covered by an SQL fixture built from the original migration files, because publishing
  Claims with the phase-3 binary needs a paid model call.
- **Bounded crash and property testing.** Crash tests cover chosen kill points with
  jittered timing, not every instruction. Property tests use about 30–60 Claims per seed
  and do not prove correctness for all graphs.
- **Benchmark scope.** One machine and one shape. Session-read propagation was not
  benchmarked at scale. `claim_support` is computed per call, not materialized.
- **Inherited temporal gap.** The `q_meeting_now` freshness limitation remains, as noted in §4.
- **Out of scope.** Conflict relations (`contradiction`, `not_conflicting`) and
  possible-conflict projections are Phase 7. Retrieval is Phases 5–6.
