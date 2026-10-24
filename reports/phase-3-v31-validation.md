# Phase 3 authorized continuation — incomplete, 2026-10-24

## Checkpoint and decision

Started at `70d5166`. Budget amendment: `1324ca8`; implementation checkpoint:
`5b9b993`. Ending evidence/report commit is the commit containing this document.
The human's **“Added credits - proceed.”** approved the existing up-to-$0.05
proposal. Its guarded SQL updated the existing ledger, preserving all 40 prior
request rows and the carried prior. No historical amount was repriced or reset.

Three v30 priority batches now have complete passing diagnostic traces, replayed
through the actual Rust gate with valid reconstructed Plan hashes. That is a
substantive improvement over the previous checkpoint's zero complete passing
traces. **Full Phase 3 semantic acceptance is still not established.** The other
three priority cases remain ineligible, and the v31 reviewer experiment still
misreads conservative arguments and emits contradictory judgments. Neither profile
is promoted. No Claim was approved or published; no `phase-3` tag was created.

Evidence: `reports/evidence/phase-3/v30-authorized/`. Original Sources, frozen
reference labels, earlier prompts and raw results remain unchanged. The five
missing current-profile cases remain unassessed, not passed. The six priority
Sources are development regressions after prompt tuning, not fresh holdouts.

## Investigation and changes

1. V30's first nested-source review returned `reject` with empty Claim/inventory
   arrays, despite two supplied Claims. Existing validation correctly rejected it.
   One bounded identical-input repeat assessed both Claims and passed. The primary
   failure remains in the denominator; the repeat is not substituted into a
   successful primary trace.
2. The diagnostic replay had assumed every saved review had a valid outcome
   sidecar. It now verifies protocol-invalid rejection artifacts against the raw
   response and exact failure reason, retaining them in repeat comparisons. It
   still fails on missing evidence or unexpected replay drift.
3. Complete-trace replay now supports v30/v31 as well as v29 and preserves failed
   source-review prefixes. It reconstructs configuration from exact saved requests;
   this is diagnostic provenance, not a previously quarantined production job.
4. V30 delay extraction separates the conjecture from lack of verification and
   retains the unspecified affected target in both. Its reviewer nevertheless
   treated Korean object ellipsis as unsupported. V31 explains licensed resolution
   to a sole salient conjecture, distinguishing that proposition from its unknown
   affected entity. The actual v31 review resolved the object, but emitted `pass`
   plus uncertainty flags. Validation rejected that contradiction without repair.
5. V30's hypothetical uses a phrase that can mark the experiment's *target*
   unknown instead of identifying an unnamed experiment. V31 frames distinguish
   entity identity from purpose/target. New extraction marks experiment identity
   unknown, preserving the conditional plan. Its source reviewer misreads the
   identity phrase as an independently asserted unknown *speaker* and rejects it.
   This is evidence against promoting the experiment, not a repaired reliability
   claim.
6. V31's diagnostic schema requires at least one Claim/inventory row when reviewing
   a nonempty batch, using the API-supported `minItems: 1`. Valid empty batches
   retain their empty-array path. Exact count/identity and semantic validation
   remain necessary; a nonempty array alone proves neither coverage nor truth.

V31 remains an opt-in unsuccessful reliability experiment. Earlier prompt assets
are frozen. There is no case-specific rewriting of generated Claims, invented
agreement, new dependency, schema migration or Phase 4 feature.

## All six v30 priority cases

| Source | Core Source propositions / generated Claims | Actual review result |
| --- | --- | --- |
| Nested report | 2 / 2 | Correct full report chain, narrator-bound possessive in both Claims and separate disagreement on editorial inspection. Primary review protocol-invalid; bounded repeat passes. No complete trace. |
| Ambiguous reference | 2 / 2 | Unknown late third person and independently targeted-as-unknown irritation. No invented lateness cause. Two readers plus three Source reviews pass; Rust gate eligible. |
| Delay conjecture / verification | 2 / 2 | Separate attributed propositions, conjecture and unknown affected target retained. Two readers pass; first Source review unresolved over elliptical verification object. Stops there. |
| Hypothetical | 1 / 1 | Condition, threshold and intention preserved; unknown-argument wording questionable. Reader and first Source review pass; fidelity review rejects. Stops there. |
| Recent habit | 1 / 1 | Recent ongoing daily 6 a.m. habit anchored to original statement; narrator role separate from authenticated user. Reader plus three Source reviews pass; Rust gate eligible. |
| Mixed Korean/English | 2 / 2 | Worker-count fact and limitation separated. English Claim names Mori/worker_count/4; Korean dependency assessed on both Claims. Two readers plus three Source reviews pass; Rust gate eligible. |

This covers **6/6 selected v30 batches, 10 generated Claims and 10 editorial core
Source propositions**. No core proposition omission was found on this assistant's
inspection. That does not establish independently adjudicated semantic accuracy:
the hypothetical's argument wording remains questionable and only **3/6** have
complete passing gate traces. Five Claims are in those three eligible diagnostic
batches; eligibility is not approval or publication. `editorial-assessment.json`
records every batch, including failed/unresolved ones.

The v31 extraction cohort is separately **1 batch / 1 Claim**, the hypothetical,
with reader pass and Source rejection. It has zero complete passing traces.
Other v31 reviews assess older candidates and cannot be credited as v31 extraction.

