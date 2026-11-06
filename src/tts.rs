//! A.I.VOICE2's local HTTP client. Extraction never waits for synthesis.
//! One request runs at a time, with only the latest sentence pending.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use crate::{mem, ocr, screen::X11, web};
#[path = "tts_audio.rs"]
mod audio;

const ENDPOINT: &str = "http://127.0.0.1:17862";
const AUDIO_LIMIT: usize = 16 << 20;
const TEXT_LIMIT: usize = 4000;
const SAMPLE: &str = "こんにちは。今日はいい天気ですね。";
static ACTIVE: OnceLock<Mutex<Weak<Inner>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub endpoint: String,
    pub voice: String,
    pub automatic: bool,
    pub volume: f64,
    pub prepare: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { endpoint: ENDPOINT.to_string(), voice: String::new(), automatic: false, volume: 0.8, prepare: true }
    }
}

impl Settings {
    fn decode(root: &Value, key: &str) -> Self {
        let saved = &root["games"][key];
        Self {
            endpoint: root["endpoint"].as_str().filter(|s| endpoint(s).is_ok()).unwrap_or(ENDPOINT).trim_end_matches('/').to_string(),
            voice: saved["voice"].as_str().unwrap_or_default().to_string(),
            automatic: saved["automatic"] == true,
            volume: saved["volume"].as_f64().filter(|v| (0.0..=1.0).contains(v)).unwrap_or(0.8),
            prepare: saved["prepare"].as_bool().unwrap_or(true),
        }
    }

    fn save(&self, key: &str) -> Result<(), String> {
        let path = ocr::config_dir("XDG_CONFIG_HOME", ".config").join("tts.json");
        let mut root = std::fs::read(&path).ok().and_then(|s| serde_json::from_slice::<Value>(&s).ok())
            .filter(Value::is_object).unwrap_or_else(|| json!({"version":1,"games":{}}));
        if !root["games"].is_object() { root["games"] = json!({}); }
        root["version"] = json!(1);
        root["endpoint"] = json!(self.endpoint);
        root["games"][key] = json!({"voice":self.voice,"automatic":self.automatic,"volume":self.volume,"prepare":self.prepare});
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temporary, format!("{root:#}\n")).and_then(|_| std::fs::rename(&temporary, &path))
            .map_err(|_| "Không lưu được cấu hình giọng đọc.".to_string())
    }
}

/// The public client accepts local services only; never follow a redirect with game text.
fn endpoint(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    let authority = value.strip_prefix("http://").unwrap_or_default();
    let valid = ["127.0.0.1", "localhost", "[::1]"].iter().any(|host| {
        authority.strip_prefix(&format!("{host}:")).is_some_and(|port| port.parse::<u16>().is_ok_and(|p| p != 0))
    });
    if valid { Ok(value.to_string()) } else { Err("Dùng địa chỉ cục bộ dạng http://127.0.0.1:17862.".to_string()) }
}

#[derive(Clone, Debug)]
pub struct Voice { pub id: String, pub name: String }
#[derive(Clone, Debug)]
pub struct Catalog { pub voices: Vec<Voice>, pub default: String, pub streaming: bool, pub preparing: bool }

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).proxy(None).max_redirects(0).build().into()
}

fn failure(error: ureq::Error) -> String {
    eprintln!("tts: local HTTP failure: {error}");
    match error {
        ureq::Error::StatusCode(400) => "A.I.VOICE2 từ chối câu hoặc giọng đã chọn. Làm mới danh sách giọng.".to_string(),
        ureq::Error::StatusCode(503) => "A.I.VOICE2 đang bận hoặc chưa sẵn sàng. Thử lại sau.".to_string(),
        ureq::Error::Timeout(_) => "A.I.VOICE2 trả lời quá lâu. Đã bỏ lần nghe này; có thể thử lại câu mới.".to_string(),
        _ => "Không nhận được phản hồi từ A.I.VOICE2. Kiểm tra dịch vụ và địa chỉ kết nối.".to_string(),
    }
}

pub fn catalog(base: &str) -> Result<Catalog, String> {
    let base = endpoint(base)?;
    let client = agent(Duration::from_secs(4));
    let mut health = client.get(format!("{base}/healthz")).header("Connection", "close").call().map_err(failure)?;
    let health: Value = health.body_mut().with_config().limit(65536).read_json().map_err(failure)?;
    if health["ready"] != true { return Err("A.I.VOICE2 chưa sẵn sàng.".to_string()); }
    let mut response = client.get(format!("{base}/v1/voices")).header("Connection", "close").call().map_err(failure)?;
    let value: Value = response.body_mut().with_config().limit(1 << 20).read_json().map_err(failure)?;
    let mut voices: Vec<Voice> = value["voices"].as_array().into_iter().flatten().filter_map(|item| {
        let id = item["id"].as_str()?.to_string();
        let raw = item["name"].as_str().unwrap_or(&id);
        let dialect = if item["language"] == "kansai" { "Kansai" } else { "Standard" };
        let family = [("KotonohaAkane_", "Akane"), ("akane_west_", "Akane"),
            ("KotonohaAoi_", "Aoi"), ("aoi_", "Aoi"), ("galaco_", "Galaco")];
        let label = family.iter().find_map(|(prefix, name)| raw.strip_prefix(prefix).map(|variant| {
            format!("{name} · {}", variant.trim_end_matches("_48"))
        })).unwrap_or_else(|| raw.to_string());
        let name = format!("{label} · {dialect}");
        Some(Voice { id, name })
    }).collect();
    voices.sort_by(|a,b| a.name.cmp(&b.name));
    if voices.is_empty() { return Err("Chưa có giọng nào. Thêm voicebank trong trình quản lý A.I.VOICE2.".to_string()); }
    let capability = |name| health["capabilities"].as_array().is_some_and(|items| items.iter().any(|item| item == name));
    Ok(Catalog { voices, default: value["default_voice"].as_str().unwrap_or_default().to_string(), streaming: capability("pcm-stream-v1"), preparing: capability("prepare-v1") })
}

