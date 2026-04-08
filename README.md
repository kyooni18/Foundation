# Foundation
Simple vector database API for personal information processing systems.

## Repository structure
- `docs`: API docs
- `main`: Swift server + DB bootstrap SQL + Docker build context
- `web`: React web app + Nginx proxy container
- `legacy`: old Python implementation
- `client`: Foundation API client code (`FoundationAPIClient.swift`)

## Runtime (Swift)
```zsh
cd ~/Foundation
docker compose -f main/docker-compose.yml up -d --build
```

This repo is configured to store PostgreSQL data in a Docker named volume (`db_data`),
so a fresh clone starts cleanly without shipping prebuilt DB files.

If your local environment was previously using bind-mounted `main/database/dbdata`, reset once:

```zsh
cd ~/Foundation
docker compose -f main/docker-compose.yml down -v
rm -rf main/database/dbdata
docker compose -f main/docker-compose.yml up -d --build
```

The API is exposed on `http://localhost:8000`.
The React web app is exposed on `http://localhost:3000`.
Settings UI is available at `http://localhost:8000/settings`.
In a browser, unauthenticated access redirects to `http://localhost:8000/settings/login`, where you can sign in with a Foundation API key.
The web app proxies `http://localhost:3000/api/*` to the `main` container, so the browser UI can manage keys, atoms, sources, and vault operations without extra CORS setup.

## Web app
The new React app is a browser control room for Foundation:
- health checks and response inspection
- API key bootstrap / verification / deletion
- atom add / find / delete flows
- source creation, listing, reindex, and similarity operations
- vault status, semantic search, full-pull inspection, and browser-based full-push uploads

Local frontend-only development:

```zsh
cd ~/Foundation/web
pnpm install
pnpm dev
```

By default the web app expects `/api` as its API base. In Docker Compose, Nginx handles that proxy automatically.

## API documentation
- English: [API_EN.md](./docs/API_EN.md)
- Korean: [API_KO.md](./docs/API_KO.md)

## Local / Xcode run
If you run the Swift app directly from Xcode (outside Docker), start only PostgreSQL first:

```zsh
cd ~/Foundation
docker compose -f main/docker-compose.yml up -d db
```

Then open `main/Package.swift` in Xcode and run the app target.
The server loads `.env` from the repository root automatically.

## API Key bootstrap
- Initial master key comes from `.env` as `INIT_MASTER_KEY` (default: `host`).
- Use `/keys/create` with the master key to mint operational keys, then rotate the master key.

## Embedding providers
- `qwen3`: local deterministic embedding backend (Qwen3-compatible mode).
- `openai`: OpenAI Embeddings API (`text-embedding-3-small`, `text-embedding-3-large`, `text-embedding-ada-002`).
- Configure provider/model/API key at `/settings` through the browser login UI, with `Authorization: Bearer <api_key>`, or via `.env`.

## Source indexing (provenance graph)
- Use `sources` to represent original materials (notes, URLs, media references) with unique `source_uid`.
- Atoms are stored in `atoms_db(content, vector, type)` where `type` is `usercreated`, `aicreated`, or `imported`.
- Link uploaded atoms/keypoints to a source via `/sources/link-atom`.
- Build source-level embeddings (centroid of linked atoms) via `/sources/reindex`.
- Measure source-to-source distance via `/sources/find-similar`.
- Persist nearest source links via `/sources/link-similar`, and inspect them via `/sources/links/:source_uid`.

### Quick API flow
```zsh
API_KEY=host

curl -X POST http://localhost:8000/sources/create \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"source_uid":"note-001","source_type":"note","label":"My Note","locator":"obsidian://note-001"}'

curl -X POST http://localhost:8000/sources/link-atom \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"source_uid":"note-001","atom_text":"first keypoint"}'

curl -X POST http://localhost:8000/sources/reindex \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"source_uid":"note-001"}'

curl -X POST http://localhost:8000/sources/find-similar \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"source_uid":"note-001","limit":5}'
```

