# Phase 3 final continuation — 2026-10-23

Latest follow-up: [authorized ceiling applied; provider-cap blocker verified](phase-3-authorized-continuation.md).
The $0.10 authorization has now been granted and the same ledger ceiling amended to
$4.88265335. The earlier report below is preserved as the pre-authorization checkpoint.

**Phase 3 remains Implemented but unverified. Semantic acceptance is Blocked.**
Engineering verification passes; missing outputs, independent adjudication, Korean
quality and reviewer reliability prevent completion. No phase-3 tag was created.

## Repository and recovery

Initial HEAD: `8ce66a0` on `main`. Final implementation HEAD:
`73b367f9f08196b281ae4e13632085c33187dd44`. Final delivery HEAD is the commit
containing this report and its verification evidence, recoverable with
`git log -1 --format=%H -- reports/phase-3-final-continuation.md` (a commit cannot
embed its own hash). The final response identifies that delivery commit.

Read the architecture, data contracts, decisions, implementation plan, README,
Phase 3 and continuation reports, extraction/provider/budget/publication code,
focused tooling, and the interrupted session's final messages/tool evidence.
The session at `~/.yeet/sessions/15eaebc6-5a90-4499-8b1e-9c818c7d5772`
ended with context-window errors; it was not repaired or rewritten. Its budget
instructions contained no authorization to increase the existing ceiling.
Foundation Memory was recalled; historical memories were checked against the tree.
Jev was unavailable in tool inventory, plugin search and the local executable path.
Focused local reproduction substituted for it.

**Complete — recovered work:** integrated `audit_focused_evidence.py`,
`test_evidence_audit.py`, and the complete pre-existing
`reports/evidence/phase-3/focused-evidence-audit/` directory. All seven historical
command-log hashes matched. Fresh execution reproduced the current candidate
JSONL, full archive replay index/summary/profile counts/input manifest, and ledger
audit byte for byte. The semantic audit reproduced except for its new input-path
metadata. Old evidence bytes were preserved, including log trailing blank lines.
The old receipt names `17da05c` as its base; it is historical evidence, not a claim
that all later tests existed at that commit. New receipts record actual execution.
The untracked project `.yeet/` and all session history remain untouched and uncommitted.

## Implemented corrections

**Complete — evaluation validation:** demonstrated two failing regressions before
fixing them: an inventory proposition marked `not_applicable` with no Claim could
produce a protocol pass, and an audit could accept changed historical reviews when
candidate text/hash stayed identical. The validator now requires pass/fail/unresolved
coverage for every inventory proposition; a disputed reference remains unresolved.
The audit binds reviews, case, run and attempt to the pinned archive as well as raw
candidate, source, plan and prompt. Boolean indices no longer masquerade as integers.

Korean/mixed packets cannot label Korean meaning preservation `not_applicable`.
The dispatcher now rejects duplicate JSON judgment members and nonfinite constants
instead of accepting last-member-wins interpretation. A regression verifies that a
malformed judgment's raw response and cost remain recorded while no judgment is
admitted. Existing immutable artifacts, frozen manifest, references, protocol text,
v23 prompts, extraction behavior and production approval are unchanged.
These are evidence/protocol corrections, not demonstrated semantic improvement.

**Complete — unapplied policy preparation:** `evaluation/phase3/proposed-policy/`
contains an exact patch for both existing hard limits and a guarded in-place SQL
transaction. Proposed ceiling: $4.88265335, exactly the historical conservative
$4.78265335 plus at most $0.10, still $0.11734665 below $5. The SQL refuses changed
starting accounting or any intervening request. It never deletes prior or requests.
The exact artifacts passed fixture tests; the patched Rust budget was compiled in a
temporary exported checkout and passed both its existing concurrency/restart/overrun
test and an exact-boundary runtime probe. None was applied to active code or ledger.
This is a proposal requiring explicit authorization, not an authorization record.

## Production integration and architectural limits

