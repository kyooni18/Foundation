"""Hand-written evaluation clusters (part B): temporal revisions, dependencies, hidden records,
no-answer and partial-evidence cases, comprehensive lists, ambiguity."""
from corpus_a import C, Q, R

C("phone", "holdout", [
    R("phn1", "I use a Galaxy phone.", date="2026-01-10"),
    R("phn2", "My Galaxy phone was stolen in May, so I switched to an iPhone.", date="2026-05-20"),
    R("phn3", "Phone plan is 20 GB per month with unlimited calls.", date="2026-02-02"),
    R("phn4", "아이폰 배터리 성능이 85%까지 떨어졌다.", date="2026-10-01"),
], [
    Q("phn-q1", "What phone do I use?", ["phn2"], "temporal", intent="temporal", depth="balanced"),
    Q("phn-q2", "What phone did I use in February?", ["phn1"], "temporal", intent="temporal", as_of="2026-02-15T00:00:00.000Z", must_not=["phn2"]),
    Q("phn-q3", "지금 쓰는 휴대폰", ["phn2"], "temporal", intent="temporal"),
], relations=[("supersedes", "phn2", "phn1")])

C("jobtitle", "dev", [
    R("job1", "I work as a backend engineer at Acme.", date="2025-03-01"),
    R("job2", "I was promoted to senior backend engineer at Acme.", date="2026-01-15"),
    R("job3", "I moved to the platform team as tech lead.", date="2026-08-01"),
    R("job4", "Team standup is at 10:00 every weekday.", date="2026-02-03"),
], [
    Q("job-q1", "What is my current role?", ["job3"], "temporal", intent="temporal"),
    Q("job-q2", "What was my job title in January 2026?", ["job1", "job2"], "temporal", intent="temporal", as_of="2026-02-01T00:00:00.000Z", must_not=["job3"]),
    Q("job-q3", "history of my roles at Acme", ["job1", "job2", "job3"], "comprehensive", intent="comprehensive", depth="exhaustive"),
], relations=[("supersedes", "job2", "job1"), ("supersedes", "job3", "job2")])

C("sqlite_decision", "holdout", [
    R("sql1", "We chose SQLite as the only datastore.", date="2026-02-01"),
    R("sql2", "The app is single-user and local-first, so a server database is unnecessary.", date="2026-01-28"),
    R("sql3", "Backups are a single-file copy of the database.", date="2026-02-02"),
    R("sql4", "Full-text search uses FTS5 over character bigrams.", date="2026-03-05"),
    R("sql5", "그래서 SQLite를 선택했고 Postgres는 쓰지 않는다.", date="2026-02-01"),
], [
    Q("sql-q1", "Why did we choose SQLite?", ["sql2", "sql1"], "relational", intent="relational"),
    Q("sql-q2", "what does the datastore decision depend on", ["sql2"], "relational", intent="relational"),
    Q("sql-q3", "SQLite를 선택한 이유", ["sql2", "sql5"], "relational", intent="relational"),
], deps=[("sql1", "sql2", "premise"), ("sql3", "sql1", "informed_by")])

C("hidden_notes", "dev", [
    R("hid1", "The quarterly salary review spreadsheet is in the HR shared drive.", agent=True, date="2026-03-01"),
    R("hid2", "Agent note: user's bank account password hint is the dog's name.", agent=True, date="2026-03-02"),
    R("hid3", "The user's wifi password is stored under router settings.", forget=True, date="2026-03-03"),
    R("hid4", "Meeting with the landlord is on Tuesday at 4pm.", retract=True, date="2026-03-04"),
    R("hid5", "Dog's name is Mango.", date="2026-03-05"),
    R("hid6", "Tuesday 4pm is free for deep work.", date="2026-03-06"),
], [
    Q("hid-q1", "salary review spreadsheet", [], "unauthorized", intent="lexical", must_not=["hid1"]),
    Q("hid-q2", "password hint", [], "unauthorized", intent="lexical", must_not=["hid2"]),
    Q("hid-q3", "wifi password router", [], "unauthorized", intent="lexical", must_not=["hid3"]),
    Q("hid-q4", "meeting with the landlord", [], "unauthorized", intent="semantic", must_not=["hid4"]),
    Q("hid-q5", "salary review spreadsheet", ["hid1"], "agent_view", intent="lexical", principal="agent"),
    Q("hid-q6", "what is the dog called", ["hid5"], "semantic", intent="semantic"),
])