fn sentence(text: &str) -> Result<String, String> {
    let text = text.trim().replace('\0', "");
    if text.is_empty() { return Err("Chưa có câu Nhật để nghe.".to_string()); }
    if text.chars().count() > TEXT_LIMIT { return Err("Đoạn quá dài. Chọn tối đa 4.000 ký tự để nghe.".to_string()); }
    Ok(text)
}

fn wav(data: &[u8]) -> Result<(), String> {
    let bad = || "A.I.VOICE2 trả về file WAV không hợp lệ.".to_string();
    if data.len() < 44 || data.len() > AUDIO_LIMIT || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" { return Err(bad()); }
    let length = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize + 8;
    if length != data.len() { return Err(bad()); }
    let (mut at, mut format, mut samples) = (12, false, false);
    while at + 8 <= data.len() {
        let size = u32::from_le_bytes(data[at+4..at+8].try_into().unwrap()) as usize;
        let chunk = data.get(at+8..at+8+size).ok_or_else(bad)?;
        if &data[at..at+4] == b"fmt " {
            format = chunk.len() >= 16 && chunk[..4] == [1,0,1,0]
                && u32::from_le_bytes(chunk[4..8].try_into().unwrap()) == 48000
                && u16::from_le_bytes(chunk[14..16].try_into().unwrap()) == 16;
        }
        if &data[at..at+4] == b"data" { samples = size > 0 && size % 2 == 0; }
        at += 8 + size + size % 2;
    }
    if format && samples && at == data.len() { Ok(()) } else { Err(bad()) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase { Idle, Waiting, Making, Playing, Error }
#[derive(Clone, Debug, PartialEq)]
pub struct Status { pub phase: Phase, pub text: String }
impl Status {
    fn new(phase: Phase, text: &str) -> Self { Self { phase, text: text.to_string() } }
    pub fn busy(&self) -> bool { matches!(self.phase, Phase::Waiting | Phase::Making | Phase::Playing) }
}

#[derive(Clone)]
struct Job { generation: u64, text: String, settings: Settings, queued: std::time::Instant }
struct Playback { child: Child, _file: File }
impl Drop for Playback {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
struct State {
    settings: Settings,
    generation: u64,
    pending: Option<Job>,
    player: Option<Playback>,
    output: Option<audio::Player>,
    status: Status,
    previous: Option<(String, String)>,
    suspended: bool,
    quit: bool,
    cached: Option<(String, String, String, std::time::Instant, Vec<u8>)>,
    preparing: Option<Preparation>,
    next: Option<String>,
    history_cache: std::collections::VecDeque<(String, String, String, std::time::Instant, Vec<u8>)>,
}
#[derive(Clone)]
struct Preparation { revision: u64, settings: Settings, text: Option<String>, warm: bool }
impl State {
    fn cancel(&mut self) {
        self.generation += 1;
        self.pending = None;
        self.player = None;
        if self.status.busy() { self.output = None; }
        self.preparing = Some(Preparation { revision: self.generation, settings: self.settings.clone(), text: None, warm: false });
        self.status = Status::new(Phase::Idle, "Sẵn sàng");
    }
    fn queue(&mut self, text: &str) -> Result<(), String> {
        self.cancel();
        if self.settings.voice.is_empty() { return Err("Chọn giọng trong tab Giọng đọc trước.".to_string()); }
        let text = sentence(text)?;
        self.pending = Some(Job { generation: self.generation, text, settings: self.settings.clone(), queued: std::time::Instant::now() });
        self.status = Status::new(Phase::Waiting, "Đang chờ tạo giọng…");
        Ok(())
    }
}
struct Inner { state: Mutex<State>, wake: Condvar, key: String, player: Vec<String>, session: String }
#[derive(Clone)]
pub struct Control(Arc<Inner>);

fn identity(pid: u32) -> String {
    let name = mem::name(pid);
    if !name.eq_ignore_ascii_case("ryujinx") { return name; }
    let title = X11::open().ok().and_then(|x| x.windows().into_iter().find(|w| w.pid == pid)).map(|w| w.title).unwrap_or_default();
    title_id(&title).map(|id| format!("Ryujinx/{id}")).unwrap_or_else(|| format!("Ryujinx/session-{pid}"))
}
fn title_id(title: &str) -> Option<String> {
    title.split(|c: char| !c.is_ascii_hexdigit()).find(|s| s.len() == 16 && s.starts_with("0100")).map(str::to_uppercase)
}

pub fn start(hub: &web::Hub, pid: u32) -> Control {
    let key = identity(pid);
    let path = ocr::config_dir("XDG_CONFIG_HOME", ".config").join("tts.json");
    let saved = std::fs::read(path).ok().and_then(|s| serde_json::from_slice(&s).ok()).unwrap_or(Value::Null);
    let control = Control::new(Settings::decode(&saved, &key), key, vec!["pw-play".to_string()]);
    control.suspend(true);
    {
        let mut state = control.0.state.lock().unwrap();
        state.preparing = Some(Preparation { revision: state.generation, settings: state.settings.clone(), text: None, warm: true });
    }
    *ACTIVE.get_or_init(|| Mutex::new(Weak::new())).lock().unwrap() = Arc::downgrade(&control.0);
    let (events, kept) = (hub.subscribe(), control.clone());
    thread::spawn(move || {
        let mut checked = std::time::Instant::now();
        loop {
            match events.recv_timeout(Duration::from_millis(40)) {
                Ok(event) => kept.event(&event),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => { kept.shutdown(); break; }
                Err(_) => {},
            }
            kept.poll();
            if kept.0.state.lock().unwrap().quit { break; }
            if kept.0.key.starts_with("Ryujinx/") && checked.elapsed() >= Duration::from_secs(2) {
                checked = std::time::Instant::now();
                let current = identity(pid);
                if !current.contains("/session-") && current != kept.0.key {
                    kept.shutdown();
                    kept.error("Game trong giả lập đã đổi. Dừng theo dõi và mở lại phụ đề cho game mới.");
                    break;
                }
            }
        }
    });
    control
}

impl Control {
    fn new(settings: Settings, key: String, player: Vec<String>) -> Self {
        let control = Self(Arc::new(Inner {
            state: Mutex::new(State { settings, generation: 0, pending: None, player: None,
                status: Status::new(Phase::Idle, "Sẵn sàng"), previous: None, suspended: false, quit: false, cached: None,
                output: None, preparing: None, next: None, history_cache: std::collections::VecDeque::new() }),
            wake: Condvar::new(), key, player, session: format!("vntext-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()),
        }));
        let kept = control.clone();
        thread::spawn(move || kept.worker());
        let kept = control.clone();
        thread::spawn(move || kept.preparer());
        control
    }
    pub fn key(&self) -> &str { &self.0.key }
    pub fn settings(&self) -> Settings { self.0.state.lock().unwrap().settings.clone() }
    pub fn status(&self) -> Status { self.0.state.lock().unwrap().status.clone() }
    pub fn paused(&self) -> bool { self.0.state.lock().unwrap().suspended }
    pub fn error(&self, text: &str) { self.0.state.lock().unwrap().status = Status::new(Phase::Error, text); }
    pub fn apply(&self, mut settings: Settings) -> Result<(), String> {
        settings.endpoint = endpoint(&settings.endpoint)?;
        if !(0.0..=1.0).contains(&settings.volume) { return Err("Âm lượng phải từ 0 đến 100%.".to_string()); }
        self.change(settings.clone())?;
        settings.save(&self.0.key)
    }
    fn change(&self, settings: Settings) -> Result<(), String> {
        let mut state = self.0.state.lock().unwrap();
        if state.quit { return Err("Phiên đọc đã kết thúc. Mở lại phụ đề cho game.".to_string()); }
        if state.settings.voice != settings.voice || state.settings.endpoint != settings.endpoint { state.cached = None; state.history_cache.clear(); }
        state.cancel();
        state.output = None;
        state.settings = settings;
        let current = state.previous.as_ref().map(|(_,text)| text.clone())
            .filter(|text| state.settings.prepare && !state.suspended && text.chars().count() <= 160);
        state.preparing = Some(Preparation { revision: state.generation, settings: state.settings.clone(), warm: current.is_none(), text: current });
        Ok(())
    }
    pub fn stop(&self) { self.0.state.lock().unwrap().cancel(); }
    pub fn suspend(&self, suspended: bool) {
        let mut state = self.0.state.lock().unwrap();
        if suspended && !state.suspended { state.cancel(); }
        state.suspended = suspended;
    }
    pub fn speak(&self, text: &str) {
        let mut state = self.0.state.lock().unwrap();
        if state.quit { return; }
        if let Err(error) = state.queue(text) { state.status = Status::new(Phase::Error, &error); }
        self.0.wake.notify_one();
    }
    pub fn sample(&self) { self.speak(SAMPLE); }
    fn event(&self, raw: &str) {
        let Ok(event) = serde_json::from_str::<Value>(raw) else { return };
        if event["type"] != "message" || !matches!(event["source"].as_str(), Some("profile" | "ocr")) { return; }
        let text = event["lines"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join("\n").trim().replace('\0', "");
        let name = event["name"].as_str().unwrap_or_default().to_string();
        let mut state = self.0.state.lock().unwrap();
        if state.quit || state.previous.as_ref() == Some(&(name.clone(), text.clone())) { return; }
        state.previous = Some((name, text.clone()));
        state.cancel();
        state.next = event["next_lines"].as_array().map(|lines| lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
            .filter(|text| !text.is_empty() && text.chars().count() <= 160);
        if state.settings.automatic && !state.suspended {
            if let Err(error) = state.queue(&text) { state.status = Status::new(Phase::Error, &error); }
            self.0.wake.notify_one();
        } else if state.settings.prepare && !state.suspended && !state.settings.voice.is_empty() && text.chars().count() <= 160 {
            state.preparing = Some(Preparation { revision: state.generation, settings: state.settings.clone(), text: Some(text), warm: false });
        }
    }
    fn poll(&self) {
        let mut state = self.0.state.lock().unwrap();
        if let Some(output) = state.output.as_mut() {
            if output.failed() {
                state.output = None;
                state.status = Status::new(Phase::Error, "Không phát được audio. Kiểm tra PipeWire và thiết bị đầu ra.");
            } else if output.finished() && state.status.phase == Phase::Playing {
                state.status = Status::new(Phase::Idle, "Sẵn sàng");
            }
        }
        let finished = state.player.as_mut().and_then(|player| player.child.try_wait().ok().flatten());
        if let Some(status) = finished {
            state.player = None;
            state.status = if status.success() { Status::new(Phase::Idle, "Sẵn sàng") }
                else { Status::new(Phase::Error, "Không phát được âm thanh. Kiểm tra PipeWire và thiết bị đầu ra.") };
        }
    }
    pub fn shutdown(&self) {
        let mut state = self.0.state.lock().unwrap();
        state.cancel(); state.output = None; state.cached = None; state.history_cache.clear(); state.quit = true;
        self.0.wake.notify_one();
    }
    fn preparer(&self) {
        loop {
            let (preparation, quit) = {
                let mut state = self.0.state.lock().unwrap();
                (state.preparing.take(), state.quit)
            };
            if let Some(work) = preparation.filter(|work| !work.settings.voice.is_empty()) {
                if let Ok(list) = catalog(&work.settings.endpoint) {
                    if list.preparing {
                        let client = agent(Duration::from_secs(4));
                        let body = json!({"session":self.0.session,"revision":work.revision});
                        let _ = client.post(format!("{}/v1/cancel",work.settings.endpoint)).header("Connection", "close").send_json(body);
                        if !quit && (work.warm || work.text.is_some()) {
                            let body = json!({"session":self.0.session,"revision":work.revision,"voice":work.settings.voice,
                                "mode":if work.warm { "warm" } else { "text" },"text":work.text});
                            let _ = client.post(format!("{}/v1/prepare",work.settings.endpoint)).header("Connection", "close").send_json(body);
                        }
                    }
                }
            }
            if quit { break; }
            thread::sleep(Duration::from_millis(40));
        }
    }
    fn begin_pcm(&self, job: &Job) -> Result<std::sync::mpsc::SyncSender<Vec<u8>>, String> {
        let mut state = self.0.state.lock().unwrap();
        if state.quit || state.generation != job.generation { return Err("Đã bỏ câu cũ.".to_string()); }
        if state.output.is_none() { state.output = Some(audio::Player::new(job.settings.volume)?); }
        let sender = state.output.as_ref().unwrap().begin();
        state.status = Status::new(Phase::Playing, "Đang phát câu Nhật");
        Ok(sender)
    }
    fn stream(&self, job: &Job) -> Result<Vec<u8>, String> {
        let began = std::time::Instant::now();
        let body = json!({"text":job.text,"voice":job.settings.voice,"session":self.0.session,"revision":job.generation});
        let mut response = agent(Duration::from_secs(45)).post(format!("{}/v1/speak/stream",job.settings.endpoint)).header("Connection", "close").send_json(body).map_err(failure)?;
        let mut reader = response.body_mut().as_reader();
        let mut output = None;
        decode_stream(&mut reader, || {
            let state = self.0.state.lock().unwrap();
            !state.quit && state.generation == job.generation
        }, |data| {
            if output.is_none() {
                output = Some(self.begin_pcm(job)?);
                eprintln!("tts: first_pcm_ms={} click_to_pcm_ms={} (before PipeWire playout)", began.elapsed().as_millis(), job.queued.elapsed().as_millis());
            }
            output.as_ref().unwrap().try_send(data.to_vec()).map_err(|_| "Đã dừng audio hoặc bộ đệm phát đầy.".to_string())
        }).map(|(pcm,info)| {
            eprintln!("tts: stream_complete_ms={} backend={}",began.elapsed().as_millis(),info);
            pcm_wav(&pcm)
        })
    }

    fn worker(&self) {
        loop {
            let job = {
                let mut state = self.0.state.lock().unwrap();
                while state.pending.is_none() && !state.quit { state = self.0.wake.wait(state).unwrap(); }
                if state.quit { return; }
                let job = state.pending.take().unwrap();
                state.status = Status::new(Phase::Making, "Đang tạo giọng…");
                job
            };
            let began = std::time::Instant::now();
            eprintln!("tts: client_queue_ms={}",job.queued.elapsed().as_millis());
            let cached = {
                let state = self.0.state.lock().unwrap();
                state.cached.iter().chain(state.history_cache.iter()).find(|(base, voice, text, at, _)| {
                    base == &job.settings.endpoint && voice == &job.settings.voice && text == &job.text && at.elapsed() < Duration::from_secs(120)
                }).map(|(_,_,_,_,data)| data.clone())
            };
            let replay = cached.is_some();
            let mut streamed = false;
            let result = if let Some(data) = cached {
                if self.0.player[0] == "pw-play" {
                    streamed = true;
                    self.begin_pcm(&job).and_then(|output| {
                        for chunk in data[44..].chunks(65536) {
                            output.try_send(chunk.to_vec()).map_err(|_| "Bộ đệm phát đầy.".to_string())?;
                        }
                        Ok(data)
                    })
                } else { Ok(data) }
            } else if self.0.player[0] == "pw-play" && catalog(&job.settings.endpoint).is_ok_and(|list| list.streaming) {
                streamed = true;
                self.stream(&job)
            } else {
                synth(&job, Duration::from_secs(45), || {
                    let state = self.0.state.lock().unwrap();
                    !state.quit && state.generation == job.generation
                })
            };
            let mut state = self.0.state.lock().unwrap();
            if state.quit || state.generation != job.generation { continue; }
            let result = result.and_then(|data| {
                eprintln!("tts: request_ms={} client_replay={replay} bytes={}", began.elapsed().as_millis(), data.len());
                if let Some(previous) = state.cached.take() {
                    if previous.2 != job.text { state.history_cache.push_back(previous); }
                }
                while state.history_cache.len() >= 7 || state.history_cache.iter().map(|item| item.4.len()).sum::<usize>() + data.len() > 32 << 20 {
                    if state.history_cache.pop_front().is_none() { break; }
                }
                state.cached = Some((job.settings.endpoint.clone(), job.settings.voice.clone(), job.text.clone(), std::time::Instant::now(), data.clone()));
                if streamed { Ok(None) } else { play(&self.0.player, &data, job.settings.volume).map(Some) }
            });
            match result {
                Ok(player) => { state.player = player; state.status = Status::new(Phase::Playing, "Đang phát câu Nhật"); }
                Err(error) => { state.output = None; state.status = Status::new(Phase::Error, &error); }
            }
            if state.status.phase == Phase::Playing && state.settings.prepare && !state.suspended {
                state.preparing = state.next.clone().map(|text| Preparation { revision: state.generation, settings: state.settings.clone(), text: Some(text), warm: false });
            }
        }
    }
}

fn decode_stream(reader: &mut impl Read, current: impl Fn() -> bool, mut chunk: impl FnMut(&[u8]) -> Result<(),String>) -> Result<(Vec<u8>,Value),String> {
    let mut magic = [0u8;4];
    reader.read_exact(&mut magic).map_err(|_| "Luồng PCM bị gián đoạn.".to_string())?;
    if &magic != b"AVS1" { return Err("Backend trả sai định dạng PCM.".to_string()); }
    let mut pcm = Vec::new();
    loop {
        let mut header = [0u8;5];
        reader.read_exact(&mut header).map_err(|_| "Luồng PCM kết thúc trước khi xác nhận thành công.".to_string())?;
        let length = u32::from_le_bytes(header[1..].try_into().unwrap()) as usize;
        if length > 65536 || (header[0] == 1 && length % 2 != 0) { return Err("Khung PCM không hợp lệ.".to_string()); }
        let mut data = vec![0;length];
        reader.read_exact(&mut data).map_err(|_| "Không đọc đủ khung PCM.".to_string())?;
        if !current() { return Err("Đã bỏ câu cũ.".to_string()); }
        match header[0] {
            1 if !data.is_empty() => {
                if pcm.len()+data.len()+44 > AUDIO_LIMIT { return Err("Audio vượt giới hạn 16 MiB.".to_string()); }
                chunk(&data)?;
                pcm.extend_from_slice(&data);
            }
            2 if !pcm.is_empty() => {
                let info: Value = serde_json::from_slice(&data).map_err(|_| "Thiếu thông tin kết thúc PCM.".to_string())?;
                if !info.is_object() || info.get("error").is_some() { return Err("Backend không xác nhận thành công.".to_string()); }
                return Ok((pcm,info));
            }
            _ => return Err("Backend không hoàn thành tổng hợp. Thử nghe lại câu mới.".to_string()),
        }
    }
}

fn pcm_wav(pcm: &[u8]) -> Vec<u8> {
    let mut data = b"RIFF".to_vec(); data.extend((pcm.len() as u32+36).to_le_bytes()); data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes()); data.extend([1,0,1,0]); data.extend(48000u32.to_le_bytes());
    data.extend(96000u32.to_le_bytes()); data.extend([2,0,16,0]); data.extend(b"data");
    data.extend((pcm.len() as u32).to_le_bytes()); data.extend(pcm); data
}

fn synth(job: &Job, timeout: Duration, current: impl Fn() -> bool) -> Result<Vec<u8>, String> {
    let base = endpoint(&job.settings.endpoint)?;
    if !current() { return Err("Đã bỏ câu cũ.".to_string()); }
    let list = catalog(&base)?;
    if !list.voices.iter().any(|v| v.id == job.settings.voice) { return Err("Giọng đã chọn không còn được cài. Chọn lại trong Giọng đọc.".to_string()); }
    if !current() { return Err("Đã bỏ câu cũ.".to_string()); }
    let mut response = agent(timeout).post(format!("{base}/v1/speak"))
        .header("Connection", "close").send_json(json!({"text":job.text,"voice":job.settings.voice})).map_err(failure)?;
    let cache = response.headers().get("x-aivoice2-cache").and_then(|value| value.to_str().ok()).unwrap_or("unknown");
    eprintln!("tts: backend_cache={}", if cache == "hit" { "hit" } else if cache == "miss" { "miss" } else { "unknown" });
    let mut data = Vec::new();
    response.body_mut().as_reader().take(AUDIO_LIMIT as u64 + 1).read_to_end(&mut data).map_err(|_| "Không đọc được audio từ A.I.VOICE2.".to_string())?;
    wav(&data)?;
    Ok(data)
}

/// A seekable anonymous file disappears on exit, including SIGKILL.
fn play(player: &[String], data: &[u8], volume: f64) -> Result<Playback, String> {
    let fd = unsafe { libc::memfd_create(c"vntext-tts".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 { return Err("Không tạo được bộ đệm âm thanh.".to_string()); }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(data).map_err(|_| "Không ghi được bộ đệm âm thanh.".to_string())?;
    let parent = std::process::id();
    let mut command = Command::new(&player[0]);
    command.args(&player[1..]);
    if player[0] == "pw-play" { command.arg("--volume").arg(volume.to_string()); }
    command.arg(format!("/proc/{parent}/fd/{}", file.as_raw_fd()))
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 { return Err(std::io::Error::last_os_error()); }
            if libc::getppid() as u32 != parent { return Err(std::io::Error::other("reader exited")); }
            Ok(())
        });
    }
    let child = command.spawn().map_err(|_| "Không mở được pw-play. Cài gói pipewire-audio rồi thử lại.".to_string())?;
    Ok(Playback { child, _file: file })
}

pub fn cleanup() {
    if let Some(active) = ACTIVE.get().and_then(|active| active.lock().unwrap().upgrade()) { Control(active).shutdown(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn audio(marker: i16) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(38u32.to_le_bytes()); bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes()); bytes.extend([1,0,1,0]);
        bytes.extend(48000u32.to_le_bytes()); bytes.extend(96000u32.to_le_bytes());
        bytes.extend([2,0,16,0]); bytes.extend(b"data"); bytes.extend(2u32.to_le_bytes());
        bytes.extend(marker.to_le_bytes()); bytes
    }
    fn wait(mut condition: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(6);
        while !condition() {
            assert!(Instant::now() < until, "Asynchronous operation did not complete");
            thread::sleep(Duration::from_millis(10));
        }
    }
    struct Fake {
        base: String,
        requests: Arc<Mutex<Vec<Value>>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        quit: Arc<AtomicBool>,
    }
    impl Fake {
        fn new() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let requests: Arc<Mutex<Vec<Value>>> = Arc::default();
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            let quit = Arc::new(AtomicBool::new(false));
            let (got, held, end) = (requests.clone(), gate.clone(), quit.clone());
            thread::spawn(move || {
                while !end.load(Ordering::Relaxed) {
                    let Ok((mut socket, _)) = listener.accept() else { thread::sleep(Duration::from_millis(5)); continue };
                    let (got, held) = (got.clone(), held.clone());
                    thread::spawn(move || {
                        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                        let mut reader = BufReader::new(socket.try_clone().unwrap());
                        let mut request = String::new(); reader.read_line(&mut request).unwrap();
                        let mut length = 0;
                        loop {
                            let mut line = String::new(); reader.read_line(&mut line).unwrap();
                            if line.trim().is_empty() { break; }
                            if let Some((name,value)) = line.split_once(':') {
                                if name.eq_ignore_ascii_case("content-length") { length = value.trim().parse::<usize>().unwrap(); }
                            }
                        }
                        let body = if request.starts_with("GET /healthz ") { br#"{"ready":true}"#.to_vec() }
                        else if request.starts_with("GET /v1/voices ") {
                            br#"{"voices":[{"id":"v","name":"KotonohaAoi_ns_48","language":"standard"},{"id":"other","name":"new_bank","language":"standard"}],"default_voice":"v"}"#.to_vec()
                        } else {
                            assert!(length < 65536);
                            let mut data = vec![0;length]; reader.read_exact(&mut data).unwrap();
                            let value: Value = serde_json::from_slice(&data).unwrap();
                            got.lock().unwrap().push(value.clone());
                            if value["text"] == "first" {
                                let (lock, changed) = &*held;
                                let _guard = changed.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(5), |ready| !*ready).unwrap();
                            }
                            if value["text"] == "timeout" { thread::sleep(Duration::from_millis(150)); }
                            if value["text"] == "invalid" { b"not a WAV".to_vec() }
                            else { audio(if value["text"] == "first" { 1 } else { 2 }) }
                        };
                        let _ = write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                        let _ = socket.write_all(&body);
                    });
                }
            });
            Self { base, requests, gate, quit }
        }
        fn release(&self) { *self.gate.0.lock().unwrap() = true; self.gate.1.notify_all(); }
    }
    impl Drop for Fake {
        fn drop(&mut self) { self.release(); self.quit.store(true, Ordering::Relaxed); }
    }
    struct Fixture { control: Control, server: Fake, root: std::path::PathBuf }
    impl Fixture {
        fn new() -> Self {
            let server = Fake::new();
            let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let root = std::env::temp_dir().join(format!("vntext-tts-{}-{unique}",std::process::id()));
            std::fs::create_dir(&root).unwrap();
            let program = "import os,sys,time,struct; d=open(sys.argv[2],'rb').read(); f=open(sys.argv[1],'a'); f.write(str(os.getpid())+':'+str(struct.unpack_from('<h',d,44)[0])+'\\n'); f.close(); time.sleep(30)";
            let player = vec!["python3".to_string(), "-c".to_string(), program.to_string(), root.join("played").to_string_lossy().into_owned()];
            let control = Control::new(Settings { endpoint: server.base.clone(), voice: "v".to_string(), automatic: true, ..Settings::default() }, "test".to_string(), player);
            Self { control, server, root }
        }
        fn played(&self) -> String { std::fs::read_to_string(self.root.join("played")).unwrap_or_default() }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { self.control.shutdown(); self.server.release(); let _ = std::fs::remove_dir_all(&self.root); }
    }
    fn event(text: &str) -> String { json!({"type":"message","source":"profile","name":"例","lines":[text]}).to_string() }

    #[test]
    fn latest_sentence_wins_and_translation_never_repeats_it() {
        let fixture = Fixture::new(); let control = &fixture.control;
        control.event(&event("first"));
        wait(|| fixture.server.requests.lock().unwrap().len() == 1);
        control.event(&event("skip"));
        control.event(&event("こんにちは。"));
        control.event(&event("こんにちは。"));
        control.event(r#"{"type":"translation","seq":3,"tr":"Xin chào"}"#);
        fixture.server.release();
        wait(|| !fixture.played().is_empty());
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(),2);
        assert_eq!(requests[1],json!({"text":"こんにちは。","voice":"v"}));
        assert_eq!(fixture.played().lines().count(),1);
        assert!(fixture.played().trim().ends_with(":2"));
        control.stop();
        // Replay uses the in-memory WAV and can still be explicitly requested.
        control.speak("こんにちは。");
        wait(|| fixture.played().lines().count() == 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(),2);
    }

    #[test]
    fn stop_discards_late_audio_and_only_kills_its_own_player() {
        let fixture = Fixture::new(); let control = &fixture.control;
        control.speak("first");
        wait(|| fixture.server.requests.lock().unwrap().len() == 1);
        control.stop(); fixture.server.release();
        thread::sleep(Duration::from_millis(150));
        assert!(fixture.played().is_empty());
        control.speak("next"); wait(|| !fixture.played().is_empty());
        let pid: u32 = fixture.played().split(':').next().unwrap().parse().unwrap();
        assert!(std::path::Path::new(&format!("/proc/{pid}")).exists());
        control.stop();
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        assert_eq!(control.status().phase,Phase::Idle);
    }

    #[test]
    fn malformed_audio_missing_voice_and_suspension_do_not_play() {
        let fixture = Fixture::new(); let control = &fixture.control;
        control.speak("invalid"); wait(|| control.status().phase == Phase::Error);
        assert!(fixture.played().is_empty());
        control.suspend(true); control.event(&event("ignored"));
        thread::sleep(Duration::from_millis(70));
        assert_eq!(fixture.server.requests.lock().unwrap().len(),1);
        control.suspend(false);
        control.0.state.lock().unwrap().settings.voice = "removed".to_string();
        control.speak("missing"); wait(|| control.status().phase == Phase::Error);
        assert_eq!(fixture.server.requests.lock().unwrap().len(),1);
        assert!(fixture.played().is_empty());
        control.0.state.lock().unwrap().settings.voice = "v".to_string();
        control.0.state.lock().unwrap().settings.endpoint = "http://127.0.0.1:1".to_string();
        control.speak("offline"); wait(|| control.status().phase == Phase::Error);
        assert!(fixture.played().is_empty());
    }

    #[test]
    fn changing_voice_and_disabling_auto_discard_a_running_response() {
        let fixture = Fixture::new(); let control = &fixture.control;
        control.event(&event("first"));
        wait(|| fixture.server.requests.lock().unwrap().len() == 1);
        let mut settings = control.settings();
        settings.voice = "other".to_string(); settings.automatic = false;
        control.change(settings).unwrap();
        fixture.server.release();
        control.event(&event("not automatic"));
        thread::sleep(Duration::from_millis(100));
        assert!(fixture.played().is_empty());
        control.speak("manual"); wait(|| !fixture.played().is_empty());
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["voice"], "other");
    }

    #[test]
    fn timeout_and_missing_player_are_errors_without_playback() {
        let fixture = Fixture::new();
        let job = Job { generation: 1, text: "timeout".to_string(), settings: fixture.control.settings(), queued: Instant::now() };
        assert!(synth(&job, Duration::from_millis(30), || true).is_err());
        fixture.control.0.state.lock().unwrap().cached = None;
        let control = Control::new(fixture.control.settings(), "test".to_string(), vec!["/missing-vntext-player".to_string()]);
        control.speak("next"); wait(|| control.status().phase == Phase::Error);
        assert!(fixture.played().is_empty());
        control.shutdown();
    }

    #[test]
    #[ignore]
    fn live_sample() {
        assert_eq!(std::env::var("VNTEXT_TTS_LIVE").as_deref(), Ok("local_fixture"));
        let catalog = catalog(ENDPOINT).unwrap();
        let voice = catalog.voices.iter().find(|voice| voice.id.starts_with("KotonohaAoi_ns_48/"))
            .or_else(|| catalog.voices.iter().find(|voice| voice.id == catalog.default)).unwrap();
        let control = Control::new(Settings { voice: voice.id.clone(), ..Settings::default() }, "sample".to_string(), vec!["pw-play".to_string()]);
        let began = Instant::now();
        control.sample();
        loop {
            control.poll();
            let status = control.status();
            if !status.busy() {
                assert_eq!(status.phase, Phase::Idle, "{}", status.text);
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(55));
            thread::sleep(Duration::from_millis(20));
        }
        eprintln!("Live local sample completed: {} ms, voice {}", began.elapsed().as_millis(), voice.id);
        let first_player = control.0.state.lock().unwrap().output.as_ref().map(audio::Player::id);
        control.sample();
        let replay_began = Instant::now();
        loop {
            control.poll();
            let status = control.status();
            if !status.busy() {
                assert_eq!(status.phase, Phase::Idle, "{}", status.text);
                break;
            }
            assert!(replay_began.elapsed() < Duration::from_secs(20));
            thread::sleep(Duration::from_millis(20));
        }
        if let Some(first) = first_player {
            assert_eq!(control.0.state.lock().unwrap().output.as_ref().unwrap().id(),first,"Idle PipeWire player should be reused");
        }
        control.shutdown();
    }

    #[test]
    fn configuration_identity_and_audio_bounds() {
        let defaults = Settings::decode(&Value::Null,"new");
        assert!(!defaults.automatic && defaults.voice.is_empty());
        let saved = json!({"endpoint":ENDPOINT,"games":{"Ryujinx/0100F6A00A684000":{"voice":"v","automatic":true,"volume":0.5}}});
        assert!(Settings::decode(&saved,"Ryujinx/0100F6A00A684000").automatic);
        assert!(!Settings::decode(&saved,"Ryujinx/0100000000000000").automatic);
        assert_eq!(title_id("Ryujinx - (0100f6a00a684000)"),Some("0100F6A00A684000".to_string()));
        for address in ["https://127.0.0.1:17862","http://example.com:1234","http://127.0.0.1:1234/path","http://localhost:0"] { assert!(endpoint(address).is_err()); }
        assert!(wav(&audio(1)).is_ok());
        assert!(wav(&audio(1)[..45]).is_err());
        let mut invalid = audio(1); invalid[34] = 8;
        assert!(wav(&invalid).is_err());
        assert!(sentence("").is_err());
        assert!(sentence(&"あ".repeat(TEXT_LIMIT+1)).is_err());
    }

    #[test]
    fn streaming_requires_a_successful_end_and_rejects_stale_or_invalid_frames() {
        let frame = |kind: u8, bytes: &[u8]| {
            let mut result = vec![kind]; result.extend((bytes.len() as u32).to_le_bytes()); result.extend(bytes); result
        };
        let mut source = b"AVS1".to_vec();
        source.extend(frame(1,&[1,0,2,0]));
        let truncated = source.clone();
        source.extend(frame(2,br#"{"cache":false,"timing":{}}"#));
        let mut chunks = Vec::new();
        let (pcm,_) = decode_stream(&mut &source[..],||true,|data| { chunks.push(data.to_vec()); Ok(()) }).unwrap();
        assert_eq!(pcm,[1,0,2,0]); assert_eq!(chunks.len(),1);
        assert!(wav(&pcm_wav(&pcm)).is_ok());
        assert!(decode_stream(&mut &truncated[..],||true,|_|Ok(())).is_err());
        assert!(decode_stream(&mut &source[..],||false,|_|panic!("Stale audio must not be played")).is_err());
        for (kind,payload) in [(1,&[1u8][..]),(3,&b"error"[..]),(2,&b"{}"[..])] {
            let data = [b"AVS1".to_vec(),frame(kind,payload)].concat();
            assert!(decode_stream(&mut &data[..],||true,|_|Ok(())).is_err());
        }
        let oversized = [b"AVS1".to_vec(),vec![1],65538u32.to_le_bytes().to_vec()].concat();
        assert!(decode_stream(&mut &oversized[..],||true,|_|panic!("Oversized frame")).is_err());
    }

    #[test]
    fn preparation_has_one_latest_slot_and_never_reads_translation() {
        let fixture = Fixture::new();
        let control = &fixture.control;
        control.0.state.lock().unwrap().settings.automatic = false;
        control.event(&json!({"type":"message","source":"profile","name":"例","lines":["current"],"next_lines":["next"]}).to_string());
        {
            let state = control.0.state.lock().unwrap();
            assert_eq!(state.next.as_deref(),Some("next"));
            assert!(state.pending.is_none());
        }
        control.suspend(true);
        control.event(&event("paused"));
        assert!(control.0.state.lock().unwrap().pending.is_none());
        control.event(r#"{"type":"translation","tr":"wrong language"}"#);
        assert_eq!(control.0.state.lock().unwrap().previous.as_ref().unwrap().1,"paused");
    }
}
