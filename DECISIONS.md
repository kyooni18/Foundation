# Prime Radiant — DECISIONS

근거 표기: `FR/` = `~/Code/Foundation-Research/` (R1: `ARCHITECTURE.md`·`EXPERIMENTS.md`, R2: `*_R2.md`, R3: `*_R3.md`·`RETRIEVAL_DECISIONS.md`·`IMPLEMENTATION_READINESS.md`), `FM/` = 기존 Foundation-MCP 구현(`~/Code/Foundation`), `OB` = Obsidian 원 설계.
상태: **Accepted** / **Provisional**(측정 전 기본값, 계약 아님) / **Rejected**.

| # | 결정 | 상태 | 근거 | 기각된 대안 | 한계 | 재검토 조건 |
|---|---|---|---|---|---|---|
| D-01 | Rust 단일 프로세스 모듈형 모놀리스 + SQLite 단일 파일(WAL), rusqlite bundled | Accepted | R1 E5: 16스레드 멱등 created=1, SIGKILL 25회 무결성 위반 0, 2.6k tx/s | PostgreSQL/pgvector(FM), 별도 벡터/그래프/큐 서비스 | 다중 프로세스 쓰기 미검증 | 두 번째 머신/프로세스가 동시에 써야 할 때 |
| D-02 | DB 소유 프로세스 하나. writer 연결 1 + reader pool, 쓰기는 `BEGIN IMMEDIATE` | Accepted | R1 E5 BUSY 0 | 다중 writer | — | writer 대기가 p99 목표를 넘을 때 |
| D-03 | DB는 `~/Library/Application Support/PrimeRadiant/`. iCloud 경로 거부. `synchronous=FULL` 기본 | Accepted | R1 E5 한계(전원 손실, iCloud 미검증, SQLite 문서) | iCloud 경로, NORMAL 기본 | FULL의 처리량 비용 미측정 | Phase 8 벤치에서 FULL이 목표 미달이면 NORMAL + 문서화된 손실 창 |
| D-04 | 사용자 ID 없음. `user`는 Principal 역할 | Accepted | R2 A15, 브리프 | 멀티테넌시 | — | 다중 사용자 요구가 실제로 생길 때 |
| D-05 | HTTP/MCP는 같은 코어 함수 호출, 권한은 코어의 `Principal`에서만 | Accepted | FM: transport별 경로 분기가 scope 버그(v7→v8 수리)의 원인 중 하나 | transport별 권한 | — | — |
| D-06 | 가시성은 서버 투영. user projection = user 기록 축자 Claim ∪ 사용자 확인 Claim, invalid 제외. 모든 집계는 투영 안에서 | Accepted | R2 A6, B2(텍스트로 의역/추론 구분 7/18–15/18), 누출 검사 6종 | basis/모델 선언 기반 가시성, 사후 필터 | 에이전트가 축자 인용해도 확인 전엔 숨김 → 사용자 UX 비용 | 서버가 사용자 발화를 직접 받는 경로가 생기면 그 경로는 recorder=user |
| D-07 | `recorder`(서버) / `speaker`·`basis`(신고) / `artifact_ref` / `retention_request` 분리. "기억해줘"는 권한이지 저자 아님 | Accepted | R2 A5, A7; FM은 에이전트 기록을 `usercreated`로 저장(R2 B0) | 단일 `origin_kind`(R1), `author` 신고 | speaker/basis는 검증 불가 | — |
| D-08 | 원문 불변·바이트 보존·정규화 금지, 원문 언어 Claim, 앵커 정확 일치 필수 검증 | Accepted | R2 A1, A2(언어 표류 55%), A4(279/280) | 영어 정규화, 원문 요약 저장 | 반어 등은 원문으로만 복구 | — |
| D-09 | 의존은 언제든 추가 가능, 비순환은 삽입 시 도달성 검사로 강제. R2의 "이전 Claim만" 트리거 폐기 | Accepted | R2 E 차분 테스트(순환이 유일한 의미 불일치 → 순환만 막으면 됨); 늦은 의존·과거 가져오기는 rowid 순서와 무관 | rowid 순서 트리거(R2) | 깊은 그래프에서 검사 비용 미측정 | 삽입 p99가 목표 초과 시 위상 레벨 캐시 |
| D-10 | 모델 판정(`model_judgment`, 불변 기록)과 단언 관계(`relation`) 분리. `potential_conflict`는 저장 라벨이 아니라 파생 투영 `possible_conflict`. `contradiction`/`not_conflicting`은 사용자만 | Accepted | R3 C: 정확도 0.65–0.78, 불일치 53%, 진짜 모순 3건 다수결 미검출, DeepSeek 번역 경유 치명 오류 3건; "any tension" recall 0.89 / precision 0.66 — 그러나 **허용 라벨 집합 기준**이므로 엄격 정밀도 아님 | R2 8라벨 relation 테이블, R3 `potential_conflict` 저장 행, 자동 contradiction | 관련·시간 변화 진술에 가능 충돌 플래그 과다 → 에이전트 노이즈 | IMPLEMENTATION_READINESS C-frontier 결정 규칙(정밀도 ≥0.95, 예측 모순 ≥20, 치명 0, 재현 ≥0.9) 충족 시에만 자동 *제안*을 늘림. 자동 단언은 여전히 금지 |
| D-11 | 관계 어휘: `relation.kind` ∈ {supersedes, exception, backfill, contradiction, not_conflicting}. R2 `potential`, R3 `potential_conflict`, `temporal_change`, `related`, `insufficient`는 `model_judgment.raw_label` 힌트로만 | Accepted | R2/R3 어휘 불일치, R3 R8(판정기의 temporal_change는 대체 신호 아님) | 어휘 통합 단일 테이블 | — | — |
| D-12 | 모델 출력은 제안. 검증 경계 통과 전 저장 안 함, 권위 기록 수정 불가, provider trait로 교체 가능 | Accepted | R1 A, R2 D, R3 C | 모델이 상태/관계 직접 기록 | — | — |
| D-13 | 추출 결과는 원문(작업) 단위 전부-또는-전무 | Accepted | 부분 저장 시 결합된 틀(귀속+부정)의 일부만 남는 위험(R2 D) | Claim 단위 부분 수락 | 긴 원문 하나의 오류로 전체 재시도 | 실패율이 높으면 앵커 구간 단위 분할 작업 |
| D-14 | 버전 교체(`revision_of`), 시간 대체(`supersedes`), 모순을 다른 개념으로 | Accepted | R1 원칙 1, R2 C | Claim UPDATE, `superseded` 상태 하나로 겸용(R1) | — | — |
| D-15 | 유효성 3값(valid/needs_revalidation/invalid), 물질화 + 철회 tx 내 증분, 뷰 재계산은 오라클 | Accepted | R2 E: 하향 폐포 0.2 ms, 물질화로 197→77 ms | 거짓 표시, 비동기 갱신만 | — | — |
| D-16 | 의존 종류 premise/restates/alternative/informed_by + `deps_unknown`(파생). 지지 = 원천 합집합 | Accepted | R1 B1/B2(결합 전제 반례), R2 A8/A9 | 원천 수/문서 수 | alternative는 실험 미검증(R1 E2는 결합만) | Phase 4 테스트에서 표현 불가 사례 발견 시 |
| D-17 | 정체성: 불변 mention + 얇은 entity + append-only `identity_assertion`, 해소는 파생. 수동 교정은 권위 단언이라 재구축에 안전 | Accepted | R1 C4(파괴적 병합 복원 불가), R3 R9; IMPLEMENTATION_READINESS의 "entity_alias rebuildable"과 "manual correction 보존"이 한 테이블에서 충돌 → 분리로 해소 | alias 테이블을 권위+파생 겸용 | 수백 엔티티 규모 미검증 | Phase 4 규모 테스트 |
| D-18 | 시간: authored/observed/captured/applies 분리, 명시된 범위만, 오래됨 ≠ 거짓 | Accepted | R2 C(상시 구간 추출 시 9/22↔19/22, 최대 6건 소실), FM 덮어쓰기 기본(과거 질의 2/10) | 유효 구간 강제, 최신만 반환 | — | — |
| D-19 | 신선도 임계 60일/14일 | Provisional | R2 C 두 셋 9/10–11/12, 미조정 | — | — | 실데이터 |
| D-20 | 의미 검색 = 원문 언어 Claim에 다국어 임베딩 1종(bge-m3 기준선), lexical fallback 필수 | Accepted(인터페이스) / Provisional(모델) | R3 B: bge-m3 R@5 47/48, 교차언어 1.00/0.92 — **캐시 임베딩, 194개, e2e 지연 미측정** | te3-small, 영어 정규화 | 소규모 합성 | Phase 6 측정 하네스(≥300 항목, 패러프레이즈)에서 R@5 <0.9 |
| D-21 | lexical = FTS5 unicode61 over 문자 bigram + ASCII 단어 | Accepted | R3 B 동일언어 1.00, 조사 제거 0.78, trigram 0.56 | 형태소 분석, trigram | — | — |
| D-22 | 결합(fusion)은 게이트형, plain RRF 금지. 게이트 임계값·후보 K·constraint-first 구성은 **설정값**이지 계약 아님 | Provisional | R3 B: RRF en>ko 0.08; A: constraint-first R@8 0.649 vs 임베딩 0.622(차이 1 target, 유의하지 않음) | plain RRF, 고정 K=8 계약 | 게이트 미보정 | Phase 6/7 보정 |
| D-23 | 함의 확장 검색 없음, 기본 번역 없음(번역 행은 off 기본 파생) | Accepted | R3 A(1:1), B(번역 16% 결함) | — | — | R3 RETRIEVAL_DECISIONS R5/R6 조건 |
| D-24 | DeepSeek V4.1 Flash 1017 사용 시 provider 어댑터가 영어 전용 입력을 강제(한글/가나/CJK 포함 시 호출 거부) | Accepted | R3 운영 제약(118 호출, 위반 0), 번역 경유 판정 치명 오류 | — | 판정기로는 비권장 | — |
| D-25 | 마이그레이션: 전진 전용 번호 마이그레이션 + 적용 전 자동 온라인 백업. down 마이그레이션은 데이터 손실 없는 경우만 | Accepted | 실용성, FM v7 손상 → v8 수리 경험 | 모든 단계 down 필수 | — | — |
| D-26 | 멱등 키 범위 = (recorder, idempotency_key), 같은 키 다른 내용 = 오류. 내용 해시 자동 중복 제거 없음 | Accepted | R1 E5, 같은 노트 재작성은 정당한 두 기록 | content-hash dedup | — | — |
| D-27 | 엔티티 자동 병합 안 함, 모델 same_as는 저장하되 해소 미반영(기본) | Accepted | R1 C1/C2 | 표면형 자동 병합 | 재현율 비용 | — |
| D-28 | 물리 삭제(purge) 계약 밖, `forget`은 숨김 + 보존 | Provisional | — | — | 사용자의 삭제 기대와 불일치 가능 | 사용자가 영구 삭제를 요구할 때 별도 결정(백업에서의 삭제 포함) |
| D-29 | Phase 순서: 2(권한)를 3(추출)보다 먼저 | Accepted | Claim은 source의 recorder/권한을 상속 — 권한 없이 만든 Claim은 재분류 필요 | 추출 우선 | — | — |

