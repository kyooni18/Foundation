# Phase 3 — implementation checkpoint; semantic acceptance blocked

Status: **not accepted; no `phase-3` tag**. The implementation and offline regression
suite are complete as a reviewable checkpoint. Real OpenRouter experiments were run
and prompts revised through v11. An earlier run was blocked by HTTP 402
(account credits exhausted; the key remained valid without a per-key cap). The user
then authorized at most $5 additional evaluation spending, and funded access was
verified. Frozen v11 evaluations resumed with a shared conservative reservation guard.
Only cheap models were used after the user requested that restriction. The fresh
20-source extraction benchmark has not yet been run. The user independently confirmed
that each Claim must explicitly describe an unknown referent; the exception-scope
review remains pending. Neither fake-provider tests nor reviewer agreement establishes
semantic acceptance. The historical experiments below remain reproducible evidence.

The first live pilot completed 36 extractions (12 development cases × 3 variants)
with `anthropic/claude-sonnet-5.5` and independent reviews by
`google/gemini-3.8-flash`. Raw outputs are in
`reports/evidence/phase-3/live-v1/pilot.jsonl`. A diagnostic disagreement is preserved
in `negation-diagnostic.json`: three review stages rejected a faithful negated
conjunction, while a separate diagnostic request to the same reviewer called it
faithful. This is evidence of inconsistent judgment, not proof from model agreement.
The broader repeated matrices and concrete failures are documented below.
No numerical semantic acceptance claim is made from unfinished experiments.

Consequently this report does not claim that the prompts meet semantic acceptance,
that reviewers improve quality, or that English intermediate processing helps Korean.
All publishable candidate batches remain quarantined until explicit local-user fidelity review.
Approval is separate from confirmation/endorsement. There is no configuration switch
that silently promotes model agreement into production acceptance.

## Implementation and architectural boundaries

`src/jobs.rs` enqueues one initial job in the Source insert transaction. Idempotent
Source retries do not create duplicate jobs. Explicit backfill queues old Sources;
re-extraction has a distinct source-scoped idempotency key. The job table is operational;
Sources, requests, candidates, runs, Claims, mentions, dependencies, and status events
remain immutable or append-only.

A worker atomically leases a job under the SQLite writer transaction. Owner, attempt
number, and expiration fence every completion. Expired jobs can be reclaimed; a worker
from an older lease cannot commit. Three attempts are allowed, with bounded exponential
backoff (10, 20, 40 seconds). Exhausted expired leases become failed. Provider failures,
timeouts, structural errors, atomicity/fidelity/coverage rejection, unresolved review,
and lifecycle refusal retain the original Source. Inspection includes attempts, budget,
next eligible time, lease expiration, and sanitized failure codes. Provider messages and
source excerpts are not copied into job failure codes. Bounded failed proposals and received
reviews are retained separately as private, untrusted local inspection diagnostics (D-49).

Retrospective validation retains the original live verdict and records the later check
separately. Rechecking all saved v4 attempts with source checks v2 rejects two earlier
structurally valid Korean-to-English drift batches; it adds no rejection in the saved v3
or v6 calibration outputs. This is deterministic language evidence, not a full fidelity score.

The first attempt freezes model configuration, prompt hash, and preceding accepted job.
Retries cannot substitute a new model or edited prompt into that request. A changed plan
requires a new re-extraction job. The previous failure code is supplied as bounded repair
feedback; the model must reconsider the entire original source, not drop difficult Claims.
The implementation does not promise that this repair policy is effective without measurement.

`LlmProvider` is independent of storage and transports. Implementations include a scripted
fake and a real OpenRouter HTTPS chat-completions adapter. Model and reviewer are explicit.
JSON output is requested, then validated locally. Requests have deadlines, and a separate
worker-wide deadline also bounds stalled implementations. Provider credentials are not
serialized into plans, prompts, database rows, errors, or API output. Credentials may come
from the environment or a canonical 0600, same-owner, single-link regular file; insecure
files and symlink components fail closed. D-24 rejects non-ASCII DeepSeek payloads before
network I/O, including non-English instructions or review context.

The optional `serve --extraction-model … --extraction-reviewer …` worker shares the server's
sole database owner. Recording remains independent of provider success. `extraction-work`
is a one-job offline maintenance runner. Without provider configuration, normal recording
still queues durable work. Process shutdown may leave a lease, recovered by expiration.

A successful model proposal undergoes deterministic validation and three source-grounded
calls: atomicity, semantic fidelity and whole-batch coverage/duplication. V11 additionally
reviews each Claim text alone first, without Source, neighbors, anchors or speaker metadata,
at bounded concurrency four. Original-source reviews still receive the complete original.
All responses must pass their complete index protocol. An unresolved or failed review
rejects the batch. Standalone coverage means its one provided text was fully reviewed;
only the later original-source calls assess coverage of the Source. These calls are logically separate reviews,
not evidence of statistically independent model errors. Self-reported confidence is unused.

