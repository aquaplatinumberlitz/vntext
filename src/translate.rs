//! Translation through the Gemini API, with a disk cache so a line
//! is only ever paid for once.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Value};

const API: &str = "https://generativelanguage.googleapis.com/v1beta";
const KEY_VAR: &str = "GEMINI_API_KEY";

pub struct Line {
    pub speaker: Option<String>,
    pub text: String,
}

/// What a request needs; cheap to clone so that requests can run without
/// holding on to the cache.
#[derive(Clone)]
pub struct Client {
    key: String,
    pub model: String,
    language: String,
}

pub struct Translator {
    pub client: Client,
    cache: HashMap<String, String>,
    cache_path: PathBuf,
}

fn language_name(code: &str) -> &str {
    match code {
        "vi" => "Vietnamese",
        "en" => "English",
        other => other,
    }
}

fn api_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(429) => "rate limit reached (HTTP 429)".to_string(),
        ureq::Error::StatusCode(code) => format!("HTTP {code}"),
        other => other.to_string(),
    }
}

impl Translator {
    pub fn new(language: &str, model: Option<String>) -> Result<Translator, String> {
        let key = std::env::var(KEY_VAR).map_err(|_| format!("set {KEY_VAR} to use the Gemini API"))?;
        let model = match model {
            Some(m) => m,
            None => newest_flash_lite(&key)?,
        };
        let dir = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vntext");
        let _ = fs::create_dir_all(&dir);
        let cache_path = dir.join(format!("{language}.tsv"));
        let cache = fs::read_to_string(&cache_path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(jp, tr)| (unescape(jp), unescape(tr)))
            .collect();
        Ok(Translator { client: Client { key, model, language: language_name(language).to_string() }, cache, cache_path })
    }

    pub fn cached(&self, text: &str) -> Option<&String> {
        self.cache.get(text)
    }

    /// Caches translations, in memory and on disk.
    pub fn store(&mut self, lines: &[Line], translations: Vec<String>) {
        let mut file = OpenOptions::new().create(true).append(true).open(&self.cache_path).ok();
        for (line, tr) in lines.iter().zip(translations) {
            if let Some(f) = file.as_mut() {
                let _ = writeln!(f, "{}\t{}", escape(&line.text), escape(&tr));
            }
            self.cache.insert(line.text.clone(), tr);
        }
    }
}

impl Client {
    /// Translates `lines` in one request. `context` holds the lines shown
    /// just before, for the model's reference only.
    pub fn translate(&self, context: &[String], lines: &[Line]) -> Result<Vec<String>, String> {
        let input = json!({
            "previous_lines": context,
            "lines": lines.iter().map(|l| json!({"speaker": l.speaker, "text": l.text})).collect::<Vec<_>>(),
        });
        let system = format!(
            "You translate lines from a Japanese visual novel into {}. \
             The input is JSON: `previous_lines` is earlier text for context only, `lines` is what to translate. \
             Reply with a JSON array of strings holding the translation of each entry of `lines`, \
             in the same order and with exactly the same number of entries. \
             An entry is one passage by one speaker and may hold several sentences on separate lines: \
             translate it as a whole and keep its line breaks. \
             Entries without a speaker are narration; use the surrounding lines to tell who does what. \
             Translate faithfully and naturally, keep the speaker's tone, never merge or split entries, and add no notes.",
            self.language
        );
        let body = json!({
            "systemInstruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": input.to_string()}]}],
            "generationConfig": {
                "temperature": 0.3,
                "responseMimeType": "application/json",
                "responseSchema": {"type": "ARRAY", "items": {"type": "STRING"}},
            },
        });
        let url = format!("{API}/models/{}:generateContent", self.model);
        let mut response = ureq::post(&url).header("x-goog-api-key", &self.key).send_json(&body).map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        let translations = parse_reply(&reply)?;
        if translations.len() != lines.len() {
            return Err(format!("asked for {} translations, got {}", lines.len(), translations.len()));
        }
        Ok(translations)
    }
}

/// What a picture of a game's text box says.
#[derive(Debug, PartialEq)]
pub struct Shown {
    pub speaker: Option<String>,
    pub text: String,
    pub translation: Option<String>,
}