## Phase 1 implementation clarifications

- **D-30 — Accepted: lifecycle administration vs read projection.** `get_source(user)` remains user-recorded ∧ live. User lifecycle commands may address any existing source, including forgotten and agent/import sources. Otherwise §4's user-all-transitions and user-only forgotten restore are impossible under PHASE1_SPEC §5's literal read-visibility gate. Non-user lifecycle commands retain the visibility gate (`NotFound` before permission checks). This is a command authorization distinction, not a broader read projection.
- Request digest encoding uses a big-endian u64 byte length per field, with `u64::MAX` for NULL (distinct from an empty string). Timestamp metadata is normalized to UTC milliseconds before hashing. Content bytes are untouched, including embedded NUL; the SQL nonempty check measures BLOB byte length.

## Phase 2 implementation resolutions

- **D-31 — Accepted: independently authenticated credentials, existing role scope.** Each agent receives an independent random credential, validated into a private Principal. This does not invent per-agent source ownership: DATA_CONTRACTS P-4 forbids using labels/session IDs as authorization and PHASE1_SPEC explicitly defines agent scope as agent-recorded sources. Thus agents share that role's view/lifecycle authority and `(recorder, idempotency_key)` namespace (D-26). User remains a role without a user ID. Credentials are independently revocable by removing their registry entries. Credential identity binds cursors internally, never becomes a user ID or client-provided authority. Per-agent ownership would require a separate future data contract, including legacy ownership and key migration.
- **D-32 — Accepted: one DB owner, MCP as authenticated loopback HTTP client.** `serve` owns App and one Tokio runtime; stdio `mcp` opens no DB and forwards tools to the same HTTP dispatcher. HTTP binds only loopback in Phase 2. No redirects, proxies, cookies, or remote hosts in the MCP client. Local 0600 token files carry 256-bit OS-random bearer tokens; the server's 0600 registry contains SHA-256 token digests and operator-assigned roles/labels. Files must be regular, owned by the process user, without symlinks or hard links; permissions are checked on open and on each authentication. Registry parsing/authentication failures are generic and never contain secrets. Unix permission semantics are required; unsupported platforms fail closed.
- **D-33 — Accepted: Source read defaults and pagination.** Phase 2 `get`/`list` exclude retracted sources by default for agent/import, with explicit `include_retracted`. User reads always require user-recorded AND live. Core lifecycle authorization retains D-30's administrative distinction, including user access to all lifecycle targets. Role-scoped record receipts/idempotency remain D-26; they are write receipts, not projection reads. Public conflict errors omit source IDs. Lists filter in SQL before counting/limiting; keyset cursors contain only a last visible ULID and filter mode, authenticated with a process-random HMAC key and bound to the authenticated credential. They expire on server restart; no global offset/event sequence/count is encoded. Each page is transactionally consistent, but pagination is a live view, not a cross-request snapshot: lifecycle changes are rechecked and concurrent insertions before the cursor are not revisited.
- **D-34 — Accepted: explicit offline maintenance boundary.** Phase 1 direct CLI operations and `Principal::for_local_cli` remain trusted local maintenance/test entry points, requiring direct filesystem/DB access and exclusive DB ownership. They are never selected by HTTP/MCP parameters. Missing network credentials never fall back to local CLI identity. Unknown operation fields (including recorder, speaker, basis, principal, confirmation flags) are rejected rather than accepted as authority. Source-only APIs expose no Claim confirmation, aliases, summaries, edges, or dependency aggregates.

## Phase 3 implementation resolutions (provisional until real-model acceptance)

- **D-35 — Accepted: lossless status-event expansion.** SQLite cannot extend the v1 CHECK/FK target constraint in place. Migration 0002 recreates status_event transactionally, copies every existing row with its original seq, and reinstalls append-only/authorization triggers. Pre-migration backup and row-preservation tests are mandatory. No source or historical event is discarded. Claims, mentions, declarations, and extraction provenance are new immutable tables; job remains operational.
- **D-36 — Accepted: batch versions and scalar revisions.** `revision_of` cannot alone represent one-to-many splitting or a successful empty replacement. An immutable extraction_run identifies each accepted job, its prior accepted job, prompt/provider/reviewer identities, and output hash. Current expression uses the latest accepted run for a source; older Claims and confirmations remain available in explicit history, never retracted automatically. Candidate revision_of may name only a Claim in the captured prior run of the same source; stale jobs cannot publish over a newer run. A new empty version is allowed only with an explicit no-propositions reason and coverage review.
- **D-37 — Provisional: semantic gate, not semantic proof.** Production candidate strategy is decomposition/frame reconstruction plus independent atomicity, fidelity and coverage calls against the ORIGINAL source. It is a research candidate, not an empirically selected winner until live evaluation passes. Exact JSON/anchors/UTF-8, schema, duplicates and conservative Korean/English script guards are deterministic; semantic reviewers are fallible. Without an accepted model-quality benchmark, jobs quarantine their proposed batch for explicit local user review before authoritative Claims are committed. Review approval is bound to the exact candidate hash, job and source; it does not imply user authorship or Claim endorsement. No model can set approval, confirm, recorder or visibility. Rejecting one candidate rejects the batch.
- **D-38 — Accepted: declarations before Phase 4.** Phase 3 stores only same-batch dependency declarations and rejects cycles. No validity propagation engine is introduced. A batch with declarations is conservatively excluded from user projection pending Phase 4 validity validation, even if confirmed; agent view marks validity `needs_revalidation`. This avoids presenting unvalidated dependencies as valid. No source anchor or model claim can override source/Claim lifecycle. Source and Claim endorsements are separate from lifecycle.
- **D-39 — Accepted: exact evidence and conservative basis.** Models return literal source substrings plus occurrence numbers, not trusted offsets. The host computes UTF-8 byte ranges. Every candidate requires evidence; mentions reference exact Claim substrings. `verbatim` additionally requires Claim text equal to its anchor, preventing a transformed paraphrase from acquiring user visibility by self-report. Semantic/atomicity review still applies to literal copies; exact copying is not proof of independent meaning. Other categories of transformed Claims require explicit user confirmation for user projection. Korean/English script checks are necessary but insufficient language checks; semantic review must verify anchor-span language, technical identifiers, unknown references and temporal scope. Unknown languages or script ambiguity are quarantined, not silently translated.

