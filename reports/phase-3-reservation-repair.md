# Phase 3 reservation correction (2026-10-24)

Starting commit: `853e657`. Implementation: `8992fb1`.

The next targeted review's reservation fell from **$0.01596040 to $0.00698020**,
a **56% reduction**. Its output limit remains 6,000 tokens. This removes the prior
reservation blocker while preserving the $4.88265335 aggregate ceiling. It reduces
reserved capacity; it does not promise half the actual API charge.

The old calculation doubled a bound that already charged one input token per byte
of the entire serialized request plus 4,096 framing tokens, and reserved 8,000 output
tokens even when the request allowed only 6,000. The corrected policy
`utf8-byte-bound-request-output-v2` retains the input/overhead bound, selects the
price tier from that bound, and prices the request's actual validated output cap.
No prompt, model, semantic checks or output limit was shortened. Halving the output
limit would risk truncation: some recent complete reviews used over 3,000 tokens,
and a historical 2,400-token attempt already truncated.

An offline audit checked token usage from all **33 prior direct-Anthropic requests**
against the new bounds at the verified current rates. Every request fit. Historical
ledger amounts and reservations remain unchanged, including the earlier attempt
accounted at higher conservative rates; the audit does not reprice the ledger.

All **40 Python tests pass**, including a regression checking the exact bound,
coverage of worst-case usage, and rejection of missing, boolean, nonpositive,
noninteger or oversized output caps. Rust was unchanged in this follow-up. The
last full Rust suite, formatting and Clippy checks remain passing at `cb2c98b`.
Saved semantic responses replay under the existing protocol without reclassification.

One previously blocked call was then completed: v28 reviewed the saved v25
`v23-fresh-003` extraction. It returned a **protocol-valid pass** on its two separate
Claims, with two source propositions inventoried. The response used 6,219 input and
2,802 output tokens; its conservative usage upper bound is **$0.00264480**. It is
still diagnostic evidence, not a full reader/three-review gate or user approval.

Final ledger snapshot:

| Item | USD |
| --- | ---: |
| Historical carried amount | $4.78265335 |
| Recorded requests (34) | $0.08874300 |
| Cumulative conservative accounting | **$4.87139635** |
| Authorized ceiling | $4.88265335 |
| Remaining | **$0.01125700** |

Zero requests are unsettled. Provider billed USD remains unknown. No automatic
retry, historical charge reduction, budget reset or ceiling increase occurred.
Phase 3 remains incomplete because full semantic acceptance evidence is missing;
the budget no longer blocks this particular review.

Evidence: `reports/evidence/phase-3/reservation-v2/` contains the before-call audit,
current replay and acceptance snapshot. The new immutable request/response/accounting
sidecars are in `reports/evidence/phase-3/semantic-repair/` under
`focused-haiku-repair-v28-v23-fresh-003-extraction-v25-primary`.