C("noanswer", "dev", [
    R("noa1", "My favorite editor is Neovim with the Gruvbox theme.", date="2026-04-01"),
    R("noa2", "I run 5 km on Saturday mornings.", date="2026-04-02"),
    R("noa3", "저녁에는 보통 책을 한 시간 읽는다.", date="2026-04-03"),
], [
    Q("noa-q1", "What is my blood type?", [], "noanswer", intent="semantic"),
    Q("noa-q2", "내 은행 계좌번호는?", [], "noanswer", intent="semantic"),
    Q("noa-q3", "Which airline did I fly last year?", [], "noanswer", intent="semantic"),
    Q("noa-q4", "xylophone repair shop", [], "noanswer", intent="lexical"),
    Q("noa-q5", "Who is the CEO of the company?", [], "noanswer", intent="semantic"),
    Q("noa-q6", "ZXQ-99281", [], "noanswer", intent="exact"),
])

C("partial", "holdout", [
    R("prt1", "The release is planned for the second week of September.", date="2026-07-15"),
    R("prt2", "QA sign-off is owned by the mobile team.", date="2026-07-16"),
], [
    Q("prt-q1", "When is the release and who is the release manager?", ["prt1"], "partial", intent="semantic", note="manager unknown: partial evidence"),
    Q("prt-q2", "release date and QA owner", ["prt1", "prt2"], "partial", intent="semantic"),
])

C("deployments", "dev", [
    R("dep1", "Orion production runs in the AWS Seoul region.", date="2026-04-01"),
    R("dep2", "Orion staging runs in the AWS Tokyo region.", date="2026-04-02"),
    R("dep3", "The Orion demo environment is hosted on a Hetzner server in Germany.", date="2026-04-03"),
    R("dep4", "Atlas runs only in the AWS Seoul region.", date="2026-04-04"),
    R("dep5", "Orion CDN assets are served from CloudFront.", date="2026-04-05"),
], [
    Q("dep-q1", "list all places where Orion is deployed", ["dep1", "dep2", "dep3"], "comprehensive", intent="comprehensive", depth="exhaustive"),
    Q("dep-q2", "모든 Orion 환경", ["dep1", "dep2", "dep3"], "comprehensive", intent="comprehensive", depth="exhaustive"),
    Q("dep-q3", "where does Atlas run", ["dep4"], "semantic", intent="semantic"),
])

C("sports", "holdout", [
    R("spt1", "I go bouldering on Tuesdays at the climbing gym near the office.", date="2026-05-05"),
    R("spt2", "화요일 저녁에는 클라이밍 짐에 간다.", date="2026-05-05"),
    R("spt3", "I cannot swim.", date="2026-03-15"),
    R("spt4", "나는 수영을 못 한다.", date="2026-03-15"),
    R("spt5", "My marathon personal best is 3:42.", date="2026-04-20"),
    R("spt6", "마라톤 개인 최고 기록은 3시간 42분이다.", date="2026-04-20"),
], [
    Q("spt-q1", "Can I swim?", ["spt3", "spt4"], "negation", intent="semantic"),
    Q("spt-q2", "수영할 줄 아나?", ["spt4", "spt3"], "negation", intent="semantic"),
    Q("spt-q3", "what day do I climb", ["spt1", "spt2"], "cross", intent="semantic"),
    Q("spt-q4", "마라톤 기록", ["spt6", "spt5"], "cross", intent="lexical"),
])

