# Phase 3 semantic repair — acceptance remains incomplete (2026-10-24)

## Latest authorized continuation

The user approved the pending up-to-$0.05 increase. Implementation commits
`1324ca8` and `5b9b993` preserve the existing ledger and add versioned reviewer
experiments plus faithful replay of invalid judgments. Three v30 batches now have
complete passing Rust-verified diagnostic traces: recent habit, ambiguous reference,
and mixed Korean/English meaning. The other three priority cases remain ineligible;
v31 still produces false-rejection risks and a contradictory pass. Neither profile
is promoted, and Phase 3 remains incomplete.

All required checks pass (48 Rust, 40 Python, fmt, Clippy). This continuation used
35 settled calls / $0.04693420 conservative upper bound. Cumulative bound:
**$4.92664225**, ceiling **$4.93265335**, remaining **$0.00601110**, zero unsettled.
Provider billing is unknown. Full results, denominators, failures and remaining work:
[current validation report](phase-3-v31-validation.md).
Machine audit: `reports/evidence/phase-3/v30-authorized/completion-audit.json`.
The sections below preserve earlier checkpoints and their historical budgets.

## Earlier offline corrections

`0038290` and `725c8a7` close repeated-inventory, blank-explanation and decomposed-
Korean review bypasses, and preserve legitimate empty extractions. All 48 Rust and
40 Python tests pass. No model calls were made; spending and the pending capacity
proposal are unchanged. See `reports/phase-3-offline-protocol-followup.md`.
Semantic acceptance remains incomplete and the completion goal remains active.

## Current continuation

The latest continuation (`05dc0a1`, `78b93c2`) fixes source-aware evaluation parity
and produces a faithful two-Claim nested-report candidate under v30. A complete
passing current-profile review trace is still missing. Cumulative conservative
accounting is $4.87970805; $0.00294530 remains under the unchanged ceiling. The next
source review requires $0.00449180. The $0.05 capacity proposal is inert and unapplied.
See `reports/phase-3-v30-continuation.md` and
`reports/evidence/phase-3/v30-continuation/completion-audit.json` for current state.
Phase 3 is not complete, tagged or promoted.

## Reservation follow-up

Commit `8992fb1` removes duplicate reservation padding and uses the request's actual
output cap. The formerly blocked review now reserves **$0.00698020** instead of
$0.01596040 (56% less) and completed with a protocol-valid pass at a conservative
usage cost of **$0.00264480**. Current cumulative accounting is **$4.87139635**, with
**$0.01125700** remaining and zero unsettled requests. Historical charges and the
total ceiling are unchanged. This reduces reservations, not the price of tokens or
historical spending. Phase 3 remains semantically incomplete. Details and current
machine-readable state: `reports/phase-3-reservation-repair.md` and
`reports/evidence/phase-3/reservation-v2/acceptance.json`.

The remaining sections preserve the `853e657` semantic-repair checkpoint.

## State, commits and scope

This continuation started at `f793f16` on `main`. Its verified implementation
checkpoints are `15f3c92`, `afd1982` and **`cb2c98b`**. The report/evidence delivery
commit is the Git commit containing this revision. Earlier repair checkpoints
`c458334` and `2737efa` remain in history. Phase 1/2 tags are unchanged; there is
**no `phase-3` tag**. The pre-existing `.yeet/` and `UI-Agent-Brief.md` are untouched.

The implementation and authorized evaluation work below are complete, but the
requested semantic acceptance is **not established**. New extraction still produced
incorrect Claims, and coherent reviews still disagreed on evidence/inventory.
No Claim was approved or published. No new profile became the production default.
This report supersedes the earlier checkpoint narrative; the historical root
`reports/evidence/phase-3/acceptance-status.json` remains an immutable older snapshot.
The checkpoint machine-readable state is `semantic-repair/continuation-acceptance.json`.

All paths below are relative to `reports/evidence/phase-3/` unless stated otherwise.
Original Sources, frozen reference labels, v23 prompts, raw historical responses,
and the separate 2,400-token truncated attempt remain unchanged. The six priority
Sources now serve as development regressions; their historical “heldout” origin
is not a claim of fresh independent holdout evaluation after this tuning.