| Area | Status | Verified scope and limit |
|---|---|---|
| Durable jobs, immutable Sources, transactional publication | Complete | Actual Rust integration tests cover leases, retries, SIGKILL, rollback, stale hashes/versions, rejected proposals and restart idempotency. |
| User-only exact-hash approval and visibility | Complete | Model verdicts cannot publish or endorse; real HTTP/MCP authentication, impersonation and projection tests pass. Tests publish only fixture Claims in temporary databases. |
| Explicit Korean routing infrastructure | Complete | Existing frozen Plan names extractor/reviewer; recorded route/hash is auditable. Tests verify original Korean inputs, each unavailable stage, no fallback, no mutation/publication, and provider identity mismatch. DeepSeek default and encoded-Korean refusal remain intact. |
| Korean semantic quality and live provider compatibility | Implemented but unverified | Scripted routing proves control flow only. No production Haiku promotion or live Korean extraction occurred; generic Rust adapter still rejects unsupported tiered pricing/parameters. Dedicated guarded Haiku evaluation is separate. |
| Cross-source user attribution | Deferred | Actual matching/unrelated/agent/retracted/forgotten user-source citation negatives pass. Fabricated IDs and citation/span fields fail schema checks. An agent report never becomes user-authored evidence. |
| Long-document chunking | Deferred | Retain intact refusal above 256 KiB combined system/input, before network/reservation; original Source and explicit job failure remain. |
| Claim pagination | Complete | Current query filters visibility before totals/pages; HMAC cursors bind credential/filter/resource, with live visibility rechecks. No demonstrated correctness/isolation defect. |

Cross-source existence, recorder and spans alone cannot establish semantic support.
The current schema places source citations in Phase 4, with no trusted cited-span
support decision. Adding an ID/text-match shortcut would weaken authority; this
continuation retains unresolved refusal and adds no dependency semantics.

Arbitrary document chunks can omit a governing condition, pronoun referent, temporal
frame or attribution anywhere outside the window. Overlap alone cannot prove coverage
or prevent semantic duplicates. There is no validated document-level dependency/coverage
protocol or funded model evaluation to justify chunk publication. The minimal correct
behavior remains bounded whole-request refusal, with no truncation or partial-success
claim. Pagination materializes visible Claims and lacks cross-page snapshots; this is
a documented scalability limitation, not a newly demonstrated isolation defect. No
unrelated optimization was introduced.

## Focused evidence and reviewer disagreement

Frozen manifest hash:
`164f84834501955b62359552ac192fb6f92fccb44d5a0424a63f8bcdaf8004a7`.
All corpus/source/reference hashes and exact archived candidate/provenance hashes
verify. Fresh focused replay reproduces the frozen packet file exactly. Selection
was previously exposed editorial material: Git proves frozen contents, not independent
within-session chronology or blinded gold.

| Measurement | Before | After |
|---|---:|---:|
| Selected cases (Korean / mixed / English) | 12 (8 / 2 / 2) | unchanged |
| Available current-profile outputs | 7 | 7 |
| Missing outputs | 5 | 5 |
| Generated Claims | 11 | 11 |
| Archived protocol-eligible batches | 3 | 3 |
| Reviewer rejected / incomplete batches | 3 / 1 | 3 / 1 |
| Assistant editorial pass / fail / unresolved batches | 5 / 0 / 2 | 5 / 0 / 2 |
| Editorial-valid protocol-accepted Claims | 5 | 5 |
| Korean editorial-valid protocol-accepted Claims | 1 | 1 |
| New Haiku primary / repeated judgments | 0 / 0 | 0 / 0 |
| New production Claims published | 0 | 0 |

Missing: `new-003`, `new-007`, `new-018`, `new-025`, `new-042`.
Five of eight Korean cases have outputs, totaling seven Claims. Available-reference
coverage remains 5/11 overall and 1/7 Korean under the editorial-plus-protocol metric;
these are not independent acceptance rates. Rejected, incomplete and missing cases
are not successes. Independently adjudicated atomicity, entailment, omission,
attribution, uncertainty, negation, conditions, entity resolution, temporal scope,
Korean fidelity and full-set valid coverage remain **unknown**, not zero errors.

`r1-ambiguous_ref` retains the same-text isolated-reader reject/pass flip. Anchor
changes were invisible to that reader, so they are not a semantic repair.
`v23-fresh-003` remains contested over whether its nonverification is a qualification
or a separately asserted report. Recorded responses do not establish the internal
cause of instability. The new source-evidence protocol distinguishes intrinsic
readability from entailment, unknown from invented identities, attributed reports
from truth, necessary qualifications from independent propositions, and source
uncertainty from reviewer uncertainty. Deterministic tests verify exact evidence,
unknown states and disagreement retention, never majority voting. Both legacy
issue-only responses fail the new schema; that does not prove the new reviewer is
more reliable. Matching judgments still cannot establish correctness or approval.

Full archived replay: 5,281 rows, 4,752 distinct payloads, 145 Source hashes;
3,646 structurally valid, 4,502 valid frozen plan hashes, 2,624 experimental-gate
eligible. These match the recovered replay. Two historical structural passes remain
rejected by existing safeguards; no new extraction improvement is inferred.

