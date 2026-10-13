//! Looking words up without a browser: what is taken from Yomitan to find
//! the dictionary form of the word a text starts with (`candidates`), the
//! dictionaries the reader imports (`store`, `import`) and the lookup that
//! asks them (`lookup`).

pub mod deinflect;
pub mod import;
pub mod lookup;
pub mod store;
pub mod variants;

use std::sync::OnceLock;

use serde_json::Value;

use deinflect::Transformer;
use variants::Tables;

/// Yomitan's Japanese rules and character tables, by tools/yomitan-dump.mjs.
const JAPANESE: &str = include_str!("japanese.json");

pub struct Japanese {
    pub transformer: Transformer,
    pub tables: Tables,
}

/// The rules, read once. They are part of the program, so they can be read.
pub fn japanese() -> &'static Japanese {
    static READ: OnceLock<Japanese> = OnceLock::new();
    READ.get_or_init(|| {
        let data: Value = serde_json::from_str(JAPANESE).expect("the rules are JSON");
        Japanese { transformer: Transformer::new(&data).expect("the rules can be read"), tables: Tables::new(&data).expect("the tables can be read") }
    })
}

/// A text to ask the dictionaries for.
#[derive(Debug, PartialEq, Clone)]
pub struct Candidate {
    pub text: String,
    /// What the text was taken for to get here. An entry found for it only
    /// counts if `Candidate::listed_as` says so of the entry's marks.
    pub conditions: u32,
    /// the inflections undone, the last one undone first; none for the text as written
    pub rules: Vec<String>,
}

impl Candidate {
    /// Whether a dictionary entry marked as these kinds of word (the `rules`
    /// of a term bank row: `v5 vt`, or nothing) is the word this text was
    /// taken for. An entry for 食べる marked `v5` is not what 食べた came from.
    pub fn listed_as(&self, marks: &str) -> bool {
        deinflect::matches(self.conditions, japanese().transformer.parts(marks))
    }
}

/// Everything a dictionary may list `source` under: the ways to write it,
/// and for each the forms it may be an inflection of. Most are no word at
/// all; the dictionary tells. The text as written comes first, and of two
/// ways to the same text taken for the same thing the shorter is kept.
///
/// `source` is one stretch of text taken as one word. To find the word a
/// longer text starts with, ask for ever shorter beginnings of it.
pub fn candidates(source: &str) -> Vec<Candidate> {
    let japanese = japanese();
    let mut found: Vec<Candidate> = Vec::new();
    for variant in japanese.tables.variants(source) {
        for form in japanese.transformer.transform(&variant) {
            if !found.iter().any(|c| c.text == form.text && c.conditions == form.conditions) {
                found.push(Candidate { text: form.text, conditions: form.conditions, rules: form.rules });
            }
        }
    }
    found
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn data() -> Value {
        serde_json::from_str(JAPANESE).unwrap()
    }

    /// What Yomitan itself makes of some text, by tools/yomitan-dump.mjs.
    pub fn vectors() -> Value {
        serde_json::from_str(include_str!("vectors.json")).unwrap()
    }

    /// A number for a text, the same one the dump makes of it.
    pub fn sum(text: &str) -> String {
        format!("{:x}", text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100000001b3)))
    }

    #[test]
    fn candidates_of_a_word() {
        let found = candidates("ﾀﾍﾞさせられた");
        assert_eq!(found[0], Candidate { text: "ﾀﾍﾞさせられた".to_string(), conditions: 0, rules: Vec::new() });
        // written in halfwidth katakana, inflected three times over
        let verb = found.iter().find(|c| c.text == "たべる" && c.listed_as("v1")).expect("the dictionary form");
        assert_eq!(verb.rules, ["causative", "potential or passive", "-た"]);
        assert!(!verb.listed_as("v5"));
        assert!(found.iter().filter(|c| c.text == "たべる" && c.conditions == verb.conditions).count() == 1);
        // what is not inflected is whatever the dictionary says it is
        assert!(found[0].listed_as("n") && found[0].listed_as(""));
    }

    #[test]
    fn candidates_of_a_name_in_latin_letters() {
        let found = candidates("Shinjuku");
        assert_eq!(found[0].text, "Shinjuku");
        assert!(found.iter().any(|c| c.text == "しんじゅく" && c.rules.is_empty()));
        assert!(found.iter().any(|c| c.text == "シンジュク"));
    }
}