### D-40 — Claim projection metadata must not widen endorsement
A confirmed Claim may use a long anchor containing unconfirmed adjacent text. Returning that
raw passage would widen user endorsement and leak agent material. The user projection therefore
omits source-relative anchor text/offsets, free-text `speaker_label`, and `index_fields`; essential
meaning must already be in Claim text. The agent/audit view retains them. Counts and cursors are
computed only from projected Claims; Claim cursors have a separate signed domain. Alternative:
returning the whole anchor was rejected by inspection of a confirmed paraphrase with a broad
source span. This changes presentation only, not persisted evidence or endorsement semantics.

### D-41 — Supported production language envelope pending evaluation
This implementation initially accepts `ko` and `en` anchors, including mixed documents whose
individual anchors follow either language. Other BCP-47 languages remain unsupported and must
not be relabeled English. Script checks are necessary but cannot establish language or semantic
fidelity; model review and explicit local review remain required. Extending the language envelope
requires a forward migration and measured fixtures. No claim of universal language validation is made.

### D-42 — Re-extraction cannot resurrect hidden historical Claims (retraction rule amended by D-50)
A fresh interpretation can restate a forgotten/retracted proposition with a new ID, even
if the model omits `revision_of`. Until an independently validated semantic mapping can
establish which propositions inherit lifecycle restrictions, publication is blocked when
any Claim of that source is currently retracted or forgotten. The original Source and
candidate remain recoverable. A user must resolve that lifecycle deliberately before
publication. Automatically accepting unrelated-looking new IDs was rejected because it
would make forget/retract ineffective; automatically copying endorsement onto rewritten
text was rejected because approval of extraction is not endorsement of truth. Prior
same-source Claim IDs/text are supplied as untrusted revision context, never as evidence.
This is a conservative availability restriction, not Phase 4 validity propagation.


### D-43 — Reproducible prompt revisions preserve frozen v1 requests
Live development experiments exposed unsupported Korean counterfactual outcomes,
causal relationship loss, correction duplication, and reviewer false rejection of
scope-preserving explanatory text. Prompt v2 adds contrastive reconstruction examples
and reviewer calibration; it is experimental until held-out validation, not a declared
winner. V1 assets remain unchanged. Plans may name an explicit `prompt_version`; an
omitted field means v1 and stays omitted on serialization, preserving existing request
JSON and provenance hashes (checked against an actual v1 live-run hash). The version
selects both extraction and review instructions. Unknown versions fail closed. Stored
extractor identifiers use the frozen plan's version. No migration, authority change,
or implicit upgrade of pending jobs is introduced. Editing v1 in place was rejected
because it would prevent reproducible replays and could strand frozen requests.


### D-44 — Output budgets and incomplete provider responses
Live OpenRouter requests without an explicit output limit reserved up to 65,536 tokens
and eventually returned HTTP 402, even though short requests still worked. ModelConfig
now permits an explicit output budget included in frozen plan serialization/hash;
omitted values preserve old request hashes and behavior. The cheap-model experiments
use 16,384 extraction tokens and 4,096 review tokens. Exceeding a budget is a failure,
never permission for partial persistence: non-stop completion reasons, including length
truncation, are rejected even when their text happens to parse as JSON. Provider error
enums distinguish quota, invalid requests and rate limits without retaining response
bodies. No automatic credit purchase, model substitution or precision reduction occurs.
Invalid local model configuration is rejected before leasing work.

### D-45 — Editorial corpus corrections and inspectable intermediate experiments
The new corpus annotations are draft semantic inventories, not independently adjudicated
gold. Native Korean review identified two annotation problems: a bare '배포 ㄴㄴ' does
not establish command versus status speech act, and an embedded instruction containing
two independent requested actions needs two reported propositions. These development
references were corrected with original source bytes unchanged. A disagreement relation
may retain its presupposed attributed opinion in one proposition; a separately asserted
opinion and a separately asserted disagreement remain distinct. Long-form editorial
cases extend the corpus to 104 sources (84 development, 20 held-out).

V3 adds contrastive examples for omitted subjects, embedded-instruction coverage,
shorthand ambiguity, and date-only values in instant fields. V1/V2 prompts remain intact.
The evaluation-only `english-staged` strategy saves an English draft and its prompt hash,
then reconstructs using the original source plus that explicitly untrusted draft, with
three final original-source review stages. Its experiment hash includes the intermediate
prompt; its production-plan hash is recorded separately. This research strategy does
not silently change the production worker or make translation authoritative. Its first
request format failure (missing literal JSON in JSON-mode instructions) is preserved as
a rejected experiment. Neither an internal-English instruction nor round-trip agreement
establishes that a translation was faithful.


### D-46 — Isolation and Korean-matrix checks follow observed v3/v4 failures
A source audit of all 64 v3 curated development batches found prospective gate-eligible
Claims with unresolved first person, lost project scope, and disconnected shorthand
rationale. V4 adds contrastive isolation examples and matching review checks. This did
not establish success: v4 also produced English Claims from a Korean technical clause
(`PrimeRadiant의 record_source는 sync이고 ...`), approved by all three model reviews.
That failed output is retained in evidence/language-drift-v4.json.

The previous necessary script checks accepted Latin-heavy Korean as English because
both anchor and text contained Latin letters. Source checks v2 additionally reject an
English label where the anchor has multiple recognizable Korean grammatical endings
attached to Latin identifiers, including a predicate/conjunction. This is a narrow,
conservative check, not a general Korean parser or a character-count language classifier.
English clauses containing Korean proper names remain supported and regression-tested.
Ambiguous code-switching and anchors lacking such evidence still require source-grounded
review and local fidelity approval. The existing original-anchor language contract is
unchanged; no blanket ban on Korean names in English was introduced.

The evaluation records its source-check version on new rows. Earlier raw results retain
their original verdicts; retrospective revalidation must be reported separately rather
than rewriting a historical model or validator outcome.

### D-47 — Real-model bounded repair is measured separately from independent runs
The manual evaluation harness can perform up to three attempts for each independent run.
It uses the same source payload and sanitized previous-failure code as the worker, and
keeps every failed full batch with attempt number and terminal status. A later success
does not erase earlier failures. Experimental review-stage ablations remain diagnostic;
production always requires all three stages. The harness does not simulate database
leasing or wall-clock backoff; deterministic crash/lease tests cover those operations.
Date-only values supplied to instant fields are schema failures, not provider outages.


### D-48 — Authored instant does not prove an authored calendar timezone
PHASE1_SPEC explicitly normalizes timestamp metadata to UTC before hashing/storage.
The same stored instant 2026-10-22T18:30:00Z can originate as October 22 in UTC or
October 23 at +09:00. The identical Korean source '어제 서울 사무실에 갔다' could then
refer to October 21 or October 22. Neither language nor the normalized Z suffix can
recover the writer's calendar intent. This is an information-loss counterexample,
not an extraction-model confidence question.

Preserve all existing timestamp storage/digest contracts. V5 keeps a relative period
explicitly bound to the known original instant with an unknown calendar timezone unless
the source supplies a legitimate calendar reference. Explicit source UTC/KST references
allow resolution; no authored_at means an unknown original reference day. This applies
the existing 'resolve only when supported' rule; it does not substitute captured_at or
add a guessed timezone, day boundary, lifetime or user ID. Adding an optional timezone
field was considered but is unnecessary to represent uncertainty safely and would require
new recording/migration contracts. Four development controls demonstrate the distinction.
The corpus now contains 108 sources: 88 development, 20 held-out.


### D-49 — Failed proposals remain private diagnostics and cannot borrow approval
The live research showed that a failure code alone cannot explain a stochastic semantic
failure. The worker now retains bounded failed proposal text and received review responses
in the existing immutable, untrusted candidate store. This creates no Claims or extraction
run and never marks a failed batch review_required. Source/job completion remains atomic
at the authoritative publication boundary. Oversized diagnostics and stale/hidden-source
results are not retained. Provider credentials, headers and HTTP error bodies remain excluded.

