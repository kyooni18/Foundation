# Foundation API Documentation (English)

Last updated: 2026-03-03

## 1. Base information

- Base URL: `http://localhost:8000`
- API format: JSON (except `GET /settings` and `POST /settings`, which are HTML/form endpoints)
- Current server implementation: Swift (Vapor)

## 2. Authentication

### 2.1 Bearer token format

Use header:

```http
Authorization: Bearer <api_key>
```

### 2.2 Public endpoints (no Bearer required)

- `GET /health`
- `GET /health/db`
- `GET /health/embed`
- `GET /keys/list`
- `POST /keys/create`
- `POST /keys/delete`
- `POST /keys/verify`
- `GET /settings`
- `POST /settings`

### 2.3 Auth-protected endpoints

- `POST /embed/text`
- `POST /add`
- `POST /delete`
- `POST /find`
- All `/sources/*` endpoints

### 2.4 Master key bootstrap

- If `auth_keys` is empty, the server auto-creates one `master_key` from env `INIT_MASTER_KEY` (default `host`).
- Call `POST /keys/create` with the master key to generate operation keys.

---

## 3. Endpoint summary

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/health` | No | Service health |
| GET | `/health/db` | No | Database health |
| GET | `/health/embed` | No | Embedding backend health |
| GET | `/keys/list` | No | List masked API keys |
| POST | `/keys/create` | No | Create a new API key (requires master key in body) |
| POST | `/keys/delete` | No | Delete an API key by plaintext key |
| POST | `/keys/verify` | No | Verify an API key |
| GET | `/settings` | No | Settings HTML page |
| POST | `/settings` | No | Save settings (form post, redirect) |
| POST | `/embed/text` | Yes | Embed input text |
| POST | `/add` | Yes | Insert atom/keypoint |
| POST | `/delete` | Yes | Delete atom by text |
| POST | `/find` | Yes | Find nearest atoms |
| POST | `/sources/create` | Yes | Create or upsert source |
| GET | `/sources/list` | Yes | List sources and index counts |
| POST | `/sources/link-atom` | Yes | Link source to atom |
| POST | `/sources/unlink-atom` | Yes | Remove source-atom link |
| POST | `/sources/reindex` | Yes | Recompute source centroid embedding |
| POST | `/sources/find-similar` | Yes | Compute nearest sources by centroid |
| POST | `/sources/link-similar` | Yes | Persist nearest source links |
| GET | `/sources/links/:source_uid` | Yes | Read persisted outgoing links |

---

## 4. Common payloads and responses

### 4.1 Common request payloads

```json
{ "text": "..." }
```

```json
{ "api_key": "..." }
```

### 4.2 Common response shapes

Success patterns:

```json
{ "ok": true, "result": "..." }
```

```json
{ "ok": true, "results": [ ... ] }
```

Failure patterns used by handlers:

```json
{ "ok": false, "error": "..." }
```

Abort failures (Vapor default):

```json
{ "error": true, "reason": "..." }
```

---

## 5. Health endpoints

## 5.1 `GET /health`

- Auth: No
- Response:

```json
{ "ok": true }
```

## 5.2 `GET /health/db`

- Auth: No
- Response:

```json
{ "ok": true, "db": "app_settings" }
```

`db` is the first public table name found by the health query.

## 5.3 `GET /health/embed`

- Auth: No
- Behavior: Runs an embedding test (`"test"`) with current embedding settings.
- Response:

```json
{ "ok": true, "embed_dim": 1024 }
```

---

## 6. API key endpoints

## 6.1 `GET /keys/list`

- Auth: No
- Response:

```json
{
  "ok": true,
  "result": "mask: master_key, created_at: 2026-03-03T05:20:00Z\n"
}
```

## 6.2 `POST /keys/create`

- Auth: No
- Request:

```json
{ "api_key": "<master_key>" }
```

- Success response:

```json
{
  "ok": true,
  "mask": "foundation_abcd************************************************************",
  "api_key": "foundation_...."
}
```

- Failure response:

```json
{ "ok": false, "error": "Invalid master key" }
```

## 6.3 `POST /keys/delete`

- Auth: No
- Request:

```json
{ "api_key": "<plain_api_key_to_delete>" }
```

- Response:

```json
{ "ok": true, "result": "Key deleted" }
```

or

```json
{ "ok": false, "error": "Key not found" }
```

## 6.4 `POST /keys/verify`

- Auth: No
- Request:

```json
{ "api_key": "<plain_api_key>" }
```

- Response:

```json
{ "ok": true, "valid": true }
```

---

## 7. Settings endpoints

## 7.1 `GET /settings`

- Auth: No
- Response content-type: `text/html`
- Purpose: UI for embedding provider/model/API key settings.

## 7.2 `POST /settings`

- Auth: No
- Request content-type: HTML form (`application/x-www-form-urlencoded` from the settings page)
- Fields:
  - `provider`: `qwen3` or `openai`
  - `qwen_model`: optional text
  - `openai_model`: optional text
  - `openai_api_key`: optional text (leave empty to keep existing key)
  - `clear_openai_key`: `"1"` to clear stored OpenAI key
- Response: redirect to `/settings?saved=1`

---

## 8. Atom endpoints

All endpoints in this section require `Authorization: Bearer <api_key>`.

## 8.1 `POST /embed/text`

- Request:

```json
{ "text": "hello world" }
```

- Response:

```json
{ "ok": true, "embedding": [0.001, -0.02, ...] }
```

## 8.2 `POST /add`

- Request:

```json
{ "text": "A keypoint sentence" }
```

- Response:

```json
{ "ok": true, "result": "text: A keypoint sentence, embed vectors: 1024" }
```

- Duplicate text:

```json
{ "ok": false, "error": "Text already exists" }
```

## 8.3 `POST /delete`

- Request:

```json
{ "text": "A keypoint sentence" }
```

- Response:

```json
{ "ok": true, "result": "Deleted text: A keypoint sentence" }
```

## 8.4 `POST /find`

- Request:

```json
{ "text": "query text" }
```

- Response:

```json
{
  "ok": true,
  "results": [
    { "id": 1, "text": "A keypoint sentence", "metadata": null, "distance": 0.1234 }
  ]
}
```

- Returns up to 5 nearest atoms by vector distance.

---

## 9. Source indexing and provenance endpoints

All endpoints in this section require `Authorization: Bearer <api_key>`.

## 9.1 Data model

- `sources`: one source object (`source_uid` unique), for notes, URLs, files, media, etc.
- `source_atoms`: many-to-many link between `sources` and atoms (`atoms_db.id`).
- `source_indexes`: one centroid embedding per source (average of linked atom embeddings).
- `source_links`: persisted nearest-neighbor edges between sources (`distance`, `method`).

## 9.2 `POST /sources/create`

- Request:

```json
{
  "source_uid": "note-001",
  "source_type": "note",
  "label": "Daily Notes",
  "locator": "obsidian://note-001",
  "metadata": "{\"origin\":\"obsidian\"}"
}
```

- Notes:
  - If `source_uid` is omitted, server auto-generates UUID.
  - Upsert behavior: same `source_uid` updates fields.
  - `metadata` accepts a string; valid JSON string is stored as JSONB object/array/value. Non-JSON text is stored as JSON string value.

- Response:

```json
{ "ok": true, "source_uid": "note-001", "source_id": 12 }
```

## 9.3 `GET /sources/list`

- Response:

```json
{
  "ok": true,
  "results": [
    {
      "source_uid": "note-001",
      "source_type": "note",
      "label": "Daily Notes",
      "locator": "obsidian://note-001",
      "metadata": "{\"origin\":\"obsidian\"}",
      "created_at": "2026-03-03 05:31:17.263138+00",
      "linked_atom_count": 8,
      "indexed_atom_count": 8
    }
  ]
}
```

## 9.4 `POST /sources/link-atom`

- Request (by id):

```json
{ "source_uid": "note-001", "atom_id": 10 }
```

- Request (by text):

```json
{ "source_uid": "note-001", "atom_text": "first keypoint" }
```

- Behavior:
  - Creates source-atom link.
  - Idempotent (`linked=false` if link already existed).
  - Automatically refreshes source centroid index.

- Response:

```json
{ "ok": true, "source_uid": "note-001", "atom_id": 10, "linked": true }
```

## 9.5 `POST /sources/unlink-atom`

- Request: same as `link-atom`
- Behavior:
  - Removes source-atom link.
  - Automatically refreshes source centroid index.
  - If source has no linked atoms after unlink, source index and related links are removed.

- Response:

```json
{ "ok": true, "source_uid": "note-001", "atom_id": 10, "linked": true }
```

`linked=false` means there was nothing to remove.

## 9.6 `POST /sources/reindex`

- Request:

```json
{ "source_uid": "note-001" }
```

- Response:

```json
{ "ok": true, "source_uid": "note-001", "atom_count": 8 }
```

## 9.7 `POST /sources/find-similar`

- Request:

```json
{ "source_uid": "note-001", "limit": 5 }
```

- Behavior:
  - Clamps `limit` to range `1..50` (default 5).
  - Refreshes requesting source index before search.
  - Compares centroid embedding to other sources.

- Response:

```json
{
  "ok": true,
  "source_uid": "note-001",
  "results": [
    { "source_uid": "note-002", "source_type": "url", "label": "Article A", "distance": 0.42 }
  ]
}
```

## 9.8 `POST /sources/link-similar`

- Request:

```json
{ "source_uid": "note-001", "limit": 5 }
```

- Behavior:
  - Computes nearest sources like `find-similar`.
  - Upserts rows in `source_links` for outgoing edges from `source_uid`.
  - `method` is stored as `"centroid"`.

- Response: same shape as `find-similar`.

## 9.9 `GET /sources/links/:source_uid`

- Example:
  - `GET /sources/links/note-001`

- Response:

```json
{
  "ok": true,
  "source_uid": "note-001",
  "results": [
    { "source_uid": "note-002", "source_type": "url", "label": "Article A", "distance": 0.42 }
  ]
}
```

Returns persisted outgoing links only.

---

## 10. Embedding behavior

- `qwen3` provider: deterministic local embedding in Swift (no external API call).
- `openai` provider: calls OpenAI Embeddings API (`/v1/embeddings`) with selected model.
- OpenAI vectors are resized to DB dimension (`EMBEDDING_DIM`, default `1024`) by truncate/pad, then L2-normalized.

---

## 11. Environment variables

| Variable | Default | Description |
|---|---|---|
| `POSTGRES_DB` | `foundation_db1` | PostgreSQL DB name |
| `POSTGRES_USER` | `foundation` | PostgreSQL user |
| `POSTGRES_PASSWORD` | `host` | PostgreSQL password |
| `POSTGRES_PORT` | `5432` | PostgreSQL port |
| `EMBEDDING_DIM` | `1024` | Vector dimension |
| `EMBEDDINGS_TABLE` | `atoms_db` | Atom table name |
| `INIT_MASTER_KEY` | `host` | Initial bootstrap master key |
| `EMBEDDING_PROVIDER` | `qwen3` | Default provider (`qwen3` or `openai`) |
| `QWEN_MODEL` | `Qwen/Qwen3-Embedding-0.6B` | Label shown in settings for Qwen mode |
| `OPENAI_EMBEDDING_MODEL` | `text-embedding-3-small` | Default OpenAI model |
| `OPENAI_API_KEY` | (empty) | OpenAI API key (can also be managed via `/settings`) |

