# Phase 3 authorized continuation — 2026-10-23

**Implemented but unverified. Semantic acceptance remains Blocked by the provider-cap
requirement and incomplete independent semantic evidence. No phase-3 tag.**

Initial HEAD: `1e2a74b081ba9ab50af1e43832efcd22f167fd90`. Implementation commit:
`ac2121d` (authorized policy and provider refusal). Final delivery HEAD is the commit
containing this report: `git log -1 --format=%H -- reports/phase-3-authorized-continuation.md`.
This adds current results to the prior final-continuation report; its earlier results
and historical evidence remain preserved.

## Complete — authorized local policy amendment

The human follow-up said: “Then complete phase 3 / Its ok to spend that amount.”
This authorizes the concrete $0.10 additional proposal described in the previous
response, retaining the cumulative restriction strictly below $5.

Applied the reviewed `evaluation/phase3/proposed-policy/ceiling.patch` to the two
existing hard limits, and the exact guarded `ceiling.sql` transaction to the existing
`.local/openrouter-phase2-phase3.sqlite`. Verified no running evaluation/production
worker before application. No ledger was replaced, reset or newly provisioned.

| Accounting | Before | After |
|---|---:|---:|
| Existing ledger ceiling | $4.50 | $4.88265335 |
| Conservative historical account-wide prior | $4.78265335 | unchanged |
| Request rows | 0 | 0 |
| Settled new cost / pending reservations | $0.00 / $0.00 | $0.00 / $0.00 |
| Local authorized headroom | $0.00 | $0.10 |

The new cumulative ceiling is $0.11734665 below $5. It constrains all consumers of
the same ledger. The dispatcher additionally retains its persistent $0.10 continuation
limit, conservative reservations, bounded requests, no retries-to-pass, and no fallback.
Historical account-wide usage remains conservative accounting, not exact task cost.
Authorization and before/after ledger hashes are in
`reports/evidence/phase-3/authorized-focused/authorization.json` and
`policy-application.json`. The authorization is now supplied; it is no longer the
reason local spending is blocked.

## Complete — current model and workload preflight

Fresh public catalog and endpoint GETs verify `anthropic/claude-haiku-5.5`:
1,000,000 context tokens, 128,000 maximum completion, `low` reasoning, `max_tokens`
and `response_format` support. The frozen evaluation uses low reasoning, JSON output,
2,400 completion tokens, a pinned endpoint, `require_parameters=true`, and no fallback.

Listed base prices on the selected endpoint are $0.10 input / $0.50 output per million
tokens. Long-context and cache-write tiers are higher. The unchanged quote logic
uses the highest advertised tier/cache-write input rate ($1.00 per million) and
output rate ($2.50 per million), plus its 2× reservation margin and 4096-token envelope.
It takes no cache discount and supplies no search tools/plugins. These are metadata
quotes, not billed execution prices.

All seven frozen candidate packets pass metadata/context/reservation preflight.
The largest conservative reservation is $0.0307, which fits the local $0.10 allowance.
Available workload remains seven primaries plus three designated repeats, with a
maximum permitted 12 primaries and 3 repeats if missing candidates become available.
No additional extraction call is inferred or silently added to this evaluation workload.
Fresh metadata, hashes, per-case quotes and missing-case IDs are saved in
`model-preflight.json` and `workload-preflight.json`.

## Blocked — existing credential has no provider cap

The actual guarded dispatcher was invoked with the existing credential, the same
persistent ledger, exact frozen packet and recorded user authorization. It returned:

`provider cap blocked: non-resetting cap required`

This happened after three metadata GETs and before reservation or POST. The API key
returns `limit=null`, `limit_remaining=null`, `limit_reset=null`: it is uncapped.
The historical credential-file pointer resolves to the same credential as the current
environment, so it is not an alternative capped key. No other key or account was used.

The current policy requires a positive non-resetting provider cap no greater than the
ledger ceiling, plus enough provider headroom for the conservative reservation. The
available key's pre-existing lifetime usage already exceeds the cumulative task ceiling.
Setting that key's absolute cap to $4.88265335 would therefore provide no usable
headroom. That lifetime usage is not substituted for task cost or added to the ledger.
No exact account-wide balance/usage is committed in the evidence.

