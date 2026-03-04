# Foundation API 문서 (한국어)

최종 업데이트: 2026-03-04

## 1. 기본 정보

- Base URL: `http://localhost:8000`
- 기본 API 포맷: JSON
- 예외: `GET /settings`, `POST /settings`는 HTML/Form 엔드포인트
- 현재 서버 구현: Swift (Vapor)

## 2. 인증

### 2.1 Bearer 토큰 형식

헤더:

```http
Authorization: Bearer <api_key>
```

### 2.2 인증 없이 호출 가능한 엔드포인트

- `GET /health`
- `GET /health/db`
- `GET /health/embed`
- `GET /keys/list`
- `POST /keys/create`
- `POST /keys/delete`
- `POST /keys/verify`
- `GET /settings`
- `POST /settings`

### 2.3 인증이 필요한 엔드포인트

- `POST /embed/text`
- `POST /add`
- `POST /delete`
- `POST /find`
- 모든 `/sources/*` 엔드포인트
- 모든 `/vaults/*` 엔드포인트

### 2.4 마스터 키 초기화

- `auth_keys` 테이블이 비어 있으면 서버가 `INIT_MASTER_KEY`(기본값 `host`)로 `master_key`를 자동 생성합니다.
- 운영용 API 키 발급은 `POST /keys/create`에 마스터 키를 넣어 호출합니다.

---

## 3. 엔드포인트 요약

| Method | Path | 인증 | 설명 |
|---|---|---|---|
| GET | `/health` | 없음 | 서비스 상태 |
| GET | `/health/db` | 없음 | DB 상태 |
| GET | `/health/embed` | 없음 | 임베딩 백엔드 상태 |
| GET | `/keys/list` | 없음 | 마스킹된 API 키 목록 |
| POST | `/keys/create` | 없음 | 새 API 키 발급(바디에 마스터 키 필요) |
| POST | `/keys/delete` | 없음 | 평문 API 키로 삭제 |
| POST | `/keys/verify` | 없음 | API 키 유효성 확인 |
| GET | `/settings` | 없음 | 설정 HTML 페이지 |
| POST | `/settings` | 없음 | 설정 저장(Form, redirect) |
| POST | `/embed/text` | 필요 | 텍스트 임베딩 반환 |
| POST | `/add` | 필요 | atom/keypoint 저장 |
| POST | `/delete` | 필요 | 텍스트 기준 atom 삭제 |
| POST | `/find` | 필요 | 최근접 atom 검색 |
| POST | `/sources/create` | 필요 | source 생성/업서트 |
| GET | `/sources/list` | 필요 | source 목록 및 인덱스 상태 |
| POST | `/sources/link-atom` | 필요 | source-atom 연결 |
| POST | `/sources/unlink-atom` | 필요 | source-atom 연결 해제 |
| POST | `/sources/reindex` | 필요 | source centroid 임베딩 재계산 |
| POST | `/sources/find-similar` | 필요 | source 간 거리 계산 |
| POST | `/sources/link-similar` | 필요 | source 유사도 링크 영속화 |
| GET | `/sources/links/:source_uid` | 필요 | 저장된 outgoing 링크 조회 |
| POST | `/vaults/sync/push` | 필요 | 변경 파일 + 변경 로그 업로드 |
| POST | `/vaults/sync/pull` | 필요 | 전체 스냅샷 또는 timestamp 기준 델타 조회 |
| POST | `/vaults/sync/status` | 필요 | 최신 timestamp, 파일별 timestamp, 변경 로그 조회 |
| POST | `/vaults/sync/full-push` | 필요 | 볼트 디렉토리 전체를 1회 업로드 |
| POST | `/vaults/sync/full-pull` | 필요 | 볼트 디렉토리 전체를 1회 다운로드 |

---

## 4. 공통 페이로드/응답

### 4.1 공통 요청 바디

```json
{ "text": "..." }
```

```json
{ "api_key": "..." }
```

### 4.2 공통 응답 형태

성공 형태:

```json
{ "ok": true, "result": "..." }
```

```json
{ "ok": true, "results": [ ... ] }
```

핸들러 내부 실패 형태:

```json
{ "ok": false, "error": "..." }
```

