# Prime Radiant — DATA_CONTRACTS

개념 스키마와 불변식. 실제 DDL은 각 Phase 구현이 작성하되, 이 문서의 이름·의미·제약을 바꾸려면 `DECISIONS.md`에 결정을 추가해야 한다.
표기: **[DB]** = SQLite 제약/트리거/트랜잭션으로 강제, **[RS]** = Rust domain 계층에서 강제(DB가 표현하기 어렵거나 정책적인 것). 둘 다 표시된 것은 이중 방어.

## 0. 공통 규칙

- ID: ULID 텍스트(시간순 정렬 가능, 클라이언트가 만들지 않음). 서버가 생성. [RS]
- 시각: RFC 3339 UTC 텍스트(밀리초). `captured_at`류는 서버 시계. 클라이언트가 준 시각은 별도 필드로만 저장. [RS]
- 텍스트: UTF-8, **정규화하지 않는다**(NFC/NFKC, 공백 정리, 줄바꿈 변환 모두 금지). 바이트 그대로. [RS + DB 해시 검증]
- 권위(authoritative) 테이블은 **불변** 또는 **append-only**다. `UPDATE`/`DELETE` 금지 트리거를 둔다. [DB] 유일한 예외는 운영 테이블 `job`(§12)과 파생 테이블(§11).
- 현재 상태는 이벤트에서 계산한 **투영(projection)**이다. 행을 고쳐 상태를 바꾸지 않는다.
- 행위자 enum `Actor`: `user | agent | import | system | model`. `system`은 서버 자신의 기계적 행위(재추출에 따른 대체 등), `model`은 모델 제안(판정 기록에만).
- 모든 FK 컬럼에 인덱스(E5: 인덱스 없을 때 불변식 검사 O(n²)). [DB]

## 1. Principal과 저자성

| 개념 | 값 | 누가 정하나 | 의미 |
|---|---|---|---|
| `recorder` | `user`, `agent`, `import` | **서버**: 인증된 Principal에서만 도출 | 이 바이트를 Prime Radiant에 쓴 주체. 서버가 확실히 아는 유일한 저자 사실 |
| `speaker` | `user`, `agent`, `external`, `unknown` (+ `speaker_label` 자유 텍스트, 예: "민수") | 추출기/에이전트 신고 | Claim이 보고하는 주장의 주인 |
| `basis` | `verbatim`, `paraphrase`, `inferred`, `observed`, `restated`, `operational` | 추출기/에이전트 신고 | 도출 방법 |
| `source_ref` | 자유 텍스트/URI (`artifact_ref`) | 클라이언트 신고 | 정보의 출처(파일, URL, 도구 출력) |
| `retention_request` | `none`, `user_requested` | 에이전트 신고(검증 불가) | "기억해줘" = 보존 권한 신고. **저자 승격이 아니다** |
| 노출 권한 | 투영 규칙 §8 + 사용자 확인 이벤트 | 서버 | |

규칙:
- P-1 [RS] `recorder`는 API 입력 필드가 아니다. transport가 인증으로 만든 `Principal`에서만 온다. Principal 없는 쓰기 경로는 존재하지 않는다(기본값 없음).
- P-2 [RS] `retention_request=user_requested`나 `speaker=user`, `basis=verbatim`은 `recorder`를 바꾸지 않으며 사용자 투영에 넣지 않는다.
- P-3 [DB+RS] `basis=verbatim`은 앵커 필수, `basis=observed`는 source의 `artifact_ref` 필수. 위반 시 거부(조용한 강등 금지).
- P-4 사용자 ID 없음. 에이전트 구분이 필요하면 `recorder_label`(클라이언트 이름)과 `session_id`를 메타데이터로 저장하되 권한 판단에는 쓰지 않는다.

## 2. 권위 테이블 개요

| 테이블 | 성질 | Phase |
|---|---|---|
| `schema_migration` | append-only | 1 |
| `source` | 불변 | 1 |
| `status_event` | append-only | 1 (source 대상), 3+ (기타 대상) |
| `source_citation` | append-only (+ 철회 이벤트) | 4 |
| `claim` | 불변 | 3 |
| `mention` | 불변 (claim과 함께 생성) | 3 |
| `claim_dependency` | append-only (+ 철회 이벤트) | 3(선언 저장) / 4(의미 적용) |
| `session_read` | append-only | 4 |
| `entity` | 불변 (얇은 ID) | 4 |
| `identity_assertion` | append-only (+ 철회 이벤트) | 4 |
| `relation` | append-only (+ 철회 이벤트) | 4 (시간), 7 (모순 확정) |
| `model_judgment` | 불변 | 4 (시간), 7 (충돌) |
| `job` | **운영(가변)** | 3 |

