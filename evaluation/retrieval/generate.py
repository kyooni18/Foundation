#!/usr/bin/env python3
"""Deterministically build evaluation/retrieval/dataset.json (new, harder set) and r3_holdout.json
(frozen Phase 3 holdout converted unchanged). Usage: python3 generate.py [--r3-source PATH]."""
import json, random, re, sys, pathlib, importlib.util

HERE = pathlib.Path(__file__).parent
sys.path.insert(0, str(HERE))
import corpus_a, corpus_b  # noqa: E402,F401  (registers clusters)
from corpus_a import CLUSTERS, R, Q  # noqa: E402

rng = random.Random(20261025)
HANGUL = re.compile(r"[ㄱ-ㆎ가-힣]")


def lang(text):
    return "ko" if HANGUL.search(text) else "en"


TOPICS = [  # (english, korean)
    ("database migration plan", "데이터베이스 마이그레이션 계획"), ("on-call rotation", "당직 순번"),
    ("pricing experiment results", "가격 실험 결과"), ("search latency regression", "검색 지연 시간 악화"),
    ("onboarding flow redesign", "온보딩 흐름 개편"), ("incident review process", "장애 회고 절차"),
    ("mobile crash rate", "모바일 크래시 비율"), ("vendor contract renewal", "외주 계약 갱신"),
    ("data retention policy", "데이터 보존 정책"), ("hiring plan for next quarter", "다음 분기 채용 계획"),
    ("API versioning strategy", "API 버전 관리 전략"), ("test flakiness", "테스트 불안정성"),
    ("cost reduction targets", "비용 절감 목표"), ("accessibility audit", "접근성 점검"),
    ("localization backlog", "현지화 백로그"), ("security training schedule", "보안 교육 일정"),
    ("feature flag cleanup", "기능 플래그 정리"), ("customer interview findings", "고객 인터뷰 결과"),
    ("deployment freeze dates", "배포 동결 기간"), ("roadmap prioritization", "로드맵 우선순위"),
]
TEAMS = [("platform", "플랫폼"), ("mobile", "모바일"), ("data", "데이터"), ("design", "디자인"),
         ("infra", "인프라"), ("growth", "그로스"), ("payments", "결제"), ("search", "검색")]
PEOPLE = ["Minjun", "Seoyeon", "Dohyun", "Haeun", "Jiwoo", "Yuna", "Taeyang", "Soomin", "Hana", "Ethan", "Olivia", "Noah"]
STATUS = [("open", "열림"), ("in review", "리뷰 중"), ("blocked", "막힘"), ("done", "완료"), ("deferred", "보류")]
ITEMS = [("passport", "여권"), ("winter coat", "겨울 코트"), ("spare keys", "여분 열쇠"), ("tax documents", "세금 서류"),
         ("camera", "카메라"), ("tent", "텐트"), ("laptop charger", "노트북 충전기"), ("umbrella", "우산"),
         ("toolbox", "공구함"), ("board games", "보드게임"), ("yoga mat", "요가 매트"), ("printer paper", "프린터 용지"),
         ("extension cord", "멀티탭"), ("first-aid kit", "구급상자"), ("suitcase", "여행 가방"), ("vacuum filters", "청소기 필터"),
         ("wedding album", "결혼 앨범"), ("sewing kit", "바느질 도구"), ("flashlight", "손전등"), ("guitar capo", "기타 카포"),
         ("rain boots", "장화"), ("photo frames", "액자"), ("paint brushes", "붓"), ("bike pump", "자전거 펌프")]
PLACES = [("hall closet", "현관 수납장"), ("bedroom drawer", "침실 서랍"), ("garage shelf", "차고 선반"),
          ("kitchen cabinet", "주방 수납장"), ("storage box under the bed", "침대 밑 수납 상자"),
          ("balcony locker", "베란다 보관함"), ("office desk", "사무실 책상"), ("living room chest", "거실 수납 궤")]
ATTRS = [("birthday", "생일"), ("phone number", "전화번호"), ("email address", "이메일 주소"), ("favorite food", "좋아하는 음식"),
         ("allergy", "알레르기"), ("shirt size", "셔츠 사이즈")]