Vapor Abort 실패 형태:

```json
{ "error": true, "reason": "..." }
```

---

## 5. Health 엔드포인트

## 5.1 `GET /health`

- 인증: 없음
- 응답:

```json
{ "ok": true }
```

## 5.2 `GET /health/db`

- 인증: 없음
- 응답:

```json
{ "ok": true, "db": "app_settings" }
```

`db` 값은 health 쿼리에서 조회된 첫 번째 public 테이블명입니다.

## 5.3 `GET /health/embed`

- 인증: 없음
- 동작: 현재 임베딩 설정으로 `"test"` 임베딩을 실행
- 응답:

```json
{ "ok": true, "embed_dim": 1024 }
```

---

## 6. API 키 엔드포인트

## 6.1 `GET /keys/list`

- 인증: 없음
- 응답:

```json
{
  "ok": true,
  "result": "mask: master_key, created_at: 2026-03-03T05:20:00Z\n"
}
```

## 6.2 `POST /keys/create`

- 인증: 없음
- 요청:

```json
{ "api_key": "<master_key>" }
```

- 성공 응답:

```json
{
  "ok": true,
  "mask": "foundation_abcd************************************************************",
  "api_key": "foundation_...."
}
```

- 실패 응답:

```json
{ "ok": false, "error": "Invalid master key" }
```

## 6.3 `POST /keys/delete`

- 인증: 없음
- 요청:

```json
{ "api_key": "<삭제할 평문 API 키>" }
```

- 응답:

```json
{ "ok": true, "result": "Key deleted" }
```

또는

```json
{ "ok": false, "error": "Key not found" }
```

## 6.4 `POST /keys/verify`

- 인증: 없음
- 요청:

```json
{ "api_key": "<평문 API 키>" }
```

- 응답:

```json
{ "ok": true, "valid": true }
```

---

## 7. 설정 엔드포인트

## 7.1 `GET /settings`

- 인증: 없음
- 응답 content-type: `text/html`
- 목적: 임베딩 provider/model/API key를 UI에서 관리

## 7.2 `POST /settings`

- 인증: 없음
- 요청 content-type: HTML form (`application/x-www-form-urlencoded`)
- 필드:
  - `provider`: `qwen3` 또는 `openai`
  - `qwen_model`: 선택
  - `openai_model`: 선택
  - `openai_api_key`: 선택 (비워두면 기존 키 유지)
  - `clear_openai_key`: `"1"`이면 저장된 OpenAI 키 삭제
- 응답: `/settings?saved=1`로 redirect

---

## 8. Atom 엔드포인트

이 섹션의 모든 엔드포인트는 `Authorization: Bearer <api_key>`가 필요합니다.

## 8.1 `POST /embed/text`

- 요청:

```json
{ "text": "hello world" }
```

- 응답:

```json
{ "ok": true, "embedding": [0.001, -0.02, ...] }
```

## 8.2 `POST /add`

- 요청:

```json
{ "text": "A keypoint sentence" }
```

- 응답:

```json
{ "ok": true, "result": "text: A keypoint sentence, embed vectors: 1024" }
```

- 중복 텍스트:

```json
{ "ok": false, "error": "Text already exists" }
```

## 8.3 `POST /delete`

- 요청:

```json
{ "text": "A keypoint sentence" }
```

- 응답:

```json
{ "ok": true, "result": "Deleted text: A keypoint sentence" }
```

## 8.4 `POST /find`

- 요청:

```json
{ "text": "query text" }
```

- 응답:

```json
{
  "ok": true,
  "results": [
    { "id": 1, "text": "A keypoint sentence", "metadata": null, "distance": 0.1234 }
  ]
}
```

- 최대 5개 최근접 atom을 반환합니다.

---

## 9. Source 인덱싱/출처 그래프 엔드포인트

이 섹션의 모든 엔드포인트는 `Authorization: Bearer <api_key>`가 필요합니다.

## 9.1 데이터 모델

- `sources`: 원본 자료(노트/URL/파일/미디어 등) 엔티티, `source_uid` 유니크
- `source_atoms`: source와 atom(`atoms_db.id`) 사이 N:M 연결
- `source_indexes`: source별 centroid 임베딩(연결된 atom 임베딩 평균)
- `source_links`: source 간 거리 링크(`distance`, `method`) 영속화

