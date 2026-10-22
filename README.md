# Prime Radiant

Phase 1 Persistence Core: a synchronous Rust library and local developer CLI.
Design contracts: [architecture](ARCHITECTURE.md), [data](DATA_CONTRACTS.md),
[decisions](DECISIONS.md), [implementation plan](IMPLEMENTATION_PLAN.md),
[Phase 1 specification](PHASE1_SPEC.md).

```sh
cargo build --locked
cargo test --offline --locked
cargo clippy --offline --locked --all-targets -- -D warnings
cargo fmt --check
```

The CLI defaults to `~/Library/Application Support/PrimeRadiant/prime_radiant.db`.
Use `--db /absolute/path` for another local database. CloudStorage and Mobile
Documents paths, including symlink aliases, are refused.

```sh
primeradiant --db /tmp/notes.db init
printf '  한글 English 👩‍💻\r\n' | primeradiant --db /tmp/notes.db record --principal user --idempotency-key note-1
primeradiant --db /tmp/notes.db get --principal user SOURCE_ID
primeradiant --db /tmp/notes.db lifecycle --principal user SOURCE_ID forget
primeradiant --db /tmp/notes.db lifecycle --principal user SOURCE_ID restore
primeradiant --db /tmp/notes.db check
primeradiant --db /tmp/notes.db backup /tmp/notes-backup.db
primeradiant --db /tmp/notes.db restore /tmp/notes-backup.db
primeradiant --db /tmp/notes.db migrate
```

`record` reads unmodified UTF-8 stdin unless `--content` is supplied. Timestamps
are optional RFC 3339 metadata normalized to UTC milliseconds. Outputs are JSON;
diagnostics go to stderr and failures have nonzero exit status. `--principal`
is required for reads/writes and is an explicit **local developer** authority
selection, not authentication. Transport authentication belongs to Phase 2.
The library's `Principal::for_local_cli` is the same explicit CLI/test boundary;
requests have no recorder field.

One `App` owns each database. Its writer is serialized with a mutex and uses
`BEGIN IMMEDIATE`; readers use independent read-only connections. Ownership is
protected by an OS advisory lock on `<db>.lock`, released on exit or SIGKILL.
Lock files may remain and should not be removed while an App is running.
External SQLite writers do not participate in this ownership protocol and are
unsupported.

Backups are verified online SQLite snapshots, published by atomic rename to a
new destination. Backup artifacts use rollback-journal mode so they are complete
single files; live database connections use WAL/FULL. Each migration first
creates `<db>.pre-vN.<ULID>.bak`. Restore requires both files to be offline,
validates before replacing the target, stages a verified copy, and preserves the
old target as `<db>.replaced.<ULID>` with any old SQLite sidecars. Retain these
files for recovery; automatic retention/purge is outside Phase 1. An interrupted
restore may require selecting a staged or replaced file manually. A pre-v1
empty snapshot is valid for recovery and initializes on the next open.

Integrity checks cover SQLite/FKs, source hashes, migration history/checksums,
event targets, and required triggers. Failure refuses startup or latches an
existing App against further writes. Restore a verified backup to recover;
there is no force-write or unsafe-location bypass.

See [Phase 1 validation](reports/phase-1.md) for acceptance evidence and the
network-denied test command. No model, HTTP, MCP, queue, or retrieval components
are included in this phase.