## 3. Source

원문. 사용자 노트, 에이전트의 `remember` 입력, 외부 가져오기 모두 Source다(원문 없는 Claim은 없다 — Foundation-MCP의 "File 없는 Atom"을 대체).

| 필드 | 필수 | 설명 |
|---|---|---|
| `id` | ✓ | ULID |
| `recorder` | ✓ | §1 |
| `recorder_label` | | 클라이언트/도구 이름 |
| `session_id` | | 에이전트 세션(의존 미선언 감지용) |
| `content` | ✓ | 원문 바이트(UTF-8 텍스트). 빈 문자열 거부 |
| `content_sha256` | ✓ | `content` 바이트의 SHA-256 hex. [DB] 저장 시 계산, 무결성 검사에서 재검증 |
| `media_type` | ✓ | Phase 1은 `text/plain`, `text/markdown`만 |
| `authored_at` | | 진술 시점(클라이언트 신고). 모르면 NULL — 캡처 시각으로 채우지 않는다 |
| `observed_at` | | 관측 시점(basis=observed의 근거 시각) |
| `captured_at` | ✓ | 서버 수신 시각 |
| `artifact_ref` | | 출처 |
| `retention_request` | ✓ | 기본 `none` |
| `idempotency_key` | | 클라이언트 키 |
| `request_digest` | ✓ | 정규화된 요청 필드 해시(§3.1) |

제약: [DB] `UNIQUE(recorder, idempotency_key)` (키가 NULL이면 중복 허용 — 같은 노트를 두 번 쓰는 것은 정당한 두 기록), CHECK enum, UPDATE/DELETE 금지 트리거.
언어: 원문 언어를 그대로 저장. 언어 태그는 파생 정보로만 계산.

### 3.1 멱등성
- 같은 `(recorder, idempotency_key)` + 같은 `request_digest` → 기존 `id` 반환, `created=false`.
- 같은 키 + 다른 digest → `IdempotencyConflict` 오류, 아무것도 쓰지 않음.
- 동시 요청: 단일 writer + `BEGIN IMMEDIATE` + UNIQUE 제약. 제약 위반을 잡아 기존 행을 재조회해 반환.
- `request_digest` = SHA-256(`recorder` ‖ `media_type` ‖ `content` ‖ `authored_at` ‖ `observed_at` ‖ `artifact_ref` ‖ `retention_request`), 필드는 길이 접두 인코딩. `captured_at`, `recorder_label`, `session_id`는 제외.

## 4. Status event와 현재 상태

`status_event(seq INTEGER PK AUTOINCREMENT, target_kind, target_id, event, actor, reason, created_at)`

- `target_kind` ∈ `source, claim, dependency, citation, identity_assertion, relation`.
- `event` ∈ 두 축:
  - **lifecycle**: `retract`(틀렸다/근거 아님), `forget`(검색·투영에서 제거, 감사용 보존), `restore`(live로 복귀)
  - **endorsement**: `confirm`, `unconfirm` (사용자만)
- 현재 lifecycle = 그 대상의 lifecycle 이벤트 중 최대 `seq`의 것(없으면 `live`; `restore`면 `live`). endorsement도 같은 방식으로 독립 계산. 두 축은 서로 덮어쓰지 않는다.
- 전이 권한 [DB 트리거 + RS]:

| event | user | agent | system | import |
|---|---|---|---|---|
| retract / forget / restore 대상이 **user가 기록**한 source(또는 그 source에서 나온 claim) | ✓ | ✗ | ✗ | ✗ |
| retract / forget / restore 기타 대상 | ✓ | ✓ (자기 recorder 범위) | `retract`만, 사유 코드 필수(예: `revision`) | ✗ |
| confirm / unconfirm | ✓ | ✗ | ✗ | ✗ |

