# Phase 3 focused verification addendum

This supplements `phase-3-focused-completion.md`; it does not replace or modify
existing evaluation evidence. Continuation started at `5be9c66`; production changes
ended at `b32e19d`. This follow-up starts at `e569920` and adds offline measurement,
regression tests and clarification. Phase 3 remains **Implemented but unverified**.
The commit containing this addendum is the final implementation checkpoint for this
follow-up (`git log -1 --format=%H -- reports/phase-3-focused-verification.md`).

## Complete — investigate the apparent failed audited shard

`completed shard=0 variant=audited exit=1` came from
`test_failed_shard_stops_pending_dispatch_and_preserves_incomplete_manifest`, not a
real experiment. The test explicitly mocks `subprocess.run.returncode = 1` and
expects `SystemExit("Incomplete experiment; inspect retained shard logs")`.
It asserts one dispatch, one failed shard, three skipped shards and no results file.
No model request, budget reservation or actual shard subprocess was executed.

The test now captures and checks that diagnostic and the exact failure category,
so a deliberate failure-path fixture no longer looks like an unexpected experiment
failure on test stdout. The expected refusal is preserved, not suppressed in the
production runner. There was no actionable failed live/offline extraction shard to
retry. The Python suite passes all **7 tests**, including this negative-path test.

## Complete — executed before/after offline semantic regression measurement

The actual starting commit `5be9c669ddae79686346a12752afaa0887374c62` was exported
with `git archive` into a temporary directory and its Rust revalidator was compiled
and executed on the seven frozen selected candidates. The current revalidator was
then rebuilt after `cargo clean -p prime_radiant` and run on the identical input.
The two output files have the same SHA256:
`1951ed1f061c3928fe880d84aba2cf9bd73234d3455286c3ecd519c11ec388f9`.
This demonstrates unchanged structural/protocol behavior on these exact candidates,
not an improvement in model semantics. No extraction text or historical verdict was
changed, and no new semantic model was run.

`focused_metrics.py` verifies Source, raw candidate, frozen Plan and prompt identity,
then applies the same saved assistant editorial source inspections retrospectively
to both runs. New semantic-regression tests check all available packets and the
Korean breakdown, enforce zero contribution from rejected batches, and prevent
contested Claims from becoming valid merely because a model gate passes. Core
atomicity/entailment/unsupported-assertion checks cannot be marked not_applicable.

| Measurement | Before | After | Korean before / after |
|---|---:|---:|---:|
| Selected Sources | 12 | 12 | 8 / 8 |
| Available verified v23 Sources | 7 | 7 | 5 / 5 |
| Missing current-profile Sources | 5 | 5 | 3 / 3 |
| Generated Claims | 11 | 11 | 7 / 7 |
| Experimental protocol accepts | 3 | 3 | 1 / 1 |
| Protocol rejected or incomplete batches | 4 | 4 | 4 / 4 |
| Editorial pass / fail / unresolved batches | 5 / 0 / 2 | 5 / 0 / 2 | 3 / 0 / 2, unchanged |
| Protocol-accepted Claims passing editorial checks | 5 | 5 | 1 / 1 |
| Covered reference propositions in eligible, editorial-pass batches | 5 of 11 available | 5 of 11 available | 1 of 7 available, unchanged |
| Published production Claims | 0 | 0 | 0 / 0 |

The four protocol-ineligible batches comprise **three recorded atomicity rejections
and one incomplete quota-blocked batch**, all Korean. A reviewer rejection is not
independent proof of a semantic error. The quota failure is not semantic rejection.
The two other languages each have one available two-Claim batch, protocol-eligible
and editorial-pass, covering two of two available reference propositions each.

The 5/11 and 1/7 measurements are explicitly **editorial-plus-protocol end-to-end
coverage on available archived candidates**; rejected/incomplete batches contribute
zero. They are not production approval, independently validated Claim coverage, or
a 12-case model success rate. Full selected-set semantic coverage and independently
adjudicated valid Claim coverage remain **unknown** because five outputs and
independent adjudication are missing. Reviewer repair effectiveness remains **unknown**:
the changed evidence-grounded reviewer protocol has not received live evaluation.
The same-text disagreement stays unresolved. Before/after semantic improvement is
not claimed; only deterministic provider safeguards were repaired and demonstrated.