Even passing proposals become immutable `extraction_candidate` rows with an exact output
hash and review responses. They create **no Claims**. Local-user approval must name that
candidate and hash. It checks that candidate’s own complete review protocol and frozen
prompt hash, revalidates against original bytes, and checks lifecycle and version
freshness before a single transaction inserts all Claims, mentions, declared dependencies,
the extraction run, and the completion state. A failure rolls the entire publication back.
Repeated approval returns the same sorted IDs, including successful empty extraction.

Re-extraction retains historical Claims. A run-level prior-job link represents whole-batch
versions, splits, and empty replacements; `revision_of` must reference a Claim from the
preceding accepted run of that same Source. Prior Claim IDs/text are supplied as untrusted
revision context, not source evidence. Default lists show the current run; agents can read
non-forgotten historical Claims by ID. No factual contradiction or temporal supersession
is inferred. Stale competing jobs cannot replace a newer accepted version.

D-35–D-55 in `DECISIONS.md` document the migration, quarantine, revision, language, projection,
and lifecycle resolutions with alternatives. In particular:

- A Source with forgotten historical Claims cannot publish a new extraction. The worker
  checks before sending prior context and publication checks again for races. A live Source
  may be explicitly re-extracted to correct a retracted wrong interpretation (D-50), without
  restoring the old Claim or its confirmation. Source retraction still blocks publication.
- Dependencies are same-batch declarations with cycle rejection. Phase 4 validity propagation
  is not implemented. Such batches are conservatively absent from the user projection and
  marked `needs_revalidation` in the agent view, even if a Claim is confirmed.
- No semantic search, embeddings, entities, conflict detection, or extraction-based truth
  adjudication was added.

## Deterministic contract validation

The JSON envelope requires every declared field, including explicit nullable fields, and
rejects unknown fields. Candidate structure, enums, sizes, duplicate text, temporal ordering,
observed/artifact requirements, mention spans, dependency bounds, and cycles are checked.
Every candidate requires an exact source quote and occurrence. The host computes UTF-8 byte
ranges and verifies boundaries/substrings; model-generated offsets are not trusted. Quotes
are checked against the immutable original, never an English intermediate representation.
`verbatim` additionally requires unchanged anchor text; framed paraphrases must identify
that basis. This deliberately prevents paraphrase from acquiring automatic user visibility.

Final `ko`/`en` language is checked against the anchor's script; spans solely in other
writing systems are rejected, while mixed proper names require semantic review. This is a coarse guard, not a complete language detector: e.g. distinguishing
English from another Latin-script language requires semantic/local review. Unknown-language
cases must not be relabeled English. Other BCP-47 languages await a tested extension.

Temporal fields remain distinct. Only supplied `authored_at` grounds relative-time
resolution; ingestion time is never passed as an authored-time substitute. Unspecified
bounds remain null. Essential attribution, modality, conditions, temporal scope, and
unknown references belong in Claim text rather than only metadata. These semantic
requirements are prompt/review requirements, not proven by a JSON parser.

SQLite additionally rejects incorrect evidence, verbatim mismatch, unsupported observed
basis, cross-source revision references, and cyclic declarations. Integrity inspection
checks exact Claim evidence, candidate hashes, foreign keys, publication/run consistency,
required immutability triggers, original Source hashes, and migration history.

## Authorship and projection isolation

The authenticated Principal remains the sole authority for Source recorder. Models cannot
set recorder, endorsements, authorization state, or credentials. A declared user speaker,
verbatim basis, retention request, or quoted user statement in an agent Source cannot grant
user visibility.

The user Claim predicate is live Source + live Claim + conservative dependency validity +
(user-recorded, user-speaker, verified verbatim **or** explicit user confirmation). Confirm
and lifecycle are independent axes: confirming a retracted Claim does not restore it.
Only the user can confirm/unconfirm. Import cannot mutate lifecycle. Agent scope follows
the existing Phase 2 role rule; labels never become authorization facts.

User results omit raw anchor passages/offsets, free-form applies_from/applies_to dates,
free-text model metadata and extractor/job
provenance. A broad anchor can contain unconfirmed adjacent text. Essential meaning must
already be in the approved Claim. The agent/audit view retains evidence and provenance,
with forgotten revision targets redacted. Counts and cursor continuation are computed
inside the visible Claim set. Cursors bind role, credential, options, and a Claim-specific
domain, and cannot be replayed as Source cursors. Errors for hidden and missing reads are
identical. No alias, derived-edge, summary, or search surface was introduced that could
expose hidden Claims.

HTTP and MCP add `claim_get`, `claim_list`, `claim_retract`, `claim_forget`, `claim_confirm`,
and `claim_unconfirm` through the same application dispatcher. Model proposal approval is
restricted to local maintenance, not an agent tool. Existing five Source tools are preserved.

