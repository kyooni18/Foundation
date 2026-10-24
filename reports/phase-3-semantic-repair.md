# Phase 3 semantic repair — incomplete checkpoint (2026-10-24)

## State and provenance

Starting commit: `3033136` on `main`, with pre-existing untracked `.yeet/` and
`UI-Agent-Brief.md` left untouched. Implementation checkpoint: `c458334`.
The ending report/evidence commit is the commit containing this file; see Git history.
Phase 1 and Phase 2 tags were not moved. No `phase-3` tag was created.

The original Sources, frozen reference labels, v23 prompts, prior direct-Anthropic
request/response artifacts, and historical 2,400-token truncated attempt remain
unchanged. No production Claim was published or approved.

## Reconstructed defects and changes

| Defect | Saved evidence and root cause | Repair / observed limit |
| --- | --- | --- |
| `v23-fresh-003` joined propositions | One candidate combined Jinwoo's possibility of an unspecified delay with his separate statement that he had not confirmed it. V23 examples urged attaching verification status and could override semantic independence. | V24 frames explicitly split separately asserted speech acts while retaining their attribution and uncertainty. A v24 live review rejected the old combined candidate with two inventory items. No v24 extraction output was generated, so generator improvement is unmeasured. |
| `r1-nested_quote` unsupported gloss | V23's own example expanded `우리 방법` into a statement about an unspecified group's contents. The original only supports the nested report and the narrator's disagreement; it does not supply that explanation. | V24 removes the example and preserves the narrator's quoted perspective. V25 added a candidate-added-meaning counterfactual audit. Both live reviewer versions still returned a coherent false `pass` on the old candidate. This mechanism remains open. |
| `r1-ambiguous_ref` causal object | The second Claim attributed the narrator's irritation to the unidentified person's lateness. Adjacency does not establish the irritation's cause or object. | V24 extraction/review instructions require an unspecified irritation target unless the Source licenses a link. No corrected live candidate or independent adjudication was obtained. |
| Contradictory review output | Four of nine o8000 responses said `pass` while carrying uncertainty flags. The two `v23-fresh-003` responses said `unresolved` despite an atomicity `fail`. | New v24/v25 protocol uses required per-Claim atomicity, entailment, attribution, polarity/scope, missing-argument, unsupported-inference and Korean-meaning results, explicit reasons, original-source inventory/evidence, and deterministic aggregate consistency. It retains raw responses and rejects contradictions. Three independent source reviews must agree on verdict, dimensions, evidence, inventory coverage, and uncertainty. |
| `v23-fresh-004` mixed-language assessment | The historic review marked Korean meaning N/A on an English Claim whose “count” referent depends on Korean context. V23 validator correctly rejected it under its conservative mixed-source rule. | New validator keeps that conservative protection because a reviewer can omit a Korean antecedent from its evidence. It does not post-hoc pass the historical response. Source-dependency-specific relaxation remains unproven. |

All v24/v25 Claims remain subject to the exact original-source and prompt-hash
checks, quarantine, local-user exact-hash approval, lifecycle validation, and
transactional publication. The new review protocol supplies diagnostic evidence;
it does not grant model authority. No schema migration or new dependency was added.

## Before/after evidence with denominators

The frozen o8000 baseline had **9/9 complete responses**, **0 truncated**, **4/9
protocol-valid**, **5/9 protocol-invalid**, **0 accepted Claims**, and **2 verdict
disagreements among 3 repeated pairs**. Nine responses cover six distinct selected
cases; five of the original 12 lacked current-profile v23 candidates and are not
counted as passes. One earlier 2,400-token truncation remains a separate attempt.

Read-only replay of all nine raw responses in
`reports/evidence/phase-3/semantic-repair/o8000-offline-replay.json` finds **6/9
aggregate verdicts inconsistent** with their own dimensions/uncertainty. Applying
the stricter aggregate rule retrospectively would leave **2/9** responses coherent
and protocol eligible after retaining the historical mixed-Korean N/A rejection.
This is a protocol reclassification, not a semantic improvement or a change to
historical v23 judgments. The replay records each original request/response hash.

