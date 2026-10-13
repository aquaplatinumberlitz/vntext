//! Other ways to write a text, tried before it is looked up: a dictionary
//! lists よみちゃん, the game may show ﾖﾐﾁｬﾝ or よみちゃーーん. These are the
//! text preprocessors of Yomitan's Japanese (ext/js/language/ja/japanese.js
//! japanese-wanakana.js and japanese-text-preprocessors.js), in its order.

use std::collections::HashMap;

use serde_json::Value;

const VOICED_MARK: char = '\u{3099}';
const HALF_VOICED_MARK: char = '\u{309a}';
const HALFWIDTH_VOICED_MARK: char = '\u{ff9e}';
const HALFWIDTH_HALF_VOICED_MARK: char = '\u{ff9f}';
const LONG_MARK: char = 'ー';
/// how far katakana is from hiragana
const KATAKANA: u32 = 0x60;
/// how far the full width letters and digits are from the plain ones
const WIDE: u32 = 0xfee0;

/// The tables of the dump: what a character becomes.
pub struct Tables {
    /// halfwidth kana as it is alone, with a voiced and with a half-voiced mark after it
    halfwidth: HashMap<char, [Option<char>; 3]>,
    voiced: HashMap<char, char>,
    half_voiced: HashMap<char, char>,
    /// the vowel that a long mark after the character stands for
    prolonged: HashMap<char, char>,
    /// square words like ㌀, spelled out
    compatibility: HashMap<char, String>,
    /// radicals that have a character of their own, as the kanji they look like
    radicals: HashMap<char, String>,
    /// old and variant kanji, as they are written now
    kanji: HashMap<char, String>,
    /// Latin letters and the kana they spell. Each is replaced throughout
    /// before the next is looked at, so the order is part of the table.
    romaji: Vec<(String, String)>,
}

fn one(value: &Value) -> Option<char> {
    let mut chars = value.as_str()?.chars();
    chars.next().filter(|_| chars.next().is_none())
}

fn table<T>(data: &Value, name: &str, read: impl Fn(&Value) -> Option<T>) -> Result<HashMap<char, T>, String> {
    let rows = data["tables"][name].as_object().ok_or(format!("no table {name}"))?;
    let row = |(from, to): (&String, &Value)| Some((one(&Value::from(from.as_str()))?, read(to)?));
    rows.iter().map(row).collect::<Option<_>>().ok_or(format!("table {name} cannot be read"))
}

impl Tables {
    pub fn new(data: &Value) -> Result<Tables, String> {
        let three = |value: &Value| {
            let row = value.as_array().filter(|row| row.len() == 3)?;
            Some([Some(one(&row[0])?), one(&row[1]), one(&row[2])])
        };
        let text = |value: &Value| value.as_str().map(str::to_string);
        let pair = |row: &Value| Some((text(&row[0])?, text(&row[1])?));
        let romaji = data["romaji"].as_array().ok_or("no romaji")?.iter().map(pair).collect::<Option<_>>().ok_or("the romaji cannot be read")?;
        Ok(Tables {
            romaji,
            halfwidth: table(data, "halfwidth", three)?,
            voiced: table(data, "voiced", one)?,
            half_voiced: table(data, "half_voiced", one)?,
            prolonged: table(data, "prolonged", one)?,
            compatibility: table(data, "compatibility", text)?,
            radicals: table(data, "radicals", text)?,
            kanji: table(data, "kanji", text)?,
        })
    }

    /// Latin letters to the hiragana they spell, as far as they spell any:
    /// Shinjuku to しんじゅく. Names and places are often written so.
    pub fn kana(&self, text: &str) -> String {
        let mut out = String::new();
        let mut letters = String::new();
        for c in text.chars().chain(std::iter::once('\0')) {
            let letter = match c {
                'a'..='z' => Some(c),
                'A'..='Z' => Some(c.to_ascii_lowercase()),
                'ａ'..='ｚ' | 'Ａ'..='Ｚ' => char::from_u32(c as u32 - WIDE).map(|c| c.to_ascii_lowercase()),
                '-' | '－' => Some('-'),
                _ => None,
            };
            match letter {
                Some(letter) => letters.push(letter),
                None => {
                    if !letters.is_empty() {
                        out.push_str(&self.spelled(&letters));
                        letters.clear();
                    }
                    out.push(c);
                }
            }
        }
        out.pop();
        out
    }