C("finance", "holdout", [
    R("fin1", "The company corporate card may only be used for business-trip expenses.", date="2026-02-20"),
    R("fin2", "회사 법인카드는 출장 경비 외에는 쓸 수 없다.", date="2026-02-20"),
    R("fin3", "Expense reports are due on the 5th of every month.", date="2026-03-01"),
    R("fin4", "Per diem for domestic trips is 60,000 KRW.", date="2026-03-02"),
    R("fin5", "I contribute 10% of salary to the pension plan.", date="2026-03-03"),
], [
    Q("fin-q1", "Can I use the corporate card for personal meals?", ["fin1", "fin2"], "negation", intent="semantic"),
    Q("fin-q2", "출장 일비 얼마야?", ["fin4"], "cross", intent="semantic"),
    Q("fin-q3", "when are expense reports due", ["fin3"], "semantic", intent="semantic"),
    Q("fin-q4", "연금 납입 비율", ["fin5"], "cross", intent="semantic"),
])

C("family", "holdout", [
    R("fam1", "Harin is in the third grade of elementary school this year.", date="2026-03-02"),
    R("fam2", "하린이는 올해 초등학교 3학년이다.", date="2026-03-02"),
    R("fam3", "Every Wednesday evening I take Harin to piano academy.", date="2026-03-01"),
    R("fam4", "Harin quit her piano lessons in September.", date="2026-09-20"),
    R("fam5", "My mother in Daejeon has dementia and can't be left alone.", date="2026-04-20"),
    R("fam6", "어머니는 대전에서 혼자 계시면 안 된다.", date="2026-04-20"),
], [
    Q("fam-q1", "Which grade is Harin in?", ["fam2", "fam1"], "cross", intent="semantic"),
    Q("fam-q2", "하린이 아직 피아노 다녀?", ["fam4"], "temporal", intent="temporal", must_not=[]),
    Q("fam-q3", "엄마 혼자 계셔도 되나?", ["fam5", "fam6"], "cross", intent="semantic"),
    Q("fam-q4", "Wednesday piano pickup", ["fam3", "fam4"], "temporal", intent="temporal"),
], relations=[("supersedes", "fam4", "fam3")])

C("security", "holdout", [
    R("sec1", "API tokens must be rotated every 90 days.", date="2026-01-18"),
    R("sec2", "API 토큰은 90일마다 교체해야 한다.", date="2026-01-18"),
    R("sec3", "Never commit .env files; secrets live in the vault under kv/prod.", date="2026-01-19"),
    R("sec4", "비밀번호는 1Password에만 저장한다.", date="2026-01-20"),
    R("sec5", "SSH access to production requires the bastion host bastion-01.", date="2026-01-21"),
    R("sec6", "2FA is mandatory for all admin accounts.", date="2026-01-22"),
], [
    Q("sec-q1", "how often rotate tokens", ["sec1", "sec2"], "cross", intent="semantic"),
    Q("sec-q2", "bastion-01", ["sec5"], "exact", intent="exact"),
    Q("sec-q3", ".env 파일 커밋해도 돼?", ["sec3"], "negation", intent="semantic"),
    Q("sec-q4", "where are passwords stored", ["sec4"], "cross", intent="semantic"),
    Q("sec-q5", "kv/prod", ["sec3"], "exact", intent="exact"),
])

C("cooking_ko", "holdout", [
    R("ckk1", "김치찌개는 신 김치를 먼저 볶은 다음 물을 붓는다.", date="2026-02-14"),
    R("ckk2", "된장찌개에는 애호박과 두부를 넣는다.", date="2026-02-15"),
    R("ckk3", "계란말이는 약불에서 천천히 말아야 터지지 않는다.", date="2026-02-16"),
    R("ckk4", "Kimchi stew tastes best with aged kimchi sautéed first.", date="2026-02-17"),
    R("ckk5", "밥은 압력솥으로 지으면 15분이면 된다.", date="2026-02-18"),
], [
    Q("ckk-q1", "how to make kimchi stew", ["ckk1", "ckk4"], "cross", intent="semantic"),
    Q("ckk-q2", "된장찌개 재료", ["ckk2"], "lexical", intent="lexical"),
    Q("ckk-q3", "계란말이가 터지는 이유", ["ckk3"], "semantic", intent="semantic"),
    Q("ckk-q4", "pressure cooker rice time", ["ckk5"], "cross", intent="semantic"),
])
