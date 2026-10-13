//! Words looked up in Yomitan, the dictionary extension of the browser,
//! through its API: a small server on this machine that the extension
//! starts when its "Enable Yomitan API" setting is on (it needs
//! https://github.com/yomidevs/yomitan-api installed).

use std::time::Duration;

use serde_json::{json, Value};

const API: &str = "http://127.0.0.1:19633/termEntries";
const WITHIN: Duration = Duration::from_secs(3);
/// Entries of a word that are kept.
const ENTRIES: usize = 6;
/// Parts of a dictionary's entry that are left out: where it has its text
/// from, and the table of other spellings.
const LEFT_OUT: [&str; 3] = ["attribution", "attribution-footnote", "forms"];
/// The colours of a small label in a dictionary's text when the dictionary
/// names none that can be used, and of the word an example is about.
const LABEL: &str = "#565656";
/// Kinds of mark that the text of a dictionary shows by itself, or that
/// say nothing to a reader.
const UNMARKED: [&str; 2] = ["partOfSpeech", "default"];
const KEYWORD: &str = "#7ee07e";

/// A piece of what a dictionary says of a word, in Pango markup.
#[derive(Debug, PartialEq)]
pub enum Block {
    Text(String),
    /// a sentence that uses the word, and what it means
    Example { japanese: String, translation: String },
}

#[derive(Debug, PartialEq)]
pub struct Definition {
    pub dictionary: String,
    /// what the dictionary marks the word as: a name and the kind of mark
    pub tags: Vec<(String, String)>,
    pub blocks: Vec<Block>,
}

/// One way to read the text looked up.
#[derive(Debug, PartialEq)]
pub struct Entry {
    pub term: String,
    pub reading: String,
    /// how common the word is, by who counted: a name and its numbers
    pub frequencies: Vec<(String, String)>,
    pub definitions: Vec<Definition>,
}

#[derive(Debug, PartialEq, Default)]
pub struct Found {
    /// how much of the text, in bytes from its start, the word found takes
    pub length: usize,
    pub entries: Vec<Entry>,
}

/// Looks up the word that `text` starts with.
pub fn lookup(text: &str) -> Result<Found, String> {
    let asked = ureq::post(API).config().timeout_global(Some(WITHIN)).build().send_json(json!({"term": text}));
    let mut response = asked.map_err(|e| e.to_string())?;
    let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(found(&reply, text))
}