The existing credential also fails the read-only management capability check:
`GET /api/v1/keys` returns **401**, and no management credential is configured.
OpenRouter documents a management key requirement for key updates and defines
`limit=null` as unlimited. See the official
[limits documentation](https://openrouter.ai/docs/api_reference/limits) and
[management-key documentation](https://openrouter.ai/docs/guides/overview/auth/management-api-keys).
No provider setting was mutated.

This continuation does not remove the provider guard, reset usage, create a new key,
or switch accounts. To use this same shared key while retaining an independent
provider backstop requires **management access to amend that existing key in place**
and an explicitly reviewed amendment expressing its absolute cap relative to its
preserved lifetime-usage baseline plus no more than the remaining $0.10 task allowance.
That changes the existing absolute-cap comparison policy and is outside the applied
ceiling.patch. The local task ledger must still remain at $4.88265335 with all history.
Alternatively, a genuinely existing credential already satisfying the current guard
would need to be identified without creating or rotating a key to bypass limits.
No such compatible credential was found in the configured paths.

The precise current blocker is provider configuration/access, not missing permission
to spend the already authorized $0.10. Funding alone cannot provide management access
or prove semantic acceptance.

## Complete — failure handling and verification

The uncapped response previously reached `Decimal(None)` and produced an incidental
conversion exception. The dispatcher now identifies absent/null/resettable/incompatible
caps explicitly and refuses before reservation or POST. A new regression covers null,
daily-reset and over-ceiling caps, asserting zero request rows and zero POSTs.
The historical policy-proposal test now starts from pinned pre-amendment source,
so it continues to test the exact reviewed patch after that patch has been applied.
Offline preparation/accounting output no longer falsely states authorization is missing.

Executed on implementation commit `ac2121d`, with actual process exit codes **0**:

- `cargo fmt --check`
- `cargo test --offline --all-targets`: **46 passed**, two subprocess helpers ignored
  at top level and exercised by parent tests.
- `cargo clippy --offline --all-targets -- -D warnings`
- `python3 -m unittest discover -s evaluation/phase3 -p 'test_*.py'`: **25 passed**.
- Build and execute the local extraction revalidator on the identical seven frozen inputs.
- Frozen 12-case source/reference/candidate/provenance audit.
- Focused archive preparation and full archived extraction replay.

The Rust suite includes production approval/rejection, Korean routing safety,
cross-source authority negatives, budget concurrency/restart, HTTP/MCP authorization,
pagination and publication crash recovery. No model verdict or test bypasses exact-hash
user-only publication. All model-quality tests remain separate from deterministic safety.
Full command arguments, tested HEAD, exit codes and log hashes are in
`reports/evidence/phase-3/authorized-focused/verification-commands.json`.

## Implemented but unverified — unchanged semantic acceptance

Before/after frozen results remain 12 selected (8 Korean / 2 mixed / 2 English),
7 available current-profile batches, 11 Claims, 5 missing, 3 archived protocol-eligible,
3 reviewer-rejected and 1 incomplete batch. Assistant editorial inspection remains
5 pass / 0 fail / 2 unresolved batches. Rejected/incomplete/missing batches do not
count as successful extraction. Editorial-plus-protocol valid Claim counts remain
5 overall and 1 Korean; independent valid semantic coverage is **unknown**.

Missing outputs: `new-003`, `new-007`, `new-018`, `new-025`, `new-042`.
`r1-ambiguous_ref` retains the identical-text reviewer flip. `v23-fresh-003` retains
its qualification-versus-independent-report dispute. No new primary or repeated
Haiku judgment exists. No empirical improvement in reviewer reliability or Korean
semantic quality is claimed. The frozen manifest, references, protocol and original
Sources are unchanged; no new production Claims were published.

Full archive replay remains 5,281 rows / 4,752 distinct payloads / 145 Source hashes;
3,646 structural passes, 4,502 plan-hash passes and 2,624 experimental-gate-eligible
payloads. Structural/protocol counts are not semantic accuracy.

**Deferred:** cross-source semantic support and safe arbitrary-document chunking.
Existing unresolved-authority refusal, 256 KiB intact request refusal, explicit
Korean-capable Plan configuration, DeepSeek English-only default/Unicode guard,
source immutability, provenance and exact-hash user approval remain in force.

## Requests, completion and commits

OpenRouter API calls during this continuation: **8 GETs, 0 completion POSTs**:
two public model preflight GETs; two read-only checks of the same key; three GETs
inside the guarded attempt; one management capability GET. Web documentation lookup
is separate. Confirmed paid requests/tokens/cost: **0 / 0 / $0.00**.
Request rows and pending reservations remain zero; authorized local budget remains
**$0.10**, subject to the provider cap and conservative per-request reservations.

Phase 3 cannot be declared complete until the guarded evaluations can run, the five
missing candidates are obtained under an explicit bounded plan, disputed cases and
all required semantic dimensions have independent adjudication, Korean quality and
reviewer disagreement are measured for the claimed scope, and any needed wider
independent acceptance evidence exists. Twelve diagnostic cases alone do not prove
universal correctness. No phase-3 tag was created.

Commit `ac2121d` contains the applied authorized policy and deterministic regressions;
the delivery commit contains this report and sanitized evidence. Existing phase-1
and phase-2 tags are unchanged. Existing untracked `.yeet/` is preserved. Foundation
Memory was consulted and updated; Jev remains unavailable in the tool inventory.
