# Phase 3 focused continuation — 2026-10-23

**Phase 3: Implemented but unverified; semantic acceptance remains incomplete.**
No `phase-3` tag was created. Phase 1/2 tags and migrations are unchanged.

Starting HEAD: `5be9c669ddae79686346a12752afaa0887374c62`, branch `main`.
Ending production-code commit: `b32e19d56379d0372271ec24c5d51fe80cf38371`.
The subsequent evaluation/documentation commit containing this report is the final
continuation commit (`git log -1 --format=%H -- reports/phase-3-focused-completion.md`).
Pre-existing untracked `.yeet/` was left untouched.

## Complete — verified engineering work

- Fixed a real English-only boundary bypass: JSON `\ud55c` notation was ASCII on
  the wire and could carry Korean to DeepSeek. The guard now rejects raw Korean,
  escaped Korean in keys/values, duplicate-key masking and nested escape notation,
  in system and input payloads. Output uses the same conservative escape guard.
  Before: the old raw `is_ascii()` predicate admitted these encoded inputs.
  After: regression tests return `LanguageRestriction` before network/budget access.
  ASCII remains a script guard, not proof that a sentence is English.
- Added an intact 256 KiB combined system/input limit before network/reservation.
  Oversized requests return `InvalidRequest`, without truncation, partial extraction
  or claimed coverage. Boundary-size requests reach the unchanged spending guard.
  Sources remain immutable; durable job failure/retry behavior is unchanged.
- Added negative regression cases for model-supplied existing/invented citation IDs
  and cross-source anchor fields. They cannot create authority through schema input.
- Prepared a frozen balanced challenge set, source/reference/corpus hashes,
  evaluation-only source-grounded protocol, offline archive runner and judgment
  validator. Tests require exact Claim identity, original-source scalar spans, all
  semantic measurements, inventory coverage and unresolved contested judgments.
  The runner has **no paid dispatch path**. A future guarded dispatcher still needs
  implementation/review after authorization; this is not a claim of live readiness.

## Frozen 12-case selection

Selection used Source/category/reference files, not passing outputs. The protocol
and manifest were written before inspecting new focused packets. All examples are
previously exposed editorial material, not fresh independent gold. Manifest SHA256:
`164f84834501955b62359552ac192fb6f92fccb44d5a0424a63f8bcdaf8004a7`.

| Case | Language | Selection reason | Verified v23 packet |
|---|---|---|---|
| new-003 | Korean | Negated conjunction scope | Missing |
| new-007 | Korean | Question versus factual assertion | Missing |
| new-018 | Korean | Conflicting correction, not silent invalidation | Missing |
| r1-nested_quote | Korean | Nested speaker attribution, disagreement, group identity | Yes |
| r1-ambiguous_ref | Korean | Unknown subject and known reviewer instability | Yes |
| r1-hypothetical | Korean | Negated threshold condition and unknown experiment | Yes |
| r1-habitual_temporal | Korean | Omitted subject, original-time habitual scope | Yes |
| v23-fresh-003 | Korean | Uncertainty, quoted nonverification, incomplete arguments | Yes |
| new-025 | Mixed | Independent technical assertions and known language drift | Missing |
| v23-fresh-004 | Mixed | Technical count versus inference limitation | Yes |
| new-042 | English | Reported uncertainty and narrator/subject perspective | Missing |
| v23-fresh-002 | English | Incomplete event target and attribution | Yes |

The three frozen repeat cases are `r1-ambiguous_ref`, `r1-nested_quote`, and
`v23-fresh-003`. All repeats are **Blocked**, not executed. Maximum intended workload
remains 12 primary + 3 independent repeats, with no retry-to-pass policy.

## Evidence and uncertainty

The focused runner replayed 546 archived rows, including 21 v23 rows, with their
archive hashes and line numbers. These are historical attempts, not independent
trials. It verifies original Source/hash and authored time and uses the Rust frozen
Plan/prompt revalidator. It chooses initial-named archive first, then path/run/attempt,
without filtering for success. All other attempts remain in the replay evidence.
Older profiles are never relabeled v23. Five missing v23 cases remain explicit gaps;
no extraction calls were made to fill them.

