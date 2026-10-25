# Phase 3 completion — focused Haiku semantic repair accepted

Start: `9511d09`. Ending implementation: `89c1d12`. Final evidence checkpoint:
`5f4cdf7`. The annotated `phase-3` delivery tag pins the implementation, evidence and
this report. Phase 1 and Phase 2 tags remain unchanged.

Acceptance is for the implemented Phase 3 workflow and the demonstrated **v54 / Claude
Haiku 5.5 priority-boundary scope**. It is not a universal accuracy claim, a production
prompt-default promotion, independent human ground truth, or approval of any real
Claim. All real diagnostic proposals remain unpublished. Broader corpus evaluation
and real-world entity resolution are outside this acceptance statement.

## Results, including failures

All six requested priority Sources have complete current-profile traces, with ten
Claims and all 28 mandatory judgments: ten isolated readers and eighteen Source
reviews. Every required dimension passes and both Source inventories agree under
the versioned identity rules. The three Source responsibilities retain all ten
checks; grouping did not remove a gate.

| Source | Final meaning and outcome |
| --- | --- |
| `v23-fresh-003` | Two Jinwoo speech acts: possible delay affecting an explicitly unidentified target; inability to check whether that delay exists. The target is repeated within each Claim. No actual delay or completed verification is asserted. Full gate and identical-request fidelity repeat pass. |
| `r1-ambiguous_ref` | An unidentified Source-labelled third person was late **again**; the original speaker felt **진짜** irritation at the original utterance time. No cause or emotional target is inferred from adjacency. Two Claims, full gate passes. |
| `r1-nested_quote` | One complete professor → Reviewer 2 → assessment relay; one separate non-agreement stance. Both bind the narrator-relative quoted **우리 방법**. The stance preserves **못** and original utterance time. No method contents, group identity or authenticated user is invented. Full gate and identical-request stance-reader repeat pass. |
| `r1-hypothetical` | One Source-relative, utterance-anchored intention: if loss in the identified-by-Source experiment does not fall below0.1, overhaul the Source-designated architecture. No independent condition-fact, unguarded action, ownership or experiment/architecture relationship is added. Full gate and identical-request fidelity repeat pass. |
| `r1-habitual_temporal` | One original-statement-relative, ongoing recent daily6 waking pattern; no permanent disposition or fabricated date/timezone. Full gate passes. |
| `v23-fresh-004` | Korean count assertion and an English epistemic assessment explicitly naming the Mori worker_count and its given value4. The count alone does not establish parallel execution; actual sequential execution is not asserted. Korean dependencies remain required for the English Claim. Two Claims, full gate passes. |

The denominator is **all six Sources**, not a passing subset. Saved primaries with the
same current extractor recipe needed repair in three of six cases. The same-recipe
sequence comprises ten actual extraction attempts: emotion1, delay2, conditional2,
nested3, mixed1, habit1. Current readers reject the bad delay primary, bad nested
primary and insufficiently time-bound intermediate nested repair. The bare-reference
conditional primary also rejects, including an exact-request repeat. Thus the result
is **3/6 eligible primaries and 6/6 final batches after bounded repair**, established
retrospectively from exposed saved candidates, not a prospective held-out success rate.
Earlier different-recipe failures remain separate and visible.

Four original known bad candidates—fused delay/checking, inferred irritation cause,
method-contents commentary and the actual v38 grammatical-commentary repair—have
eight coherent current-recipe Source rejections across primary/repeat pairs. Their
Source request bytes match v54; no reused response is counted as a new independent
call. The additional bare-reference conditional has two coherent v54 reader
rejections. Three selected positive pairs (delay fidelity, conditional fidelity,
nested stance reader) agree, with actual wire hashes verified identical. Any
nonpassing repeat would have blocked acceptance; no majority vote was used.

## Mechanisms repaired

1. **Actual generation order and evidence grounding.** Canonical transport had sorted
   schema properties, undoing the intended evidence-first order. Ordered JSON and
   exact wire hashes now bind the real request recipe. Unique literal Source-span
   choices, zero-based occurrences, text-before-basis verification and optional empty
   mentions prevent the observed coordinate/basis/phantom-mention mistakes without
   inventing support. Exact evidence still does not imply entailment.
2. **Coherent, complete review.** Atomicity/independence, complete-Claim entailment and
   coverage are distinct mandatory responsibilities. The seven meaning features share
   one whole-Claim interpretation. Exact identity, original evidence, scalar bijective
   inventory mappings, nonblank reasons and coherent aggregate status are enforced.
   Reject/unresolved or contradictory pass/flags cannot be normalized into agreement.
3. **Reference and time contract.** Isolated and Source-aware reviewers previously
   disagreed about the same unidentified affected-target phrase. Both now use one
   shared memory-context contract, distinct from grammatical verb valency. Unknown
   context restrictions do not invent an identity or document-absence fact. Bare
   demonstratives and current intentions still need explicit binding. Named or
   assigned role identifiers do not require fictional real names. This repaired the
   demonstrated Reviewer2 false rejection while preserving rejection of bare
   experiment/architecture references.
