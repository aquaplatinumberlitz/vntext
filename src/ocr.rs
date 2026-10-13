//! For games without a profile: a picture of the game's text box goes to
//! the Gemini API whenever it has changed, and the text read from it comes
//! back, with its translation if asked for.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::screen::{Picture, Region, Screen};
use crate::{die, overlay, translate, web, Opts};

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

fn config_dir(variable: &str, under_home: &str) -> PathBuf {
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

fn saved_region(game: &str) -> Option<Region> {
    let file = std::fs::read_to_string(regions_file()).ok()?;
    file.lines().rev().find_map(|line| parse_region(line, game))
}

fn save_region(game: &str, region: Region) {
    let file = regions_file();
    let _ = std::fs::create_dir_all(file.parent().unwrap());
    let mut lines: Vec<String> = std::fs::read_to_string(&file).unwrap_or_default().lines().map(str::to_string).collect();
    lines.retain(|line| parse_region(line, game).is_none());
    lines.push(format!("{game}\t{:.4}\t{:.4}\t{:.4}\t{:.4}", region.left, region.top, region.right, region.bottom));
    if let Err(e) = std::fs::write(&file, lines.join("\n") + "\n") {
        eprintln!("cannot save the text box to {}: {e}", file.display());
    }
}

/// Asks where the text box is: the pointer is put on two of its corners.
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

pub fn run(pid: u32, o: &Opts) {
    let screen = Screen::open().unwrap_or_else(|e| die(&e));
    let started = Instant::now();
    let window = loop {
        match screen.window_of(pid) {
            Some(window) => break window,
            None if started.elapsed() < WINDOW_WITHIN => thread::sleep(Duration::from_millis(500)),
            None => die("the process has no X11 window; a native Wayland window cannot be pictured"),
        }
    };
    let game = std::fs::read(format!("/proc/{pid}/comm")).unwrap_or_default();
    let game = String::from_utf8_lossy(&game).trim().replace('\t', " ");
    let region = match saved_region(&game).filter(|_| !o.pick) {
        Some(region) => region,
        None => {
            let region = pick(&screen, window);
            save_region(&game, region);
            region
        }
    };
    let mut translator = translate::Translator::new(&o.to, o.model.clone()).unwrap_or_else(|e| die(&e));
    eprintln!("reading the text box of {game} with {}", translator.client.model);
    let hub = web::Hub::default();
    if o.web {
        match hub.serve(o.port) {
            Ok(()) => eprintln!("page for Yomitan: http://127.0.0.1:{}", o.port),
            Err(e) => die(&format!("cannot serve on port {}: {e}", o.port)),
        }
    }
    if o.overlay {
        overlay::start(hub.subscribe(), pid);
    }

    let mut before: Option<Picture> = None;
    let mut rests = 0u32;
    let mut read: VecDeque<Picture> = VecDeque::new();
    let mut context: VecDeque<String> = VecDeque::new();
    let mut asked = Instant::now() - BETWEEN_REQUESTS;
    let mut seq = 0u64;
    loop {
        thread::sleep(TICK);
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            die("target process is gone");
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
                eprintln!("\x1b[33mreading failed: {e}\x1b[0m");
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
        if shown.text.is_empty() || context.back() == Some(&shown.text) {
            continue;
        }

        seq += 1;
        let lines: Vec<&str> = shown.text.lines().collect();
        shown.speaker.iter().map(String::as_str).chain(lines.iter().copied()).for_each(|l| println!("{l}"));
        if let Some(translation) = &shown.translation {
            println!("\x1b[36m{translation}\x1b[0m");
        }
        let _ = std::io::stdout().flush();
        if o.clip {
            let _ = Command::new("wl-copy").arg("--").arg(&shown.text).status();
        }
        hub.publish(
            serde_json::json!({"type": "message", "seq": seq, "name": shown.speaker, "lines": lines, "translating": shown.translation.is_some()})
                .to_string(),
        );
        if let Some(translation) = shown.translation {
            hub.publish(serde_json::json!({"type": "translation", "seq": seq, "tr": translation}).to_string());
            translator.store(&[translate::Line { speaker: shown.speaker, text: shown.text.clone() }], vec![translation]);
        }
        context.push_back(shown.text);
        if context.len() > CONTEXT {
            context.pop_front();
        }
    }
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
}