impl Client {
    /// Reads the Japanese text in a PNG picture of a game's text box and,
    /// if asked, translates it in the same request. `context` holds the text
    /// read just before, for the model's reference only.
    pub fn read(&self, png: &[u8], context: &[String], translate: bool) -> Result<Shown, String> {
        let mut system = "The picture is the text box of a Japanese visual novel. \
             Give the Japanese text in it as `text`, exactly as written and with its line breaks, \
             leaving out furigana (the small readings above kanji) and anything that is not part of the passage, such as buttons. \
             If the name of the speaker is shown apart from the passage give it as `speaker`, otherwise leave `speaker` empty. \
             If the picture shows no Japanese passage, leave `text` empty."
            .to_string();
        let mut properties = json!({"speaker": {"type": "STRING"}, "text": {"type": "STRING"}});
        let mut required = vec!["speaker", "text"];
        if translate {
            system += &format!(
                " Then give as `translation` a faithful and natural translation of `text` into {}, \
                 keeping the speaker's tone and the line breaks. \
                 The text that follows the picture is JSON whose `previous_lines` is what was shown before, for context only.",
                self.language
            );
            properties["translation"] = json!({"type": "STRING"});
            required.push("translation");
        }
        let picture = json!({"inlineData": {"mimeType": "image/png", "data": crate::screen::base64(png)}});
        let body = json!({
            "systemInstruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [picture, {"text": json!({"previous_lines": context}).to_string()}]}],
            "generationConfig": {
                "temperature": 0.3,
                "responseMimeType": "application/json",
                "responseSchema": {"type": "OBJECT", "properties": properties, "required": required},
            },
        });
        let url = format!("{API}/models/{}:generateContent", self.model);
        let mut response = ureq::post(&url).header("x-goog-api-key", &self.key).send_json(&body).map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        parse_shown(&reply)
    }
}

fn reply_text(reply: &Value) -> Result<&str, String> {
    reply["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("no text in reply: {}", reply.to_string().chars().take(200).collect::<String>()))
}

fn parse_reply(reply: &Value) -> Result<Vec<String>, String> {
    serde_json::from_str(reply_text(reply)?).map_err(|e| format!("reply is not a JSON array of strings: {e}"))
}

fn parse_shown(reply: &Value) -> Result<Shown, String> {
    let shown: Value = serde_json::from_str(reply_text(reply)?).map_err(|e| format!("reply is not JSON: {e}"))?;
    let field = |name: &str| shown[name].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(Shown { speaker: field("speaker"), text: field("text").unwrap_or_default(), translation: field("translation") })
}

/// Model ids change every few months, so ask the API which Flash-Lite models
/// exist and take the highest version, preferring a stable one over a preview.
fn newest_flash_lite(key: &str) -> Result<String, String> {
    let mut response =
        ureq::get(&format!("{API}/models?pageSize=1000")).header("x-goog-api-key", key).call().map_err(api_error)?;
    let list: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    let mut best: Option<((bool, Vec<u32>), String)> = None;
    for model in list["models"].as_array().into_iter().flatten() {
        let name = model["name"].as_str().unwrap_or("").trim_start_matches("models/");
        let generates = model["supportedGenerationMethods"]
            .as_array()
            .is_some_and(|m| m.iter().any(|v| v == "generateContent"));
        if !generates || !name.contains("flash-lite") || ["tts", "image", "live", "audio"].iter().any(|w| name.contains(w)) {
            continue;
        }
        let rank = (!name.contains("preview"), version_of(name));
        if best.as_ref().is_none_or(|b| rank > b.0) {
            best = Some((rank, name.to_string()));
        }
    }
    best.map(|b| b.1).ok_or_else(|| "the API lists no Flash-Lite model; pass --model".to_string())
}

/// "gemini-3.1-flash-lite" -> [3, 1]
fn version_of(name: &str) -> Vec<u32> {
    name.split('-').nth(1).unwrap_or("").split('.').filter_map(|n| n.parse().ok()).collect()
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_parsing() {
        let reply = json!({"candidates": [{"content": {"parts": [{"text": "[\"a\", \"b\"]"}]}}]});
        assert_eq!(parse_reply(&reply).unwrap(), vec!["a", "b"]);
        assert!(parse_reply(&json!({"error": {"message": "nope"}})).is_err());
    }

    #[test]
    fn shown_parsing() {
        let reply = |text: &str| json!({"candidates": [{"content": {"parts": [{"text": text}]}}]});
        let shown = parse_shown(&reply(r#"{"speaker": "", "text": " 一行目\n二行目 ", "translation": "x"}"#)).unwrap();
        assert_eq!(shown, Shown { speaker: None, text: "一行目\n二行目".to_string(), translation: Some("x".to_string()) });
        assert_eq!(parse_shown(&reply(r#"{"speaker": "誰か", "text": ""}"#)).unwrap().text, "");
    }

    #[test]
    fn cache_escaping_round_trips() {
        let s = "line one\nline\\two\tend";
        assert_eq!(unescape(&escape(s)), s);
    }

    #[test]
    fn versions_order() {
        assert!(version_of("gemini-3.1-flash-lite") > version_of("gemini-2.5-flash-lite"));
    }
}
