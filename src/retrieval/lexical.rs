//! Lexical index (derived, versioned, rebuildable) and projection-safe BM25 search.
//!
//! FTS5 is used only to enumerate *matching* Claims. Corpus statistics (document count, average
//! length, per-term document frequency) are computed over the caller's projection alone, so a
//! hidden Claim can neither change a visible Claim's score nor its rank. This is deliberately not
//! FTS5's built-in `bm25()`, whose statistics cover the whole table.
use super::{
    access,
    tokenizer::{self, Kind, Term},
};
use crate::{domain::*, store::integrity::sha256};
use rusqlite::{Connection, params};
use std::collections::HashMap;

/// Index (or refresh) lexical rows for Claims, in the caller's writer transaction. Idempotent:
/// unchanged rows are left alone and a changed tokenizer version rewrites the row.
pub(crate) fn index_claims(conn: &Connection, claims: &[(String, String)]) -> Result<usize> {
    let mut written = 0;
    let now = Timestamp::now();
    for (id, text) in claims {
        let sha = sha256(text.as_bytes());
        let existing: Option<(i64, i64, String)> = conn
            .prepare_cached("SELECT lex_id,tokenizer,text_sha FROM claim_lex WHERE claim_id=?1")?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .ok();
        if let Some((_, version, old_sha)) = &existing
            && *version == tokenizer::VERSION
            && *old_sha == sha
        {
            continue;
        }
        if let Some((lex_id, _, _)) = existing {
            conn.prepare_cached("DELETE FROM claim_fts WHERE rowid=?1")?.execute([lex_id])?;
            conn.prepare_cached("DELETE FROM claim_lex WHERE lex_id=?1")?.execute([lex_id])?;
        }
        let (terms, count) = tokenizer::index_text(text);
        conn.prepare_cached("INSERT INTO claim_lex(claim_id,tokenizer,ntok,text_sha,indexed_at) VALUES(?1,?2,?3,?4,?5)")?
            .execute(params![id, tokenizer::VERSION, count as i64, sha, now.as_str()])?;
        let lex_id = conn.last_insert_rowid();
        conn.prepare_cached("INSERT INTO claim_fts(rowid,terms) VALUES(?1,?2)")?
            .execute(params![lex_id, terms])?;
        written += 1;
    }
    if written > 0 {
        access::bump(conn, "lexical")?;
    }
    Ok(written)
}

/// Drop every lexical row (rebuild starts from authoritative Claims via queued jobs).
pub(crate) fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM claim_fts", [])?;
    conn.execute("DELETE FROM claim_lex", [])?;
    access::bump(conn, "lexical")?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub claim_id: String,
    pub text: String,
    pub bm25: f64,
    /// IDF-weighted share of the query's terms present in the Claim (0..=1).
    pub coverage: f64,
    pub matched_terms: usize,
}
#[derive(Debug, Default)]
pub struct Outcome {
    pub hits: Vec<Hit>,
    /// Indexed Claims inside the caller's projection (the statistics population).
    pub population: usize,
    /// More matches existed than the candidate budget allowed.
    pub truncated: bool,
    pub query_terms: usize,
}

const K1: f64 = 1.2;
const B: f64 = 0.75;
/// Document-frequency counting stops here; terms this common carry negligible weight.
const DF_CAP: i64 = 5000;

