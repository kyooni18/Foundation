# Prime Radiant — PHASE1_SPEC (Persistence Core)

이 문서만으로 Phase 1을 구현할 수 있어야 한다. 근본 계약을 새로 설계해야 하는 지점이 발견되면 구현을 멈추고 `DECISIONS.md`에 항목을 추가한 뒤 진행한다.

## 1. 범위

포함: 단일 crate `prime_radiant`(lib + bin `primeradiant`), `domain`, `store`, `ingest`, 최소 `authz`(Principal 타입과 Source 권한 검사), CLI, 테스트.
제외: HTTP/MCP, 토큰, Claim/추출, job 큐, 모델·임베딩·네트워크, 검색 인덱스, Tokio(아직 비동기 I/O 없음 — 동기 API로 시작하고 P2에서 `spawn_blocking`으로 감싼다).

의존성(책임이 명확한 것만): `rusqlite`(features `bundled`, `backup`), `sha2`, `ulid`, `time`(RFC 3339), `serde`/`serde_json`(CLI 출력), `thiserror`, `tracing` + `tracing-subscriber`, `clap`(CLI). 테스트: `tempfile`. 그 외 추가는 사유를 커밋 메시지에 남긴다.

## 2. 모듈 구조

```
src/
  lib.rs            pub struct App; pub mod ...
  domain/{mod,ids,principal,source,status,error}.rs
  store/{mod,conn,migrate,tx,integrity,backup}.rs
  store/migrations/0001_init.sql
  ingest.rs
  authz.rs
  bin/primeradiant.rs
tests/{ingest,idempotency,concurrency,crash,migrate,backup,integrity,authorship}.rs
```

## 3. 도메인 타입

```rust
pub enum PrincipalKind { User, Agent, Import }
pub struct Principal { kind: PrincipalKind, label: Option<String>, session_id: Option<String> }
// 생성자는 crate 내부 + 명시적 테스트/CLI 경로만. Default 구현 금지.

pub enum MediaType { TextPlain, TextMarkdown }
pub enum RetentionRequest { None, UserRequested }
pub enum LifecycleEvent { Retract, Forget, Restore }
pub enum Lifecycle { Live, Retracted, Forgotten }

pub struct RecordSource {           // API 입력. recorder 필드 없음(P-1)
  content: String, media_type: MediaType,
  authored_at: Option<Timestamp>, observed_at: Option<Timestamp>,
  artifact_ref: Option<String>, retention_request: RetentionRequest,
  idempotency_key: Option<String>,   // 1..=200 bytes, 출력 가능한 ASCII
}
pub struct Recorded { source_id: SourceId, created: bool }
```

`App` API (동기):
- `record_source(&Principal, RecordSource) -> Result<Recorded>`
- `get_source(&Principal, SourceId) -> Result<SourceView>` — Phase 1 투영: user는 `recorder=user` ∧ live만, agent/import는 forget 아닌 전부(현재 lifecycle을 함께 반환; 목록 조회가 생기는 P2부터 retracted는 기본 제외). 보이지 않으면 `NotFound`(존재 누출 금지).
- `set_source_lifecycle(&Principal, SourceId, LifecycleEvent, reason: Option<String>) -> Result<()>`
- `verify_integrity() -> Result<IntegrityReport>`; `backup_to(path)`; `App::restore_from(backup, target)`.

오류(`thiserror`): `InvalidInput{field, reason}`, `IdempotencyConflict{source_id}`, `NotFound`, `Forbidden`, `IntegrityFailure`, `SchemaTooNew{db, binary}`, `UnsafeLocation(path)`, `Storage(rusqlite::Error)`. `Forbidden`은 대상이 보이는 Principal에게만 반환(보이지 않으면 `NotFound`).

입력 검증 [RS]: content 비어 있지 않음, ≤ 1 MiB(설정 상수), UTF-8은 `String`이 보장. 어떤 정규화도 하지 않음. 타임스탬프 RFC 3339 파싱 가능해야 함(저장은 받은 문자열이 아니라 UTC 정규 표기 — 원문이 아닌 메타데이터이므로 허용).

## 4. SQLite

연결 설정(모든 연결): `journal_mode=WAL`, `synchronous=FULL`, `foreign_keys=ON`, `busy_timeout=10000`, `trusted_schema=OFF`. writer 연결 1개를 `Mutex`로 보호, 쓰기는 `BEGIN IMMEDIATE`. reader는 연결당 하나씩 열어도 됨(풀은 P2).

위치: 기본 `~/Library/Application Support/PrimeRadiant/prime_radiant.db`, `--db`로 변경 가능. 경로를 canonicalize해 `/Library/Mobile Documents/` 또는 `/Library/CloudStorage/` 포함 시 `UnsafeLocation`(우회 플래그 없음).

### 4.1 `0001_init.sql` (스케치 — 이름·의미는 계약, 세부 문법은 구현 재량)

