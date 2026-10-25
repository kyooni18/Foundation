//! One independent responsibility per Source review; no synthesized dimensions.
use super::{Batch, SourceView, invalid, keys, review_v24::quote_evidence};
use crate::domain::Result;
use serde_json::Value;
use std::collections::HashSet;

pub(super) const DEFINITIONS: &str = include_str!("../../prompts/extraction-v39/facets.json");
pub(super) const STAGES: [&str; 9] = [
    "atomicity and independence",
    "complete-Claim entailment",
    "attribution and perspective",
    "polarity and scope",
    "argument binding",
    "unsupported inference",
    "Korean meaning",
    "Source-commentary expansion",
    "whole-batch coverage",
];
pub(super) const DEFINITIONS_41: &str = include_str!("../../prompts/extraction-v41/facets.json");
pub(super) const DEFINITIONS_40: &str = include_str!("../../prompts/extraction-v40/facets.json");
pub(super) fn spec(stage: &str) -> Result<Value> {
    spec_for(stage, DEFINITIONS)
}
pub(super) fn spec_for(stage: &str, definitions: &str) -> Result<Value> {
    let definitions: Vec<Value> = serde_json::from_str(definitions).expect("frozen facets");
    definitions
        .into_iter()
        .find(|s| s["stage"] == stage)
        .ok_or_else(|| invalid("schema", "unknown review responsibility"))
}
fn nonblank(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("schema", "nonblank review text required"))
}
fn flags(value: &Value) -> Result<bool> {
    let flags = value
        .as_array()
        .ok_or_else(|| invalid("schema", "flags required"))?;
    for flag in flags {
        nonblank(flag)?;
    }
    Ok(!flags.is_empty())
}
fn status(value: &Value, allow_na: bool) -> Result<&str> {
    let text = nonblank(value)?;
    if matches!(text, "pass" | "reject" | "unresolved") || (allow_na && text == "not_applicable") {
        Ok(text)
    } else {
        Err(invalid("schema", "invalid review status"))
    }
}
fn evidence(value: &Value, source: &SourceView) -> Result<()> {
    quote_evidence(value, &source.content)?;
    if value != &serde_json::json!([{"quote":source.content}]) {
        return Err(invalid("schema", "complete original context required"));
    }
    Ok(())
}

