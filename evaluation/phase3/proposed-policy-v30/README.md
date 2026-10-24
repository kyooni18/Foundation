# Inert proposal: $0.05 additional Phase 3 capacity

**Not applied. Not authorization.** The current ceiling remains $4.88265335.
The user must explicitly authorize this additional capacity before any real change.

The proposal raises the same cumulative ceiling to **$4.93265335**, still strictly
below $5. Historical prior ($4.78265335), all 40 request records and their charges
remain unchanged. It adds $0.05 to the existing $0.10 continuation allowance.
There is no new ledger, account, provider, automatic retry or funding reset.

- `ceiling.patch` changes only the Rust maximum and the Python maximum/continuation
  constants. It has passed `git apply --check` in an isolated file copy.
- `ceiling.sql` requires the exact audited 40-request, fully settled checkpoint
  before changing only `budget.ceiling`. It was executed on a private SQLite
  backup; all request rows and historical prior were compared unchanged. The
  real ledger remains untouched by this proposal.
- `authorization-template.json` deliberately has `explicit_user_authorization=false`.
  It cannot dispatch. A distinct actual authorization receipt is needed after a grant;
  historical authorization files must not be edited.
- `proposal.json` records the amounts and performed checks.

Current remaining capacity is $0.00294530; the next v30 nested-report review
requires a $0.00449180 reservation. More capacity permits validation; it does not
prejudge semantic acceptance. Further changes in ledger state require this proposal
to be audited again before applying its guarded SQL.
