# Prime Radiant — local Compose operations

Run commands from `~/Code/PrimeRadiant`. Docker must be running. No host HTTP port is
published; the API remains `127.0.0.1:7823` **inside** the container. Restart policy is
`no`; nothing starts at login. macOS Metal is not available to Linux containers.

## First run

```sh
# Optional: cp .env.example .env   # non-secret configuration only
# Build before provisioning. Never point this deployment at a development DB.
docker compose build
docker compose run --rm radiant init
docker compose up -d --build --wait
docker compose ps
docker compose logs --tail 40 radiant
```

Initialization provisions independent user/agent tokens only once. Re-running init
validates that both files exist; it does not rotate tokens. Partial provisioning fails
closed: investigate/restore the credentials volume, do not delete it to fix startup.
Normal startup never issues credentials. Changing the Compose project name selects
**different volumes**; keep the project name stable in all operations and MCP clients.

Default `EMBEDDING_MODE=off` is an **explicit degraded mode**: asynchronous lexical
indexing plus exact/structured/graph retrieval, not semantic/cross-language matching.
Korean and English lexical queries work on matching approved text; recording a Source
alone does not create searchable Claims. No extraction provider is enabled by default.
Sources stay `stored_not_extracted`, with pending extraction jobs. This is not data loss.
The logs explicitly report that extraction is unavailable. See [verification](VERIFICATION.md)
for measured CPU results and the reason for this conservative default.

The host model directory defaults to
`~/Library/Application Support/PrimeRadiant/models/nemotron-3-embed-1b`, mounted at
`/model` read-only. Override `MODEL_DIR` in `.env` only if necessary. A missing path
fails rather than creating an empty model directory. No download or model copy occurs.
CPU inference exhausted the current engine's memory during initialization; it is
**not a verified usable mode on this machine's current Docker allocation**. The
default `MEMORY_LIMIT=1g` protects other services. Only experiment with
`EMBEDDING_MODE=local` after increasing both Docker VM memory and `MEMORY_LIMIT`
substantially (the probe already exceeded 6 GiB, with no completed inference).
Then `docker compose up -d --force-recreate`. Weights load lazily;
`model-check` alone is not an inference test. No larger-memory success is claimed.

## Exact MCP configuration

**Default MCP exposes four tools only:** `record` (save a raw Source),
`search` (find approved Claims), `get` (inspect a Source) and `forget`
(forget a Source). Search planning is automatic; agents are not shown
queue controls, Claim administration, identity editing or other maintenance
operations. Recording does not publish Claims until extraction and explicit approval.

For an agent MCP client (standard newline JSON-RPC stdio, no shell wrapper or TTY):

```json
{
  "mcpServers": {
    "prime-radiant": {
      "command": "/opt/homebrew/bin/docker",
      "args": [
        "compose", "--project-directory", "/Users/kyooni18/Code/PrimeRadiant",
        "-p", "primeradiant", "exec", "-T", "radiant",
        "primeradiant", "mcp", "--token-file", "/credentials/agent.token"
      ]
    }
  }
}
```

The executable path is verified on this machine (`command -v docker`). This exact
project is the default for this repository. For a
trusted **owner-only** client use a separate server entry with
`/credentials/user.token`; do not give that configuration to agents. The bridge reads
tokens from files, never argv/environment or logs. stdout is protocol-only. Restart
an existing MCP connection after a container replacement; the same configuration and
persistent token work again. Docker access itself is administrator access: this is
not isolation against a hostile host process with arbitrary `docker exec` privileges.

Useful default MCP calls (each is a separate `tools/call` request):

```json
{"name":"record","arguments":{"content":"서버는 SQLite를 사용한다. The server uses SQLite.","idempotency_key":"my-note-1"}}
{"name":"search","arguments":{"query":"SQLite"}}
{"name":"get","arguments":{"id":"SOURCE_ID"}}
{"name":"forget","arguments":{"id":"SOURCE_ID"}}
```

For *explicitly trusted administrative clients only*, append `--full-tools`
to the `primeradiant mcp` arguments to expose the original 29 operations,
including `source_processing`, `queue_status`, `queue_jobs`, and `queue_retry`.
The HTTP API and its authorization rules remain unchanged. Retained advanced
tools are not required for ordinary memory use. Never give the user token
or full administrative configuration to an ordinary agent.
User and agent projections remain intentionally distinct.

Authenticated readiness (does not open a second DB owner):

```sh
docker compose exec -T radiant python3 /opt/radiant/health.py
# Owner-only full MCP (with --full-tools) can inspect queue_status/queue_jobs.
# The default four-tool MCP surface intentionally does not advertise queue internals.
```

## Optional extraction (explicit authorization required)

No paid calls were used in deployment verification. OpenRouter is optional and
fail-closed on missing budget/key. Configure both `EXTRACTION_MODEL` and
`EXTRACTION_REVIEWER` in `.env`; choose an independent reviewer. Empty
`EXTRACTION_PROMPT_VERSION` preserves the application's existing audited default;
no experimental prompt was promoted. To provision **after authorizing spending**:

```sh
docker compose stop radiant
# Key comes over stdin, never command arguments; use an existing protected key file.
docker compose run --rm -T --no-deps --entrypoint sh radiant -ec \
  'umask 077; test ! -e /credentials/openrouter-key; cat > /credentials/openrouter-key; test -s /credentials/openrouter-key' < /secure/path/openrouter-key
# Example hard lifetime budget; explicitly account for any prior spending.
docker compose run --rm radiant budget-init /credentials/budget.db --limit-usd 1 --prior-spend-usd 0
docker compose up -d --force-recreate
```

The persistent ledger is never automatically reset or increased. Never roll back an
active spending ledger during a memory restore (that could permit spending twice).
When recovering a lost ledger, reconcile provider spending before enabling extraction.
Provider failures keep evidence and jobs with bounded retries. Source → queue →
extractor → independent review → quarantine → **explicit exact-hash user approval** →
Claims → asynchronous indexes remains intact. Reviewer success alone never publishes.

Owner review/approval currently uses the existing trusted offline CLI, not an agent
tool. Stop the service for all DB-owning CLI commands:

```sh
docker compose stop radiant
docker compose run --rm radiant extraction-inspect
docker compose run --rm radiant extraction-review CANDIDATE_ID
# Inspect source evidence and proposal; explicitly approve the exact displayed hash.
docker compose run --rm radiant extraction-approve CANDIDATE_ID OUTPUT_HASH
docker compose up -d --wait
```

When a budget/provider becomes available, start/recreate the configured service. For
terminal failed/deferred jobs use owner `queue_retry`; pending jobs respect backoff.
Do not bulk re-extract historical work or change prompts just to clear the queue.

## Storage, backup and restore

| Named volume (default project) | Container location | Contents |
|---|---|---|
| `primeradiant_data` | `/data` | `radiant.db`, WAL/SHM/lock and migration safety snapshots |
| `primeradiant_credentials` | `/credentials` | registry, user/agent token files, optional key and spending ledger |
| `primeradiant_backups` | `/backups` | verified standalone SQLite snapshots and protected credential archives |

UID/GID 10001 owns all three directories (0700); credentials and new files use 0600
under umask 077. Runtime is non-root, read-only root filesystem, dropped capabilities,
no-new-privileges, temporary `/tmp`. SQLite retains WAL + synchronous FULL, transactions,
exclusive process ownership and backward-compatible existing migrations. A second CLI
owner refuses to open a running DB. No existing host DB is mounted or imported.

```sh
# Stops gracefully, uses SQLite Backup API + integrity checks, archives credentials
# only while stopped, then restarts. On failure it deliberately leaves service stopped.
./scripts/compose-backup
```

Do not copy a live `radiant.db` or its sidecars. The script's resulting
`/backups/TIMESTAMP.db` is standalone and safe to copy. Named volumes alone do not
protect against Docker VM/disk loss. Export snapshots off the Docker VM securely:

```sh
mkdir -p "$HOME/PrimeRadiant-backups"; chmod 700 "$HOME/PrimeRadiant-backups"
(umask 077; docker compose exec -T radiant tar -C /backups -cf - . \
  > "$HOME/PrimeRadiant-backups/snapshots-$(date -u +%Y%m%dT%H%M%SZ).tar")
```

This copies **completed backup artifacts only**, not the active DB/ledger. The archive
contains credentials; protect/encrypt it and never commit it. Credential archives are
not a replacement for reconciling the spending ledger after a disaster.

Restore a selected memory snapshot into the same deployment (existing credentials
and budget remain unchanged):

```sh
docker compose stop radiant
docker compose run --rm --no-deps radiant restore /backups/TIMESTAMP.db
docker compose run --rm --no-deps radiant check
docker compose up -d --wait
```

Restore checks integrity and preserves the replaced DB and sidecars under
`/data/radiant.db.replaced.*`; it does not delete user history as setup. To import an
exported backup artifact first, stream it into `/backups` with a one-off container
and umask 077. The imported snapshot must be on a writable volume because the restore
command also takes an exclusive `.lock` beside it; do not restore directly from a
read-only bind mount. For total credential-volume loss, restore a trusted credentials archive
into an **empty** new credentials volume before init/start; never regenerate tokens
and expect old clients to authenticate. Example with service stopped:

```sh
docker compose run --rm -T --no-deps --entrypoint sh radiant -ec \
  'umask 077; test ! -e /backups/import.db; cat > /backups/import.db' < /secure/path/snapshot.db
# Only for an EMPTY replacement credentials volume; not for routine memory restore:
docker compose run --rm -T --no-deps --entrypoint sh radiant -ec \
  'test ! -e /credentials/registry.json; umask 077; tar --no-same-owner -xf - -C /credentials' < /secure/path/TIMESTAMP.credentials.tar
```

## Stop / restart / upgrades

```sh
docker compose down                  # preserves all named volumes
docker compose up -d --build --wait   # same project, same credentials and records
```

**Never use `docker compose down -v` in normal operations.** Back up before upgrading.
Pinned base-image digests, a fixed Debian package snapshot and `Cargo.lock` make the
build inputs reproducible; deliberately update pins for security releases. No launchd,
login startup, UI, remote HTTP listener, or paid embedding fallback is installed.

## Repeat verification (disposable projects only)

See [VERIFICATION.md](VERIFICATION.md) for actual results and commands. The `verify`
image stage includes tests, saved fixtures and probe executables; these are **not**
present in the runtime image. `compose_fixture` deliberately writes test Sources and
explicitly approves fake reviewed proposals; never mount user data into it.
