use crate::{
    App,
    authentication::{hex, unhex},
    authz,
    domain::*,
    store,
};
use hmac::{Hmac, Mac};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSources {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub include_retracted: bool,
    pub cursor: Option<String>,
}
fn default_limit() -> u32 {
    50
}
impl Default for ListSources {
    fn default() -> Self {
        Self {
            limit: 50,
            include_retracted: false,
            cursor: None,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SourcePage {
    pub sources: Vec<SourceView>,
    pub total: u64,
    pub next_cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    after: String,
    include_retracted: bool,
}
impl App {
    fn cursor_mac(&self, p: &Principal, body: &[u8]) -> Hmac<Sha256> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.cursor_key).expect("HMAC accepts any key");
        mac.update(p.kind.as_str().as_bytes());
        mac.update(&p.credential.unwrap_or([0; 32]));
        mac.update(body);
        mac
    }
    fn cursor_encode(&self, p: &Principal, pos: Position) -> Result<String> {
        let bytes = serde_json::to_vec(&pos).map_err(|_| Error::InvalidCursor)?;
        let signature = self.cursor_mac(p, &bytes).finalize().into_bytes();
        Ok(format!("{}.{}", hex(&bytes), hex(&signature)))
    }
    fn cursor_decode(&self, p: &Principal, cursor: &str, include: bool) -> Result<String> {
        if cursor.len() > 1024 {
            return Err(Error::InvalidCursor);
        }
        let (body, signature) = cursor.split_once('.').ok_or(Error::InvalidCursor)?;
        let bytes = unhex(body).ok_or(Error::InvalidCursor)?;
        let signature = unhex(signature).ok_or(Error::InvalidCursor)?;
        self.cursor_mac(p, &bytes)
            .verify_slice(&signature)
            .map_err(|_| Error::InvalidCursor)?;
        let pos: Position = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidCursor)?;
        if pos.include_retracted != include || pos.after.parse::<SourceId>().is_err() {
            return Err(Error::InvalidCursor);
        }
        Ok(pos.after)
    }
    pub fn list_sources(&self, p: &Principal, request: ListSources) -> Result<SourcePage> {
        if !(1..=100).contains(&request.limit) {
            return Err(invalid("limit", "must be 1..=100"));
        }
        let after = match &request.cursor {
            Some(c) => self.cursor_decode(p, c, request.include_retracted)?,
            None => String::new(),
        };
        let mut conn = self.store.reader()?;
        let tx = conn.transaction()?;
        let predicate = authz::source_predicate(p, request.include_retracted);
        // All aggregates and LIMIT are evaluated inside the projection.
        let visible = format!(
            "WITH projected AS (SELECT id,recorder,COALESCE((SELECT event FROM status_event WHERE target_kind='source' AND target_id=source.id ORDER BY seq DESC LIMIT 1),'restore') AS lifecycle_event FROM source) SELECT id FROM projected WHERE {predicate}"
        );
        let total = tx.query_row(&format!("SELECT count(*) FROM ({visible})"), [], |r| {
            r.get(0)
        })?;
        let ids: Vec<String> = tx
            .prepare(&format!("{visible} AND id>?1 ORDER BY id LIMIT ?2"))?
            .query_map(params![after, request.limit + 1], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let more = ids.len() > request.limit as usize;
        let sources: Vec<_> = ids
            .iter()
            .take(request.limit as usize)
            .map(|id| store::get(&tx, &id.parse()?))
            .collect::<Result<_>>()?;
        let next_cursor = if more {
            Some(self.cursor_encode(
                p,
                Position {
                    after: sources.last().expect("positive page limit").id.to_string(),
                    include_retracted: request.include_retracted,
                },
            )?)
        } else {
            None
        };
        tx.commit()?;
        Ok(SourcePage {
            sources,
            total,
            next_cursor,
        })
    }
}