pub(super) fn validate(
    raw: &str,
    stage: &str,
    batch: &Batch,
    source: &SourceView,
    violation_key: &str,
    single_mapping: bool,
    definitions: &str,
) -> Result<()> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err(invalid("schema", "review too large"));
    }
    let v = super::json::parse(raw).map_err(|_| invalid("schema", "invalid review JSON"))?;
    keys(
        &v,
        &[
            "stage",
            "verdict",
            "uncertainty_flags",
            "claims",
            "inventory",
        ],
    )?;
    if v["stage"] != stage {
        return Err(invalid("schema", "review responsibility changed"));
    }
    let definition = if definitions == DEFINITIONS {
        spec(stage)?
    } else {
        spec_for(stage, definitions)?
    };
    let required: Vec<&str> = definition["dimensions"]
        .as_array()
        .expect("dimensions")
        .iter()
        .map(|v| v.as_str().expect("dimension"))
        .collect();
    let verdict = status(&v["verdict"], false)?;
    let mut unresolved = flags(&v["uncertainty_flags"])?;
    let mut rejects = false;
    let claims = v["claims"]
        .as_array()
        .ok_or_else(|| invalid("schema", "claims required"))?;
    if claims.len() != batch.claims.len() {
        return Err(invalid("schema", "review omitted claims"));
    }
    for (i, (claim, candidate)) in claims.iter().zip(&batch.claims).enumerate() {
        keys(
            claim,
            &[
                "index",
                "text",
                "evidence",
                "source_meaning",
                "candidate_meaning",
                "dimensions",
                violation_key,
                "uncertainty_flags",
            ],
        )?;
        if claim["index"].as_u64() != Some(i as u64)
            || claim["text"].as_str() != Some(candidate.text.as_str())
        {
            return Err(invalid("schema", "review changed claim identity"));
        }
        evidence(&claim["evidence"], source)?;
        nonblank(&claim["source_meaning"])?;
        nonblank(&claim["candidate_meaning"])?;
        let dimensions = &claim["dimensions"];
        keys(dimensions, &required)?;
        let mut local_failure = false;
        for dimension in &required {
            let d = &dimensions[*dimension];
            keys(d, &["status", "reason"])?;
            nonblank(&d["reason"])?;
            let s = status(&d["status"], *dimension == "korean_meaning")?;
            if s == "not_applicable" && (candidate.lang == "ko" || super::korean(&source.content)) {
                return Err(invalid(
                    "schema",
                    "Korean source dependency was not assessed",
                ));
            }
            rejects |= s == "reject";
            unresolved |= s == "unresolved";
            local_failure |= matches!(s, "reject" | "unresolved");
        }
        if definitions == DEFINITIONS_47
            && claim["dimensions"]["entailment"]["status"] == "pass"
            && ["unsupported_inference", "source_meta_expansion"]
                .iter()
                .any(|key| {
                    matches!(
                        claim["dimensions"][key]["status"].as_str(),
                        Some("reject" | "unresolved")
                    )
                })
        {
            return Err(invalid("schema", "contradictory complete-Claim entailment"));
        }
        let differences = claim[violation_key]
            .as_array()
            .ok_or_else(|| invalid("schema", "differences required"))?;
        for difference in differences {
            nonblank(difference)?;
        }
        let local_flags = flags(&claim["uncertainty_flags"])?;
        if (!differences.is_empty() || local_flags) && !local_failure {
            return Err(invalid(
                "schema",
                "passing responsibility contradicts reported differences",
            ));
        }
        unresolved |= local_flags;
    }
    let inventory = v["inventory"]
        .as_array()
        .ok_or_else(|| invalid("schema", "inventory required"))?;
    let assesses_inventory = definition["inventory"] == true;
    if !assesses_inventory && !inventory.is_empty() {
        return Err(invalid(
            "schema",
            "inventory is outside this responsibility",
        ));
    }
    if assesses_inventory
        && inventory.is_empty()
        && (!batch.claims.is_empty()
            || batch
                .empty_reason
                .as_deref()
                .is_none_or(|s| s.trim().is_empty()))
    {
        return Err(invalid(
            "schema",
            "empty inventory requires valid empty extraction",
        ));
    }
    let mut propositions = HashSet::new();
    let mut mapped = HashSet::new();
    let mut bijective = true;
    for row in inventory {
        keys(
            row,
            &[
                "proposition",
                "evidence",
                if single_mapping {
                    "claim_index"
                } else {
                    "claim_indices"
                },
                "status",
                "reason",
            ],
        )?;
        let proposition = nonblank(&row["proposition"])?;
        nonblank(&row["reason"])?;
        evidence(&row["evidence"], source)?;
        quote_evidence(&serde_json::json!([{"quote":proposition}]), &source.content)?;
        if !propositions.insert(proposition) {
            return Err(invalid("schema", "duplicate source inventory entry"));
        }
        let scalar_indices;
        let indices = if single_mapping {
            scalar_indices = if row["claim_index"].is_null() {
                vec![]
            } else {
                vec![row["claim_index"].clone()]
            };
            &scalar_indices
        } else {
            row["claim_indices"]
                .as_array()
                .ok_or_else(|| invalid("schema", "indices required"))?
        };
        let mut local = HashSet::new();
        for index in indices {
            let index = index
                .as_u64()
                .ok_or_else(|| invalid("schema", "invalid inventory index"))?;
            if index >= batch.claims.len() as u64 || !local.insert(index) {
                return Err(invalid("schema", "invalid inventory index"));
            }
            bijective &= mapped.insert(index);
        }
        bijective &= indices.len() == 1;
        let s = status(&row["status"], false)?;
        rejects |= s == "reject";
        unresolved |= s == "unresolved";
        if s == "pass" && indices.is_empty() {
            return Err(invalid("schema", "passing inventory has no Claim"));
        }
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
    if verdict == "pass"
        && assesses_inventory
        && (!bijective
            || mapped.len() != batch.claims.len()
            || inventory.len() != batch.claims.len())
    {
        return Err(invalid(
            "schema",
            "passing inventory must map propositions and Claims one-to-one",
        ));
    }
    match verdict {
        "pass" => Ok(()),
        "unresolved" => Err(invalid("unresolved", "review cannot establish compliance")),
        _ => Err(invalid("semantic", "review rejected")),
    }
}

pub(super) fn inventory_signature(raw: &str, unordered: bool) -> Result<Value> {
    let v = super::json::parse(raw).map_err(|_| invalid("schema", "invalid review JSON"))?;
    let mut rows:Vec<Value>=v["inventory"].as_array().expect("validated inventory").iter().map(|i|serde_json::json!({
        "proposition":i["proposition"],"evidence":i["evidence"],"claim_indices":if let Some(index)=i.get("claim_index") {if index.is_null() {serde_json::json!([])} else {serde_json::json!([index])}} else {i["claim_indices"].clone()},"status":i["status"]
    })).collect();
    if unordered {
        rows.sort_by_cached_key(Value::to_string);
    }
    Ok(Value::Array(rows))
}

pub(super) const DEFINITIONS_42: &str = include_str!("../../prompts/extraction-v42/facets.json");

pub(super) const DEFINITIONS_43: &str = include_str!("../../prompts/extraction-v43/facets.json");

pub(super) const DEFINITIONS_44: &str = include_str!("../../prompts/extraction-v44/facets.json");

pub(super) const DEFINITIONS_45: &str = include_str!("../../prompts/extraction-v45/facets.json");

pub(super) const DEFINITIONS_46: &str = include_str!("../../prompts/extraction-v46/facets.json");

pub(super) const DEFINITIONS_47: &str = include_str!("../../prompts/extraction-v47/facets.json");
pub(super) const STAGES_47: [&str; 3] = [
    "atomicity and independence",
    "complete-Claim entailment",
    "whole-batch coverage",
];

pub(super) const DEFINITIONS_48: &str = include_str!("../../prompts/extraction-v48/facets.json");

pub(super) const DEFINITIONS_49: &str = include_str!("../../prompts/extraction-v49/facets.json");