## Root causes and implemented corrections

| Mechanism | Correction | Demonstrated result / limitation |
| --- | --- | --- |
| A conjecture and a separate verification statement were merged. | V24+ decomposition separates independently assessable speech acts while copying actual attribution and uncertainty. | V25/v26 fresh extraction produces two Claims for `v23-fresh-003`. V24 and v28 reviewers reject the old fused Claim. The shorter v28 extractor regresses on argument completeness. |
| A quoted `우리 방법` was expanded into unsupported method/group commentary. | V27 adds required `source_meta_expansion`; v28 distinguishes necessary unknown-argument markers from added assertions about document contents. Every separate Claim must bind quoted possessive perspective. | V27 rejects the old gloss once; v28 rejects it twice and rejects new `사용자 팀` inventions. V25/v26 extraction still leaves the second Claim's quoted perspective ambiguous. |
| Irritation was assigned a target/cause from adjacency. | Extraction/review instructions distinguish reference resolution licensed by discourse from an unasserted causal relation. | V25 no longer invents the irritation target, but adds an independent source-commentary clause. V28 rejects the original unsupported target. |
| Overly restrictive entailment review rejected contextual reference resolution. | V28 permits an omitted disagreement object to resolve to the sole salient preceding assessment, with evidence for both clauses. | A corrected narrator-perspective control passes twice. It is assistant-authored editorial material, not independently adjudicated gold. |
| A reader copied from v23 still endorsed the bad gloss and merged verification example. | V27 removes those contradictory examples; source-blind reading remains limited to isolation and atomicity. | Deterministic protocol integration is verified. No new live standalone-reader trial or full worker acceptance trial is credited. |
| Schema-valid review could omit a generated Claim from a passing inventory. | All Claim indices must have passing inventory support before aggregate pass. Three-review comparison now includes inventory proposition text as well as mappings, status and evidence. | Targeted regression rejects an orphaned candidate and an incompatible inventory with otherwise identical evidence/mappings. Different inventory wording remains unresolved because deterministic code cannot prove equivalence. |
| Contradictory aggregate verdicts and invalid evidence could pass weaker protocols. | V24+ source-bound checks retain exact text/index identity, unique original quotations, required dimensions, explicit uncertainty, coherent aggregate verdicts and three-review agreement. Python diagnostic checking now also rejects malformed field types, duplicate inventory indices and absent coverage. | All 13 saved repair reviews replay as internally coherent. Coherence does not establish semantic truth. |
| English output depended on Korean antecedents. | Keep conservative Korean assessment for any Korean-containing Source; evidence cannot safely determine all omitted dependencies. | V28 assesses Korean meaning on both Claims of corrected `v23-fresh-004`, including its English Claim, and passes. Historical N/A rejection is unchanged. |

V26's added isolation audit did not resolve the tested perspective problem. V28's
shorter extractor introduced new errors despite its improved reviewer contrast.
Those approaches are retained as **unsuccessful experiments**, not promoted repairs.
There is no case-specific postprocessing, approximate evidence matching, schema
migration, added dependency, or Phase 4 feature.

Existing Source hashes, prompt versions, quarantine, exact-hash local-user approval,
recorder authority, lifecycle validation and transactional publication remain the
publication boundaries. The new source-bound checks run again at approval. A model
review cannot supply user authority or override a stale/forgotten Source.

## Baseline and fresh extraction results

The frozen v23/o8000 baseline remains **9 complete / 9 responses**, **0 truncated**,
**4 protocol-valid**, **5 protocol-invalid**, **0 accepted Claims**, and **2 verdict
disagreements / 3 repeated pairs**. Six distinct selected Sources were represented.
The earlier truncated 2,400-token attempt remains separate. Retrospective replay
still detects **6/9 aggregate inconsistencies**; this reclassification is not a
change to historical results or a semantic improvement.

This continuation made **10 extraction requests**, producing **18 Claims** from
**6 distinct priority Sources**: v25 covers all six; v26 and v28 each revisit the
two central failures under distinct frozen prompts. All 10 responses completed;
all 10 pass both local envelope/anchor checks and Rust `source-checks-v4` structural
replay. **None has a complete Plan + standalone-reader + three-source-review proof**,
so all 10 remain ineligible for experimental gate credit. The replay's null Plan
and false Plan-hash result mean missing gate evidence, not a fabricated prompt hash
or a semantic failure classification. Exact actual request-system hashes are retained.

