# Phase 2 — Authorship and Authorization

Implemented on 2026-10-22 from the completed Phase 1 tree. The immutable annotated
`phase-1` checkpoint still resolves to `50dbccb34fc3b5960fe02de2af56c8c33f7b5916`.
Implementation checkpoints: `1e8f835` (credentials/core/transports), `cc0f397`
(authorization and actual transport tests). Final documentation completes the
`phase-2` checkpoint.

## Implementation

- `authentication`: operator provisioning, independent 256-bit OS-random bearer
  tokens, SHA-256 registry digests, constant-time digest comparison, private
  Principal construction. Credential files must have exact 0600 mode, correct
  effective UID ownership, regular-file type, one hard link, and no symlinks.
  File content reads are bounded. Invalid input, files, and authentication errors
  never include token bytes. Principal Debug output excludes credential material.
- `authz` and `App`: role-derived recorder, lifecycle authorization, default-live
  reads and explicit history reads. The single user is a role without a user ID.
  Client-supplied recorder/principal/speaker/basis/confirmation fields are rejected.
  Retention requests do not promote authorship or visibility.
- `list`: SQL projection before counting/limiting, one read transaction per page,
  ULID keyset traversal, HMAC-SHA256 cursor authentication, credential/filter
  binding. Hidden records do not affect result totals, page fullness, cursor
  content, visible metadata, or NotFound responses.
- `api`: one shared operation dispatcher, strict request shapes, and sanitized
  success/error envelopes. HTTP dispatches to App; MCP forwards to that same
  dispatcher. There is no transport-specific role policy or SQL.
- `transport::http`: axum on numeric loopback addresses, one Tokio runtime,
  blocking workers for authentication/SQLite, bounded request bodies, no-store
  responses, bearer-only authentication, and browser Origin rejection.
- `transport::mcp`: separate stdio bridge to the sole DB-owning process. No DB
  connection, proxies, redirects, or remote endpoint support. Bounded newline
  framing, initialization gating, ping, tool discovery, tool calls, notifications
  without side effects, and structured application errors. Protocol version is
  explicitly `2025-06-18`, following the official
  [MCP schema](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-06-18/schema.ts).
- CLI: `credential-issue`, `serve`, and `mcp`, alongside all existing offline
  maintenance commands. Provisioning writes tokens directly to private files and
  never prints them. Defaults do not synthesize a user/agent identity on auth
  failure.

New dependencies and their reasons are recorded in commit `1e8f835`: axum/Tokio
for server/runtime, reqwest (default features disabled) for the loopback bridge,
getrandom for entropy, hmac/subtle for authentication primitives, libc for Unix
file flags/effective UID. There are no model SDKs or extraction/search/embedding
components. The Phase 1 no-HTTP dependency requirement was phase-scoped; Phase 2
explicitly introduces HTTP. The original Phase 1 tag remains intact.

## Contract resolutions and compatibility

D-31–D-34 in DECISIONS.md document the implementation boundaries before
acceptance. Independent agent credentials retain the accepted recorder-role
scope, including shared agent lifecycle rights and shared idempotency keys.
Labels/session metadata never become authorization keys. User lifecycle
administration retains D-30, including access to hidden targets; get/list remains
strictly projected. Role-scoped idempotent record receipts remain D-26.

No schema migration is necessary: all Source fields, authoritative tables,
triggers, and migration checksums remain unchanged. No destructive schema work
was performed. Forward-only migration/backups remain covered by Phase 1 tests.

An additional compatibility experiment built the actual `phase-1` source from
`git archive` in a temporary directory, offline. That binary created a database
with user and agent records and a retraction event. The Phase 2 binary opened it,
ran check/get/backup/restore, and preserved every source, status_event, and
schema_migration row exactly. No extra migration or pre-migration backup was
created. Actual CLI credential provisioning also verified mode 0600 and absence
of raw token material in registry/output.