## Prompt research and evaluation design

The historical R1/R2 experiments in `~/Code/Foundation-Research/` were inspected before
implementation. R2's downstream reader scored 0.667 on bare text, 0.869 with fields,
0.935 on framed text and 0.952 with anchors, versus 0.964 on original text. These are
historical reader-task scores, not Phase 3 atomicity measurements. R2 also documented
Korean-to-English drift, literalized irony, unresolved references and validators sharing
extractor errors. The original R2 prompt and summaries remain under `evaluation/phase3/`.

The seven initial strategies were actually exercised on the same development cases:

| Strategy | Distinction |
|---|---|
| single | Contract-only direct extraction |
| r2-frame | Historical framing adapted to the current output/language contract |
| decompose | Proposition inventory, semantic independence and frame reconstruction |
| english-intermediate | Instruction to reason through English; hidden processing is not observable |
| atomicity-review | Decomposition plus one separate atomicity review |
| fidelity-review | Decomposition plus one separate fidelity review |
| audited | Decomposition plus atomicity, fidelity and whole-batch coverage reviews |

The adapted R2 baseline is not a byte-identical R2 replication. R2 supplied date-only
`note date` context; the adapted fixtures encode those dates as synthetic midnight-UTC
reference instants. These are benchmark assumptions, not evidence of actual author clock
time or calendar timezone. All compared strategies receive the same fixtures. Production always applies
all three reviews; stage ablations are research conditions, not alternate authorization
rules. No model confidence score or majority vote establishes correctness.

Prompt revisions are immutable assets, selected explicitly in the frozen plan:

- **v1:** direct/frame/decomposition comparisons with contrastive atomicity examples.
- **v2:** response to observed counterfactual historical inventions, causal relationship
  loss, correction duplication and false rejection of scope-preserving explanations.
- **v3:** omitted subjects, quoted-instruction coverage, shorthand speech-act ambiguity,
  and valid RFC 3339 instant fields instead of invented date-only bounds.
- **v4:** response to a source audit finding unresolved first person, lost project context
  and disconnected rationale. A further real output translated a Korean technical clause
  into English despite three passing reviews; source checks v2 now reject that clear case.
- **v5:** preserves unknown authored calendar timezone. UTC normalization preserves an
  instant but cannot recover whether “yesterday” meant a UTC or Korean-calendar day.
  Explicit source UTC/KST calendar references permit resolution; no inference from language.
- **v6:** evidence-calibrated pragmatics distinguish supported praise, contextual irony and
  unresolved isolated evaluations. Each derived date retains its calendar scope. Review
  calibration distinguishes preserved source uncertainty from uncertain extraction fidelity.
  The focused 20-case, two-run Luna Pro calibration produced 54 attempts, 37 terminal gate
  passes and three exhausted runs. These are model diagnostics, not semantic acceptance.
- **v7:** targeted response to held-out loss of an explicit unverified-status frame when
  split from its report. Four same-target English/Korean control runs lost the frame under
  v6; all four preserved it under v7. Four unrelated-target control runs remained correctly
  split under both. These are narrow assistant source audits, not independent human gold.
- **v8:** reorganizes the prompt around an inventory and frame-binding procedure before
  decomposition. V6 long-form output had disconnected an exception from its rule and left
  a known policy referent unresolved. Matched controls and exposed regressions are under
  evaluation. The procedure is instructional, not an observed or trusted intermediate.
- **v9:** adds an explicit behavioral counterexample distinguishing a positive IF rule
  from a negative exception, and binds deictic time to the original statement. Matched
  reviewer controls removed the known false passes in those narrow cases; false rejections
  remain, and no population error rate is inferred.
- **v10:** retains that semantic procedure and supplies host-computed candidate count and
  required indices to each review. V8 long-form reviews omitted the eleventh candidate;
  this is an enumeration aid, not automatic approval. Older frozen profiles retain their
  previous payload shape.
- **v11:** adds the measured one-Claim-at-a-time isolation reader, followed by all three
  original-source reviews. The reader prompt itself is included in provenance hashing.
  A new profile preserves old request hashes and review protocols; real extraction
  evaluation remains necessary before selection.

The live v1 negated-conjunction diagnostic is instructive: three reviewer calls rejected
an apparently faithful explanation, then a separate diagnostic call to the same model
called it faithful. Both outcomes are retained. Review agreement was not treated as proof.

An additional **english-staged** research condition saves an actual English semantic draft,
its exact source quotations and prompt hash, then reconstructs with the original source
and explicitly untrusted draft. Final reviews always receive the original. This allows
inspection of transformation errors, unlike an instruction to reason internally in English.
It is evaluation-only; the production worker is not silently changed into this architecture.
The initial JSON-mode request error and corrected pilot are retained separately.