- **New valid Haiku judgments: 0.** Prepared primary packets: 7/12.
- Prepared Korean coverage: 5/8 Sources, 7 Claims; mixed: 1/2 Sources, 2 Claims;
  English: 1/2 Sources, 2 Claims. Total: 11 Claim texts.
- All seven packets pass structural checks; only three meet the archived experimental
  review protocol. Four ineligible batches are **not successful extractions**.
- Assistant editorial original-source inspection (not independent human adjudication
  or API model judgments): five batches have no identified defect; two remain
  unresolved. Eight Claim atomicity assessments pass editorial inspection and three
  are unresolved. Korean-only: three batches editorial pass, two unresolved; four
  Claim atomicity assessments pass, three unresolved. Eleven editorial reference
  propositions are represented, but this is not accepted valid-Claim coverage.
- New production acceptance/rejection counts: **0 / 0**. No publication was attempted.
  Accepted semantic Claim coverage, live error rates and repeated-review reliability
  remain **unknown**, not zero. No universal quality inference follows.

Structured per-Claim editorial judgments include exact Source evidence and flags in
`focused-continuation/editorial-inspection.jsonl`. In the selected initial unknown-
person proposal, “지칭하며” may be a reference-binding qualifier or a separately
asserted naming act. In the quoted-delay proposal, “말했으며 … 못했다고도 말했다”
may be two reports or one explicitly unverified possibility. Both stay unresolved;
the editorial inventory and a source-grounded model pass do not settle atomicity.
No new definite semantic defect rate is claimed from these contested cases.

### Reviewer disagreement mechanism

`reviewer-disagreement.json` pins the two exact lines of
`v23-user-unknown-continuation.jsonl.gz`, source/prompt/archive hashes, Claim index 1,
identical Claim text and both review results. The standalone reader changed from
`reject/ambiguity` to `pass`; source-grounded atomicity passed both times. The anchor
broadened on retry, but the isolated reader never receives the anchor, so this is
**not semantic repair**. Identical text and prompt yielding different verdicts is
observed nondeterminism/instruction adherence instability. The old issue-only
responses cannot establish the model's precise internal cause.

Missing context is intentional for intrinsic readability, but makes that reviewer
unsuitable for source entailment/coverage. Existing v23 instructions already permit
explicit unknown identities; another arbitrary retry would not repair this. The new
evaluation protocol separates responsibilities, requires exact evidence and competing
interpretations, and preserves disagreements instead of majority approval. Its effect
on actual model reliability is **Implemented but unverified**. Production v23 prompts
and all archived false-acceptance/false-rejection evidence remain unchanged.

## Korean architecture, provenance and approval

**Implemented but unverified:** the existing frozen Plan explicitly names extractor
and reviewer models. Direct Korean-capable processing is the preferred investigation
over adding an unsupported English translation step: it preserves access to original
Korean evidence, but its semantic reliability is not established. No automatic route
selection or production Haiku replacement was added. The configured DeepSeek English-
only path fails closed for Korean (including escaped forms), with no fallback or
translation. An explicitly configured Korean-capable model still requires provider
availability, budget, all validation and exact-hash local user review. A supported
API language capability is not semantic acceptance or proof of lossless processing.

**Deferred:** cross-source citation verification. Current contracts place
`source_citation` in Phase 4; Phase 3 has no trusted cited-span/semantic-support
authority model. Existence of a user Source alone cannot establish support or authorize
an agent statement. No identifier-based authority shortcut was introduced. Non-user
Sources with `speaker=user` continue to fail unresolved. Full existence, recorder,
span, lifecycle and semantic-support verification requires a reviewed schema/authority
extension rather than accepting arbitrary identifiers.