VALUES = {"birthday": ["March 14", "June 2", "October 30", "January 9", "August 21", "December 5"],
          "phone number": ["010-4821-7733", "010-9012-3345", "010-5567-0021", "010-7788-1290", "010-3344-9087", "010-6150-2284"],
          "email address": ["jiho.lee@example.com", "hana.k@example.org", "d.park@example.net", "yuna.choi@example.com", "t.kim@example.org", "s.han@example.net"],
          "favorite food": ["naengmyeon", "lasagna", "bibimbap", "tacos", "kimbap", "ramen"],
          "allergy": ["pollen", "penicillin", "dust mites", "latex", "eggs", "sesame"],
          "shirt size": ["M", "L", "S", "XL", "M", "L"]}
CONTACTS = ["Jiho", "Hana", "Dami", "Yuna", "Taeho", "Sumin"]
HABITS = [("run along the river", "강변을 달린다", "Saturday", "토요일"), ("meal-prep lunches", "점심 도시락을 미리 만든다", "Sunday", "일요일"),
          ("call my parents", "부모님께 전화한다", "Sunday", "일요일"), ("water the plants", "화분에 물을 준다", "Monday", "월요일"),
          ("back up my laptop", "노트북을 백업한다", "Friday", "금요일"), ("review the budget", "가계부를 점검한다", "first of the month", "매월 1일"),
          ("clean the fridge", "냉장고를 정리한다", "Thursday", "목요일"), ("practice guitar", "기타를 연습한다", "Tuesday", "화요일"),
          ("visit the library", "도서관에 간다", "Wednesday", "수요일"), ("do laundry", "빨래를 한다", "Saturday", "토요일"),
          ("write in my journal", "일기를 쓴다", "night", "밤"), ("stretch for ten minutes", "10분간 스트레칭한다", "morning", "아침"),
          ("take out the trash", "쓰레기를 내놓는다", "Tuesday", "화요일"), ("check the mailbox", "우편함을 확인한다", "Friday", "금요일"),
          ("read the news digest", "뉴스 요약을 읽는다", "weekday morning", "평일 아침"),
          ("deep-clean the kitchen", "주방 대청소를 한다", "Sunday", "일요일"), ("plan the week", "한 주 계획을 세운다", "Sunday evening", "일요일 저녁"),
          ("swim laps", "수영 레인을 돈다", "Thursday", "목요일"), ("visit the farmers market", "농산물 시장에 간다", "Saturday", "토요일"),
          ("update the shopping list", "장보기 목록을 정리한다", "Wednesday", "수요일"), ("walk the dog", "강아지 산책을 시킨다", "evening", "저녁"),
          ("water the balcony herbs", "베란다 허브에 물을 준다", "Monday", "월요일"), ("sort the recycling", "분리수거를 한다", "Thursday", "목요일"),
          ("pay the utility bills", "공과금을 낸다", "25th of the month", "매월 25일"), ("review the calendar", "일정을 점검한다", "Monday morning", "월요일 아침"),
          ("call my sister", "언니에게 전화한다", "Saturday", "토요일"), ("charge the e-reader", "전자책 리더를 충전한다", "Friday", "금요일"),
          ("clean the keyboard", "키보드를 청소한다", "first Monday", "첫째 월요일"), ("check the smoke alarm", "화재 경보기를 점검한다", "month end", "월말"),
          ("do the crossword", "십자말풀이를 한다", "Sunday morning", "일요일 아침")]