    /// The kana a run of small Latin letters spells.
    fn spelled(&self, letters: &str) -> String {
        let mut text = letters.to_string();
        for (romaji, kana) in &self.romaji {
            if text.contains(romaji.as_str()) {
                text = text.replace(romaji.as_str(), kana);
            }
        }
        // A doubled letter became a small tsu and the letter. In a longer run
        // every other letter is left over between two of them: ttta to っtった.
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut at = 0;
        while at < chars.len() {
            out.push(chars[at]);
            let gap = chars[at] == 'っ' && chars.get(at + 1).is_some_and(char::is_ascii_lowercase) && chars.get(at + 2) == Some(&'っ');
            if gap {
                out.push('っ');
            }
            at += if gap { 2 } else { 1 };
        }
        out
    }

    /// ﾊﾞｲｸ to バイク.
    pub fn fullwidth_kana(&self, text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let Some(forms) = self.halfwidth.get(&c) else {
                out.push(c);
                continue;
            };
            let marked = match chars.peek() {
                Some(&HALFWIDTH_VOICED_MARK) => forms[1],
                Some(&HALFWIDTH_HALF_VOICED_MARK) => forms[2],
                _ => None,
            };
            if marked.is_some() {
                chars.next();
            }
            out.extend(marked.or(forms[0]));
        }
        out
    }

    /// A kana and the mark after it to the one character for both.
    pub fn combined(&self, text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let marked = match chars.peek() {
                Some(&VOICED_MARK) => self.voiced.get(&c),
                Some(&HALF_VOICED_MARK) => self.half_voiced.get(&c),
                _ => None,
            };
            if marked.is_some() {
                chars.next();
            }
            out.push(*marked.unwrap_or(&c));
        }
        out
    }

    /// コーヒー to こうひい: a long mark becomes the vowel it draws out.
    pub fn hiragana(&self, text: &str) -> String {
        let mut out = String::new();
        for c in text.chars() {
            out.push(match c {
                'ヵ' | 'ヶ' => c,
                LONG_MARK => out.chars().next_back().and_then(|before| self.prolonged.get(&before)).copied().unwrap_or(c),
                'ァ'..='ヶ' => char::from_u32(c as u32 - KATAKANA).unwrap_or(c),
                _ => c,
            });
        }
        out
    }

    fn replaced(table: &HashMap<char, String>, text: &str) -> String {
        let mut out = String::new();
        for c in text.chars() {
            match table.get(&c) {
                Some(other) => out.push_str(other),
                None => out.push(c),
            }
        }
        out
    }

    /// The text and the other ways to write it, the text itself first.
    pub fn variants(&self, text: &str) -> Vec<String> {
        type Change<'a> = Box<dyn Fn(&str) -> Vec<String> + 'a>;
        let changes: [Change; 9] = [
            Box::new(|t| vec![self.fullwidth_kana(t)]),
            Box::new(|t| vec![self.kana(t)]),
            Box::new(|t| vec![self.combined(t)]),
            Box::new(|t| vec![Tables::replaced(&self.compatibility, t)]),
            Box::new(|t| vec![Tables::replaced(&self.radicals, t)]),
            Box::new(|t| vec![narrow(t), wide(t)]),
            Box::new(|t| vec![katakana(t), self.hiragana(t)]),
            Box::new(|t| vec![collapsed(t, false), collapsed(t, true)]),
            Box::new(|t| vec![Tables::replaced(&self.kanji, t)]),
        ];
        let mut found = vec![text.to_string()];
        for change in &changes {
            let mut next: Vec<String> = Vec::new();
            for variant in &found {
                for made in std::iter::once(variant.clone()).chain(change(variant)) {
                    if !next.contains(&made) {
                        next.push(made);
                    }
                }
            }
            found = next;
        }
        found
    }
}