## Executed verification

All commands below exited **0**. Actual process results, logs and hashes are in
`reports/evidence/phase-3/final-continuation/`, not inferred from old committed logs.

| Command | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo test --offline --all-targets` | 46 passed; 2 ignored subprocess helpers exercised by parent tests |
| `cargo clippy --offline --all-targets -- -D warnings` | pass |
| `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'` | final 24 passed (initial 21) |
| `cargo build --offline --example revalidate_extraction` | pass |
| `target/debug/examples/revalidate_extraction --input .../selected-input.jsonl --output .../current-revalidated.jsonl` | 7 exact frozen outputs replayed |
| `python3 evaluation/phase3/focused_review.py --output .../focused-replay` | 546 archived rows; 7 packets; 5 explicit gaps |
| `python3 evaluation/phase3/audit_focused_evidence.py --current .../current-revalidated.jsonl --output .../verified-audit` | all 12 inventories verified; unchanged measurements; zero headroom |
| `python3 evaluation/phase3/replay_archives.py --output .../full-replay` | 4,752 distinct payloads |
| `python3 evaluation/phase3/guarded_haiku.py` | offline only; no dispatch |
| Isolated proposed policy `cargo test --offline --lib budget::` | 2 passed; temporary patched checkout only |

The 19 passing extraction tests cover Korean routing safety, cross-source attribution
negatives, production approval/rejection, stale publication, concurrency and restart.
Six transport tests cover real HTTP/MCP paths; five authorization tests and ten
persistence tests cover Phase 1–2 integration. Python covers frozen evidence, reviewer
disagreement, guarded accounting/failures and the inert policy proposal. Before the
fix, the two new negative cases each failed with exit 1; after, both pass. No Rust
production behavior or frozen semantic comparison changed. The isolated Rust probe
is not included in the 46 active-code test count.

## Spending and remaining authorization

**Blocked — paid evaluation.** This continuation made **0 OpenRouter requests**,
including metadata requests, and incurred **$0.00 confirmed new spending**. No current
availability/pricing claim is made from the historical catalog snapshot. The dispatcher
must refresh and verify `anthropic/claude-haiku-5.5`, endpoint prices/tiers, context,
low reasoning and structured-output support before any future authorized paid request.
No credentials were read, provider caps changed, or background paid requests scheduled.

The existing ledger still has ceiling **$4.50**, conservative historical account-wide
prior **$4.78265335**, zero request rows and zero pending reservations. Authorized
headroom is **$0.00**. Historical task-attributed spending cannot be reconstructed from
that account-wide delta. Ledger SHA256 before/after:
`36c1b1413343da0f15f62e9171360358898d8fd0d65dbcbc9f868c465d1af260`.
The $0.21734665 arithmetic remainder below $5 is not authorized spending capacity.

Required authorization: explicitly approve the proposed **in-place $4.88265335
cumulative ceiling**, preserving the same ledger/history, allowing at most $0.10
additional with conservative reservations and strictly less than $5 total. If the
unchanged provider cap prevents dispatch, separately authorize its necessary in-place
amendment. The existing authorization record must reference an actual user grant;
the agent must not create one from this report. Funding alone does not supply the five
missing candidates or independent adjudication, nor guarantee enough budget for 15 calls.

## Completion gates and commits

Before semantic acceptance: obtain missing exact current-profile outputs under an
explicitly funded extraction plan; run up to 12 primary and 3 designated repeated
source-grounded evaluations within all guards; independently adjudicate disputed
atomicity and every required semantic dimension; evaluate Korean quality for the
claimed scope; measure and contain reviewer disagreement; preserve production
approval boundaries; and obtain wider independent evidence where the 12-case
diagnostic set is insufficient. Any unresolved known bundled proposition or unsupported
assertion prevents acceptance. Engineering passes alone never justify a phase-3 tag.

Commits:

- `2170743` — recovered audit/evidence, historical review identity and coverage checks.
- `52686be` — strict judgment JSON and mandatory Korean-fidelity assessment.
- `73b367f` — unapplied budget proposal and isolated verification tests.
- Report/evidence delivery commit — resolved by the command in the repository section.

Preserved phase-1 target: `50dbccb34fc3b5960fe02de2af56c8c33f7b5916`.
Preserved phase-2 target: `ec11625f53f1bc12cb1f9e3e9849044f8e0d7d0d`.
No phase tags were moved. No phase-3 tag exists.
