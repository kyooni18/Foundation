# Phase 3 — implementation checkpoint; live semantic evaluation in progress

Status: **not accepted; no `phase-3` tag**. Provider access was restored on
2026-10-23 using the existing OpenRouter credential identified by the user. The
credential is never included in evidence. Live evaluations are now in progress;
the earlier missing-credential result below is historical, not a current blocker.

The first live pilot completed 36 extractions (12 development cases × 3 variants)
with `anthropic/claude-sonnet-5.5` and independent reviews by
`google/gemini-3.8-flash`. Raw outputs are in
`reports/evidence/phase-3/live-v1/pilot.jsonl`. A diagnostic disagreement is preserved
in `negation-diagnostic.json`: three review stages rejected a faithful negated
conjunction, while a separate diagnostic request to the same reviewer called it
faithful. This is evidence of inconsistent judgment, not proof from model agreement.
The broader repeated development matrix and revised prompts are still being audited.
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

A successful model proposal undergoes deterministic validation and three separate calls:
atomicity, original-source semantic fidelity, and whole-batch coverage/duplication audit.
Every review must cover all candidate indices, return no issues, and pass coverage. An
unresolved or failed review rejects the batch. These calls are logically separate reviews,
not evidence of statistically independent model errors. Self-reported confidence is unused.

Even passing proposals become immutable `extraction_candidate` rows with an exact output
hash and review responses. They create **no Claims**. Local-user approval must name that
candidate and hash. It checks that candidate’s own three successful reviews and frozen
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

D-35–D-50 in `DECISIONS.md` document the migration, quarantine, revision, language, projection,
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

User results omit raw anchor passages/offsets, free-text model metadata and extractor/job
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

The adapted R2 baseline is not a byte-identical R2 replication. Production always applies
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

There are **108 sources: 88 development and 20 held-out**:

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
Held-out sources are not used as prompt examples or repair feedback.

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

Final revised-prompt, held-out and legacy audits remain subject to their actual run
results. Neither a measured best strategy nor final zero-error acceptance is asserted
before those comparisons and source audits finish.

## Reproduction and operational use

Configure an actual authorized model and an independent reviewer, and provide credentials
through an existing canonical 0600 file (`OPENROUTER_API_KEY_FILE`) or process environment
(`OPENROUTER_API_KEY`). Never put the secret on the command line or in the repository.

```sh
cargo run --offline --example evaluate_extraction -- \
  --corpus evaluation/phase3/curated.json --split dev \
  --model "$EXTRACTION_MODEL" --reviewer "$REVIEW_MODEL" \
  --runs 3 --output /absolute/private/path/dev.jsonl
```

Run the same development comparison on `legacy.json`. Review concrete failures, revise
prompts and draft annotations, and rerun affected development cases. Freeze the selected
variant before evaluating the held-out split against `r2-frame` with repeated runs:

```sh
cargo run --offline --example evaluate_extraction -- \
  --corpus evaluation/phase3/curated.json --split heldout \
  --model "$EXTRACTION_MODEL" --reviewer "$REVIEW_MODEL" \
  --variants r2-frame,audited --runs 3 --output /absolute/private/path/heldout.jsonl
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