```sql
CREATE TABLE schema_migration(version INTEGER PRIMARY KEY, name TEXT NOT NULL,
  checksum TEXT NOT NULL, applied_at TEXT NOT NULL);

CREATE TABLE source(
  id TEXT PRIMARY KEY,
  recorder TEXT NOT NULL CHECK (recorder IN ('user','agent','import')),
  recorder_label TEXT, session_id TEXT,
  content TEXT NOT NULL CHECK (length(content) > 0),
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  media_type TEXT NOT NULL CHECK (media_type IN ('text/plain','text/markdown')),
  authored_at TEXT, observed_at TEXT, captured_at TEXT NOT NULL,
  artifact_ref TEXT,
  retention_request TEXT NOT NULL DEFAULT 'none' CHECK (retention_request IN ('none','user_requested')),
  idempotency_key TEXT, request_digest TEXT NOT NULL,
  UNIQUE(recorder, idempotency_key)
) STRICT;
CREATE TRIGGER source_no_update BEFORE UPDATE ON source BEGIN SELECT RAISE(ABORT,'source is immutable'); END;
CREATE TRIGGER source_no_delete BEFORE DELETE ON source BEGIN SELECT RAISE(ABORT,'source is immutable'); END;

CREATE TABLE status_event(
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  target_kind TEXT NOT NULL CHECK (target_kind IN ('source')),   -- 이후 마이그레이션에서 확장
  target_id TEXT NOT NULL,
  event TEXT NOT NULL CHECK (event IN ('retract','forget','restore')),
  actor TEXT NOT NULL CHECK (actor IN ('user','agent','import','system')),
  reason TEXT, created_at TEXT NOT NULL
) STRICT;
CREATE INDEX status_event_target ON status_event(target_kind, target_id, seq);
CREATE TRIGGER status_no_update BEFORE UPDATE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_no_delete BEFORE DELETE ON status_event BEGIN SELECT RAISE(ABORT,'append-only'); END;
CREATE TRIGGER status_source_exists BEFORE INSERT ON status_event
  WHEN NEW.target_kind='source' AND NOT EXISTS (SELECT 1 FROM source WHERE id=NEW.target_id)
  BEGIN SELECT RAISE(ABORT,'unknown target'); END;
CREATE TRIGGER status_protect_user_source BEFORE INSERT ON status_event
  WHEN NEW.target_kind='source' AND NEW.actor<>'user'
   AND EXISTS (SELECT 1 FROM source WHERE id=NEW.target_id AND recorder='user')
  BEGIN SELECT RAISE(ABORT,'only the user can change a user-recorded source'); END;
CREATE TRIGGER status_restore_forgotten BEFORE INSERT ON status_event
  WHEN NEW.event='restore' AND NEW.actor<>'user'
   AND (SELECT event FROM status_event WHERE target_kind=NEW.target_kind AND target_id=NEW.target_id
        ORDER BY seq DESC LIMIT 1) = 'forget'
  BEGIN SELECT RAISE(ABORT,'only the user can restore a forgotten target'); END;
```
권한 매트릭스의 나머지(agent는 자기 recorder 범위만: agent는 agent source, import는 아무것도 못 함)는 [RS]에서 검사하고 위 트리거는 최후 방어선이다. `confirm/unconfirm`, Claim 대상은 Phase 1에 없다.

### 4.2 마이그레이션
- 바이너리에 `include_str!`로 내장, 번호 순 적용. 각 버전: 적용 전 `backup_to(<db>.pre-v{N}.{ts}.bak)` → `BEGIN IMMEDIATE` → SQL → `schema_migration` 행(checksum = SQL SHA-256) → `PRAGMA user_version=N` → COMMIT.
- 기동 시: 적용된 행의 checksum이 내장 SQL과 다르면 `IntegrityFailure`; DB 버전 > 바이너리 최대 → `SchemaTooNew`, 열기 거부.
- 되돌리기: 전진 전용. 롤백 = pre-migration 백업 복원(테스트로 검증). down SQL은 Phase 1에서 만들지 않는다(D-25).

## 5. 기록 알고리즘

```
record_source(p, req):
  validate(req)
  digest = sha256(len-prefixed(p.kind, media_type, content, authored_at, observed_at, artifact_ref, retention_request))
  BEGIN IMMEDIATE
    if req.idempotency_key:
      row = SELECT id, request_digest FROM source WHERE recorder=? AND idempotency_key=?
      if row: COMMIT; return row.digest==digest ? Recorded{row.id, false} : Err(IdempotencyConflict)
    INSERT source(id=ulid(), recorder=p.kind, ..., content_sha256=sha256(content bytes), captured_at=now())
  COMMIT
  return Recorded{id, true}
```
writer 직렬화 때문에 UNIQUE 위반은 정상 경로에서 발생하지 않지만, 발생하면(다른 프로세스 등) 재조회 후 같은 규칙으로 처리한다.

lifecycle 이벤트: `BEGIN IMMEDIATE` → 대상 조회(보이지 않으면 NotFound) → 권한 검사 → 현재 상태가 이미 목표 상태면 no-op → INSERT → COMMIT.