def families():
    records, queries = [], []
    # --- near-duplicate weekly syncs: (team, topic) unique pairs
    pairs = [(t, p) for t in range(len(TEAMS)) for p in range(len(TOPICS))]
    rng.shuffle(pairs)
    for i, (t, p) in enumerate(pairs[:60]):
        d = f"2026-{1 + i % 9:02d}-{1 + (i * 7) % 27:02d}"
        en = i % 2 == 0
        text = (f"Weekly sync on {d}: the {TEAMS[t][0]} team discussed the {TOPICS[p][0]}." if en else
                f"{d} 주간 회의에서 {TEAMS[t][1]}팀이 {TOPICS[p][1]}에 대해 논의했다.")
        records.append(R(f"syn{i:02d}", text, date=d, family="sync"))
        if i < 14:
            split = "dev" if i < 8 else "holdout"
            if i % 3 == 0:  # cross-language paraphrase
                q = (f"{TEAMS[t][1]}팀 {TOPICS[p][1]} 회의" if en else f"{TEAMS[t][0]} team meeting about {TOPICS[p][0]}")
                cat = "cross"
            elif i % 3 == 1:
                q = (f"which meeting covered the {TOPICS[p][0]} with {TEAMS[t][0]}" if en else f"{TEAMS[t][1]}팀과 {TOPICS[p][1]}를 논의한 회의")
                cat = "semantic"
            else:
                q = f"{TEAMS[t][0]} {TOPICS[p][0]}" if en else f"{TEAMS[t][1]} {TOPICS[p][1]}"
                cat = "candidate_set"
            queries.append(Q(f"syn-q{i:02d}", q, [f"syn{i:02d}"], cat, intent="semantic" if cat != "candidate_set" else "lexical", split=split, family=True))
    # --- tickets with ids
    for i in range(45):
        n = 4000 + rng.randrange(5000)
        person = PEOPLE[i % len(PEOPLE)]
        st = STATUS[i % len(STATUS)]
        proj = ["PRJ", "OPS", "WEB"][i % 3]
        tid = f"{proj}-{n}"
        text = (f"Ticket {tid} was assigned to {person} and is {st[0]}." if i % 2 == 0 else
                f"{tid} 티켓은 {person}에게 배정되었고 상태는 {st[1]}이다.")
        records.append(R(f"tic{i:02d}", text, date=f"2026-{1 + i % 9:02d}-{1 + (i * 5) % 27:02d}", family="ticket"))
        if i < 12:
            split = "dev" if i < 7 else "holdout"
            queries.append(Q(f"tic-q{i:02d}", tid, [f"tic{i:02d}"], "exact", intent="exact", split=split, family=True))
        if 12 <= i < 18:
            queries.append(Q(f"tic-q{i:02d}", f"who owns {tid}" if i % 2 == 0 else f"{tid} 담당자", [f"tic{i:02d}"], "exact", intent="exact", split="dev" if i < 15 else "holdout", family=True))
    # --- inventory with cross-language lookup
    placements = [(ITEMS[i], PLACES[(i * 3) % len(PLACES)]) for i in range(len(ITEMS))]
    for i, ((ie, ik), (pe, pk)) in enumerate(placements[:45]):
        en = i % 2 == 0
        text = f"The {ie} is in the {pe}." if en else f"{ik}은(는) {pk}에 있다."
        records.append(R(f"inv{i:02d}", text, date=f"2026-0{1 + i % 8}-1{i % 9}", family="inventory"))
        if i < 12:
            split = "dev" if i < 7 else "holdout"
            q = f"{ik} 어디 있지?" if en else f"where is my {ie}"
            queries.append(Q(f"inv-q{i:02d}", q, [f"inv{i:02d}"], "cross", intent="semantic", split=split, family=True))
    # --- contacts: attribute facts about people
    k = 0
    for ci, person in enumerate(CONTACTS):
        for ai, (ae, ak) in enumerate(ATTRS):
            val = VALUES[ae][ci]
            en = (ci + ai) % 2 == 0
            text = (f"{person}'s {ae} is {val}." if en else f"{person}의 {ak}은(는) {val}이다.")
            records.append(R(f"con{k:02d}", text, date=f"2026-02-{1 + k % 27:02d}", family="contact"))
            if k % 4 == 0 and len(queries) < 10**6:
                if ae in ("phone number", "email address"):
                    queries.append(Q(f"con-q{k:02d}", val, [f"con{k:02d}"], "exact", intent="exact", split="dev" if ci < 3 else "holdout", family=True))
                else:
                    q = f"{person} {ak}" if en else f"{person} {ae}"
                    queries.append(Q(f"con-q{k:02d}", q, [f"con{k:02d}"], "cross", intent="lexical", split="dev" if ci < 3 else "holdout", family=True))
            k += 1
    # --- habits
    for i, (ve, vk, de, dk) in enumerate(HABITS):
        en = i % 2 == 0
        text = f"I {ve} every {de}." if en else f"나는 {dk}마다 {vk}."
        records.append(R(f"hab{i:02d}", text, date=f"2026-03-{1 + i:02d}", family="habit"))
        if i < 8:
            q = f"when do I {ve}" if not en else f"{vk} 언제?"
            queries.append(Q(f"hab-q{i:02d}", q, [f"hab{i:02d}"], "cross", intent="semantic", split="dev" if i < 5 else "holdout", family=True))
    return records, queries


