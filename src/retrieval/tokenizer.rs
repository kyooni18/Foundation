//! Lexical tokenisation (derived-index version `VERSION`). Character bigrams for Hangul, Han and
//! kana runs (no Korean morphological analysis: particle stripping and trigrams measured worse in
//! the R3 experiments) and whole ASCII words/identifiers plus their parts. Stored text is never
//! normalised; only the index and the query are folded:
//! - conjoining Hangul jamo are composed into syllables (NFD input matches NFC input),
//! - full-width ASCII is mapped to ASCII, and letters are lower-cased.
//!
//! The same function builds index terms and query terms, so exact identifiers, file paths, code
//! symbols and numbers are matched as whole tokens.
pub const VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Two adjacent CJK characters.
    Bigram,
    /// A one-character CJK run (also used as a prefix term for one-character queries).
    Unigram,
    /// Alphanumeric word without joiners.
    Word,
    /// Token containing joiners (`src/main.rs`, `user_id`, `2026-10-25`) or mixed case/digits.
    Identifier,
    /// A part of an identifier (camelCase, joiner or letter/digit boundary).
    Part,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Term {
    pub text: String,
    pub kind: Kind,
}

/// Scripts present in a text; used by the lexical gate (lexical evidence only counts when it
/// shares a script with the query).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scripts {
    pub hangul: bool,
    pub han: bool,
    pub kana: bool,
    pub latin: bool,
    pub other_letters: bool,
}
impl Scripts {
    pub fn of(text: &str) -> Self {
        let mut s = Self::default();
        for c in text.chars() {
            match class(c) {
                Class::Cjk(Cjk::Hangul) => s.hangul = true,
                Class::Cjk(Cjk::Han) => s.han = true,
                Class::Cjk(Cjk::Kana) => s.kana = true,
                Class::Word if c.is_ascii() => s.latin |= c.is_ascii_alphabetic(),
                // Latin-extended letters count as Latin; other alphabetic scripts as "other".
                Class::Word if c.is_alphabetic() => {
                    if matches!(c as u32, 0xC0..=0x24F) {
                        s.latin = true
                    } else {
                        s.other_letters = true
                    }
                }
                _ => (),
            }
        }
        s
    }
    pub fn overlaps(self, other: Self) -> bool {
        (self.hangul && other.hangul)
            || (self.han && other.han)
            || (self.kana && other.kana)
            || (self.latin && other.latin)
            || (self.other_letters && other.other_letters)
    }
    pub fn any(self) -> bool {
        self.hangul || self.han || self.kana || self.latin || self.other_letters
    }
    pub fn mixed(self) -> bool {
        [
            self.hangul || self.han || self.kana,
            self.latin,
            self.other_letters,
        ]
        .iter()
        .filter(|b| **b)
        .count()
            > 1
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cjk {
    Hangul,
    Han,
    Kana,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Cjk(Cjk),
    Word,
    Joiner,
    Separator,
}
fn class(c: char) -> Class {
    match c as u32 {
        0xAC00..=0xD7A3 | 0x3131..=0x318E | 0x1100..=0x11FF => Class::Cjk(Cjk::Hangul),
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF => Class::Cjk(Cjk::Han),
        0x3040..=0x30FF | 0x31F0..=0x31FF => Class::Cjk(Cjk::Kana),
        _ if c.is_alphanumeric() => Class::Word,
        _ if "_-./:#@+".contains(c) => Class::Joiner,
        _ => Class::Separator,
    }
}

/// Compose jamo, fold full-width ASCII. Case is preserved here (camelCase splitting needs it).
pub fn fold(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let u = c as u32;
        if (0x1100..=0x1112).contains(&u)
            && i + 1 < chars.len()
            && (0x1161..=0x1175).contains(&(chars[i + 1] as u32))
        {
            let (l, v) = (u - 0x1100, chars[i + 1] as u32 - 0x1161);
            let mut t = 0;
            let mut used = 2;
            if i + 2 < chars.len() && (0x11A8..=0x11C2).contains(&(chars[i + 2] as u32)) {
                t = chars[i + 2] as u32 - 0x11A7;
                used = 3;
            }
            out.push(char::from_u32(0xAC00 + (l * 21 + v) * 28 + t).expect("syllable"));
            i += used;
            continue;
        }
        out.push(match u {
            0xFF01..=0xFF5E => char::from_u32(u - 0xFEE0).expect("ascii"),
            0x3000 => ' ',
            _ => c,
        });
        i += 1;
    }
    out
}

fn split_parts(word: &str) -> Vec<String> {
    // Boundaries: joiners, lower->upper, letter<->digit, and an upper run followed by lower
    // ("HTTPServer" -> "HTTP", "Server").
    let chars: Vec<char> = word.chars().collect();
    let mut parts = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if matches!(class(c), Class::Joiner) {
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some(prev) = cur.chars().last() {
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let boundary = (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_ascii_digit() != c.is_ascii_digit()
                    && c.is_alphanumeric()
                    && prev.is_alphanumeric())
                || (prev.is_uppercase() && c.is_uppercase() && next_lower);
            if boundary {
                parts.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// All terms of a text in document order (duplicates preserved so term frequency is available).
pub fn terms(text: &str) -> Vec<Term> {
    let folded = fold(text);
    let chars: Vec<char> = folded.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match class(chars[i]) {
            Class::Cjk(_) => {
                let start = i;
                while i < chars.len() && matches!(class(chars[i]), Class::Cjk(_)) {
                    i += 1;
                }
                let run = &chars[start..i];
                if run.len() == 1 {
                    out.push(Term {
                        text: run[0].to_lowercase().collect(),
                        kind: Kind::Unigram,
                    });
                }
                for pair in run.windows(2) {
                    out.push(Term {
                        text: pair.iter().collect(),
                        kind: Kind::Bigram,
                    });
                }
            }
            Class::Word => {
                let start = i;
                // Joiners are part of the token only between word characters.
                while i < chars.len() {
                    match class(chars[i]) {
                        Class::Word => i += 1,
                        Class::Joiner
                            if i + 1 < chars.len()
                                && i > start
                                && matches!(class(chars[i + 1]), Class::Word) =>
                        {
                            i += 1
                        }
                        _ => break,
                    }
                }
                let raw: String = chars[start..i].iter().collect();
                let lower = raw.to_lowercase();
                let parts = split_parts(&raw);
                let plain = parts.len() == 1 && lower == parts[0].to_lowercase();
                out.push(Term {
                    text: lower.clone(),
                    kind: if plain { Kind::Word } else { Kind::Identifier },
                });
                if !plain {
                    for part in parts {
                        let part = part.to_lowercase();
                        if part != lower {
                            out.push(Term {
                                text: part,
                                kind: Kind::Part,
                            });
                        }
                    }
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Terms stored in the index (space separated; every term is made of FTS token characters).
pub fn index_text(text: &str) -> (String, usize) {
    let t = terms(text);
    (
        t.iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        t.len(),
    )
}

/// Distinct query terms. A one-character CJK query term is a prefix term (matches every bigram
/// beginning with that character). Duplicates collapse.
pub fn query_terms(text: &str) -> Vec<Term> {
    let mut seen = std::collections::HashSet::new();
    terms(text)
        .into_iter()
        .filter(|t| seen.insert(t.text.clone()))
        .collect()
}
pub fn is_prefix(t: &Term) -> bool {
    t.kind == Kind::Unigram
}
/// FTS5 MATCH expression for one term (always quoted; prefix for one-character CJK terms).
pub fn fts_term(t: &Term) -> String {
    let quoted = format!("\"{}\"", t.text.replace('"', "\"\""));
    if is_prefix(t) {
        format!("{quoted}*")
    } else {
        quoted
    }
}

/// Words that carry no retrieval signal for query-coverage decisions (not used for ranking:
/// ranking relies on corpus statistics inside the caller's projection).
pub fn is_function_word(w: &str) -> bool {
    const EN: &[&str] = &[
        "a", "an", "the", "of", "to", "in", "on", "at", "for", "and", "or", "is", "are", "was",
        "were", "be", "been", "do", "does", "did", "i", "my", "me", "we", "our", "you", "your",
        "it", "its", "that", "this", "with", "as", "by", "from", "what", "which", "who", "when",
        "where", "how", "why", "can", "could", "should", "would", "have", "has", "had", "not",
        "no", "any", "all", "about",
    ];
    EN.contains(&w)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn texts(s: &str) -> Vec<String> {
        terms(s).into_iter().map(|t| t.text).collect()
    }
    #[test]
    fn bigrams_words_identifiers_and_edge_cases() {
        assert_eq!(texts("서버는 Rust"), ["서버", "버는", "rust"]);
        assert_eq!(texts("물"), ["물"]);
        assert!(texts("src/main.rs").starts_with(&[
            "src/main.rs".to_string(),
            "src".into(),
            "main".into(),
            "rs".into()
        ]));
        assert_eq!(texts("getUserName"), ["getusername", "get", "user", "name"]);
        assert!(texts("v2.1-beta").contains(&"v2.1-beta".to_string()));
        assert_eq!(texts("2026-10-25"), ["2026-10-25", "2026", "10", "25"]);
        // NFD Hangul (conjoining jamo) equals precomposed text; full-width ASCII folds.
        assert_eq!(texts("\u{1112}\u{1161}\u{11AB}"), texts("한"));
        assert_eq!(texts("ＡＢＣ１２３"), ["abc123", "abc", "123"]);
        assert!(texts("🙂 ... !!").is_empty());
        assert_eq!(
            fts_term(&Term {
                text: "a\"b".into(),
                kind: Kind::Word
            }),
            "\"a\"\"b\""
        );
    }
}