fn list(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

/// Reads the reply to a lookup of `text`: Yomitan's own entries.
fn found(reply: &Value, text: &str) -> Found {
    // the length is counted the way JavaScript counts
    let mut units = reply["originalTextLength"].as_u64().unwrap_or(0) as usize;
    let mut length = 0;
    for c in text.chars() {
        if units == 0 {
            break;
        }
        units = units.saturating_sub(c.len_utf16());
        length += c.len_utf8();
    }
    let mut entries = Vec::new();
    for entry in list(&reply["dictionaryEntries"]).take(ENTRIES) {
        let headword = &entry["headwords"][0];
        let (Some(term), reading) = (headword["term"].as_str(), headword["reading"].as_str().unwrap_or_default()) else { continue };
        let named = |of: &Value| of["dictionaryAlias"].as_str().filter(|a| !a.is_empty()).or(of["dictionary"].as_str()).unwrap_or_default().to_string();

        // the numbers of one counter are put together: "9425, 10426"
        let mut frequencies: Vec<(String, String)> = Vec::new();
        for frequency in list(&entry["frequencies"]) {
            let number = frequency["displayValue"].as_str().map(str::to_string).or_else(|| frequency["frequency"].as_u64().map(|n| n.to_string()));
            let (Some(number), counter) = (number, named(frequency)) else { continue };
            match frequencies.iter_mut().find(|f| f.0 == counter) {
                Some(known) => known.1 = format!("{}, {number}", known.1),
                None => frequencies.push((counter, number)),
            }
        }
        let mut definitions: Vec<Definition> = Vec::new();
        for definition in list(&entry["definitions"]) {
            let mut tags: Vec<(String, String)> = Vec::new();
            for tag in list(&definition["tags"]) {
                let tag = (tag["name"].as_str().unwrap_or_default().to_string(), tag["category"].as_str().unwrap_or_default().to_string());
                if !tag.0.is_empty() && !UNMARKED.contains(&tag.1.as_str()) && !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            let mut writer = Writer::default();
            for part in list(&definition["entries"]) {
                let content = if part["type"] == "structured-content" { &part["content"] } else if part["type"] == "text" { &part["text"] } else { part };
                writer.write(content);
                writer.line();
            }
            let blocks = writer.done();
            if blocks.is_empty() {
                continue;
            }
            // what one dictionary says in several places is put together
            let dictionary = named(definition);
            match definitions.iter_mut().find(|d| d.dictionary == dictionary) {
                Some(known) => {
                    known.blocks.extend(blocks);
                    tags.into_iter().for_each(|tag| if !known.tags.contains(&tag) { known.tags.push(tag) });
                }
                None => definitions.push(Definition { dictionary, tags, blocks }),
            }
        }
        entries.push(Entry { term: term.to_string(), reading: reading.to_string(), frequencies, definitions });
    }
    Found { length, entries }
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\'', "&apos;").replace('"', "&quot;")
}

/// A colour as markup can take it: `#` and hexadecimal digits.
fn colour(style: Option<&Value>, of: &str) -> Option<String> {
    let named = style?.get(of)?.as_str()?;
    (named.starts_with('#') && [4, 7].contains(&named.len()) && named[1..].chars().all(|c| c.is_ascii_hexdigit())).then(|| named.to_string())
}

/// Turns a dictionary's structured content, a tree of nodes `{tag, content,
/// data, style}` in the manner of HTML, into blocks of markup.
#[derive(Default)]
struct Writer {
    blocks: Vec<Block>,
    text: String,
    /// how deep in lists
    depth: usize,
    /// meanings numbered so far, where the dictionary does not number them
    meanings: u32,
}

impl Writer {
    /// Ends the line being written, if there is anything on it.
    fn line(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }

    /// Ends the block of text being written.
    fn block(&mut self) {
        let text: Vec<&str> = self.text.lines().map(str::trim_end).filter(|line| !line.trim().is_empty() && line.trim() != "•").collect();
        if !text.is_empty() {
            self.blocks.push(Block::Text(text.join("\n")));
        }
        self.text.clear();
    }

    fn done(mut self) -> Vec<Block> {
        self.block();
        self.blocks
    }

    fn write(&mut self, content: &Value) {
        let node = match content {
            Value::String(text) => return self.text.push_str(&escape(text)),
            Value::Array(parts) => return parts.iter().for_each(|part| self.write(part)),
            Value::Object(node) => node,
            _ => return,
        };
        let tag = node.get("tag").and_then(Value::as_str).unwrap_or_default();
        let data = node.get("data");
        let part = data.and_then(|d| d.get("content")).and_then(Value::as_str).unwrap_or_default();
        let inner = node.get("content").unwrap_or(&Value::Null);
        // readings above kanji, pictures, and what is beside the meaning
        if ["rt", "rp", "img"].contains(&tag) || LEFT_OUT.contains(&part) {
            return;
        }
        if part == "example-sentence" {
            self.block();
            let (mut japanese, mut translation) = (String::new(), String::new());
            for half in list(inner) {
                let translated = half["data"]["content"] == "example-sentence-b";
                inline(half, if translated { &mut translation } else { &mut japanese });
            }
            if !inner.is_array() {
                inline(inner, &mut japanese);
            }
            return self.blocks.push(Block::Example { japanese: japanese.trim().to_string(), translation: translation.trim().to_string() });
        }
        // a small label: what kind of word, how it is used
        if let Some(back) = colour(node.get("style"), "backgroundColor").or_else(|| (tag == "span" && data.is_some_and(|d| d.get("code").is_some())).then(|| LABEL.to_string())) {
            let mut said = String::new();
            inline(inner, &mut said);
            self.text.push_str(&format!("<span background='{back}' foreground='#ffffff' size='smaller' weight='bold'> {} </span> ", said.trim()));
            return;
        }
        match tag {
            "li" => {
                self.line();
                self.text.push_str(&"   ".repeat(self.depth.saturating_sub(1)));
                // a numbered meaning, a group that opens with labels, or a plain item
                let number = data.and_then(|d| d.get("sense-number")).and_then(Value::as_str).and_then(|n| n.parse::<u32>().ok());
                let labelled = list(inner).next().is_some_and(|first| first["style"]["backgroundColor"].is_string() || first["data"]["code"].is_string());
                // An item that opens with labels is a meaning and is given
                // the next number, or it is a group of meanings that the
                // dictionary numbers itself.
                let group = labelled && number.is_none() && inner.to_string().contains("\"sense-number\"");
                let number = number.or_else(|| {
                    self.meanings += (labelled && !group) as u32;
                    (labelled && !group).then_some(self.meanings)
                });
                match number.filter(|n| (1..=20).contains(n)).and_then(|n| char::from_u32(0x2460 + n - 1)) {
                    Some(circled) => self.text.push_str(&format!("{circled} ")),
                    None if group => {}
                    None => self.text.push_str("• "),
                }
            }
            "br" | "div" | "tr" | "details" | "ol" | "ul" => self.line(),
            "td" | "th" => self.text.push(' '),
            _ => {}
        }
        let listed = ["ol", "ul"].contains(&tag);
        self.depth += listed as usize;
        self.write(inner);
        self.depth -= listed as usize;
        if listed || tag == "div" {
            self.line();
        }
    }
}

/// The text of a node on one line, the word an example is about in colour.
fn inline(content: &Value, out: &mut String) {
    match content {
        Value::String(text) => out.push_str(&escape(text)),
        Value::Array(parts) => parts.iter().for_each(|part| inline(part, out)),
        Value::Object(node) => {
            let tag = node.get("tag").and_then(Value::as_str).unwrap_or_default();
            let part = node.get("data").and_then(|d| d.get("content")).and_then(Value::as_str).unwrap_or_default();
            if ["rt", "rp", "img"].contains(&tag) || LEFT_OUT.contains(&part) {
                return;
            }
            let Some(inner) = node.get("content") else { return };
            if node.get("style").is_some_and(|s| s.get("color").is_some()) {
                out.push_str(&format!("<span foreground='{KEYWORD}'>"));
                inline(inner, out);
                out.push_str("</span>");
            } else {
                inline(inner, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_of_a_reply() {
        let reply: Value = serde_json::from_str(
            r##"{
            "index": 0,
            "originalTextLength": 3,
            "dictionaryEntries": [{
                "headwords": [{"term": "分かる", "reading": "わかる"}],
                "frequencies": [
                    {"dictionary": "JPDB", "dictionaryAlias": "", "frequency": 455, "displayValue": "455㋕"},
                    {"dictionary": "JPDB", "dictionaryAlias": "", "frequency": 465, "displayValue": null}
                ],
                "definitions": [
                    {"dictionary": "Jitendex", "dictionaryAlias": "", "tags": [{"name": "★", "category": "popular"}], "entries": [{"type": "structured-content", "content": [
                        {"tag": "ul", "content": [{"tag": "li", "content": [
                            {"tag": "span", "data": {"code": "v5r"}, "style": {"backgroundColor": "#565656", "color": "white"}, "content": "5-dan"},
                            {"tag": "ol", "content": [{"tag": "li", "data": {"sense-number": "1"}, "content": [
                                {"tag": "ul", "data": {"content": "glossary"}, "content": [{"tag": "li", "content": "to understand"}, {"tag": "li", "content": "a < b"}]},
                                {"tag": "div", "data": {"content": "extra-info"}, "content": {"tag": "div", "data": {"content": "example-sentence"}, "content": [
                                    {"tag": "div", "data": {"content": "example-sentence-a"}, "content": [{"tag": "ruby", "content": ["気", {"tag": "rt", "content": "き"}]}, "が", {"tag": "span", "style": {"color": "green"}, "content": "分かる"}]},
                                    {"tag": "div", "data": {"content": "example-sentence-b"}, "content": ["I see", {"tag": "span", "data": {"content": "attribution-footnote"}, "content": "[1]"}]}
                                ]}}
                            ]}]}
                        ]}]},
                        {"tag": "div", "data": {"content": "attribution"}, "content": "JMdict"}
                    ]}]},
                    {"dictionary": "Jitendex", "dictionaryAlias": "", "tags": [{"name": "★", "category": "popular"}], "entries": ["to become clear"]},
                    {"dictionary": "大辞林", "dictionaryAlias": "大", "entries": [{"type": "text", "text": "物事の意味がはっきりする。"}, {"type": "image", "path": "a.png"}]}
                ]
            }, {"headwords": [], "definitions": []}]
        }"##,
        )
        .unwrap();
        let found = found(&reply, "分かるかな");
        assert_eq!(found.length, "分かる".len());
        assert_eq!(found.entries.len(), 1);
        let entry = &found.entries[0];
        assert_eq!((entry.term.as_str(), entry.reading.as_str()), ("分かる", "わかる"));
        assert_eq!(entry.frequencies, [("JPDB".to_string(), "455㋕, 465".to_string())]);
        let jitendex = &entry.definitions[0];
        assert_eq!(jitendex.dictionary, "Jitendex");
        assert_eq!(jitendex.tags, [("★".to_string(), "popular".to_string())]);
        let labels = "<span background='#565656' foreground='#ffffff' size='smaller' weight='bold'> 5-dan </span>";
        assert_eq!(jitendex.blocks[0], Block::Text(format!("{labels}\n   ①\n      • to understand\n      • a &lt; b")));
        assert_eq!(jitendex.blocks[1], Block::Example { japanese: format!("気が<span foreground='{KEYWORD}'>分かる</span>"), translation: "I see".to_string() });
        assert_eq!(jitendex.blocks[2], Block::Text("to become clear".to_string()));
        assert_eq!(entry.definitions[1], Definition { dictionary: "大".to_string(), tags: Vec::new(), blocks: vec![Block::Text("物事の意味がはっきりする。".to_string())] });
    }

    #[test]
    fn nothing_found() {
        assert_eq!(found(&json!({"index": 0, "dictionaryEntries": [], "originalTextLength": 0}), "あ"), Found::default());
        assert_eq!(found(&json!({"error": "x"}), "あ"), Found::default());
    }
}