/// よみちゃん to ヨミチャン.
pub fn katakana(text: &str) -> String {
    text.chars().map(|c| if ('ぁ'..='ゖ').contains(&c) { char::from_u32(c as u32 + KATAKANA).unwrap_or(c) } else { c }).collect()
}

/// ｙｏｍｉ to yomi.
pub fn narrow(text: &str) -> String {
    text.chars().map(|c| if matches!(c, '０'..='９' | 'Ａ'..='Ｚ' | 'ａ'..='ｚ') { char::from_u32(c as u32 - WIDE).unwrap_or(c) } else { c }).collect()
}

/// yomi to ｙｏｍｉ.
pub fn wide(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_alphanumeric() { char::from_u32(c as u32 + WIDE).unwrap_or(c) } else { c }).collect()
}

/// Drawn-out sounds in the middle of the text cut short: すっっごーーい to
/// すっごーい, or with `all` to すごい. Those at either end stay.
pub fn collapsed(text: &str, all: bool) -> String {
    let emphatic = |c: &char| matches!(*c, 'っ' | 'ッ' | LONG_MARK);
    let chars: Vec<char> = text.chars().collect();
    let left = chars.iter().take_while(|c| emphatic(c)).count();
    let right = chars.len() - chars.iter().rev().take_while(|c| emphatic(c)).count();
    if left >= right {
        return text.to_string();
    }
    let mut out: String = chars[..left].iter().collect();
    let mut last = None;
    for c in &chars[left..right] {
        if !emphatic(c) {
            last = None;
            out.push(*c);
        } else if last != Some(*c) {
            last = Some(*c);
            if !all {
                out.push(*c);
            }
        }
    }
    out.extend(&chars[right..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::tests::{data, sum, vectors};

    /// Everything the functions make of a text, in the order of the dump's fields.
    fn made(tables: &Tables, text: &str) -> Vec<String> {
        vec![tables.kana(text), tables.hiragana(text), katakana(text), tables.fullwidth_kana(text), tables.combined(text), narrow(text), wide(text), collapsed(text, false), collapsed(text, true)]
    }

    #[test]
    fn the_same_text_as_yomitan() {
        let tables = Tables::new(&data()).unwrap();
        let vectors = vectors();
        let fields: Vec<&str> = vectors["fields"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(fields, ["kana", "hiragana", "katakana", "halfwidth", "combined", "narrow", "wide", "collapsed", "emptied"]);
        let samples = vectors["conversions"].as_array().unwrap();
        assert!(samples.len() > 30);
        for sample in samples {
            let text = sample["text"].as_str().unwrap();
            for (field, made) in fields.iter().zip(made(&tables, text)) {
                assert_eq!(made, sample[*field].as_str().unwrap(), "{field} of {text}");
            }
            let variants: Vec<&str> = sample["variants"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            assert_eq!(tables.variants(text), variants, "variants of {text}");
        }
    }

    /// Thousands of texts, of each only a number for all Yomitan made of it.
    #[test]
    fn the_same_text_as_yomitan_at_large() {
        let tables = Tables::new(&data()).unwrap();
        let vectors = vectors();
        let texts = vectors["converted"].as_array().unwrap();
        assert!(texts.len() > 10000);
        for row in texts {
            let text = row[0].as_str().unwrap();
            let made = made(&tables, text);
            assert_eq!(sum(&made.join("\n")), row[1].as_str().unwrap(), "{text} made into {made:?}");
            let variants = tables.variants(text);
            assert_eq!(variants.len() as u64, row[2].as_u64().unwrap(), "variants of {text}: {variants:?}");
            assert_eq!(sum(&variants.join("\n")), row[3].as_str().unwrap(), "variants of {text}: {variants:?}");
        }
    }
}