Early runs used Sonnet 5.5/Gemini 3.8 Flash. After HTTP 402, the user replenished credits and
requested cheap models. Subsequent runs use GPT-6 Luna and Claude Haiku 5.5. The inspected
OpenRouter catalog quotes both at $0.10/M input and $0.50/M output for these payload sizes.
A focused Luna Pro comparison uses the same catalog price, prompt and reviewer. No further
expensive-model run is authorized by this selection. DeepSeek is not used on Korean inputs:
D-24's English-only restriction still applies, including review context.

Completed and ongoing raw experiments are retained under `reports/evidence/phase-3/`.
The first pair completed a 36-extraction pilot, 448 v1 curated development runs, 192 v2
curated runs and 32 v2 long-form runs. The cheap pair repeats the seven-way v1 comparison
on 36 development sources twice, compares three v3 strategies on the same sources, and
runs explicit English drafts and R1/R2 legacy comparisons. V4/V5 additionally measure
up to three bounded attempts per independent run. Every attempt is retained; retry success
does not erase the preceding failure. Terminal-only and full-attempt summaries are distinct.
The final experiment manifest records completed counts rather than inferring success from
an output file's existence.

## Corpus, annotations and metrics

The original v6 round contains **108 sources: 88 development and 20 held-out**:

| Corpus | Development | Held-out | Reference status |
|---|---:|---:|---|
| Legacy R1/R2 | 48 | 0 | Original criteria plus assistant editorial proposition inventories |
| New curated | 32 | 16 | Human-reviewable editorial semantic inventories |
| Long form | 4 | 4 | 306–406 characters, mixed independent and dependent propositions |
| Calendar controls | 4 | 0 | Reproducible UTC/KST normalization ambiguity and explicit-calendar cases |

The long-form extension addresses a measured gap: the original 96 sources were all shorter
than 100 characters. Sources include shared subjects, conjunction/disjunction, negated
conjunctions, nested conditions/quotations, uncertainty, suggestions/decisions, corrections,
causality, temporal changes, sarcasm, exaggeration, shorthand, mixed technical notes,
repetition, contextual references, injection and empty extraction. Development references
were corrected where the initial labels themselves forced an unsupported interpretation;
source bytes did not change. These inventories are not independently adjudicated gold.
The v6 held-out run exposed semantic failures; all 20 of its sources are consequently
regression material for subsequent revisions, not a fresh benchmark. Eight new attachment
controls bring the next development/regression collection to 116 sources. A separate
`heldout-v10.json` contains 20 newly authored cases (14 Korean, four English, two mixed),
created after the v10 prompt assets were frozen and reserved until profile selection.
There are therefore 136 unique corpus sources across rounds. Small paired reviewer-control
fixtures are additional diagnostics, not counted as independent extraction sources.
Every new reference remains an editorial draft; no independent human gold is fabricated.

`score.py` requires complete per-Claim boolean annotations, validates denominators, rejects
duplicate run/attempt identities and keeps model, prompt version, source language and error
category separate. Missing labels are unreviewed, never passes. It reports generated and
eligible-output errors, meaningful coverage (rejected batches contribute zero), over-splits,
unsupported assertions, framing, language, evidence, duplication and reviewer confusion.
Lexical repeatability is measured separately from semantic equivalence. Translation-stage
errors require inspecting the saved original/English/final triple; output disagreement
alone cannot establish language-specific causation.

The first complete **assistant source audit** covers all 64 v3 audited curated runs. Its
labels and original outputs are retained in `luna-v3-assistant-audit.jsonl`, with metrics
in `luna-v3-assistant-metrics.json` and readable source/output pairs in `luna-v3-review.md`.
These are provisional assistant judgments, not independent human adjudication. Here
“eligible” means the experimental three-review gate passed; it does not mean publication.

| v3 audit metric | Korean | English | Mixed |
|---|---:|---:|---:|
| Runs audited | 56 | 4 | 4 |
| Generated Claims | 82 | 6 | 8 |
| Known generated atomicity violations | 0/82 | 0/6 | 0/8 |
| Known generated unsupported assertions | 0/82 | 0/6 | 0/8 |
| Generated fidelity/context errors | 8/82 | 0/6 | 2/8 |
| Generated epistemic/frame errors | 8/82 | 0/6 | 0/8 |
| Generated language-label errors in this subset | 0/82 | 0/6 | 0/8 |
| Exact-anchor/verbatim contract errors | 3/82 | 1/6 | 0/8 |
| Known generated duplicate Claims | 0/82 | 0/6 | 0/8 |
| Coverage before the experimental gate | 70/78 | 6/6 | 6/8 |
| Coverage after the experimental gate | 41/78 | 4/6 | 6/8 |
| Fidelity errors still eligible after the gate | 3/46 | 0/4 | 2/8 |
| Meaning-changing over-splits among eligible decompositions | 1/10 | 0/1 | 0/4 |

