//! Looking up the word a text starts with, in the dictionaries imported or
//! in Yomitan, whichever the reader has chosen.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::store::{Hit, Store};
use crate::yomitan::{self, Entry, Found};
use crate::{dict, ocr};

/// Where words are looked up.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    /// the dictionaries imported if there are any, else Yomitan if it answers
    Auto,
    Tool,
    Yomitan,
}

impl Source {
    pub const ALL: [(Source, &'static str, &'static str); 3] = [(Source::Auto, "auto", "Tự chọn"), (Source::Tool, "tool", "Từ điển trong tool"), (Source::Yomitan, "yomitan", "Yomitan (Firefox)")];
}

/// Why a word could not be looked up.
#[derive(Debug, PartialEq)]
pub enum Trouble {
    /// there is nothing to look in: no dictionary imported and no Yomitan
    NoDictionary,
    Yomitan(String),
    Store(String),
}

fn source_file() -> PathBuf {
    ocr::config_dir("XDG_CONFIG_HOME", ".config").join("lookup.json")
}

pub fn source() -> Source {
    let file = std::fs::read_to_string(source_file()).unwrap_or_default();
    let saved: Value = serde_json::from_str(&file).unwrap_or_default();
    Source::ALL.iter().find(|s| saved["source"] == s.1).map_or(Source::Auto, |s| s.0)
}

pub fn set_source(source: Source) {
    let id = Source::ALL.iter().find(|s| s.0 == source).unwrap().1;
    let file = source_file();
    let _ = std::fs::create_dir_all(file.parent().unwrap());
    if let Err(e) = std::fs::write(&file, format!("{:#}\n", json!({"source": id}))) {
        eprintln!("cannot save where words are looked up to {}: {e}", file.display());
    }
}

/// Looks up the word that `text` starts with.
pub fn find(text: &str) -> Result<Found, Trouble> {
    let own = || -> Result<Found, Trouble> {
        let store = Store::open().map_err(Trouble::Store)?;
        search(&store, text).map_err(Trouble::Store)
    };
    match source() {
        Source::Tool => own(),
        Source::Yomitan => yomitan::lookup(text).map_err(Trouble::Yomitan),
        Source::Auto => match Store::open() {
            Ok(store) if store.ready() => search(&store, text).map_err(Trouble::Store),
            _ => yomitan::lookup(text).map_err(|_| Trouble::NoDictionary),
        },
    }
}

