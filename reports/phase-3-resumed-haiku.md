# Superseding Phase 3 delivery (2026-10-25)

The focused v54 / Haiku 5.5 semantic repair scope is accepted: six complete priority
Source gates, ten Claims, known negative controls and selected exact-request repeats.
See [the final report](phase-3-semantic-acceptance.md) for the 3/6 primary and 6/6
bounded-repair denominators, exact accounting, test results and limitations. The
annotated `phase-3` tag identifies delivery. No universal accuracy or production-default
promotion is claimed. The incomplete status and blocker descriptions below are
retained only as historical checkpoints.

---

# Phase 3 resumed Haiku repair — semantic acceptance incomplete

Work resumed from `9511d09`; implementation checkpoints include `7ee359c`,
`fabff13`, `6bc7ebb`, `6e5a9bc`, `d6d77ac`, `adf26df` and `852d4d2`.
`852d4d2` is the ending implementation commit; subsequent commits preserve evidence
and this report. No Phase 3 tag or production default promotion is justified.

## What changed and why

The historical pending-request count conflated old unknown billing with live calls.
After the human explicitly resumed work, direct dispatch now requires an exclusive,
nonblocking file lock throughout reservation and transport. Every unknown reservation
still consumes its full estimate. The same ledger, prior, historical charges and
strict cumulative ceiling below $6 remain in place; the approximate account credit
balance was not treated as a reset or additional cumulative allowance.

Canonical JSON serialization sorted schema properties and silently defeated the
intended evidence-first generation order. v44+ preserves actual property order and
records the exact wire hash. Replays check both the frozen recipe and serialized
bytes. Literal unique Source-span vocabulary and zero-based occurrence constraints
reduce fabricated evidence coordinates without asserting semantic correctness.
Finished text is checked before assigning verbatim basis or optional mentions.

Nine narrow Source questions fragmented the interpretation and created avoidable
contradictions. v47+ uses the three requested responsibilities: atomicity and
independence, complete-Claim entailment, and whole-batch coverage. All ten dimensions
remain mandatory, including the seven shared meaning features. Exact Claim/Source
identity, complete inventory-to-Claim bijection, coherent aggregate status and any
reject/unresolved veto remain. Inventory row order can differ; proposition identity,
quotes and mappings cannot. No majority vote or raw-output normalization was added.

v49 condensed accumulated extraction instructions around minimal complete assertions,
individually bound perspective/time, intensity, modal force and no document-absence
explanation. v50 adds mandatory standalone interpretation and reason/status findings
for atomicity, reference and time before the final verdict. Rust and Python reject
missing analysis, blank reasons, mismatched issues and contradictory verdicts. The
worker, quarantine/approval checks, evaluators, saved replay and bounded repair
feedback use the same versioned reader protocol. Older reader protocols remain
frozen. Existing tests were extended; no new compiled test method, dependency,
migration or Phase 4 feature was introduced.

## Real outcomes and their denominators

The six priority Sources are exposed development cases, not a held-out set. The
current cohort reconstructs one v50 recipe. Earlier request names are reused only
when complete generation/Source payloads and ordered wire hashes match v50; all 17
saved v49 Source requests were checked for exact recipe equality. This is receipt
reuse, not 17 additional independent reviews.

| Source | Current observed result |
| --- | --- |
| Emotion / ambiguous person | Two Claims preserve repeated lateness, unknown Source-relative person, original utterance time and **진짜**. No inferred irritation target/cause or document-absence assertion. Complete gate eligible. |
| Nested report | Initial v50 output left **우리** implicit and the reader rejected it. One actual bounded repair binds the narrator-relative possessive in both Claims, retains the full professor/reviewer relay and **못**, and separates disagreement. Complete gate eligible. |
| Mixed Korean/English | Korean numeric assertion plus English assessment explicitly naming Mori worker_count and its given value. Korean dependencies remain assessed. Complete gate eligible. |
| Recent habit | One current-period, ongoing daily-6 habit tied to the original statement, without a permanent disposition. Complete gate eligible. |
| Delay conjecture/checking | Two speech acts remain separate. The new reader withheld a bare affected argument. Its actual repair supplies an explicitly unidentified affected target in both Claims, but Source entailment then marked that same target slot unsupported/unresolved. Complete gate withheld. |
| Conditional intention | One IF-scoped intention remains, but bare experiment/architecture references and intention time lack explicit binding. The reader's passing reason invents speaker ownership/current activity. Source reviews also pass individually. Their inventories differ by a final period, so the **complete Core gate remains unresolved**, independently of the editorial objection. No trimming or approximate matching makes it pass. |

