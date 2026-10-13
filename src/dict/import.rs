//! Reading a dictionary in Yomitan's format, a zip file, into the store.
//! The format is the one of Yomitan's `ext/data/schemas/` (version 3).

use std::io::{Read, Seek};

use serde_json::Value;
use zip::ZipArchive;

use super::store::{Store, Term};

/// What the index of a dictionary says of it.
#[derive(Debug, PartialEq)]
pub struct Index {
    pub title: String,
    pub revision: String,
}

pub fn parse_index(text: &str) -> Result<Index, String> {
    let index: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("index.json đọc không được: {e}"))?;
    // newer dictionaries say `format`, older ones `version`
    let format = index["format"].as_i64().or_else(|| index["version"].as_i64());
    if format != Some(3) {
        return Err(match format {
            Some(n) => format!("Từ điển này theo định dạng bản {n}, tool chỉ đọc được bản 3."),
            None => "index.json không ghi bản định dạng của từ điển.".to_string(),
        });
    }
    let text = |name: &str| index[name].as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    Ok(Index { title: text("title").ok_or("index.json không ghi tên từ điển.")?, revision: text("revision").unwrap_or_default() })
}

fn text(row: &[Value], at: usize) -> String {
    row.get(at).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn number(row: &[Value], at: usize) -> i64 {
    row.get(at).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64))).unwrap_or(0)
}

/// A row of a term bank: `[term, reading, tags, rules, score, glossary,
/// sequence, term tags]`, and its sequence number. Of the glossary, the
/// pictures and the entries of other dictionaries' inflections are not kept.
fn term_row(row: &Value) -> Option<(Term, i64)> {
    let row = row.as_array()?;
    let term = text(row, 0);
    let glossary: Vec<Value> = row.get(5)?.as_array()?.iter().filter(|item| !item.is_array() && item["type"] != "image").cloned().collect();
    // a row that held nothing but pictures says nothing here
    (!term.is_empty() && !glossary.is_empty()).then(|| (Term { term, reading: text(row, 1), tags: text(row, 2), rules: text(row, 3), score: number(row, 4), glossary, term_tags: text(row, 7) }, number(row, 6)))
}

/// How a count of a word is shown: a number, a string, `{value,
/// displayValue}`, or `{reading, frequency}` of one of those for a reading.
fn shown(data: &Value) -> Option<(Option<String>, String)> {
    let plain = |value: &Value| match value {
        Value::Number(n) => Some(n.as_i64().map_or_else(|| n.to_string(), |n| n.to_string())),
        Value::String(s) => Some(s.clone()),
        _ => None,
    };
    let counted = |value: &Value| match value {
        Value::Object(o) => o.get("displayValue").filter(|d| d.is_string()).or_else(|| o.get("value")).and_then(plain),
        other => plain(other),
    };
    match data {
        Value::Object(o) if o.contains_key("frequency") => Some((o.get("reading").and_then(Value::as_str).map(str::to_string), counted(&o["frequency"])?)),
        other => Some((None, counted(other)?)),
    }
}

/// Which bank files a zip holds, by name: `(kind, name)`; the tags come
/// first so that the terms can be shown with them.
fn banks(names: impl Iterator<Item = String>) -> Vec<(&'static str, String)> {
    let mut banks: Vec<(&'static str, String)> = Vec::new();
    for name in names {
        let numbered = |prefix: &str| name.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(".json")).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        // `term_meta_bank_` also starts with `term_`, so the longer is asked first
        if numbered("tag_bank_") {
            banks.push(("tags", name));
        } else if numbered("term_meta_bank_") {
            banks.push(("meta", name));
        } else if numbered("term_bank_") {
            banks.push(("terms", name));
        }
    }
    let rank = |kind: &str| ["tags", "terms", "meta"].iter().position(|k| *k == kind).unwrap();
    banks.sort_by(|a, b| (rank(a.0), number_of(&a.1), &a.1).cmp(&(rank(b.0), number_of(&b.1), &b.1)));
    banks
}

fn number_of(name: &str) -> u64 {
    name.trim_end_matches(".json").rsplit('_').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// The text of a file of the zip.
fn entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<String, String> {
    let mut file = zip.by_name(name).map_err(|e| format!("Không mở được {name} trong file zip: {e}"))?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| format!("{name} đọc không được: {e}"))?;
    Ok(text)
}

