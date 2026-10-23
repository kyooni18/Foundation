# Phase 3 guarded focused dispatcher — offline follow-up

Overall task started at `5be9c66`; this follow-up started at `2c6a7f6`.
The commit containing this report is its ending checkpoint (`git log -1 --format=%H
-- reports/phase-3-guarded-dispatcher.md`). Earlier reports/evidence remain unchanged.
**Phase 3: Implemented but unverified. No completion tag.**

## Complete — offline-tested evaluation dispatcher

`evaluation/phase3/guarded_haiku.py` is an evaluation-only dispatcher, not a production
provider replacement. It uses the **existing operational ledger/schema**, not a new
billing path, and does not create ledgers, change ceilings, provision keys or publish
Claims. Its CLI fixes the ledger path to `.local/openrouter-phase2-phase3.sqlite`.
Temporary fixture ledgers and injected transports exist only in unit tests.

- Rechecks the frozen manifest/protocol, pinned seven-packet artifact and archived
  provenance hashes. No historical profiles are relabeled. Five missing outputs are
  still missing; no extraction path was added.
- Fetches fresh catalog and endpoint metadata before an authorized request. Requires
  advertised low reasoning, reasoning/max_tokens/response_format support and context
  capacity. Sends `reasoning.effort=low`, `max_tokens=2400`, no temperature, no tools
  or search plugins, one pinned endpoint and no fallback.
- Prices with Decimal, upward nano-dollar rounding, the maximum input/cache-write
  and output rate across **all** advertised long-context tiers, one token per UTF-8
  byte plus 4096 envelope tokens, and a factor-of-two reservation margin. Unknown
  nonzero charges fail closed. No discount is credited. Search fees are inapplicable
  because this payload supplies neither search tools nor search plugins.
- Checks the unchanged $4.50 local ceiling, provider non-resetting cap, global pending
  reservations/overruns and a persistent $0.10 continuation ceiling. Uses the same
  SQLite `BEGIN IMMEDIATE` reservation transaction and schema as Rust. Unknown costs
  keep their money and slot reserved. Settled overruns stop later dispatch.
- Uses deterministic case/primary-or-repeat request IDs: no retry-to-pass, duplicate
  dispatch or additional repeats. Maximum remains 12 primary + three designated
  repeats, with only seven primaries currently available. Conservative pricing may
  stop a run before that workload; never weaken reservations to finish it.
- Creates/fsyncs exclusive request, raw response, validated judgment and comparison
  files. A request record and committed reservation precede POST. Usage settlement
  happens before semantic/schema validation; truncated/invalid judgments still cost
  money and cannot become accepted results. Partial failures preserve evidence.

Authorization is an explicit operator-supplied record referencing the user's grant;
the dispatcher cannot authenticate a conversation or grant itself permission. No such
record was created for this task. Even a record cannot override the hard ceiling.
Any future authorized policy amendment must update the existing budget guards
coherently **in place**; this implementation deliberately does not implement a cap
increase. The generic Rust provider continues to reject unsupported tiered pricing;
the new pricing support is confined to this dedicated Haiku evaluation path.

## Complete — deterministic disagreement containment

Regression tests load the actual saved identical-text verdict flip and demonstrate
that **both** old issue-only responses fail the new source-evidence judgment schema.
The dispatcher supplies the entire original Source and exact candidate, not an isolated
Claim or historical reviewer verdict. Repeat payloads are identical and do not include
the previous judgment. The frozen protocol distinguishes intrinsic readability from
source entailment and allows explicitly bound unknown identities.

The comparison implementation preserves per-Claim dimension differences (including
different reasons), evidence/uncertainty changes and inventory differences; it never
votes. Tests demonstrate that two matching responses cannot override one differing
response, known historical disagreement or an editorial contest. The previously
contested `r1-ambiguous_ref` and `v23-fresh-003` remain unresolved. Even uncontested
agreement is labeled `model_judgments_only`, with semantic acceptance unknown and
production approval false.

**Implemented but unverified:** whether this protocol actually reduces model
nondeterminism or produces correct atomicity judgments. Synthetic responses test
control flow, not Korean semantics. Rejecting an old underspecified schema is not
proof that a new model judgment is correct. Contested atomicity cannot honestly be
resolved by a deterministic validator; source-grounded independent judgments and
adjudication remain necessary. No new semantic success or reliability rate is claimed.

## Verification

All required commands passed after the implementation:

```sh
cargo fmt --check
cargo test --offline --all-targets
cargo clippy --offline --all-targets -- -D warnings
python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'
python3 evaluation/phase3/guarded_haiku.py  # offline summary only
```

**44 Rust tests, 18 Python tests passed.** Eleven new Python tests cover catalog/tier
pricing, parameter payloads, immutable evidence, budget/authorization failure,
timeout holds, malformed usage, truncated judgments, actual overruns, independent
repeat accounting, frozen-packet rejection and disagreement containment. No test
uses live credentials/network. Existing focused metrics tests still exercise the
unchanged before/after regression evidence; structural replay is not semantic proof.

## Blocked — actual evaluation

**New paid requests: 0. New spend: $0.00. New valid Haiku judgments: 0.**
The real ledger remains unchanged at **$4.50 ceiling**, **$4.78265335 conservative
historical account-wide spend**, **$0.00 authorized headroom**. No additional metadata
requests were made during this follow-up; pricing tests reuse the immutable snapshot.
Paid dispatch needs explicit user authorization for an in-place policy amendment,
retaining all accounting, allowing at most $0.10 additional and strictly less than
$5 cumulative with conservative margin. The authorization document alone cannot
change the ceiling. Do not run `--execute` without that authorization and reviewed
policy amendment.

Unchanged gaps: five current-profile outputs; all 12 intended primary and three
repeat live judgments; independently adjudicated semantic coverage; reviewer
reliability and Korean-route acceptance. Cross-source authority schema and reliable
chunking remain **Deferred**. Source immutability, original Korean, DeepSeek's language
boundary and user-only exact-hash publication remain intact.