Thus four of all six current Source batches have complete eligible diagnostic traces;
two remain withheld. This is not an independently adjudicated semantic success rate.
Initial failures are not erased by repair outcomes. The five original cases without
current-profile candidates (`new-003`, `new-007`, `new-018`, `new-025`, `new-042`) remain
unassessed, not passes; the archive-only audit remains separate.

Four known bad candidates cover fused conjecture/checking, invented irritation cause,
unsupported method-contents commentary and the actual v38 grammatical-commentary
repair. v48 and v49 each produced eight coherent rejections across those four
candidates plus one exact-request repeat apiece. These are 16 judgments on four
known controls, not 16 independent negative cases. v50's Source recipe is identical
to v49, so no new Source-control calls are claimed.

Earlier v48 delay primary/meaning repeat disagreement remains visible. Its actual
repeated receipt is reconstructible as a failed trace, with its own request/response
hashes; it does not overwrite the primary or become a majority vote. Latest positive
repeat results and exact request comparisons are in the current audit and full replay.

## Korean findings and rejected approaches

Dropping 진짜 while preserving 또 still loses emotional intensity. Restoring intensity
by appending a separate assertion that the Source omits an emotion target adds content;
the fidelity gate rejected that repair. A simple strongly felt emotion needs no cause.
The nested **못 → 않** change lost modal force; preserving 못 and binding 우리 in both
Claims fixed that measured defect through actual failed-trace feedback.

The remaining delay dispute is a semantic-policy conflict: the isolated reader requires
an explicit affected target, while the Source reviewer says existential 지연 has no
such slot. Treating either judgment as automatically authoritative would conceal the
problem. The conditional reader's fabricated ownership/binding is a demonstrated
individual-review false acceptance even though another gate currently withholds the
batch for an inventory difference. Accidental rejection elsewhere is not proof that
this interpretation defect is repaired.

The six-call thinking-disabled/high-effort probe was rejected: an invalid fused pass
and an overly literal rejection of a valid checking target did not support adopting
that mode. All raw probe requests, responses and costs remain; production reasoning
mode is unchanged. Initial truncation and explicit higher-output-capacity generation
are separately named, not silently retried or repaired as raw JSON.

## Verification, accounting and limits

All required commands exit 0 in
`reports/evidence/phase-3/resumed-haiku/verification-v50/results.json`:
`cargo fmt --check`, `cargo test --offline --all-targets`,
`cargo clippy --offline --all-targets -- -D warnings`, and the Python discovery suite.
Rust has 48 passing tests and two ignored subprocess workers exercised by their parent
tests; Python has 40 passing methods. Current worker tests exercise v50 through
quarantine and exact-hash user-only approval, preserve Source bytes, and reject agent
publication. Model diagnostics themselves approved/published zero Claims.

Terminal accounting: 573 ledger attempts, carried prior **$4.78265335**,
recorded conservative amount **$1.06646340**, cumulative
**$5.84911675**, unchanged ceiling **$5.99999999**,
and **$0.15088324** headroom. This resumed sequence added
**276 attempts** and **$0.54366690** of conservative amount over
the before snapshot. The repeated prompt experiments remain a small Source sample
despite this large call count. No further paid call is dispatched merely to spend
that headroom or erase an observed disagreement.

Both selected positive fidelity repeats pass with identical actual wire hashes:
emotion and mixed-language, **2/2** coherent repeat agreements. These are two pairs,
not repeated evidence for all six Sources. Historical v48 delay disagreement remains
in the same denominator-aware record.

The terminal ledger snapshot, unchanged historical-row check, complete six-Source
trace hashes, repeat results and offline replay are under
`reports/evidence/phase-3/resumed-haiku/`. Conservative amounts are computed bounds,
not confirmed provider invoices. Four old unknown outcomes retain full reservations;
no new unresolved transport is excused or repriced. The original 297 request rows and
budget row are checked unchanged. Keys were read through a private interactive-shell
pipe and were neither printed nor saved in evidence. Unrelated `.yeet/` and
`UI-Agent-Brief.md` remain untouched.

No independent whole-corpus human ground truth exists. Synthetic protocol consistency,
exact spans and model agreement do not establish truth. The residual reference-binding
false acceptance and conflicting event-target obligations prevent Phase 3 semantic
acceptance. Keep the current profile opt-in and the incomplete status explicit.
