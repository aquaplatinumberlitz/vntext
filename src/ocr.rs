//! For games without a profile: a picture of the game's text box goes to
//! the Gemini API whenever it has changed, and the text read from it comes
//! back, with its translation if asked for.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::screen::{Picture, Region, Screen};
use crate::{die, mem, overlay, translate, web, Opts};

const TICK: Duration = Duration::from_millis(150);
/// Two pictures in a row that differ in less than this share of their
/// pixels are the same text box at rest; a blinking cursor stays below it.
const AT_REST: f64 = 0.005;
/// Pictures in a row at rest before one is read: text that appears letter
/// by letter is still coming.
const RESTS: u32 = 2;
/// A picture is new if it differs from each one already read in at least
/// this share of its pixels: about two characters in a text box.
const NEW: f64 = 0.001;
/// Pictures already read that are remembered, so that a text box that
/// cycles through a few looks is read once.
const REMEMBERED: usize = 8;
/// Text read before, sent along for the model's reference.
const CONTEXT: usize = 6;
const BETWEEN_REQUESTS: Duration = Duration::from_secs(1);
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// How long a game is given to open its window.
const WINDOW_WITHIN: Duration = Duration::from_secs(20);

/// What a reader needs of the command line.
#[derive(Clone)]
pub struct Settings {
    translate: bool,
    to: String,
    model: Option<String>,
    clip: bool,
    debug: bool,
}

impl Settings {
    pub fn of(o: &Opts) -> Settings {
        Settings { translate: o.translate, to: o.to.clone(), model: o.model.clone(), clip: o.clip, debug: o.debug }
    }
}

/// Whether the text comes from pictures, and from which reader: starting a
/// new one retires the one before.
#[derive(Default)]
pub struct Control {
    on: AtomicBool,
    reader: AtomicU64,
}

impl Control {
    pub fn on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.on.store(false, Ordering::Relaxed);
    }

    fn begin(&self) -> u64 {
        self.on.store(true, Ordering::Relaxed);
        self.reader.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn wants(&self, reader: u64) -> bool {
        self.on() && self.reader.load(Ordering::Relaxed) == reader
    }
}

pub fn config_dir(variable: &str, under_home: &str) -> PathBuf {
    let dir = std::env::var_os(variable).map(PathBuf::from);
    dir.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(under_home))).unwrap_or_else(|| PathBuf::from(".")).join("vntext")
}

/// The file of text boxes: `game, left, top, right, bottom` on each line,
/// the numbers in fractions of the window.
fn regions_file() -> PathBuf {
    config_dir("XDG_CONFIG_HOME", ".config").join("regions.tsv")
}

fn parse_region(line: &str, game: &str) -> Option<Region> {
    let mut columns = line.split('\t');
    if columns.next()? != game {
        return None;
    }
    let mut number = || columns.next()?.parse::<f64>().ok();
    Some(Region { left: number()?, top: number()?, right: number()?, bottom: number()? })
}

pub fn saved_region(game: &str) -> Option<Region> {
    let file = std::fs::read_to_string(regions_file()).ok()?;
    file.lines().rev().find_map(|line| parse_region(line, game))
}

pub fn save_region(game: &str, region: Region) {
    let file = regions_file();
    let _ = std::fs::create_dir_all(file.parent().unwrap());
    let mut lines: Vec<String> = std::fs::read_to_string(&file).unwrap_or_default().lines().map(str::to_string).collect();
    lines.retain(|line| parse_region(line, game).is_none());
    lines.push(format!("{game}\t{:.4}\t{:.4}\t{:.4}\t{:.4}", region.left, region.top, region.right, region.bottom));
    if let Err(e) = std::fs::write(&file, lines.join("\n") + "\n") {
        eprintln!("cannot save the text box to {}: {e}", file.display());
    }
}

/// Asks in the terminal where the text box is: the pointer is put on two of
/// its corners.
fn pick(screen: &Screen, window: u32) -> Region {
    let corner = |which: &str| loop {
        eprintln!("put the pointer on the {which} corner of the game's text box, then press Enter here");
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            die("no text box picked");
        }
        match screen.pointer(window) {
            Some(at) => break at,
            None => eprintln!("the pointer is not on the game's window"),
        }
    };
    let (a, b) = (corner("top left"), corner("bottom right"));
    let region = Region { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0), bottom: a.1.max(b.1) };
    if region.right - region.left < 0.02 || region.bottom - region.top < 0.02 {
        die("the two corners are too close together");
    }
    region
}

/// Tells the reader something went wrong, in the terminal and on the strip.
fn notice(hub: &web::Hub, text: &str) {
    eprintln!("\x1b[33m{text}\x1b[0m");
    hub.publish(serde_json::json!({"type": "notice", "text": text}).to_string());
}

/// Starts reading the game's text box from pictures, in place of whoever
/// read it before. It goes on until `control` is stopped or given to another.
pub fn spawn(pid: u32, settings: Settings, hub: web::Hub, control: Arc<Control>) {
    let reader = control.begin();
    thread::spawn(move || {
        if let Err(e) = read(pid, &settings, &hub, &control, reader) {
            notice(&hub, &format!("OCR: {e}"));
            if control.wants(reader) {
                control.stop();
            }
        }
    });
}