This audit **fails** the semantic acceptance target. Five gate-eligible Claims retained
known context errors: first person, project ownership and shorthand rationale. No such
experimental output was automatically published. Zero entries elsewhere are limited to
this small provisional audit; they are not universal guarantees. English-intermediate
causal error rates are not fabricated from the direct-run sample (their denominator is zero).

The audit also found false positives: a faithful password question was rejected as losing
uncertainty, although it matches the user's positive example. Across its 58 structurally
valid batches, provisional TP/FP/FN/TN counts were 2/8/6/42 for atomicity, 1/8/7/42 for
fidelity, and 2/8/6/42 for coverage. The small, selected sample and correlated reviewer
calls preclude population estimates. The important finding is concrete: both false
acceptance and false rejection occur, so model agreement is not a publication authority.

The separate synthetic reviewer controls contain six good/corrupted pairs, three stages
and two repetitions, for 144 reviews across v1/v2. V1 returned 35 true negatives, 36 true
positives and one provider error; v2 returned 36 true negatives and 36 true positives.
These simple controls did not predict the natural-case errors above. They are diagnostic,
not evidence that reviewer false-positive or false-negative rates are universally zero.

The completed v6 comparison uses 88 development and 20 held-out sources, each twice, with
matched cheap models/configuration for `r2-frame` and `audited`. It is a genuine held-out
run for v6. Audited development produced 240 attempts for 176 terminal runs: 164 model-gate
passes, 29 successes after repair and 12 exhausted runs. Audited held-out produced 75
attempts for 40 terminal runs: 29 gate passes, nine repaired successes and 11 exhausted
runs. Only three of eight long-form held-out runs passed. The adapted R2 structural-only
condition passed 176/176 development and 40/40 held-out after anchor retries; it has no
semantic reviews, and those structural pass counts do not establish fidelity. Held-out
reviews also had nine InvalidOutput call failures and one Unavailable error. Earlier
experimental repair feedback grouped such review errors as provider failures; the corrected
harness distinguishes their production-equivalent sanitized classes without rewriting old
rows. Detailed language/category diagnostics and every attempt are retained.

Its exposed failures are regression material for v7/v8, **not** fresh held-out
evidence for those later revisions. Concrete observed failures include:

| Original requirement | Observed v6 audited output defect |
|---|---|
| `new-042`: tentative report explicitly unverified | Split report from lack of checking; both runs passed all model gates |
| `new-047`: heard access allowed, no direct permission | Initial one-Claim reference was overmerged; separate hearsay and direct-access-permission status are acceptable (D-57) |
| `long-007`: requeue rule with completed-work exception | Broad requeue Claim lost exception into a separate neighbor |
| `long-007`: opinion about a described policy | “this policy” remained unresolved in the opinion Claim |
| `new-043`: writing-time lack of current-system statement | “now” remained relative to the reader rather than the original statement |

The adapted R2 baseline also invented dates from its historical example `오늘 -> 2026-10-11`
when authored_at was null. The original example is preserved rather than silently fixed
inside the baseline. This is prompt-example contamination, not legitimate temporal
resolution. R2's user-note assumption also does not justify user identity for arbitrary
new source material. These failures are visible in the raw comparison; the baseline is
explicitly an adaptation with historical assumptions, not a production recommendation.

Four new epistemic-attachment controls, each twice, reproduced v6's loss in all four
same-target runs and preserved the correct split in all four unrelated-target runs.
V7 retained the explicit frame in all four same-target runs without falsely attaching it
in the unrelated-target controls. The complete paired outputs and provisional assistant
labels are retained in `attachment-*.jsonl.gz` and `attachment-assistant-metrics.json`.
The narrow result supports the targeted revision; it does not establish general quality.
The broader v8 scope-binding experiment remains subject to its actual outputs and a fresh
benchmark after prompt selection. No final zero-error acceptance is asserted.

## Reproduction and operational use

Configure an actual authorized model and an independent reviewer, and provide credentials
through an existing canonical 0600 file (`OPENROUTER_API_KEY_FILE`) or process environment
(`OPENROUTER_API_KEY`). Never put the secret on the command line or in the repository.

```sh
cargo run --offline --example evaluate_extraction -- \
  --corpus evaluation/phase3/curated.json --split dev \
  --model "$EXTRACTION_MODEL" --reviewer "$REVIEW_MODEL" \
  --prompt-version "$PROMPT_VERSION" --max-output-tokens 16384 \
  --review-max-output-tokens 4096 --repair-attempts 3 \
  --runs 2 --output /absolute/private/path/dev.jsonl
```

Prompt selection must be explicit: omission retains v1 serialization for compatibility
with previously frozen requests. V6 is a failed acceptance candidate; v7–v11 are subsequent
experiments, not accepted defaults. Current low-cost experiments use
`openai/gpt-6-luna-pro` and `anthropic/claude-haiku-5.5`.

