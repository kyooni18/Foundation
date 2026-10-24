//! Versioned, fail-closed semantic review protocol. Model conclusions remain diagnostic.
use super::{Batch, SourceView, invalid, keys};
use crate::domain::Result;
use serde_json::Value;

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
fn quote_evidence(value: &Value, source: &str) -> Result<()> {
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
        if quote.is_empty() || source.matches(quote).count() != 1 {
            return Err(invalid(
                "schema",
                "evidence quote must identify one exact source span",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate(raw: &str, batch: &Batch, source: &SourceView) -> Result<()> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err(invalid("schema", "review too large"));
    }
    let v = super::json::parse(raw).map_err(|_| invalid("schema", "invalid review JSON"))?;
    keys(&v, &["verdict", "uncertainty_flags", "claims", "inventory"])?;
    let verdict = status(&v["verdict"], false)?;
    let flags = v["uncertainty_flags"]
        .as_array()
        .ok_or_else(|| invalid("schema", "uncertainty flags required"))?;
    if flags.iter().any(|f| f.as_str().is_none_or(str::is_empty)) {
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
        let dimensions = &claim["dimensions"];
        keys(
            dimensions,
            &[
                "atomicity",
                "entailment",
                "attribution",
                "polarity_scope",
                "missing_argument",
                "unsupported_inference",
                "korean_meaning",
            ],
        )?;
        for key in [
            "atomicity",
            "entailment",
            "attribution",
            "polarity_scope",
            "missing_argument",
            "unsupported_inference",
            "korean_meaning",
        ] {
            keys(&dimensions[key], &["status", "reason"])?;
            if dimensions[key]["reason"].as_str().is_none_or(str::is_empty) {
                return Err(invalid("schema", "dimension explanation required"));
            }
            let s = status(&dimensions[key]["status"], key == "korean_meaning")?;
            if key == "korean_meaning"
                && s == "not_applicable"
                && (candidate.lang == "ko"
                    || source
                        .content
                        .chars()
                        .any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c)))
            {
                return Err(invalid(
                    "schema",
                    "Korean source dependency was not assessed",
                ));
            }
            rejects |= s == "reject";
            unresolved |= s == "unresolved";
        }
        let local_flags = claim["uncertainty_flags"]
            .as_array()
            .ok_or_else(|| invalid("schema", "claim uncertainty flags required"))?;
        if local_flags
            .iter()
            .any(|f| f.as_str().is_none_or(str::is_empty))
        {
            return Err(invalid("schema", "invalid claim uncertainty flag"));
        }
        unresolved |= !local_flags.is_empty();
    }
    let inventory = v["inventory"]
        .as_array()
        .ok_or_else(|| invalid("schema", "source inventory required"))?;
    if inventory.is_empty() && !source.content.trim().is_empty() {
        return Err(invalid("schema", "empty source inventory"));
    }
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
        if row["proposition"].as_str().is_none_or(str::is_empty)
            || row["reason"].as_str().is_none_or(str::is_empty)
        {
            return Err(invalid("schema", "inventory explanation required"));
        }
        quote_evidence(&row["evidence"], &source.content)?;
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
        let s = status(&row["status"], false)?;
        if s == "pass" && indices.is_empty() {
            return Err(invalid("schema", "passing inventory item has no Claim"));
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
        Ok(())
    } else if verdict == "unresolved" {
        Err(invalid("unresolved", "review cannot establish compliance"))
    } else {
        Err(invalid("semantic", "review rejected"))
    }
}