- `forget`된 대상의 `restore`는 user만.
- 물리 삭제(purge)는 계약 밖이다(향후 별도 관리 작업, 백업 정책과 함께 결정).
- 같은 대상에 같은 현재 상태를 만드는 이벤트는 no-op 성공(멱등) — 이벤트를 추가하지 않는다. [RS]

## 5. Claim

원문 하나에서 추출된 자기완결적 의미 단위. 내용 불변.

| 필드 | 필수 | 설명 |
|---|---|---|
| `id`, `source_id` | ✓ | |
| `revision_of` | | 같은 원문의 이전 추출 버전(재추출). §5.2 |
| `text` | ✓ | 틀 유지 문장. 귀속·인용·부정·불확실·주관·가정·조건·질문을 문장 안에 명시, 시간 지시어 절대화(근거 시각 = `authored_at`, 없으면 "미상 시점"), 해소 불가 지시어는 "미상의 인물/대상". 대명사·지시어 미해소 금지 |
| `lang` | ✓ | BCP-47 주 언어. **원문 언어와 같아야 함** [RS] |
| `category` | ✓ | `Fact | View | Intent | Rule` — 의미 범주, 진위 보장 아님 |
| `speaker`, `speaker_label`, `basis` | ✓/–/✓ | §1 |
| `anchor_start`, `anchor_end` | 조건부 | `source.content`의 UTF-8 **바이트** 오프셋, char 경계여야 함. verbatim은 필수 |
| `tkind` | | `habit | state | event | observation | decision | report_past` |
| `applies_from`, `applies_to` | | 원문에 명시된 경우만. 한쪽만 명시되면 다른 쪽은 NULL(=미상). 지속 기간 발명 금지 |
| `standing` | ✓ | 지속적 제약/규칙/선호 여부 — **검색 힌트일 뿐**, 정합성 판단 근거 아님 |
| `index_fields` | | JSON: stance, holder, certainty, condition. 필터용. 문장과 충돌 시 문장 우선 |
| `extractor_id` | ✓ | provider + model id + prompt hash |
| `job_id` | ✓ | 생성한 작업 (멱등 키) |
| `created_at` | ✓ | |

### 5.1 검증 경계 [RS]
모델 제안은 다음을 모두 통과해야 저장된다: 앵커가 원문의 정확한 부분 문자열; `lang` = 원문 주 언어(혼합 원문은 앵커 구간의 언어); 필수 필드; enum; P-3. 하나라도 실패한 Claim이 있으면 그 작업의 결과 전체를 거부하고 사유를 `job.last_error`에 남긴다(부분 저장으로 인한 의미 누락 방지, D-13).

### 5.2 버전 교체 ≠ 시간적 대체 ≠ 모순
- **버전 교체**(`revision_of`): 같은 원문의 더 나은 해석. 이전 Claim은 철회되지 않는다. "현재 표현" 투영에서 빠질 뿐, 이를 전제로 한 파생 Claim은 유효성을 유지한다. 이전 해석이 **틀렸다**면 별도로 `retract` 이벤트를 쓴다(→ 의존 전파).
- **시간적 대체**(`relation.kind=supersedes`): 세계가 바뀌었다. 둘 다 역사적으로 참일 수 있다.
- **모순**: 같은 시점·대상에 대해 양립 불가. 사용자 확정만(§7).

## 6. 의존과 증거

`claim_dependency(id, child, parent, kind, declared_by, created_at)` + `status_event(target_kind=dependency)`로 철회.

| kind | 의미 | 무효화 전파 | 독립성 |
|---|---|---|---|
| `premise` | 결합 전제(모두 필요) | ✓ | 같은 원천 공유 |
| `restates` | 같은 명제의 재진술/인용 | ✓ | 원천 공유 — 독립 증거 아님 |
| `alternative` | 대체 증거(하나면 충분) | 모든 alternative가 무효일 때만 | |
| `informed_by` | 참고했으나 필수 아님 | ✗ | 독립성 미확인 표시만 |

