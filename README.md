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

Derived Atom metadata can contain:

- `validFrom`
- `expires`
- `subject`
- `confidence`
- `memoryType`: Preference, Fact, Project, Decision, Context, or Instruction
- `scope`: Global, Project, or Conversation
- `active`

Inactive Atoms are preserved but excluded from Atom semantic retrieval. This is also the soft-delete mechanism used by Notion synchronization.

## Retrieval

Foundation combines:

- exact match


- PostgreSQL full-text search
- `pg_trgm` similarity
- tag overlap
- pgvector cosine similarity
- Atom-to-File evidence

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

Current schema version: **5**.

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

`recall` retrieves durable memory with hybrid exact, lexical, tag, and semantic matching. `remember` is the preferred durable write path: it decomposes compound input and conservatively reconciles duplicates, compatible updates, superseded facts, and conflicts. `forget` accepts an exact memory ID returned by `recall` and removes that memory from active recall without exposing low-level storage objects.

Model-visible expansion is explicit server policy. `FOUNDATION_MCP_FILE_ACCESS=read|write` exposes a single `file` facade; read mode provides `get`/`list`, while write mode additionally provides `ingest`, `create`, `update`, `interpret`, and `delete`. `FOUNDATION_MCP_ADMIN=true` exposes `admin` (`stats`, `reindex`), and `FOUNDATION_MCP_NOTION=true` exposes `notion` (`status`, `sync`) when Notion is configured.

Low-level Atom CRUD, docking, FileLink plumbing, Vault CRUD, hard Atom deletion, embedding internals, and database maintenance primitives are intentionally not model-visible. They remain available through the REST/CLI/service layer. MCP results also omit binary payloads and embedding implementation details; File listings omit document bodies.


### Durable remember semantics

`remember` is the preferred write path for ChatGPT-style memory. It is not append-only.

For each atomic fact Foundation:

1. checks exact active content first;
2. gathers related Atoms using lexical similarity, tags, and provenance-compatible vector search;
3. asks the selected LLM provider (OpenAI or Apple Foundation Models) to classify the relationship;
4. applies conservative confidence thresholds before any automatic replacement;
5. records the decision in `memory_events`.

Actions:

- `duplicate`: keep the existing Atom and refresh tags/metadata.
- `update`: update the same Atom when the change is compatible and history is not meaningfully lost.
- `supersede`: create a new current Atom, deactivate the old Atom, and record a `supersedes` relation.
- `conflict`: preserve both active Atoms and record a `conflicts_with` relation.
- `new`: store independent knowledge.

A caller may provide `requestId`. Concurrent calls with the same ID share one in-flight operation, and completed results are persisted in `memory_requests` so retries after completion or process restart do not create another memory event.

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
npm run cli -- search "파스타" --limit=10
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