Three new targeted reviewer calls on two distinct *old* candidates were completed:

| Prompt | Case | Protocol | Model verdict | Assistant source inspection |
| --- | --- | --- | --- | --- |
| v24 | `r1-nested_quote` | valid | pass | False acceptance of the known added gloss. |
| v24 | `v23-fresh-003` | valid | reject | Correctly flags one Claim containing two propositions; inventory has two items. |
| v25 | `r1-nested_quote` | valid | pass | The added-meaning instruction still failed to prevent false acceptance. |

Thus **3/3** new judgments were protocol coherent, but **2/3** accepted a known
unsupported candidate; those two calls concern the same case under different
prompts. This is not an independent quality rate. No new extraction outputs or
independently adjudicated whole-corpus ground truth were obtained. Generated-Claim
coverage, missing-proposition rate, Korean fidelity rate, and false-rejection rate
for the new prompt remain unknown. No accepted or published Claims are credited.

The raw request, response, token usage, judgment and outcome for each call are in
`reports/evidence/phase-3/semantic-repair/`. No retry-to-pass or majority vote was
used. The v24 and v25 live results rule out claiming reliable reviewer repair.

## Korean and evidence findings

The direct Korean nested-report test shows that a well-formed, fully reasoned
review can still rationalize an unsupported expansion of `우리 방법`. V24 instructions
also cover omitted subjects, nested reports, unknown referents, deictic time,
uncertainty, and causal scope, but none of those corrections has a measured new
extraction success rate. The ambiguous-irritation case remains unresolved.

The original character-offset re-anchoring remains limited to a unique exact
quotation; no fuzzy matching was added. Targeted tests cover Korean Unicode scalar
indices, repeated text, nested quotations, emoji, combining characters, and mixed
spans. An exact anchor establishes a location, never semantic entailment. V24/v25
review evidence uses unique exact Source quotes and does not rely on model-generated
offsets. Ambiguous repeated quotes fail the new review protocol.

## Verification and cost

Exit codes were 0 for `cargo test --offline --all-targets` (47 passing tests,
2 ignored subprocess workers), `cargo clippy --offline --all-targets -- -D warnings`,
`python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'` (38 passing),
and final `cargo fmt --check`. The targeted v24 review regression passed. Offline
replay regenerated byte-identical output in a fresh `/tmp` file. Evidence was
scanned for credential strings; none was found.

Before paid execution, the [official Haiku 5.5 price card](https://platform.claude.com/docs/en/models/haiku-5-5/overview)
and [Messages structured-output reference](https://platform.claude.com/docs/en/api/http/messages)
were checked on 2026-10-24. The existing direct Anthropic transport, immutable
receipts and SQLite ledger were reused. All three requests were durably reserved
before dispatch and settled; there are **0 unsettled requests**. Their combined
conservative token-price upper bound is **$0.00597390**, not a confirmed provider
bill. The carried-forward historical amount is **$4.78265335**; all recorded
request upper bounds total **$0.03501600**; cumulative conservative accounting is
**$4.81766935** against the unchanged **$4.88265335** ceiling. Remaining ceiling:
**$0.06498400**. The aggregate remains strictly below $5. No budget reset or
alternate billing account was used.

## Remaining acceptance risk

A protocol-valid Haiku `pass` still accepted the known unsupported nested-quote
gloss twice. The new prompt's ability to produce atomic Claims has not been
measured with fresh extraction. Mixed-language dependency judgments can still be
wrong, and no independent adjudicator has reviewed the full corpus. Three source
reviews and exact-hash user approval reduce exposure but do not prove semantic
correctness. Phase 3 is **not semantically accepted**; v24/v25 remain experimental
and are not production defaults. The five missing current-profile cases are not
counted as successes. Further work should obtain corrected extraction candidates,
independent source-level adjudication, and repeated source-grounded reviewer
measurements before promotion or a completion tag.
