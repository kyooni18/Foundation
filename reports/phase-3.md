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
All candidate batches remain quarantined until explicit local-user fidelity review.
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
source excerpts are not copied into failure diagnostics.

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
candidate and hash. It revalidates against original bytes and checks lifecycle and version
freshness before a single transaction inserts all Claims, mentions, declared dependencies,
the extraction run, and the completion state. A failure rolls the entire publication back.
Repeated approval returns the same sorted IDs, including successful empty extraction.

Re-extraction retains historical Claims. A run-level prior-job link represents whole-batch
versions, splits, and empty replacements; `revision_of` must reference a Claim from the
preceding accepted run of that same Source. Prior Claim IDs/text are supplied as untrusted
revision context, not source evidence. Default lists show the current run; agents can read
non-forgotten historical Claims by ID. No factual contradiction or temporal supersession
is inferred. Stale competing jobs cannot replace a newer accepted version.

D-35–D-42 in `DECISIONS.md` document the migration, quarantine, revision, language, projection,
and lifecycle resolutions with alternatives. In particular:

- A source with currently hidden historical Claims cannot publish a new extraction until
  lifecycle is deliberately resolved; new IDs must not silently circumvent retract/forget.
  The worker checks before sending prior Claim context, and publication checks again for races.
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

Prompt version: `extraction-v1`. Files under `prompts/extraction-v1/` are versioned in Git.
Plan hashes cover extraction instructions, framing examples, review instructions, and
provider/model configuration. The request stores the frozen hash; accepted Claim provenance
includes provider, model, version, and hash. The evidence manifest records file hashes.

Seven executable variants are provided:

| Variant | Experimental distinction | Fresh real-model results |
|---|---|---|
| single | Direct extraction with output contract, no framing examples | Not run |
| r2-frame | Adapted historical R2 framing instructions | Not run |
| decompose | Proposition enumeration, independence test, frame reconstruction | Not run |
| english-intermediate | English intermediate decomposition; original-language final text | Not run |
| atomicity-review | Decomposition plus separate atomicity review | Not run |
| fidelity-review | Decomposition plus separate fidelity review | Not run |
| audited | Decomposition plus atomicity, fidelity, and coverage reviews | Not run |

`audited` is a provisional conservative operational strategy, **not an empirically selected
winner**. Production still applies all three reviews to any selected extraction variant;
the evaluation harness varies review stages to measure their incremental effect. Extraction
and review prompts/payloads are shared between production and the harness. The baseline is
explicitly adapted to Phase 3's output schema and language rule; its results must not be
presented as a byte-identical replication of R2. The original R2 prompt is archived separately.

The prompts include positive and negative examples: split independent Rust/SQLite/network
properties; repeat the condition when splitting shutdown consequences; retain `not(A and B)`;
retain each quotation's attribution; preserve uncertain toner depletion and an unverified
status; preserve VPN password questions as questions; explicitly retain unknown referents
and unresolved irony. Causal/comparative/counterfactual relationships are not split into
unsupported independent events. Instructions embedded in Source text remain data.

Historical evidence reviewed from `~/Code/Foundation-Research/`:

- R1 fidelity/schema comparisons and adversarial checks, plus R2 experiment D and its scripts.
- R2's stronger downstream reader scored 0.667 on bare text, 0.869 on bare text plus fields,
  0.935 on frame-preserving text, and 0.952 with anchors, versus 0.964 on original text.
  These are **historical downstream multiple-choice scores**, not Phase 3 atomicity rates.
- Concrete reported R2 failures included literalized ironic praise of a deployment process,
  unresolved “걔가 어제 또 늦었다”, and confident interpretation of “이번 분기 목표 진짜
  현실적이다” where intent was indeterminate. The ambiguous case failed in all 12 reader
  observations, including original-source conditions.
- R2 reported 55% English drift for one extractor's Korean notes (20/45 remained Korean).
  Its validator caught none of the residual frame-preserving sarcasm/ambiguity errors.

These findings justify explicit ambiguity and language guards, original-source verification,
and the local review gate. They do **not** establish that the new prompts fix these defects.
`evaluation/phase3/r2-evidence.json` preserves the relevant historical summaries.

## Corpus and scoring

There are now 104 cases (including eight long-form extensions), with no held-out examples copied into framing examples:

| Corpus | Development | Held-out | Annotation status |
|---|---:|---:|---|
| Legacy R1/R2 | 48 (47 Korean, 1 English) | 0 | Original adversarial criteria/questions; proposition inventories need review |
| New curated | 32 (28 Korean, 2 English, 2 mixed) | 16 (13 Korean, 2 English, 1 mixed) | Editorial draft semantic inventories; independent human adjudication pending |

New cases include conjunction/disjunction scope, nested conditions/quotations, double
negation, shared subjects, compound nouns, complex predicates, reports/beliefs, proposal vs
decision, counterfactuals, corrections, historical scope, sarcasm, rhetorical language,
figurative exaggeration, typos, mixed technical identifiers, long notes, repetition,
contextual references, exceptions, prompt injection, and valid empty extraction.
References describe propositions, required qualifiers, and unsupported interpretations;
they are not exact-string answer keys. They were authored during implementation and are
not independent gold labels. Legacy annotations also need explicit proposition inventories
before using them as coverage denominators.

The harness defaults to three runs per variant on a selected split and writes each result
immediately to a new 0600 JSONL file: model configuration, prompt/source hashes, case,
raw output, structural validation, and raw review results. Human reviewers should annotate
truth-condition equivalence against the **original source**, blinded to strategy where
possible. Ambiguous/high-impact cases require a second independent reviewer and adjudication.
No model confidence or majority vote substitutes for this process.

`score.py` aggregates annotated rows by strategy, language, and category:

- Accepted-Claim atomicity, unsupported assertion, fidelity, epistemic, language, anchor,
  and duplication error rates, with numerator/denominator.
- Meaning-changing over-splits per annotated decomposition.
- End-to-end coverage: rejected batches contribute zero, preventing abstention from
  disguising omissions. Annotation inventory supplies the denominator.
- Translation-induced errors per reviewed intermediate-language candidate; attribution
  requires comparing paired direct/intermediate outputs against the original.
- Reviewer TP/FP/FN/TN against human labels, plus lexical output variation per case.
  Lexical variation is a repeatability diagnostic, not semantic equivalence measurement.

| Fresh Phase 3 semantic metric | Korean | English | Mixed |
|---|---|---|---|
| Atomicity / over-splitting / coverage / fidelity | Unmeasured | Unmeasured | Unmeasured |
| Unsupported / epistemic / language / anchor errors | Unmeasured | Unmeasured | Unmeasured |
| Translation errors / duplication / repeatability | Unmeasured | Unmeasured | Unmeasured |
| Reviewer false positives / false negatives | Unmeasured | Unmeasured | Unmeasured |

There are zero fresh evaluated model batches, so the zero-known-error acceptance target
is **unverified**, not a measured 0% error rate. Held-out results and per-category real-model
failures are unavailable. No fresh failed output was suppressed or replaced with fake data.

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
  stale competing runs, and prevention of hidden-Claim resurrection.
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