4. **Interpretable standalone inspection and bounded feedback.** Readers reconstruct
   meaning and give atomicity/reference/time findings before deriving their verdict.
   Runtime, approval, evaluators, replay and repair feedback validate the same frozen
   protocol. Actual failed reports—not fabricated criticism—drive bounded repairs.
   Intensity, modal force and current-stance time losses remain recorded before their
   corrected outputs. English output continues to respect Korean source dependencies.
5. **Source-grounded inventory presentation.** New generation constrains one exact
   spelling without the terminal sentence period. Separately, v53+ derives equal
   identity only for an unmatched curly-double framing delimiter whose actual balanced
   counterpart exists outside the unique label in ORIGINAL. Raw reviews, Source,
   Claim text, evidence and mappings stay unchanged. Paired word quotation is not
   stripped; a shortened meaning/scope or fabricated closing quotation still rejects.
   The actual v52 delay disagreement remains unresolved under v52 and becomes eligible
   under the explicit v53+ rule, with zero additional model calls for that correction.

The rejected thinking-disabled experiment and all failed/truncated/invalid historical
attempts remain archived. Adaptive reasoning was retained. No new dependency,
migration, Phase 4 feature or automatic publication was introduced.

## Verification and authority

All required commands exit0 in `verification-v54/results.json`:

- `cargo fmt --check`
- `cargo test --offline --all-targets`: 48 passed; two ignored subprocess workers are
  exercised by their parent tests.
- `cargo clippy --offline --all-targets -- -D warnings`
- `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'`: 40 methods passed.

Existing tests were extended rather than adding compiled test methods. They cover
current-profile quarantine, exact-hash user-only approval, no agent publication,
Source immutability, frozen legacy handling, missing dimensions, contradictory reader
analysis, byte-order drift, wrong mappings and the actual framing-identity regression.
Unicode/Jamo, exact-anchor ambiguity, lifecycle, lease fencing, transactional/idempotent
publication, recorder/speaker separation and the DeepSeek English-only guard remain.
No API experiment approved or published a real Claim; approval is still a separate
trusted user action and does not confer Source endorsement.

## Accounting and limits

The same ledger now has634 attempts. Prior **$4.78265335** plus recorded conservative
amount **$1.20557470** gives **$5.98822805**, below the unchanged **$5.99999999** ceiling;
headroom is **$0.01177194**. This resumed sequence added337 attempts and a conservative
amount of **$0.68277820** over the before snapshot. These are computed bounds, not
confirmed provider invoices or the account's remaining credit balance.

All297 older rows and the budget row remain unchanged. The same four old unknown
outcomes retain their full **$0.02206760** reservations; no new unknown transport
remains. New direct dispatch requires an exclusive nonblocking live-call lock. This
separates old unknown billing from live concurrency without releasing any monetary
hold, resetting accounting or silently adding capacity. Keys were read through a
private shell pipe, never printed or committed. Jev was unavailable; Foundation
Memory was recalled and updated. Unrelated `.yeet/` and `UI-Agent-Brief.md` were untouched.

The full saved replay includes70 extractions over six Sources,402 Source reviews and
138 isolated readers; these historical different-profile attempts are not pooled as
current semantic successes. The six-call reasoning-mode probe is separate; all costs
remain in the shared ledger. The large call count on a small exposed sample is an
explicit generalization limitation.

The five originally missing current-profile cases (`new-003`, `new-007`, `new-018`,
`new-025`, `new-042`) remain unassessed, not passed. No independent whole-corpus human
gold is available. Reviewer explanations are fallible even when a rejection is correct;
model agreement and this assistant's audit do not establish universal truth. Awkward
Korean particle wording in the conditional was retained, not silently polished.

The validated profile is explicitly `extraction-v54`. Legacy/default v1 remains frozen
for existing hashes and is not certified by this scope. No automatic production default
promotion or silent alternate-model selection was made. The existing OpenRouter runtime
retains its own provider-cap and pending-request checks; these direct Anthropic diagnostic
results are not a claim that a live production OpenRouter job or publication was performed.

## Reproducible evidence

All paths below are relative to `reports/evidence/phase-3/resumed-haiku/`:

- `current-audit-v54-final.json` and `audit_v54.py`: six complete gate results, immutable
  historical ledger-row check, ten same-recipe generation attempts and exact repeat IDs.
- `editorial-v54-final.json`: Original Source and all ten selected Claim texts with the
  assistant's proposition-by-proposition assessment; explicitly not independent human gold.
- `v54-*-trace.json`, `v54-cohort.json`: actual request/response receipts and reconstructed
  current Plan hashes, including exact generation/Source recipe reuse.
- `v53-negative-reuse.json`, `v54-reference-probe.json`,
  `v54-final-boundary-results.json`: negative controls, failed prefixes and selected repeats;
  the final audit rechecks all reused Source bytes against v54.
- `full-replay-v54-final.json`, `verification-v54/`: full archive classifications and actual
  required-command exit codes. `current-audit-v54.json` was preliminary and its prose profile
  label was stale; the `-final` audit supersedes it without changing model evidence.

Frozen Sources, reference labels, older prompts and original raw responses were not
rewritten. Earlier checkpoint reports remain historical, including their superseded
incomplete status and funding blockers.
