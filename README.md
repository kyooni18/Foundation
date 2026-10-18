# Foundation

Foundation is an Atom-first personal knowledge substrate. It keeps durable knowledge independent from source documents, stores source material without rewriting it, provides provenance-safe hybrid retrieval over PostgreSQL/pgvector, and exposes the same domain service through HTTP MCP, stdio MCP, REST, and a CLI.

This repository is a fresh implementation. The older `Foundation-MCP` project was used only as an architectural reference.

## Object model

`Atom` is the top-level knowledge object. An Atom can exist on its own or dock to zero or more Files.

`File` is a source/document object. Raw notes stay in `File.content`; interpreted binary content is stored separately in `File.interpreted`.

`Vault` is an optional workspace for File collections such as Obsidian vaults. A Vault does not own Atoms.

`AtomFileDock` is the many-to-many relationship between independent Atoms and source Files.

`FileLink` relates two Files. `Modification` records File/Vault lifecycle activity. `external_bindings` stores synchronization identity for external systems such as Notion.

A note such as:

```text
도쿄 파이어볼 파스타집 맛있는듯
```

is stored verbatim as File raw text. The LLM may derive several independent Atoms, for example that the person was in Tokyo, that a named restaurant exists there, that the person ate there, and that the person evaluated it positively. Those Atoms dock to the source File. Search can use Atom evidence while returning the original File text.

Direct memory such as “remember that I like spicy pasta” creates an Atom directly; it does not fabricate a File.

## Temporal knowledge

Note ingestion accepts an optional `sourceTime` and Foundation has a configured time zone. The extraction layer receives both, so relative expressions such as “today”, “yesterday”, “tonight”, and their Korean equivalents can be grounded to the time at which the source was actually written rather than the time of import.

Derived Atom metadata can contain `validFrom`, `expires`, `subject`, `confidence`, `memoryType` (Preference, Fact, Project, Decision, Context, Instruction), `scope` and `active`. Since schema v7 the fields that drive behaviour are real columns, kept in sync with metadata for compatibility:

| column | meaning |
| --- | --- |
| `status` | `active`, `superseded` (replaced, kept as history) or `forgotten` (removed from recall by `forget`/deactivate) |
| `valid_from`, `expires_at` | the interval in which the fact was/is true |
| `scope_kind`, `scope_key` | `global`, or `project`/`conversation` plus the normalized project name / conversation id |

Reads are **current** by default: `status='active'`, already valid, not expired. `asOf=<ISO-8601>` answers "what was true then" (superseded Atoms are visible inside the interval in which they were true) and `history=true` includes every non-forgotten Atom ranked below current ones, each labelled `state: superseded|expired|future`. Forgotten Atoms are never returned. Ranking freshness uses `valid_from`, never `updated_at`, so touching an old fact does not make it look current.

`supersede` sets the old Atom's `expires_at` to the new fact's validity start and keeps it as history. If the new fact is *older* than the current one (late arrival) it is stored as history and the current fact is untouched. If it is valid only in the future, the old fact stays current until then. Conflicting facts are never auto-resolved: both stay current, linked by `conflicts_with`, and recall reports `conflictsWith`.

## Scope isolation

Identity of a memory is **(scope, content)**, not content alone. The same sentence remembered in two projects is two Atoms; they never merge, are never offered to each other's reconciliation, and forgetting one leaves the other.

- `remember` accepts `project`, `conversationId` and `scope`. A project/conversation scope is only stored when its key is present; otherwise the Atom falls back to Global and is flagged `scopeUnresolved`.
- `recall`/`search` accept `project` and `conversationId` (or `project:<name>` inside the query). A scoped read sees that project **plus Global** memories (Global slightly discounted); other projects and other conversations are invisible. A read with neither argument is *unscoped* and sees everything (legacy behaviour).
- `FOUNDATION_PROJECT` sets a default project for callers that name none, which gives per-workspace isolation to a stdio MCP server with no client changes.
- Files are scoped by `metadata.project` / `metadata.conversationId`, or by the Atoms they are docked to: a File whose only Atoms belong to another project is hidden. Files stay searchable independently of their Atoms.

## Retrieval

Retrieval is a two-stage pipeline:

1. **Candidate generation** — one index-friendly query per signal, each with a LIMIT: exact hash, full-text (GIN), `pg_trgm` word similarity (GIN), identifier substring (trigram GIN), tag overlap (GIN), and pgvector cosine (HNSW with iterative scan so scope/status filters cannot starve it). Scope, lifecycle and metadata filters are added to the SQL only when they apply, so they do not block index plans.
2. **Rescoring and fusion** — the union is rescored by primary key with exact signals, then fused with weighted reciprocal-rank fusion.

