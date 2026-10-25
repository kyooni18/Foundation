//! Deterministic, explainable query planning. No generative model: the plan is a pure function
//! of the query text, the caller's hints and the request shape. Hints are advisory and never
//! authorization; the plan lists every signal that produced it and every uncertainty, and when
//! intent is ambiguous it chooses complementary paths rather than guessing one.
use super::tokenizer::{self, Kind, Scripts};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Exact,
    Lexical,
    Semantic,
    Relational,
    Temporal,
    Comprehensive,
    #[default]
    Auto,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    Focused,
    Balanced,
    Exhaustive,
    #[default]
    Auto,
}

/// What the query looks like, independent of what the caller asked for.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Signals {
    pub quoted: Vec<String>,
    pub identifiers: Vec<String>,
    pub numbers: Vec<String>,
    pub time_cues: Vec<String>,
    pub relation_cues: Vec<String>,
    pub exhaustive_cues: Vec<String>,
    pub tokens: usize,
    pub natural_language: bool,
    pub question: bool,
    pub mixed_script: bool,
    pub cjk: bool,
    pub latin: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct PathPlan {
    /// exact | lexical | semantic | graph | temporal
    pub path: &'static str,
    /// always | if_insufficient
    pub when: &'static str,
    pub reason: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct Budgets {
    pub lexical_candidates: usize,
    pub semantic_candidates: usize,
    pub graph_hops: usize,
    pub graph_nodes: usize,
    /// Claims per Source in one result page (0 = unlimited).
    pub per_source: usize,
    pub deadline_ms: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub intent: Intent,
    pub depth: Depth,
    pub requested_intent: Intent,
    pub requested_depth: Depth,
    pub signals: Signals,
    pub paths: Vec<PathPlan>,
    pub budgets: Budgets,
    /// Optional paths are skipped once this many strong results exist.
    pub sufficient_results: usize,
    pub stop: &'static str,
    pub fallback: &'static str,
    pub uncertainty: Vec<&'static str>,
}

const TIME_EN: &[&str] = &[
    "yesterday",
    "today",
    "tomorrow",
    "last",
    "ago",
    "before",
    "after",
    "since",
    "until",
    "during",
    "when",
    "previously",
    "earlier",
    "later",
    "history",
    "changed",
    "formerly",
    "originally",
    "currently",
    "now",
    "then",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "as",
    "of",
];
const TIME_KO: &[&str] = &[
    "어제",
    "오늘",
    "내일",
    "지난",
    "작년",
    "올해",
    "이전",
    "이후",
    "당시",
    "그때",
    "예전",
    "과거",
    "현재",
    "변경",
    "바뀌",
    "언제",
    "월",
    "년",
    "주전",
    "개월",
    "이력",
    "히스토리",
];
const REL_EN: &[&str] = &[
    "because",
    "depends",
    "dependency",
    "dependencies",
    "related",
    "relationship",
    "cited",
    "citation",
    "source",
    "evidence",
    "supports",
    "supersedes",
    "contradicts",
    "derived",
    "caused",
    "reason",
    "why",
    "premise",
    "justification",
    "origin",
];
const REL_KO: &[&str] = &[
    "때문", "의존", "관련", "근거", "출처", "인용", "대체", "모순", "이유", "전제", "원인", "배경",
    "파생",
];
const ALL_EN: &[&str] = &[
    "all",
    "every",
    "everything",
    "list",
    "complete",
    "entire",
    "exhaustive",
    "each",
    "count",
];
const ALL_KO: &[&str] = &[
    "모든",
    "전부",
    "전체",
    "목록",
    "빠짐없이",
    "모두",
    "몇",
    "개수",
    "나열",
];
const QUESTION_EN: &[&str] = &[
    "what", "which", "who", "whom", "where", "when", "how", "why", "is", "are", "do", "does",
    "did", "can", "could", "should",
];

fn quoted_phrases(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let pairs = [
        ('"', '"'),
        ('“', '”'),
        ('「', '」'),
        ('『', '』'),
        ('\'', '\''),
    ];
    let chars: Vec<char> = q.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some(&(_, close)) = pairs.iter().find(|(o, _)| *o == chars[i]) {
            // A straight apostrophe inside a word is not a quotation mark.
            let word_internal = chars[i] == '\''
                && i > 0
                && chars[i - 1].is_alphanumeric()
                && chars.get(i + 1).is_some_and(|c| c.is_alphanumeric());
            if !word_internal && let Some(len) = chars[i + 1..].iter().position(|&c| c == close) {
                let phrase: String = chars[i + 1..i + 1 + len].iter().collect();
                if !phrase.trim().is_empty() {
                    out.push(phrase.trim().to_owned());
                }
                i += len + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}
fn is_ulid_like(w: &str) -> bool {
    w.len() == 26
        && w.chars().all(|c| c.is_ascii_alphanumeric())
        && w.chars().any(|c| c.is_ascii_digit())
}

pub fn analyze(query: &str, as_of: bool) -> Signals {
    let mut s = Signals {
        quoted: quoted_phrases(query),
        ..Default::default()
    };
    let scripts = Scripts::of(query);
    s.cjk = scripts.hangul || scripts.han || scripts.kana;
    s.latin = scripts.latin;
    s.mixed_script = scripts.mixed();
    let unquoted = {
        let mut rest = query.to_owned();
        for q in &s.quoted {
            rest = rest.replace(q.as_str(), " ");
        }
        rest
    };
    let lower = unquoted.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || "_-./:#@+".contains(c)))
        .filter(|w| !w.is_empty())
        .collect();
    s.tokens = unquoted.split_whitespace().count();
    for t in tokenizer::terms(&unquoted) {
        match t.kind {
            Kind::Identifier if !s.identifiers.contains(&t.text) => {
                // Dates and ordinary numbers are signals of their own; code symbols, paths and ids are exact.
                if t.text
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '-')
                    .count()
                    == t.text.len()
                    && t.text.len() >= 8
                {
                    s.time_cues.push(t.text.clone());
                } else {
                    s.identifiers.push(t.text);
                }
            }
            Kind::Word if t.text.chars().all(|c| c.is_ascii_digit()) => {
                if (t.text.len() == 4 && (t.text.starts_with("19") || t.text.starts_with("20")))
                    || t.text.len() == 8
                {
                    s.time_cues.push(t.text);
                } else if t.text.len() >= 3 {
                    s.numbers.push(t.text);
                }
            }
            _ => (),
        }
    }
    for w in &words {
        if is_ulid_like(w) && !s.identifiers.contains(&w.to_string()) {
            s.identifiers.push(w.to_string());
        }
    }
    let has = |list: &[&str], w: &str| list.contains(&w);
    let cue = |dst: &mut Vec<String>, en: &[&str], ko: &[&str], strict_en: bool| {
        for w in &words {
            let hit = (has(en, w) && (!strict_en || w.len() > 2))
                || ko.iter().any(|k| w.contains(k) && !w.is_ascii());
            if hit && !dst.contains(&w.to_string()) {
                dst.push(w.to_string());
            }
        }
    };
    // Time words that are also ordinary function words ("when", "as", "of", "then", "now") count
    // only together with another temporal cue.
    let mut time = Vec::new();
    cue(&mut time, TIME_EN, TIME_KO, true);
    let weak: &[&str] = &["when", "as", "of", "then", "now", "last", "after", "before"];
    let strong_time =
        time.iter().any(|w| !weak.contains(&w.as_str())) || !s.time_cues.is_empty() || as_of;
    if strong_time {
        for w in time {
            if !s.time_cues.contains(&w) {
                s.time_cues.push(w);
            }
        }
    }
    cue(&mut s.relation_cues, REL_EN, REL_KO, true);
    cue(&mut s.exhaustive_cues, ALL_EN, ALL_KO, false);
    s.question = query.trim_end().ends_with(['?', '？'])
        || words.first().is_some_and(|w| has(QUESTION_EN, w))
        || query.contains("무엇")
        || query.contains("어떻게")
        || query.contains("왜");
    let cjk_run = s.cjk
        && unquoted
            .chars()
            .filter(|c| tokenizer::Scripts::of(&c.to_string()).any() && !c.is_ascii())
            .count()
            >= 6;
    s.natural_language = s.question || s.tokens >= 4 || cjk_run;
    s
}