## Validation

Required commands, all passing on macOS arm64 with Rust/Cargo 1.96.0:

```sh
cargo fmt --check
cargo test --offline --all-targets
cargo clippy --offline --all-targets -- -D warnings
```

There are 22 passing acceptance tests, plus the existing ignored crash-worker
entry point that is explicitly launched 20 times by its parent test. All Phase 1
tests are retained. Tests use temporary databases/credentials; the new network
tests use only actual loopback connections, with real server and MCP subprocesses.
Cargo offline mode means no dependency fetch; it does not disable those loopback
integration connections.

| Coverage | Evidence |
|---|---|
| Core authorization | 108 actor/recorder/lifecycle/transition combinations, including two independently authenticated agents; missing IDs, default and explicit-history reads |
| Credentials | Valid/invalid/missing tokens; exact file modes including 0644/0640/0666/0400/0700; symlink/hard-link rejection; corrupt registry; permission changes; independent revocation |
| Metadata impersonation | HTTP and MCP reject recorder, principal, speaker, basis, recorder_label, session_id, confirmed, credential, actor request fields; user_requested stays agent |
| Transport equivalence | Actual HTTP requests versus real newline-JSON-RPC MCP subprocesses for record/get/list/retract/forget; equal success and error envelopes |
| Lifecycle | Forbidden agent changes to user/import records; import lifecycle denial; retracted opt-in; forgotten exclusion; user administration; no-op behavior |
| Pagination | Multiple complete pages without duplicates, projection-only totals, hidden-record invariance, cursor tampering/cross-role/cross-credential/filter-change rejection, live visibility recheck |
| Error isolation | Missing and hidden IDs return identical NotFound; conflicts omit IDs; malformed requests and auth failures do not echo input or credentials |
| Protocol boundary | Initialization gating, no notification-triggered writes, unknown tools, invalid arguments, Origin rejection, duplicate Authorization rejection, numeric loopback endpoint enforcement |
| Persistence | Original exact-byte, immutability, idempotency, concurrency, 20 SIGKILL cycles, migration restore, integrity latch, backup/restore and unsafe-path tests pass |

The research rationale identifies six leakage paths: retrieval, aliases,
counts, summaries, derived edges, and dependency-count metadata. Phase 2 verifies
retrieval through Source get/list and counts through `total`. For the remaining
four, tests verify that unsupported operations fail and that those fields are
absent from Source output/tool surfaces. Hidden agent/import insertion leaves
complete user page responses and visible Source metadata unchanged. Errors and
pagination cursors receive additional explicit isolation checks. This does not
claim to validate future Claim/search/graph functionality that has not been built.

## Known limitations

- Unix owner/mode semantics are required for credentials. Unsupported platforms
  fail closed; Windows ACL support is not implemented.
- Credentials authorize local clients, not mutually hostile processes under the
  same OS account. Those processes can already access the DB and private files.
  Phase 1 offline maintenance remains an explicit trusted filesystem boundary.
- Agent permissions and idempotency keys remain shared by recorder role, not by
  individual token. Per-agent ownership would require a new data contract and a
  legacy-record ownership policy.
- HTTP is loopback-only without TLS. There is no remote deployment, OAuth,
  automatic token expiry, rate limiting, or credential-management API. Revocation
  is an operator registry edit; already authenticated in-flight requests may finish.
- MCP supports the documented 2025-06-18 tools subset, not newer protocol eras,
  resources/prompts, streaming, cancellation, or server-initiated requests.
- Pages are live views, not durable snapshots. Cursors expire on restart and
  readers recheck visibility; concurrent changes before the cursor are not replayed.
- Source-level confirmation, Claims, LLM extraction, search, embeddings, queues,
  and derived summaries/aliases/relationships remain outside this phase.

Foundation Memory was used to recover and retain project context. No Jev tools
were available. The existing `.yeet/` directory remains untouched and untracked.