/// The entries for the word at the start of `text`: those for the longest
/// stretch that is a word first, then for each shorter one. `text` is as
/// much as is worth looking at, not a whole message.
pub fn search(store: &Store, text: &str) -> Result<Found, String> {
    // a hit, by the stretch of text it was found for and the inflections undone to find it
    let mut hits: Vec<(usize, usize, Hit)> = Vec::new();
    let ends = text.char_indices().map(|(at, c)| at + c.len_utf8());
    for end in ends {
        for candidate in dict::candidates(&text[..end]) {
            for hit in store.find(&candidate.text)? {
                // an entry only counts for what its kind of word allows it to come from
                if candidate.listed_as(&hit.term.rules) {
                    hits.push((end, candidate.rules.len(), hit));
                }
            }
        }
    }
    hits.sort_by_key(|a| (std::cmp::Reverse(a.0), a.1, a.2.position, std::cmp::Reverse(a.2.term.score)));
    let length = hits.first().map_or(0, |h| h.0);
    // one entry for each word and reading, whoever lists it
    let mut words: Vec<((String, String), Vec<Hit>)> = Vec::new();
    let mut seen = Vec::new();
    for (_, _, hit) in hits {
        if seen.contains(&hit.id) {
            continue;
        }
        seen.push(hit.id);
        // a reading left out is the word itself, as in the dictionaries of kana words
        let key = (hit.term.term.clone(), if hit.term.reading.is_empty() { hit.term.term.clone() } else { hit.term.reading.clone() });
        match words.iter().position(|w| w.0 == key) {
            Some(at) => words[at].1.push(hit),
            None if words.len() < yomitan::ENTRIES => words.push((key, vec![hit])),
            None => {}
        }
    }
    let mut categories: HashMap<i64, HashMap<String, String>> = HashMap::new();
    let mut entries = Vec::new();
    for ((term, reading), hits) in words {
        let mut frequencies = Vec::new();
        for count in store.counts(&term)? {
            if count.reading.as_ref().is_none_or(|r| *r == reading || (*r == term && reading == term)) {
                yomitan::add_frequency(&mut frequencies, count.title, count.shown);
            }
        }
        let mut definitions = Vec::new();
        for hit in &hits {
            if let std::collections::hash_map::Entry::Vacant(e) = categories.entry(hit.dict) {
                e.insert(store.categories(hit.dict)?);
            }
            let known = &categories[&hit.dict];
            let marks = hit.term.tags.split_whitespace().chain(hit.term.term_tags.split_whitespace());
            let marks = marks.map(|name| (name.to_string(), known.get(name).cloned().unwrap_or_else(|| "default".to_string())));
            yomitan::add_definition(&mut definitions, hit.title.clone(), marks, yomitan::blocks(&hit.term.glossary));
        }
        let shown = hits.iter().map(|h| h.term.reading.as_str()).find(|r| !r.is_empty()).unwrap_or_default().to_string();
        entries.push(Entry { term, reading: shown, frequencies, definitions });
    }
    Ok(Found { length, entries })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::super::import::{import, tests::zipped};
    use super::*;

    fn store_of(dictionaries: &[Vec<u8>]) -> Store {
        let mut store = Store::in_memory();
        for dictionary in dictionaries {
            import(&mut store, Cursor::new(dictionary.clone()), &mut |_, _| true).unwrap();
        }
        store
    }

    fn dictionary(title: &str, terms: Value, meta: Value) -> Vec<u8> {
        zipped(&[
            ("index.json", json!({"title": title, "revision": "1", "format": 3})),
            ("tag_bank_1.json", json!([["P", "popular", 0, "", 0]])),
            ("term_bank_1.json", terms),
            ("term_meta_bank_1.json", meta),
        ])
    }

    fn names(found: &Found) -> Vec<String> {
        found.entries.iter().map(|e| e.term.clone()).collect()
    }

    #[test]
    fn the_longest_word_is_first() {
        let terms = json!([["日本", "にほん", "", "", 0, ["Japan"], 1, ""], ["日本語", "にほんご", "", "", 0, ["Japanese"], 2, ""], ["語", "ご", "", "", 0, ["word"], 3, ""]]);
        let store = store_of(&[dictionary("D", terms, json!([]))]);
        let found = search(&store, "日本語を話す").unwrap();
        assert_eq!(names(&found), ["日本語", "日本"]);
        // the length is of the longest word, in bytes
        assert_eq!(found.length, "日本語".len());
        assert_eq!(search(&store, "を話す").unwrap(), Found::default());
        assert_eq!(search(&store, "").unwrap(), Found::default());
    }

    #[test]
    fn inflected_words_are_found_by_their_kind() {
        let terms = json!([
            ["食べる", "たべる", "", "v1", 0, ["to eat"], 1, ""],
            // the same text as a verb of the other kind: not what 食べた comes from
            ["食べる", "たべる", "", "v5", 0, ["wrong kind"], 2, ""],
            ["食べ", "たべ", "", "n", 0, ["food"], 3, ""]
        ]);
        let store = store_of(&[dictionary("D", terms, json!([]))]);
        let found = search(&store, "食べたい").unwrap();
        assert_eq!(found.length, "食べたい".len());
        assert_eq!(names(&found), ["食べる", "食べ"]);
        assert_eq!(found.entries[0].definitions.len(), 1);
        let blocks = &found.entries[0].definitions[0].blocks;
        assert_eq!(blocks, &[yomitan::Block::Text("to eat".to_string())]);
    }

    #[test]
    fn dictionaries_in_their_order_put_together() {
        let one = dictionary("One", json!([["猫", "ねこ", "P", "", 1, ["cat"], 1, "P"], ["猫", "ねこ", "", "", 9, ["feline"], 2, ""]]), json!([]));
        let two = dictionary("Two", json!([["猫", "ねこ", "", "", 0, ["neko"], 1, ""], ["猫", "ネコ", "", "", 0, ["other reading"], 2, ""]]), json!([]));
        let mut store = store_of(&[one, two]);
        let found = search(&store, "猫").unwrap();
        // a word and a reading make an entry; each dictionary says its part
        assert_eq!(found.entries.len(), 2);
        let first = &found.entries[0];
        assert_eq!(first.reading, "ねこ");
        assert_eq!(first.definitions.iter().map(|d| d.dictionary.as_str()).collect::<Vec<_>>(), ["One", "Two"]);
        // what one dictionary says in two rows is one definition, best score first
        assert_eq!(first.definitions[0].blocks, [yomitan::Block::Text("feline".to_string()), yomitan::Block::Text("cat".to_string())]);
        assert_eq!(first.definitions[0].tags, [("P".to_string(), "popular".to_string())]);
        assert_eq!(found.entries[1].reading, "ネコ");
        // the order of dictionaries is the order of meanings
        let two = store.dictionaries().unwrap()[1].id;
        store.shift(two, true).unwrap();
        assert_eq!(search(&store, "猫").unwrap().entries[0].definitions[0].dictionary, "Two");
    }

    #[test]
    fn counts_come_from_the_meta_banks() {
        let terms = json!([["猫", "ねこ", "", "", 0, ["cat"], 1, ""], ["猫", "ネコ", "", "", 0, ["cat"], 2, ""]]);
        let meta = json!([["猫", "freq", 120], ["猫", "freq", {"reading": "ネコ", "frequency": 7}], ["猫", "freq", {"reading": "ねこ", "frequency": {"value": 5, "displayValue": "5㋕"}}]]);
        let store = store_of(&[dictionary("Words", terms, json!([])), dictionary("Counts", json!([["犬", "いぬ", "", "", 0, ["dog"], 1, ""]]), meta)]);
        let found = search(&store, "猫").unwrap();
        let counts = |at: usize| found.entries[at].frequencies.clone();
        assert_eq!(counts(0), [("Counts".to_string(), "120, 5㋕".to_string())]);
        assert_eq!(counts(1), [("Counts".to_string(), "120, 7".to_string())]);
    }

    #[test]
    fn a_word_with_no_reading_is_its_own() {
        let one = dictionary("One", json!([["ありがとう", "", "", "", 0, ["thanks"], 1, ""]]), json!([]));
        let two = dictionary("Two", json!([["ありがとう", "ありがとう", "", "", 0, ["thank you"], 1, ""]]), json!([]));
        let found = search(&store_of(&[one, two]), "ありがとう").unwrap();
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].definitions.len(), 2);
        assert_eq!(found.entries[0].reading, "ありがとう");
    }

    #[test]
    fn only_so_many_entries() {
        let rows: Vec<Value> = (0..20).map(|n| json!(["猫", format!("ねこ{n}"), "", "", 0, ["cat"], n, ""])).collect();
        let found = search(&store_of(&[dictionary("D", Value::Array(rows), json!([]))]), "猫").unwrap();
        assert_eq!(found.entries.len(), yomitan::ENTRIES);
    }
}
