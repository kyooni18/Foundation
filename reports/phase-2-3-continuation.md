# Phase 2 / Phase 3 continuation — 2026-10-23

Phase 2 remains complete. Phase 3 remains an implemented, quarantined extraction
pipeline **without semantic acceptance or a phase-3 tag**. This continuation closes
production spending and user-attribution gaps; it does not relabel historical model
agreement as semantic proof.

## Initial state and preserved boundaries

Initial HEAD was `a7c1a2d`, not `50dbccb`. Phase 1 (`50dbccb`) and Phase 2
(`ec11625`) already had tags. The tree contained prompts through opt-in v23, a durable
extraction worker, candidate review/approval, Claim persistence/projections, extensive
Korean fixtures and saved experiments. Only pre-existing `.yeet/` was untracked.
Foundation recalled this context; no Jev tools were available.

Baseline verification passed 40 Rust tests. Existing Phase 2 coverage includes
trusted credential-derived authorship, loopback HTTP, stdio MCP forwarding to the
same dispatcher, projection filtering in SQLite, structured errors, bounded pages,
credential-bound cursors, negative authorization, repeated/concurrent ingestion,
restart/crash recovery, immutable original bytes, and migration compatibility.
The original migrations and Phase 1/2 tag targets are unchanged. No production
Source database was opened or modified by this continuation.

## Implemented changes

- `87da869`: mandatory shared SQLite budget in the production OpenRouter adapter;
  fresh endpoint pricing, conservative atomic reservations, bounded output, four
  outstanding request slots, provider price constraints, provider-key cap verification,
  actual-cost settlement before output validation, and persistent uncertain charges.
  Requires a non-resetting key cap at or below the configured ceiling, never raises
  one. The configured ceiling cannot exceed $4.50. Both evaluation paths now use
  this adapter. No new dependencies or memory-schema migrations.
- Explicit general-purpose default `deepseek/deepseek-v4.1-flash-1017`, with no
  implicit replacement. Non-ASCII decoded output is rejected in addition to input.
  ASCII checking is not a complete English-language detector. Korean inputs cannot
  enter this boundary; unvalidated language conversion is not introduced.
- `fe5e6b9` and `669a044`: source-checks-v3 rejects `speaker=user` proposals
  from agent/import Sources without verified citations. Phase 3 has no cross-source
  citation verification, so these stay unresolved, even with passing model reviews.
  Agent assertions remain possible with agent speaker authority. Existing published
  immutable history is not rewritten. Tests retain user-only confirmation and
  transport isolation coverage with correctly attributed agent fixtures.

The architecture still separates source ingestion, durable job leasing, candidate
extraction, decomposition/reference prompts, structural/Unicode anchor validation,
standalone and source-grounded atomicity/fidelity/coverage reviews, exact-hash local
user approval, and transactional publication. Failures preserve the Source; approval
cannot confer recorder authority or truth endorsement. No automatic contradiction
invalidation, synthetic user identifiers, or later-phase features were added.

## Verification and quality limits

Final full suite: **42 Rust tests passed, zero failures**, plus two ignored helper
entry points exercised by parent crash tests. Three Python tests passed. Formatting
and Clippy passed. Two transient test runs failed because the new concurrency fixture
still expected one successful $0.10 reservation after the slot limit became four;
changing the fixture to a $0.50 reservation tested the intended ceiling correctly.
No production semantic prompt was changed to make a test pass.

Commands:

```text
cargo fmt --check
cargo test --offline --all-targets
cargo clippy --offline --all-targets -- -D warnings
python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'
cargo run --offline --example revalidate_extraction -- --input INPUT --output OUTPUT
cargo test --offline --test extraction agent_metadata_never_promotes_and_confirmations_are_user_only
```

Logs and replay artifacts: `reports/evidence/phase-3/runtime-budget-20261023/`.
The complete Rust suite covers the implementation through `fe5e6b9`; the subsequent
failure-label-only change has its focused regression check. The original baseline
also passed all tests before any edits.