- **늦게 발견된 의존과 과거 가져오기**: Phase 2 R2의 "이전 Claim만 참조" 트리거를 폐기한다(D-09). 의존은 언제든 추가할 수 있고, 비순환은 삽입 트랜잭션 안에서 `parent`에서 `child`로 도달 가능한지 재귀 CTE로 검사해 거부한다. [RS+DB tx]
- `source_citation(child_source, parent_source)`: 원문 간 인용. 독립성 계산용, 무효화 전파 안 함.
- **유효성(3값, 물질화 `claim_validity`)**:
  - `invalid`: 자신 또는 자기 source가 retract/forget, 또는 premise/restates 부모 중 invalid가 있음, 또는 alternative가 있고 전부 invalid.
  - `needs_revalidation`: invalid는 아니지만 (a) `informed_by`/미선언 의존(§6.1) 대상 중 invalid가 생김, 또는 (b) 생성 시 전제들의 상태가 이후 바뀜.
    - **명확화(Phase 4, D-92)**: (b)는 이력이 아니라 현재 상태로 해석한다 — premise/restates 부모가 현재 `needs_revalidation`이거나, alternative가 있으나 valid인 것이 하나도 없음. 이력 해석은 의존·가져오기·이벤트 도착 순서에 따라 결과가 달라져 순서 독립성(늦게 발견된 의존, 과거 가져오기)과 모순된다. 유효성은 현재 권위 상태의 순수 함수다.
  - `valid`: 나머지.
  - 어떤 경우에도 "거짓"으로 표시하지 않는다. invalid = 근거가 사라짐.
- 원문 철회와 의존 철회는 같은 메커니즘이다: source retract → 그 source의 claim invalid → 하향 전파. 철회 이벤트와 `claim_validity` 증분 갱신은 **같은 트랜잭션**. 뷰 기반 전체 재계산을 오라클로 테스트한다.
- **지지(support)**: 명제에 대해 valid Claim들의 원천 집합 = `restates`와 `source_citation`을 거슬러 올라간 비-재진술 source 집합. 표시는 "원천 n개(독립성 미확인 m개)". 숫자를 확률로 쓰지 않는다. 같은 source에서 나온 Claim 여러 개는 원천 1개다.

### 6.1 미선언 의존
`session_read(session_id, claim_id, at)`: 에이전트 세션에 반환한 Claim 기록. 에이전트 Source의 Claim이 의존을 선언하지 않았고 같은 세션 read가 있으면 `deps_unknown`(파생 플래그). 지지 집계에서 독립성 미확인, 같은 세션에서 읽은 Claim이 invalid가 되면 `needs_revalidation`.

## 7. 관계와 모델 판정

두 테이블로 분리한다(D-10). **모델 판정은 기록이고, 관계는 단언이다.**

`model_judgment(id, task, claim_a, claim_b, provider, model_id, prompt_hash, input_digest, raw_label, output_json, created_at)` — 불변.
- `task` ∈ `conflict, temporal`. `raw_label`은 모델이 낸 그대로(`contradiction, potential, temporal_change, related, insufficient, none` / `supersedes, exception, backfill, compatible`). **사실이 아니라 힌트**.
- 멱등: `UNIQUE(task, claim_a, claim_b, model_id, prompt_hash, input_digest)`. 다른 모델의 판정은 새 행 → 불일치 보존.
- 입력이 영어 표현으로 변환된 경우(`input_digest`에 표현 버전 포함) `output_json.via_translation=true`.

`relation(id, kind, subject, object, actor, judgment_id, reason, created_at)` + `status_event(target_kind=relation)`.
- `kind` ∈ `supersedes, exception, backfill, contradiction, not_conflicting`.
- `contradiction`, `not_conflicting`은 **actor=user만** [DB 트리거]. 사용자가 판정을 보고 확정/기각한 결과이며 `judgment_id`로 근거 판정을 가리킬 수 있다.
- `supersedes/exception/backfill`은 user 또는 agent(명시적 정정 "앞의 말은 틀렸고…")가 단언. agent 단언은 `provisional`, user 단언은 `confirmed` (standing은 actor에서 계산).
- 모델은 `relation`을 쓰지 못한다. 시간 파이프라인의 모델 출력도 `model_judgment(task=temporal)`에 남는다.

