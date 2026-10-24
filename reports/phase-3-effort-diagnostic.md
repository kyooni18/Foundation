# Phase 3 effort diagnostic — acceptance remains incomplete

2026-10-24. Starting commit `c602a03`; ending commit is the delivery commit containing
this report. The previous goal turn made substantive progress: three complete v30
passing traces and preserved v31 failures. This turn tests a specific alternative
explanation of the remaining reviewer errors, without another prompt rewrite.

## Matched experiment

Hypothesis: fixed low effort contributes to mistaken role/scope judgments. One
bounded direct-Anthropic request uses the **same v31 prompt, Source, generated v31
hypothetical candidate, atomicity stage, schema and 3,000 total output-token cap**
as the saved low-effort request. Only `output_config.effort` changes to `medium`.
Exact payload comparison after changing that one field back proves the match.

The current official [effort documentation](https://platform.claude.com/docs/en/build-with-claude/effort)
was checked: medium is supported and thinking counts toward the output cap. Existing
model pricing, conservative accounting, model identity and authorized ledger remain
in force. No provider, account, prompt asset or historical charge changes.

| Observation | Low | Medium |
| --- | ---: | ---: |
| Protocol valid | yes | yes |
| Aggregate verdict | reject | reject |
| Input tokens | 5,419 | 5,419 |
| Total output tokens, including thinking | 989 | 2,195 |
| Thinking tokens within output | 0 | 1,343 |
| Conservative USD bound | $0.00157830 | $0.00218130 |

Medium clears the earlier atomicity, attribution, entailment, Korean-meaning and
unsupported-inference nonpass statuses, but still rejects `source_meta_expansion`.
It still interprets the unknown **experiment identity** phrase as an assertion about
missing **speaker** details. The experiment does not establish semantic acceptance
or a general improvement in reliability. The candidate's Korean `신원` wording for
an experiment is itself awkward; editorial interpretation is not independent gold.
More effort alone did not resolve the observed failure, so no further effort sweep
or passing retry was purchased. This is one matched **configuration comparison**,
not an identical-request repeat or a full review trace.

Evidence: `reports/evidence/phase-3/effort-diagnostic/comparison.json`; raw immutable
request/response/accounting/judgment files remain in the semantic-repair folder with
`atomicity-effort-medium-primary` in their identity.

## Measurement and failure-handling corrections

The replay previously grouped repeats by system and user input only. It now hashes
the **entire request body**, including model, effort, output cap, schema and tier.
Different configurations cannot be counted as identical repeated requests. Replay
also exposes actual effort/output caps and checks receipt effort against the body.
The historical nested v30 identical-request disagreement remains in the comparison;
the new medium-effort request forms a distinct configuration.

The diagnostic CLI permits medium only as an explicitly identified v31 Source-review
experiment. Existing low-effort request bodies are unchanged. Medium identities
include `effort-medium`; the production Plan/provider configuration is unchanged.
The experiment cannot be reconstructed as a low-effort production gate trace.

A capped non-end-turn Source response now gets a durable `incomplete` rejection
sidecar after usage settlement. Its raw response is preserved and cannot become a
judgment. Offline replay verifies its exact request recipe, stop reason and charge,
then retains it as incomplete. An existing test exercises this path using a
synthetic max-token response, temporary SQLite ledger and fake transport; no live
truncation was observed in this experiment. This avoids losing a bounded attempt
from reporting merely because its response cannot be parsed as a judgment.

No new test method, dependency, migration or prompt version was added. Existing
protocol tests cover distinct configuration identity and unsupported effort; the
existing adapter test now proves capped-response settlement and replay.

## Verification and accounting

All required commands exit 0 (`effort-diagnostic/verification/results.json`):
`cargo fmt --check`, `cargo test --offline --all-targets` (48 passing, two ignored
subprocess workers exercised by parents), `cargo clippy --offline --all-targets --
-D warnings`, and Python discovery (40 passing). Updated full saved-evidence replay
passes, including the new configuration and previous protocol-invalid judgments.
Archive totals: 19 extraction requests, 37 Source reviews and 10 isolated readers.

The one new call cost **$0.00218130 conservative bound**, not confirmed provider
billing. Same ledger: **76 settled requests**, zero unsettled/overruns, prior
$4.78265335 unchanged, recorded $0.14617020, cumulative **$4.92882355**, authorized
ceiling **$4.93265335**, remaining **$0.00382980**. No ceiling increase or historical
repricing. Another corresponding Source review needs a **$0.00463680** reservation;
the actual read-only guard check returns `budget blocked`. No blocked request was
dispatched. `audit.json` retains these observations.

## Acceptance audit

The previous requirement audit remains incomplete: only three of six priority v30
batches have complete passing traces; v31 has none. Existing evidence does not
establish full Korean semantic reliability, consistent positive/negative review or
current-profile acceptance of the nested, delay and hypothetical cases. All five
missing-profile cases remain unassessed. Model agreement and structural validity
are not independent truth; user-only exact-hash approval and lifecycle boundaries
remain unchanged. No Claim was approved/published, no default promoted and no
`phase-3` tag created.

Further live evaluation needs both a justified general correction and authorized
headroom sufficient for its worst-case reservation. Additional effort alone is not
a demonstrated correction. The goal remains active and incomplete; this turn made
verified progress in diagnosis and trustworthy comparison/failure reporting.