Review records include attempt, previous failure code and source-check version; legacy
review arrays remain readable. The local inspection surface exposes these records only
to its existing trusted user-maintenance role; no HTTP/MCP tool, projection, count or cursor
exposes them. Approval now validates the selected candidate's own three successful reviews,
current source checks and frozen prompt hash, in addition to the exact output hash and
lifecycle/version fences. A rejected candidate cannot borrow another attempt's successful
job state. The cross-attempt regression verifies this with two structurally valid proposals.
This stores diagnostic proposals, not partially accepted knowledge; D-13's all-or-nothing
Claim transaction is unchanged.


### D-50 — Correcting an extraction must not require restoring a known wrong Claim
DATA_CONTRACTS §5.2 says a better interpretation replaces the current representation,
while an earlier wrong interpretation is separately retracted. D-42's blanket block on
any retracted historical Claim made that workflow impossible: a regression with a Friday
Source, an erroneous Thursday Claim and a retracted old interpretation failed with
`extraction/lifecycle` before the correction could run. The before/after evidence is
retained in the Phase 3 report artifacts.

The barrier now distinguishes retraction from forgetting. A live Source may be explicitly
re-extracted through the existing user-only request and exact-hash, user-only publication
flow while its old wrong interpretation stays retracted. The new revision still requires
its own source checks and three successful reviews. `revision_of` is a version link, not
a premise that inherits old invalidity; no old lifecycle or confirmation is restored.
Prior context labels the old lifecycle event and remains untrusted. Source retraction
continues to block all publication. A forgotten historical Claim still blocks before
provider invocation and at publication, and its revision ID is redacted from visible
metadata. The regression verifies correction without revival, unchanged Source bytes,
forgotten-record gating before any provider call, and revision-ID redaction.

Requiring users to revive a known wrong interpretation was rejected because it contradicts
§5.2. Removing the forgotten barrier was rejected because it could expose forgotten
knowledge under new IDs. No agent receives re-extraction/publication authority and no
Phase 4 dependency-validity propagation is introduced. Public Claim lifecycle fields now
use the existing live/retracted/forgotten enum, consistently with Source responses;
append-only status events retain retract/forget/restore action labels unchanged.

### D-51 — An evidential qualifier must survive retrieval of its report alone
V6's held-out `new-042` source reports a possible server outage and explicitly states
that its speaker had not checked. A three-review-passing output split that into a
possibility report and a separate checking-status Claim. The draft reference inventory
already required a single unverified report. Although the full set contains both phrases,
retrieving only the report drops an explicitly communicated evidential limitation. This
is a source-grounded failure of self-contained framing, not a schema or anchor failure.

V7 explicitly tests qualifier attachment by target and discourse relation before splitting,
with Korean and English contrastive examples. The counterexample separates a hypothesis
about one device from a testing action about a different device; unrelated limitations
must not be invented. Four new development controls compare v6/v7 with the same cheap
extractor/reviewer. The original v6 run and held-out status remain recorded; after seeing
this failure, those exposed cases are regression material for v7, not a fresh held-out
benchmark. No automatic acceptance or new completion claim follows from the revision.
Historical prompt assets and frozen request hashes remain unchanged; no data migration or
authorship/visibility decision changes. This applies the existing self-contained Claim
contract rather than requiring every epistemic qualifier to become an independent Claim.

### D-52 — Scope binding precedes decomposition in the next prompt experiment
V6's three-review-passing `long-007` held-out batch emitted a broad conditional requeue
Claim and placed its completed-work exception in another Claim. It also reported an
opinion about 'this policy' without resolving the policy described by the Source. Neither
neighboring text nor valid anchors repairs retrieval of those Claims alone.

V8 reorganizes the extraction prompt around proposition inventory, binding governing
frames, decomposition, isolated reconstruction and a reverse source audit. This is an
instructional procedure within the existing call, not an observable intermediate output
or proof that the model followed it. Contrastive Korean/English development controls
separate an exception's governed action from an unrelated consequence, and resolve a
source-defined policy as an opinion's object. It retains logical-negation, causal,
quotation, epistemic, language, pragmatic, temporal and injection contrasts. Review prompts
apply the same binding tests independently against the original Source.

V7 remains an immutable, narrower epistemic-attachment experiment; its paired control
runs repaired four same-target framing losses while preserving four unrelated-target
splits. That small development result does not establish generalization. V8 is tested
against matched v6 controls and exposed held-out regressions. Exposed v6 held-out material
cannot count as a fresh held-out benchmark for either revision. No schema, publication,
authorization or Phase 4 semantics change is justified by this prompt experiment.

### D-53 — An explicit negative exception is not merely a positive condition
The v8 scope control generated a rule requiring restart only in the positive non-maintenance
case, without preserving the source's explicit prohibition in maintenance mode. Haiku and
Luna Pro each passed this deliberately weakened control in all six reviews; Qwen Flash
also passed five times and returned one invalid completion. Agreement did not establish
logical equivalence. A concrete behavioral counterexample (trigger=true, exception=true,
action=true) satisfies the positive conditional alone but violates the explicit negative
exception; the deterministic calculation is retained as evidence.

V9 adds an IF-versus-ONLY-IF counterexample audit and explicit binding of deictic time to
the original statement. Matched reviewer controls then rejected all 24 deliberately bad
instances for each Haiku/Pro profile, while false rejections of valid controls remained
(5/24 Haiku, 4/24 Pro). These are narrow editorial controls, not population error rates.
The temporal controls are also editorial judgments about the self-contained-time contract;
independent review remains important. No purely syntactic exception detector or universal
natural-language theorem prover is claimed. Historical prompts/results remain unchanged.

### D-54 — Structural review index hints do not authorize acceptance
Both v8 long-form regression runs produced 11 candidates but received reviews listing
only indices 0..9. The strict checker correctly rejected these batches. Supplying the
correct count is preferable to weakening coverage validation or silently filling a model's
missing indices. V10 uses v9's semantic procedure plus host-computed candidate_count and
required_checked_indices in the review input, and a versioned instruction to review each
index. These are structural hints, never evidence of fidelity or authorship. Every model
response must still explicitly include every index exactly once and pass all other gates.

Core and evaluation harness share the same review-message function. V1–V9 retain their
previous payload shape and frozen hashes; only the new v10 profile adds hints. A regression
inside the existing prompt/protocol test verifies the distinction and preserves the v1
hash. No data schema, Claim visibility or publication authority changes.

Reviewer max-output budgets remain explicit configuration. A focused complex-source
budget comparison returned stop for all six reviews at 4,096 and all six at 8,192 tokens;
it does not prove that a larger budget fixes prior invalid completions. The next profile
uses the larger bound as headroom, with the actual bound frozen in provenance. Evaluation
failure codes now match production's sanitized quota/request/rate-limit/schema classes;
older saved rows retain the feedback codes actually used. Summary diagnostics separately
include reviewer call failures rather than counting only extractor failures.

### D-55 — User Claim projection cannot expose unrelated temporal metadata
The existing transport regression was extended with an agent Source containing an
unconfirmed separate schedule and a Claim about local server execution. A simulated
untrusted proposal copied that schedule into applies_from/applies_to. After explicit
confirmation of the server Claim, those free-form date fields were exposed in user
responses despite the schedule being absent from the confirmed text. The red/green
HTTP/MCP regression demonstrates this direct metadata channel; semantic model reviews
must not be an authorization boundary.

The user Claim projection now returns null for applies_from/applies_to, like the already
redacted broad anchors and free metadata in D-40. Every essential temporal qualifier must
already be in Claim text. Agent/audit views retain the values. This changes presentation,
not persisted dates, lifecycle, endorsements or authorship. It does not assert constant-time
or arbitrary covert-channel noninterference.

The same regression separately reproduced a storage-format defect: valid +09:00 Claim
metadata was persisted verbatim rather than in DATA_CONTRACTS §0's UTC-millisecond format.
Publication now normalizes new Claim date metadata through the existing Timestamp type.
Raw proposals/hashes, original text/anchors, historical immutable rows and Source timestamps
remain unchanged. There is no migration or destructive rewrite. The original Phase 1 DDL
and Phase 1/2 checkpoints are unchanged. The test is added to the existing transport test,
not a new collection of compile-time tests.

### D-56 — Measure isolation without Source or neighboring-Claim context
Natural v10/reference controls showed that all three source-aware reviewers could accept
a quoted unresolved reference by filling it from the Source or neighboring Claim. A
research-only standalone reader saw just one Claim text, with no Source, neighbors,
anchors, speaker metadata or original timestamps. On 22 editorial calibration controls
twice, each of Haiku, Luna Pro and Qwen returned 24 true negatives and 20 true positives.
Some controls overlap prompt examples, so those counts are diagnostic, not generalization.