Offline replay of **29 saved v23 rows**: 29 valid plan hashes, 28 structurally valid
rows, 17 experimental-gate-eligible rows, one provider/schema failure. These include
baseline and intermediate attempts: **17/29 is not a semantic success rate**.
No new candidate was accepted into a production database. Existing `curated.json`
and `legacy.json` together contain 96 cases (88 Korean, 3 mixed, 5 English), with
additional targeted and held-out sets. References remain editorial except where
explicit user adjudications are recorded.

Historical v23 assistant inspection of four targeted fresh final proposals found
seven represented reference propositions and no known combined independent assertions,
unsupported facts, semantic/anchor/language errors or duplicates. Korean coverage in
that subset is only two Sources / three Claims. Three of four R2 baseline outputs
omitted the explicitly unknown affected target. This small selected subset does not
establish final-profile recall, rejection rate, negation/condition/attribution
accuracy, entity-resolution accuracy, or full-corpus Korean fidelity. Those rates
remain **unverified**, not zero. No translation-quality result is inferred.

Known problems remain: unstable standalone reviewers, false rejection of qualified
questions/disjunctions, earlier false acceptance of attribution shifts and missing
unknown targets, incomplete broad v23 reruns, incomplete repeated held-out comparison,
and absent independent whole-corpus adjudication. Full semantic atomicity cannot be
proved by syntax checks or model agreement. All model proposals remain quarantined;
we do not claim success by accepting nothing. Phase 3 therefore remains incomplete.

## Spending and operational state

This continuation executed **zero billed model requests**, zero billed tokens, and
**$0.00 confirmed new OpenRouter spending**; no uncertain new charges exist. Only
public documentation/pricing reads were made. No extraction/translation/embedding/
judge model was called. Loopback provider tests use fictional prices and credentials.

The pre-existing final checkpoint records a **$4.78265335 account-wide usage delta**
under an earlier $5 allowance. It can include unrelated activity and is not confirmed
request-attributed task spending. Its historical model evaluations include
`openai/gpt-6-luna-pro`; earlier models/results remain in `reports/phase-3.md`.
This continuation neither resets that allowance nor claims the earlier spending met
the newly specified $4.50 operational ceiling.

The private persistent ledger `.local/openrouter-phase2-phase3.sqlite` carries
that entire conservative historical amount against the stricter $4.50 ceiling and
contains zero new request rows. **Spendable remaining budget is $0.00**. The arithmetic
remainder under the earlier absolute $5 ceiling is $0.21734665, but it is not available
for further calls under the current $4.50 guard. No API key/credit limit was changed,
no credits purchased, and no paid requests are running or scheduled. Request-level
historical actual versus unaccounted spending cannot be reconstructed from the
account-wide delta alone; no exact cumulative-cost claim is made.

Public protocol sources used for the guard:
[OpenRouter key limits](https://openrouter.ai/docs/api/reference/limits),
[provider routing and price ceilings](https://openrouter.ai/docs/guides/routing/provider-selection),
and the public model endpoint pricing catalog. Live provider enforcement and cost
reporting remain unverified by this continuation; deterministic loopback checks pass.

## Files and remaining work

Changed implementation: `src/budget.rs`, `src/providers.rs`, `src/lib.rs`,
`src/bin/primeradiant.rs`, `src/extraction/mod.rs`; focused tests in
`tests/extraction.rs` and `tests/transports.rs`; evaluation adapter/harness files in
`examples/` and `evaluation/phase3/`; README, decisions, this report and evidence.
Private accounting is ignored; existing `.yeet/` is untouched. No completion tag was
created or moved. Phase 3 needs sufficiently broad source-level semantic validation,
a safe validated Korean path under the requested model policy, and resolution of
reviewer reliability limits before completion. No additional funds are assumed.
