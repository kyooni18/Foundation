# Phase 3 boundary validation — incomplete, 2026-10-24

## Authorized scope and checkpoints

The human approved the pending additional $0.05 proposal: “U can use that credit.
For LLM-required testing, focus on problematic boundary.” Commit `9845016` applies
the exact guarded policy amendment to the existing ledger, preserving all 76 old
request rows and the carried prior. Starting HEAD was `493ad4b`; implementation
checkpoints are `b55e6a2` and `9c3d3f4`. Ending delivery commit contains this report.

Live work concentrated on three problematic Sources (nested report, hypothetical,
conjecture/verification), with an archived irritation batch as a Source-commentary
negative control. Stable habit/mixed-language cases were not rerun. Original data,
frozen labels, old profiles and raw results remain unchanged. Neither v32 nor v33
is promoted; no Claim is approved/published and no `phase-3` tag is created.

**Phase 3 remains incomplete.** New complete passing diagnostic traces exist for
v32 delay and v33 hypothetical. The v33 nested batch is structurally valid and has
three individually passing Source reviews, but the full gate correctly withholds
it for an inventory disagreement about the governing reporting construction.

## Boundary investigations and general corrections

### Unknown identity versus an extra assertion

V32 distinguishes person-specific Korean 신원 from an inanimate object's 식별 정보
or 정체. It also explains the difference between an identifying unknown-argument
noun phrase and an additional statement that a document omits information. The
archived v25 irritation batch explicitly appends such a document-commentary
assertion; v32 rejects it for atomicity/Source-meta/unsupported-inference failures.

A generated v32 hypothetical uses `식별되지 않은 이번 실험`. Its reviewer treats
this as changing a definite experiment into an unidentified world entity. A
separate editorial control scopes that identity phrase to the Source, and is also
rejected as commentary. A second editorial control instead binds the actual Source
expression to its original utterance (`원문에서 ‘이번 실험’으로 지칭한 실험`) and
passes. Only the identifying noun phrase differs; expected labels/authorship are
excluded from model input. These are assistant-authored controls, not independent
ground truth or generated-output success.

V33 therefore permits a stable unnamed referential role bound to the original
utterance's expression. It does not invent a real-world identity, purpose, ownership
or affected target. Its isolated-reader instructions distinguish that role from a
bare unbound pronoun. This is a general provenance/reference correction, not a
case-specific output rewrite. The actual v33 hypothetical still uses `이번 실험`
inside the original-narrator reporting frame rather than the explicit quoted-role
phrase. Its primary and bounded repeat readers pass, as do all Source reviews;
these results do not independently adjudicate every possible deictic interpretation.

### Reported content versus independent world verification

V32 nested coverage raises uncertainty because the relayed reviewer's assessment
has not been independently verified, despite a Claim preserving the report chain.
V33 explicitly distinguishes Source entailment of “A said X” from proof of X. A
faithful report is not rejected merely because its content is unverified; a bare
X or an invented membership/content gloss must still fail. Attribution, polarity,
missing arguments, Korean meaning, Source-meta expansion and duplicates remain
required. No raw uncertainty flag is removed or aggregate verdict repaired.

V33 nested extraction preserves the full professor → reviewer → assessment report,
binds the narrator's quoted possessive in both Claims, and separates disagreement.
All three Source reviews individually pass, but the first inventory label ends at
`...했다고`'s interior `...했다`, whereas the other two preserve the whole outer
`...했다고 전해주셨다` report. These are different grammatical scope boundaries.
The gate retains the disagreement rather than normalizing labels or selecting two
passing interpretations by majority.

### Structural preflight before spending on semantic review

V32 nested output contains null members inside `index_fields`, despite the contract
requiring either a null object or four string values. The Python diagnostic's
partial envelope/anchor checks missed this, so two readers and three Source reviews
were purchased before authoritative Rust validation. Those attempts remain in the
denominator and receive **no valid-extraction or gate credit**.

The diagnostic review CLI now runs the actual offline Rust structural validator on
every selected batch before dispatch, including controls. A key-free CLI audit on
the saved invalid batch rejects before key lookup/reservation; all real request
rows remain identical. Existing tests prove the real invalid example fails and a
private fixture changing only the nullable object passes; raw model output is never
edited. V33 frames prefer `index_fields=null` when text is complete and prohibit
partial objects with null values. Both new v33 batches use null and pass Rust.
The production worker already performed this structural check first; the repair
aligns the diagnostic harness with that ordering.

## All newly generated batches

| Profile / Source | Generated Claims | Actual result |
| --- | ---: | --- |
| v32 hypothetical | 1 | Structurally valid; reader pass, first Source review rejects unknown-identity wording. |
| v32 delay | 2 | Both isolated readers and all three Source reviews pass; exact reconstructed Plan hash and full Rust gate eligible. Conjecture and failure to verify remain separate, attributed to Jinwoo, with unknown affected target in both. |
| v32 nested | 2 | Rust rejects null index members; diagnostic reviews retained but ineligible. Coverage additionally unresolved. |
| v33 hypothetical | 1 | Structurally valid; reader plus three Source reviews pass; full Rust gate eligible. One identical-request reader repeat also passes. No named experiment or architecture ownership is invented. |
| v33 nested | 2 | Structurally valid; two readers and three Source reviews individually pass; full Rust gate **ineligible** for inventory disagreement. |