**Complete (regression verified):** source immutability, original linkage, exact-hash
candidate approval, user-only approval/confirmation, lifecycle checks, transactional
publication, re-extraction and idempotency. No model judgment publishes memory.
Claim pagination still filters the materialized visible set before totals/pages and
uses credential/filter/resource-bound authenticated keyset cursors. Existing isolation,
stale-publication and pagination tests pass; no correctness defect justified rewriting
it. Cross-batch dependency semantics, contradiction invalidation and endorsement-history
logic remain **Deferred** to Phase 4. No speculative relationships were added.

## Blocked — paid evaluation and production promotion

Two unauthenticated public GETs checked the OpenRouter catalog and requested model's
endpoint catalog. The immutable snapshot is `model-availability.json`.
`anthropic/claude-haiku-5.5` was listed: text/image/file input, text output, 1M context,
128K maximum completion. Catalog supports reasoning with efforts including `low`;
the frozen evaluation intends `low` with 2,400 maximum output tokens. Flat advertised
base rates are $0.10 input / $0.50 output per million tokens, or $0.11/$0.55 on listed
regional endpoints. Cache fees, web-search fees and long-context price overrides are
also present. These are public advertised prices, **not a billed execution quote**.

The current adapter deliberately rejects unsupported price components/tier overrides.
It also sends temperature=0, which is not in this model's advertised parameter list.
Thus endpoint support is an additional implementation blocker, even after funding:
do not bypass conservative pricing or `require_parameters` to force a call. No live
capability, reasoning behavior or Korean quality was tested.

Confirmed new paid OpenRouter requests/tokens/cost: **0 / 0 / $0.00**.
Public metadata requests: 2; no authenticated provider request. Read-only ledger
inspection before and after found the unchanged $4.50 ceiling, conservative prior
**$4.78265335**, and zero request rows. Historical cost is account-wide, not proven
task-attributed. Spendable balance: **$0.00**. Arithmetic room under the original
strict $5 ceiling is $0.21734665, but is not authorized spendable headroom.

Exactly what is needed: explicit authorization to revise the existing $4.50 budget
policy **in place**, retaining the same ledger and all history (and authorizing a
provider-cap change only if necessary), while remaining strictly below $5 cumulative,
at most $0.10 additional, and leaving a conservative reservation margin. None was
granted or performed here. Pricing/reasoning-compatible guarded dispatch must then be
reviewed, with immutable request-level usage, before any call. No new account, key,
ledger, credits or alternate billing route was created. Public availability does not
override the funding prohibition.

## Verification and reproducibility

All passed after the final code changes:

```sh
cargo fmt --check
cargo test --offline --all-targets
cargo clippy --offline --all-targets -- -D warnings
python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'
cargo build --offline --example revalidate_extraction
python3 evaluation/phase3/focused_review.py --output NEW_FOCUSED_DIRECTORY
python3 evaluation/phase3/replay_archives.py --output NEW_REPLAY_DIRECTORY
cargo test --offline --test extraction schema_language_anchors_and_dependencies_fail_closed
cargo test --offline --test extraction agent_metadata_never_promotes_and_confirmations_are_user_only
cargo test --offline --test extraction pagination_dependencies_and_stale_publication
```

Rust: **44 passed**, two ignored child-process helpers exercised by parent tests.
Python: **5 passed**. Full offline archive replay is unchanged from the previous
checkpoint: 5,281 rows, 4,752 distinct payloads, 145 Source hashes; 3,646 structural
passes, 4,502 valid frozen plan hashes, 2,624 experimental-gate-eligible payloads.
These are deterministic structural/protocol results, not semantic success rates.
Evidence and command logs: `reports/evidence/phase-3/focused-continuation/`.

Remaining acceptance gaps: five current-profile packets, all new Haiku judgments and
three repeats, independent adjudication, contested atomicity, demonstrated reviewer
stability, accepted Korean processing, and safe future citation verification. Reliable
chunking is **Deferred**; oversized requests fail intact. Phase 3 is not complete.
