# Unapplied spending-policy proposal

**Blocked — explicit user authorization is missing.** These files are review material,
not authorization, a migration, or an alternate dispatcher. No startup code loads them.

Proposed exact cumulative ceiling: **$4.88265335**, the recorded conservative prior
$4.78265335 plus at most $0.10. This stays $0.11734665 below $5.00. The patch changes
the existing Rust and Python hard limits together. The SQL changes only the existing
ledger ceiling in place after checking the exact audited initial state; prior charges
and request rows are never deleted. It refuses any intervening request or changed prior.
A future application must stop workers, obtain explicit user authorization, recheck
accounting, apply both code changes and the transaction, and verify before restarting.

The unchanged dispatcher still uses the existing ledger, conservative reservations,
12 primary / 3 designated repeat maximum, $0.10 persistent continuation cap, unknown
charge holds, no fallback, and fresh model/endpoint/key-cap validation. The stricter
cumulative ceiling also prevents general Rust requests from using more than the same
$0.10 aggregate headroom. Authorization does not promise enough budget for 15 calls
or authorize extra extraction calls to fill the five missing packets.

Provider caps remain untouched. If the existing provider cap prevents dispatch,
separate explicit authorization for an in-place provider-cap amendment is required;
there is no proposed provider mutation here because no current authenticated cap
inspection was performed. No new key, account, ledger, or authorization record exists.

Tests apply these exact artifacts only to temporary source copies and fixture databases.
They verify unchanged historical accounting, refusal on changed state, the new ceiling,
continued exhaustion of the real policy, reservations, concurrency, and overruns.
The Rust patch is also compiled and tested in an isolated exported checkout during
this continuation. Passing these tests is not permission to apply the proposal.