fn budgets(depth: Depth, limit: usize) -> Budgets {
    let (lex, sem, hops, nodes, per_source) = match depth {
        Depth::Focused => (500, 50, 1, 8, 3),
        Depth::Balanced | Depth::Auto => (2_000, 200, 1, 25, 5),
        Depth::Exhaustive => (20_000, 2_000, 2, 100, 0),
    };
    Budgets {
        lexical_candidates: lex.max(limit * 10),
        semantic_candidates: sem.max(limit * 4),
        graph_hops: hops,
        graph_nodes: nodes,
        per_source,
        deadline_ms: match depth {
            Depth::Focused => 1_000,
            Depth::Balanced | Depth::Auto => 3_000,
            Depth::Exhaustive => 30_000,
        },
    }
}

pub fn plan(
    query: &str,
    intent: Intent,
    depth: Depth,
    as_of: bool,
    filtered: bool,
    limit: usize,
) -> Plan {
    let signals = analyze(query, as_of);
    let exact_signal = !signals.quoted.is_empty() || !signals.identifiers.is_empty();
    let short = signals.tokens <= 3;
    let mut uncertainty: Vec<&'static str> = Vec::new();
    // Independent evidence for each interpretation.
    let temporal = as_of || !signals.time_cues.is_empty();
    let relational = !signals.relation_cues.is_empty();
    let comprehensive = !signals.exhaustive_cues.is_empty();
    let exact =
        exact_signal && (short || !signals.quoted.is_empty() || signals.identifiers.len() >= 2);
    let natural = signals.natural_language && !exact;
    let fired = [temporal, relational, exact, natural]
        .iter()
        .filter(|b| **b)
        .count();
    let inferred = if comprehensive {
        Intent::Comprehensive
    } else if exact {
        Intent::Exact
    } else if temporal {
        Intent::Temporal
    } else if relational {
        Intent::Relational
    } else if natural {
        Intent::Semantic
    } else {
        Intent::Lexical
    };
    if fired + usize::from(comprehensive) > 1 {
        uncertainty.push("multiple_intent_signals");
    }
    if !signals.natural_language && !exact_signal && !comprehensive {
        uncertainty.push("short_or_keyword_query");
    }
    if signals.mixed_script {
        uncertainty.push("mixed_script_query");
    }
    let resolved = match intent {
        Intent::Auto => inferred,
        hint => {
            let supported = match hint {
                Intent::Exact => exact_signal,
                Intent::Temporal => temporal,
                Intent::Relational => relational,
                Intent::Comprehensive => comprehensive,
                _ => true,
            };
            if !supported {
                uncertainty.push("hint_not_supported_by_query");
            }
            hint
        }
    };
    let resolved_depth = match depth {
        Depth::Auto => {
            if resolved == Intent::Comprehensive {
                Depth::Exhaustive
            } else if resolved == Intent::Exact && uncertainty.is_empty() {
                Depth::Focused
            } else {
                Depth::Balanced
            }
        }
        d => d,
    };
    let mut paths: Vec<PathPlan> = Vec::new();
    let push = |paths: &mut Vec<PathPlan>, path, when, reason| {
        if !paths.iter().any(|p| p.path == path) {
            paths.push(PathPlan { path, when, reason });
        }
    };
    match resolved {
        Intent::Exact => {
            push(
                &mut paths,
                "exact",
                "always",
                "quoted phrase or identifier in the query",
            );
            push(
                &mut paths,
                "lexical",
                "always",
                "complements exact matching with word and character-bigram evidence",
            );
            push(
                &mut paths,
                "semantic",
                "if_insufficient",
                "paraphrase fallback when exact evidence is thin",
            );
        }
        Intent::Lexical => {
            push(&mut paths, "lexical", "always", "keyword query");
            push(
                &mut paths,
                "semantic",
                "if_insufficient",
                "paraphrase or cross-language fallback",
            );
        }
        Intent::Semantic => {
            push(&mut paths, "semantic", "always", "natural-language query");
            push(
                &mut paths,
                "lexical",
                "always",
                "gated lexical evidence for exact words",
            );
        }
        Intent::Relational => {
            push(&mut paths, "lexical", "always", "locate seed Claims");
            push(
                &mut paths,
                "semantic",
                "if_insufficient",
                "seed Claims by meaning",
            );
            push(
                &mut paths,
                "graph",
                "always",
                "relationship cues: follow dependencies, citations and revisions",
            );
        }
        Intent::Temporal => {
            push(&mut paths, "lexical", "always", "locate candidate Claims");
            push(
                &mut paths,
                "semantic",
                "if_insufficient",
                "candidates by meaning",
            );
            push(
                &mut paths,
                "temporal",
                "always",
                "time cues or as_of: evaluate applicability and keep history",
            );
        }
        Intent::Comprehensive => {
            push(
                &mut paths,
                "lexical",
                "always",
                "exhaustiveness cue: widest lexical coverage",
            );
            push(
                &mut paths,
                "semantic",
                "always",
                "exhaustiveness cue: widest semantic coverage",
            );
            push(
                &mut paths,
                "graph",
                "if_insufficient",
                "expand from covered Claims",
            );
        }
        Intent::Auto => unreachable!("resolved above"),
    }
    // Ambiguity favors complementary paths over a single guess.
    if uncertainty.contains(&"multiple_intent_signals")
        || uncertainty.contains(&"mixed_script_query")
    {
        push(
            &mut paths,
            "lexical",
            "always",
            "ambiguous intent: keep lexical evidence",
        );
        push(
            &mut paths,
            "semantic",
            "always",
            "ambiguous intent: keep semantic evidence",
        );
    }
    if exact_signal && resolved != Intent::Exact {
        push(
            &mut paths,
            "exact",
            "always",
            "identifier or quoted phrase present",
        );
    }
    if temporal && resolved != Intent::Temporal {
        push(&mut paths, "temporal", "always", "time cue present");
    }
    if relational && resolved != Intent::Relational && resolved_depth != Depth::Focused {
        push(
            &mut paths,
            "graph",
            "if_insufficient",
            "relationship cue present",
        );
    }
    let _ = filtered;
    Plan {
        intent: resolved,
        depth: resolved_depth,
        requested_intent: intent,
        requested_depth: depth,
        signals,
        paths,
        budgets: budgets(resolved_depth, limit),
        sufficient_results: limit.min(10),
        stop: match resolved_depth {
            Depth::Exhaustive => "all_paths_to_budget",
            _ => "enough_strong_results_or_budget",
        },
        fallback: "lexical_only_when_semantic_unavailable",
        uncertainty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan_of(q: &str) -> Plan {
        plan(q, Intent::Auto, Depth::Auto, false, false, 10)
    }
    fn names(p: &Plan) -> Vec<&'static str> {
        p.paths.iter().map(|p| p.path).collect()
    }
    #[test]
    fn deterministic_intents_and_complementary_paths() {
        assert_eq!(plan_of("\"git rebase --onto\"").intent, Intent::Exact);
        assert_eq!(plan_of("src/main.rs").intent, Intent::Exact);
        assert_eq!(plan_of("모든 배포 목록").intent, Intent::Comprehensive);
        assert_eq!(plan_of("list all deployments").depth, Depth::Exhaustive);
        let t = plan_of("what did we decide before 2025-03-01 about the database?");
        assert!(names(&t).contains(&"temporal"));
        let r = plan_of("why does the billing service depend on the queue");
        assert_eq!(r.intent, Intent::Relational);
        assert!(names(&r).contains(&"graph"));
        let k = plan_of("서버 재시작 방법이 뭐였지?");
        assert_eq!(k.intent, Intent::Semantic);
        assert!(names(&k).contains(&"lexical") && names(&k).contains(&"semantic"));
        // A hint is honoured but flagged when the query does not support it; it grants nothing.
        let h = plan("cats", Intent::Exact, Depth::Auto, false, false, 10);
        assert_eq!(h.intent, Intent::Exact);
        assert!(h.uncertainty.contains(&"hint_not_supported_by_query"));
        assert_eq!(plan_of("cats").signals, plan_of("cats").signals);
    }
}
