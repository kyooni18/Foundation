# Prime Radiant

Phase 2: immutable persistence, authenticated loopback HTTP, and stdio MCP.
The application core remains synchronous; transports run it on Tokio blocking workers.
Design contracts: [architecture](ARCHITECTURE.md), [data](DATA_CONTRACTS.md),
[decisions](DECISIONS.md), [implementation plan](IMPLEMENTATION_PLAN.md),
[Phase 1 specification](PHASE1_SPEC.md).

```sh
cargo build --locked
cargo test --offline --all-targets
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
selection for offline maintenance, not transport authentication. These commands
require exclusive DB access and cannot run alongside `serve`.
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

See [Phase 1 validation](reports/phase-1.md) for persistence evidence and
[Phase 2 validation](reports/phase-2.md) for authentication and transport tests.
Claims, models, extraction, search, embeddings, and queues remain outside scope.


## Authenticated service and MCP

Create credentials in a private directory using real paths without symlink
components (on macOS, `/tmp` is a symlink; use its canonical path if needed).
Each invocation issues a fresh independent token. Tokens are written directly to
0600 files; the command prints only `{"issued":true}`. The registry stores token
hashes and trusted role/label assignments, not raw tokens.

```sh
mkdir -p "$HOME/.config/primeradiant"
chmod 700 "$HOME/.config/primeradiant"
primeradiant credential-issue --credentials "$HOME/.config/primeradiant/credentials.json" --token-file "$HOME/.config/primeradiant/user.token" --kind user
primeradiant credential-issue --credentials "$HOME/.config/primeradiant/credentials.json" --token-file "$HOME/.config/primeradiant/agent-a.token" --kind agent --label agent-a
primeradiant credential-issue --credentials "$HOME/.config/primeradiant/credentials.json" --token-file "$HOME/.config/primeradiant/import.token" --kind import
primeradiant serve --credentials "$HOME/.config/primeradiant/credentials.json"
```

`serve` defaults to `127.0.0.1:7823` and refuses non-loopback bindings. It owns the
database. Start an MCP bridge per agent with that agent's token file; bridges
connect to the running server and never open SQLite:

```sh
primeradiant mcp --server http://127.0.0.1:7823 --token-file "$HOME/.config/primeradiant/agent-a.token"
```

Configure these arguments in the MCP client's stdio server configuration. The
bridge speaks MCP `2025-06-18`: initialize, initialized notification, tools/list,
tools/call, and ping, one JSON-RPC message per line. Stdout contains protocol
messages only. Never put raw tokens in tool arguments or CLI command arguments.

HTTP uses `POST /v1/{record|get|list|retract|forget}` with a JSON body and
`Authorization: Bearer <token>` supplied by the client. Requests have no role or
recorder field. A local application's HTTP client should read the private token
file into its Authorization header; do not print or log that header. The MCP
bridge does this internally. `/v1/authenticate` is an empty authenticated probe
used at bridge startup, not a tool or user-data endpoint.

| Operation | JSON fields |
|---|---|
| record | `content` required; optional `media_type`, `authored_at`, `observed_at`, `artifact_ref`, `retention_request`, `idempotency_key` |
| get | `id` required; optional `include_retracted` (default false) |
| list | optional `limit` (1–100, default 50), `cursor`, `include_retracted` (default false) |
| retract / forget | `id` required; optional `reason` (up to 4 KiB) |

Unknown fields are rejected. Both transports return the same application
envelope: `{"ok":true,"result":...}` or
`{"ok":false,"error":{"code":"not_found","message":"not found"}}`.
MCP places it in `structuredContent` and a JSON text content block, with
`isError=true` on application failures. HTTP uses 200/400/401/403/404/409/500
according to the result and sets `Cache-Control: no-store`. Browser Origin
requests are rejected; there is no CORS, cookie authentication, proxy, redirect,
or remote HTTP support in this phase.

User get/list contains only live user-recorded sources. Agent/import get/list
excludes forgotten sources and excludes retracted sources unless requested.
Import cannot change lifecycle. Agents can change agent-recorded sources within
the existing shared recorder-role scope. Independent credentials do **not** add
per-agent ownership or change the shared agent idempotency-key namespace; choose
unique keys across agent clients. User lifecycle administration retains D-30 and
can target hidden sources; its command results are not projection reads.
Source confirmation is not exposed.

List results contain `sources`, a projection-only `total`, and `next_cursor`.
Cursors are bound to the credential/filter and expire on server restart. Each
page reads one consistent snapshot, but the traversal is a live view: changes
before the cursor are not revisited, and later pages recheck visibility. No
global offsets, event sequences, or hidden counts are sent.

Credential files must remain regular, single-link, owned by the effective OS
user, and exactly 0600. Missing, corrupt, symlinked, incorrectly permissioned,
or unknown credentials fail closed. The server reopens the registry on every
request; the MCP bridge also rechecks its token file for every tool call. To
revoke a credential, remove its entry from the private registry while preserving
0600 permissions (prefer atomic replacement); provisioning is available through
`credential-issue`. There is no automatic expiry or remote credential management.
Files accessible to the same OS user are within the trusted local boundary;
tokens do not sandbox malicious processes running as that same OS user.

## Phase 3 implementation checkpoint (acceptance pending)

Sources now enqueue durable extraction work. The optional OpenRouter worker proposes
atomic, source-grounded Claims through separate atomicity, fidelity, and coverage reviews.
All proposals remain quarantined for local user review because real-model quality has not
been evaluated. There is **no phase-3 completion tag**. See [the Phase 3 report](reports/phase-3.md)
for evidence, decisions, limitations, and the reproducible evaluation procedure.

Provider credentials use `OPENROUTER_API_KEY` or `OPENROUTER_API_KEY_FILE` (canonical path,
0600, same owner, no symlinks). Model and reviewer names must be supplied explicitly.

```sh
primeradiant --db /path/to/db extraction-backfill
primeradiant --db /path/to/db extraction-work --model "$EXTRACTION_MODEL" --reviewer "$REVIEW_MODEL"
primeradiant --db /path/to/db extraction-inspect
# Review the original Source and the entire exact candidate before approving fidelity:
primeradiant --db /path/to/db extraction-approve CANDIDATE_ID OUTPUT_SHA256
primeradiant --db /path/to/db claims --principal user
```

For continuous processing, `serve` accepts `--extraction-model` and
`--extraction-reviewer`; the worker shares the sole database owner. Offline CLI maintenance
requires stopping that server. HTTP/MCP also expose `claim_get`, `claim_list`,
`claim_retract`, `claim_forget`, `claim_confirm`, and `claim_unconfirm`, all through the
same core authorization. Extraction approval does not confer user endorsement.