Run the same development comparison on `legacy.json`. Review concrete failures, revise
prompts and draft annotations, and rerun affected development cases. Freeze the selected
variant before evaluating the held-out split against `r2-frame` with repeated runs:

```sh
cargo run --offline --example evaluate_extraction -- \
  --corpus evaluation/phase3/curated.json --split heldout \
  --model "$EXTRACTION_MODEL" --reviewer "$REVIEW_MODEL" \
  --prompt-version "$PROMPT_VERSION" --max-output-tokens 16384 \
  --review-max-output-tokens 4096 --repair-attempts 3 \
  --variants r2-frame,audited --runs 2 --output /absolute/private/path/heldout.jsonl
python3 evaluation/phase3/score.py /absolute/private/path/adjudicated.jsonl
```

`--offline` applies to Cargo dependency resolution; the example deliberately makes provider
network requests. Do not run evaluations against private production Sources by accident.
Development/held-out scoring consumes explicit human annotations documented in `score.py`.
Replace `audited` in the held-out command with the development winner if it differs.

For local operation (CLI is the existing trusted maintenance boundary):

```sh
primeradiant --db /path/to/db extraction-backfill
primeradiant --db /path/to/db extraction-work --model "$EXTRACTION_MODEL" --reviewer "$REVIEW_MODEL"
primeradiant --db /path/to/db extraction-inspect
primeradiant --db /path/to/db extraction-approve CANDIDATE_ID EXACT_OUTPUT_SHA256
primeradiant --db /path/to/db claims --principal user
primeradiant --db /path/to/db extraction-request SOURCE_ID NEW_IDEMPOTENCY_KEY
```

Review the entire original Source, candidate, anchors, and review findings before approval.
The inspect output includes the Source ID for fetching the original. Approval acknowledges
extraction fidelity, not truth; `claim-status CLAIM_ID confirm` is a separate deliberate act.
Stop the server before using offline maintenance on its database. To process new records
while serving, configure the optional worker on `serve`; it shares the same core/database owner.

## Validation evidence

The current offline suite passes **40 top-level tests**. Two ignored helper tests are explicitly
launched by their parent crash tests, not omitted acceptance work. Coverage includes:

- All Phase 1/2 persistence, authorization, credential, HTTP, and MCP regressions.
- Atomic job insertion/backfill; concurrent leasing; expiry/fencing; bounded attempts;
  provider failure, timeout, and explicit empty success; frozen plan hash changes.
- Schema/enum/field failures, Korean and combining-character UTF-8 anchors, exact evidence,
  dependency checks, missing artifact handling, and authority-field rejection.
- Batch rollback after a later invalid revision, idempotent approval, historical preservation,
  stale competing runs, prevention of forgotten-Claim resurrection, and correction of a
  retracted interpretation without restoring it. Cross-attempt approval cannot borrow reviews.
- User/agent provenance, confirmation versus lifecycle, filtered counts, pagination isolation,
  cross-domain cursors, and actual nonempty HTTP/MCP Claim results/errors.
- 20 original Source SIGKILL/restart cycles; leased-worker SIGKILL recovery; three process
  kills at the publication boundary, allowing only zero or the complete 50-Claim batch,
  followed by idempotent replay to exactly 50 Claims.
- A populated schema-v1 fixture with lifecycle history migrates without changing Source bytes
  or event sequence/reason; the pre-v2 backup remains readable as schema v1.

Commands passed: `cargo fmt --check`, `cargo test --offline --all-targets`, and
`cargo clippy --offline --all-targets -- -D warnings`. See `reports/evidence/phase-3/` for
captured logs and CLI smoke evidence. Dependencies for HTTPS were fetched once; the stated
verification commands subsequently ran offline. No real-provider test is included in the
offline suite. Fake-provider success validates orchestration and gates, not model semantics.

Bugs found and fixed during verification: approval replay ID ordering; a test accidentally
selecting a backup lock file rather than the `.bak`; credential test paths traversing the
macOS `/var` symlink; broad-anchor metadata leakage; prompt/config provenance freezing;
and re-extraction potentially bypassing historical lifecycle restrictions.

A second live smoke exercise used `serve` with the v6 Luna Pro/Haiku worker, explicit
16,384/4,096 output limits, and an agent-recorded Korean report attributing two statements
to the user. Recording through HTTP queued work; before local fidelity approval there were
zero Claims. After source inspection, exact-hash approval published two Claims and replay
returned the same IDs. Both had `speaker=user`, but authoritative recorder remained agent
and the user Claim count stayed zero. HTTP and actual stdio MCP matched; explicit user
confirmation exposed one Claim, retraction filtered the other, and Source forgetting hid
both. Database integrity checks passed. `live-http-mcp-v6-smoke.json` retains synthetic
request/response evidence without credentials. This verifies interoperability and isolation,
not corpus-wide semantic quality.