## Vault sync (Obsidian vault)
- Auth required: all `/vaults/*` endpoints require `Authorization: Bearer <api_key>`.
- Storage is separated into dedicated tables: `vaults`, `vault_files`, `vault_changes`, `file_atoms`, `file_links`, `file_processing_jobs`.
- `vault_files` stores the file-level object (`name`, `base64`, optional text `content`, async `subject` / `interpreted`, and vector fields).
- Markdown notes are automatically processed into atomic keypoints, embedded, and persisted in `atoms_db` with `file_atoms` links.
- Similar notes are auto-linked in `file_links`, and each note gets a managed `## Related Notes` block with Obsidian links (`[[relative/path]]`).
- Obsidian link targets are normalized from NFD to NFC to avoid Korean filename composition issues.
- Vault keypoint/note/query embeddings use the active embedding provider from `/settings` (set provider to `openai` to use the provided OpenAI API key).
- Uploads clear stale derived fields and enqueue processing jobs: `file_enrichment` and `atomize`.
- Original file bytes are mirrored in workdir: `./vault_storage/<vault_uid>/...`.
- `POST /vaults/sync/push`: upload changed files + changelog entries.
- `POST /vaults/sync/pull`: fetch full snapshot (no timestamp) or delta (`since_unix_ms`).
- `POST /vaults/sync/status`: inspect latest timestamp + per-file timestamps + changelog.
- `POST /vaults/sync/full-push`: upload whole vault snapshot in one request.
- `POST /vaults/sync/full-pull`: download whole vault snapshot in one request.
- `POST /vaults/search`: semantic search over processed keypoints (query embedding + nearest note keypoints).
- Integrity verification is not enforced (`content_sha256` is optional metadata).
- If you see `payload too large` (HTTP 413), increase `.env` `MAX_REQUEST_BODY_MB` (default: 128) and rebuild.
- `client/vault_sync.swift` also supports request batching via `--max-upload-bytes`.
- `client/vaultctl.swift` provides a higher-level CLI with saved profiles for endpoint, vault path, API key, and sync defaults.

### Quick vault sync flow
```zsh
API_KEY=host

curl -X POST http://localhost:8000/vaults/sync/push \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{
    "vault_uid":"my-vault",
    "device_id":"macbook",
    "changes":[
      {"file_path":"Daily/2026-03-04.md","action":"modified","changed_at_unix_ms":1772595300123,"content_base64":"IyBoZWxsbw=="}
    ]
  }'

curl -X POST http://localhost:8000/vaults/sync/pull \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"vault_uid":"my-vault","since_unix_ms":1772595200000}'

curl -X POST http://localhost:8000/vaults/sync/status \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"vault_uid":"my-vault","since_unix_ms":1772595200000,"limit":100}'

curl -X POST http://localhost:8000/vaults/sync/full-push \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{
    "vault_uid":"my-vault",
    "device_id":"macbook",
    "files":[
      {"file_path":"Daily/2026-03-04.md","content_base64":"IyBoZWxsbw=="},
      {"file_path":"Projects/plan.md","content_base64":"IyBwbGFu"}
    ]
  }'

curl -X POST http://localhost:8000/vaults/sync/full-pull \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"vault_uid":"my-vault"}'

curl -X POST http://localhost:8000/vaults/search \
  -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"vault_uid":"my-vault","query":"project timeline risks","limit":5}'
```

### Vault CLI
```zsh
./client/vaultctl.swift profile save archive \
  --base-url https://foundation.kyooni.kr \
  --api-key host \
  --vault-uid archive \
  --local-path ~/Documents/archive \
  --set-default

./client/vaultctl.swift profile list
./client/vaultctl.swift profile show archive
./client/vaultctl.swift full-push
./client/vaultctl.swift delta-pull

./client/vaultctl.swift
vaultctl> profile list
vaultctl> delta-push
vaultctl> quit
```

Saved profiles are stored at `~/.foundation/vaultctl/config.json`.
You can override that location with `FOUNDATION_VAULTCTL_CONFIG=/path/to/config.json`.
Running `./client/vaultctl.swift` with no arguments starts an interactive shell so you can issue multiple sync commands without restarting the program.

### Vault CLI (Python)
```zsh
./client/vaultctl.py profile save archive \
  --base-url https://foundation.kyooni.kr \
  --api-key host \
  --vault-uid archive \
  --local-path ~/Documents/archive \
  --set-default

./client/vaultctl.py profile list
./client/vaultctl.py full-push
./client/vaultctl.py delta-pull
./client/vaultctl.py status

./client/vaultctl.py
vaultctl> profile list
vaultctl> delta-push
vaultctl> quit
```

`./client/vaultctl.py` stores profiles at the same config path (`~/.foundation/vaultctl/config.json`) and also starts an interactive shell when run without arguments.

## Legacy Python code
The old Python implementation was preserved in:
- `legacy/python/main`
- `legacy/python/database`
- `legacy/python/docker-compose.python.yml`