Reproduction (use new output paths; evidence files are never overwritten):

```sh
# Export baseline to a new temporary directory and run its revalidator.
BASELINE=$(mktemp -d)
git archive 5be9c669ddae79686346a12752afaa0887374c62 | tar -x -C "$BASELINE"
cargo run --offline --manifest-path "$BASELINE/Cargo.toml" \
  --target-dir "$PWD/target/baseline-reproduction" --example revalidate_extraction -- \
  --input "$PWD/reports/evidence/phase-3/focused-verification/selected-input.jsonl" \
  --output "$PWD/baseline-new.jsonl"
cargo run --offline --example revalidate_extraction -- \
  --input reports/evidence/phase-3/focused-verification/selected-input.jsonl \
  --output current-new.jsonl
python3 evaluation/phase3/focused_metrics.py \
  --baseline baseline-new.jsonl --current current-new.jsonl --output metrics-new.json
python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py' -v
```

Saved measurements and hashed inputs:
`reports/evidence/phase-3/focused-verification/clean-before-after-metrics.json`.
The previous full offline replay remains 4,752 distinct payloads with unchanged
structural/protocol counts. None of these replays contacts a model provider.

## Blocked — budget, with reproducible accounting

`budget-readonly-audit.json` contains exact SELECT queries and results from the
existing `.local/openrouter-phase2-phase3.sqlite`, opened with SQLite `mode=ro`,
`PRAGMA query_only=ON` and a read transaction. Results, in integer nano-USD:

- Ceiling: 4,500,000,000 = **$4.50**.
- Conservative historical account delta: 4,782,653,350 = **$4.78265335**.
- Request rows, settled actual costs, pending reservations and unsettled count:
  **0 / 0 / 0 / 0**.
- Ledger SHA256 before and after inspection:
  `36c1b1413343da0f15f62e9171360358898d8fd0d65dbcbc9f868c465d1af260`.
- Confirmed new paid requests/tokens/spending: **0 / 0 / $0.00**.
- Spendable headroom: **$0.00**. The $0.21734665 arithmetic remainder below $5 is
  not authorized headroom under the unchanged $4.50 guard.

The prior remains account-wide conservative accounting, not reconstructed task-level
usage. The zero request rows do not mean zero historical spending; that spending is
carried in `prior`. No key, account, cap, ledger or historical evidence was changed.

Exactly what must be authorized before paid work: an explicit **in-place revision of
the existing $4.50 ceiling and its hardcoded policy**, preserving all prior accounting
and request rows, with at most $0.10 additional continuation spending, strictly below
$5 cumulative, and conservative reservations/uncertain charges included. A provider
cap amendment would require explicit authorization as well if necessary. There is
**no such authorization now**, so paid access remains blocked. Even with permission,
Haiku's tiered pricing and reasoning/parameter-compatible guarded adapter must be
implemented and verified; funding permission alone does not make it safe to dispatch.

## Final committed-tree verification

Required commands are rerun **after committing this follow-up**, with each actual
process exit code and tested HEAD recorded in an ignored local operational receipt
under `.local/focused-final-verification/`, and their results reported in the final
response. No subsequent tracked edit is made after that verification. Committed log
filenames are not used as proof of execution. The commands are:

```sh
cargo fmt --check
cargo test --offline --all-targets
cargo clippy --offline --all-targets -- -D warnings
python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'
```

**Complete:** deterministic guard fixes, negative tests, archive replay and honest
before/after measurement. **Implemented but unverified:** reviewer repair effect and
Korean semantic route acceptance. **Blocked:** new paid judgments/repeats and missing
current-profile extraction calls. **Deferred:** cross-source authority schema and
reliable chunking. Phase 3 semantic acceptance and a `phase-3` tag remain withheld.
