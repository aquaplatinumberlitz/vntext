//! Translation by one of several services, with a disk cache so a line is
//! only ever paid for once.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use serde_json::{json, Value};

const GEMINI: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Google Translate as its browser extension asks it: no key, no promise
/// that it stays. It takes several texts at once and answers with a JSON
/// array of their translations.
const GOOGLE: &str = "https://clients5.google.com/translate_a/t?client=dict-chrome-ex&sl=ja";
/// Google turns away whoever does not look like a browser.
const BROWSER: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";
const CLAUDE_VERSION: &str = "2023-06-01";
/// Room for a reply: the translations of a dozen passages and, with models
/// that think first, their thinking.
const CLAUDE_REPLY: u32 = 16000;

/// Who can translate.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Service {
    Gemini,
    OpenAi,
    DeepSeek,
    Claude,
    /// any server that speaks the way OpenAI's does: Ollama, LM Studio, a proxy
    Local,
    Google,
}

/// What is known of a service before the reader sets it up.
pub struct Known {
    pub service: Service,
    /// its name on the command line and in the file of setups
    pub id: &'static str,
    pub label: &'static str,
    /// the variable of the environment its key is also taken from
    pub key_var: &'static str,
    pub base: &'static str,
    /// the model taken when none is named; empty if one must be named
    pub model: &'static str,
}

pub const SERVICES: [Known; 6] = [
    // without a model named, the newest Flash-Lite the API offers
    Known { service: Service::Gemini, id: "gemini", label: "Gemini", key_var: "GEMINI_API_KEY", base: "", model: "" },
    Known { service: Service::OpenAi, id: "openai", label: "OpenAI", key_var: "OPENAI_API_KEY", base: "https://api.openai.com/v1", model: "" },
    Known { service: Service::DeepSeek, id: "deepseek", label: "DeepSeek", key_var: "DEEPSEEK_API_KEY", base: "https://api.deepseek.com", model: "deepseek-chat" },
    Known { service: Service::Claude, id: "claude", label: "Claude", key_var: "ANTHROPIC_API_KEY", base: "https://api.anthropic.com/v1", model: "claude-opus-5-5" },
    Known { service: Service::Local, id: "local", label: "Local", key_var: "", base: "http://127.0.0.1:11434/v1", model: "" },
    Known { service: Service::Google, id: "google", label: "Google Dịch", key_var: "", base: "", model: "" },
];

impl Service {
    pub fn known(self) -> &'static Known {
        SERVICES.iter().find(|k| k.service == self).unwrap()
    }

    pub fn named(id: &str) -> Option<Service> {
        SERVICES.iter().find(|k| k.id == id).map(|k| k.service)
    }

    /// Whether it asks for a key: Google Translate has none, and a server
    /// of one's own may or may not want one.
    pub fn keyed(self) -> bool {
        !matches!(self, Service::Local | Service::Google)
    }
}

/// How the reader has set a service up.
#[derive(Clone, PartialEq, Debug)]
pub struct Setup {
    pub service: Service,
    pub key: String,
    pub model: String,
    pub base: String,
}

/// Counts the times the setups were saved, for those who translate to see
/// that they must look again.
static SAVED: AtomicU64 = AtomicU64::new(0);

pub fn saves() -> u64 {
    SAVED.load(Ordering::Relaxed)
}

/// The file of setups: `{"use": id, "services": {id: {key, model, base}}}`.
fn setups_file() -> PathBuf {
    crate::ocr::config_dir("XDG_CONFIG_HOME", ".config").join("translate.json")
}