## 9.2 `POST /sources/create`

- 요청:

```json
{
  "source_uid": "note-001",
  "source_type": "note",
  "label": "Daily Notes",
  "locator": "obsidian://note-001",
  "metadata": "{\"origin\":\"obsidian\"}"
}
```

- 동작:
  - `source_uid`를 생략하면 서버가 UUID 자동 생성
  - 같은 `source_uid`면 업서트(업데이트)
  - `metadata`는 문자열 입력이며:
    - JSON 문자열이면 JSONB로 파싱 저장
    - 일반 문자열이면 JSON 문자열 값으로 저장

- 응답:

```json
{ "ok": true, "source_uid": "note-001", "source_id": 12 }
```

## 9.3 `GET /sources/list`

- 응답:

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

- 요청 (ID 기준):

```json
{ "source_uid": "note-001", "atom_id": 10 }
```

- 요청 (텍스트 기준):

```json
{ "source_uid": "note-001", "atom_text": "first keypoint" }
```

- 동작:
  - source-atom 연결 생성
  - 중복 연결은 무시(`linked=false`)
  - 연결 후 source centroid 인덱스 자동 갱신

- 응답:

```json
{ "ok": true, "source_uid": "note-001", "atom_id": 10, "linked": true }
```

## 9.5 `POST /sources/unlink-atom`

- 요청: `link-atom`과 동일
- 동작:
  - source-atom 연결 제거
  - 제거 후 source centroid 인덱스 자동 갱신
  - 연결 atom이 0개가 되면 source index 및 관련 링크 삭제

- 응답:

```json
{ "ok": true, "source_uid": "note-001", "atom_id": 10, "linked": true }
```

`linked=false`는 제거할 연결이 없었다는 뜻입니다.

## 9.6 `POST /sources/reindex`

- 요청:

```json
{ "source_uid": "note-001" }
```

- 응답:

```json
{ "ok": true, "source_uid": "note-001", "atom_count": 8 }
```

## 9.7 `POST /sources/find-similar`

- 요청:

```json
{ "source_uid": "note-001", "limit": 5 }
```

- 동작:
  - `limit`는 `1..50` 범위로 제한 (기본 5)
  - 검색 전에 요청 source 인덱스를 재계산
  - centroid 임베딩 기준으로 다른 source와 거리 계산

- 응답:

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

- 요청:

```json
{ "source_uid": "note-001", "limit": 5 }
```

- 동작:
  - `find-similar`과 동일하게 최근접 source 계산
  - 결과를 `source_links`에 업서트 저장 (요청 source 기준 outgoing 링크)
  - `method`는 `"centroid"`로 저장

- 응답: `find-similar`과 동일 형태

## 9.9 `GET /sources/links/:source_uid`

- 예시:
  - `GET /sources/links/note-001`

- 응답:

```json
{
  "ok": true,
  "source_uid": "note-001",
  "results": [
    { "source_uid": "note-002", "source_type": "url", "label": "Article A", "distance": 0.42 }
  ]
}
```

저장된 outgoing 링크만 반환합니다.

---

## 10. 임베딩 동작

- `qwen3` provider: Swift 내부 deterministic 임베딩(외부 API 호출 없음)
- `openai` provider: OpenAI Embeddings API (`/v1/embeddings`) 호출
- OpenAI 벡터는 DB 차원(`EMBEDDING_DIM`, 기본 `1024`)에 맞게 truncate/pad 후 L2 normalize 적용

---

## 11. 환경 변수

