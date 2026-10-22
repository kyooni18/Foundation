# Phase 1 validation — 2026-10-22

Implemented from Phase 0 commit `70b5e35`. Tested locally on macOS arm64,
Rust/Cargo 1.96.0, bundled SQLite via rusqlite 0.37.0. No research benchmark
results were reused. The direct dependencies are all from PHASE1_SPEC's list;
no additional direct dependencies were needed.

| Contract | Verification | Result |
|---|---|---|
| T1 | Exact Korean NFC/NFD, English/mixed, ZWJ emoji, CRLF, whitespace, tab, NUL bytes and SHA-256; input bounds and timestamp metadata | Pass |
| T2 | Direct SQL UPDATE/DELETE on source, status_event, and schema_migration | All rejected |
| T3 | Same/different digests, NULL key duplicates, principal scopes, metadata coverage, label/session exclusions | Pass |
| T4 | 32 barrier-synchronized requests, one key | One row, one created=true, identical IDs |
| T5 | Four writers × 1,000 distinct keys plus four readers checking stored content | 4,000 rows, zero BUSY/errors, integrity OK |
| T6 | Twenty child processes, fresh keys per restart, jittered SIGKILL after first commit; verify acknowledged IDs and all row content | Pass |
| T7 | v1 initialization/reopen; checksum tampering; test-only v2 and too-new rejection; pre-v2 restore | Pass, original v1 data retained |
| T8 | Drop immutability trigger and corrupt source content | Hash and trigger violations reported; later writes refused |
| T9 | Online backup during writes; restored rows/hash comparison; corrupt file rejection; old target preserved; active-target refusal | Pass |
| T10 | Agent retention_request=user_requested | Stays agent, hidden from user get |
| T11 | Core and direct-SQL permission failures; user transitions and no-ops | Pass |
| T12 | Both cloud directory families, nonexistent descendants, symlink aliases; second owner refusal | Pass |
| T13 | Full dependency-tree review and network-denied execution | No HTTP client/model SDK; all tests pass with networking denied |

The suite has 12 acceptance tests. Its additional ignored test is the crash
worker entry point: T6 explicitly launches it in each of its 20 child processes.
T13 is an environment/dependency audit, not an extra Rust compile-time test.

Commands used:

```sh
cargo test --offline --locked -- --nocapture
/usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' cargo test --offline --locked -- --nocapture
cargo tree --offline --locked --prefix none
cargo clippy --offline --locked --all-targets -- -D warnings
cargo fmt --check
```

A control socket connection to 127.0.0.1 under the same sandbox failed with
EPERM, confirming network denial rather than relying on Cargo offline mode.
The network-denied run recorded 4,000 writes and 1,595 reads in approximately
1.07 seconds, with zero BUSY errors; 203 acknowledged records were checked over
20 SIGKILL cycles. These are local observations, not performance guarantees or
power-loss testing.

CLI operations were exercised through the built binary in a temporary
directory: init, migrate, record, get, lifecycle forget/restore, check, backup,
and restore. Verification included exact stdin bytes (NFD/emoji/CRLF/tab/NUL),
idempotent retry, conflict exit status, agent visibility, forbidden transition,
forgotten-source restore, successful integrity output, and replaced-file
preservation. All passed.

D-30 in DECISIONS.md documents the necessary distinction between user lifecycle
administration and the user read projection. Existing accepted choices remain:
WAL/FULL, single writer, server-derived recorder, immutable content, append-only
history, no normalization, and forward-only migrations with backups. The lock,
NULL digest encoding, and standalone backup format are implementation details.

Jev was unavailable. Foundation Memory was used to recover Phase 0 context and
retain implementation and validation knowledge. The pre-existing `.yeet/`
directory was left untouched and untracked.