파생 투영:
- **possible_conflict(a,b)** = conflict 판정 중 `raw_label ∈ {contradiction, potential, temporal_change, insufficient}`가 하나 이상 있고, 같은 쌍에 live `contradiction`/`not_conflicting` relation이 없으며 두 Claim이 모두 invalid가 아님. Phase 3 연구의 `potential_conflict`는 이 **파생 상태의 이름**이지 저장 라벨이 아니다(D-10). 에이전트에만 노출, 어떤 Claim의 유효성·가시성·순위도 바꾸지 않는다.
- **confirmed contradiction** = live `relation(kind=contradiction, actor=user)`. 두 Claim 모두 유지·병기, 자동 해소 없음.
- **effective supersedes (as_of t)** = live supersedes relation ∪ (temporal 판정 `supersedes` 중 사용자가 기각하지 않은 것, `provisional` 표시). as_of t에서는 t 시점에 이미 효력이 있던 것만. 대체돼도 이전 Claim은 숨기지 않고 "이력"으로 반환.
- 판정 철회: 판정 행은 불변이므로, 잘못된 판정은 같은 쌍에 사용자 `not_conflicting`으로 덮거나 판정의 기반 Claim이 invalid가 되면 자동으로 투영에서 빠진다. 모델/프롬프트 단위 무효화가 필요하면 `system` actor의 relation이 아니라 투영 필터(`model_id` 차단 목록, 설정)로 처리한다.

## 8. 시간

| 시각 | 위치 | 의미 |
|---|---|---|
| 진술 시점 | `source.authored_at` | 말/쓴 시각(신고, NULL 가능) |
| 관측 시점 | `source.observed_at` | 관측한 시각 |
| 캡처 시점 | `source.captured_at` | 서버 수신 |
| 적용 시점 | `claim.applies_from/to` | 명시된 경우만 |

- 역사적 단언은 적용성이 바뀌어도 그대로 남는다. 현재 질의는 신선도 플래그(`stale_hint`)를 붙일 뿐 숨기거나 무효화하지 않는다. 임계값(60일/14일)은 잠정 설정값이지 계약이 아니다.
- "지난주까지" 같은 종료만 명시된 진술의 시작은 NULL.
- 시간 감쇠는 순위 신호로만.

## 9. 정체성

- `mention(id, claim_id, surface, span)`: Claim 문장 속 표면형. 불변.
- `entity(id, kind, created_at)`: 얇은 ID. 이름은 파생.
- `identity_assertion(id, kind, subject, object, actor, reason, judgment_id, created_at)` + 철회 이벤트:
  - `refers_to(mention → entity)`, `same_as(entity ↔ entity)`, `distinct_from(entity ↔ entity)`, `alias(entity, text)`.
  - actor ∈ `model`(추출기 제안), `agent`, `user`.
- **현재 해소(파생)**: live 단언으로 union-find. 우선순위 user > agent > model. user `distinct_from`이 있으면 어떤 `same_as`도 그 둘을 합치지 못함. 병합 해제 = `same_as` 철회 → 해소 재계산 → 해당 엔티티에 의존한 파생(alias 인덱스, 후보 풀)은 재구축 작업 대상.
- **수동 교정은 재구축 가능한가**: 예. `entity_alias` 인덱스는 파생이고, 수동 교정은 권위 `identity_assertion(actor=user|agent)`에 있으므로 재추출로 지워지지 않는다. 재추출은 이전 `model` 단언을 `system`이 철회(`reason=revision`)하고 새 단언을 추가한다. user 단언은 system이 철회할 수 없다.
- 자동 병합 기본값: 하지 않음. 모델 `same_as` 제안은 저장하되 해소에는 신뢰도 정책(설정)이 허용할 때만 반영 — 초기값은 "반영 안 함, alias 후보로만 사용". 동명이인은 "같은 이름 = 후보 포함"으로 재현율을 확보하고 해소는 descriptor 기반으로.

## 10. 가시성 (투영)

서버 계산. 모델·에이전트의 선언으로 결정하지 않는다.

- **user projection**:
  - Source: `recorder=user` ∧ lifecycle=live.
  - Claim: lifecycle(자신·source) ≠ retract/forget ∧ validity ≠ invalid ∧ ( (`source.recorder=user` ∧ `speaker=user` ∧ `basis=verbatim` ∧ 앵커 검증) ∨ 사용자 `confirm` endorsement 현재 유효 ).
  - Relation/판정: 양 끝 Claim이 모두 user projection 안이고 actor=user인 relation만. 모델 판정·possible_conflict는 노출 안 함.
  - 엔티티 alias/이름: user projection Claim의 mention에서 온 것 + user 단언만.
