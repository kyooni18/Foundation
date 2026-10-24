//! Versioned, fail-closed semantic review protocol. Model conclusions remain diagnostic.
use super::{Batch, SourceView, invalid, keys};
use crate::domain::Result;
use serde_json::Value;

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

fn status(value: &Value, allow_na: bool) -> Result<&str> {
    let s = value
        .as_str()
        .ok_or_else(|| invalid("schema", "review status must be text"))?;
    if matches!(s, "pass" | "reject" | "unresolved") || (allow_na && s == "not_applicable") {
        Ok(s)
    } else {
        Err(invalid("schema", "invalid review status"))
    }
}
pub(super) fn quote_evidence(value: &Value, source: &str) -> Result<()> {
    let spans = value
        .as_array()
        .ok_or_else(|| invalid("schema", "evidence array required"))?;
    if spans.is_empty() {
        return Err(invalid("schema", "source evidence required"));
    }
    for span in spans {
        keys(span, &["quote"])?;
        let quote = span["quote"]
            .as_str()
            .ok_or_else(|| invalid("schema", "quote text required"))?;
        // `matches` skips overlapping occurrences ("aaa" occurs twice in "aaaa").
        // Count every Unicode-scalar boundary before trusting a unique quotation.
        let occurrences = source
            .char_indices()
            .filter(|(start, _)| source[*start..].starts_with(quote))
            .take(2)
            .count();
        if quote.is_empty() || occurrences != 1 {
            return Err(invalid(
                "schema",
                "evidence quote must identify one exact source span",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate(
    raw: &str,
    batch: &Batch,
    source: &SourceView,
    require_meta: bool,
    canonical_inventory: bool,
    require_duplicates: bool,
    strict_entailment: bool,
) -> Result<()> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err(invalid("schema", "review too large"));
    }
    let v = super::json::parse(raw).map_err(|_| invalid("schema", "invalid review JSON"))?;
    keys(&v, &["verdict", "uncertainty_flags", "claims", "inventory"])?;
    let verdict = status(&v["verdict"], false)?;
    let flags = v["uncertainty_flags"]
        .as_array()
        .ok_or_else(|| invalid("schema", "uncertainty flags required"))?;
    if flags.iter().any(|f| f.as_str().is_none_or(blank)) {
        return Err(invalid("schema", "invalid uncertainty flag"));
    }
    let claims = v["claims"]
        .as_array()
        .ok_or_else(|| invalid("schema", "review claims required"))?;
    if claims.len() != batch.claims.len() {
        return Err(invalid("schema", "review omitted claims"));
    }
    let mut rejects = false;
    let mut unresolved = !flags.is_empty();
    for (i, (claim, candidate)) in claims.iter().zip(&batch.claims).enumerate() {
        keys(
            claim,
            &[
                "index",
                "text",
                "evidence",
                "dimensions",
                "uncertainty_flags",
            ],
        )?;
        if claim["index"].as_u64() != Some(i as u64)
            || claim["text"].as_str() != Some(candidate.text.as_str())
        {
            return Err(invalid("schema", "review changed claim identity"));
        }
        quote_evidence(&claim["evidence"], &source.content)?;
        if canonical_inventory && claim["evidence"] != serde_json::json!([{"quote":source.content}])
        {
            return Err(invalid("schema", "complete original context required"));
        }
        let dimensions = &claim["dimensions"];
        let mut required = vec![
            "atomicity",
            "entailment",
            "attribution",
            "polarity_scope",
            "missing_argument",
            "unsupported_inference",
            "korean_meaning",
        ];
        if require_meta {
            required.push("source_meta_expansion");
        }
        if require_duplicates {
            required.push("duplicates");
        }
        keys(dimensions, &required)?;
        for key in required {
            keys(&dimensions[key], &["status", "reason"])?;
            if dimensions[key]["reason"].as_str().is_none_or(blank) {
                return Err(invalid("schema", "dimension explanation required"));
            }
            let s = status(&dimensions[key]["status"], key == "korean_meaning")?;
            if key == "korean_meaning"
                && s == "not_applicable"
                && (candidate.lang == "ko" || super::korean(&source.content))
            {
                return Err(invalid(
                    "schema",
                    "Korean source dependency was not assessed",
                ));
            }
            rejects |= s == "reject";
            unresolved |= s == "unresolved";
        }
        // Entailment assesses the complete Claim, including every added qualifier.
        if strict_entailment
            && dimensions["entailment"]["status"] == "pass"
            && (dimensions["unsupported_inference"]["status"] == "reject"
                || dimensions["source_meta_expansion"]["status"] == "reject")
        {
            return Err(invalid("schema", "contradictory complete-claim entailment"));
        }
        let local_flags = claim["uncertainty_flags"]
            .as_array()
            .ok_or_else(|| invalid("schema", "claim uncertainty flags required"))?;
        if local_flags.iter().any(|f| f.as_str().is_none_or(blank)) {
            return Err(invalid("schema", "invalid claim uncertainty flag"));
        }
        unresolved |= !local_flags.is_empty();
    }
    let inventory = v["inventory"]
        .as_array()
        .ok_or_else(|| invalid("schema", "source inventory required"))?;
    if inventory.is_empty()
        && (!batch.claims.is_empty() || batch.empty_reason.as_deref().is_none_or(blank))
    {
        return Err(invalid(
            "schema",
            "empty inventory requires a valid empty extraction",
        ));
    }
    let mut covered = std::collections::HashSet::new();
    let mut inventory_entries = std::collections::HashSet::new();
    for row in inventory {
        keys(
            row,
            &[
                "proposition",
                "evidence",
                "claim_indices",
                "status",
                "reason",
            ],
        )?;
        if row["proposition"].as_str().is_none_or(blank) || row["reason"].as_str().is_none_or(blank)
        {
            return Err(invalid("schema", "inventory explanation required"));
        }
        quote_evidence(&row["evidence"], &source.content)?;
        if canonical_inventory {
            if row["evidence"] != serde_json::json!([{"quote":source.content}]) {
                return Err(invalid("schema", "complete inventory context required"));
            }
            quote_evidence(
                &serde_json::json!([{"quote":row["proposition"]}]),
                &source.content,
            )?;
        }
        let indices = row["claim_indices"]
            .as_array()
            .ok_or_else(|| invalid("schema", "inventory claim indices required"))?;
        let mut seen = std::collections::HashSet::new();
        for index in indices {
            let n = index
                .as_u64()
                .ok_or_else(|| invalid("schema", "invalid inventory index"))?;
            if n >= batch.claims.len() as u64 || !seen.insert(n) {
                return Err(invalid("schema", "invalid inventory index"));
            }
        }
        // A different explanation or reordered support does not identify another
        // Source proposition. Compare copies only; retain raw review evidence.
        let mut quotes: Vec<_> = row["evidence"]
            .as_array()
            .expect("validated evidence")
            .iter()
            .map(|span| span["quote"].as_str().expect("validated quote").to_owned())
            .collect();
        quotes.sort();
        quotes.dedup();
        let mut mapped: Vec<_> = seen.iter().copied().collect();
        mapped.sort_unstable();
        let identity = (
            row["proposition"]
                .as_str()
                .expect("validated proposition")
                .to_owned(),
            quotes,
            mapped,
        );
        if !inventory_entries.insert(identity) {
            return Err(invalid("schema", "duplicate source inventory entry"));
        }
        let s = status(&row["status"], false)?;
        if s == "pass" && indices.is_empty() {
            return Err(invalid("schema", "passing inventory item has no Claim"));
        }
        if s == "pass" {
            covered.extend(seen);
        }
        rejects |= s == "reject";
        unresolved |= s == "unresolved";
    }
    let expected = if rejects {
        "reject"
    } else if unresolved {
        "unresolved"
    } else {
        "pass"
    };
    if verdict != expected {
        return Err(invalid("schema", "contradictory aggregate review verdict"));
    }
    if verdict == "pass" {
        if covered.len() != batch.claims.len() {
            return Err(invalid(
                "schema",
                "passing inventory omitted a candidate Claim",
            ));
        }
        let mut mapped = std::collections::HashSet::new();
        let mut propositions = std::collections::HashSet::new();
        if inventory.len() != batch.claims.len()
            || inventory.iter().any(|row| {
                let indices = row["claim_indices"].as_array().expect("validated indices");
                indices.len() != 1
                    || !mapped.insert(indices[0].as_u64().expect("validated index"))
                    || !propositions
                        .insert(row["proposition"].as_str().expect("validated proposition"))
            })
        {
            return Err(invalid(
                "schema",
                "passing inventory must map propositions and Claims one-to-one",
            ));
        }
        Ok(())
    } else if verdict == "unresolved" {
        Err(invalid("unresolved", "review cannot establish compliance"))
    } else {
        Err(invalid("semantic", "review rejected"))
    }
}

#[cfg(test)]
mod tests {
    use super::quote_evidence;
    use serde_json::json;

    #[test]
    fn overlapping_exact_quotations_are_ambiguous() {
        assert!(quote_evidence(&json!([{"quote":"aaa"}]), "aaaa").is_err());
        assert!(quote_evidence(&json!([{"quote":"é"}]), "👩🏽‍💻é").is_ok());
    }
}
