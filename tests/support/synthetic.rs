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