| 변수명 | 기본값 | 설명 |
|---|---|---|
| `POSTGRES_DB` | `foundation_db1` | PostgreSQL DB 이름 |
| `POSTGRES_USER` | `foundation` | PostgreSQL 사용자 |
| `POSTGRES_PASSWORD` | `host` | PostgreSQL 비밀번호 |
| `POSTGRES_PORT` | `5432` | PostgreSQL 포트 |
| `EMBEDDING_DIM` | `1024` | 벡터 차원 |
| `EMBEDDINGS_TABLE` | `atoms_db` | atom 테이블 이름 |
| `INIT_MASTER_KEY` | `host` | 초기 부트스트랩 마스터 키 |
| `EMBEDDING_PROVIDER` | `qwen3` | 기본 provider (`qwen3` 또는 `openai`) |
| `QWEN_MODEL` | `Qwen/Qwen3-Embedding-0.6B` | Qwen 모드 UI 표시 모델명 |
| `OPENAI_EMBEDDING_MODEL` | `text-embedding-3-small` | 기본 OpenAI 모델 |
| `OPENAI_API_KEY` | (빈값) | OpenAI API 키 (`/settings`에서도 관리 가능) |
| `MAX_REQUEST_BODY_MB` | `128` | 전역 HTTP 요청 본문 최대 크기(MB). 큰 볼트 동기화의 413 대응에 사용 |

---

## 12. Vault 동기화 엔드포인트

이 섹션의 모든 엔드포인트는 `Authorization: Bearer <api_key>`가 필요합니다.

Vault 동기화는 기존 atom/source 도메인과 분리된 전용 테이블을 사용합니다.

- `vaults`: vault 식별자(`vault_uid`)
- `vault_files`: 서버가 보관하는 현재 전체 스냅샷
- `vault_changes`: 델타 동기화를 위한 append-only 변경 로그

`vault_files`, `vault_changes`는 `vault_id`(=`vaults.id` FK)로 연결됩니다.
추가로 push된 원본 파일 바이트는 서버 workdir의
`./vault_storage/<vault_uid>/...` 경로에도 미러링됩니다.