/// Reads a dictionary from the zip file in `source` into the store, in one
/// transaction: a dictionary that fails to be read is not there at all.
/// `progress` is told how many of the files are done and how many there are,
/// and says whether to go on. Gives the title of the dictionary.
pub fn import<R: Read + Seek>(store: &mut Store, source: R, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<String, String> {
    let mut zip = ZipArchive::new(source).map_err(|e| format!("Đây không phải file zip đọc được: {e}"))?;
    let index = parse_index(&entry(&mut zip, "index.json").map_err(|_| "Không thấy index.json: đây không phải từ điển của Yomitan.".to_string())?)?;
    let files = banks(zip.file_names().map(str::to_string));
    if !files.iter().any(|f| f.0 == "terms") {
        return Err("Từ điển này không có term_bank nào: tool chỉ nhập từ điển từ vựng.".to_string());
    }
    let total = files.len();
    store.replace(&index.title, &index.revision, |filler| {
        progress(0, total);
        for (done, (kind, name)) in files.iter().enumerate() {
            let rows = serde_json::from_str::<Value>(entry(&mut zip, name)?.trim_start_matches('\u{feff}')).map_err(|e| format!("{name} không phải JSON: {e}"))?;
            for row in rows.as_array().into_iter().flatten() {
                match *kind {
                    "terms" => {
                        if let Some((term, sequence)) = term_row(row) {
                            filler.term(&term, sequence)?;
                        }
                    }
                    "meta" => {
                        // only counts of how often words are used are kept
                        if let (Some(term), Some("freq"), Some((reading, how))) = (row[0].as_str(), row[1].as_str(), shown(&row[2])) {
                            filler.count(term, reading.as_deref(), &how)?;
                        }
                    }
                    _ => {
                        if let Some(row) = row.as_array().filter(|r| r.first().is_some_and(Value::is_string)) {
                            filler.tag(&text(row, 0), &text(row, 1), number(row, 2), &text(row, 3), number(row, 4))?;
                        }
                    }
                }
            }
            if !progress(done + 1, total) {
                return Err("Đã huỷ.".to_string());
            }
        }
        Ok(())
    })?;
    Ok(index.title)
}

#[cfg(test)]
pub mod tests {
    use std::io::{Cursor, Write};

    use serde_json::json;
    use zip::write::SimpleFileOptions;

    use super::*;

    /// A zip of the files given, by name.
    pub fn zipped(files: &[(&str, Value)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(content.to_string().as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// A small dictionary of meanings: a verb, a noun with two readings, a name.
    pub fn meanings() -> Vec<u8> {
        zipped(&[
            ("index.json", json!({"title": "Mini", "revision": "r1", "format": 3})),
            ("tag_bank_1.json", json!([["P", "popular", 0, "common", 1], ["v5", "partOfSpeech", 0, "godan", 0]])),
            (
                "term_bank_1.json",
                json!([
                    ["食べる", "たべる", "v1 P", "v1", 5, ["to eat", {"type": "text", "text": "to live on"}, {"type": "image", "path": "a.png"}, ["x", ["v5"]]], 1, ""],
                    ["食べ", "たべ", "", "", 0, ["food"], 2, ""],
                    ["書く", "かく", "v5", "v5", 3, ["to write"], 3, ""],
                    ["書く", "かく", "v5", "v5k", 9, [{"type": "structured-content", "content": "to draw"}], 4, ""],
                    ["日本", "にほん", "", "", 1, ["Japan"], 5, ""],
                    ["日本", "にっぽん", "", "", 0, ["Nippon"], 6, ""]
                ]),
            ),
            ("term_bank_2.json", json!([["猫", "ねこ", "", "", 0, ["cat"], 7, ""]])),
        ])
    }

    #[test]
    fn the_index() {
        assert_eq!(parse_index(r#"{"title":" A ","revision":"2","format":3}"#), Ok(Index { title: "A".to_string(), revision: "2".to_string() }));
        // the old name for the format, and a byte order mark
        assert!(parse_index("\u{feff}{\"title\":\"A\",\"version\":3}").is_ok());
        assert!(parse_index(r#"{"title":"A","format":1}"#).unwrap_err().contains("bản 1"));
        assert!(parse_index(r#"{"title":"A"}"#).is_err());
        assert!(parse_index(r#"{"format":3}"#).is_err());
        assert!(parse_index("not json").is_err());
    }

    #[test]
    fn rows_of_a_term_bank() {
        let (term, sequence) = term_row(&json!(["食べる", "たべる", "v1", "v1", 5.0, ["a", {"type": "image"}, ["b", []], {"type": "text", "text": "c"}], 12, "P"])).unwrap();
        assert_eq!((term.term.as_str(), term.reading.as_str(), term.rules.as_str(), term.score, sequence, term.term_tags.as_str()), ("食べる", "たべる", "v1", 5, 12, "P"));
        // the picture and the list of inflections are left out
        assert_eq!(term.glossary, [json!("a"), json!({"type": "text", "text": "c"})]);
        assert_eq!(term_row(&json!(["", "", "", "", 0, []])), None);
        assert_eq!(term_row(&json!(["a", "b"])), None);
    }

    #[test]
    fn the_forms_of_a_count() {
        assert_eq!(shown(&json!(455)), Some((None, "455".to_string())));
        assert_eq!(shown(&json!("455㋕")), Some((None, "455㋕".to_string())));
        assert_eq!(shown(&json!({"value": 7, "displayValue": "7 (rare)"})), Some((None, "7 (rare)".to_string())));
        assert_eq!(shown(&json!({"value": 7})), Some((None, "7".to_string())));
        assert_eq!(shown(&json!({"reading": "かな", "frequency": 3})), Some((Some("かな".to_string()), "3".to_string())));
        assert_eq!(shown(&json!({"reading": "かな", "frequency": {"value": 3, "displayValue": "3x"}})), Some((Some("かな".to_string()), "3x".to_string())));
        assert_eq!(shown(&json!(null)), None);
    }

    #[test]
    fn bank_files_in_order() {
        let names = ["term_bank_10.json", "term_bank_2.json", "index.json", "tag_bank_1.json", "term_meta_bank_1.json", "term_bank_x.json", "kanji_bank_1.json", "term_bank_1.json"];
        let found: Vec<String> = banks(names.iter().map(|n| n.to_string())).into_iter().map(|b| b.1).collect();
        assert_eq!(found, ["tag_bank_1.json", "term_bank_1.json", "term_bank_2.json", "term_bank_10.json", "term_meta_bank_1.json"]);
    }

    #[test]
    fn a_dictionary_comes_in_with_its_files() {
        let mut store = Store::in_memory();
        let mut steps = Vec::new();
        let title = import(&mut store, Cursor::new(meanings()), &mut |done, total| {
            steps.push((done, total));
            true
        })
        .unwrap();
        assert_eq!(title, "Mini");
        // tags, two term banks
        assert_eq!(steps, [(0, 3), (1, 3), (2, 3), (3, 3)]);
        let eat = store.find("食べる").unwrap();
        assert_eq!(eat.len(), 1);
        assert_eq!(eat[0].term.glossary, [json!("to eat"), json!({"type": "text", "text": "to live on"})]);
        assert_eq!(store.find("ねこ").unwrap()[0].term.term, "猫");
        let dict = store.dictionaries().unwrap()[0].id;
        assert_eq!(store.categories(dict).unwrap().get("P").map(String::as_str), Some("popular"));
    }

    #[test]
    fn meta_banks_give_counts() {
        let mut store = Store::in_memory();
        let zip = zipped(&[
            ("index.json", json!({"title": "Counts", "revision": "1", "format": 3})),
            ("term_bank_1.json", json!([["猫", "ねこ", "", "", 0, ["cat"], 1, ""]])),
            ("term_meta_bank_1.json", json!([["猫", "freq", 120], ["猫", "freq", {"reading": "ねこ", "frequency": {"value": 5, "displayValue": "5㋕"}}], ["猫", "pitch", {"reading": "ねこ"}], ["犬", "freq", "n/a"]])),
        ]);
        import(&mut store, Cursor::new(zip), &mut |_, _| true).unwrap();
        let counts = store.counts("猫").unwrap();
        assert_eq!(counts.iter().map(|c| (c.title.as_str(), c.reading.as_deref(), c.shown.as_str())).collect::<Vec<_>>(), [("Counts", None, "120"), ("Counts", Some("ねこ"), "5㋕")]);
    }

    #[test]
    fn a_dictionary_is_replaced_or_cancelled_whole() {
        let mut store = Store::in_memory();
        import(&mut store, Cursor::new(meanings()), &mut |_, _| true).unwrap();
        // cancelled halfway: the dictionary that was there stays as it was
        let again = import(&mut store, Cursor::new(meanings()), &mut |done, _| done < 2);
        assert_eq!(again, Err("Đã huỷ.".to_string()));
        assert_eq!(store.dictionaries().unwrap().len(), 1);
        assert_eq!(store.find("猫").unwrap().len(), 1);
        // the same title again is one dictionary
        import(&mut store, Cursor::new(meanings()), &mut |_, _| true).unwrap();
        assert_eq!(store.dictionaries().unwrap().len(), 1);
        assert_eq!(store.find("猫").unwrap().len(), 1);
    }

    #[test]
    fn what_is_not_a_dictionary() {
        let mut store = Store::in_memory();
        let mut go = |_: usize, _: usize| true;
        assert!(import(&mut store, Cursor::new(b"not a zip".to_vec()), &mut go).unwrap_err().contains("zip"));
        let none = zipped(&[("readme.txt", json!("x"))]);
        assert!(import(&mut store, Cursor::new(none), &mut go).unwrap_err().contains("index.json"));
        let old = zipped(&[("index.json", json!({"title": "Old", "format": 1})), ("term_bank_1.json", json!([]))]);
        assert!(import(&mut store, Cursor::new(old), &mut go).unwrap_err().contains("bản 1"));
        let kanji = zipped(&[("index.json", json!({"title": "K", "format": 3})), ("kanji_bank_1.json", json!([]))]);
        assert!(import(&mut store, Cursor::new(kanji), &mut go).unwrap_err().contains("term_bank"));
        assert!(store.dictionaries().unwrap().is_empty());
    }
}
