# Foundation
Simple vector database API for personal information processing systems.

## Repository structure
- `docs`: API docs
- `main`: Swift server + DB bootstrap SQL + Docker build context
- `legacy`: old Python implementation
- `client`: Foundation API client code (`FoundationAPIClient.swift`)

## Runtime (Swift)
```zsh
cd ~/Foundation
docker compose -f main/docker-compose.yml up -d --build
```

The API is exposed on `http://localhost:8000`.
Settings UI is available at `http://localhost:8000/settings`.

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
- Configure provider/model/API key at `/settings` or via `.env`.

## Source indexing (provenance graph)
- Use `sources` to represent original materials (notes, URLs, media references) with unique `source_uid`.
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
- Storage is separated into dedicated tables: `vaults`, `vault_files`, `vault_changes`.
- Original file bytes are mirrored in workdir: `./vault_storage/<vault_uid>/...`.
- `POST /vaults/sync/push`: upload changed files + changelog entries.
- `POST /vaults/sync/pull`: fetch full snapshot (no timestamp) or delta (`since_unix_ms`).
- `POST /vaults/sync/status`: inspect latest timestamp + per-file timestamps + changelog.
- `POST /vaults/sync/full-push`: upload whole vault snapshot in one request.
- `POST /vaults/sync/full-pull`: download whole vault snapshot in one request.
- Integrity verification is not enforced (`content_sha256` is optional metadata).
- If you see `payload too large` (HTTP 413), increase `.env` `MAX_REQUEST_BODY_MB` (default: 128) and rebuild.
- `client/vault_sync.swift` also supports request batching via `--max-upload-bytes`.

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
```

## Legacy Python code
The old Python implementation was preserved in:
- `legacy/python/main`
- `legacy/python/database`
- `legacy/python/docker-compose.python.yml`
