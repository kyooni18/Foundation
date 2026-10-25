# Docker Compose MVP verification

Verified 2026-10-25 UTC (host local date 2026-10-26), Apple Silicon, Linux ARM64
Docker Engine **29.7.2**, Compose **5.6.1**, Docker VM **5 CPUs / 7.893 GiB RAM**.
Deployment source and commands are in [README.md](README.md). Historical reports,
prompts, databases, model files and retrieval-v1 evidence were not removed or migrated.

## Results

| Check | Actual result |
|---|---|
| Image and Compose | ARM64 release image built successfully; `docker compose config -q` passed. Base digests, Debian snapshot and Cargo.lock pinned. Runtime image approximately 51.5 MB (Docker-reported size). |
| Startup and readiness | Authenticated queue health passed; `up -d --build --wait` and Docker health status healthy. No published ports; loopback-only internal listener retained. |
| Runtime restrictions | UID/GID 10001; directories 0700, DB and token/registry files 0600; read-only root, no capabilities, restart policy `no`, final default memory cap 1 GiB. |
| Model reuse | Existing macOS path mounted read-only at `/model`; runtime could read weights and could not write them. No model download or disk duplication. |
| MCP | Initialize 2025-06-18, initialized notification, tools/list and tools/call passed through `compose exec -T`; stdout parsed strictly as JSON-RPC. Tokens only read from persistent files. |
| Recording and queues | Source recording and idempotent duplicate recording passed; owner queue_status and source_processing report stored-not-extracted pending work while provider is disabled. No automatic publication. |
| Extraction and approval | Offline fake provider failed once, durable 10-second backoff was respected (11-second wait), then independent fake reviews quarantined a valid source-anchored proposal. No Claims before approval; agent approval rejected; owner exact-hash approval and repeated approval returned the same single Claim ID. |
| Asynchronous indexing | Started the real Compose app after offline approval: background lexical job completed (one indexed Claim); no manual SQL/index insertion. Embeddings intentionally off. |
| Retrieval | Korean `서버` and English `SQLite` returned the same authorized Claim with its originating Source ID; 100% lexical term coverage. Tiny fixture query times **1.677 ms / 2.540 ms** respectively, not a corpus-scale benchmark. |
| Security | HTTP without credentials returned 401; user read of agent Source denied; agent queue inspection and user-source lifecycle administration denied. Forgotten Claim inaccessible to user and agent and absent from search; owner restore succeeded. |
| Exclusive ownership | A second DB-owning CLI check while serving refused with the exclusive-ownership error. |
| Persistence | Smoke test repeated after stop/start, `compose restart`, `compose down`, image rebuild and new container creation. Same recorded Source ID, approved Claim, queues and credentials; credential file hashes compared equal without publishing them. |
| Backup | `scripts/compose-backup` stopped gracefully, made a standalone SQLite Backup API snapshot, archived stopped credentials with umask 077, passed integrity checks and restarted healthy. |
| Same-volume restore | Forgotten the fixture Claim offline, observed no visible Claims, restored snapshot and passed all integrity checks; grounded MCP retrieval returned again. Replaced DB retained under `/data/radiant.db.replaced.*`. |
| Disaster restore | Restored credential archive and standalone snapshot into separate **new** Compose volumes (`pr-mvp-verify-restore`), without credential issuance. Integrity, authentication and full MCP smoke passed; credential hashes identical to original. |
| Rust tests | `cargo test --locked --release --offline`: **83 passed, 0 failed, 6 ignored**. All unit/integration/doc test groups completed. Ignored entries are existing benchmark or subprocess helper tests, not newly skipped failures. `cargo fmt --check` passed. |
| Paid calls | **None.** No OpenRouter key, budget or paid provider was enabled during testing. |

## Measured resource usage and CPU limitation

For the healthy **off / lexical** service after the MCP workload:

- App RSS / peak RSS: **12,188 KiB**, 8 threads.
- Cgroup sampled current usage: **19,152,896 bytes**; peak including health/exec
  probes: **23,494,656 bytes**. Configured limit: 1,073,741,824 bytes.
- Docker stats (separate sample after exec exited): **9.973 MiB**, **0.21% CPU**.
  Docker subtracts some page cache, so it is not identical to cgroup current usage.