/// Search the caller's projection. `filter_sql` is a trusted fragment over alias `c` (claim) and
/// `v` (claim_validity); it only narrows candidates and never enters the statistics.
pub(crate) fn search(
    conn: &Connection,
    p: &Principal,
    terms: &[Term],
    filter_sql: &str,
    budget: usize,
) -> Result<Outcome> {
    let col = access::column(p);
    let mut out = Outcome {
        query_terms: terms.len(),
        ..Default::default()
    };
    if terms.is_empty() {
        return Ok(out);
    }
    let (population, total_len): (i64, i64) = conn.query_row(
        &format!("SELECT count(*),COALESCE(sum(l.ntok),0) FROM claim_lex l JOIN claim_access a ON a.claim_id=l.claim_id WHERE a.{col}=1"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    out.population = population as usize;
    if population == 0 {
        return Ok(out);
    }
    let avg_len = (total_len as f64 / population as f64).max(1.0);
    // Document frequency of every query term inside the projection.
    let mut stats: Vec<(usize, i64)> = Vec::new();
    for (i, t) in terms.iter().enumerate() {
        let df: i64 = conn
            .prepare_cached(&format!(
                "SELECT count(*) FROM (SELECT 1 FROM claim_fts f JOIN claim_lex l ON l.lex_id=f.rowid JOIN claim_access a ON a.claim_id=l.claim_id WHERE claim_fts MATCH ?1 AND a.{col}=1 LIMIT ?2)"
            ))?
            .query_row(params![tokenizer::fts_term(t), DF_CAP + 1], |r| r.get(0))?;
        stats.push((i, df.min(DF_CAP)));
    }
    let n = population as f64;
    let idf = |df: i64| ((n - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln();
    // Candidates: documents matching the rarest terms first, plus the conjunction of the rarest few.
    let mut by_rarity = stats.clone();
    by_rarity.sort_by_key(|&(i, df)| (df, i));
    let present: Vec<&(usize, i64)> = by_rarity.iter().filter(|(_, df)| *df > 0).collect();
    let mut candidates: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut fetch = |expr: &str, limit: usize, candidates: &mut Vec<String>| -> Result<bool> {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT l.claim_id FROM claim_fts f JOIN claim_lex l ON l.lex_id=f.rowid JOIN claim_access a ON a.claim_id=l.claim_id JOIN claim c ON c.id=l.claim_id JOIN claim_validity v ON v.claim_id=c.id WHERE claim_fts MATCH ?1 AND a.{col}=1 {filter} LIMIT ?2",
            filter = if filter_sql.is_empty() { String::new() } else { format!("AND {filter_sql}") }
        ))?;
        let mut rows = stmt.query(params![expr, limit as i64 + 1])?;
        let mut got = 0;
        while let Some(row) = rows.next()? {
            got += 1;
            if got > limit {
                return Ok(true);
            }
            let id: String = row.get(0)?;
            if seen.insert(id.clone()) {
                candidates.push(id);
            }
        }
        Ok(false)
    };
    if present.len() >= 2 {
        let top: Vec<String> = present.iter().take(3).map(|(i, _)| tokenizer::fts_term(&terms[*i])).collect();
        out.truncated |= fetch(&top.join(" AND "), budget, &mut candidates)?;
    }
    for (i, _) in &present {
        if candidates.len() >= budget {
            out.truncated = true;
            break;
        }
        let room = budget - candidates.len();
        out.truncated |= fetch(&tokenizer::fts_term(&terms[*i]), room.max(1), &mut candidates)?;
    }
    if candidates.is_empty() {
        return Ok(out);
    }
    // Score candidates with projection-local statistics.
    let json = serde_json::to_string(&candidates).map_err(|_| invalid("ids", "serialization"))?;
    let rows: Vec<(String, String, i64)> = conn
        .prepare_cached("SELECT c.id,c.text,l.ntok FROM claim c JOIN claim_lex l ON l.claim_id=c.id WHERE c.id IN (SELECT value FROM json_each(?1))")?
        .query_map([json], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let weights: Vec<f64> = stats.iter().map(|&(_, df)| idf(df)).collect();
    let total_weight: f64 = weights.iter().sum::<f64>().max(f64::MIN_POSITIVE);
    let index: HashMap<&str, usize> = terms.iter().enumerate().map(|(i, t)| (t.text.as_str(), i)).collect();
    for (id, text, ntok) in rows {
        let mut tf = vec![0u32; terms.len()];
        for dt in tokenizer::terms(&text) {
            if let Some(&i) = index.get(dt.text.as_str()) {
                tf[i] += 1;
            }
        }
        // One-character CJK query terms match any bigram beginning with that character.
        for (i, t) in terms.iter().enumerate() {
            if tokenizer::is_prefix(t) && tf[i] == 0 {
                tf[i] = tokenizer::terms(&text)
                    .iter()
                    .filter(|d| matches!(d.kind, Kind::Bigram | Kind::Unigram) && d.text.starts_with(&t.text))
                    .count() as u32;
            }
        }
        let norm = K1 * (1.0 - B + B * ntok as f64 / avg_len);
        let mut score = 0.0;
        let mut matched = 0.0;
        let mut count = 0;
        for (i, &f) in tf.iter().enumerate() {
            if f > 0 {
                score += weights[i] * (f as f64 * (K1 + 1.0)) / (f as f64 + norm);
                matched += weights[i];
                count += 1;
            }
        }
        if count > 0 {
            out.hits.push(Hit {
                claim_id: id,
                text,
                bm25: score,
                coverage: matched / total_weight,
                matched_terms: count,
            });
        }
    }
    out.hits
        .sort_by(|a, b| b.bm25.total_cmp(&a.bm25).then_with(|| a.claim_id.cmp(&b.claim_id)));
    Ok(out)
}