fn read(pid: u32, o: &Settings, hub: &web::Hub, control: &Control, reader: u64) -> Result<(), String> {
    let screen = Screen::open()?;
    let window = screen.window_of(pid).ok_or("the game has no X11 window")?;
    let game = mem::name(pid);
    let region = saved_region(&game).ok_or("no text box picked yet")?;
    let mut translator = {
        // a model named for another service is not one of Gemini's
        let model = o.model.clone().filter(|_| translate::chosen() == translate::Service::Gemini);
        translate::Translator::new(&o.to, model, Some(translate::Service::Gemini))?
    };
    eprintln!("reading the text box of {game} with {}", translator.client.model);
    if o.translate {
        translate::set_at_work(translator.client.service);
    }

    let mut before: Option<Picture> = None;
    let mut rests = 0u32;
    let mut read: VecDeque<Picture> = VecDeque::new();
    let mut context: VecDeque<String> = VecDeque::new();
    let mut asked = Instant::now() - BETWEEN_REQUESTS;
    loop {
        thread::sleep(TICK);
        if !control.wants(reader) {
            return Ok(());
        }
        let Some(now) = screen.grab(window, region) else { continue };
        rests = if before.as_ref().is_some_and(|b| b.difference(&now) < AT_REST) { rests + 1 } else { 0 };
        before = Some(now.clone());
        if rests < RESTS || asked.elapsed() < BETWEEN_REQUESTS || read.iter().any(|p| p.difference(&now) < NEW) {
            continue;
        }

        let png = now.png();
        if o.debug {
            let file = config_dir("XDG_CACHE_HOME", ".cache").join("ocr.png");
            let _ = std::fs::write(&file, &png);
            eprintln!("\x1b[2m  sending {}x{}, {} bytes ({})\x1b[0m", now.width, now.height, png.len(), file.display());
        }
        asked = Instant::now();
        let earlier: Vec<String> = context.iter().cloned().collect();
        let shown = match translator.client.read(&png, &earlier, o.translate) {
            Ok(shown) => shown,
            Err(e) => {
                notice(hub, &format!("OCR: {e}"));
                thread::sleep(RETRY_AFTER);
                continue;
            }
        };
        if o.debug {
            eprintln!("\x1b[2m  answered in {} ms\x1b[0m", asked.elapsed().as_millis());
        }
        read.push_back(now);
        if read.len() > REMEMBERED {
            read.pop_front();
        }
        // the same text again: the box changed its look, not what it says
        if shown.text.is_empty() || context.back() == Some(&shown.text) || !control.wants(reader) {
            continue;
        }

        let seq = hub.next_seq();
        let lines: Vec<&str> = shown.text.lines().collect();
        shown.speaker.iter().for_each(|name| println!("\x1b[1;33m{name}\x1b[0m"));
        lines.iter().for_each(|l| println!("{l}"));
        if let Some(translation) = &shown.translation {
            println!("\x1b[36m{translation}\x1b[0m");
        }
        let _ = std::io::stdout().flush();
        if o.clip {
            let _ = Command::new("wl-copy").arg("--").arg(&shown.text).status();
        }
        hub.publish(
            serde_json::json!({"type": "message", "seq": seq, "source": "ocr", "name": shown.speaker, "lines": lines, "translating": shown.translation.is_some()})
                .to_string(),
        );
        if let Some(translation) = shown.translation {
            hub.publish(serde_json::json!({"type": "translation", "seq": seq, "tr": translation}).to_string());
            translator.store(&[translate::Line { speaker: shown.speaker, text: shown.text.clone() }], vec![translation], translate::Via::Model);
        }
        context.push_back(shown.text);
        if context.len() > CONTEXT {
            context.pop_front();
        }
    }
}

/// The game is read from pictures alone. With `reading` it starts at once;
/// without, it waits for the reader to ask for it on the overlay.
pub fn run(pid: u32, o: &Opts, reading: bool) {
    let hub = web::Hub::default();
    if o.web {
        match hub.serve(o.port) {
            Ok(()) => eprintln!("page for Yomitan: http://127.0.0.1:{}", o.port),
            Err(e) => die(&format!("cannot serve on port {}: {e}", o.port)),
        }
    }
    let control = Arc::new(Control::default());
    if o.overlay {
        // the overlay asks for the text box itself
        overlay::start(hub.clone(), pid, control.clone(), Settings::of(o), reading);
        if !reading {
            notice(&hub, "Chưa nhận ra engine của game này. Bấm OCR để đọc khung thoại bằng ảnh.");
        }
    } else {
        let screen = Screen::open().unwrap_or_else(|e| die(&e));
        let started = Instant::now();
        let window = loop {
            match screen.window_of(pid) {
                Some(window) => break window,
                None if started.elapsed() < WINDOW_WITHIN => thread::sleep(Duration::from_millis(500)),
                None => die("the process has no X11 window; a native Wayland window cannot be pictured"),
            }
        };
        let game = mem::name(pid);
        if o.pick || saved_region(&game).is_none() {
            save_region(&game, pick(&screen, window));
        }
        spawn(pid, Settings::of(o), hub.clone(), control.clone());
    }
    while std::path::Path::new(&format!("/proc/{pid}")).exists() {
        thread::sleep(Duration::from_millis(500));
        if !o.overlay && !control.on() {
            // without an overlay nothing can start the reader again
            std::process::exit(1);
        }
    }
    die("target process is gone");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_lines() {
        let region = Region { left: 0.1, top: 0.7, right: 0.9, bottom: 0.95 };
        assert_eq!(parse_region("game.exe\t0.1000\t0.7000\t0.9000\t0.9500", "game.exe"), Some(region));
        assert_eq!(parse_region("other.exe\t0.1\t0.7\t0.9\t0.95", "game.exe"), None);
        assert_eq!(parse_region("game.exe\t0.1\t0.7", "game.exe"), None);
    }

    #[test]
    fn a_new_reader_retires_the_old() {
        let control = Control::default();
        let first = control.begin();
        assert!(control.wants(first));
        let second = control.begin();
        assert!(!control.wants(first) && control.wants(second));
        control.stop();
        assert!(!control.wants(second));
    }
}
