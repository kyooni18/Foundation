# Foundation
Simple vector database API for personal information processing systems.

## Runtime (Swift)
```zsh
cd ~/Foundation
docker compose up -d --build
```

The API is exposed on `http://localhost:8000`.
Settings UI is available at `http://localhost:8000/settings`.

## Local / Xcode run
If you run the Swift app directly from Xcode (outside Docker), start only PostgreSQL first:

```zsh
cd ~/Foundation
docker compose up -d db
```

Then run the app target in Xcode. It will read `.env` from the repository root automatically.

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

## Legacy Python code
The old Python implementation was preserved in:
- `legacy/python/main`
- `legacy/python/database`
- `legacy/python/docker-compose.python.yml`
