# Phase 3 offline continuation — 2026-10-23

Implemented at `e2f47bf`, starting from `9a88ceb`. Phase 3 remains **not accepted**;
no phase-3 tag is created. This continuation adds working fidelity-inspection and
validation features, runs broad archived-output checks, and makes measurement gaps
explicit without making paid calls or changing semantic prompts.

## Implementation

`primeradiant --db PATH extraction-review CANDIDATE_ID` returns one source-bound
review packet, restricted to the local user principal. It includes the original
Source verbatim, recorder authority, frozen plan/prompt hash, exact raw candidate
and its hash, each Claim and source anchor, saved model reviews, and current
structural/protocol results. Spans carry separate zero-based half-open UTF-8 byte
and Unicode scalar offsets; they are not grapheme-cluster positions. Repeated
quotes use the declared occurrence. Hash mismatches and inactive Sources fail
closed. Inspection neither publishes Claims nor creates an endorsement. The
existing explicit approval command still rechecks current lifecycle/version and
all publication constraints. This is an inspection surface, not an ordinary user
projection. Unassessed semantic checklist entries are null, not implicit passes.

Source-checks-v4 rejects duplicate JSON members at every object depth, including
Unicode-escaped equivalent member names. Previously the intermediate
`serde_json::Value` parse silently retained the last value, so ambiguous source
anchors, Claim text or condition metadata could become plausible schema-valid
proposals. A small recursive deserializer now rejects that ambiguity before typed
validation. Original proposal bytes are retained; no silent repair occurs.
No migrations, dependency additions, source rewrites, or prompt changes were made.

The evaluation scorer now supports optional per-Claim `semantic_checks` for
multi-proposition content, semantic omissions, negation, conditions, uncertainty,
attribution, temporal scope, entity resolution and Korean meaning. Values are
`pass`, `fail`, `not_applicable`, or `unknown`. Missing labels are unknown. Detailed
failures must also appear in aggregate error flags. Accepted and all generated
Claims have separate metrics, assessed denominators and annotation coverage.
Rejected batches remain zero in end-to-end coverage. Existing coarse annotations
remain readable but do not become fine-grained fidelity measurements.

## Verification

- `cargo fmt --check`: passed.
- `cargo test --offline --all-targets`: **43 passed**, zero failures, two ignored
  helper entry points exercised by parent crash/recovery tests.
- `cargo clippy --offline --all-targets -- -D warnings`: passed.
- `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'`: **3 passed**.
- Broad offline replay: `cargo run --offline --example revalidate_extraction --
  --input INPUT --output OUTPUT`, using 61 existing compressed extraction artifacts. The reproducible driver is
  `python3 evaluation/phase3/replay_archives.py --output NEW_DIRECTORY`; it
  fingerprints every input artifact and refuses to overwrite an output directory.
- Fine-grained metric check: `python3 evaluation/phase3/score.py
  reports/evidence/phase-3/v23-heldout-assistant-audit.jsonl
  --annotation-field assistant_review`.

Only one new Rust test method was added. Existing schema and scorer tests were
extended. New coverage includes Korean repeated anchors, emoji and combining
characters, exact source/Claim hashes, zero publication on inspection, agent/import
inspection denial, inactive Source denial, duplicate top-level and nested JSON keys,
escaped-equivalent keys, and absent/inconsistent fine-grained annotations.
All Phase 1/2 and existing extraction regressions remain passing.

## Recorded-output replay results

The 61 archives contain 5,281 rows. Deduplicating identical case/source/plan/prompt/
variant/run/attempt/raw-output/review payloads gives **4,752 distinct payloads** and
**145 distinct source hashes**. These are archived attempts, not independent trial
counts or a current-profile benchmark. The saved index links each payload digest to
its archive and original metadata without copying every original response again.

| Offline result | Distinct payloads |
| --- | ---: |
| Current structural validation passes | 3,646 |
| Frozen plan hash matches | 4,502 |
| Experimental protocol gate eligible | 2,624 |
| Anchor failure | 131 |
| Schema failure | 40 |
| Language failure | 17 |
| Timestamp failure | 5 |
| Missing provider output or schema failure | 913 |

The 250 plan-hash mismatches remain ineligible; this replay does not repair them or
promote old evidence. Two historical structural passes now fail language checks:
`new-025`, v4, the previously documented Korean language-drift pair. No new ambiguity
failure was found in the archived candidates; new duplicate-member controls verify
the parser fix. **These counts measure structural/protocol behavior, not semantic
atomicity, recall, Korean fidelity or final-profile model quality.**

Re-scoring the existing small v23 assistant audit leaves its earlier coarse labels
unchanged. All newly introduced fine-grained dimensions remain unknown because
those historical annotations do not contain the required individual judgments.
Their error rates are null, not 0%. No fresh semantic annotation or independent
human validation is claimed, and no production candidate was approved.

Evidence is in `reports/evidence/phase-3/offline-review-20261023/`: command logs,
replay summary, per-profile counts, compressed payload index and fine-grained
scorer output. Frozen original artifacts remain unchanged.

## Spending, commits and remaining acceptance

New OpenRouter requests/tokens/spending: **0 / 0 / $0.00 confirmed**, with no unknown
new charges. No live model was used. The existing private shared ledger is unchanged:
$4.78265335 conservative historical account delta carried against the $4.50 ceiling,
zero new request rows and **$0.00 spendable remainder**. The historical delta is not
exact request-attributed spending. No new allowance, key limit change, credit purchase,
or background paid work was introduced.

Phase 1/2 tags and immutable migrations remain unchanged. Pre-existing `.yeet/` is
untouched. Foundation was consulted and updated; no Jev tools were available.
The remaining acceptance gaps are broad final-profile semantic evaluation,
independent source-level adjudication, reviewer instability and a validated Korean
path compatible with the requested default model's English-only boundary. The new
review packets and honest denominators support that work but do not settle it.
