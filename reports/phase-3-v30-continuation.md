# Phase 3 continuation: faithful nested extraction, acceptance still incomplete

Started at `8ffe895`; implementation checkpoints `05dc0a1` and `78b93c2`.
The previous goal turn made progress by fixing reservations and completing another
review. This turn made additional implementation and empirical progress. The full
objective remains completing Phase 3, not accepting a smaller passing subset.

## Corrections

The v25–v27 extraction example itself left the second Claim's quoted possessive
unbound: `리뷰어 2의 “우리 방법” ...`. V29 replaces that example with a grammatical
pattern binding the original narrator's expression inside BOTH independent Claims.
Its compact frames distinguish an unknown argument inside a noun phrase from an
additional assertion about missing document details. Old assets remain frozen.

V29 then emitted a duplicate weakened inner report alongside the complete nested
relay. Its coverage review inventoried both as separate propositions. V30 therefore
adds an assertion-layer audit: `A relayed that B said X` is one original assertion
with an embedded object, not two separately asserted reports. It requires a separate
`duplicates` review dimension, with normal reject/unresolved aggregation. Unnamed
narrator roles must not acquire authenticated-user speaker metadata.

The new v30 nested extraction produced exactly:

1. 원문 화자는 교수님으로부터 리뷰어 2가 원문 화자의 표현인 ‘우리 방법’이 새롭지 않다고 말했다는 내용을 전해 들었다.
2. 원문 화자는 원문 화자의 표현인 ‘우리 방법’이 새롭지 않다는 리뷰어 2의 평가에 동의하지 않는다고 밝혔다.

Both have `speaker=unknown`. Editorial source inspection finds two independent core
propositions, preserved nested attribution and narrator-owned perspective, no old
group/method-content explanation and no redundant inner-report Claim. This is a
**generated improvement, not full semantic acceptance**: no v30 standalone-reader,
three-stage source review or independent adjudication is credited.

## Evaluation and gate parity

Both the live Rust evaluator and archived-output revalidator still used the old
`validate_review` payload protocol on v24+ source reviews. They now call the same
public source-aware validator as the worker. Full bound-profile eligibility also
checks the complete reader/three-review group and its agreement. The unbound
public helper explicitly refuses bound profiles, preventing old-shaped passing
objects from masquerading as source-aware validation.

V29/v30 review evidence uses the complete original Source verbatim, preserving all
antecedents and report context. Inventory identities use unique exact original
clauses; no generated paraphrase is silently normalized into agreement. Reasons
and all semantic dimensions remain model judgments. A valid full-Source quotation
is context/location evidence, not entailment proof. V29's erroneous three-item
inventory demonstrates that exact original substrings alone do not prove correct
proposition decomposition. V30's explicit duplicate obligation remains unmeasured
with live reviews, so it is not claimed as a proven reliability fix.

`replay_complete_semantic_trace.py` reconstructs diagnostic configuration from exact
saved API requests, verifies extraction/reader/source request recipes and response
identity, and runs Rust revalidation. It supports an actual failure prefix: the
worker stops on a failed/unresolved review, so later responses are neither invented
nor purchased merely to fill a trace. This is not a frozen production job Plan.

## New live evidence: every attempt retained

Six calls completed, with **zero truncations**, across two existing development
Sources. The three extraction requests produced **6 Claims** in total.

| Request | Observed result |
| --- | --- |
| v29 nested extraction | 3 Claims: narrator perspective fixed, but one duplicate weakened inner report. |
| v29 nested coverage review | Protocol-valid `unresolved`; incorrectly inventories 3 propositions and adds uncertainty flags. It does not substantiate duplicate rejection. |
| v29 recent-habit extraction | 1 Claim, original-speaker/recent-time frame retained; `speaker=user` metadata remains an attribution question. |
| v29 isolated reader on habit | Protocol-valid `pass` on the ONE text, with no Source or neighboring Claims supplied. |
| v29 source atomicity review on habit | Protocol-valid `unresolved` over the omitted-subject/speaker inference. Other dimensions mostly pass. This is a source/reader distinction, not model agreement. |
| v30 nested extraction | 2 Claims shown above; no duplicate inner report; both speaker fields unknown. |

The attempted recent-habit sequence stopped after its first source review, matching
worker failure behavior. Offline reconstruction verifies the Plan hash and produces
`valid=true`, `experimental_gate_eligible=false`, `review_error=unresolved`.
No missing fidelity/coverage calls are counted as passes. We did not retry either
unresolved review or use majority voting. No current-profile accepted quality rate
is calculated; independent semantic adjudication is unavailable.

Across the repair evidence directory there are now 13 extraction requests from
6 distinct priority Sources, 16 source reviews, and 1 isolated reader. Earlier
failures remain in this population. The other priority Sources and the five original
missing-profile cases are not relabeled as current v30 successes.

## Verification and remaining acceptance

`cargo fmt --check`, `cargo test --offline --all-targets` (48 passing, 2 ignored
subprocess workers exercised by parent tests), `cargo clippy --offline --all-targets
-- -D warnings`, and the Python suite (40 passing) all exited 0. Existing tests were
extended; no additional Rust test method was added. They cover canonical context,
unique original inventory keys, required duplicate assessment, coherent rejection
and refusal of source-free validation for bound profiles. Protocol tests do not
establish model quality. Source immutability, exact-hash local-user approval,
transactional publication, lifecycle checks and the DeepSeek boundary remain intact.
No schema migration, new dependency, production Claim or prompt-default promotion
was introduced. Phase 1/2 tags remain unchanged and no `phase-3` tag exists.

Fresh diagnostic replay and the failure-prefix replay are saved under
`reports/evidence/phase-3/v30-continuation/`. Historical o8000 replay is byte-identical
to its saved artifact. Raw request/response/accounting sidecars remain in
`reports/evidence/phase-3/semantic-repair/`. Current acceptance and a requirement-by-
requirement completion audit are saved separately; they explicitly retain incomplete
items. Credentials were excluded from and scanned out of the new evidence.

## Cost and next action

Official [Haiku pricing](https://platform.claude.com/docs/en/models/haiku-5-5/overview)
and [structured-output schema support](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)
were checked before the new calls. Each used the existing direct Anthropic transport,
shared durable ledger, request bound and immutable receipts. The v29/v30 compact
extraction inputs use far fewer tokens than v25; that reduces actual token usage,
in addition to the earlier reservation correction. No historical charge is repriced.

The six new calls cost **$0.00831170** in conservative token-price upper bounds,
not confirmed provider billing. Final accounting is prior $4.78265335 plus recorded
request amounts $0.09705470 = **$4.87970805**. The unchanged ceiling is $4.88265335;
remaining **$0.00294530**, with **0 unsettled requests**. The next required v30 review
reserves **$0.00449180**, so it was dry-run only.

The next concrete task is source-aware validation of the new nested extraction,
then repeated/profile-wide checks of the known failures and remaining priorities.
The prepared `evaluation/phase3/proposed-policy-v30/` is an **inert, unapplied**
proposal for up to $0.05 additional capacity, new cumulative ceiling $4.93265335
(still below $5). Patch applicability and guarded SQL were checked on private copies;
the real ceiling/prior/requests did not change. Its authorization template is false.
Applying it requires an explicit additional grant under the user's existing ceiling
instruction. Additional funding would allow evaluation, not guarantee its outcome.

**Phase 3 remains incomplete.** Current v30 evidence is too narrow for the original
acceptance scope; full reviewer/reader reliability and semantic coverage remain
unproven. The completion goal stays active.