참고:
- `vault_uid`는 `vault_storage` 하위 실제 폴더명으로 사용됩니다 (`/`, `\` 포함 불가).
- 무결성 검증은 강제하지 않습니다. `content_sha256`는 선택 메타데이터입니다.

### 12.1 `POST /vaults/sync/push`

- 목적: 클라이언트/디바이스에서 변경 파일 + 변경 로그를 업로드
- 요청:

```json
{
  "vault_uid": "my-obsidian-vault",
  "device_id": "iphone-15",
  "changes": [
    {
      "file_path": "Daily/2026-03-04.md",
      "action": "modified",
      "changed_at_unix_ms": 1772595300123,
      "content_base64": "IyBEYWlseSBub3RlLi4u"
    },
    {
      "file_path": "Drafts/old.md",
      "action": "deleted",
      "changed_at_unix_ms": 1772595315000
    }
  ]
}
```

- 동작:
  - `action`: `added`, `modified`, `deleted` (별칭 `changed`, `updated`도 허용)
  - `added`/`modified`는 `content_base64` 필수
  - `file_path`는 상대 경로만 허용 (`..`, 절대경로 금지)

- 응답:

```json
{
  "ok": true,
  "vault_uid": "my-obsidian-vault",
  "applied_changes": 2,
  "latest_change_id": 189,
  "latest_change_unix_ms": 1772595315000
}
```

### 12.2 `POST /vaults/sync/pull`

- 목적:
  - `since_unix_ms` 미포함: 전체 스냅샷
  - `since_unix_ms` 포함: 델타 변경 + 변경 로그
- 요청 (전체):

```json
{ "vault_uid": "my-obsidian-vault", "limit": 5000 }
```

- 요청 (델타):

```json
{
  "vault_uid": "my-obsidian-vault",
  "since_unix_ms": 1772595000000,
  "limit": 5000
}
```

- 응답 (full 모드):

```json
{
  "ok": true,
  "vault_uid": "my-obsidian-vault",
  "mode": "full",
  "latest_change_id": 189,
  "latest_change_unix_ms": 1772595315000,
  "snapshot_files": [
    {
      "file_path": "Daily/2026-03-04.md",
      "content_base64": "IyBEYWlseSBub3RlLi4u",
      "content_sha256": "",
      "size_bytes": 128,
      "updated_unix_ms": 1772595300123
    }
  ]
}
```

- 응답 (delta 모드):

```json
{
  "ok": true,
  "vault_uid": "my-obsidian-vault",
  "mode": "delta",
  "since_unix_ms": 1772595000000,
  "latest_change_id": 189,
  "latest_change_unix_ms": 1772595315000,
  "changed_files": [
    {
      "file_path": "Daily/2026-03-04.md",
      "action": "modified",
      "changed_at_unix_ms": 1772595300123,
      "content_base64": "IyBEYWlseSBub3RlLi4u",
      "content_sha256": "",
      "size_bytes": 128
    },
    {
      "file_path": "Drafts/old.md",
      "action": "deleted",
      "changed_at_unix_ms": 1772595315000
    }
  ],
  "change_log": [
    {
      "change_id": 188,
      "file_path": "Daily/2026-03-04.md",
      "action": "modified",
      "changed_at_unix_ms": 1772595300123,
      "device_id": "iphone-15"
    },
    {
      "change_id": 189,
      "file_path": "Drafts/old.md",
      "action": "deleted",
      "changed_at_unix_ms": 1772595315000,
      "device_id": "iphone-15"
    }
  ]
}
```

### 12.3 `POST /vaults/sync/full-push`

- 목적: 볼트 디렉토리 전체를 한 번에 업로드(스냅샷 교체)
- 동작:
  - `files`에 포함된 파일 집합이 서버 최신 전체 스냅샷이 됩니다.
  - 기존 서버 파일 중 `files`에 없는 항목은 삭제 처리됩니다.
  - `./vault_storage/<vault_uid>/...` 미러 디렉토리를 전체 재작성합니다.

- 요청:

```json
{
  "vault_uid": "my-obsidian-vault",
  "device_id": "macbook",
  "uploaded_at_unix_ms": 1772595400000,
  "files": [
    { "file_path": "Daily/2026-03-04.md", "content_base64": "IyBEYWlseSBub3RlLi4u" },
    { "file_path": "Projects/plan.md", "content_base64": "IyBQbGFuLi4u" }
  ]
}
```

- 응답: `POST /vaults/sync/push`와 동일한 형태

### 12.4 `POST /vaults/sync/full-pull`

- 목적: 볼트 디렉토리 전체를 한 번에 다운로드
- 요청:

```json
{ "vault_uid": "my-obsidian-vault", "limit": 5000 }
```

- 응답: `POST /vaults/sync/pull`의 full 모드(`mode: "full"`, `snapshot_files`)와 동일

### 12.5 `POST /vaults/sync/status`

- 목적:
  - 현재 vault 최신 sync timestamp(`latest_change_unix_ms`) 확인
  - 파일별 서버 timestamp(`file_timestamps`) 조회로 델타 push 대상 탐색
  - `since_unix_ms` 기준 변경 로그(`change_log`) 조회
- 요청:

```json
{
  "vault_uid": "my-obsidian-vault",
  "since_unix_ms": 1772595000000,
  "limit": 5000
}
```

- 참고:
  - `since_unix_ms`는 선택입니다. 생략하면 최신 `limit`개 변경 로그를 반환합니다.
  - `file_timestamps`는 현재 vault 파일 인덱스를 항상 반환하며 `is_deleted` 상태를 포함합니다.

- 응답:

```json
{
  "ok": true,
  "vault_uid": "my-obsidian-vault",
  "since_unix_ms": 1772595000000,
  "latest_change_id": 189,
  "latest_change_unix_ms": 1772595315000,
  "file_timestamps": [
    {
      "file_path": "Daily/2026-03-04.md",
      "updated_unix_ms": 1772595300123,
      "size_bytes": 128,
      "is_deleted": false,
      "last_change_id": 188
    },
    {
      "file_path": "Drafts/old.md",
      "updated_unix_ms": 1772595315000,
      "size_bytes": 0,
      "is_deleted": true,
      "last_change_id": 189
    }
  ],
  "change_log": [
    {
      "change_id": 188,
      "file_path": "Daily/2026-03-04.md",
      "action": "modified",
      "changed_at_unix_ms": 1772595300123,
      "device_id": "iphone-15"
    },
    {
      "change_id": 189,
      "file_path": "Drafts/old.md",
      "action": "deleted",
      "changed_at_unix_ms": 1772595315000,
      "device_id": "iphone-15"
    }
  ]
}
```