fn setups() -> Value {
    let file = fs::read_to_string(setups_file()).unwrap_or_default();
    serde_json::from_str(&file).ok().filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// The service the reader has chosen; Gemini until one is.
pub fn chosen() -> Service {
    setups()["use"].as_str().and_then(Service::named).unwrap_or(Service::Gemini)
}

/// A service as set up: what was saved, or else its key from the
/// environment and what is known of it.
pub fn setup(service: Service) -> Setup {
    let known = service.known();
    let saved = &setups()["services"][known.id];
    let field = |name: &str| saved[name].as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let from_environment = || std::env::var(known.key_var).ok().filter(|k| !k.is_empty());
    // a Gemini key entered before there was a file of setups
    let earlier = || fs::read_to_string(setups_file().with_file_name("gemini.key")).ok().filter(|_| service == Service::Gemini).map(|k| k.trim().to_string());
    Setup {
        service,
        key: field("key").or_else(from_environment).or_else(earlier).unwrap_or_default(),
        model: field("model").unwrap_or_else(|| known.model.to_string()),
        base: field("base").unwrap_or_else(|| known.base.to_string()),
    }
}

/// Keeps a setup for later runs and makes its service the one chosen.
pub fn save_setup(setup: &Setup) -> std::io::Result<()> {
    let known = setup.service.known();
    let mut all = setups();
    all["use"] = json!(known.id);
    all["services"][known.id] = json!({"key": setup.key.trim(), "model": setup.model.trim(), "base": setup.base.trim().trim_end_matches('/')});
    write_setups(&all)
}

/// Makes a service the one chosen, if it is set up well enough to
/// translate. What is wrong is said for the reader of the overlay.
pub fn choose(service: Service) -> Result<(), String> {
    let (setup, label) = (setup(service), service.known().label);
    if service.keyed() && setup.key.is_empty() {
        return Err(format!("{label} chưa có API key. Bấm nút ổ khoá để nhập."));
    }
    if setup.model.is_empty() && !matches!(service, Service::Gemini | Service::Google) {
        return Err(format!("{label} chưa có tên model. Bấm nút ổ khoá để ghi."));
    }
    let mut all = setups();
    all["use"] = json!(service.known().id);
    write_setups(&all).map_err(|e| format!("Không lưu được lựa chọn: {e}"))
}

/// Writes the file of setups, which only its owner can read since it holds
/// keys, and lets those who translate know.
fn write_setups(all: &Value) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = setups_file();
    fs::create_dir_all(file.parent().unwrap())?;
    let _ = fs::remove_file(&file);
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(&file)?.write_all(format!("{all:#}\n").as_bytes())?;
    SAVED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The service that translates in this run, as its place in `SERVICES`;
/// none while nothing is translated.
static AT_WORK: AtomicUsize = AtomicUsize::new(usize::MAX);

pub fn at_work() -> Option<Service> {
    SERVICES.get(AT_WORK.load(Ordering::Relaxed)).map(|known| known.service)
}

pub fn set_at_work(service: Service) {
    AT_WORK.store(SERVICES.iter().position(|known| known.service == service).unwrap(), Ordering::Relaxed);
}

/// The models a service offers to one with the key in `setup`, by name.
pub fn models(setup: &Setup) -> Result<Vec<String>, String> {
    let base = setup.base.trim().trim_end_matches('/');
    let (key, known) = (setup.key.trim(), setup.service.known());
    if setup.service.keyed() && key.is_empty() {
        return Err(format!("{} needs its API key first", known.label));
    }
    let asked = match setup.service {
        Service::Google => return Ok(Vec::new()),
        Service::Gemini => ureq::get(&format!("{GEMINI}/models?pageSize=1000")).header("x-goog-api-key", key).call(),
        Service::Claude => ureq::get(&format!("{base}/models?limit=1000")).header("x-api-key", key).header("anthropic-version", CLAUDE_VERSION).call(),
        Service::OpenAi | Service::DeepSeek | Service::Local => {
            let request = ureq::get(&format!("{base}/models"));
            if key.is_empty() { request.call() } else { request.header("Authorization", &format!("Bearer {key}")).call() }
        }
    };
    let reply: Value = asked.map_err(api_error)?.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(model_names(&reply))
}

/// The names in a list of models: Gemini's under `models`, each a `name`
/// that starts with `models/`; the others' under `data`, each an `id`.
fn model_names(reply: &Value) -> Vec<String> {
    let gemini = reply["models"].as_array().into_iter().flatten().filter(|m| {
        // those that write text, not those that only embed or draw
        m["supportedGenerationMethods"].as_array().is_some_and(|ways| ways.iter().any(|w| w == "generateContent"))
    });
    let gemini = gemini.filter_map(|m| m["name"].as_str()).map(|name| name.trim_start_matches("models/").to_string());
    let others = reply["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).map(str::to_string);
    let mut names: Vec<String> = gemini.chain(others).collect();
    names.sort();
    names
}

pub struct Line {
    pub speaker: Option<String>,
    pub text: String,
}

/// What a request needs; cheap to clone so that requests can run without
/// holding on to the cache.
#[derive(Clone)]
pub struct Client {
    pub service: Service,
    key: String,
    pub model: String,
    base: String,
    language: String,
    /// the language as Google wants it: `vi`, `en`
    code: String,
}

pub struct Translator {
    pub client: Client,
    cache: HashMap<String, String>,
    cache_path: PathBuf,
    /// texts whose translation is Google's, kept for this run only
    google: HashSet<String>,
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

impl Client {
    /// The client of `service`, or of the service chosen, as it is set up.
    /// `model` is taken over the one set up. A service that lacks its key
    /// gives way to Google Translate.
    pub fn new(language: &str, model: Option<String>, service: Option<Service>) -> Result<Client, String> {
        let mut setup = setup(service.unwrap_or_else(chosen));
        if let Some(model) = model {
            setup.model = model;
        }
        if setup.service.keyed() && setup.key.is_empty() {
            eprintln!("\x1b[33mno API key for {}: translating with Google Translate\x1b[0m", setup.service.known().label);
            setup = Setup { service: Service::Google, key: String::new(), model: String::new(), base: String::new() };
        }
        let model = match setup.service {
            Service::Google => "Google Translate".to_string(),
            Service::Gemini if setup.model.is_empty() => newest_flash_lite(&setup.key)?,
            _ if setup.model.is_empty() => return Err(format!("name a model for {}", setup.service.known().label)),
            _ => setup.model,
        };
        Ok(Client { service: setup.service, key: setup.key, model, base: setup.base, language: language_name(language).to_string(), code: language.to_string() })
    }
}

impl Translator {
    pub fn new(language: &str, model: Option<String>, service: Option<Service>) -> Result<Translator, String> {
        let client = Client::new(language, model, service)?;
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
        Ok(Translator { client, cache, cache_path, google: HashSet::new() })
    }

    pub fn cached(&self, text: &str) -> Option<&String> {
        self.cache.get(text)
    }

    pub fn by_google(&self, text: &str) -> bool {
        self.google.contains(text)
    }

    /// Caches translations in memory, and a model's on disk as well:
    /// Google's are the lesser ones and not worth keeping from a later run.
    pub fn store(&mut self, lines: &[Line], translations: Vec<String>, via: Via) {
        let mut file = OpenOptions::new().create(true).append(true).open(&self.cache_path).ok().filter(|_| via == Via::Model);
        for (line, tr) in lines.iter().zip(translations) {
            if via == Via::Google {
                self.google.insert(line.text.clone());
            }
            if let Some(f) = file.as_mut() {
                let _ = writeln!(f, "{}\t{}", escape(&line.text), escape(&tr));
            }
            self.cache.insert(line.text.clone(), tr);
        }
    }
}

/// Who made a translation: a language model, or Google Translate.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Via {
    Model,
    Google,
}

impl Client {
    /// Translates `lines` with Google Translate, each on its own: it knows
    /// neither who speaks nor what was said before.
    pub fn google(&self, lines: &[Line]) -> Result<Vec<String>, String> {
        let mut form: Vec<(&str, &str)> = vec![("tl", &self.code)];
        form.extend(lines.iter().map(|l| ("q", l.text.as_str())));
        let mut response = ureq::post(GOOGLE).header("User-Agent", BROWSER).send_form(form).map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        google_reply(&reply, lines.len())
    }

    /// Translates `lines` in one request. `context` holds the lines shown
    /// just before, for the model's reference only.
    pub fn translate(&self, context: &[String], lines: &[Line]) -> Result<(Vec<String>, Via), String> {
        let input = json!({
            "previous_lines": context,
            "lines": lines.iter().map(|l| json!({"speaker": l.speaker, "text": l.text})).collect::<Vec<_>>(),
        })
        .to_string();
        let translations = match self.service {
            Service::Google => return self.google(lines).map(|tr| (tr, Via::Google)),
            Service::Gemini => self.gemini(&input)?,
            Service::Claude => self.claude(&input)?,
            Service::OpenAi | Service::DeepSeek | Service::Local => self.chat(&input)?,
        };
        if translations.len() != lines.len() {
            return Err(format!("asked for {} translations, got {}", lines.len(), translations.len()));
        }
        Ok((translations, Via::Model))
    }

    fn instructions(&self) -> String {
        format!(
            "You translate lines from a Japanese visual novel into {}. \
             The input is JSON: `previous_lines` is earlier text for context only, `lines` is what to translate. \
             Reply with a JSON array of strings holding the translation of each entry of `lines`, \
             in the same order and with exactly the same number of entries, and with nothing before or after the array. \
             An entry is one passage by one speaker and may hold several sentences on separate lines: \
             translate it as a whole and keep its line breaks. \
             Entries without a speaker are narration; use the surrounding lines to tell who does what. \
             Translate faithfully and naturally, keep the speaker's tone, never merge or split entries, and add no notes.",
            self.language
        )
    }

    fn gemini(&self, input: &str) -> Result<Vec<String>, String> {
        let body = json!({
            "systemInstruction": {"parts": [{"text": self.instructions()}]},
            "contents": [{"role": "user", "parts": [{"text": input}]}],
            "generationConfig": {
                "temperature": 0.3,
                "responseMimeType": "application/json",
                "responseSchema": {"type": "ARRAY", "items": {"type": "STRING"}},
            },
        });
        let url = format!("{GEMINI}/models/{}:generateContent", self.model);
        let mut response = ureq::post(&url).header("x-goog-api-key", &self.key).send_json(&body).map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        parse_reply(&reply)
    }

    /// The chat API that OpenAI's servers speak, and DeepSeek's, Ollama's
    /// and many others' after them. Nothing is asked of it beyond a model
    /// and messages, since what else a server takes differs from one to the
    /// next.
    fn chat(&self, input: &str) -> Result<Vec<String>, String> {
        let body = json!({
            "model": self.model,
            "messages": [{"role": "system", "content": self.instructions()}, {"role": "user", "content": input}],
        });
        let mut request = ureq::post(&format!("{}/chat/completions", self.base));
        if !self.key.is_empty() {
            request = request.header("Authorization", &format!("Bearer {}", self.key));
        }
        let mut response = request.send_json(&body).map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        let text = reply["choices"][0]["message"]["content"].as_str().ok_or_else(|| unexpected(&reply))?;
        strings_in(text)
    }

    /// Anthropic's Messages API. The effort asked for is low, a translation
    /// needing little thought and the reader waiting for it; models that do
    /// not know of effort are asked again without.
    fn claude(&self, input: &str) -> Result<Vec<String>, String> {
        let ask = |effort: bool| {
            let mut body = json!({
                "model": self.model,
                "max_tokens": CLAUDE_REPLY,
                "system": self.instructions(),
                "messages": [{"role": "user", "content": input}],
            });
            if effort {
                body["output_config"] = json!({"effort": "low"});
            }
            ureq::post(&format!("{}/messages", self.base)).header("x-api-key", &self.key).header("anthropic-version", CLAUDE_VERSION).send_json(&body)
        };
        let mut response = match ask(true) {
            Err(ureq::Error::StatusCode(400)) => ask(false),
            other => other,
        }
        .map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        if reply["stop_reason"] == "refusal" {
            return Err("Claude declined to translate this".to_string());
        }
        // the reply is a list of blocks, of which those of text are the answer
        let blocks = reply["content"].as_array().ok_or_else(|| unexpected(&reply))?;
        let text: String = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
        strings_in(&text)
    }
}

fn unexpected(reply: &Value) -> String {
    format!("no text in reply: {}", reply.to_string().chars().take(200).collect::<String>())
}

/// The JSON array of strings in a model's answer, which may come wrapped in
/// a code fence or a sentence although none was asked for.
fn strings_in(text: &str) -> Result<Vec<String>, String> {
    let array = text.find('[').zip(text.rfind(']')).filter(|(from, to)| from < to).map(|(from, to)| &text[from..=to]);
    serde_json::from_str(array.unwrap_or(text)).map_err(|e| format!("reply is not a JSON array of strings: {e}"))
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
        if self.service != Service::Gemini {
            return Err("reading pictures needs a Gemini API key: enter one with the key button of the overlay".to_string());
        }
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
        let url = format!("{GEMINI}/models/{}:generateContent", self.model);
        let mut response = ureq::post(&url).header("x-goog-api-key", &self.key).send_json(&body).map_err(api_error)?;
        let reply: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        parse_shown(&reply)
    }
}

fn reply_text(reply: &Value) -> Result<&str, String> {
    reply["candidates"][0]["content"]["parts"][0]["text"].as_str().ok_or_else(|| unexpected(reply))
}

fn parse_reply(reply: &Value) -> Result<Vec<String>, String> {
    serde_json::from_str(reply_text(reply)?).map_err(|e| format!("reply is not a JSON array of strings: {e}"))
}

/// Google answers several texts with an array of strings, and one text
/// with an array of one.
fn google_reply(reply: &Value, wanted: usize) -> Result<Vec<String>, String> {
    let translations: Vec<String> = reply.as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect();
    if translations.len() != wanted {
        return Err(format!("Google gave {} translations for {wanted} texts", translations.len()));
    }
    Ok(translations)
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
        ureq::get(&format!("{GEMINI}/models?pageSize=1000")).header("x-goog-api-key", key).call().map_err(api_error)?;
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
    fn arrays_in_answers() {
        assert_eq!(strings_in(r#"["a", "b"]"#).unwrap(), vec!["a", "b"]);
        assert_eq!(strings_in("Here you go:\n```json\n[\"một [x]\"]\n```").unwrap(), vec!["một [x]"]);
        assert!(strings_in("sorry").is_err());
    }

    #[test]
    fn lists_of_models() {
        let gemini = json!({"models": [
            {"name": "models/gemini-b", "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/embed", "supportedGenerationMethods": ["embedContent"]},
            {"name": "models/gemini-a", "supportedGenerationMethods": ["generateContent", "countTokens"]}
        ]});
        assert_eq!(model_names(&gemini), ["gemini-a", "gemini-b"]);
        assert_eq!(model_names(&json!({"data": [{"id": "m2"}, {"id": "m1"}]})), ["m1", "m2"]);
        assert!(model_names(&json!({"error": "no"})).is_empty());
    }

    #[test]
    fn services_by_name() {
        assert_eq!(Service::named("deepseek"), Some(Service::DeepSeek));
        assert_eq!(Service::named("bing"), None);
        assert!(Service::Claude.keyed() && !Service::Local.keyed());
    }

    #[test]
    fn google_replies() {
        assert_eq!(google_reply(&json!(["một", "hai"]), 2).unwrap(), vec!["một", "hai"]);
        assert!(google_reply(&json!(["một"]), 2).is_err());
        assert!(google_reply(&json!({"error": 1}), 1).is_err());
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
