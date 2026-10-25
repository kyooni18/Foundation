//! Conservative fusion of independent evidence lists.
//!
//! Scores from BM25 and cosine similarity are *not* probabilities and are never added or
//! compared across paths. Each path contributes only a rank, and only if its evidence passes a
//! gate: lexical evidence must share a script with the query and cover enough of the query's
//! informative terms (R3 found that ungated rank fusion lets lexical noise outrank correct
//! cross-language semantic hits); semantic evidence must reach a minimum cosine. Gate values
//! are calibrated on development data only (see `reports/retrieval.md`); the holdout is never
//! used for calibration.
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Params {
    /// Reciprocal-rank-fusion constant (Cormack et al.); the literature default.
    pub rrf_k: f64,
    /// Minimum IDF-weighted share of query terms a lexical hit must contain.
    pub lexical_min_coverage: f64,
    /// Minimum cosine for a semantic hit to count.
    pub semantic_min_cosine: f32,
    /// A lexical hit covering at least this much of the query is "strong" on its own.
    pub lexical_strong_coverage: f64,
}
impl Default for Params {
    fn default() -> Self {
        Self { rrf_k: 60.0, lexical_min_coverage: 0.30, semantic_min_cosine: 0.30, lexical_strong_coverage: 0.80 }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Evidence {
    pub exact: bool,
    pub bm25: Option<f64>,
    pub coverage: Option<f64>,
    pub lexical_rank: Option<usize>,
    pub cosine: Option<f32>,
    pub semantic_rank: Option<usize>,
    pub fused: f64,
}

/// Reciprocal-rank fusion over already-gated ranked lists.
pub fn rrf(lists: &[&[String]], k: f64) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *out.entry(id.clone()).or_default() += 1.0 / (k + rank as f64 + 1.0);
        }
    }
    out
}

/// "exact" > "strong" (two independent paths agree, or lexical covers the whole query) >
/// "moderate" (one path passes its gate) > "weak" (graph neighbour only).
pub fn strength(e: &Evidence, p: &Params) -> &'static str {
    let lexical = e.coverage.is_some();
    let semantic = e.cosine.is_some();
    if e.exact {
        "exact"
    } else if (lexical && semantic) || e.coverage.is_some_and(|c| c >= p.lexical_strong_coverage) {
        "strong"
    } else if lexical || semantic {
        "moderate"
    } else {
        "weak"
    }
}

/// Normalised text for duplicate suppression (whitespace/case/width insensitive).
pub fn normal_form(text: &str) -> String {
    super::tokenizer::fold(text)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Collapse identical normalised texts, keeping the best-ranked; returns kept ids in order and
/// the ids each kept Claim stands for.
pub fn suppress_duplicates(ordered: &[String], text_of: &HashMap<String, String>) -> (Vec<String>, HashMap<String, Vec<String>>) {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut kept = Vec::new();
    let mut dups: HashMap<String, Vec<String>> = HashMap::new();
    for id in ordered {
        let nf = text_of.get(id).map(|t| normal_form(t)).unwrap_or_default();
        match seen.get(&nf) {
            Some(first) if !nf.is_empty() => dups.entry(first.clone()).or_default().push(id.clone()),
            _ => {
                seen.insert(nf, id.clone());
                kept.push(id.clone());
            }
        }
    }
    (kept, dups)
}
/// At most `cap` Claims per Source (0 = unlimited): one Source cannot crowd out the page.
pub fn cap_per_source(ordered: Vec<String>, source_of: &HashMap<String, String>, cap: usize) -> (Vec<String>, usize) {
    if cap == 0 {
        return (ordered, 0);
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    let mut kept = Vec::new();
    let mut suppressed = 0;
    let mut seen: HashSet<String> = HashSet::new();
    for id in ordered {
        let source = source_of.get(&id).map(String::as_str).unwrap_or("");
        let n = count.entry(source).or_default();
        if *n < cap && seen.insert(id.clone()) {
            *n += 1;
            kept.push(id);
        } else {
            suppressed += 1;
        }
    }
    (kept, suppressed)
}