## Review reliability and controls

V30 Source reviews: **14 requests: 11 pass, 1 reject, 1 unresolved, 1 protocol-invalid**.
All eight v30 isolated-reader requests pass. The complete traces for habit,
ambiguous reference and mixed language have exact agreement across their three
Source reviews on required dimensions, full-Source evidence, proposition inventory,
Claim mappings and uncertainty. Reasons may differ; semantic statuses and support
must agree. Rust replay verifies these obligations, not just Python diagnostics.

The single bounded v30 identical-input pair has differing returned verdicts
(`reject` without any assessment versus `pass`) and differing complete signatures:
**0/1 label agreement, 0/1 complete agreement**. Because the first response is
invalid, this is not a pair of two valid semantic interpretations. It still
precludes claiming reliable repeated judgment. No majority vote is used.

V31 Source reviews: **6 requests: 3 reject, 2 unresolved, 1 protocol-invalid**:

- Original bad nested gloss: reject.
- Original fused conjecture/verification: reject, with two inventory items.
- Original inferred irritation target: unresolved; withheld, not counted a correct rejection.
- Corrected v30 delay: all dimensions pass, but `pass` with flags is invalid.
- New v31 hypothetical: reject with an erroneous unknown-speaker rationale.
- Existing assistant-authored positive nested control: unresolved, confusing unknown
  narrator identity with the identified professor and adding a certainty concern.

Thus all **3/3 known-defective control requests are withheld**, including **2/3
explicit rejections**. This narrow result does not establish a population false-
acceptance rate. The positive control and hypothetical expose continuing false-
rejection risks; there is no defensible independently adjudicated general false-
rejection rate. One v31 reader passes. V31 does not demonstrate improved overall
reviewer reliability and is not promoted. Raw outputs and failure classifications
are retained rather than normalized.

The complete semantic-repair archive now contains **19 extraction requests** over
six Sources, **36 Source-review requests** and **10 isolated-reader requests**.
Those totals include older failed experiments. Historical o8000 remains 9 complete,
4 protocol-valid, 5 invalid, 0 approved/published and 2 verdict disagreements among
3 pairs; its earlier 2,400-token truncation stays separate.

## Verification and integrity

All required final commands exit **0** (`verification-final/results.json`):

- `cargo fmt --check`
- `cargo test --offline --all-targets`: 48 pass; two ignored subprocess workers
  are exercised by their parent recovery tests.
- `cargo clippy --offline --all-targets -- -D warnings`
- `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'`: 40 pass.

Existing tests were extended for actual omitted-review evidence, v31 required
review dimensions, nonempty-versus-empty output schemas, and the amended budget.
An initial formatting failure was corrected; its log remains in `verification/`.
No new Rust or Python test method was added.

`replay-final.json` validates exact saved request recipes, Source and response
hashes, sidecars, usage and ledger charges. Three complete traces pass the Rust
experimental gate; v30 delay/hypothetical and v31 hypothetical prefixes fail at
the actual recorded stage. All reconstructed Plan hashes validate. Nested remains
without a complete trace. No absent response or later review is fabricated.

Existing authority, Source immutability, exact-hash user approval, lifecycle,
transactional publication, Unicode exact-quote anchoring and DeepSeek boundary
checks pass. No model review supplies user authority. Preexisting `.yeet/` and
`UI-Agent-Brief.md` remain untouched. Phase 1/2 tags are unchanged.

## Spending

Current official [Haiku model pricing](https://platform.claude.com/docs/en/models/haiku-5-5/overview)
and [structured-output schema support](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)
were checked on 2026-10-24. Standard/global, no caching/tools, existing conservative
input-side rates and actual capped output reservation remain in force. Each call
reserved before dispatch in the same SQLite ledger; no unbounded retry occurred.

| Accounting item | Conservative USD |
| --- | ---: |
| Carried prior, unchanged | $4.78265335 |
| Cumulative before this authorized continuation | $4.87970805 |
| This continuation: 35 settled calls | $0.04693420 |
| Recorded request total: 75 settled calls | $0.14398890 |
| Cumulative upper bound | **$4.92664225** |
| Approved cumulative ceiling | **$4.93265335** |
| Remaining authorized headroom | **$0.00601110** |

Zero unsettled requests and zero reservation overruns. All new responses completed;
no new truncation. Provider-billed USD remains unknown: these are conservative
computed accounting bounds. Unused capacity is retained. The ceiling stays strictly
below $5. Historical requests, prior and charge conventions are unchanged.

## Remaining acceptance work

The original acceptance scope remains in force. The model still confuses
conservative unknown arguments with independently asserted source commentary,
misclassifies licensed Korean ellipsis as uncertainty, and sometimes contradicts
its own aggregate verdict. Source-review failure correctly withholds these
batches; that safety does not demonstrate useful, reliable extraction acceptance.

Further work must produce a justified general reviewer correction, validate it on
both positive and defective cases, retain bounded repeat failures, and establish
complete current-profile traces for the unresolved priority cases. The five
missing-profile cases remain unassessed and broader Korean quality remains
unproven. Additional prompt versions or passing tests alone cannot satisfy that
requirement. The machine-readable completion audit explicitly remains incomplete.
