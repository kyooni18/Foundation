//! Exact brute-force cosine search over persisted vectors, isolated by embedding model identity.
//!
//! Vectors are a derived index. The in-memory copy holds them in f32 for a fast scan and is
//! refreshed from `claim_vec`; it never caches authorization. A scan scores every vector, then
//! the best candidates are verified against the caller's projection (and filters) in SQL, doubling
//! the window until enough visible candidates are found or the corpus is exhausted. A hidden
//! vector therefore never changes a visible Claim's score and never counts toward anything.
use super::{access, queue::decode};
use crate::domain::*;
use rusqlite::{Connection, params};
use std::collections::{HashMap, VecDeque};

#[derive(Default)]
pub struct VectorCache {
    key: String,
    dim: usize,
    ids: Vec<String>,
    flat: Vec<f32>,
    max_vec_id: i64,
}
impl VectorCache {
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.flat.len() * 4
    }
    /// Bring the cache in line with the table; appends new rows incrementally when possible.
    pub(crate) fn refresh(&mut self, conn: &Connection, key: &str, dim: usize) -> Result<()> {
        let (count, max): (i64, i64) = conn.query_row(
            "SELECT count(*),COALESCE(max(vec_id),0) FROM claim_vec WHERE model_key=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if self.key == key
            && self.dim == dim
            && self.ids.len() as i64 == count
            && self.max_vec_id == max
        {
            return Ok(());
        }
        let newer: i64 = if self.key == key && self.dim == dim {
            conn.query_row(
                "SELECT count(*) FROM claim_vec WHERE model_key=?1 AND vec_id>?2",
                params![key, self.max_vec_id],
                |r| r.get(0),
            )?
        } else {
            -1
        };
        let after = if newer >= 0 && self.ids.len() as i64 + newer == count {
            self.max_vec_id
        } else {
            *self = Self {
                key: key.into(),
                dim,
                ..Default::default()
            };
            0
        };
        let mut stmt = conn.prepare_cached("SELECT vec_id,claim_id,vec FROM claim_vec WHERE model_key=?1 AND vec_id>?2 ORDER BY vec_id")?;
        let mut rows = stmt.query(params![key, after])?;
        while let Some(row) = rows.next()? {
            let blob: Vec<u8> = row.get(2)?;
            let vector = decode(&blob);
            if vector.len() != dim {
                return Err(Error::IntegrityFailure("vector dimensions".into()));
            }
            self.max_vec_id = row.get(0)?;
            self.ids.push(row.get(1)?);
            self.flat.extend_from_slice(&vector);
        }
        Ok(())
    }
    fn scores(&self, query: &[f32]) -> Vec<f32> {
        let n = self.ids.len();
        let mut out = vec![0f32; n];
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        let chunk = n.div_ceil(threads).max(1);
        std::thread::scope(|s| {
            for (t, slice) in out.chunks_mut(chunk).enumerate() {
                s.spawn(move || {
                    for (i, o) in slice.iter_mut().enumerate() {
                        let row = &self.flat[(t * chunk + i) * self.dim..][..self.dim];
                        *o = crate::embedding::dot(query, row);
                    }
                });
            }
        });
        out
    }
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// (claim id, cosine) best first, visible and filtered.
    pub hits: Vec<(String, f32)>,
    pub scanned: usize,
    /// Vectors examined by the visibility window (>= hits).
    pub window: usize,
}

/// Top `want` visible candidates whose cosine is at least `min_score`.
pub(crate) fn search(
    conn: &Connection,
    cache: &VectorCache,
    p: &Principal,
    query: &[f32],
    filter_sql: &str,
    want: usize,
    min_score: f32,
) -> Result<Outcome> {
    let mut out = Outcome {
        scanned: cache.len(),
        ..Default::default()
    };
    if cache.is_empty() || query.len() != cache.dim {
        return Ok(out);
    }
    let scores = cache.scores(query);
    let mut order: Vec<usize> = (0..scores.len())
        .filter(|&i| scores[i] >= min_score)
        .collect();
    let by_score = |a: &usize, b: &usize| {
        scores[*b]
            .total_cmp(&scores[*a])
            .then_with(|| cache.ids[*a].cmp(&cache.ids[*b]))
    };
    let col = access::column(p);
    let mut window = (want * 4).max(64);
    loop {
        let take = window.min(order.len());
        if take < order.len() {
            order.select_nth_unstable_by(take - 1, by_score);
        }
        let mut head: Vec<usize> = order[..take].to_vec();
        head.sort_by(by_score);
        let ids: Vec<&String> = head.iter().map(|&i| &cache.ids[i]).collect();
        let json = serde_json::to_string(&ids).map_err(|_| invalid("ids", "serialization"))?;
        let visible: std::collections::HashSet<String> = conn
            .prepare_cached(&format!(
                "SELECT a.claim_id FROM claim_access a JOIN claim c ON c.id=a.claim_id JOIN claim_validity v ON v.claim_id=c.id WHERE a.{col}=1 {} AND a.claim_id IN (SELECT value FROM json_each(?1))",
                if filter_sql.is_empty() { String::new() } else { format!("AND {filter_sql}") }
            ))?
            .query_map([json], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        out.hits = head
            .iter()
            .filter(|&&i| visible.contains(&cache.ids[i]))
            .take(want)
            .map(|&i| (cache.ids[i].clone(), scores[i]))
            .collect();
        out.window = take;
        if out.hits.len() >= want || take >= order.len() {
            return Ok(out);
        }
        window = (window * 4).min(order.len());
    }
}

/// Query-embedding cache: text -> vector per model key. Contains no authorization information
/// and no results, only the deterministic embedding of the caller's own query text.
pub struct QueryCache {
    map: HashMap<(String, String), Vec<f32>>,
    order: VecDeque<(String, String)>,
    capacity: usize,
    pub hits: u64,
    pub misses: u64,
}
impl Default for QueryCache {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: 1024,
            hits: 0,
            misses: 0,
        }
    }
}
impl QueryCache {
    pub fn get(&mut self, key: &str, text: &str) -> Option<Vec<f32>> {
        let found = self.map.get(&(key.to_owned(), text.to_owned())).cloned();
        if found.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        found
    }
    pub fn put(&mut self, key: &str, text: &str, vector: Vec<f32>) {
        let k = (key.to_owned(), text.to_owned());
        if self.map.insert(k.clone(), vector).is_none() {
            self.order.push_back(k);
            while self.order.len() > self.capacity {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }
}
