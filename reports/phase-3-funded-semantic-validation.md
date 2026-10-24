# Phase 3 funded semantic validation

## Decision

**Semantic acceptance remains incomplete.** No production default promotion,
`phase-3` tag, model-authorized publication, or real user approval was performed.
The latest concise extractor (v41) is rejected for promotion: none of its six
primary batches completed the acceptance gate. A valid protocol or model agreement
is not independent semantic truth.

Work began at `3033136`; the renewed $6 continuation starts from `51fc7eb`.
Implemented checkpoints include `b1f1d4e` (authorized ceiling), `064262f`
(proposition/Claim bijection), `fce7fc4` (separate Source responsibilities),
`f05fb6e` (supported wire schema and durable transport errors), `32e1ef6`
(identifying qualifiers versus assertions), `22d405a` (v41), and `33b7c11`
(separately traced bounded repair). Evidence checkpoint: `f885869`.
Ending implementation/replay commit: `214f487`. The final report/evidence checkpoint
is identified in Git history; its parent and
verification result are saved in `six-dollar-continuation/final-checkpoint.json`.

## What changed and why

- Extraction instructions preserve nested report chains, quoted first-person
  perspective, original-utterance time, unknown semantic arguments and independently
  asserted conjecture/checking status. Historical prompts, Sources and references
  remain frozen. New profiles are opt-in experiments.
- Source review now assigns nine separate responsibilities: atomicity/duplicates,
  full-Claim entailment, attribution, scope, arguments, inference, Korean meaning,
  Source-commentary expansion and coverage. Each report must assess its assigned
  dimensions against the full original Source. A pass in one role cannot cancel
  a failure in another. Atomicity and coverage inventories must agree.
- `source-checks-v6` requires a one-to-one proposition/Claim inventory on pass.
  Multiple inventory entries cannot reuse one Claim, and one entry cannot credit
  multiple Claims. Failed inventories may describe fusion, omission or duplication.
- v40 distinguishes identifying noun phrases from additional assertions and unknown
  semantic arguments from commentary about document grammar. v41 calls actual
  defects `violations` and rejects nonblank-field/aggregate contradictions locally;
  its supported wire schema also requests nonwhitespace explanations.
- Review stage identity is bound in production, evaluator, hash and replay. Repair
  context retains the actual failed prefix and remains explicitly untrusted.
  One repair is recorded separately from its immutable primary; malformed output
  is never stripped, rewritten or replaced with a later passing primary.
- Rust preflight blocks paid reviews of invalid saved candidates. Unicode evidence
  re-anchoring remains exact and unique only. Korean dependencies are assessed even
  when the Claim is English. DeepSeek's English-only boundary is unchanged.
- An async fake-provider test exercises v41 through the actual worker, ten reviews
  (one isolated reader plus nine Source roles), quarantine and exact-hash user
  approval. Agent approval is forbidden; Source text stays unchanged. This test
  verifies integration and authority, not model semantic accuracy.

## Real-model findings and rejected approaches

### v35–v38: broad agreement can accept a semantic error

Six initial regenerated batches across the frozen priority Sources were followed by
one bounded delay repair. The v38 repair's second Claim was:

> 진우는 원문에서 목적어가 생략된 ‘확인’을 하지 못했다고 말했다.

It substitutes commentary about a grammatical omission for the proposition that
Jinwoo could not verify the possible delay. Two isolated readers and three broad
Source reviews passed it; the complete gate was eligible. Editorial inspection
therefore rejects this candidate. It is retained as an actual false-acceptance
control and is **not** credited as extraction success. Complete-Claim comparison
must examine modifiers and omitted targets, not just a supported core assertion.

### Focused mechanism experiments

The candidate-blind Source-first experiment used four calls on one delay Source.
It invented a third redundant outer report and passed all three candidate variants,
including the defective v38 repair. It is rejected and not integrated.

A separate narrow-entailment experiment used four calls on that same Source:
three distinct candidate comparisons and one repeat of the bad candidate. The good
v32/v35 candidates passed; v38 was coherently rejected for losing its checking target
and adding grammar commentary. The repeat also said reject but changed Claim
identity, so it is protocol-invalid. This supports the narrow comparison mechanism
on one boundary; it does not establish reliable repeat behavior or corpus truth.

### v39–v40: narrower roles expose different failures

v39 correctly rejected the bad v38 repair's full meaning. It also exposed an
ambiguous self-reference in a delay candidate, then a single bounded re-extraction
introduced an unnecessary “앞서 말한” reference. That repair failed an isolated
reader and was not substituted into a passing result.

A mixed-language candidate was falsely treated as duplicating the number 4 inside
an identifying noun phrase. v40 repaired that distinction. Its mixed candidate
then reached the Korean role, whose raw pass contained positive paraphrase notes
in the defect list: this is protocol-invalid and remains invalid. A habit review's
blank explanation is likewise retained as invalid. Exact unchanged v39 requests,
including failed requests, were reused only after full payload equality checks.

Of the six v40 selected traces, nested report and ambiguous emotion were fully gate
eligible (2/6). Hypothetical and mixed traces were invalid, habit remained invalid,
and the delay repair failed its reader. This is a diagnostic gate result, not an
independently adjudicated 2/6 semantic-success estimate.