| Source | Source inventory / v25 generated Claims | Editorial inspection of v25 output |
| --- | --- | --- |
| `r1-nested_quote` | 2 / 2 | Both central propositions represented; no old method-content gloss. Second Claim still ambiguously owns `우리 방법`. |
| `v23-fresh-003` | 2 / 2 | Conjecture and lack of confirmation split; uncertainty and Jinwoo attribution preserved; affected target explicitly unidentified in both. Demonstrative isolation remains unverified by a live reader. |
| `r1-ambiguous_ref` | 2 / 2 | Unknown late person and irritation preserved without a causal target. Second Claim has one extra assertion about the Source's absent target/cause; this is an atomicity/source-commentary defect. |
| `r1-hypothetical` | 1 / 1 | Loss threshold, conditional intention and unidentified experiment retained. Wording narrows the architecture to one used in that experiment; exact argument fidelity remains unresolved. |
| `r1-habitual_temporal` | 1 / 1 | Original-speaker and recent habitual time frame retained; no invented date or permanent habit. |
| `v23-fresh-004` | 2 / 2 | Korean worker-count assertion and English limit on what it establishes preserved. English `count` explicitly resolves to Mori `worker_count=4`. |

These are editorial inspections of all generated v25 batches, not a passing-subset
success rate. `continuation-editorial-assessment.json` records generated counts,
source inventories, visible atomicity/unsupported-assertion defects, omissions and
unknown measurements for **all 10 extraction attempts**. Core proposition recognition
does not mean complete Claim validity. V28's nested report adds team ownership in
**both** Claims; its delay batch loses the verification target and the faithful
representation of one source proposition. These failures are included in the
population. Independently validated Korean-fidelity and general false-rejection
rates remain unknown. No semantic success rate is calculated from structural passes.

The five original missing-profile cases (`new-003`, `new-007`, `new-018`, `new-025`,
`new-042`) remain missing current-profile evidence; they are not counted as passes.
Existing broader archive replay remains historical and is not relabeled a fresh
trial. The frozen editorial reference entries that merge the delay speech acts or
infer an irritation target are not silently substituted for the user's corrected
requirements. They remain unchanged; the new editorial inventory is separate.

## Reviewer results and repeated disagreement

Across the earlier repair checkpoint and this continuation there are **13 repair
review requests**: v24=2, v25=1, v27=2, v28=8. All completed and are protocol-valid;
there were no new truncations or retries. The earlier two false passes remain in
this denominator. No result is normalized into a fabricated agreement.

| Version / candidate | Calls | Result |
| --- | ---: | --- |
| v24 / old nested gloss | 1 | False pass. |
| v24 / old fused delay Claim | 1 | Reject; two source propositions inventoried. |
| v25 / old nested gloss | 1 | False pass. |
| v27 / old nested gloss | 1 | Reject for added source commentary/interpretation. |
| v27 / v25 nested output | 1 | Reject for ambiguous perspective; reasoning also over-restricts contextual disagreement resolution. |
| v28 / old nested gloss, identical requests | 2 | Both reject; no new false pass on this known defect. |
| v28 / new team-ownership invention | 1 | Rejects both Claims. |
| v28 / corrected nested editorial control, identical requests | 2 | Both pass; expected label is excluded from reviewer input. |
| v28 / old fused delay Claim | 1 | Rejects atomicity and added commentary. |
| v28 / old irritation-target Claim | 1 | Rejects invented target, entailment and Korean meaning. |
| v28 / corrected mixed-language output | 1 | Pass; Korean dependency assessed on English output. |

V28 returned reject on **5/5 targeted defective-candidate requests** and pass on
**3/3 positive-candidate requests**. These are narrow diagnostic counts, including
repeated inputs and two assistant-authored control reviews; they are not independent
population estimates of false acceptance/rejection. The same known gloss previously
passed v24/v25, so the new rejection is useful evidence about that mechanism.