Relevance is thresholded: a result is returned only if at least one strong signal clears its own bar (exact match, identifier match, idf-weighted term coverage, fuzzy match, semantic similarity within a band of the best vector hit, or tag coverage). A query can therefore return nothing. When one result is decisive (exact, or every term of a multi-term query matches) weaker partial matches are dropped; near-duplicate Atoms are suppressed and runs from one source File are softened. Without embeddings, retrieval keeps working lexically, by exact identifier and by tag.

**Identity.** `recall` results of `kind: "atom"` always carry the **Atom id** — the same id `forget` accepts. The Files an Atom is docked to are reported as `sources`/`fileIds` (provenance) and are folded into the Atom unless the File matched better on its own, in which case it is a separate `kind: "file"` result. Passing a File id to `forget` is rejected with an explanatory error, because a File can back several memories.

Diagnostics (not exposed as MCP tools): `foundation explain <query> [--plans]` shows each generator's rows/latency/plan, every candidate's signals and why it was cut; `foundation relations <atom-id>` shows relations, sources, history and reconciliation events; `foundation diagnose` prints scope/status/embedding health counts without memory text.

All embedding columns use `HALFVEC(3072)` with HNSW cosine indexes.

Each stored vector has provenance beside it:

```text
provider
model
dimensions
embedded_at
```

A query vector is compared only with stored vectors whose provider, model, and dimensions exactly match. Changing embedding providers therefore cannot silently mix incompatible vector spaces.

The current production database is using Gemini `gemini-embedding-2` at 3072 dimensions. OpenAI `text-embedding-3-large` remains supported and can be selected through configuration.

## Schema

Current schema version: **7**. See `docs/QUALITY.md` for the v6→v7 migration notes.

Major tables:

```text
atoms
vaults
files
atom_file_docks
modifications
file_links
file_link_atoms
atom_relations
memory_requests
memory_events
external_bindings
oauth_clients
oauth_authorization_codes
oauth_tokens
foundation_schema
```

Older vectors with unknown or incompatible provenance are invalidated without deleting Atom/File source data. They can then be rebuilt with `foundation reindex`.

## AI providers

LLM and embedding providers are independent.

LLM providers:

- `apple` — macOS Foundation Models through the native Swift bridge
- `openai`
- `none`

Embedding providers:

- `openai`
- `gemini`
- `none`

The Apple bridge is in:

```text
native/apple-foundation-model
```

Build it with:

```sh
npm run build:apple-model
```

The TypeScript host performs all database writes and side effects. The Swift bridge only performs native model inference.

## Binary File interpretation

A binary File can be converted into searchable `interpreted` text through the opt-in MCP `file` facade action `interpret`, REST, or the CLI.

Supported paths:

- text / JSON / XML: local UTF-8 decoding
- images: OpenAI Responses vision
- PDF and supported document formats: OpenAI Responses file input
- audio: OpenAI transcription
- video: local `ffmpeg` frame/audio extraction, transcription, then multimodal interpretation

Interpretation provenance is stored in File metadata. The resulting text is immediately embedded using the active embedding provider.

The current implementation limits one interpreted binary File to 50 MiB.

## MCP

HTTP MCP endpoint:

```text
POST /mcp
```

stdio MCP:

```sh
npm run mcp:stdio
```

or, after package bin linking:

```sh
foundation-mcp
```

MCP exposes a deliberately small model surface. By default the model sees exactly three high-level memory tools: `recall`, `remember`, and `forget`.

`recall` retrieves durable memory with hybrid exact, lexical, tag, and semantic matching. `remember` is the preferred durable write path: it decomposes compound input and conservatively reconciles duplicates, compatible updates, superseded facts, and conflicts. `forget` accepts the exact memory ID returned by `recall` (always an Atom id) and removes that memory from active recall without exposing low-level storage objects.

Model-visible expansion is explicit server policy. `FOUNDATION_MCP_FILE_ACCESS=read|write` exposes a single `file` facade; read mode provides `get`/`list`, while write mode additionally provides `ingest`, `create`, `update`, `interpret`, and `delete`. `FOUNDATION_MCP_ADMIN=true` exposes `admin` (`stats`, `reindex`), and `FOUNDATION_MCP_NOTION=true` exposes `notion` (`status`, `sync`) when Notion is configured.

Low-level Atom CRUD, docking, FileLink plumbing, Vault CRUD, hard Atom deletion, embedding internals, and database maintenance primitives are intentionally not model-visible. They remain available through the REST/CLI/service layer. MCP results also omit binary payloads and embedding implementation details; File listings omit document bodies.


### Durable remember semantics

`remember` is the preferred write path for ChatGPT-style memory. It is not append-only.

For each atomic fact Foundation:

1. **plans outside any transaction** — embeds, gathers same-scope candidates (lexical similarity, tags, provenance-compatible vectors) and asks the selected LLM provider to classify the relationship, so no database transaction is held open across model calls;
2. applies conservative confidence thresholds (duplicate ≥ 0.70, supersede ≥ 0.75, update ≥ 0.80; below that it keeps both Atoms as a `conflict`) and ignores resolver targets that are not same-scope candidates;
3. **applies everything in one transaction** — takes row locks on the target, re-validates that it is unchanged since planning (otherwise degrades to `conflict`, never overwrites), writes Atoms, relations, `memory_events` and the `memory_requests` record together, so a failure leaves nothing partial.

Actions:

- `duplicate`: keep the existing Atom and refresh tags and `lastRememberedAt`; its validity and provenance are not overwritten. Re-remembering something that was forgotten revives it.
- `update`: update the same Atom (same id) when the change is compatible; the previous wording is kept in `metadata.history`.
- `supersede`: create a new current Atom, mark the old one `superseded` with an expiry at the new fact's start, and record a `supersedes` relation.
- `conflict`: preserve both active Atoms and record a `conflicts_with` relation.
- `new`: store independent knowledge.

A caller may provide `requestId`. Idempotency is enforced in the database (a per-request advisory lock plus the `memory_requests` record written in the same transaction as the memories), so retries after failure, process restarts and concurrent instances never create a second memory event. Reusing a `requestId` with different content is an error.

Embedding failure does not discard memory. The Atom is stored with `embeddingDeferred=true`, and a later reindex fills the missing vector.

## REST

REST base path defaults to:

```text
/api/v1
```

It exposes stats, search, durable remember, note ingestion, reindexing, Atom/File/Vault lifecycle operations, File interpretation, and Notion sync. HTTP MCP and REST both use the same Foundation service layer.

## CLI

Build first:

```sh
npm run build
```

Examples:

```sh
npm run cli -- stats
npm run cli -- search "파스타" --limit=10 --project=Mun
npm run cli -- explain "deploy target" --project=Mun --plans
npm run cli -- relations <atom-id>
npm run cli -- diagnose
npm run cli -- ingest "오늘 도쿄에서 파스타 먹음" --source-time=2026-09-23T12:00:00+09:00
npm run cli -- reindex --kind=atoms --concurrency=4
npm run cli -- list atoms --limit=50
npm run cli -- similar <atom-id>
npm run cli -- merge <source-atom-id> <target-atom-id>
npm run cli -- deactivate <atom-id>
npm run cli -- interpret <file-id>
npm run cli -- notion-status
npm run cli -- notion-sync
```

## Reindexing and provider changes

The opt-in MCP `admin` facade action `reindex` or the CLI `foundation reindex` command rebuilds missing or stale embeddings when provider/model provenance changes.

Reindex supports bounded concurrency:

```sh
npm run cli -- reindex --kind=all --limit=1000 --concurrency=4
```

Embedding maintenance intentionally does **not** change Atom semantic `updated_at`. This keeps reindexing from being mistaken for a content edit by synchronization layers.

## Notion AI Memory sync

Foundation can synchronize top-level Atoms with the existing Notion **AI Memory** data source using the authenticated local `ntn` CLI. The CLI keeps its authentication in the local OS credential store, so Foundation does not require a second Notion token in `.env`.

Configure:

```env
NOTION_CLI=ntn
NOTION_DATA_SOURCE_ID=<data-source-id>
```

Mapped AI Memory fields include:

```text
Memory
Content
Active
Confidence
Keywords
Scope
Source
Subject
Type
Importance
Valid From
Expires
```

Use a read-only preview first:

```sh
npm run cli -- notion-sync --dry-run
```

Then synchronize:

```sh
npm run cli -- notion-sync
```

Bindings store the Notion page ID and both sides' synchronization baselines. If both sides changed, the newer actual edit wins and the conflict is reported. `Active=false` maps to an inactive Atom. During a full sync, a previously bound Notion page that is no longer returned (for example because it was archived) deactivates its Atom.

Hard-deleting a local Atom is not used as cross-system delete intent; if its Notion page still exists, a later sync can restore the Atom. Use `active=false` / `deactivate` for synchronized deletion semantics.

The current local database has been initialized from the existing AI Memory data source and converges cleanly on repeated sync.

## Authentication

Foundation supports both a static Bearer API key and local OAuth.

Static token:

```env
FOUNDATION_API_KEY=...
```

OAuth supports:

- RFC-style protected-resource discovery
- authorization-server discovery
- Dynamic Client Registration
- Authorization Code + PKCE S256
- opaque access/refresh tokens stored only as hashes
- `foundation:read`
- `foundation:write`
- `offline_access`
- refresh-token rotation
- refresh-family revocation on token reuse

Discovery endpoints:

```text
/.well-known/oauth-protected-resource
/.well-known/oauth-protected-resource/mcp
/.well-known/oauth-authorization-server
```

OAuth endpoints:

```text
POST /oauth/register
GET  /oauth/authorize
POST /oauth/authorize
POST /oauth/token
```

For local use, `OAUTH_APPROVAL_SECRET` can be set explicitly. If it is omitted, Foundation can reuse `FOUNDATION_API_KEY` as the authorization approval secret.

When binding to a non-loopback host, Foundation refuses to start unless an API key or enabled OAuth protection is available.

## Local macOS setup

Install dependencies and copy the environment template:

```sh
npm install
cp .env.example .env
```

Build:

```sh
npm run build
npm run build:apple-model
```

Start PostgreSQL/pgvector:

```sh
npm run db:up
```

The local database scripts bootstrap Apple Container when needed, start the configured Docker machine, run Compose against the remote Docker daemon over SSH, and wait for PostgreSQL to become healthy. Because Docker's automatic host loopback forwarding does not reliably expose PostgreSQL to the macOS-native Foundation process, the provided tunnel maps macOS `127.0.0.1:55432` to the Docker VM's `127.0.0.1:5432`:

```sh
npm run db:tunnel
```

Then start Foundation:

```sh
npm start
```

Or use:

```sh
npm run local:up
```

Stop the tunnel with:

```sh
npm run db:tunnel:stop
```

## Quick start (Docker Compose)

Requires Docker Compose. Copy the template, set a strong database password and provider API keys, then start Foundation with PostgreSQL/pgvector:

```sh
cp .env.example .env
# Edit .env: set POSTGRES_PASSWORD, OPENAI_API_KEY, and GEMINI_API_KEY
docker compose up --build
```

Open `http://localhost:8787`. Data persists in `./data/`. Stop with `Ctrl-C` or `docker compose down`. Apple Foundation Models are host-only; the container uses a cloud LLM provider.


## Environment

See `.env.example` for the complete configuration. Important groups are:

```text
DATABASE_URL
FOUNDATION_TIME_ZONE

HOST
PORT
MCP_PATH
REST_PATH
FOUNDATION_MCP_FILE_ACCESS=off|read|write
FOUNDATION_MCP_ADMIN=true|false
FOUNDATION_MCP_NOTION=true|false

FOUNDATION_API_KEY
FOUNDATION_PUBLIC_BASE_URL
OAUTH_*

LLM_PROVIDER
EMBEDDING_PROVIDER
EMBEDDING_DIMENSIONS

OPENAI_*
GEMINI_*

NOTION_CLI
NOTION_DATA_SOURCE_ID

APPLE_FOUNDATION_BRIDGE
AI_TIMEOUT_MS
```

## Verification

Fast checks:

```sh
npm run check
npm run build
npm run build:apple-model
```

The pgvector smoke test now creates and drops its **own isolated PostgreSQL database**. It never truncates the configured Foundation database:

```sh
npm run smoke:pgvector
```

The implementation has also been validated against real PostgreSQL/pgvector for schema migration, provenance isolation, lifecycle/merge/deactivation behavior, HTTP and stdio MCP, REST, OAuth PKCE/refresh rotation, Notion synchronization, and live image interpretation plus embedding.

## Current runtime

Default HTTP surfaces:

```text
http://127.0.0.1:8787/mcp
http://127.0.0.1:8787/api/v1
http://127.0.0.1:8787/health
```

The database schema is initialized transactionally under an advisory lock, and all transport layers call the same Foundation domain service.

### Live MCP configuration and OpenAI Flex

Set `FOUNDATION_MCP_ADMIN=true` to enable `admin` configuration actions on
**both stdio and HTTP MCP** (the default three-tool surface is unchanged).
Call `admin` with `{"action":"getConfig"}` to inspect safe settings, or:

```json
{"action":"setConfig","input":{"openAIFlexEnabled":false,"aiTimeoutMs":120000}}
```

Live settings are `openAIFlexEnabled`, `openAILLMModel`,
`openAIInterpretationModel`, `openAITranscriptionModel`, and `aiTimeoutMs`
(1,000–120,000 ms). Changes affect subsequent requests in the current server
process, across HTTP sessions; they do not rewrite `.env` or survive restart.
Credentials, authentication, provider selection and tool-exposure policy cannot
be read or modified through this facade. HTTP config writes require write scope.

`OPENAI_FLEX_ENABLED=true` is the default: OpenAI chat completion and Responses
requests send `service_tier: "flex"`. Set it to `false` for standard processing or
for compatible endpoints/models that do not support Flex. Embedding and audio
transcription requests never receive this parameter. Flex can have higher latency
or capacity errors; errors propagate normally without an automatic paid-tier retry.