**CPU inference is not usable on the currently allocated Docker VM.** Two actual
ARM64 CPU probes used the existing pinned Nemotron weights, without network access.
The first, concurrent with a build, was OOM-killed after approximately **30.83 s**.
A second probe without a simultaneous build, capped at 6500 MiB/no swap, was again
**OOM-killed (exit 137)** during initialization after **14.673 s**. A cgroup sample
recorded a **6,475,059,200-byte peak (~6.03 GiB)** before exit. No initialization
completion log was emitted. Korean and English representative queries were scheduled
but **neither completed**, so no successful initialization time, inference latency or
semantic retrieval result is claimed. The existing CPU implementation expands weights
to float32; full model initialization has substantial transient memory overhead.

The probes produced Docker VM memory pressure; several other containers were observed
with recent uptimes afterwards (all healthy when inspected). No unrelated containers
were deliberately stopped or reconfigured. No further CPU probes were attempted.
The final service is explicitly `EMBEDDING_MODE=off` and capped at **1 GiB** to protect
the VM. Do not simply enable `local` on this allocation. Larger-memory CPU operation
has **not** been verified; Metal remains host-only. No cloud embedding substitution
was introduced. Off mode does not provide semantic paraphrase or cross-language recall.

## Failures encountered and resolved / safe refusals

- First image attempt omitted compiled-in prompts; fixed the build context/COPY.
- Verification initially omitted the saved evaluation text and one historical review
  evidence fixture; included only those required tracked files. An initial extraction
  test failed because its fixture was absent; final full offline run has no failures.
- Initial MCP harness treated a tool's serialized Reply as the unwrapped result;
  corrected the harness to validate Reply.ok and extract Reply.result. No API/security
  behavior was weakened.
- A disaster-restore attempt directly from a read-only backup mount correctly refused:
  restore takes a write-capable ownership lock beside its input. Imported the already
  completed standalone snapshot to writable `/backups`, then restored successfully.
  This requirement is documented; no live SQLite file was copied.
- CPU OOM is **not fixed or hidden**; the explicitly supported default is the verified
  lexical mode described above. Real paid extraction remains untested by design.

## Repeat safely

Choose a **new, never-used** disposable project name beginning `pr-mvp-verify` and use
that same project in every command. Never run fixture generation on user volumes:

```sh
P=pr-mvp-verify-new
# Build tests and isolated fixture/probe executables; fake providers only.
docker build --target verify -t primeradiant-verify .
docker compose -p "$P" build
docker compose -p "$P" run --rm radiant init
# Service has not started; fixture owns the DB exclusively.
docker run --rm --network none --user 10001:10001 \
  -v "${P}_data:/data" primeradiant-verify /compose_fixture /data/radiant.db
docker compose -p "$P" up -d --build --wait
python3 scripts/verify-compose.py "$P" --forget
COMPOSE_PROJECT_NAME="$P" ./scripts/compose-backup
docker compose -p "$P" restart radiant
docker compose -p "$P" up -d --wait
python3 scripts/verify-compose.py "$P"
docker compose -p "$P" down
docker compose -p "$P" up -d --build --wait
python3 scripts/verify-compose.py "$P"
docker compose -p "$P" down  # no -v; retain verification records/backups
```

Do **not** repeat the CPU probe on this VM merely to rerun verification. Its source is
`examples/container_embed_probe.rs`; it is available only in the verify image. Use it
only with explicitly provisioned adequate memory and a bounded container on a machine
where memory pressure will not disrupt other services.

Evidence: [`reports/evidence/docker-compose-mvp/`](../reports/evidence/docker-compose-mvp/)
contains final offline Rust output, MCP workload/results, resource samples, CPU exit
state, backup and successful disaster-restore checks. No credentials or live DB copies
are committed. Disposable projects have been stopped with `down` (volumes retained).

## Handoff state

The normal **`primeradiant`** project was separately built, initialized and left
**running healthy**, with the exact documented agent MCP configuration verified
(29 tools). Its database is clean (zero Claims/open jobs); no fixture or development
data was imported. Its one-time credentials persist in `primeradiant_credentials`.
A verified initial backup exists at `/backups/20261025T161301Z.db`, with the protected
credential archive `/backups/20261025T161301Z.credentials.tar`, both inside
`primeradiant_backups`. The normal backup script stopped and restarted this project
successfully too. Use the operational README to record real data or stop the service.
