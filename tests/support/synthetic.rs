//! Synthetic published Claims for Phase 4 tests and the benchmark. Rows are written with
//! plain SQL that the schema triggers still validate (anchors, job/run linkage, acyclicity);
//! derived validity is rebuilt by `App::open`. Test-only: never part of the library surface.
#![allow(dead_code)]
use prime_radiant::extraction::hash;
use rusqlite::{Connection, params};

/// xorshift64*: reproducible from the printed seed, no external crates.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
}
pub struct SourceSpec<'a> {
    pub recorder: &'a str,
    pub session: Option<&'a str>,
    pub captured_at: &'a str,
    pub authored_at: Option<&'a str>,
    pub claims: Vec<ClaimSpec<'a>>,
}
#[derive(Clone)]
pub struct ClaimSpec<'a> {
    pub text: String,
    pub tkind: Option<&'a str>,
    pub applies_from: Option<&'a str>,
    pub applies_to: Option<&'a str>,
    pub index_fields: Option<&'a str>,
}
impl ClaimSpec<'_> {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tkind: None,
            applies_from: None,
            applies_to: None,
            index_fields: None,
        }
    }
}
fn ulid() -> String {
    ulid::Ulid::new().to_string()
}
/// Insert a Source with one accepted extraction run and its Claims; returns (source, claims).
pub fn source(conn: &Connection, spec: &SourceSpec) -> (String, Vec<String>) {
    let content = spec
        .claims
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let id = ulid();
    conn.execute(
        "INSERT INTO source(id,recorder,session_id,content,content_sha256,media_type,authored_at,captured_at,retention_request,request_digest) VALUES(?1,?2,?3,?4,?5,'text/plain',?6,?7,'none',?5)",
        params![id, spec.recorder, spec.session, content, hash(&content), spec.authored_at, spec.captured_at],
    )
    .unwrap();
    let job = ulid();
    conn.execute(
        "INSERT INTO job(id,kind,target_id,idem_key,state,attempts,run_after,created_at,updated_at) VALUES(?1,'extract',?2,?3,'done',1,?4,?4,?4)",
        params![job, id, format!("initial/{id}"), spec.captured_at],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO extraction_run(job_id,source_id,extractor_id,output_hash,approval_actor,created_at) VALUES(?1,?2,'synthetic',?3,'user',?4)",
        params![job, id, hash(&job), spec.captured_at],
    )
    .unwrap();
    let speaker = if spec.recorder == "user" {
        "user"
    } else {
        "agent"
    };
    let mut offset = 0;
    let mut claims = Vec::new();
    for c in &spec.claims {
        let claim = ulid();
        let end = offset + c.text.len();
        conn.execute(
            "INSERT INTO claim(id,source_id,text,lang,category,speaker,basis,anchor_start,anchor_end,anchor_quote,tkind,applies_from,applies_to,standing,index_fields,extractor_id,job_id,created_at) VALUES(?1,?2,?3,'en','Fact',?4,'verbatim',?5,?6,?3,?7,?8,?9,0,?10,'synthetic',?11,?12)",
            params![claim, id, c.text, speaker, offset, end, c.tkind, c.applies_from, c.applies_to, c.index_fields, job, spec.captured_at],
        )
        .unwrap();
        offset = end + 1;
        claims.push(claim);
    }
    (id, claims)
}
pub fn dependency(conn: &Connection, child: &str, parent: &str, kind: &str) -> String {
    let id = ulid();
    conn.execute(
        "INSERT INTO claim_dependency(id,child,parent,kind,declared_by,created_at) VALUES(?1,?2,?3,?4,'system','2026-01-01T00:00:00.000Z')",
        params![id, child, parent, kind],
    )
    .unwrap();
    id
}
pub fn mention(conn: &Connection, claim: &str, surface: &str) -> String {
    let id = ulid();
    let text: String = conn
        .query_row("SELECT text FROM claim WHERE id=?1", [claim], |r| r.get(0))
        .unwrap();
    let start = text.find(surface).expect("surface in claim text");
    conn.execute(
        "INSERT INTO mention(id,claim_id,surface,span) VALUES(?1,?2,?3,?4)",
        params![
            id,
            claim,
            surface,
            serde_json::json!({"start":start,"end":start+surface.len()}).to_string()
        ],
    )
    .unwrap();
    id
}
/// Open a schema-current database that no App currently owns.
pub fn connect(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}
/// A schema-v2 (Phase 3) database built with the original migration SQL, holding
/// `sources * per` Claims in premise chains plus lifecycle history.
pub fn v2_fixture(path: &std::path::Path, sources: usize, per: usize) {
    let mut conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    let tx = conn.transaction().unwrap();
    for (v, name, sql) in [
        (
            1,
            "0001_init",
            include_str!("../../src/store/migrations/0001_init.sql"),
        ),
        (
            2,
            "0002_extraction",
            include_str!("../../src/store/migrations/0002_extraction.sql"),
        ),
    ] {
        tx.execute_batch(sql).unwrap();
        tx.execute(
            "INSERT INTO schema_migration VALUES(?1,?2,?3,'2026-01-01T00:00:00.000Z')",
            params![v, name, hash(sql)],
        )
        .unwrap();
        tx.pragma_update(None, "user_version", v).unwrap();
    }
    let t = "2026-01-01T00:00:00.000Z";
    let mut previous: Option<String> = None;
    for s in 0..sources {
        let texts: Vec<String> = (0..per).map(|i| format!("legacy {s}.{i}.")).collect();
        let content = texts.join(" ");
        let id = ulid();
        tx.execute("INSERT INTO source(id,recorder,content,content_sha256,media_type,captured_at,retention_request,request_digest) VALUES(?1,?2,?3,?4,'text/plain',?5,'none',?4)",params![id,["user","agent"][s%2],content,hash(&content),t]).unwrap();
        let job = ulid();
        tx.execute("INSERT INTO job(id,kind,target_id,idem_key,state,attempts,run_after,created_at,updated_at) VALUES(?1,'extract',?2,?3,'done',1,?4,?4,?4)",params![job,id,format!("initial/{id}"),t]).unwrap();
        tx.execute("INSERT INTO extraction_run(job_id,source_id,extractor_id,output_hash,approval_actor,created_at) VALUES(?1,?2,'legacy',?3,'user',?4)",params![job,id,hash(&job),t]).unwrap();
        let mut offset = 0;
        for text in &texts {
            let claim = ulid();
            tx.execute("INSERT INTO claim(id,source_id,text,lang,category,speaker,basis,anchor_start,anchor_end,anchor_quote,standing,extractor_id,job_id,created_at) VALUES(?1,?2,?3,'en','Fact','unknown','verbatim',?4,?5,?3,0,'legacy',?6,?7)",params![claim,id,text,offset,offset+text.len(),job,t]).unwrap();
            offset += text.len() + 1;
            if let Some(parent) = &previous {
                tx.execute(
                    "INSERT INTO claim_dependency VALUES(?1,?2,?3,'premise','system',?4)",
                    params![ulid(), claim, parent, t],
                )
                .unwrap();
            }
            previous = Some(claim);
        }
        if s % 7 == 3 {
            tx.execute("INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES('source',?1,'retract','user','fixture',?2)",params![id,t]).unwrap();
            tx.execute("INSERT INTO status_event(target_kind,target_id,event,actor,reason,created_at) VALUES('source',?1,'restore','user','fixture',?2)",params![id,t]).unwrap();
        }
    }
    tx.commit().unwrap();
}
pub fn counts(conn: &Connection) -> Vec<i64> {
    [
        "source",
        "claim",
        "claim_dependency",
        "status_event",
        "job WHERE kind IN ('extract','revalidate')",
        "extraction_run",
    ]
    .iter()
    .map(|t| {
        conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}

/// One Claim for bulk fixtures (language and category are explicit).
pub struct BulkClaim {
    pub text: String,
    pub lang: &'static str,
}
/// Insert a user-verbatim (or agent) Source with one accepted run and its Claims quickly, using
/// cached statements. Derived tables (validity, access, index jobs) are produced by `App::open`.
pub fn bulk_source(
    conn: &Connection,
    recorder: &str,
    captured_at: &str,
    claims: &[BulkClaim],
) -> (String, Vec<String>) {
    bulk_source_at(conn, recorder, captured_at, None, claims)
}
pub fn bulk_source_at(
    conn: &Connection,
    recorder: &str,
    captured_at: &str,
    authored_at: Option<&str>,
    claims: &[BulkClaim],
) -> (String, Vec<String>) {
    let content = claims
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let id = ulid();
    conn.prepare_cached("INSERT INTO source(id,recorder,content,content_sha256,media_type,captured_at,authored_at,retention_request,request_digest) VALUES(?1,?2,?3,?4,'text/plain',?5,?6,'none',?4)")
        .unwrap()
        .execute(params![id, recorder, content, hash(&content), captured_at, authored_at])
        .unwrap();
    let job = ulid();
    conn.prepare_cached("INSERT INTO job(id,kind,target_id,idem_key,state,attempts,run_after,created_at,updated_at,finished_at) VALUES(?1,'extract',?2,?3,'done',1,?4,?4,?4,?4)")
        .unwrap()
        .execute(params![job, id, format!("initial/{id}"), captured_at])
        .unwrap();
    conn.prepare_cached("INSERT INTO extraction_run(job_id,source_id,extractor_id,output_hash,approval_actor,created_at) VALUES(?1,?2,'synthetic',?3,'user',?4)")
        .unwrap()
        .execute(params![job, id, hash(&job), captured_at])
        .unwrap();
    let speaker = if recorder == "user" { "user" } else { "agent" };
    let mut offset = 0;
    let mut ids = Vec::new();
    for c in claims {
        let claim = ulid();
        let end = offset + c.text.len();
        conn.prepare_cached("INSERT INTO claim(id,source_id,text,lang,category,speaker,basis,anchor_start,anchor_end,anchor_quote,standing,extractor_id,job_id,created_at) VALUES(?1,?2,?3,?4,'Fact',?5,'verbatim',?6,?7,?3,0,'synthetic',?8,?9)")
            .unwrap()
            .execute(params![claim, id, c.text, c.lang, speaker, offset, end, job, captured_at])
            .unwrap();
        offset = end + 1;
        ids.push(claim);
    }
    (id, ids)
}
