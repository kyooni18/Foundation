# Spending-policy proposal — authorized and applied

**Applied after explicit human authorization** following delivery commit `1e2a74b`.
The authorization and before/after ledger receipt are preserved in
`reports/evidence/phase-3/authorized-focused/`. The exact patch and SQL remain as
reviewed historical artifacts; they must not be applied again. No startup code loads them.

Authorized exact cumulative ceiling: **$4.88265335**, the recorded conservative prior
$4.78265335 plus at most $0.10. This stays $0.11734665 below $5.00. The patch changes
the existing Rust and Python hard limits together. The SQL changes only the existing
ledger ceiling in place after checking the exact audited initial state; prior charges
and request rows are never deleted. It refuses any intervening request or changed prior.
Application verified no running workers, checked the audited accounting, applied
both code constants and the transaction, and verified the prior and request rows.

The unchanged dispatcher still uses the existing ledger, conservative reservations,
12 primary / 3 designated repeat maximum, $0.10 persistent continuation cap, unknown
charge holds, no fallback, and fresh model/endpoint/key-cap validation. The stricter
cumulative ceiling also prevents general Rust requests from using more than the same
$0.10 aggregate headroom. Authorization does not promise enough budget for 15 calls
or authorize extra extraction calls to fill the five missing packets.

Provider caps remain untouched. If the existing provider cap prevents dispatch,
separate explicit authorization for an in-place provider-cap amendment is required;
there is no proposed provider mutation here because no current authenticated cap
inspection was performed. No new key, account or ledger exists. The explicit user grant is now recorded.

Tests apply these exact artifacts only to temporary source copies and fixture databases.
They verify unchanged historical accounting, refusal on changed state, the new ceiling,
the authorized policy boundary, reservations, concurrency, and overruns.
The Rust patch is also compiled and tested in an isolated exported checkout during
this continuation. The latest user grant supplies permission for this local ceiling amendment.
Paid dispatch remains blocked by the existing uncapped credential, not by missing
authorization for this amendment.