def build():
    records, queries, relations, deps, citations = [], [], [], [], []
    for c in CLUSTERS:
        for r in c["records"]:
            records.append(dict(r, cluster=c["name"]))
        for q in c["queries"]:
            queries.append(dict(q, split=c["split"], cluster=c["name"]))
        relations += [dict(kind=k, subject=s, object=o) for k, s, o in c["relations"]]
        deps += [dict(child=a, parent=b, kind=k) for a, b, k in c["deps"]]
    fr, fq = families()
    records += fr
    queries += fq
    keys = {r["key"] for r in records}
    for r in records:
        r["lang"] = lang(r["text"])
        r["recorder"] = "agent" if r.pop("agent", False) else "user"
        r["lifecycle"] = "forgotten" if r.pop("forget", False) else ("retracted" if r.pop("retract", False) else "live")
        r.pop("forget", None); r.pop("retract", None)
    for q in queries:
        assert all(g in keys for g in q["gold"] + q.get("must_not", [])), q
        q.setdefault("principal", "user")
        q.setdefault("must_not", [])
        q.setdefault("depth", "auto")
        q.setdefault("intent", "auto")
        q.setdefault("as_of", None)
    visible = [r for r in records if r["recorder"] == "user" and r["lifecycle"] == "live"]
    assert len(visible) >= 300, len(visible)
    return dict(version=1, seed=20261025, records=records, relations=relations, deps=deps, citations=citations, queries=queries)


def r3(source):
    spec = importlib.util.spec_from_file_location("holdout", source)
    h = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(h)
    recs = [dict(key=f"r3:{k}", text=v[0], lang=lang(v[0]), recorder="user", lifecycle="live", date=v[1]) for k, v in h.STORE.items()]
    recs += [dict(key=f"r3:d{i:03d}", text=t, lang=lang(t), recorder="user", lifecycle="live", date="2026-05-01") for i, t in enumerate(h.DISTRACTORS)]
    qs = [dict(id=f"r3:{q['id']}", text=q["text"], gold=[f"r3:{g}" for g in q["gold"]], cat=q["dir"], split="r3_holdout",
               principal="user", must_not=[], depth="auto", intent="auto", as_of=None, note=q.get("note", "")) for q in h.QUERIES]
    return dict(version=1, source="Foundation-Research/experiments/phase3/fixtures/holdout.py (frozen)", records=recs, relations=[], deps=[], citations=[], queries=qs)


if __name__ == "__main__":
    data = build()
    (HERE / "dataset.json").write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n")
    vis = sum(1 for r in data["records"] if r["recorder"] == "user" and r["lifecycle"] == "live")
    from collections import Counter
    print("records", len(data["records"]), "visible", vis, "queries", len(data["queries"]),
          dict(Counter(q["cat"] for q in data["queries"])), dict(Counter(q["split"] for q in data["queries"])))
    src = pathlib.Path.home() / "Code/Foundation-Research/experiments/phase3/fixtures/holdout.py"
    if "--r3-source" in sys.argv:
        src = pathlib.Path(sys.argv[sys.argv.index("--r3-source") + 1])
    if src.exists():
        (HERE / "r3_holdout.json").write_text(json.dumps(r3(src), ensure_ascii=False, indent=1) + "\n")
        print("r3 records", len(json.load(open(HERE / "r3_holdout.json"))["records"]))