- **agent view**: forget되지 않은 모든 것 + 메타데이터(recorder, speaker, basis, validity, 원천 수, stale_hint, possible_conflict, revision 여부). retract된 것은 기본 제외, `include_retracted` 시 표시.
- 누출 방지 [RS + 테스트]: 검색 점수, 결과 개수, 총계, 자동완성/alias, 요약, 관계, 오류 메시지("존재하지만 권한 없음" 금지 → "없음"), 페이지네이션 커서 모두 투영 집합 안에서 계산. Phase 2 누출 검사 6종을 Phase 2·5·6에서 재구현.

## 11. 파생 인덱스 (권위 아님, 언제든 재구축)

| 이름 | 입력 | 재구축 키 |
|---|---|---|
| `claim_validity` | claim, source, dependency, status_event | 전체 재계산 = 오라클 |
| `claim_fts` | claim.text → 문자 bigram + ASCII 단어 | 토크나이저 버전 |
| `source_fts` (선택) | source.content | 동일 |
| `claim_vec(claim_id, model_id, dim, vec)` | claim.text 원문 언어 | embedding model id |
| `entity_alias` | live identity_assertion + mention | 해소 버전 |
| `claim_translation` (기본 off) | claim.text | 번역 모델 id; 표시·판정 금지 |
| `projection_cache` (필요 시) | 위 전부 | — |

재구축은 권위 테이블만 읽는다. 재구축 전후 권위 테이블 해시가 같아야 한다(테스트).

## 12. 작업 (운영 테이블)

`job(id, kind, target_id, idem_key UNIQUE, state, attempts, max_attempts, lease_owner, lease_until, run_after, last_error, created_at, updated_at)`
- state: `pending → leased → done | pending(backoff) | failed`; `failed → pending`(수동 재시도). 
- 임대 만료 시 다른 워커가 재획득 가능 → 핸들러는 결과 쓰기를 `job_id` 멱등으로(예: claim.job_id 존재 시 no-op).
- 결과 쓰기와 `done` 전이는 같은 트랜잭션.

## 13. 트랜잭션 경계 요약

| 연산 | 하나의 트랜잭션에 포함 |
|---|---|
| 원문 기록 | source (+ Phase 3부터 extraction job) |
| 추출 결과 | claims + mentions + 선언된 dependencies + 모델 identity 단언 + 후속 jobs + job done |
| 상태 이벤트 | status_event + claim_validity 증분 갱신 + 재검증 jobs |
| 의존 추가 | 순환 검사 + dependency + validity 갱신 |
| 판정 기록 | model_judgment + job done |
| 사용자 확정 | relation + (선택) endorsement 이벤트 |
| 마이그레이션 | 한 버전 = 한 트랜잭션 + schema_migration 행 |

모델·네트워크 호출은 어떤 트랜잭션 안에도 두지 않는다.

## 14. 검색과 처리 큐 (Phase 5)
- 스키마 v4: `job.kind ∈ {extract, revalidate, index_lexical, index_embed}`, `state ∈ {pending, leased, done, failed, deferred, cancelled}`, `priority`, `generation`(= extraction_run seq), `detail`(embedding model key), `first_leased_at/started_at/finished_at`.
- `claim_access`(파생): Claim별 user/agent 가시성. 생명주기·유효성·확정·게시와 같은 트랜잭션에서 갱신. 검색 후보는 이 투영 안에서만 만들고, 반환 직전에 권위 쿼리로 재확인한다.
- `claim_lex` + FTS5 `claim_fts`(파생, tokenizer 버전 기록), `embedding_model`, `claim_vec`(model_key별 격리, f16), `index_epoch`.
- 영수증: `record`는 `{source_id, created, processing}`를 돌려준다. `processing`은 단계(stored_not_extracted → awaiting_approval → published_awaiting_lexical → awaiting_embedding → fully_indexed / failed / deferred / stale)이며 색인 완료를 뜻하지 않는다.
- API: `search`, `source_processing`, `queue_status`, `queue_jobs`, `queue_retry`(큐 관리는 user 전용; 원문 provider 오류는 저장·반환하지 않고 고정 코드만).
- `search` 응답: results(match_type, evidence, score, why, related, temporal), plan, executed, semantic_used, graph_used, coverage, indexing, continuation(서명된 커서), timing.