## Remaining acceptance work and limitations

1. Complete the ongoing cheap-model development/legacy comparisons and inspect actual
   failures. Provider access is working and funding was replenished; no model-quality
   acceptance can be inferred from offline tests.
2. Independently adjudicate semantic inventories, run the held-out baseline comparison,
   measure every listed rate and reviewer error mode, and revise general mechanisms when
   defects appear. Neither Korean-direct nor English-intermediate processing is yet selected
   empirically. A smaller pipeline may outperform the provisional audited one.
3. Local review currently gates every proposed batch. This is a safe review workflow, not
   fully automatic trustworthy knowledge generation. Review quality itself is not infallible.
4. Languages beyond Korean/English, cross-batch dependency declarations/validity semantics,
   automatic lifecycle mapping across re-extraction, and semantic repeatability scoring are
   not implemented. Phase 4 must address dependency and endorsement-history semantics.
5. Entire Sources are sent as context. There is no validated long-document chunking strategy;
   provider context-limit failures remain recoverable failures, not partial extraction.
6. Claim pagination materializes the visible set before slicing. It is isolated and stable
   by ID, but not optimized for large stores. Timing is not a constant-time privacy guarantee.
7. Live provider interoperability and a complete synthetic CLI lifecycle have now been
   exercised. Semantic acceptance, context limits and prompt-injection robustness remain
   under evaluation; passing selected examples does not establish universal correctness.

Preserved checkpoints: `phase-1` = `50dbccb34fc3b5960fe02de2af56c8c33f7b5916`,
`phase-2` = `ec11625f53f1bc12cb1f9e3e9849044f8e0d7d0d`. Implementation commits begin with
`482a897` and `d641815`; the following documentation commit records this evidence. No
completion tag should be created until real-model semantic acceptance is satisfied.


### Additional metadata regression
A simulated untrusted proposal copied a separate unconfirmed schedule into a confirmed
server Claim's temporal metadata. The prior user HTTP/MCP response leaked that date;
the updated projection returns null for both date fields while retaining all temporal
meaning in text. Agent/audit responses retain dates, now normalized to UTC milliseconds
on new publication as required by the existing data contract. Raw proposals and anchor
bytes are unchanged. `date-projection-before.txt`, `date-normalization-before.txt` and
`date-projection-after.txt` preserve the two reproduced failures and passing regression.


### Standalone reader experiments
Source-aware reference controls could accept an unresolved pronoun by filling its meaning
from the Source or another candidate. A standalone reader instead receives only one text
and structural index hints. Its 22 calibration controls, twice, yielded 24 TN/20 TP for each
of Haiku, Luna Pro and Qwen. Some controls overlap prompt examples; this is calibration.
On a separately authored 16-control reader held-out set, twice, all models rejected all
16 deliberately bad instances. Valid-instance false rejections were 4/16 Haiku and 1/16
each Luna Pro/Qwen. Those are editorial diagnostic judgments, not independent human gold
or population rates. The prompts and input isolation changed together, so their individual
causal contributions are not separated. A source-aware approval-target control also had
potential duplication/atomicity confounds; its provisional “valid” label is not used as
clean evidence of reviewer false positives. Independent Korean review remains requested.

The opt-in v11 core protocol requires N standalone responses followed by three original-
source responses. Missing standalone evidence prevents both quarantine approval and
publication. Fake-provider tests inspect the actual context-free requests and verify
rejection publishes no Claims while preserving the Source. Passing fake tests establishes
orchestration only; the real v11 pilot remains a separate experiment. Reader calls share
the existing worker-wide deadline, and total review bytes are bounded. Very large batches
can still fail that explicit deadline rather than partially persist.


### Reference correction: permission is not verification
The draft `new-047` inventory initially merged “접근해도 된다고 들었지만 직접 허가를 받은 건
아니다.” into one Claim. Review using the operational independence test distinguishes a
reported hearing from the independent receipt of direct permission. Two Claims are
acceptable when each keeps the original speaker and the access target, and the first
remains hearsay rather than an asserted permission. Absence of a permission-grant act is
not automatically an epistemic limitation on whether the hearing occurred. This differs
from the user's explicitly unverified-hypothesis example. The source bytes and historical
run/reference snapshots remain unchanged; only the editorial inventory is corrected.
No source-specific production exception was added. A bare “permission was not received”
still loses the known access scope and must not borrow it from a neighboring Claim.


### Final v11 checkpoint and interrupted evaluation
The 20-case, two-run v11 pilot produced 66 attempts: 29 automated gate passes, three
atomicity failures, one fidelity failure, one timeout, and six quota terminations.
The 116-case, two-run development/regression matrix produced 651 attempts before
completion of its scheduled rows: 25 gate passes, one fidelity failure, and 206
quota terminations. These are operational outcomes, not semantic accuracy rates.
A terminal quota row does not mean its Source was evaluated. Raw outputs, frozen
manifests and diagnostics are retained in `pro-v11-*.jsonl.gz` and adjacent files.