A separately authored 16-control reader held-out set, twice, kept the reader prompt frozen.
Each model rejected all 16 deliberately bad instances. Haiku falsely rejected four of 16
valid instances; Luna Pro and Qwen each falsely rejected one. The labels are editorial,
not independent human gold. The quoted-pronoun interpretation has also been submitted to
the user for independent Korean review. Focused instructions and physical input isolation
changed together; these experiments do not isolate their individual causal effects.

V11 adds this measured protocol as a new opt-in production profile: per-Claim standalone
atomicity/isolation reviews, then the original three source-grounded reviews, then unchanged
exact-hash local fidelity approval. Reader calls run at bounded concurrency four and
never see another Claim or metadata. Reader coverage_complete refers to that one provided
text; original-source coverage is still checked by the three later reviews. This does
not prove source fidelity, statistical independence or universal semantic correctness.
The same configured reviewer model is used; no provider-specific domain logic is added.

Stored review arrays contain N standalone responses followed by three source-grounded
responses. Quarantine and publication both require the complete protocol; each standalone
response must explicitly cover its one index. Total review bytes are bounded for the new
profile. Reader prompt bytes are included in the frozen plan hash. V1–V10 request hashes,
review counts and payload shapes remain unchanged. Zero-Claim batches still require all
three source-grounded reviews. Failed reader checks retain only private diagnostics and
publish no Claims. New fake-provider assertions inspect the actual request payloads,
verify physical context omission, reject missing reader evidence and verify failed-batch
rollback without adding another top-level test. Real v11 extraction still requires
separate empirical evaluation; this decision does not declare Phase 3 acceptance.

### D-57 — Distinguish permission-grant status from epistemic verification
The draft `new-047` reference expected one merged Claim for “접근해도 된다고 들었지만 직접
허가를 받은 건 아니다.” Applying the independence test gives two source-supported
propositions: the original speaker heard a report allowing access; the original speaker
has not received direct permission for that access. Either can vary independently. The
hearsay Claim must remain reported hearing, not an asserted permission; the status Claim
must name the access target. Merely saying “permission” without its known access scope
is not self-contained.

Unlike a source's unverified hypothesis, receipt of direct permission is a separate
normative act/status, not verification that a hearing occurred or that its reported
content is objectively true. Treating every denial of authority as an epistemic qualifier
would merge independent facts and undermine strict atomicity. The initial overmerged
editorial inventory is corrected; historical run snapshots and Source bytes are retained.
One reported, explicitly unverified possibility remains a complete qualified proposition
under the user's positive example. No production code, per-case rule or authorization
change follows from this annotation resolution. Models and hearsay never grant actual
application permissions; only authenticated Principal and core policy do so.

### D-58 — Human adjudication of unknown referents applies to every Claim
On 2026-10-23 the user reviewed the actual short-form annoyance Claim about “걔” and
required the explicit unknown referent in that Claim itself. Quoting the original
pronoun is insufficient; the same referent description must appear in each dependent
Claim's text. Source-aware reviewers accepting the short form were filling context
that isolated readers do not have. This confirms the existing contract and v11
prompt rule; it does not change Source bytes, historical hashes or authorization.
The user's additional live-evaluation budget is capped at $5. The manual evaluator
uses a shared, locked conservative request-reservation ledger for the
priced cheap model; local budget rejection stops evaluation like quota exhaustion.
Production provider/job semantics are unchanged.

### D-59 — Versioned same-job repair diagnostics, never reused approval
Funded v11 retries repeated standalone-reference rejections (for example unbound
“이 메모”) while the next extractor saw only `atomicity`. A coarse failure class is
insufficient to identify the failed candidate or the review's actual issue. Opt-in
v12 supplies a bounded, explicitly untrusted previous candidate plus stage-labelled
review diagnostics. The worker selects only the immediately preceding attempt of
the same job; the evaluator follows the same input contract. The original Source
remains the sole semantic evidence. Reviewer diagnostics can be wrong and must be
checked against that Source, not obeyed as instructions. Every repaired batch receives
all new reader and source-grounded reviews, and still requires exact-hash local user
approval. No review result is inherited as publication authority. Older frozen
profiles retain their prior request shape and hashes. No migration is needed.

V12 also tests reverse scope substitution: compare each isolated Claim's actor,
affected object, system/project, reporting holder and time frame with the Source.
Earlier gate-passing output lost a deployment target on a rollback event, and a
funded calendar output resolved an omitted event actor to the original speaker
without explicit supporting context. The revision makes these comparisons explicit,
with a new analogous example rather than a per-fixture rule. These are empirical
quality changes, not authorship inference: the recorder never identifies an actor.

Evaluation accounting follow-up: the initial no-refund request guard reached its
conservative ceiling at $0.399282 account usage increase. Successful completions
may now settle reserved amounts to a conservative bound using provider-reported
token counts. Failed/unmetered calls retain their full reservation. Locked in-flight
reservations remain charged, preventing concurrent shards from exceeding the shared
ceiling. Token usage remains provider-layer operational data and never enters Claim
semantics or the HTTP/MCP response contract.

### D-60 — Distinguish the Claim's original-source root from missing referents
The v12 pilot repeatedly rejected the standalone proposition that the original memo's
dates use UTC/Korean Standard Time, even after the repair replaced “이 메모” with an
explicit original-memo designation. The original Source and its narrator can be
identified through that intrinsic provenance frame; understanding this proposition
does not require a globally unique document title. This differs from a person quoted
as 걔 or a policy defined elsewhere, whose essential identity/content remains absent.
V13 changes only the standalone-reader prompt, retaining v12 extractor/source-review
prompts and same-job repair protocol. It accepts a bound original-source designation
while still rejecting unbound “this memo”, bare quoted pronouns and missing policy
content. In 26 editorial calibration controls repeated twice, the cheap selected
reviewer returned 28 correct passes and 24 correct rejections. Several examples overlap
instructions; these are calibration results, not proof of general semantic accuracy.
The previous reader prompts and experiment outputs remain immutable. Further pipeline
and separate reader regression evaluation are required before acceptance.

### D-61 — Assess the scope of a composite object before splitting its description
The long mixed-language pilot exposed standalone reviewers rejecting an opinion about
one fully described policy because the policy's definition contains multiple actions.
“One person considers [this composite policy] safe” is one assessment of one object;
its defining clauses are not separate assertions that the described events occurred,
nor separate safety judgments about each subrule. Splitting the opinion would invent
judgments about different objects and change its truth conditions. This differs from
“a person said A occurred and B occurred” or two evaluations of different targets,
which must split. V14 adds explicit positive/negative scope controls and reader
instructions, while retaining all strict output, anchor and authorship checks. It
also clarifies that an explicitly unidentified technical component can be a subject
when the Source gives no instance identity. Existing Source-provided names remain
mandatory. These changes require empirical evaluation; they do not declare acceptance.

### D-62 — Align evaluation repair inputs with application-core retry behavior
The manual evaluation harness collected every independent review, while the core
stopped at its first failed review. Passing the richer host envelopes to repair was
not equivalent to production. Normalize both representations to the same ordered
prefix through the first failed review. Existing core string inputs retain their
behavior; a regression assertion compares both payloads. New evaluation rows mark
`repair_input_protocol=core-prefix-v1`. Earlier repaired outputs remain evidence of
those experiments, not proof of identical production retry behavior.

### D-63 — Korean omitted subjects can identify the original speaker linguistically
For isolated “요즘 매일 아침 6시에 일어나는 중”, the user explicitly adjudicated
“Use the original speaker.” The previous blanket unknown-actor fallback weakened a
speaker-oriented habitual self-report. V15 resolves such omissions using Korean
pragmatics, checking named discourse subjects and quotation scope first. Explicit
third-person 걔 remains unidentified when unresolved (D-58). Linguistic narrator
resolution does not establish authenticated user authorship, recorder identity or
privileges. This prompt correction changes no stored Source, authorization rule or
schema. Paired controls cover self-report, named third party, quotation and unresolved
third person; empirical quality acceptance remains required.

### D-64 — Relative time inside speech belongs to its own utterance frame
The v15 quotation control “민수가 ‘요즘 매일 새벽에 달리는 중’이라고 했다”
produced two gate-passing Claims binding 요즘 to the outer original-source statement
time. The Source does not establish that this equals Min-su's quoted utterance time.
V16 requires explicit inner-speaker temporal binding, including nested reports. Outer
authored_at cannot silently date an inner utterance; an explicit date of that utterance
can. Direct speaker-oriented self-reports still use the original speaker's statement
frame. This clarifies existing temporal fidelity rather than changing timestamp schema
or allowing capture-time substitution. Prior v15 outputs remain archived as known
reviewer misses. The v15 reserved benchmark began before this discovery; any exposed
cases cannot subsequently serve as untouched held-out evidence for v16.

