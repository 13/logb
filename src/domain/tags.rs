//! Free-text tags on objects and entries.
//!
//! Stored as a JSON array of strings in a `tags` TEXT column. Every write goes through
//! `normalize` first, so a reader can trust the shape; `from_json` still tolerates bad text
//! because a hand-edited database or a legacy row must not make a read fail.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

pub const MAX_TAGS: usize = 10;
pub const MAX_TAG_CHARS: usize = 32;

/// The comparison key for a tag: lower case with accents removed, so "Fahrräder" and
/// "FAHRRADER" are one tag. NFD splits "ä" into "a" plus a combining mark, which is then
/// dropped. `unicode-normalization` was already in the build through sqlx, so this costs no
/// new download.
pub fn fold(tag: &str) -> String {
    tag.nfd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase()
}

/// Trims each tag, collapses whitespace runs, drops empty tags and later duplicates (equal
/// under `fold`, first spelling kept), then enforces the limits. `Err` is the sentence a 400
/// answers with.
pub fn normalize(input: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for raw in input {
        let tag = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(format!("a tag can be at most {MAX_TAG_CHARS} characters long"));
        }
        let key = fold(&tag);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(tag);
    }
    // Counted after dropping duplicates: "Winter, winter" is one tag, not two.
    if out.len() > MAX_TAGS {
        return Err(format!("an object or entry can carry at most {MAX_TAGS} tags"));
    }
    Ok(out)
}

pub fn to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".into())
}

pub fn from_json(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}

/// Serialises a `tags` column (JSON text) as the array it holds, so every response carries
/// `tags: string[]` whichever struct holds the row.
pub fn serialize_json_text<S: serde::Serializer, T: AsRef<str>>(text: &T, s: S) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    from_json(text.as_ref()).serialize(s)
}

/// Whether a stored `tags` column carries `wanted`, compared under `fold`. The caller folds
/// `wanted` once rather than per row.
pub fn carries(text: &str, folded_wanted: &str) -> bool {
    from_json(text).iter().any(|t| fold(t) == folded_wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(tags: &[&str]) -> Vec<String> { tags.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn trims_collapses_and_drops_empty() {
        assert_eq!(normalize(&v(&["  Garage   2 ", "", "   "])).unwrap(), v(&["Garage 2"]));
    }

    #[test]
    fn duplicates_ignoring_case_and_accents_keep_the_first_spelling() {
        assert_eq!(normalize(&v(&["Winter", "winter", "WINTER", "Fahrräder", "fahrrader"])).unwrap(), v(&["Winter", "Fahrräder"]));
    }

    #[test]
    fn limits_are_refused_with_a_reason() {
        let long = "x".repeat(33);
        assert!(normalize(&v(&[&long])).unwrap_err().contains("32"));
        let many: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
        assert!(normalize(&many).unwrap_err().contains("10"));
        assert!(normalize(&v(&[&"é".repeat(32)])).is_ok(), "the limit counts characters, not bytes");
    }

    #[test]
    fn folding_ignores_case_and_accents() {
        assert_eq!(fold("Élan Vital"), fold("elan vital"));
    }

    #[test]
    fn json_round_trips_and_bad_text_reads_empty() {
        let tags = v(&["a \"quoted\" tag", "Zweite"]);
        assert_eq!(from_json(&to_json(&tags)), tags);
        assert!(from_json("not json").is_empty());
        assert!(from_json("").is_empty());
    }
}