The runner previously retried quota failures and continued scheduling later shards.
It now saves the quota diagnostic, stops that shard immediately, stops dispatching
pending shards after any failed child, and marks the matrix incomplete. Already
running shards can finish and preserve their artifacts. No aggregate success file is
created for an incomplete matrix. A deterministic Python regression verifies that
one failed shard prevents the remaining queued work from launching. Production job
retry/lease semantics are unchanged. No new paid calls were used for this regression.

`live-http-mcp-v11-smoke.json` records the actual optional HTTP extraction worker,
quarantine, exact-hash approval, idempotent publication, HTTP/MCP equivalence, explicit
confirmation, retraction, Source forgetting and integrity checks on two synthetic
reported Claims. The authenticated agent remained the recorder; neither Claim was
visible in the user projection before confirmation. This validates integration and
authorization, not corpus-wide semantic correctness.

To finish acceptance: replenish access with a bounded additional evaluation budget;
resume the interrupted development cases as a separate preserved experiment; audit
concrete outputs and resolve outstanding Korean judgments; freeze the selected
strategy; run the reserved fresh benchmark and repeated comparison; record metrics
and any failures. If these checks expose a general defect, revise and regress before
considering the completion tag. All historical evidence and prior Git tags remain.


### User review and bounded funded continuation
The user rejected a Claim that used only quoted “걔” and required the unknown person
to be described explicitly in that Claim. This supplies a human judgment for that
concrete failure, not independent labels for the whole corpus. The frozen v11 prompt
already contains the general rule and was not modified for this example.

The additional budget is $5. The manual evaluator can use
`PHASE3_EVALUATION_BUDGET_LEDGER`, a shared JSON file with `limit_micro_usd`,
`reserved_micro_usd` and `rejected_requests`. Before each GPT-6 Luna Pro request it
locks and durably reserves `(UTF-8 system/input bytes + 4096)/2`, rounded up, plus
the explicit maximum output tokens, in microdollars. Rates of $0.50/M input and
$1/M output exceed the frozen current model catalog rates including its higher tier
and cache-write rates; byte counts conservatively bound input token counts with
additional envelope allowance. The initial no-refund guard stopped conservatively before actual spending reached the ceiling. Successful metered calls now settle to a bound computed from provider-reported token counts at those same conservative rates. Failed or unmetered calls keep their full reservation.
All concurrent shards share the same ceiling. Invalid/missing ledgers, unpriced
models or unspecified output caps fail closed. Local refusal is represented as quota
in the provider-compatible diagnostic and stops dispatch; the ledger distinguishes
local budget exhaustion from an actual HTTP 402. This guard is evaluation-only.
A zero-budget check verified no request dispatch and one durable rejected reservation.


### Repair-feedback experiment (v12)
Funded v11 repeated calendar-document Claims with unbound “이 메모” after a
standalone reference rejection; the retry input only said `atomicity`. V12 now
supplies bounded same-job, immediately preceding candidate/review diagnostics with
stage labels. This is untrusted repair context, not new evidence or a pass. All
repaired candidates undergo new complete reviews and publication approval. An
existing fake-provider regression now inspects the actual retry input and verifies
that four new reviews are required and no Claims publish automatically. V11 and
earlier frozen inputs remain unchanged. Real paired results remain to be evaluated.


The first funded v11 continuation saved 42 attempts: 12 terminal gate passes, four
atomicity failures, one language failure and four local-budget terminations. Most
scheduled shards were skipped when reservations filled; this is not a completed
116-source semantic evaluation. The account usage increase after every child exited
was $0.399282. The next shared ceiling was reduced by that amount, preserving the
$5 total allowance. These are account-level increases since a private baseline,
not fabricated attribution of earlier account usage to this task.

OpenRouter optionally exposes bounded token usage with a completion. Its existing
provider trait still returns only text; domain semantics and transport responses
never receive billing metadata. The manual guard settles successful reservations
using reported prompt/completion counts, preserving locked in-flight reservations.
Invalid usage or accounting failures stop the evaluator; missing usage retains the
full reservation. The existing response-decoding regression also checks usage
decoding; no additional compile-time test was added.


V12 repair supplied useful diagnostics but all four calendar convention runs still
failed because the standalone reader rejected an explicit original-memo designation.
V13 isolates a reader-only change: the Claim's original source/narrator is a valid
intrinsic provenance root, unlike an unresolved third-party referent or an undefined
policy. The extractor and source reviewers remain the frozen v12 templates. On 26
calibration controls repeated twice, the reader returned 28 correct passes and 24
correct rejections with no provider/schema errors. These labels are editorial and
partly overlap the prompt; the whole pipeline and separate reader regressions are
being evaluated. The user's short-form unknown-person rejection remains enforced.