### D-65 — Correct editorial references when source grammar does not justify them
The quoted-time controls used bare “내일 출발해” inside speech. Their draft reference
assumed a first-person departure plan, but the Korean ending also permits a directive.
A v16 output correctly retained unresolved speech-act interpretation while resolving
내일 from the explicitly dated inner utterance. The reference now allows that ambiguity
and does not require invented speaker departure intent. Source bytes and model outputs
are unchanged; prior annotations remain in Git. This is a reference correction justified
by Korean grammar, not a relaxation of fidelity or an exact-output exception.

### D-66 — Calibrate bound source-time reading and preserve Korean hearsay layers
V16 exhausted one faithful habitual self-report after standalone “reference” rejections,
although its actor and relative time were explicitly bound to the original speaker's
statement. V17's reader distinguishes that internal binding from unframed first-person
or third-party references. Across 42 editorial controls repeated twice, it returned
all expected passes/rejections; these overlapping calibration controls are not held-out
semantic acceptance. Source-aware reviewers still check the actual temporal viewpoint.
The named-person -대 control also exposed a candidate reducing hearsay to a generic
narrator report. That batch failed another gate and remained unpublished. V17 explicitly
preserves the further hearsay layer without inventing an informant or adding hearsay
to plain assertions. This is a general morphology/frame rule, not a fixture-specific
exception. Source bytes, authoritative authorship and strict schema checks are unchanged.

### D-67 — Conservatively bind explicitly ongoing states to the source utterance
A v17 paired-control output copied “지수는 야간 근무 중이다” without a statement-time
frame and passed its model gates. Independent user judgment on whether the verbatim
form suffices is pending. Pending that judgment, v18 uses the conservative faithful
paraphrase “원문 발화 시점에 지수는 야간 근무 중이었다.” This preserves an explicitly
ongoing source state without implying it continues at retrieval time, inventing a date,
or substituting capture time. The rule applies to asserted progressive states, not a
substring ban or a requirement to date every generic property. Timeless conditionals
containing an ongoing-state condition do not assert that the condition actually holds.
Adding a frame makes the result paraphrase rather than verbatim; existing projection
and explicit-confirmation rules continue to apply without an authorship exception.

### D-68 — Compare semantic targets and attach same-target epistemic limitations
The v18 development run repeatedly combined the English commit-message requirement
and Korean-document permission despite their different target domains. Those batches
were rejected. More seriously, a gate-passing batch split uncertain anger attribution
from the speaker's ignorance of its reason. The latter limitation must remain in the
same independently retrievable assessment. V19 adds paired target-domain/true-subset
exception contrasts, same-target/different-target epistemic attachment contrasts, and
explicit unidentified-reference handling. Not knowing WHY is not equivalent to lacking
evidence or failing to verify WHETHER. No schema, authorization or lifecycle rule changes.

### D-69 — Preserve perspective in indirect versus direct reported speech
A rejected v18 nested-report batch attributed the narrator's unquoted 우리 방법 to
Reviewer 2 as though Reviewer 2 had used the quotation “our method.” This relocates
ownership. V20 preserves the original narrator's group in indirect speech, while a
first-person expression inside an actual direct quotation belongs to that quotation's
speaker. The same grammatical/discourse analysis governs temporal perspective; do not
blanket-shift every unquoted relative expression to the innermost reported speaker.
Unknown group members or method contents remain explicit without inventing identity.
This corrects extraction framing and does not identify a Principal or change authorship.

### D-70 — Separate batch-quality agreement from responsibility-aligned review metrics
An isolated reader cannot detect a qualifier omitted from an unseen original Source.
The scorer's historical reviewer_confusion table compared every reviewer with whole-
batch quality and therefore must not be interpreted as stage-specific diagnostic
accuracy. It remains labeled as that legacy association; new expected_reviews labels
permit responsibility-aligned confusion counts, leaving unlabeled decisions unscored.
The existing metric regression covers this distinction without adding a test method.
Two new synthetic expected-pass controls also contained unbound present-tense intent,
confounding their reference-only purpose. Their v19 outputs remain archived. New v20
focused controls bind the intent to the original utterance and use new IDs. This is a
control correction, not evidence that the prior temporal rejections were false positives.

### D-71 — Independent user adjudication of ongoing time and unidentified experiments
The user explicitly required “원문 발화 시점에 지수는 야간 근무 중이었다” for the
undated ongoing-state Source, confirming D-67 rather than the unframed verbatim form.
The user also required the experiment to be explicitly unidentified in the conditional
loss/architecture Source; merely quoting “이번 실험” is insufficient. These independent
judgments confirm the conservative source-time and unidentified-reference handling in
the current profile. V17's unframed state and v18's source-relative experiment label are
known gate-passing defects, not acceptable equivalent formulations. Neither correction
establishes user Principal identity or authorizes any model-proposed metadata.

### D-72 — Human adjudication supersedes the same-target unknown-reason merge
The user independently allowed TWO self-contained Claims for “지민이 나한테 화난 것
같은데 이유를 모르겠다”: the tentative anger assessment and ignorance of why the
apparent anger exists. Both retain uncertainty and assert no actual anger. This
supersedes D-68's requirement to merge the unknown reason into the assessment. Shared
target alone does not make independently variable mental states one proposition.
Ignorance of WHY differs from an evidential limitation on WHETHER; the explicit
unchecked-toner example still retains its verification limitation. V21 removes the
conflicting merge instructions from extractor, source reviewers and isolated reader.
The legacy editorial reference is corrected with the user's exact adjudication; source
bytes and historical prompts/outputs remain unchanged. The earlier v18 two-Claim output
is not counted as a known fidelity failure merely for that split. All other scope,
reference, temporal and authorship constraints remain intact.

### D-73 — Derive evaluation protocol features from the core frozen profile
The offline revalidator's duplicated version list stopped at v15 and would incorrectly
expect only three reviews for newer profiles. It now reads the core Plan's reader
feature and validates the saved plan/prompt hash. Repair-context support is likewise
shared by the core and harness rather than repeated growing version lists. Existing
profile behavior and prompt hashes are unchanged. Offline revalidation of 18 saved v18
rows verified all 18 plan hashes and retained the same 16 terminal gate-eligible runs.
This verifies protocol bookkeeping, not semantic acceptance. Existing tests were extended
without increasing Rust/Python test-method counts.

### D-74 — Preserve assertion layers when splitting verification limitations
The user independently requires separate nodes for the held-out Lena/Omar report and
the narrator's statement that Omar had not checked for a leak, explicitly retaining
an unidentified leak target. V21 and the editorial reference moved the narrator's
verification assertion into Lena's report. This is a confirmed framing failure despite
model agreement. Assertion-layer ownership takes precedence over mechanical attachment
of every verification limitation. V22 distinguishes outside-narrator assertions from
limitations inside reported speech, preserving both without inventing a causal or
additional temporal “since” relationship absent from the original. The user's example
wording is treated as a splitting/unknown-target adjudication, not new source evidence.
The direct unchecked-toner example and D72 independent-cause rule remain intact.
V21 prompts, held-out references and outputs stay frozen; subsequent v22 tests of these
now-exposed cases are regression tests, not held-out acceptance. No storage/schema/auth
contract changes; no default prompt promotion. V22 also clarifies that an unidentified
alternative in a disjunction and an explicitly unverified question are governing frames,
responding to reproducible isolated-review false rejections. These changes require fresh
real-model evaluation and cannot retroactively turn v21 failures into passes.

### D-75 — An event noun does not fill its missing semantic argument
V22 split Lena/Omar's assertion layers correctly but an accepted regression output
still omitted the explicitly unidentified leak target required by the user's D74
adjudication. V23 adds argument completeness to all three semantic prompt roles:
leak/damage/failure event names alone do not identify their affected target; each Claim
must explicitly preserve an unknown target without inventing equipment or a location.
This general rule differs from demanding proper names for every supplied common noun
(e.g. battery). The prior unknown-reference invariant is unchanged. V22 evidence and
prompts stay frozen and the failed acceptance target is recorded, not retroactively
relabeled as a success. V23 is opt-in and unpromoted pending actual evaluation.