For the **2 identical-input repeated pairs**, verdict disagreement is **0/2**, but
complete evidence/inventory agreement is **0/2**. Dimension statuses agree; quotations
and inventory wording/support differ. The corrected control's second inventory
sometimes cites only the disagreement, omitting the preceding assessment needed to
support its target. The final gate therefore withholds acceptance. Exact inventory
text comparison strengthens this conservative rule further. Agreement alone would
still not be independent evidence of truth. There is no majority vote.

These calls use a diagnostic direct-Anthropic request recipe. V28's system contains
the same review/frames/fidelity-stage composition as the corresponding Plan, but
these requests do not constitute full production worker runs, reader validation,
three-stage consensus, user approval or publication. Earlier review receipts hash
the review asset in `prompt_sha256`; the immutable payload and replay also record
the complete actual system hash, including appended frames/stage. Later receipts
add `system_sha256` explicitly. No historical receipt is rewritten.

## Offline verification and evidence integrity

All required commands exited **0** on the final implementation:

- `cargo fmt --check`
- `cargo test --offline --all-targets`: **48 passing**, 2 ignored subprocess workers
  exercised by their parent recovery tests.
- `cargo clippy --offline --all-targets -- -D warnings`
- `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'`: **39 passing**.

Logs and exact exit codes are in `semantic-repair/continuation-verification/`.
The existing targeted source-bound review test was extended for new dimensions,
orphaned inventory and inconsistent proposition identities. One Python regression
uses a saved live control to reject contradictory flags, omitted dimensions,
duplicate indices, boolean indices, uncovered Claims and Korean N/A. Tests establish
protocol behavior, not model semantics.

`evaluation/phase3/replay_semantic_repair.py` performs no network calls. It verifies
saved raw output against candidate/judgment sidecars, exact frozen Source and request
recipes, actual prompt-system hashes, token accounting and the read-only SQLite
ledger; it invokes Rust structural validation in a temporary database. Final output
is `semantic-repair/continuation-replay-v2.json`. V1 is retained as a prior audit
snapshot; v2 additionally compares proposition text. A fresh replay matches v2
byte-for-byte. Historical o8000 replay also matches its saved artifact.

Unique exact-quote re-anchoring and Unicode protections remain intact: scalar/byte
differences, overlapping/repeated quotations, nested quotes, emoji, combining marks
and mixed spans are covered by existing tests. A valid coordinate never establishes
entailment. The DeepSeek English boundary is unchanged. New evidence was scanned
for credential values; none was found. Foundation was recalled and updated; Jev
was unavailable in the enabled tool inventory.

## Budget and remaining acceptance work

The [official Haiku 5.5 price card](https://platform.claude.com/docs/en/models/haiku-5-5/overview)
and [Messages structured-output schema](https://platform.claude.com/docs/en/api/http/messages)
were rechecked before paid execution. Every call used the existing direct adapter's
transport, model identity check, conservative usage accounting, durable reservation
and shared ledger. The interrupted request settled successfully and was not retried.

Conservative accounting at `853e657`, **not confirmed provider billing**:

| Item | USD |
| --- | ---: |
| Carried historical amount | $4.78265335 |
| All 33 recorded request upper bounds | $0.08609820 |
| Cumulative upper bound | **$4.86875155** |
| Unchanged authorized ceiling | $4.88265335 |
| Remaining capacity | **$0.01390180** |
| This continuation: 20 calls beyond `f793f16` | $0.05108220 |

There are **0 unsettled requests**. The next relevant review of corrected delay
output requires a **$0.01596040** reservation, exceeding remaining capacity; it was
dry-run only. No further paid dispatch, budget reset, account substitution or
ceiling increase occurred. The total remains strictly below $5.

The current evidence supports stronger rejection of the observed gloss, causal
target and fused-speech-act defects, and preserves user authority. It does **not**
support reliable accepted standalone Korean Claims across the claimed scope.
Extraction still has perspective/argument errors; repeated reviews still lack
complete evidence/inventory agreement; live reader and full worker acceptance
remain unmeasured for these repairs; independent whole-corpus adjudication is
unavailable. The next acceptance work requires a validated extraction profile,
source-level adjudication and complete reader/review evidence. **Phase 3 remains
explicitly incomplete; no completion tag or production promotion is justified.**