Denominator: **5 generated batches / 8 Claims / 3 distinct Sources**. No generated
batch is removed from the population because it failed. Complete passing diagnostic
traces: **2/5**, not an independently adjudicated semantic success rate. Prior v30
passing habit, ambiguous-reference and mixed-language traces remain historical v30
evidence, not relabeled as v32/v33 runs. The five missing current-profile cases stay
unassessed and the six development regressions are not fresh holdouts.

## Reviews, controls and repeat behavior

New v32 Source reviews: **11 total — 6 pass, 3 reject, 2 unresolved**. Five isolated
readers pass. The three rejects include the generated hypothetical, Source-scoped
identity control and archived Source-commentary batch. The exact Source-role
editorial control passes; the old bad nested gloss is unresolved, not credited as
a correct rejection. Delay has exact complete Source-review agreement.

New v33 Source reviews: **7 total — 6 pass, 1 incomplete**. Four isolated readers
pass, including the single bounded repeat. The old bad nested control hits
`max_tokens`: 3,000 output tokens include **1,990 thinking tokens**, leaving too
little visible output for its full structured review. Its raw partial response,
usage settlement and incomplete rejection are retained. No retry was purchased;
it is neither a semantic rejection nor a pass. It exposes a remaining response-
budget/negative-control limitation under this profile.

The two actual v33 hypothetical isolated-reader requests have identical complete
payloads and identical passing judgments (**1/1 verdict and complete agreement**).
This measures the source-blind reader only, not nine-dimension Source-review
reliability. The nested Source inventory disagreement remains **unresolved** even
though all its per-dimension statuses pass. No majority vote or synthetic consensus.
No general false-acceptance/false-rejection rate is claimed from these small,
selected and partly editorial controls. Full current-profile negative-control
coverage and broader Korean reliability are still insufficient.

## Verification and evidence

All final required commands exit **0**, with logs/exit codes in
`reports/evidence/phase-3/boundary-continuation/verification/`:

- `cargo fmt --check`
- `cargo test --offline --all-targets`: 48 pass; two ignored subprocess workers
  are exercised by their parents.
- `cargo clippy --offline --all-targets -- -D warnings`
- Python discovery: 40 pass.

Existing Rust/Python tests were extended; no new test method, dependency or migration.
The new preflight helper invokes the authoritative Rust validator rather than
reimplementing its schema. The complete archive replay includes **24 extractions,
55 Source reviews and 19 isolated readers** across six original Sources, including
this round's incomplete response and earlier protocol-invalid judgments.

New traces, exact hashes, preflight audit, reader pair and inventory comparison are
in `reports/evidence/phase-3/boundary-continuation/`. Actual raw API artifacts remain
in `semantic-repair/`. Separate canonical comparisons do not rewrite raw responses.
`completion-audit.json` records each still-unproved acceptance obligation.

Source immutability, exact Unicode quote/provenance checks, auth/recorder separation,
DeepSeek English boundary, lifecycle, transactional publication and user-only
exact-hash approval remain intact. No Phase 4 feature is added. Phase 1/2 tags and
the preexisting `.yeet/` and `UI-Agent-Brief.md` are untouched.

## Conservative accounting

Same ledger, no reset/repricing/alternate account. Each paid call reserves its
request's worst-case bound first. Current Anthropic schema/pricing checks remain
those verified on 2026-10-24; thinking is included in output accounting. Provider-
billed USD remains unknown.

| Item | Conservative USD |
| --- | ---: |
| Cumulative before this grant | $4.92882355 |
| New capacity approved | $0.05000000 |
| Previously unused headroom carried into this round | $0.00382980 |
| This round, 32 settled calls | $0.05000250 |
| Recorded request total, 108 settled calls | $0.19617270 |
| Carried prior, unchanged | $4.78265335 |
| Cumulative bound | **$4.97882605** |
| Approved ceiling | **$4.98265335** |
| Remaining approved headroom | **$0.00382730** |

The round's $0.00000250 above newly added capacity uses previously authorized unused
headroom; cumulative accounting remains below the approved ceiling and strictly
below $5. Zero unsettled requests and zero reservation overruns. One of the 32 calls
is the retained truncation; 31 complete. No blocking condition is bypassed to fit
another Source review into the remaining headroom.

## Remaining acceptance work

The nested inventory must preserve a complete governing-report boundary consistently;
passing dimension labels cannot override that disagreement. Full negative-control
responses and stronger current-profile repeat/semantic evidence remain needed.
The hypothetical's external entity is still unnamed; model passes and the narrator
frame do not supply independent ground truth. Structural preflight fixes prevent
wasted review, but are not semantic acceptance proof. No default promotion or Phase 3
completion is justified by the present evidence. The full original goal remains
active and incomplete.