### D-76 — One persistent production budget boundary, including diagnostic tools
The 2026-10-23 continuation found that production OpenRouter lacked the experiment
harness's accounting, while the Rust/Python harnesses used hardcoded estimates and
optional separate ledgers. All current paid entry points now use the Rust adapter's
mandatory SQLite budget. Provisioning is explicit, requires prior conservative spend,
refuses overwrite, and caps configured ceilings at $4.50. No key limits are raised.
The adapter requires an existing non-resetting provider key cap at or below that
ceiling, refreshes endpoint prices before each call, pins a supported flat-price
endpoint, disables fallback/redirects, sets provider price ceilings and bounds output.
Unknown price components or tier schedules fail closed. Reservations use twice the
UTF-8 byte input bound plus 4096 envelope tokens, bounded output and request fees;
cache rates are conservatively included. Integer nano-USD amounts round upward.
BEGIN IMMEDIATE/FULL transactions share at most four outstanding requests across
processes. Cancellation and unmetered/error responses retain reservations. Actual
reported charges are reconciled before semantic parsing, including truncated output;
over-estimate charges freeze further requests. There is no automatic release of an
uncertain charge, increase, reset, or hidden model substitution. Ledger access is a
trusted local filesystem boundary like other offline maintenance.

The new explicit general model default is deepseek/deepseek-v4.1-flash-1017.
Non-ASCII output, including escaped Unicode decoded from JSON, is now rejected as
well as input. ASCII is a conservative script gate, not a complete English detector;
no multilingual semantic validation claim follows from it. Existing quarantined
extraction and frozen prompt defaults remain unchanged. Broad semantic acceptance
is still unverified. Public OpenRouter pricing/limits documentation was consulted;
no paid request was performed and no historical spending allowance was reset.

### D-77 — Non-user records cannot supply unverified user attribution
The explicit continuation requirement is stronger than merely hiding agent Claims
from user projections. Source-checks-v3 rejects `speaker=user` whenever the immutable
Source recorder is not user. A proposed speaker label or agent quotation is not a
verifiable user citation. Cross-source citation verification remains outside Phase 3,
so these derivations are unresolved rather than accepted. This check runs both before
quarantine and again before approval/publication. Agent assertions remain agent Claims;
user fidelity approval/confirmation cannot rewrite source authorship. Historical rows
and frozen model prompts are unchanged. Existing negative-authority and transport tests
were extended without adding redundant test methods.

### D-78 — Reject ambiguous candidate JSON before schema conversion
Source-checks-v4 replaces the proposal's intermediate permissive JSON Value parse
with recursive duplicate-member rejection. Last-key-wins interpretation can hide
conflicting Claim text, anchor quotes or semantic metadata, including escaped
spellings of the same member. Reject rather than choose or repair. Typed review
responses already reject duplicate fields. Original source/candidate bytes and
frozen prompt hashes remain unchanged; archived revalidation is separate evidence.

### D-79 — Source-bound local fidelity inspection and explicit unknown measurements
The new extraction-review command is local-user-only explicit inspection, not a
projection or approval. It reads candidate, source and frozen plan in one read
transaction, verifies hashes, and supplies exact byte/scalar anchor positions and
unassessed semantic checks. Existing approval separately validates freshness and
lifecycle. No model review or empty checklist implies semantic acceptance.
Fine-grained scorer dimensions are optional for old labels; missing entries remain
unknown with null error rates. Accepted and generated populations remain separate.
Broad archived structural replay is not presented as a fresh semantic trial.

### D-80 — Encoded input is not an English-only routing exception
DeepSeek's input guard now inspects Unicode escape notation and decoded JSON as
well as raw ASCII. Escaped Korean, nested strings and duplicate-key masking must
not carry Korean across the English-only boundary. Conservative rejection of literal
non-ASCII Unicode escape notation is intentional; ASCII is still not proof of English.
No model substitution, translation or production Korean-model promotion is authorized.
The explicit frozen Plan remains the model routing configuration; unavailable paths
fail closed. Korean-capable direct processing remains semantically unaccepted.

OpenRouter also rejects a combined system/input envelope above 256 KiB before any
network request or budget operation. This is an operational safety bound, not a
tokenizer/context guarantee or chunking implementation. No truncation or partial
coverage is allowed. Sources stay intact and normal durable job failure/retry applies.

### D-81 — Focused evidence rather than retry-to-pass semantic acceptance
The frozen 12-case challenge manifest and source-grounded evaluation protocol live
in evaluation/phase3/focused-*. Historical v23 outputs are selected by fixed archive/
run/attempt ordering without filtering for success; older profiles remain historical.
The new protocol demands exact Claim identity, original-source evidence, explicit
unknown measurements and competing interpretations. It is evaluation-only, not a
retroactive modification of frozen v23 or a publication gate. An identical-text reader
verdict flip is disagreement, not extraction improvement. Its live repair effect is
unverified. Paid dispatch is blocked by the unchanged persistent budget, and the
offline runner deliberately has no alternate billing path.

Cross-source citation verification remains deferred: source_citation is Phase 4 and
Phase 3 has no trusted cited-span/semantic-support authority model. Even an existing
user Source ID supplied by a model cannot confer user authority. Unsupported citation
fields fail schema validation; non-user speaker=user remains unresolved.

### D-82 — Semantic review remains diagnostic after a coherent false acceptance
Frozen v23 prompt/evidence is unchanged. V24 corrects contradictory decomposition and
quoted-reference examples and introduces a source-bound, per-dimension review schema.
The v24/v25 validator checks exact Claim identity and unique original quotations,
rejects inconsistent aggregate verdicts, and treats disagreement across its three
source-grounded reviews as unresolved. The same checks run at proposal quarantine,
local fidelity inspection, and exact-hash user approval; standalone readers remain
source-blind and cannot establish entailment. In mixed/Korean Sources,
`korean_meaning=not_applicable` is rejected conservatively even on an English Claim:
a model can omit the Korean antecedent from its evidence, so source-dependency
classification is not yet reliable enough to relax this guard.

This protocol establishes internal consistency, not semantic truth. On 2026-10-24,
Claude Haiku 5.5 returned a protocol-valid `pass` for the known unsupported
`r1-nested_quote` candidate under both v24 and v25 prompts. V24 correctly rejected
the known non-atomic `v23-fresh-003` candidate. These targeted outcomes do not
justify promoting a new prompt, treating model agreement as ground truth, or tagging
Phase 3. All proposals remain quarantined pending the existing exact-hash local-user
review. Reviewer false acceptance and extraction quality remain open acceptance risks.

### D-83 — Source commentary is not a substitute for a faithful unknown argument
V27+ source-bound review requires a separate `source_meta_expansion` dimension.
An explicit unknown actor/affected-target marker can make a Claim standalone;
appending an assertion about absent document details can instead add an independent
proposition. V28 distinguishes licensed grammatical/discourse reference resolution
from invented causal, ownership or method-content assertions. Its targeted reviewer
rejects the old nested gloss twice, while a corrected narrator-perspective editorial
control passes twice. Both repeated pairs still disagree on evidence/inventory,
and the shorter v28 extractor invents new team ownership and loses a delay target.
These results do not justify production promotion or a Phase 3 tag.

A passing inventory must account for every candidate Claim. Three-review comparison
also includes proposition text, so incompatible inventories sharing the same evidence
and index mappings cannot masquerade as agreement. Textually different paraphrases
remain unresolved: deterministic code does not prove their semantic equivalence.
Raw outputs remain immutable and reviews remain diagnostic, with exact-hash user
approval and lifecycle/authority checks required separately. Full results and the
unchanged budget ceiling are recorded in reports/phase-3-semantic-repair.md.

### D-84 — Reserve a bounded request once, using its actual output cap
The direct-Anthropic evaluator's future reservations use one token per serialized
request UTF-8 byte plus 4,096 framing tokens, select pricing from that upper input
bound, and price the request's validated output cap. The prior blanket 2x multiplier
and fixed 8,000-token reservation duplicated padding for a 6,000-token request.
Removing that duplication lowers the next review's reserve by 56% while preserving
full output and the aggregate ceiling. Historical reservations, charges and receipts
are not recalculated. Reservation reductions are not reductions in actual token
prices or historical spending. All 33 prior requests' token usage fit the new bound
at current verified prices. One formerly blocked review completed and passed at a
$0.00264480 conservative usage upper bound; this does not establish Phase 3 acceptance.

### D-85 — Worker and evaluators must share the source-bound review protocol
V24+ source reviews are not old reader-checklist objects. The live evaluator and
archived-output revalidator now call the worker's public source-aware validator;
full gate eligibility includes the complete group and disagreement checks. The
source-free proposal-review helper refuses bound profiles. Offline replay reports
an actual failed prefix before an incomplete-group error, matching worker failure
behavior, and never fabricates later review calls. Original results remain intact.