### v41: concise generation did not improve the demonstrated scope

All six frozen priority Sources received one new extraction because the generator
recipe changed. Ten Claim texts were generated. Outcomes across **all six primary
batches**, without a passing-subset denominator:

| Source | Claims | Primary outcome |
|---|---:|---|
| Delay and checking | 2 | Second isolated reader rejected an unbound affected argument |
| Mixed Korean/English count | 2 | Rust rejected language contract; no paid review |
| Ambiguous person and irritation | 2 | First isolated reader rejected reference |
| Nested report and disagreement | 2 | First isolated reader rejected reference |
| Conditional architectural intention | 1 | Reader passed; first Source review timed out |
| Original-speaker habit | 1 | Reader rejected unbound relative time |

**0/6 complete eligible batches**. Reader failures can include false rejections;
readers supply issue classes, not enough explanation to adjudicate every rejection.
For example a Source-bound quoted entity role can be legitimate, while the habit
candidate genuinely omitted its original-utterance time binding. No wholesale
semantic-error rate is claimed from these mixed failure types.

Three separately recorded, trace-linked repairs used unchanged v41 prompts:
- Delay: fenced JSON, rejected without stripping the fence.
- Mixed language: echoed input plus output, rejected as extra JSON data.
- Ambiguous person: generated two Claims but reader rejected the first.

Thus **0/3 repairs completed the gate**. The model must produce a valid extraction
envelope and preserve required arguments; repair feedback is not a proven remedy.
No automatic further repair or primary replacement was performed.

The v41 negative nested review also timed out. The fused-delay control had a request
and reservation saved when the orchestrator was interrupted; no response or terminal
provider receipt exists. Its outcome is unknown. Causal-emotion and bad-v38 controls
were not dispatched in this cohort. None are counted as successful rejections.
The nonwhitespace schema feature's relation to the timeouts is unproven.

## Reproducibility and limitations

`reports/evidence/phase-3/six-dollar-continuation/replay-v41.json` replays 41 extraction
requests, 188 Source-review attempts and 50 reader attempts across six distinct
Sources. These are all saved diagnostic attempts, not 279 independent examples.
Four attempts have no saved response. Invalid JSON, contradictory judgments,
truncations, historical validation versions and unknown reservations remain visible.
Original receipts are immutable; the current validation result is separate.

Current complete traces reconstruct configuration from immutable requests; they are
not previously quarantined production jobs. Earlier diagnostic profiles sometimes
used a 3,000-token isolated reader and a 6,000-token Source reviewer, so a single
production reviewer configuration does not fully reproduce that older routing.
v39+ use 6,000 for both. v41's actual worker path is tested with synthetic responses.

The five named original gaps (`new-003`, `new-007`, `new-018`, `new-025`, `new-042`)
have only older archived candidates. Their archive audit remains in
`completion-recheck/five-case-archive-audit.json`; none is counted as a current pass.
The exposed six Sources are a development set, not fresh held-out validation.
Independent whole-corpus Korean ground truth is unavailable.

## Budget, transport and verification

The human authorized a cumulative **strictly below $6** ceiling. Old charges were
preserved; “reset” did not reset consumption. Official pricing/schema were rechecked:
[Haiku overview](https://platform.claude.com/docs/en/models/haiku-5-5/overview),
[structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs).
Unsupported `maxItems` and `minItems>1` caused an initial v39 HTTP400; a separate
supported wire revision was used afterwards. The HTTP400 reservation was retained.
No historical request, label, actual token usage or charge was repriced.

| Accounting | Conservative USD |
|---|---:|
| Historical prior | 4.78265335 |
| Recorded actual upper bounds plus unknown reservations | 0.52279650 |
| Cumulative upper bound | **5.30544985** |
| Authorized ceiling | 5.99999999 |
| Monetary headroom | **0.69455014** |
| Increase since the $6 grant checkpoint | 0.30984550 |
| v41 primary/repair round, 19 requests | 0.03121240 |

297 total ledger requests: 293 settled, four unknown; no overrun. Provider invoiced
USD is unknown. Unknown reservations total $0.02206760 and remain included. They are:
one historical v39 HTTP400, two v41 timeouts, and the interrupted fused-delay request.
The unchanged `pending < 4` gate now blocks new paid dispatch despite monetary
headroom. No extra credits are requested, no reservation is freed, and no paid
process remains live.

Required verification: 48 Rust tests passed, two ignored; 40 Python tests passed;
`cargo fmt --check` and Clippy with warnings denied passed. Full saved-evidence replay
also passed. Exit codes are retained in the final checkpoint. No unnecessary new
compile-time test methods, dependencies, migrations or Phase 4 features were added.

## Remaining work

Semantic acceptance requires faithful standalone generation, coherent current-profile
negative controls and repeat evidence, plus coverage without grammar substitution.
The v41 generator/schema change does not supply that evidence. Preserve incomplete
status, frozen profiles, exact-hash user approval, and all failed attempts. Unknown
transport outcomes must be addressed before the existing pending-request gate permits
further paid investigation.