구현 명확화(D-30): user lifecycle 관리 명령은 read projection과 달리 모든 기존 source를 대상으로 허용한다(망각 복원 포함). 비-user 명령에는 위 가시성 검사를 그대로 적용한다. `get_source`의 투영은 변경하지 않는다.

## 6. 무결성 검사 (`verify_integrity`, CLI `check`)
1. `PRAGMA integrity_check` = ok, `PRAGMA foreign_key_check` 비어 있음.
2. 모든 source에 대해 `sha256(content) == content_sha256`.
3. `schema_migration` checksum 일치, `user_version` = 최대 version.
4. 모든 status_event의 target 존재.
5. 트리거 존재 확인(이름 목록).
보고서: 항목별 ok/위반 개수/첫 위반 id. 하나라도 실패하면 이후 쓰기 거부(`App`이 read-only 모드로 전환).

## 7. 백업/복원
- 백업: rusqlite online backup API로 임시 파일에 복사 → 그 파일에 대해 §6 검사 → 원자적 rename. 쓰기 진행 중에도 가능해야 한다.
- 복원: 서버(App) 미실행 상태에서만. 백업 검사 → 대상이 존재하면 `<target>.replaced.{ts}`로 이동(삭제 금지) → 복사 → 검사.

## 8. 테스트와 수락 기준

모든 테스트는 `tempfile` 디렉터리, 네트워크 없음. 실험(R1 E5) 수치를 재사용하지 않고 독립적으로 측정한다.

| # | 테스트 | 수락 기준 |
|---|---|---|
| T1 원문 보존 | 한국어(조사·자모 조합·NFD 형태 포함 "한글"), 영어, 혼합, 이모지·ZWJ, CRLF, 앞뒤 공백, 탭을 기록 후 조회 | 바이트 동일, `content_sha256` 일치, NFC/NFD 구분 유지 |
| T2 불변 | source UPDATE/DELETE, status_event UPDATE/DELETE를 직접 SQL로 시도 | 모두 ABORT |
| T3 멱등 | 같은 키·같은 내용 2회 / 같은 키·다른 내용 / 키 없음 2회 / 같은 키 user vs agent | created=true→false 같은 id / IdempotencyConflict, 행 증가 없음 / 2행 / 2행 |
| T4 동시 중복 | 32 스레드가 같은 키·내용으로 동시 기록 | source 1행, 모든 결과 같은 id, created=true 정확히 1개 |
| T5 동시 혼합 | writer 4 × 1,000 기록(서로 다른 키) + reader 4 | 4,000행, BUSY 오류 0, 무결성 ok (처리량은 기록만, 기준 아님) |
| T6 강제 종료 | 자식 프로세스가 연속 기록하며 진행 로그(커밋된 id)를 stdout에 출력, 부모가 무작위 시점에 SIGKILL, 20회 반복 | 매 회 재시작 후 §6 ok; 출력된 커밋 id 전부 존재; 반쯤 쓰인 행 없음 |
| T7 마이그레이션 | 빈 DB → v1; 재기동 시 재적용 없음; checksum 변조 감지; user_version을 바이너리보다 크게 → 열기 거부; 테스트 전용 v2 마이그레이션 적용 후 pre-v2 백업 복원 시 v1 상태와 데이터 동일 | 전부 |
| T8 무결성 검사 | 정상 DB ok; 트리거를 임시로 우회(테스트 전용 연결에서 트리거 drop 후 content 변조) | 해시 불일치 감지, 이후 쓰기 거부 |
| T9 백업/복원 | 기록 중 백업, 복원 후 모든 행·해시 일치; 손상된 백업(바이트 변조) 복원 거부; 기존 대상은 `.replaced`로 보존 | 전부 |
| T10 저자 승격 없음 | agent Principal이 `retention_request=user_requested`로 기록 | recorder=agent, user 투영 `get_source`에서 NotFound |
| T11 권한 | agent가 user source retract/forget; import가 이벤트; agent가 forgotten agent source restore; user가 모든 전이 | Forbidden(또는 보이지 않으면 NotFound) / 트리거도 같은 결과를 직접 SQL에서 강제 / user 성공 |
| T12 위치 | iCloud 경로로 열기 | UnsafeLocation |
| T13 의존성 없음 | `cargo tree`에 HTTP 클라이언트·모델 SDK 없음; 테스트는 네트워크 차단 환경에서 통과 | 전부 |

Phase 1 완료 = T1–T13 녹색 + `cargo clippy -D warnings` + `primeradiant init/record/get/check/backup/restore` 수동 확인 + 커밋 + 태그 `phase-1`. 성능 목표는 Phase 1에서 주장하지 않는다.

## 9. 구현 중 결정해도 되는 것 / 안 되는 것
- 해도 됨: 파일 분할, 함수 이름, CLI 출력 형식, 상수 값(크기 상한), 테스트 헬퍼.
- 안 됨: 테이블·필드 이름과 의미, recorder 도출 방식, 멱등 규칙, 정규화 금지, synchronous=FULL, 전진 전용 마이그레이션 — 바꾸려면 DECISIONS.md 갱신.