### D-86 — Preserve assertion layers and distinguish canonical context from truth
V29 replaces an extraction example that itself left a quoted narrator possessive
unbound in the second Claim. It uses full original-Source review context and unique
verbatim original clauses for inventory identity. Exact context and literal clauses
do not prove correct semantics: a v29 review still counted a relay and its embedded
report separately. V30 requires an explicit duplicates dimension and tells extraction
to keep the complete attribution chain without emitting its redundant inner report.
It also separates a linguistic original-narrator role from authenticated-user identity.
A live v30 nested example produces two faithful editorially inspected core Claims
with unknown speaker metadata, but has not completed source review or independent
adjudication. No current-profile semantic acceptance, production default change or
Phase 3 tag follows from this isolated improvement.

### D-87 — Distinct inventory evidence, nonblank explanations and valid empty results
A different explanation does not create another Source proposition. Bound review
rejects repeated inventory identities based on exact proposition text, quote support
and Claim mapping, ignoring order only in comparison copies. Raw judgments are
retained. Required explanations and inventory descriptions must be nonblank.

An empty inventory is valid only for a zero-Claim batch with a nonblank empty reason;
three coherent, agreeing source reviews and later user approval remain required.
Nonempty batches cannot omit inventory, and uncertain empty results fail closed.
These consistency checks do not decide whether a Source truly lacks propositions.

The Korean review obligation now uses the same conservative Hangul/Jamo presence
check as extraction, including decomposed modern Hangul, rather than only precomposed
syllables. Detection does not normalize Source bytes or alter evidence coordinates.
Regression tests exposed all three prior failure groups and passed after correction;
no new model-quality or Phase 3 acceptance claim follows from them.

### D-88 — Preserve invalid reviews and retain v31 as an unpromoted experiment
The authorized five-cent continuation preserves the existing ledger and all historical
charges. V30 now has full diagnostic gate traces for habit, ambiguous reference and
mixed Korean/English dependency. An omitted nested review and a contradictory v31
pass remain invalid evidence, included in replay and comparisons. No passing repeat
is substituted for a failed primary trace. V31 distinguishes unknown entity identity
from affected target and clarifies licensed verification ellipsis; actual evaluation
still shows false-rejection risks and contradictory uncertainty. It is not promoted.
A full exact-Source quote, nonempty schema array or agreeing models is not independent
semantic truth. Phase 3 remains incomplete under its unchanged acceptance criteria.

### D-89 — Compare exact configurations and retain capped diagnostic failures
Repeat identity is the full canonical request body, including effort, output cap,
schema, model and tier; system/user text alone is insufficient. One bounded matched
v31 medium-effort review still rejects the hypothetical, so medium is a diagnostic
option, not a production change or a proven reliability repair. It keeps the same
3,000-token cap. Capped Source reviews retain a durable incomplete rejection after
usage settlement; replay includes the failure rather than parsing it as a judgment.
All historical raw evidence and existing authority/approval boundaries are preserved.

### D-90 — Source-relative roles, report scope and structural diagnostic preflight
V32/v33 distinguish unidentified entity roles from independent document-commentary
assertions and preserve original-utterance binding without supplying world identity.
Reported content need not be independently world-verified to preserve a Source's
report; stripping attribution still fails. A real v32 nested output exposed nullable
index projection members missed by Python's partial envelope check. Paid diagnostic
review now runs the actual offline Rust validator first. No malformed raw output is
repaired. V33 fixes that generated metadata shape, but its three Source reviews still
disagree on the complete outer-report inventory boundary. The gate withholds the
batch. V32 delay and v33 hypothetical complete traces are diagnostic evidence only;
all failures, the truncated negative control and profile distinctions remain visible.
Phase 3 remains incomplete, with no default promotion or completion tag.

### D-91 — Complete governing predicates and complete-Claim entailment
V34 inventory identities retain all governing report layers and cannot stop inside
Korean inflection/connective endings. Its unchanged v33 extraction/reader components
may reuse archived results only after complete request equality; provenance keeps
actual IDs. An explicit 6,000-token Source diagnostic cap is a different configuration,
not an identical retry or replacement for the initial truncated 3,000-token attempt.
The nested report now has a coherent full diagnostic trace with exact Plan hash.

A negative repeated batch exposes entailment pass despite rejected unsupported
content. Source-checks-v5 rejects that cross-dimension contradiction for v34 in both
Rust and Python, without normalizing model output. Replay retains historical receipt
classifications and separately reports stricter current validity. Both negative raw
aggregates reject, but their full signatures disagree and both current objects are
invalid. This is safe failure, not demonstrated general reviewer reliability. No
profile promotion or Phase 3 completion follows. The original strictly-below-$5 cap
is respected; final cumulative upper bound is $4.99560435 with no unsettled requests.

## Phase 3 funded semantic boundary investigation (2026-10-25)

The human authorized a cumulative ceiling strictly below $6; no ledger reset.
Retain `source-checks-v6` one-to-one proposition/Claim mapping and opt-in v39+ separate
Source responsibilities. Exact stage/Claim/Source identities and all required gates
remain mandatory; no majority voting or normalized contradictory passes.

Reject candidate-blind Source-first inventory as a demonstrated remedy: it invented
a redundant report and accepted an actual bad repair. Narrow whole-Claim entailment
correctly rejected that bad grammatical-commentary repair on one Source, but a repeat
was protocol-invalid. v41 concise generation and its bounded repairs did not produce
a complete passing cohort; keep the profile unpromoted and Phase 3 incomplete.
Unknown transport outcomes retain full reservations. Current totals and failed
attempts are in `reports/phase-3-funded-semantic-validation.md`; older budgets and
model agreements are historical observations, not current semantic acceptance.

### Explicit extraction wire contract after failed v41 repairs

Use a separately named optional `contract-v1` wire schema for direct Anthropic
extraction, rather than changing frozen prompts or stripping malformed responses.
The historical evaluator used unrestricted text whereas the production adapter
already requests a JSON object. New schema-labelled request IDs and receipt-bound
replay preserve the distinction. HTTP/connection/timeout failures retain their full
reservation and redacted error evidence. This is an offline-verified envelope fix;
no live semantic improvement, profile promotion or Phase 3 completion is claimed.

## Resumed Haiku boundary repair (2026-10-25)

Keep all historical request charges and unknown reservations. Following the human's
resume instruction, new direct calls require an exclusive nonblocking dispatch slot
throughout reservation and transport. Old unknown billing no longer counts as live
concurrency, but still consumes its full reserved monetary capacity. The strict
cumulative cap is unchanged. This supersedes the obsolete pending-count blocker.

Preserve actual ordered JSON transport and its wire hash for v44+, including schema
property order. Equal parsed objects do not prove equal generation recipes. Keep
strict scalar proposition/Claim mappings. v47+ combines the seven related meaning
features into one shared comparison while retaining independent atomicity and
coverage, all ten required dimensions, all evidence and any reject/unresolved veto.
Inventory row order can differ; proposition identity, support and mapping cannot.

An exact-repeat rejection is independently replayable by its actual receipt. Do not
replace a passing primary, invent a failed report or vote past disagreement. In v48
the delay primary passed and its repeat rejected; the case remains withheld. Actual
repair feedback fixed nested 못 but generated new emotion and delay defects, which
remain recorded rather than silently hand-edited. v49 condenses extraction guidance
and requires full conditional scope in inventory identity. These profiles remain
opt-in; completion and default promotion require the measured semantic scope in the
current audit, independently of green synthetic tests.

Reject the thinking-disabled experiment: one fused control was raw-pass but
protocol-invalid, and a good verification target was rejected by overly literal
interpretation. Retain all six paid probe requests and charges; production reasoning
mode is unchanged. No migration, new dependency or Phase 4 feature is introduced.

### v50 standalone analysis and current acceptance boundary

Require interpretation and reason/status findings for atomicity, reference and time
before issues/verdict. Validate exact coherence in runtime, approval, evaluators and
bound repair context. Reuse v49 Source/generation receipts only by actual full body
and ordered wire hash equality; no saved receipt is a new independent call. Current
v50 complete gates pass four of six exposed Sources after bounded repairs; preserve
the two unresolved batches, positive repeat agreements and all initial failures.

Structured reasons exposed conditional-reader invented ownership/time binding and
a delay affected-target policy conflict between reader and Source evaluator. Keep
these as unresolved semantic defects despite green protocol tests; do not normalize
period-different inventories just to turn this known questionable conditional into
a pass. No production default promotion, completion tag or user approval is made.
Current accounting and raw evidence are in reports/phase-3-resumed-haiku.md.
