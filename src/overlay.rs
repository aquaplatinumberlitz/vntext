//! One floating panel holds the message and its controls. It can fold into
//! a small restore button without losing its place by the game's window.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{cairo, gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::{json, Value};

use crate::dict::import;
use crate::explain::Action;
use crate::dict::lookup::{self, Source, Trouble};
use crate::dict::store::{Info, Store};
use crate::screen::{self, Region, Screen, WindowId};
use crate::translate::{self, Service};
use crate::{mem, ocr, tts, web, yomitan};

const TICK: Duration = Duration::from_millis(100);
/// Between the game's window and what floats by it.
const GAP: i32 = 6;
/// Successful actions should not leave a permanent banner over the game.
const TOAST: Duration = Duration::from_secs(5);
/// Shown while a message waits for its translation.
const PENDING: &str = "…";
/// Messages kept to step back through.
const KEPT: usize = 200;
const FONT: i32 = 22;
const FONTS: std::ops::RangeInclusive<i32> = 10..=72;
/// How opaque the strip's background may be set, and how it starts.
const OPACITIES: std::ops::RangeInclusive<f64> = 0.2..=1.0;
const OPACITY: f64 = 0.72;
/// Colours of the text to choose from, and the one it starts in.
const COLOURS: [&str; 6] = ["#ffffff", "#ffe08a", "#9ecbff", "#a6e3a1", "#f5a9c8", "#ffb27a"];
const COLOUR: &str = COLOURS[0];
/// Where the file of looks says that the first-time help has been read; no
/// game is named that.
const INTRO_KEY: &str = "@intro";
const NARROWEST: i32 = 200;
/// A manual viewport must leave room for the header and a line of text.
const SHORTEST: i32 = 140;
const INSET: i32 = 16;
const PADDING: i32 = 42;
/// How much of the text from the pointer on is looked up: the longest word
/// it starts with is found.
const LOOKED_UP: usize = 20;
/// How wide the meaning of a word may be shown.
const MEANING: i32 = 360;
/// How tall the meaning may be before it scrolls.
const TALLEST: i32 = 330;
/// Ticks that the meaning stays after the pointer has left its word.
const LINGERS: u32 = 3;
/// A text box smaller than this share of the game's window was not meant.
const SMALLEST_BOX: f64 = 0.02;
const STYLE: &str = "
window.vntext, window.vntext-pick, window.vntext-ask { background: none; }
.vntext scrollbar slider, .vntext-ask scrollbar slider { min-width: 6px; min-height: 6px; margin: 0; border: none; border-radius: 3px; background: rgba(255, 255, 255, 0.35); }
.vntext scrollbar, .vntext-ask scrollbar { background: none; border: none; }
.vntext .strip { border-radius: 10px; padding: 8px 14px; }
.vntext .strip label { color: white; text-shadow: 0 0 3px black, 0 0 3px black; }
.vntext .strip label.speaker { color: #e0a458; font-weight: bold; }
.vntext .strip label.translation { color: #9ecbff; }
.vntext .strip label.alone { color: white; }
.vntext .strip label.pending { color: rgba(255, 255, 255, 0.5); }
.vntext .strip label.source { font-size: 10px; color: rgba(255, 255, 255, 0.6); padding: 0 0 0 10px; text-shadow: none; }
.vntext .strip label.source.pictures { color: #e0a458; }
.vntext .strip button.service { min-height: 0; min-width: 0; padding: 1px 6px; margin-left: 10px; border: none; box-shadow: none;
    border-radius: 4px; background: rgba(255, 255, 255, 0.16); }
.vntext .strip button.service:hover { background: rgba(255, 255, 255, 0.35); }
.vntext .strip button.service label { font-size: 10px; color: white; text-shadow: none; }
.vntext .strip button.hint { min-height: 0; min-width: 0; padding: 2px 8px; border: none; box-shadow: none; border-radius: 4px; background: #e0a458; }
.vntext .strip button.hint label { font-size: 11px; color: black; text-shadow: none; }
.vntext .strip label.hinted { font-size: 12px; color: #e0a458; }
.vntext .strip label.intro { color: white; }
.vntext .strip button.understood { min-height: 0; padding: 2px 10px; border: none; box-shadow: none; border-radius: 4px; background: #e0a458; }
.vntext .strip button.understood label { font-size: 12px; color: black; text-shadow: none; }
.vntext .strip label.grip { color: #e0a458; padding: 0 8px; }
.vntext .meaning { background: rgba(30, 31, 40, 0.97); border: 1px solid #4a4c5c; border-radius: 8px; padding: 8px 10px; }
.vntext .meaning scrolledwindow, .vntext .meaning viewport { background: none; }
.vntext .meaning label { color: #d4d4d4; font-size: 14px; }
.vntext .meaning label.reading { color: #9a9aa6; font-size: 13px; margin-bottom: -4px; }
.vntext .meaning label.term { color: #e8e8e8; font-size: 26px; }
.vntext .meaning label.gloss { font-size: 14px; }
.vntext .meaning label.pill { color: white; font-size: 12px; font-weight: bold; padding: 1px 6px; border-radius: 4px; background: #8a8a91; }
.vntext .meaning label.pill.dictionary { background: #aa66cc; }
.vntext .meaning label.pill.popular { background: #0275d8; }
.vntext .meaning label.pill.frequent { background: #4a8fa3; }
.vntext .meaning label.pill.archaism { background: #d9534f; }
.vntext .meaning label.pill.name { background: #b6327a; }
.vntext .meaning label.pill.expression { background: #b6853f; }
.vntext .meaning label.pill.partOfSpeech { background: #565656; }
.vntext .meaning .counted { border: 1px solid #4a9a4a; border-radius: 4px; }
.vntext .meaning label.pill.counter { background: #4a9a4a; border-radius: 3px 0 0 3px; }
.vntext .meaning label.pill.numbers { background: none; font-weight: normal; font-size: 13px; color: #d4d4d4; }
.vntext .meaning .example { background: rgba(255, 255, 255, 0.06); border-left: 3px solid #5a5c6c; border-radius: 4px; padding: 4px 8px; margin: 2px 0 2px 18px; }
.vntext .meaning label.sentence { font-size: 16px; color: #e8e8e8; }
.vntext .meaning label.meant { font-size: 12px; color: #b0b0b8; }
.vntext .meaning flowboxchild { padding: 0; min-width: 0; min-height: 0; }
.vntext .meaning separator { background: #4a4c5c; margin: 6px 0; }
.vntext-ask .box { background: #1e1e1e; border-radius: 10px; padding: 12px 14px; }
.vntext-ask .box label { color: #bbb; font-size: 12px; }
.vntext-ask .box button label { color: black; font-size: 13px; }
.vntext-ask .box button.tab label { color: white; }
.vntext-ask .box button.tab.on label { color: black; }
.vntext-ask list, .vntext-ask row { background: #2c2c2c; }
.vntext-ask scrolledwindow, .vntext-ask viewport { background: none; }
.vntext-ask button.swatch { min-width: 22px; min-height: 22px; padding: 0; border: 2px solid #1e1e1e; border-radius: 11px; }
.vntext-ask button.swatch.on { border-color: white; }
.vntext-ask button.icon { background: #333; padding: 3px 6px; }
.vntext-ask button.tab { background: #333; padding: 3px 9px; }
.vntext-ask button.tab label { color: white; }
.vntext-ask button.tab.on { background: #e0a458; }
.vntext-ask button.tab.on label { color: black; }
.vntext-ask entry { background: #2c2c2c; color: white; caret-color: white; border: 1px solid #555; box-shadow: none; min-height: 30px; }
.vntext-ask entry image { color: #bbb; }
.vntext-ask button { background: #e0a458; border: none; box-shadow: none; padding: 4px 16px; }
.vntext-ask button label { color: black; font-size: 13px; }
.vntext-ask button.icon image { color: white; }
.vntext-ask .box dropdown button { background: #2c2c2c; border: 1px solid #555; padding: 2px 8px; }
.vntext-ask .box dropdown button label, .vntext-ask .box dropdown button arrow { color: white; }
.vntext-ask popover contents, .vntext-ask popover listview { background: #2c2c2c; }
.vntext-ask .box popover label, .vntext-ask .box popover image { color: white; }
.vntext-ask popover row:hover, .vntext-ask popover row:selected { background: #505050; }
.vntext-ask switch { background: #555; }
.vntext-ask switch:checked { background: #e0a458; }
.vntext-ask scale trough { background: #555; }
.vntext-ask scale highlight { background: #e0a458; }
.vntext-ask progressbar trough { background: #555; }
.vntext-ask progressbar progress { background: #e0a458; }
.vntext-ask .box list label { color: #e8e8e8; }
.vntext-pick label { color: white; font-size: 20px; background: rgba(0, 0, 0, 0.8); padding: 8px 16px; border-radius: 8px; }
.vntext .strip { border: 1px solid rgba(255, 255, 255, 0.12); border-radius: 12px; padding: 16px 20px; }
.vntext .strip .panel-header { opacity: 0.72; transition: opacity 180ms ease; }
.vntext .strip .panel-header.active { opacity: 1; }
.vntext .strip .panel-header button, .vntext .strip button.restore { min-height: 22px; min-width: 22px; padding: 5px 7px; border-radius: 7px; background: rgba(255, 255, 255, 0.08); color: #e6edf7; border: none; box-shadow: none; }
.vntext .strip .panel-header button:hover, .vntext .strip button.restore:hover { background: rgba(255, 255, 255, 0.18); }
.vntext .strip .panel-header label, .vntext .strip button.restore label { font-size: 12px; text-shadow: none; }
.vntext .strip .panel-header label.panel-title { font-size: 13px; font-weight: bold; color: #aeb7c8; }
.vntext .strip.collapsed { padding: 4px; }
.vntext .strip button.restore { min-width: 36px; min-height: 36px; padding: 4px; }
.vntext .strip.editing { border-color: #e0a458; }
.vntext .strip.editing .panel-header { opacity: 1; }
.vntext .strip .layout-editor { padding: 5px 0; }
.vntext .strip .layout-editor label { font-size: 12px; text-shadow: none; }
.vntext .strip .layout-editor button { min-height: 22px; padding: 4px 8px; border: none; box-shadow: none; border-radius: 6px; background: #293548; color: #e6edf7; }
.vntext .strip .layout-editor button.done { background: #e0a458; color: #171b24; }
.vntext .strip .layout-editor button.done label { color: #171b24; }
.vntext .strip .layout-editor scale trough { background: #3b485d; }
.vntext .strip .layout-editor scale highlight { background: #e0a458; }
.vntext .resize-handle { background: rgba(224, 164, 88, 0.4); border-radius: 3px; }
.vntext .resize-handle:hover { background: #e0a458; }
.vntext .strip label.intro { font-size: 14px; }
.vntext .strip label.source { font-size: 11px; padding: 0; color: #aeb7c8; }
.vntext .strip label.caption, .vntext .strip label.hinted { font-size: 12px; text-shadow: none; }
.vntext .strip .history { padding-top: 5px; }
.vntext .strip .history button, .vntext .strip button.service { min-height: 22px; padding: 3px 7px; border-radius: 6px; }
.vntext .strip .history button label { font-size: 12px; text-shadow: none; }
.vntext .strip .history button { background: #293548; color: #e6edf7; border: none; box-shadow: none; }
.vntext .strip .history button:hover { background: #3b4b64; }
.vntext .strip .history button:disabled { opacity: 0.4; }
.vntext .strip button.hint { min-height: 24px; min-width: 24px; }
.vntext .strip button.dismiss { background: #293548; color: #e6edf7; }
.vntext .strip label.caption { color: #aeb7c8; }
.vntext .strip button.listen { min-width: 24px; min-height: 24px; padding: 5px; border-radius: 7px; background: #293548; color: #e6edf7; border: none; box-shadow: none; }
.vntext .strip button.listen:hover { background: #3b4b64; }
.vntext .strip button.listen.on { background: #e0a458; color: #171b24; }
.vntext-menu contents { background: #18202d; color: #e6edf7; border: 1px solid #354155; border-radius: 10px; padding: 8px; }
.vntext-menu button { min-height: 28px; padding: 4px 10px; background: #252f40; color: #e6edf7; border: none; box-shadow: none; }
.vntext-menu button:hover { background: #354155; }
.vntext-menu label { color: #e6edf7; font-size: 13px; text-shadow: none; }
.vntext-menu label.caption { color: #aeb7c8; font-size: 11px; }
.vntext-ask .box { background: #18202d; border: 1px solid #354155; border-radius: 14px; padding: 16px; }
.vntext-ask .box label { color: #e6edf7; font-size: 13px; }
.vntext-ask .box label.heading { font-size: 17px; font-weight: bold; }
.vntext-ask .box label.caption { color: #aeb7c8; font-size: 12px; }
.vntext-ask .box button { min-height: 24px; min-width: 24px; padding: 5px 10px; border-radius: 7px; background: #293548; color: #e6edf7; }
.vntext-ask .box button label { color: #e6edf7; }
.vntext-ask .box button:hover { background: #35455c; }
.vntext-ask .box button.on, .vntext-ask .box button.suggested-action { background: #e0a458; color: #171b24; }
.vntext-ask .box button.on label, .vntext-ask .box button.suggested-action label { color: #171b24; }
.vntext-ask .box button.swatch { padding: 0; border-radius: 16px; }
.vntext-ask .box button.swatch.on { border-color: #e0a458; }
.vntext-ask .box entry { background: #111824; border-color: #3b485d; border-radius: 6px; }
.vntext-ask .box spinbutton { background: #111824; color: #e6edf7; border: 1px solid #3b485d; border-radius: 7px; box-shadow: none; }
.vntext-ask .box spinbutton > text { background: none; color: #e6edf7; caret-color: #e6edf7; min-width: 40px; padding: 0 8px; }
.vntext-ask .box spinbutton > text selection { background: #3b536f; color: white; }
.vntext-ask .box spinbutton > button { min-width: 35px; min-height: 34px; margin: 0; padding: 0; border-radius: 0; border-left: 1px solid #3b485d; background: #293548; color: #e6edf7; }
.vntext-ask .box spinbutton > button image { color: #e6edf7; -gtk-icon-size: 16px; }
.vntext-ask .box spinbutton > button.up { border-radius: 0 6px 6px 0; }
.vntext-ask .box spinbutton > button:hover { background: #3b4b64; }
.vntext-ask .box spinbutton > button:active { background: #4b607d; }
.vntext-ask .box spinbutton > button:disabled { opacity: 0.4; }
.vntext-ask .box spinbutton:focus-within { border-color: #e0a458; outline: 2px solid #e0a458; outline-offset: -2px; }
.vntext-ask .box dropdown button { background: #252f40; }
.vntext-ask .box flowboxchild { padding: 0; }
";

/// Starts the overlay for the game `pid`. It shows the messages and notices
/// sent to the page from now on. With `reading` it has the text box read
/// from pictures at once, asking where the box is if it was never told.
/// Without the overlay, a reading that is `seen` nowhere else ends: started
/// from the launcher, it would go on with nothing on screen to tell of it.
pub fn start(hub: web::Hub, pid: u32, pictures: Arc<ocr::Control>, settings: ocr::Settings, reading: bool, seen: bool) {
    let events = hub.subscribe();
    let speech = tts::start(&hub, pid);
    thread::spawn(move || {
        if let Err(e) = run(events, hub, pid, pictures, settings, reading, speech) {
            if seen {
                eprintln!("\x1b[33mno overlay: {e}\x1b[0m");
            } else {
                crate::die(&format!("no overlay: {e}"));
            }
        }
    });
}

/// Which of a message the strip shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// its translation, or the message itself if it has none
    Translation,
    Japanese,
    Both,
}

impl Mode {
    const ALL: [(Mode, &'static str, &'static str); 3] = [(Mode::Translation, "translation", "Bản dịch"), (Mode::Japanese, "japanese", "Nhật"), (Mode::Both, "both", "Nhật + dịch")];

    fn label(self) -> &'static str {
        Mode::ALL.iter().find(|m| m.0 == self).unwrap().2
    }

    /// What the mode is called where there is room to say it.
    fn name(self) -> &'static str {
        match self {
            Mode::Translation => "Bản dịch",
            Mode::Japanese => "Tiếng Nhật",
            Mode::Both => "Cả hai",
        }
    }

    fn next(self) -> Mode {
        let at = Mode::ALL.iter().position(|m| m.0 == self).unwrap();
        Mode::ALL[(at + 1) % Mode::ALL.len()].0
    }
}

/// How the reader has set the strip up for one game.
#[derive(Clone, PartialEq, Debug)]
struct Look {
    font: i32,
    /// how opaque the background of the strip is
    opacity: f64,
    /// the colour of its text, as `#rrggbb`
    colour: String,
    mode: Mode,
    /// where the strip is from the top left corner of the game's window and
    /// how wide; `None` to have it put by the window
    place: Option<(i32, i32, i32)>,
    /// Total expanded height, or automatic sizing from the message.
    height: Option<i32>,
    /// Collapsed icon position relative to the game; independent of expanded size.
    icon: Option<(i32, i32)>,
    hidden: bool,
    copy: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look { font: FONT, opacity: OPACITY, colour: COLOUR.to_string(), mode: Mode::Translation, place: None, height: None, icon: None, hidden: false, copy: false }
    }
}

impl Look {
    fn from_json(saved: &Value) -> Look {
        let number = |v: &Value| v.as_i64().map(|n| n as i32);
        let place = saved["place"].as_array().and_then(|p| Some((number(p.first()?)?, number(p.get(1)?)?, number(p.get(2)?)?)));
        Look {
            font: number(&saved["font"]).filter(|f| FONTS.contains(f)).unwrap_or(FONT),
            opacity: saved["opacity"].as_f64().filter(|o| OPACITIES.contains(o)).unwrap_or(OPACITY),
            colour: saved["colour"].as_str().filter(|c| is_colour(c)).unwrap_or(COLOUR).to_string(),
            mode: Mode::ALL.iter().find(|m| saved["mode"] == m.1).map_or(Mode::Translation, |m| m.0),
            place: place.filter(|p| p.2 >= NARROWEST),
            height: number(&saved["height"]).filter(|h| *h >= SHORTEST),
            icon: saved["icon"].as_array().and_then(|p| Some((number(p.first()?)?, number(p.get(1)?)?))),
            hidden: saved["hidden"] == true,
            copy: saved["copy"] == true,
        }
    }

    fn to_json(&self) -> Value {
        let mode = Mode::ALL.iter().find(|m| m.0 == self.mode).unwrap().1;
        json!({"font": self.font, "opacity": self.opacity, "colour": self.colour, "mode": mode, "place": self.place.map(|p| [p.0, p.1, p.2]), "height": self.height, "icon": self.icon.map(|p| [p.0, p.1]), "hidden": self.hidden, "copy": self.copy})
    }
}

/// Whether a text is a colour as `#rrggbb`, the only way that one is let into the style.
fn is_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// The style of the strip for a look, in two parts. The first, its background
/// and the size of its text, is given up to whatever `STYLE` says of a part
/// (a provider loaded later wins over an earlier one whatever the selectors,
/// so this one is loaded before). The second, the colour of the message,
/// wins over it.
fn strip_style(look: &Look) -> (String, String) {
    (
        format!(".vntext .strip {{ background: rgba(0, 0, 0, {:.2}); }} .vntext .strip label {{ font-size: {}px; }}", look.opacity, look.font),
        format!(".vntext .strip label.japanese, .vntext .strip label.alone {{ color: {}; }}", look.colour),
    )
}

/// Keep the expanded panel inside its output, even when its button is folded.
fn panel(game: (i32, i32, i32, i32), saved: Option<(i32, i32, i32)>, screen: (i32, i32, i32, i32), minimum: i32, tall: i32) -> (i32, i32, i32) {
    let (x, y, width, height) = game;
    let (left, top, wide, high) = screen;
    let (px, py, wanted) = saved.map_or((x + INSET, y + height + GAP, width - INSET * 2), |(dx, dy, w)| (x + dx, y + dy, w));
    let width = wanted.max(minimum).min(wide.max(1));
    let py = if saved.is_none() && py + tall > top + high { y + height - tall - GAP } else { py };
    (px.clamp(left, left + wide - width), py.clamp(top, top + (high - tall).max(0)), width)
}

/// Resize the chosen edges, keeping the opposite edges still and the panel on screen.
fn resized(rect: (i32, i32, i32, i32), delta: (i32, i32), edges: (i32, i32), minimum: (i32, i32), screen: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    fn axis(pos: i32, size: i32, delta: i32, edge: i32, min: i32, start: i32, space: i32) -> (i32, i32) {
        let end = start + space;
        let min = min.min(space);
        let size = size.clamp(min, space);
        let pos = pos.clamp(start, end - size);
        match edge {
            -1 => { let next = (pos + delta).clamp(start, pos + size - min); (next, pos + size - next) }
            1 => (pos, (size + delta).clamp(min, end - pos)),
            _ => (pos, size),
        }
    }
    let (x, w) = axis(rect.0, rect.2, delta.0, edges.0, minimum.0, screen.0, screen.2);
    let (y, h) = axis(rect.1, rect.3, delta.1, edges.1, minimum.1, screen.1, screen.3);
    (x, y, w, h)
}

/// The file of looks: a JSON object with one look for each game.
fn looks_file() -> PathBuf {
    ocr::config_dir("XDG_CONFIG_HOME", ".config").join("overlay.json")
}

fn saved_looks() -> Value {
    let file = std::fs::read_to_string(looks_file()).unwrap_or_default();
    serde_json::from_str(&file).ok().filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Whether the file of looks says that the first-time help was read.
fn intro_read(looks: &Value) -> bool {
    looks[INTRO_KEY] == true
}

fn save_intro_read() {
    let mut looks = saved_looks();
    looks[INTRO_KEY] = json!(true);
    let file = looks_file();
    let _ = std::fs::create_dir_all(file.parent().unwrap());
    if let Err(e) = std::fs::write(&file, format!("{looks:#}\n")) {
        eprintln!("cannot save the overlay's look to {}: {e}", file.display());
    }
}

fn save_look(game: &str, look: &Look) {
    let mut looks = saved_looks();
    looks[game] = look.to_json();
    let file = looks_file();
    let _ = std::fs::create_dir_all(file.parent().unwrap());
    if let Err(e) = std::fs::write(&file, format!("{looks:#}\n")) {
        eprintln!("cannot save the overlay's look to {}: {e}", file.display());
    }
}

struct Message {
    seq: u64,
    /// its place in the game's script, when it comes from a profile
    index: Option<usize>,
    name: Option<String>,
    lines: Vec<String>,
    translation: Option<String>,
    pending: bool,
    /// read from a picture of the text box, not taken from the game's script
    pictured: bool,
    /// translated by Google Translate
    google: bool,
}

/// Something to tell the reader, and what they may do about it.
#[derive(Clone, PartialEq, Debug)]
struct Said {
    text: String,
    action: Option<Action>,
}

/// The messages the game has shown and the one the strip is on.
#[derive(Default)]
struct Shown {
    messages: Vec<Message>,
    at: usize,
    /// Notices are kept beside the message, never in place of it.
    notice: Option<Said>,
    /// said beside the message, which is still shown
    hint: Option<Said>,
    expires: Option<Instant>,
}

impl Shown {
    /// Takes an event of the page: `message` with a number, a name and
    /// lines, `translation` for a number, or `notice`.
    fn take(&mut self, event: &str) {
        let Ok(event) = serde_json::from_str::<Value>(event) else { return };
        let seq = event["seq"].as_u64().unwrap_or(0);
        let text = |v: &Value| v.as_str().map(str::to_string);
        match event["type"].as_str() {
            Some("message") => {
                let following = self.old().is_none();
                let lines = event["lines"].as_array().into_iter().flatten().filter_map(text).collect();
                let pending = event["translating"] == true;
                let pictured = event["source"] == "ocr";
                let index = event["index"].as_u64().map(|index| index as usize);
                self.messages.push(Message { seq, index, name: text(&event["name"]), lines, translation: None, pending, pictured, google: false });
                if self.messages.len() > KEPT {
                    self.messages.remove(0);
                    self.at = self.at.saturating_sub(1);
                }
                if following {
                    self.latest();
                }
                // Errors with an offered remedy stay until dismissed or recovered.
                if self.notice.as_ref().is_some_and(|notice| notice.action.is_none()) {
                    self.notice = None;
                    self.expires = None;
                }
                self.hint = None;
            }
            Some("translation") => {
                if let Some(message) = self.messages.iter_mut().rev().find(|m| m.seq == seq) {
                    (message.translation, message.pending) = (text(&event["tr"]), false);
                    message.google = event["via"] == "google";
                }
                // what failed before has been made good
                if self.expires.is_none() && self.old().is_none() && self.messages.last().is_some_and(|m| m.seq == seq) && event["tr"].is_string() {
                    self.dismiss();
                }
            }
            Some(kind @ ("notice" | "hint" | "toast")) => {
                let said = text(&event["text"]).map(|text| Said { text, action: event["action"].as_str().and_then(Action::named) });
                if kind != "hint" {
                    self.notice = said;
                    self.expires = (kind == "toast").then(|| Instant::now() + TOAST);
                } else {
                    self.hint = said;
                }
            }
            _ => {}
        }
    }

    /// Who says the message shown.
    fn speaker(&self) -> Option<&str> {
        self.messages.get(self.at)?.name.as_deref()
    }

    /// Where the message shown comes from, in a word for the strip.
    fn source(&self) -> Option<String> {
        let message = self.messages.get(self.at)?;
        Some(if message.pictured { "Đọc từ ảnh" } else { "Trích text trực tiếp" }.to_string())
    }

    /// The message shown among those there are, while it is an old one.
    fn old(&self) -> Option<(usize, usize)> {
        (self.at + 1 < self.messages.len()).then_some((self.at + 1, self.messages.len()))
    }

    /// What is said beside the message: a hint, or the button of a notice,
    /// and what the button does.
    fn offered(&self) -> Option<(String, Option<Action>)> {
        match (&self.notice, &self.hint) {
            (Some(notice), _) => Some((notice.text.clone(), notice.action)),
            (None, Some(hint)) => Some((hint.text.clone(), hint.action)),
            (None, None) => None,
        }
    }

    fn dismiss(&mut self) {
        self.notice = None;
        self.hint = None;
        self.expires = None;
    }

    fn expire(&mut self, now: Instant) -> bool {
        if self.expires.is_some_and(|until| now >= until) {
            self.notice = None;
            self.expires = None;
            return true;
        }
        false
    }

    fn latest(&mut self) {
        self.at = self.messages.len().saturating_sub(1);
    }

    /// Has the message shown translated anew, if it is one of the script's
    /// and has a translation; gives its number and place in the script.
    fn retranslate(&mut self) -> Option<(u64, usize)> {
        let message = self.messages.get_mut(self.at).filter(|m| !m.pending && m.translation.is_some())?;
        let index = message.index?;
        (message.translation, message.pending) = (None, true);
        Some((message.seq, index))
    }

    /// Whether the message shown can be translated anew.
    fn retranslatable(&self) -> bool {
        self.messages.get(self.at).is_some_and(|m| m.index.is_some() && !m.pending && m.translation.is_some())
    }

    /// Whether Google Translate made the translation shown.
    fn by_google(&self) -> bool {
        self.messages.get(self.at).is_some_and(|m| m.google)
    }

    /// What of the message shown is put on the clipboard: what the strip
    /// shows of it.
    fn copied(&self, mode: Mode) -> Option<String> {
        let (japanese, translation, placeholder) = self.texts(mode);
        let parts: Vec<String> = japanese.into_iter().chain(translation.filter(|_| !placeholder)).collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }

    fn step(&mut self, by: isize) {
        self.at = self.at.saturating_add_signed(by).min(self.messages.len().saturating_sub(1));
    }

    /// What to show: the message, its translation, and whether what stands
    /// for the translation is only a placeholder.
    fn texts(&self, mode: Mode) -> (Option<String>, Option<String>, bool) {
        let Some(message) = self.messages.get(self.at) else { return (None, None, false) };
        let japanese = message.lines.join("\n");
        let translation = message.translation.clone().or_else(|| message.pending.then(|| PENDING.to_string()));
        match (mode, translation) {
            (Mode::Japanese, _) | (_, None) => (Some(japanese), None, false),
            (Mode::Translation, translation) => (None, translation, message.pending),
            (Mode::Both, translation) => (Some(japanese), translation, message.pending),
        }
    }
}

struct Ui {
    shown: Shown,
    look: Look,
    /// Only moving and resizing depend on this; text can always be selected.
    locked: bool,
    game: Option<WindowId>,
    /// where the game's window is, while it is the one being played
    placed: Option<(i32, i32, i32, i32)>,
    /// where the strip is on screen and how wide
    strip: (i32, i32, i32),
    /// the reader is pointing out the text box
    picking: bool,
    /// pictures are to be read as soon as the game's window is found
    reading: bool,
    /// the help for the first time is on the strip, in place of messages
    intro: bool,
    /// the selection as it was a tick ago, and the one last copied
    selection: Option<(i32, i32)>,
    copied: Option<(i32, i32)>,
    /// where in the message the pointer is, in bytes, and where in the strip
    hover: Option<(usize, i32)>,
    /// the place last looked up and the number of that lookup
    asked: Option<usize>,
    lookups: u64,
    /// the pointer is on the meaning shown, and for how many ticks it has
    /// been on neither that nor a word
    over_meaning: bool,
    away: u32,
    history_open: bool,
    expanded_height: i32,
    chrome_height: i32,
    icon_at: (i32, i32),
}

struct Overlay {
    ui: RefCell<Ui>,
    /// The settings are open. They take the keyboard from the game, which
    /// must still count as the window being read: the strip stays for the
    /// reader to see what is changed, and the text box can be picked.
    asking: Cell<bool>,
    events: Receiver<String>,
    screen: Box<dyn Screen>,
    display: gdk::Display,
    pid: u32,
    game: String,
    hub: web::Hub,
    pictures: Arc<ocr::Control>,
    settings: ocr::Settings,
    /// the background and the size of the text of the strip, and the colour of the message
    sizes: gtk4::CssProvider,
    look: gtk4::CssProvider,
    strip: gtk4::Window,
    stage: gtk4::Fixed,
    frame: gtk4::Overlay,
    content: gtk4::Box,
    /// the name of who speaks, set apart from what is said
    speaker: gtk4::Label,
    japanese: gtk4::Label,
    translation: gtk4::Label,
    text_scroll: gtk4::ScrolledWindow,
    passage: gtk4::Box,
    /// says in small letters where the message comes from
    source: gtk4::Label,
    /// says who translates, and lets another be chosen
    service: gtk4::Button,
    /// the help for the first time
    intro: gtk4::Box,
    /// what is said beside the message, and the button that does something about it
    hinted: gtk4::Label,
    hint: gtk4::Button,
    dismiss: gtk4::Button,
    history: gtk4::Box,
    position: gtk4::Label,
    previous: gtk4::Button,
    next: gtk4::Button,
    latest: gtk4::Button,
    offer: Cell<Option<Action>>,
    move_area: gtk4::Box,
    title: gtk4::Label,
    handles: Vec<((i32, i32), gtk4::Box)>,
    editor: gtk4::Box,
    done: gtk4::Button,
    opacity: gtk4::Scale,
    opacity_now: gtk4::Label,
    automatic: gtk4::CheckButton,
    reset: gtk4::Button,
    layout_sync: Cell<bool>,
    /// the meaning of the word under the pointer, from the dictionaries
    popup: gtk4::Window,
    meaning: gtk4::Box,
    scrolled: gtk4::ScrolledWindow,
    asks: Sender<(u64, String)>,
    answers: Receiver<(u64, Result<yomitan::Found, Trouble>)>,
    header: gtk4::Box,
    body: gtk4::Box,
    restore: gtk4::Button,
    icon_moved: Cell<bool>,
    history_summary: gtk4::Button,
    history_controls: gtk4::Box,
    empty: gtk4::Label,
    hide: gtk4::Button,
    mode: gtk4::Button,
    /// says on the button which of a message is shown
    mode_now: gtk4::Label,
    speech: tts::Control,
    listen: gtk4::Button,
    speech_status: gtk4::Label,
}

/// A surface that floats above the windows at a place of its own choosing,
/// never taking the keyboard.
fn floating(class: &str) -> gtk4::Window {
    let window = gtk4::Window::new();
    window.add_css_class(class);
    window.init_layer_shell();
    window.set_namespace(Some("vntext"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    // placed by its distance from the top left corner of the screen, panels or not
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window.set_exclusive_zone(-1);
    window
}

fn label(class: &str) -> gtk4::Label {
    let label = gtk4::Label::new(None);
    label.add_css_class(class);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label
}

/// A button of the bar that shows an icon.
fn pictured(icon: &str, tip: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tip));
    button.set_focusable(false);
    button
}

/// A row of small coloured labels that goes on to the next line when full.
fn pills() -> gtk4::FlowBox {
    let row = gtk4::FlowBox::new();
    row.set_selection_mode(gtk4::SelectionMode::None);
    row.set_max_children_per_line(12);
    row.set_column_spacing(4);
    row.set_row_spacing(3);
    row.set_halign(gtk4::Align::Start);
    row
}

fn pill(text: &str, classes: &[&str]) -> gtk4::Label {
    let pill = gtk4::Label::new(Some(text));
    pill.add_css_class("pill");
    classes.iter().for_each(|class| pill.add_css_class(class));
    pill
}

/// One entry of a word, laid out as Yomitan lays it out: the reading over
/// the word, how common it is, what it is marked as and by which
/// dictionary, then what each dictionary says.
fn word(entry: &yomitan::Entry) -> gtk4::Box {
    let all = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    let plain = |class: &str, text: &str| {
        let label = label(class);
        label.set_text(text);
        label
    };
    if !entry.reading.is_empty() && entry.reading != entry.term {
        all.append(&plain("reading", &entry.reading));
    }
    all.append(&plain("term", &entry.term));
    if !entry.frequencies.is_empty() {
        let row = pills();
        for (counter, numbers) in &entry.frequencies {
            let both = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            both.add_css_class("counted");
            both.append(&pill(counter, &["counter"]));
            both.append(&pill(numbers, &["numbers"]));
            row.insert(&both, -1);
        }
        all.append(&row);
    }
    for definition in &entry.definitions {
        let row = pills();
        for (name, kind) in &definition.tags {
            row.insert(&pill(name, &["tag", kind]), -1);
        }
        if !definition.dictionary.is_empty() {
            row.insert(&pill(&definition.dictionary, &["dictionary"]), -1);
        }
        all.append(&row);
        for block in &definition.blocks {
            match block {
                yomitan::Block::Text(markup) => {
                    let gloss = label("gloss");
                    gloss.set_markup(markup);
                    all.append(&gloss);
                }
                yomitan::Block::Example { japanese, translation } => {
                    let example = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
                    example.add_css_class("example");
                    let sentence = label("sentence");
                    sentence.set_markup(japanese);
                    example.append(&sentence);
                    if !translation.is_empty() {
                        let meant = label("meant");
                        meant.set_markup(translation);
                        example.append(&meant);
                    }
                    all.append(&example);
                }
            }
        }
    }
    all
}

/// A tab of the settings window.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Display,
    Translate,
    Dictionary,
    Ocr,
    Speech,
}

impl Tab {
    const ALL: [(Tab, &'static str, &'static str); 5] = [(Tab::Display, "display", "Hiển thị"), (Tab::Translate, "translate", "Dịch"), (Tab::Dictionary, "dictionary", "Từ điển"), (Tab::Ocr, "ocr", "OCR"), (Tab::Speech, "speech", "Giọng đọc")];

    fn id(self) -> &'static str {
        Tab::ALL.iter().find(|t| t.0 == self).unwrap().1
    }
}

/// Where dictionaries are to be had.
const DICTIONARIES: &str = "https://yomitan.wiki/dictionaries/";

/// What the thread that imports dictionaries tells.
enum Imported {
    /// the file at this place of the list, and how much of it is read
    Reading(usize, f64),
    Added(String),
    Failed(String, String),
    Finished,
}

/// Reads the zip files in `paths` into the store, one after the other, until
/// `cancel` is set.
fn import_all(paths: Vec<PathBuf>, cancel: Arc<AtomicBool>, tell: Sender<Imported>) {
    let result = Store::open().map(|mut store| {
        for (at, path) in paths.iter().enumerate() {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let read = std::fs::File::open(path).map_err(|e| format!("Không mở được file: {e}")).and_then(|file| {
                import::import(&mut store, std::io::BufReader::new(file), &mut |done, total| {
                    let _ = tell.send(Imported::Reading(at, done as f64 / total.max(1) as f64));
                    !cancel.load(Ordering::Relaxed)
                })
            });
            let _ = tell.send(match read {
                Ok(title) => Imported::Added(title),
                Err(e) => Imported::Failed(name, e),
            });
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }
    });
    if let Err(e) = result {
        let _ = tell.send(Imported::Failed("kho từ điển".to_string(), e));
    }
    let _ = tell.send(Imported::Finished);
}

/// Lists the dictionaries imported, each with its switch, its place in the
/// order and a button to remove it.
fn fill_dictionaries(list: &gtk4::ListBox, note: &gtk4::Label) {
    while let Some(old) = list.first_child() {
        list.remove(&old);
    }
    let all = Store::open().and_then(|store| store.dictionaries());
    let all: Vec<Info> = match all {
        Ok(all) => all,
        Err(e) => return note.set_text(&e),
    };
    if all.is_empty() {
        list.append(&gtk4::Label::new(Some("Chưa có từ điển nào.")));
    }
    for info in all {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let on = gtk4::Switch::new();
        on.set_active(info.enabled);
        on.set_valign(gtk4::Align::Center);
        let id = info.id;
        let told = note.clone();
        on.connect_active_notify(move |on| {
            if let Err(e) = Store::open().and_then(|store| store.enable(id, on.is_active())) {
                told.set_text(&e);
            }
        });
        let named = gtk4::Label::new(Some(&if info.revision.is_empty() { info.title.clone() } else { format!("{} ({})", info.title, info.revision) }));
        named.set_xalign(0.0);
        named.set_hexpand(true);
        named.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        row.append(&on);
        row.append(&named);
        let act = |icon: &str, tip: &str, act: Box<dyn Fn(&mut Store) -> Result<(), String>>| {
            let button = pictured(icon, tip);
            button.add_css_class("icon");
            let (list, note) = (list.clone(), note.clone());
            button.connect_clicked(move |_| {
                if let Err(e) = Store::open().and_then(|mut store| act(&mut store)) {
                    note.set_text(&e);
                }
                fill_dictionaries(&list, &note);
            });
            row.append(&button);
        };
        act("go-up-symbolic", "Lên trước: nghĩa của từ điển này hiện sớm hơn", Box::new(move |store| store.shift(id, true)));
        act("go-down-symbolic", "Xuống sau", Box::new(move |store| store.shift(id, false)));
        act("user-trash-symbolic", "Xoá từ điển này khỏi tool", Box::new(move |store| store.remove(id)));
        list.append(&row);
    }
}

/// The tab of dictionaries: those imported, a way to import more (a file
/// chosen, or dropped on the tab) and where words are looked up.
fn dictionaries(window: &gtk4::Window) -> gtk4::Box {
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_min_content_height(120);
    scroll.set_max_content_height(240);
    scroll.set_propagate_natural_height(true);
    page.append(&scroll);
    let note = gtk4::Label::new(None);
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.set_max_width_chars(48);
    fill_dictionaries(&list, &note);

    let progress = gtk4::ProgressBar::new();
    let cancel = gtk4::Button::with_label("Huỷ");
    let working = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    progress.set_hexpand(true);
    progress.set_valign(gtk4::Align::Center);
    working.append(&progress);
    working.append(&cancel);
    working.set_visible(false);

    // the files named are read off this thread, and this one is told how far they are
    let stopping: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::default();
    let begin = {
        let (list, note, working, progress, stopping) = (list.clone(), note.clone(), working.clone(), progress.clone(), stopping.clone());
        Rc::new(move |paths: Vec<PathBuf>| {
            if paths.is_empty() || stopping.borrow().is_some() {
                return;
            }
            let (cancel, (tell, told)) = (Arc::new(AtomicBool::new(false)), mpsc::channel());
            *stopping.borrow_mut() = Some(cancel.clone());
            let files = paths.len();
            thread::spawn(move || import_all(paths, cancel, tell));
            note.set_text("");
            progress.set_fraction(0.0);
            working.set_visible(true);
            let (list, note, working, progress, stopping) = (list.clone(), note.clone(), working.clone(), progress.clone(), stopping.clone());
            let (mut added, mut failed) = (Vec::new(), Vec::new());
            glib::timeout_add_local(TICK, move || {
                let mut finished = false;
                for told in told.try_iter() {
                    match told {
                        Imported::Reading(at, share) => progress.set_fraction((at as f64 + share) / files as f64),
                        Imported::Added(title) => added.push(title),
                        Imported::Failed(name, e) => failed.push(format!("{name}: {e}")),
                        Imported::Finished => finished = true,
                    }
                }
                if !finished {
                    return glib::ControlFlow::Continue;
                }
                *stopping.borrow_mut() = None;
                working.set_visible(false);
                fill_dictionaries(&list, &note);
                let mut said = Vec::new();
                if !added.is_empty() {
                    said.push(format!("Đã thêm: {}.", added.join(", ")));
                }
                said.append(&mut failed);
                note.set_text(&said.join("\n"));
                glib::ControlFlow::Break
            });
        })
    };
    let stop = stopping.clone();
    cancel.connect_clicked(move |_| {
        if let Some(cancel) = stop.borrow().as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    });

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let add = gtk4::Button::with_label("Thêm từ điển…");
    let download = gtk4::Button::with_label("Tải từ điển");
    download.set_tooltip_text(Some(DICTIONARIES));
    buttons.append(&add);
    buttons.append(&download);
    page.append(&buttons);
    let how = gtk4::Label::new(Some("Tải file zip về rồi thêm vào đây, không cần giải nén. Cũng kéo thả file zip vào tab này được."));
    how.set_xalign(0.0);
    how.set_wrap(true);
    how.set_max_width_chars(48);
    page.append(&how);
    page.append(&working);
    page.append(&note);

    let chosen = gtk4::DropDown::from_strings(&Source::ALL.map(|s| s.2));
    chosen.set_selected(Source::ALL.iter().position(|s| s.0 == lookup::source()).unwrap_or(0) as u32);
    chosen.connect_selected_notify(|chosen| lookup::set_source(Source::ALL.get(chosen.selected() as usize).map_or(Source::Auto, |s| s.0)));
    let wanted = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let nth = gtk4::Label::new(Some("Tra từ bằng"));
    nth.set_hexpand(true);
    nth.set_xalign(0.0);
    wanted.append(&nth);
    wanted.append(&chosen);
    page.append(&wanted);

    download.connect_clicked(move |_| {
        let _ = gtk4::gio::AppInfo::launch_default_for_uri(DICTIONARIES, None::<&gtk4::gio::AppLaunchContext>);
    });
    // The window floats above every other, so it steps aside while the
    // dialog is open, and comes back when it is closed.
    let aside = window.clone();
    let reading = begin.clone();
    add.connect_clicked(move |_| {
        let filter = gtk4::FileFilter::new();
        filter.add_suffix("zip");
        filter.set_name(Some("Từ điển Yomitan (.zip)"));
        let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        let dialog = gtk4::FileDialog::builder().title("Chọn từ điển").filters(&filters).build();
        aside.set_visible(false);
        let (back, reading) = (aside.clone(), reading.clone());
        dialog.open_multiple(None::<&gtk4::Window>, gtk4::gio::Cancellable::NONE, move |files| {
            back.set_visible(true);
            let files = files.map(|files| files.into_iter().flatten().filter_map(|f| f.downcast::<gtk4::gio::File>().ok()).filter_map(|f| f.path()).collect()).unwrap_or_default();
            reading(files);
        });
    });
    let dropped = gtk4::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    dropped.connect_drop(move |_, value, _, _| {
        let paths: Vec<PathBuf> = value.get::<gdk::FileList>().map(|files| files.files().into_iter().filter_map(|f| f.path()).collect()).unwrap_or_default();
        begin(paths);
        true
    });
    page.add_controller(dropped);
    page
}

fn switch(button: &gtk4::Button, on: bool) {
    if on {
        button.add_css_class("on");
    } else {
        button.remove_css_class("on");
    }
}

impl Overlay {
    fn save(&self) {
        save_look(&self.game, &self.ui.borrow().look);
    }

    /// Puts into the strip and on the buttons what the state says.
    fn render(&self) {
        let mut ui = self.ui.borrow_mut();
        let (japanese, translation, placeholder) = ui.shown.texts(ui.look.mode);
        self.speaker.set_visible(ui.shown.speaker().is_some());
        self.speaker.set_text(ui.shown.speaker().unwrap_or_default());
        self.japanese.set_visible(japanese.is_some());
        let changed = self.japanese.text() != japanese.as_deref().unwrap_or_default();
        if changed {
            self.japanese.set_text(japanese.as_deref().unwrap_or_default());
            (ui.selection, ui.copied) = (None, None);
            (ui.hover, ui.asked) = (None, None);
            self.japanese.set_attributes(None);
            self.popup.set_visible(false);
        }
        self.translation.set_visible(translation.is_some());
        self.translation.set_text(translation.as_deref().unwrap_or_default());
        for (class, on) in [("pending", placeholder), ("alone", japanese.is_none() && !placeholder)] {
            if on {
                self.translation.add_css_class(class);
            } else {
                self.translation.remove_css_class(class);
            }
        }
        let source = ui.shown.source();
        self.source.set_visible(source.is_some());
        self.source.set_text(source.as_deref().unwrap_or_default());
        if source.is_some_and(|s| s == "Đọc từ ảnh") {
            self.source.add_css_class("pictures");
        } else {
            self.source.remove_css_class("pictures");
        }
        let (sizes, colour) = strip_style(&ui.look);
        self.sizes.load_from_data(&sizes);
        self.look.load_from_data(&colour);
        // the first-time help takes the place of what the game says
        self.intro.set_visible(ui.intro);
        if ui.intro {
            [&self.speaker, &self.japanese, &self.translation, &self.source].iter().for_each(|l| l.set_visible(false));
        }
        let offered = ui.shown.offered().filter(|_| !ui.intro);
        self.offer.set(offered.as_ref().and_then(|o| o.1));
        self.hinted.set_visible(offered.as_ref().is_some_and(|o| !o.0.is_empty()));
        self.hinted.set_text(offered.as_ref().map_or("", |o| o.0.as_str()));
        self.hint.set_visible(self.offer.get().is_some());
        self.dismiss.set_visible(offered.is_some());
        if let Some(action) = self.offer.get() {
            self.hint.set_label(action.label());
        }
        self.japanese.set_selectable(true);
        self.listen.set_visible(!ui.intro && !ui.shown.messages.is_empty());
        let editing = !ui.locked && !ui.look.hidden;
        self.handles.iter().for_each(|(_, handle)| handle.set_visible(editing));
        self.editor.set_visible(editing);
        self.move_area.set_cursor_from_name(editing.then_some("move"));
        self.move_area.set_tooltip_text(editing.then_some("Kéo hàng tiêu đề để di chuyển dải"));
        self.title.set_text(if editing { "⠿ Kéo để di chuyển" } else { "vntext" });
        if editing { self.content.add_css_class("editing"); } else { self.content.remove_css_class("editing"); }
        self.layout_sync.set(true);
        self.opacity.set_value(ui.look.opacity);
        self.opacity_now.set_text(&format!("{:.0}%", ui.look.opacity * 100.0));
        self.automatic.set_active(ui.look.height.is_none());
        self.layout_sync.set(false);
        self.mode_now.set_text(ui.look.mode.label());
        self.header.set_visible(!ui.look.hidden);
        self.body.set_visible(!ui.look.hidden);
        self.restore.set_visible(ui.look.hidden);
        if ui.look.hidden {
            self.content.add_css_class("collapsed");
            self.popup.set_visible(false);
            if ui.hover.is_some() || ui.asked.is_some() {
                (ui.hover, ui.asked) = (None, None);
                ui.lookups += 1;
                self.japanese.set_attributes(None);
            }
        } else {
            self.content.remove_css_class("collapsed");
        }
        self.empty.set_visible(ui.shown.messages.is_empty() && !ui.intro && offered.is_none());
        self.history.set_visible(ui.shown.messages.len() > 1 && !ui.intro);
        let details = ui.history_open || ui.shown.old().is_some();
        self.history_summary.set_visible(!details);
        self.history_summary.set_label(&format!("Lịch sử · {} câu", ui.shown.messages.len()));
        self.history_controls.set_visible(details);
        self.previous.set_sensitive(ui.shown.at > 0);
        self.next.set_sensitive(ui.shown.old().is_some());
        self.latest.set_label(if ui.shown.old().is_some() { "Về câu mới" } else { "Thu gọn" });
        self.position.set_text(&match ui.shown.old() {
            Some((at, of)) => format!("Lịch sử · {at}/{of}"),
            None => "Câu mới nhất".to_string(),
        });
    }

    /// Puts the panel where it belongs by the game's window,
    /// or out of sight while the game is not the window being played.
    fn place(&self) {
        let mut ui = self.ui.borrow_mut();
        let Some((x, y, width, height)) = ui.placed.filter(|_| !ui.picking) else {
            self.strip.set_visible(false);
            self.popup.set_visible(false);
            return;
        };
        let monitors = self.display.monitors();
        let screens: Vec<gdk::Monitor> = monitors.into_iter().flatten().filter_map(|m| m.downcast::<gdk::Monitor>().ok()).collect();
        let monitor = screens.iter().min_by_key(|m| {
            let s = m.geometry();
            let cx = x + width / 2;
            let cy = y + height / 2;
            (cx - cx.clamp(s.x(), s.x() + s.width())).abs() + (cy - cy.clamp(s.y(), s.y() + s.height())).abs()
        });
        let bounds = monitor.map(|m| m.geometry()).unwrap_or_else(|| gdk::Rectangle::new(0, 0, width, height));
        if self.strip.monitor().as_ref() != monitor {
            self.strip.set_monitor(monitor);
            self.popup.set_monitor(monitor);
        }
        let minimum = if ui.look.hidden { ui.strip.2.max(NARROWEST) } else {
            (self.header.measure(gtk4::Orientation::Horizontal, -1).0.max(self.editor.measure(gtk4::Orientation::Horizontal, -1).0) + PADDING).max(NARROWEST)
        };
        let screen = (bounds.x(), bounds.y(), bounds.width(), bounds.height());
        let wide = panel((x, y, width, height), ui.look.place, screen, minimum, ui.expanded_height).2;
        self.content.set_size_request(if ui.look.hidden { -1 } else { wide }, -1);
        if !ui.look.hidden {
            let has_text = !ui.intro && !ui.shown.messages.is_empty();
            self.text_scroll.set_visible(has_text);
            // Measure chrome separately, then reserve the actual wrapped passage height.
            // GtkFixed measures unconstrained children; relying on its natural height
            // lets the scroller shrink to a sliver even when there is room for the text.
            self.text_scroll.set_max_content_height(-1);
            self.text_scroll.set_min_content_height(0);
            self.text_scroll.set_propagate_natural_height(false);
            self.text_scroll.set_vexpand(false);
            self.body.set_vexpand(false);
            ui.chrome_height = self.content.measure(gtk4::Orientation::Vertical, wide).1
                - self.text_scroll.measure(gtk4::Orientation::Vertical, (wide - PADDING).max(1)).1;
            let available = (bounds.height() - ui.chrome_height).max(1);
            let viewport = if !has_text { 0 } else if let Some(tall) = ui.look.height {
                (tall - ui.chrome_height).max(48).min(available)
            } else {
                let wrapped = self.passage.measure(gtk4::Orientation::Vertical, (wide - PADDING).max(1)).1;
                let line = self.japanese.create_pango_layout(Some("あ")).pixel_size().1;
                let comfortable = line * if ui.look.mode == Mode::Both { 4 } else { 2 } + 12;
                let cap = (bounds.height() * 2 / 5).min(360).min(available);
                wrapped.max(comfortable).min(cap)
            };
            self.text_scroll.set_min_content_height(viewport);
            self.text_scroll.set_max_content_height(viewport);
            let natural = self.content.measure(gtk4::Orientation::Vertical, wide).1;
            ui.expanded_height = natural.min(bounds.height());
            self.content.set_size_request(wide, ui.expanded_height);
        }
        let strip = panel((x, y, width, height), ui.look.place, screen, minimum, ui.expanded_height);
        if ui.look.hidden {
            let wanted = ui.look.icon.map(|(dx, dy)| (x + dx, y + dy))
                .unwrap_or(if ui.strip.2 == 0 { (strip.0, strip.1) } else { ui.icon_at });
            let w = self.content.measure(gtk4::Orientation::Horizontal, -1).1;
            let h = self.content.measure(gtk4::Orientation::Vertical, w).1;
            ui.icon_at = (wanted.0.clamp(bounds.x(), bounds.x() + (bounds.width() - w).max(0)),
                          wanted.1.clamp(bounds.y(), bounds.y() + (bounds.height() - h).max(0)));
        }
        ui.strip = strip;
        // Editing and the collapsed icon use stationary native coordinates.
        // Only the child moves; input outside the visible child passes through.
        let stationary = !ui.locked || ui.look.hidden;
        let at = if ui.look.hidden { ui.icon_at } else { (strip.0, strip.1) };
        self.strip.set_anchor(Edge::Right, stationary);
        self.strip.set_anchor(Edge::Bottom, stationary);
        self.stage.set_size_request(if stationary { bounds.width() } else { -1 }, if stationary { bounds.height() } else { -1 });
        self.stage.move_(&self.frame, if stationary { (at.0 - bounds.x()) as f64 } else { 0.0 }, if stationary { (at.1 - bounds.y()) as f64 } else { 0.0 });
        self.strip.set_default_size(if stationary { bounds.width() } else { strip.2 }, if stationary { bounds.height() } else { ui.expanded_height });
        self.strip.set_margin(Edge::Left, if stationary { 0 } else { at.0 - bounds.x() });
        self.strip.set_margin(Edge::Top, if stationary { 0 } else { at.1 - bounds.y() });
        let (japanese, _, _) = ui.shown.texts(ui.look.mode);
        self.strip.set_visible(true);
        if let Some(surface) = self.strip.surface() {
            let through = ui.locked && japanese.is_none() && !ui.intro;
            surface.set_input_region(Some(&self.input(through)));
        }
    }

    /// Translation-only leaves the game clickable while all controls still work.
    fn input(&self, through: bool) -> cairo::Region {
        if self.restore.is_visible() {
            let b = self.restore.compute_bounds(&self.strip).unwrap();
            return cairo::Region::create_rectangle(&cairo::RectangleInt::new(b.x().floor() as i32, b.y().floor() as i32, b.width().ceil() as i32, b.height().ceil() as i32));
        }
        if !through {
            let b = self.frame.compute_bounds(&self.strip).unwrap();
            return cairo::Region::create_rectangle(&cairo::RectangleInt::new(b.x().floor() as i32, b.y().floor() as i32, b.width().ceil() as i32, b.height().ceil() as i32));
        }
        let taking = cairo::Region::create();
        let widgets: [&gtk4::Widget; 10] = [self.header.upcast_ref(), self.restore.upcast_ref(), self.history_summary.upcast_ref(), self.service.upcast_ref(), self.hint.upcast_ref(), self.dismiss.upcast_ref(), self.previous.upcast_ref(), self.next.upcast_ref(), self.latest.upcast_ref(), self.listen.upcast_ref()];
        for button in widgets {
            if let Some(b) = button.compute_bounds(&self.strip).filter(|_| button.is_visible() && button.is_mapped()) {
                let _ = taking.union_rectangle(&cairo::RectangleInt::new(b.x().floor() as i32, b.y().floor() as i32, b.width().ceil() as i32, b.height().ceil() as i32));
            }
        }
        taking
    }

    fn tick(self: &Rc<Self>) {
        let mut changed = false;
        for event in self.events.try_iter() {
            self.ui.borrow_mut().shown.take(&event);
            changed = true;
        }
        changed |= self.ui.borrow_mut().shown.expire(Instant::now());
        if changed {
            self.render();
        }
        let mut ui = self.ui.borrow_mut();
        ui.game = ui.game.take().or_else(|| self.screen.window_of(self.pid));
        ui.placed = ui.game.as_ref().and_then(|window| self.screen.placement(window)).filter(|_| self.asking.get() || self.screen.active_is(self.pid));
        if ui.placed.is_none() && !ui.picking {
            // the window may have been closed and opened anew
            ui.game = None;
        }
        let begin = ui.reading && ui.placed.is_some() && !ui.picking;
        if begin {
            ui.reading = false;
        }
        // text selected in the strip goes to the clipboard once the selection rests
        let selection = self.japanese.selection_bounds().filter(|(from, to)| from != to);
        let copy = selection.filter(|s| ui.look.copy && ui.selection == Some(*s) && ui.copied != Some(*s));
        ui.selection = selection;
        if let Some((from, to)) = copy {
            ui.copied = copy;
            let selected: String = self.japanese.text().chars().skip(from as usize).take((to - from) as usize).collect();
            let _ = Command::new("wl-copy").arg("--").arg(selected).spawn();
        }
        self.look_up(&mut ui);
        drop(ui);
        // who translates may have changed behind the overlay's back
        let translating = translate::at_work().map(|service| {
            let google = self.ui.borrow().shown.by_google() && service != Service::Google;
            if google { "Google (tạm)".to_string() } else { service.known().label.to_string() }
        });
        self.service.set_visible(translating.is_some());
        if translating.as_deref() != self.service.label().as_deref() {
            self.service.set_label(translating.as_deref().unwrap_or_default());
        }
        if begin {
            self.read_pictures();
        }
        self.place();
        let ui = self.ui.borrow();
        self.speech.suspend(ui.placed.is_none() || ui.shown.old().is_some());
        drop(ui);
        let status = self.speech.status();
        let busy = status.busy();
        self.listen.set_icon_name(if busy { "media-playback-stop-symbolic" } else { "audio-volume-high-symbolic" });
        self.listen.set_tooltip_text(Some(if busy { "Dừng âm thanh và bỏ câu đang chờ" } else { "Nghe câu Nhật đang hiện (dù dải chỉ hiện bản dịch)" }));
        switch(&self.listen, busy);
        let text = if status.phase == tts::Phase::Idle && self.speech.settings().automatic {
            if self.speech.paused() { "Tự đọc: tạm ngưng" } else { "Tự đọc: bật" }
        } else { &status.text };
        if self.speech_status.text() != text { self.speech_status.set_text(text); }
        self.speech_status.set_visible(status.phase != tts::Phase::Idle || self.speech.settings().automatic);
        self.speech_status.set_tooltip_text(Some(&status.text));
    }

    /// Looks up the word under the pointer, and shows what was found by the
    /// strip, the word marked in the message.
    fn look_up(self: &Rc<Self>, ui: &mut Ui) {
        if ui.look.hidden { return; }
        // The meaning stays while the pointer is on its word or on the
        // meaning itself, and a moment longer: the way from one to the
        // other leads over neither.
        let at = ui.hover.map(|h| h.0);
        ui.away = if at.is_some() || ui.over_meaning { 0 } else { ui.away + 1 };
        if let Some(start) = at.filter(|_| at != ui.asked) {
            ui.asked = at;
            ui.lookups += 1;
            let text = self.japanese.text();
            let from: String = text.get(start..).unwrap_or_default().chars().take_while(|c| *c != '\n').take(LOOKED_UP).collect();
            let _ = self.asks.send((ui.lookups, from));
        }
        if ui.away > LINGERS && ui.asked.is_some() {
            ui.asked = None;
            ui.lookups += 1;
            self.japanese.set_attributes(None);
            self.popup.set_visible(false);
        }
        // only the answer to the last question matters
        let Some((_, answer)) = self.answers.try_iter().filter(|a| a.0 == ui.lookups).last() else { return };
        let (Some(at), Some((_, x))) = (ui.asked, ui.hover) else { return };
        while let Some(old) = self.meaning.first_child() {
            self.meaning.remove(&old);
        }
        let said = |class: &str, markup: &str| {
            let label = label(class);
            label.set_markup(markup);
            label
        };
        match answer {
            Ok(found) if found.entries.is_empty() => {
                self.japanese.set_attributes(None);
                self.popup.set_visible(false);
                return;
            }
            Ok(found) => {
                let marks = gtk4::pango::AttrList::new();
                let mut mark = gtk4::pango::AttrColor::new_background(0x5a5a, 0x4a4a, 0x1a1a);
                mark.set_start_index(at as u32);
                mark.set_end_index((at + found.length) as u32);
                marks.insert(mark);
                self.japanese.set_attributes(Some(&marks));
                for (nth, entry) in found.entries.iter().enumerate() {
                    if nth > 0 {
                        self.meaning.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
                    }
                    self.meaning.append(&word(entry));
                }
            }
            Err(Trouble::NoDictionary) => {
                self.meaning.append(&said("gloss", "<span foreground='#e0a458'>Chưa có từ điển</span>\nTải một từ điển của Yomitan (file zip) rồi thêm vào tool."));
                let open = gtk4::Button::with_label("Mở tab Từ điển");
                open.set_halign(gtk4::Align::Start);
                open.set_focusable(false);
                let overlay = self.clone();
                open.connect_clicked(move |_| overlay.set_up(Tab::Dictionary));
                self.meaning.append(&open);
            }
            Err(Trouble::Yomitan(e)) => {
                eprintln!("\x1b[33mYomitan: {e}\x1b[0m");
                let how = "Mở Firefox, vào Yomitan Settings, bật Advanced ở góc dưới trái, rồi bật General → Enable Yomitan API. Hoặc dùng từ điển nhập vào tool.";
                self.meaning.append(&said("gloss", &format!("<span foreground='#e0a458'>Chưa hỏi được Yomitan</span>\n{how}")));
                let open = gtk4::Button::with_label("Mở tab Từ điển");
                open.set_halign(gtk4::Align::Start);
                open.set_focusable(false);
                let overlay = self.clone();
                open.connect_clicked(move |_| overlay.set_up(Tab::Dictionary));
                self.meaning.append(&open);
            }
            Err(Trouble::Store(e)) => {
                self.meaning.append(&said("gloss", &format!("<span foreground='#e0a458'>Chưa tra được trong kho từ điển</span>\n{}", yomitan::escape(&e))));
            }
        }
        self.scrolled.vadjustment().set_value(0.0);
        // under the word, or above the strip when there is no room below
        let tall = (self.meaning.measure(gtk4::Orientation::Vertical, MEANING).1 + 20).min(TALLEST);
        let bounds = self.strip.monitor().map(|m| m.geometry()).unwrap_or_else(|| gdk::Rectangle::new(0, 0, 1920, 1080));
        let below = ui.strip.1 + self.frame.height() + 2;
        let top = if below + tall <= bounds.y() + bounds.height() { below } else { ui.strip.1 - tall - 2 };
        // the least may never be above the most, so the most is let go of first
        self.scrolled.set_max_content_height(-1);
        self.scrolled.set_min_content_height(tall - 20);
        self.scrolled.set_max_content_height(tall - 20);
        self.popup.set_margin(Edge::Left, (ui.strip.0 + x - 24 - bounds.x()).clamp(0, (bounds.width() - MEANING - 30).max(0)));
        self.popup.set_margin(Edge::Top, (top - bounds.y()).max(0));
        self.popup.set_default_size(-1, -1);
        self.popup.set_visible(true);
    }

    /// The menu of the message's text, in place of the toolkit's own, most
    /// of which (cut, paste, delete) can do nothing to text that is only read.
    fn text_menu(self: &Rc<Self>, x: f64, y: f64) {
        let menu = gtk4::Popover::new();
        menu.add_css_class("vntext-menu");
        menu.set_has_arrow(false);
        menu.set_parent(&self.japanese);
        menu.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let selected = self.japanese.selection_bounds().filter(|(from, to)| from != to);
        let item = |text: &str, act: Box<dyn Fn(&Overlay)>| {
            let choice = gtk4::Button::with_label(text);
            choice.set_focusable(false);
            let (overlay, menu) = (self.clone(), menu.clone());
            choice.connect_clicked(move |_| {
                act(&overlay);
                menu.popdown();
            });
            list.append(&choice);
        };
        item(
            if selected.is_some() { "Chép phần đã chọn" } else { "Chép cả câu" },
            Box::new(move |o| {
                let text = o.japanese.text();
                let copied: String = match selected {
                    Some((from, to)) => text.chars().skip(from as usize).take((to - from) as usize).collect(),
                    None => text.to_string(),
                };
                let _ = Command::new("wl-copy").arg("--").arg(copied).spawn();
            }),
        );
        let speech = self.speech.clone();
        let japanese = self.japanese.text();
        let spoken = selected.map(|(from,to)| japanese.chars().skip(from as usize).take((to-from) as usize).collect::<String>()).unwrap_or_else(|| japanese.to_string());
        item(if selected.is_some() { "Nghe phần Nhật đã chọn" } else { "Nghe câu Nhật" }, Box::new(move |_| {
            speech.speak(&spoken);
        }));
        // a translation that came out wrong, or from a service since replaced
        if translate::at_work().is_some() && self.ui.borrow().shown.retranslatable() {
            item(
                "Dịch lại câu này",
                Box::new(|o| {
                    let asked = o.ui.borrow_mut().shown.retranslate();
                    if let Some((seq, index)) = asked {
                        translate::redo(seq, index);
                        o.render();
                    }
                }),
            );
        }
        item("Chọn tất cả", Box::new(|o| o.japanese.select_region(0, -1)));
        menu.set_child(Some(&list));
        menu.connect_closed(|menu| menu.unparent());
        menu.popup();
    }

    /// Less frequent actions stay out of the small toolbar.
    fn more_menu(self: &Rc<Self>) -> gtk4::Popover {
        let menu = gtk4::Popover::new();
        menu.add_css_class("vntext-menu");
        menu.set_position(gtk4::PositionType::Bottom);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        let action = |name: &str, act: Box<dyn Fn(&Rc<Overlay>)>| {
            let button = gtk4::Button::with_label(name);
            let (overlay, menu) = (self.clone(), menu.clone());
            button.connect_clicked(move |_| {
                menu.popdown();
                act(&overlay);
            });
            list.append(&button);
        };
        action("Chỉnh bố cục…", Box::new(|o| o.editing(true)));
        action("Chép câu đang hiện", Box::new(|o| {
            let text = {
                let ui = o.ui.borrow();
                ui.shown.copied(ui.look.mode)
            };
            if let Some(text) = text {
                let _ = Command::new("wl-copy").arg("--").arg(text).spawn();
                o.hub.publish(json!({"type": "toast", "text": "Đã chép câu đang hiện."}).to_string());
            }
        }));
        action("Đọc từ ảnh (OCR)…", Box::new(|o| o.set_up(Tab::Ocr)));
        action("Giọng đọc A.I.VOICE2…", Box::new(|o| o.set_up(Tab::Speech)));
        list.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
        action("Dừng theo dõi game", Box::new(|_| crate::leave(0)));
        menu.set_child(Some(&list));
        menu
    }

    /// Changes how the strip looks, keeps that for the game, and shows it.
    fn restyle(self: &Rc<Self>, change: impl FnOnce(&mut Look)) {
        change(&mut self.ui.borrow_mut().look);
        self.save();
        self.render();
        self.place();
    }

    /// Does what a hint or notice offers.
    fn act(self: &Rc<Self>, action: Action) {
        match action {
            Action::Google => {
                let (kind, told) = match translate::choose(Service::Google) {
                    Ok(()) => ("toast", "Từ câu sau dịch bằng Google Dịch.".to_string()),
                    Err(e) => ("notice", e),
                };
                self.hub.publish(json!({"type": kind, "text": told}).to_string());
            }
            Action::Translate => self.set_up(Tab::Translate),
            Action::Dictionary => self.set_up(Tab::Dictionary),
        }
    }

    /// The tab of how the strip looks: the size and colour of its text, how
    /// opaque it is, which of a message it shows, and where it is.
    fn display_page(self: &Rc<Self>) -> gtk4::Box {
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let saved = gtk4::Label::new(Some("Thay đổi hiển thị được áp dụng và tự lưu ngay."));
        saved.add_css_class("caption");
        saved.set_wrap(true);
        saved.set_xalign(0.0);
        page.append(&saved);
        let look = self.ui.borrow().look.clone();
        let row = |name: &str, widget: &gtk4::Widget| {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let named = gtk4::Label::new(Some(name));
            named.set_xalign(0.0);
            named.set_hexpand(true);
            row.append(&named);
            row.append(widget);
            page.append(&row);
        };
        let font = gtk4::SpinButton::with_range(*FONTS.start() as f64, *FONTS.end() as f64, 1.0);
        font.set_numeric(true);
        font.set_width_chars(3);
        font.set_alignment(0.5);
        font.set_value(look.font as f64);
        let overlay = self.clone();
        font.connect_value_changed(move |font| overlay.restyle(|l| l.font = font.value() as i32));
        row("Cỡ chữ", font.upcast_ref());

        let opacity = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, *OPACITIES.start(), *OPACITIES.end(), 0.05);
        opacity.set_value(look.opacity);
        opacity.set_width_request(180);
        let overlay = self.clone();
        opacity.connect_value_changed(move |opacity| overlay.restyle(|l| l.opacity = opacity.value().clamp(*OPACITIES.start(), *OPACITIES.end())));
        row("Độ đậm nền", opacity.upcast_ref());

        let colours = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let swatches: Rc<RefCell<Vec<(&str, gtk4::Button)>>> = Rc::default();
        for (at, colour) in COLOURS.into_iter().enumerate() {
            let swatch = gtk4::Button::new();
            // each swatch is in the colour it chooses
            swatch.add_css_class("swatch");
            swatch.add_css_class(&format!("swatch{at}"));
            swatch.set_focusable(false);
            switch(&swatch, look.colour == colour);
            let (overlay, all) = (self.clone(), swatches.clone());
            swatch.connect_clicked(move |_| {
                all.borrow().iter().for_each(|(c, b)| switch(b, *c == colour));
                overlay.restyle(|l| l.colour = colour.to_string());
            });
            colours.append(&swatch);
            swatches.borrow_mut().push((colour, swatch));
        }
        row("Màu chữ", colours.upcast_ref());

        let modes = gtk4::DropDown::from_strings(&Mode::ALL.map(|m| m.0.name()));
        modes.set_selected(Mode::ALL.iter().position(|m| m.0 == look.mode).unwrap_or(0) as u32);
        let overlay = self.clone();
        modes.connect_selected_notify(move |modes| overlay.restyle(|l| l.mode = Mode::ALL.get(modes.selected() as usize).map_or(Mode::Translation, |m| m.0)));
        row("Hiện", modes.upcast_ref());

        let layout = gtk4::Button::with_label("Chỉnh bố cục trên dải…");
        let overlay = self.clone();
        layout.connect_clicked(move |button| {
            if let Some(window) = button.root().and_downcast::<gtk4::Window>() { window.destroy(); }
            overlay.editing(true);
        });
        page.append(&layout);

        let copy = gtk4::Switch::new();
        copy.set_active(look.copy);
        let overlay = self.clone();
        copy.connect_active_notify(move |copy| overlay.restyle(|look| look.copy = copy.is_active()));
        row("Tự chép phần chữ đã chọn", copy.upcast_ref());

        // back to how the strip is for a game never set up: where it is, how wide, and how it looks
        let reset = gtk4::Button::with_label("Đặt lại mặc định");
        reset.set_tooltip_text(Some("Đặt lại vị trí, kích thước, cỡ chữ, độ đậm nền, màu, chế độ; tắt tự chép phần chọn"));
        reset.set_halign(gtk4::Align::Start);
        let (overlay, font, opacity, modes, all, copy) = (self.clone(), font.clone(), opacity.clone(), modes.clone(), swatches.clone(), copy.clone());
        reset.connect_clicked(move |_| {
            let fresh = Look::default();
            overlay.restyle(|l| *l = Look { hidden: l.hidden, ..fresh.clone() });
            // the widgets follow, which has each of them set its own part once more to the same
            font.set_value(fresh.font as f64);
            opacity.set_value(fresh.opacity);
            modes.set_selected(Mode::ALL.iter().position(|m| m.0 == fresh.mode).unwrap_or(0) as u32);
            copy.set_active(fresh.copy);
            all.borrow().iter().for_each(|(c, b)| switch(b, *c == fresh.colour));
        });
        page.append(&reset);
        page
    }

    /// The tab of reading the text box from pictures.
    fn ocr_page(self: &Rc<Self>, window: &gtk4::Window) -> gtk4::Box {
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let about = gtk4::Label::new(Some("OCR đọc khung thoại từ ảnh chụp game, cho game chưa có profile. Cần key Gemini (tab Dịch)."));
        about.set_xalign(0.0);
        about.set_wrap(true);
        about.set_max_width_chars(48);
        page.append(&about);
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let named = gtk4::Label::new(Some("Đọc bằng ảnh chụp"));
        named.set_xalign(0.0);
        named.set_hexpand(true);
        let on = gtk4::Switch::new();
        on.set_active(self.pictures.on());
        let (overlay, closing) = (self.clone(), window.clone());
        on.connect_active_notify(move |on| {
            if on.is_active() == overlay.pictures.on() {
                return;
            }
            if on.is_active() {
                // the box may have to be pointed out first, over the game
                closing.destroy();
                overlay.read_pictures();
            } else {
                overlay.pictures.stop();
            }
        });
        row.append(&named);
        row.append(&on);
        page.append(&row);
        let pick = gtk4::Button::with_label("Khoanh lại khung thoại");
        pick.set_halign(gtk4::Align::Start);
        let (overlay, closing) = (self.clone(), window.clone());
        pick.connect_clicked(move |_| {
            closing.destroy();
            overlay.pick(false);
        });
        page.append(&pick);
        page
    }

    /// Lists the services by the strip for the reader to choose who
    /// translates from here on. One that is not set up says what it lacks.
    fn choose_service(self: &Rc<Self>) {
        if self.pictures.on() {
            let told = "Khi OCR bật, Gemini đọc ảnh và dịch luôn trong một lần gọi, nên không đổi nguồn dịch được.";
            return self.hub.publish(json!({"type": "notice", "text": told}).to_string());
        }
        let menu = gtk4::Popover::new();
        menu.add_css_class("vntext-menu");
        menu.set_parent(&self.service);
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        for known in &translate::SERVICES {
            let ready = translate::setup(known.service);
            let lacking = known.service.keyed() && ready.key.is_empty();
            let choice = gtk4::Button::with_label(&if lacking { format!("{} (chưa có key)", known.label) } else { known.label.to_string() });
            choice.set_focusable(false);
            switch(&choice, translate::at_work() == Some(known.service));
            let (overlay, menu, service) = (self.clone(), menu.clone(), known.service);
            choice.connect_clicked(move |_| {
                let (kind, told) = match translate::choose(service) {
                    Ok(()) => ("toast", format!("Từ câu sau dịch bằng {}.", service.known().label)),
                    Err(e) => ("notice", e),
                };
                overlay.hub.publish(json!({"type": kind, "text": told}).to_string());
                menu.popdown();
            });
            list.append(&choice);
        }
        menu.set_child(Some(&list));
        menu.connect_closed(|menu| menu.unparent());
        menu.popup();
    }

    /// Has the text box read from pictures, asking first where it is if it
    /// was never pointed out.
    fn read_pictures(self: &Rc<Self>) {
        if ocr::saved_region(&self.game).is_some() {
            ocr::spawn(self.pid, self.settings.clone(), self.hub.clone(), self.pictures.clone());
        } else {
            self.pick(true);
        }
    }

    /// Dims the screen and lets the reader drag a rectangle around the
    /// game's text box. With `read`, pictures of it are read from then on.
    fn pick(self: &Rc<Self>, read: bool) {
        let Some((x, y, width, height)) = self.ui.borrow().placed else { return };
        self.ui.borrow_mut().picking = true;
        self.place();
        let window = floating("vntext-pick");
        for edge in [Edge::Bottom, Edge::Right] {
            window.set_anchor(edge, true);
        }
        let area = gtk4::DrawingArea::new();
        let hint = gtk4::Label::new(Some("Kéo chuột quanh khung thoại của game · chuột phải để huỷ"));
        hint.set_halign(gtk4::Align::Center);
        hint.set_valign(gtk4::Align::Start);
        hint.set_margin_top(24);
        hint.set_can_target(false);
        let layers = gtk4::Overlay::new();
        layers.set_child(Some(&area));
        layers.add_overlay(&hint);
        window.set_child(Some(&layers));

        // the corners dragged between, on screen
        let dragged: Rc<Cell<Option<(f64, f64, f64, f64)>>> = Rc::default();
        let drawn = dragged.clone();
        area.set_draw_func(move |_, cr, _, _| {
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.5);
            let _ = cr.paint();
            // the game's window stays a little lighter than the rest
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.25);
            cr.rectangle(x as f64, y as f64, width as f64, height as f64);
            let _ = cr.fill();
            if let Some((x0, y0, x1, y1)) = drawn.get() {
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
                cr.rectangle(x0, y0, x1 - x0, y1 - y0);
                let _ = cr.fill_preserve();
                cr.set_operator(cairo::Operator::Over);
                cr.set_source_rgb(0.88, 0.64, 0.35);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
            }
        });

        let done = {
            let (overlay, window) = (self.clone(), window.clone());
            Rc::new(move |region: Option<Region>| {
                window.destroy();
                overlay.ui.borrow_mut().picking = false;
                let Some(region) = region else { return };
                ocr::save_region(&overlay.game, region);
                // a reader at work goes on with the new box
                if read || overlay.pictures.on() {
                    ocr::spawn(overlay.pid, overlay.settings.clone(), overlay.hub.clone(), overlay.pictures.clone());
                }
            })
        };
        let drag = gtk4::GestureDrag::new();
        let (from, area_) = (dragged.clone(), area.clone());
        drag.connect_drag_begin(move |_, x0, y0| from.set(Some((x0, y0, x0, y0))));
        let to = dragged.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some((x0, y0, _, _)) = to.get() {
                to.set(Some((x0, y0, x0 + dx, y0 + dy)));
                area_.queue_draw();
            }
        });
        let finish = done.clone();
        drag.connect_drag_end(move |_, _, _| {
            let Some((x0, y0, x1, y1)) = dragged.get() else { return finish(None) };
            let across = |a: f64, b: f64, from: i32, size: i32| {
                let share = |v: f64| ((v - from as f64) / size as f64).clamp(0.0, 1.0);
                (share(a.min(b)), share(a.max(b)))
            };
            let ((left, right), (top, bottom)) = (across(x0, x1, x, width), across(y0, y1, y, height));
            let meant = right - left >= SMALLEST_BOX && bottom - top >= SMALLEST_BOX;
            finish(meant.then_some(Region { left, top, right, bottom }));
        });
        area.add_controller(drag);
        let cancel = gtk4::GestureClick::new();
        cancel.set_button(gdk::BUTTON_SECONDARY);
        cancel.connect_pressed(move |_, _, _, _| done(None));
        area.add_controller(cancel);
        window.set_visible(true);
    }

    /// Layout editing is temporary; only the resulting look is saved.
    fn editing(self: &Rc<Self>, enabled: bool) {
        let mut ui = self.ui.borrow_mut();
        ui.locked = !enabled;
        if enabled {
            ui.look.hidden = false;
            if let Some((x, y, _, _)) = ui.placed {
                ui.look.place = Some((ui.strip.0 - x, ui.strip.1 - y, ui.strip.2));
            }
        }
        drop(ui);
        self.render();
        self.place();
        if !enabled { self.save(); }
    }

    /// Raw event positions use the stationary native surface, never the moving grip.
    fn dragging(self: &Rc<Self>, grip: &gtk4::Widget, edges: (i32, i32)) {
        let drag = gtk4::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        let origin = Rc::new(Cell::new(None));
        let (overlay, widget, begun) = (self.clone(), grip.clone(), origin.clone());
        drag.connect_drag_begin(move |gesture, x, y| {
            begun.set(None);
            let ui = overlay.ui.borrow();
            if ui.locked || ui.look.hidden {
                gesture.set_state(gtk4::EventSequenceState::Denied);
                return;
            }
            let pointer = gesture.current_event().and_then(|e| e.position()).or_else(|| {
                widget.compute_point(&overlay.strip, &gtk4::graphene::Point::new(x as f32, y as f32)).map(|p| (p.x() as f64, p.y() as f64))
            });
            let Some(pointer) = pointer else { return };
            begun.set(Some((pointer, (ui.strip.0, ui.strip.1, overlay.frame.width(), overlay.frame.height()), (x, y), false)));
        });
        let (overlay, widget, begun) = (self.clone(), grip.clone(), origin.clone());
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some((pointer, rect, local, started)) = begun.get() else { return };
            let mut ui = overlay.ui.borrow_mut();
            if ui.locked { return; }
            let Some((gx, gy, _, _)) = ui.placed else { return };
            let point = gesture.current_event().and_then(|e| e.position()).or_else(|| {
                widget.compute_point(&overlay.strip, &gtk4::graphene::Point::new((local.0 + dx) as f32, (local.1 + dy) as f32)).map(|p| (p.x() as f64, p.y() as f64))
            });
            let Some(point) = point else { return };
            let delta = ((point.0 - pointer.0).round() as i32, (point.1 - pointer.1).round() as i32);
            if !started && delta.0.abs().max(delta.1.abs()) < 4 { return; }
            begun.set(Some((pointer, rect, local, true)));
            if edges == (0, 0) {
                ui.look.place = Some((rect.0 + delta.0 - gx, rect.1 + delta.1 - gy, rect.2));
            } else {
                let bounds = overlay.strip.monitor().unwrap().geometry();
                // The requested size is the current rectangle, not a minimum for resizing.
                overlay.content.set_size_request(-1, -1);
                let minimum = (overlay.header.measure(gtk4::Orientation::Horizontal, -1).0.max(overlay.editor.measure(gtk4::Orientation::Horizontal, -1).0) + PADDING,
                               ui.chrome_height + 48);
                let next = resized(rect, delta, edges, (minimum.0.max(NARROWEST), minimum.1.max(SHORTEST)),
                                   (bounds.x(), bounds.y(), bounds.width(), bounds.height()));
                ui.look.place = Some((next.0 - gx, next.1 - gy, next.2));
                if edges.1 != 0 { ui.look.height = Some(next.3); }
            }
            drop(ui);
            overlay.render();
            overlay.place();
        });
        let (overlay, begun) = (self.clone(), origin.clone());
        drag.connect_drag_end(move |_, _, _| { if begun.take().is_some_and(|(_, _, _, moved)| moved) { overlay.save(); } });
        drag.connect_cancel(move |_, _| origin.set(None));
        grip.add_controller(drag);
    }

    /// The collapsed icon moves independently; claiming a drag cancels the button click.
    fn icon_drag(self: &Rc<Self>) {
        let drag = gtk4::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let origin = Rc::new(Cell::new(None));
        let (overlay, begun) = (self.clone(), origin.clone());
        drag.connect_drag_begin(move |gesture, x, y| {
            begun.set(None);
            overlay.icon_moved.set(false);
            if !overlay.ui.borrow().look.hidden {
                gesture.set_state(gtk4::EventSequenceState::Denied);
                return;
            }
            let p = gesture.current_event().and_then(|e| e.position()).or_else(|| {
                overlay.restore.compute_point(&overlay.strip, &gtk4::graphene::Point::new(x as f32, y as f32)).map(|p| (p.x() as f64, p.y() as f64))
            });
            if let Some(p) = p { begun.set(Some((p, overlay.ui.borrow().icon_at, (x, y)))); }
        });
        let (overlay, begun) = (self.clone(), origin.clone());
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some((pointer, at, local)) = begun.get() else { return };
            let p = gesture.current_event().and_then(|e| e.position()).or_else(|| {
                overlay.restore.compute_point(&overlay.strip, &gtk4::graphene::Point::new((local.0 + dx) as f32, (local.1 + dy) as f32)).map(|p| (p.x() as f64, p.y() as f64))
            });
            let Some(p) = p else { return };
            let delta = ((p.0 - pointer.0).round() as i32, (p.1 - pointer.1).round() as i32);
            if !overlay.icon_moved.get() && delta.0.abs().max(delta.1.abs()) < 4 { return; }
            overlay.icon_moved.set(true);
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let mut ui = overlay.ui.borrow_mut();
            if !ui.look.hidden { return; }
            let Some((gx, gy, _, _)) = ui.placed else { return };
            let bounds = overlay.strip.monitor().unwrap().geometry();
            let x = (at.0 + delta.0).clamp(bounds.x(), bounds.x() + (bounds.width() - overlay.frame.width()).max(0));
            let y = (at.1 + delta.1).clamp(bounds.y(), bounds.y() + (bounds.height() - overlay.frame.height()).max(0));
            ui.look.icon = Some((x - gx, y - gy));
            drop(ui);
            overlay.place();
        });
        let (overlay, begun) = (self.clone(), origin.clone());
        drag.connect_drag_end(move |_, _, _| {
            if begun.take().is_some() && overlay.icon_moved.get() { overlay.save(); }
        });
        drag.connect_cancel(move |_, _| origin.set(None));
        self.restore.add_controller(drag);
    }

    /// A button of the bar that changes the state and shows it anew.
    fn button(self: &Rc<Self>, icon: &str, tip: &str, act: impl Fn(&Rc<Overlay>) + 'static) -> gtk4::Button {
        let button = pictured(icon, tip);
        self.acting(&button, act);
        button
    }

    fn speech_page(self: &Rc<Self>, window: &gtk4::Window) -> gtk4::Box {
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        let about = label("caption");
        about.set_text("Nghe câu Nhật gốc bằng A.I.VOICE2. Tự đọc mặc định tắt để tránh chồng tiếng game.");
        page.append(&about);
        let scope = label("caption");
        scope.set_text(if self.speech.key().contains("/session-") {
            "Chưa đọc được mã game: thiết lập chỉ dùng cho phiên Ryujinx này."
        } else if self.speech.key().starts_with("Ryujinx/") {
            "Thiết lập giọng đọc tự lưu riêng cho game hiện tại trên Ryujinx."
        } else { "Thiết lập giọng đọc tự lưu cho game này." });
        page.append(&scope);
        let connection = label("caption");
        connection.set_text("Mở tab để kiểm tra kết nối và tải giọng.");
        page.append(&connection);
        let voices = gtk4::DropDown::from_strings(&[]);
        voices.set_enable_search(true);
        voices.set_sensitive(false);
        let refresh = gtk4::Button::with_label("Làm mới giọng");
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        voices.set_hexpand(true);
        row.append(&voices);
        row.append(&refresh);
        page.append(&row);
        let automatic = gtk4::CheckButton::with_label("Tự đọc câu Nhật mới");
        automatic.set_active(self.speech.settings().automatic);
        page.append(&automatic);
        let preparing = gtk4::CheckButton::with_label("Chuẩn bị audio trước khi nghe");
        preparing.set_active(self.speech.settings().prepare);
        preparing.set_tooltip_text(Some("Chuẩn bị câu đang hiện; sau khi nghe, tải trước một câu ngắn kế tiếp nếu có kịch bản. Tác vụ nghe được ưu tiên."));
        page.append(&preparing);
        let volume = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 5.0);
        volume.set_value(self.speech.settings().volume * 100.0);
        volume.set_digits(0);
        volume.set_hexpand(true);
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.append(&gtk4::Label::new(Some("Âm lượng TTS (%)")));
        row.append(&volume);
        page.append(&row);
        let sample = gtk4::Button::with_label("Nghe thử");
        sample.set_sensitive(false);
        let stop = gtk4::Button::with_label("Dừng âm thanh");
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.append(&sample);
        row.append(&stop);
        page.append(&row);
        let status = label("caption");
        status.set_text(&self.speech.status().text);
        page.append(&status);
        let advanced = gtk4::Expander::new(Some("Kết nối nâng cao"));
        let address = gtk4::Entry::new();
        address.set_text(&self.speech.settings().endpoint);
        address.set_placeholder_text(Some("http://127.0.0.1:17862"));
        let details = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        details.append(&gtk4::Label::new(Some("Địa chỉ dịch vụ cục bộ · bấm Làm mới giọng để áp dụng")));
        details.append(&address);
        advanced.set_child(Some(&details));
        page.append(&advanced);
        let help = label("caption");
        help.set_text("Giọng, âm lượng và tự đọc tự lưu. Câu mới dừng tiếng cũ; xem lịch sử hoặc chuyển ứng dụng tạm ngưng tự đọc. Không tự phát lại câu đã bỏ qua.");
        page.append(&help);

        let loaded: Rc<RefCell<Vec<tts::Voice>>> = Rc::default();
        let loading = Rc::new(Cell::new(false));
        let generation = Rc::new(Cell::new(0u64));
        let reload = {
            let (speech, address, connection, voices, sample, loaded, loading, generation, window) =
                (self.speech.clone(), address.clone(), connection.clone(), voices.clone(), sample.clone(), loaded.clone(), loading.clone(), generation.clone(), window.downgrade());
            Rc::new(move || {
                let mut settings = speech.settings();
                settings.endpoint = address.text().to_string();
                if settings.endpoint.trim().trim_end_matches('/') != speech.settings().endpoint {
                    if let Err(error) = speech.apply(settings.clone()) { connection.set_text(&error); return; }
                }
                generation.set(generation.get() + 1);
                let request = generation.get();
                connection.set_text("Đang kết nối A.I.VOICE2…");
                sample.set_sensitive(false);
                voices.set_sensitive(false);
                let (answer, answers) = mpsc::channel();
                thread::spawn(move || { let _ = answer.send(tts::catalog(&settings.endpoint)); });
                let (speech, connection, voices, sample, loaded, loading, generation, window) =
                    (speech.clone(), connection.clone(), voices.clone(), sample.clone(), loaded.clone(), loading.clone(), generation.clone(), window.clone());
                glib::timeout_add_local(TICK, move || {
                    if generation.get() != request || window.upgrade().is_none_or(|window| !window.is_visible()) { return glib::ControlFlow::Break; }
                    let catalog = match answers.try_recv() {
                        Ok(catalog) => catalog,
                        Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                        Err(_) => return glib::ControlFlow::Break,
                    };
                    match catalog {
                        Err(error) => connection.set_text(&error),
                        Ok(catalog) => {
                            let mut settings = speech.settings();
                            if settings.voice.is_empty() {
                                settings.voice = catalog.voices.iter().find(|voice| voice.id == catalog.default).unwrap_or(&catalog.voices[0]).id.clone();
                                if let Err(error) = speech.apply(settings.clone()) { connection.set_text(&error); return glib::ControlFlow::Break; }
                            }
                            let selected = catalog.voices.iter().position(|voice| voice.id == settings.voice);
                            let names: Vec<&str> = catalog.voices.iter().map(|voice| voice.name.as_str()).collect();
                            loading.set(true);
                            voices.set_model(Some(&gtk4::StringList::new(&names)));
                            *loaded.borrow_mut() = catalog.voices;
                            voices.set_selected(selected.map_or(gtk4::INVALID_LIST_POSITION, |at| at as u32));
                            loading.set(false);
                            voices.set_sensitive(true);
                            sample.set_sensitive(selected.is_some());
                            connection.set_text(if selected.is_some() { "A.I.VOICE2 đã sẵn sàng." } else { "Giọng đã lưu không còn được cài. Chọn lại một giọng bên dưới." });
                        }
                    }
                    glib::ControlFlow::Break
                });
            })
        };
        let reload_shown = reload.clone();
        page.connect_map(move |_| reload_shown());
        refresh.connect_clicked(move |_| reload());
        let (speech, loaded_, loading_, sample_, connection_) = (self.speech.clone(), loaded.clone(), loading.clone(), sample.clone(), connection.clone());
        voices.connect_selected_notify(move |voices| {
            if loading_.get() { return; }
            let Some(voice) = loaded_.borrow().get(voices.selected() as usize).cloned() else { return };
            let mut settings = speech.settings(); settings.voice = voice.id;
            match speech.apply(settings) {
                Ok(()) => { sample_.set_sensitive(true); connection_.set_text("Đã chọn giọng. Thay đổi được tự lưu."); }
                Err(error) => connection_.set_text(&error),
            }
        });
        let (speech, connection) = (self.speech.clone(), connection.clone());
        automatic.connect_toggled(move |automatic| {
            let mut settings = speech.settings();
            if automatic.is_active() && !loaded.borrow().iter().any(|voice| voice.id == settings.voice) {
                automatic.set_active(false);
                connection.set_text("Kết nối và chọn giọng trước khi bật tự đọc.");
                return;
            }
            settings.automatic = automatic.is_active();
            if let Err(error) = speech.apply(settings) { connection.set_text(&error); }
        });
        let (speech, connection) = (self.speech.clone(), status.clone());
        preparing.connect_toggled(move |preparing| {
            let mut settings = speech.settings(); settings.prepare = preparing.is_active();
            if let Err(error) = speech.apply(settings) { connection.set_text(&error); }
        });
        let speech = self.speech.clone();
        volume.connect_value_changed(move |volume| {
            let mut settings = speech.settings(); settings.volume = volume.value() / 100.0;
            if let Err(error) = speech.apply(settings) { speech.error(&error); }
        });
        let speech = self.speech.clone();
        sample.connect_clicked(move |_| speech.sample());
        let speech = self.speech.clone();
        stop.connect_clicked(move |_| speech.stop());
        let (speech, window) = (self.speech.clone(), window.downgrade());
        glib::timeout_add_local(TICK, move || {
            if window.upgrade().is_none_or(|window| !window.is_visible()) { return glib::ControlFlow::Break; }
            let text = speech.status().text;
            if status.text() != text { status.set_text(&text); }
            glib::ControlFlow::Continue
        });
        page
    }

    /// The settings of the strip, in a small window over the game: who
    /// translates, set up on the first tab (the service shown when it is
    /// saved is the one that translates from then on), and the dictionaries.
    fn set_up(self: &Rc<Self>, tab: Tab) {
        let Some((x, y, width, height)) = self.ui.borrow().placed else { return };
        if self.asking.get() {
            return;
        }
        let window = floating("vntext-ask");
        // Kept by whether the window is on screen, not by its end: what is
        // connected to the window holds on to it, so it is never told that
        // it was destroyed, only that it is no longer shown.
        let overlay = self.clone();
        window.connect_map(move |_| overlay.asking.set(true));
        let overlay = self.clone();
        window.connect_unmap(move |_| overlay.asking.set(false));
        // typing needs the keyboard, which a floating surface otherwise never takes
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        let tabs = gtk4::FlowBox::new();
        tabs.set_selection_mode(gtk4::SelectionMode::None);
        tabs.set_column_spacing(6);
        tabs.set_row_spacing(6);
        tabs.set_max_children_per_line(3);
        let asked = gtk4::Label::new(Some("Chọn nguồn dịch, chỉnh cấu hình rồi bấm Lưu cấu hình dịch."));
        asked.set_xalign(0.0);
        asked.set_wrap(true);
        content.append(&asked);
        content.append(&tabs);
        // a row for each thing a service may want
        let row = |name: &str, entry: &gtk4::Widget| {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let named = gtk4::Label::new(Some(name));
            named.set_xalign(0.0);
            named.set_width_request(52);
            entry.set_hexpand(true);
            row.append(&named);
            row.append(entry);
            content.append(&row);
            row
        };
        let (key, model, base) = (gtk4::PasswordEntry::new(), gtk4::Entry::new(), gtk4::Entry::new());
        key.set_show_peek_icon(true);
        key.set_hexpand(true);
        // the model can be typed, or picked from the list the service gives
        let (naming, list) = (gtk4::Box::new(gtk4::Orientation::Horizontal, 4), gtk4::Button::with_label("Danh sách"));
        list.add_css_class("tab");
        list.set_tooltip_text(Some("Tải danh sách model của nguồn này (cần key)"));
        model.set_hexpand(true);
        naming.append(&model);
        naming.append(&list);
        let rows = [row("API key", key.upcast_ref()), row("Model", naming.upcast_ref()), row("Địa chỉ", base.upcast_ref())];
        let offered = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let offers = gtk4::ScrolledWindow::new();
        offers.set_child(Some(&offered));
        offers.set_min_content_height(150);
        offers.set_visible(false);
        content.append(&offers);
        let note = gtk4::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_max_width_chars(48);
        content.append(&note);
        let save = gtk4::Button::with_label("Lưu cấu hình dịch");
        save.add_css_class("suggested-action");
        save.set_halign(gtk4::Align::End);
        content.append(&save);
        let pages = gtk4::Stack::new();
        pages.add_named(&self.display_page(), Some(Tab::Display.id()));
        pages.add_named(&content, Some(Tab::Translate.id()));
        pages.add_named(&dictionaries(&window), Some(Tab::Dictionary.id()));
        pages.add_named(&self.ocr_page(&window), Some(Tab::Ocr.id()));
        pages.add_named(&self.speech_page(&window), Some(Tab::Speech.id()));
        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
        outer.add_css_class("box");
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        let title = gtk4::Label::new(Some("Thiết lập vntext"));
        title.add_css_class("heading");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        top.append(&title);
        let close = pictured("window-close-symbolic", "Đóng thiết lập (Esc)");
        close.add_css_class("icon");
        let closing = window.clone();
        close.connect_clicked(move |_| closing.destroy());
        top.append(&close);
        outer.append(&top);
        let tabbed = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let buttons: Rc<RefCell<Vec<(Tab, gtk4::Button)>>> = Rc::default();
        for (this, id, name) in Tab::ALL {
            let button = gtk4::Button::with_label(name);
            button.add_css_class("tab");
            button.set_focusable(false);
            let (pages, buttons_) = (pages.clone(), buttons.clone());
            button.connect_clicked(move |_| {
                pages.set_visible_child_name(id);
                buttons_.borrow().iter().for_each(|(t, b)| switch(b, *t == this));
            });
            tabbed.append(&button);
            buttons.borrow_mut().push((this, button));
        }
        outer.append(&tabbed);
        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&pages));
        let monitor = self.display.monitors().into_iter().flatten()
            .filter_map(|item| item.downcast::<gdk::Monitor>().ok())
            .find(|monitor| monitor.geometry().contains_point(x + width / 2, y + height / 2));
        let geometry = monitor.as_ref().map(|monitor| monitor.geometry()).unwrap_or(gdk::Rectangle::new(0, 0, 1024, 768));
        let wide = 560.min((geometry.width() - 32).max(240));
        let tall = 640.min((geometry.height() - 32).max(240));
        scroll.set_min_content_width(wide - 36);
        scroll.set_max_content_height(tall - 140);
        window.set_monitor(monitor.as_ref());
        outer.append(&scroll);
        let help = gtk4::Label::new(Some("Esc để đóng và trở lại game"));
        help.add_css_class("caption");
        help.set_xalign(0.0);
        outer.append(&help);
        window.set_child(Some(&outer));
        pages.set_visible_child_name(tab.id());
        buttons.borrow().iter().for_each(|(t, b)| switch(b, *t == tab));
        window.set_margin(Edge::Left, (x - geometry.x() + width / 2 - wide / 2).clamp(16, (geometry.width() - wide - 16).max(16)));
        window.set_margin(Edge::Top, (y - geometry.y() + height / 2 - tall / 2).clamp(16, (geometry.height() - tall - 16).max(16)));

        let shown = Rc::new(Cell::new(translate::chosen()));
        let chosen: Rc<RefCell<Vec<(Service, gtk4::Button)>>> = Rc::default();
        // puts into the rows what is set up for a service
        let show = {
            let (shown, chosen, key, model, base, note, offers) = (shown.clone(), chosen.clone(), key.clone(), model.clone(), base.clone(), note.clone(), offers.clone());
            Rc::new(move |service: Service| {
                shown.set(service);
                // the list is another service's
                offers.set_visible(false);
                let setup = translate::setup(service);
                key.set_text(&setup.key);
                model.set_text(&setup.model);
                model.set_placeholder_text(Some(service.known().model));
                base.set_text(&setup.base);
                let chat = matches!(service, Service::OpenAi | Service::DeepSeek | Service::Local);
                for (row, wanted) in rows.iter().zip([service != Service::Google, service != Service::Google, chat]) {
                    row.set_visible(wanted);
                }
                note.set_text(match service {
                    Service::Gemini => "Để trống model thì dùng Flash-Lite mới nhất. Key này cũng dùng cho OCR.",
                    Service::OpenAi => "Phải ghi tên model.",
                    Service::DeepSeek => "",
                    Service::Claude => "Mặc định claude-opus-5-5; claude-haiku-5-5 rẻ và nhanh hơn nhiều.",
                    Service::Local => "Server nói kiểu OpenAI (Ollama, LM Studio...). Phải ghi tên model; key có thể để trống.",
                    Service::Google => "Không cần key. Dịch từng câu rời, không có ngữ cảnh, kém các model.",
                });
                note.set_visible(!note.text().is_empty());
                for (tab, button) in chosen.borrow().iter() {
                    switch(button, *tab == service);
                }
            })
        };
        for known in &translate::SERVICES {
            let tab = gtk4::Button::with_label(known.label);
            tab.add_css_class("tab");
            let (show, service) = (show.clone(), known.service);
            tab.connect_clicked(move |_| show(service));
            tabs.insert(&tab, -1);
            chosen.borrow_mut().push((known.service, tab));
        }
        show(shown.get());

        // The list is asked of the service off this thread, with the key and
        // the address as they stand in the rows, saved or not.
        let listing = (shown.clone(), key.clone(), model.clone(), base.clone(), note.clone(), offers.clone());
        list.connect_clicked(move |_| {
            let (shown, key, model, base, note, offers) = listing.clone();
            let setup = translate::Setup { service: shown.get(), key: key.text().to_string(), model: String::new(), base: base.text().to_string() };
            let (answer, answers) = mpsc::channel();
            thread::spawn(move || answer.send(translate::models(&setup)));
            note.set_text("Đang tải danh sách model...");
            note.set_visible(true);
            let offered = offered.clone();
            glib::timeout_add_local(TICK, move || {
                let names = match answers.try_recv() {
                    Ok(names) => names,
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                };
                while let Some(old) = offered.first_child() {
                    offered.remove(&old);
                }
                let names = names.unwrap_or_else(|e| {
                    note.set_text(&format!("Không tải được danh sách model: {e}"));
                    Vec::new()
                });
                if !names.is_empty() {
                    note.set_text(&format!("{} model. Bấm một tên để chọn.", names.len()));
                }
                for name in &names {
                    let pick = gtk4::Button::with_label(name);
                    pick.add_css_class("tab");
                    pick.set_halign(gtk4::Align::Start);
                    let (model, name) = (model.clone(), name.clone());
                    pick.connect_clicked(move |_| model.set_text(&name));
                    offered.append(&pick);
                }
                offers.set_visible(!names.is_empty());
                glib::ControlFlow::Break
            });
        });

        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let closing = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            closing.destroy();
            glib::Propagation::Stop
        });
        window.add_controller(keys);
        let keep = {
            let (overlay, window, key, model, base) = (self.clone(), window.clone(), key.clone(), model.clone(), base.clone());
            move || {
                let setup = translate::Setup { service: shown.get(), key: key.text().to_string(), model: model.text().to_string(), base: base.text().to_string() };
                let (kind, told) = match translate::save_setup(&setup) {
                    Ok(()) => ("toast", format!("Đã lưu: dịch bằng {}.", setup.service.known().label)),
                    Err(e) => ("notice", format!("Không lưu được thiết lập dịch: {e}")),
                };
                overlay.hub.publish(json!({"type": kind, "text": told}).to_string());
                window.destroy();
            }
        };
        for entry in [key.upcast_ref::<gtk4::Widget>(), model.upcast_ref(), base.upcast_ref()] {
            let entered = keep.clone();
            // an entry of either kind says so when Enter is pressed in it
            if let Some(entry) = entry.downcast_ref::<gtk4::Entry>() {
                entry.connect_activate(move |_| entered());
            } else if let Some(entry) = entry.downcast_ref::<gtk4::PasswordEntry>() {
                entry.connect_activate(move |_| entered());
            }
        }
        save.connect_clicked(move |_| keep());
        window.set_visible(true);
        if tab == Tab::Translate && key.is_visible() {
            key.grab_focus();
        }
    }

    fn acting(self: &Rc<Self>, button: &gtk4::Button, act: impl Fn(&Rc<Overlay>) + 'static) {
        let overlay = self.clone();
        button.connect_clicked(move |_| {
            act(&overlay);
            overlay.render();
            overlay.place();
        });
    }
}

fn run(events: Receiver<String>, hub: web::Hub, pid: u32, pictures: Arc<ocr::Control>, settings: ocr::Settings, reading: bool, speech: tts::Control) -> Result<(), String> {
    let overlay = build(screen::open()?, events, hub, pid, pictures, settings, reading, speech)?;
    glib::timeout_add_local(TICK, move || {
        overlay.tick();
        glib::ControlFlow::Continue
    });
    glib::MainLoop::new(None, false).run();
    Ok(())
}

fn build(screen: Box<dyn Screen>, events: Receiver<String>, hub: web::Hub, pid: u32, pictures: Arc<ocr::Control>, settings: ocr::Settings, reading: bool, speech: tts::Control) -> Result<Rc<Overlay>, String> {
    gtk4::init().map_err(|e| e.to_string())?;
    if !gtk4_layer_shell::is_supported() {
        return Err("the compositor has no layer shell".to_string());
    }
    let display = gdk::Display::default().ok_or("no display")?;
    let style = gtk4::CssProvider::new();
    let swatches: String = COLOURS.iter().enumerate().map(|(at, c)| format!(".vntext-ask .box button.swatch{at} {{ background: {c}; }}\n")).collect();
    style.load_from_data(&format!("{STYLE}{swatches}"));
    let look_style = gtk4::CssProvider::new();
    let sizes = gtk4::CssProvider::new();
    gtk4::style_context_add_provider_for_display(&display, &sizes, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    gtk4::style_context_add_provider_for_display(&display, &style, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    gtk4::style_context_add_provider_for_display(&display, &look_style, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

    // The header, message and footer move as one layer-shell surface.
    let strip = floating("vntext");
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.add_css_class("strip");
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("panel-header");
    let motion = gtk4::EventControllerMotion::new();
    let entered = header.clone();
    motion.connect_enter(move |_, _, _| entered.add_css_class("active"));
    let left = header.clone();
    motion.connect_leave(move |_| left.remove_css_class("active"));
    content.add_controller(motion);
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    let restore = gtk4::Button::new();
    // Reuse the tray's embedded artwork without depending on an installed icon theme.
    let pixels = glib::Bytes::from_static(include_bytes!("vntext-64.rgba"));
    let texture = gdk::MemoryTexture::new(64, 64, gdk::MemoryFormat::R8g8b8a8, &pixels, 64 * 4);
    let icon = gtk4::Image::from_paintable(Some(&texture));
    icon.set_pixel_size(32);
    restore.set_child(Some(&icon));
    restore.update_property(&[gtk4::accessible::Property::Label("vntext — Hiện phụ đề")]);
    restore.add_css_class("restore");
    restore.set_tooltip_text(Some("vntext — Bấm để hiện phụ đề; giữ và kéo để di chuyển"));
    restore.set_cursor_from_name(Some("grab"));
    restore.set_focusable(false);
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    text.set_hexpand(true);
    text.set_vexpand(true);
    let empty = label("caption");
    empty.set_text("Đang chờ câu thoại trong game…");
    text.append(&empty);
    let (speaker, japanese, translation) = (label("speaker"), label("japanese"), label("translation"));
    let passage = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    passage.append(&speaker);
    let lines = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    lines.set_hexpand(true);
    lines.append(&japanese);
    lines.append(&translation);
    let sentence = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let listen = pictured("audio-volume-high-symbolic", "Nghe câu Nhật đang hiện");
    listen.add_css_class("listen");
    listen.set_valign(gtk4::Align::Start);
    sentence.append(&lines);
    sentence.append(&listen);
    passage.append(&sentence);
    let text_scroll = gtk4::ScrolledWindow::new();
    text_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    text_scroll.set_propagate_natural_height(true);
    text_scroll.set_max_content_height(280);
    text_scroll.set_child(Some(&passage));
    text.append(&text_scroll);
    // what is said beside the message, with a button to do something about it
    let (hinted, hint) = (label("hinted"), gtk4::Button::new());
    hint.add_css_class("hint");
    hint.set_focusable(false);
    let hints = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hints.append(&hinted);
    hints.append(&hint);
    hinted.set_hexpand(true);
    let dismiss = pictured("window-close-symbolic", "Đóng thông báo");
    dismiss.add_css_class("hint");
    dismiss.add_css_class("dismiss");
    hints.append(&dismiss);
    text.append(&hints);
    let history = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    history.add_css_class("history");
    let history_summary = gtk4::Button::with_label("Lịch sử");
    history_summary.set_halign(gtk4::Align::Start);
    history_summary.set_focusable(false);
    history_summary.set_tooltip_text(Some("Mở câu trước/sau; có thể lăn chuột ở hàng này để xem lại"));
    let history_controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    history.append(&history_summary);
    history.append(&history_controls);
    let position = label("caption");
    position.set_hexpand(true);
    position.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let previous = pictured("go-previous-symbolic", "Câu trước");
    let next = pictured("go-next-symbolic", "Câu sau");
    let latest = gtk4::Button::with_label("Về câu mới");
    latest.set_tooltip_text(Some("Về câu mới nhất và tiếp tục theo dõi game"));
    latest.set_focusable(false);
    history_controls.append(&previous);
    history_controls.append(&next);
    history_controls.append(&position);
    history_controls.append(&latest);
    history.set_tooltip_text(Some("Lăn chuột ở đây để xem lịch sử; chữ dài có vùng cuộn riêng"));
    text.append(&history);
    // the first time: three lines of help in place of the message
    let intro = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    for line in ["Rê chuột lên chữ Nhật để tra nghĩa; quét chữ rồi chuột phải để chép.", "Bấm bánh răng để chỉnh hiển thị, nguồn dịch và từ điển.", "Muốn dời hoặc đổi kích thước: mở menu ⋯ → Chỉnh bố cục."] {
        intro.append(&label("intro"));
        intro.last_child().and_downcast::<gtk4::Label>().unwrap().set_text(line);
    }
    let understood = gtk4::Button::with_label("Đã hiểu");
    understood.add_css_class("understood");
    understood.set_focusable(false);
    understood.set_halign(gtk4::Align::Start);
    intro.append(&understood);
    intro.set_visible(false);
    text.append(&intro);
    // not a label that wraps: there is never room for it, and it would be broken letter by letter
    let source = gtk4::Label::new(None);
    source.add_css_class("source");
    source.set_xalign(0.0);
    source.set_hexpand(true);
    source.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let move_area = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    move_area.set_hexpand(true);
    let title = label("panel-title");
    title.set_text("vntext");
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_hexpand(true);
    move_area.append(&title);
    header.append(&move_area);
    body.append(&text);
    let service = gtk4::Button::new();
    service.add_css_class("service");
    service.set_focusable(false);
    service.set_halign(gtk4::Align::End);
    service.set_tooltip_text(Some("Nguồn đang dịch. Bấm để đổi sang nguồn khác"));
    let metadata = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    metadata.append(&source);
    metadata.append(&service);
    let speech_status = label("caption");
    speech_status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    speech_status.set_max_width_chars(28);
    metadata.append(&speech_status);
    text.prepend(&metadata);
    let editor = gtk4::Box::new(gtk4::Orientation::Vertical, 5);
    editor.add_css_class("layout-editor");
    let background = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    background.append(&gtk4::Label::new(Some("Độ đậm nền")));
    let opacity = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, *OPACITIES.start(), *OPACITIES.end(), 0.01);
    opacity.set_hexpand(true);
    opacity.set_width_request(80);
    opacity.set_tooltip_text(Some("Chỉ thay nền mờ; chữ và nút vẫn rõ"));
    let opacity_now = gtk4::Label::new(None);
    opacity_now.set_width_chars(4);
    background.append(&opacity);
    background.append(&opacity_now);
    let layout = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let automatic = gtk4::CheckButton::with_label("Chiều cao tự động");
    automatic.set_hexpand(true);
    automatic.set_tooltip_text(Some("Tự co giãn theo câu thoại; kéo cạnh trên/dưới để dùng chiều cao cố định"));
    let reset = gtk4::Button::with_label("Đặt lại");
    reset.set_focusable(false);
    reset.set_tooltip_text(Some("Đặt lại bố cục: vị trí, kích thước và độ đậm nền"));
    let done = gtk4::Button::with_label("Xong");
    done.set_focusable(false);
    done.add_css_class("done");
    layout.append(&automatic);
    layout.append(&reset);
    layout.append(&done);
    editor.append(&background);
    editor.append(&layout);
    content.append(&header);
    content.append(&editor);
    content.append(&body);
    content.append(&restore);
    let frame = gtk4::Overlay::new();
    frame.set_child(Some(&content));
    let mut handles = Vec::new();
    for (horizontal, vertical, cursor) in [(-1, -1, "nw-resize"), (0, -1, "n-resize"), (1, -1, "ne-resize"), (-1, 0, "w-resize"), (1, 0, "e-resize"), (-1, 1, "sw-resize"), (0, 1, "s-resize"), (1, 1, "se-resize")] {
        let handle = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        handle.add_css_class("resize-handle");
        handle.set_cursor_from_name(Some(cursor));
        handle.set_tooltip_text(Some("Kéo để đổi kích thước dải"));
        let align = |direction| match direction { -1 => gtk4::Align::Start, 1 => gtk4::Align::End, _ => gtk4::Align::Fill };
        handle.set_halign(align(horizontal));
        handle.set_valign(align(vertical));
        handle.set_size_request(if horizontal == 0 { -1 } else { 8 }, if vertical == 0 { -1 } else { 8 });
        if horizontal == 0 { handle.set_margin_start(16); handle.set_margin_end(16); }
        if vertical == 0 { handle.set_margin_top(16); handle.set_margin_bottom(16); }
        frame.add_overlay(&handle);
        handles.push(((horizontal, vertical), handle));
    }
    let stage = gtk4::Fixed::new();
    stage.put(&frame, 0.0, 0.0);
    strip.set_child(Some(&stage));

    // the meaning of a word: Yomitan's entries, one under the other, scrolled when long
    let popup = floating("vntext");
    let meaning = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    meaning.set_size_request(MEANING, -1);
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_child(Some(&meaning));
    let framed = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    framed.add_css_class("meaning");
    framed.append(&scrolled);
    popup.set_child(Some(&framed));
    // a lookup may wait for the browser or a big dictionary, so it is made off this thread
    let (asks, asked) = mpsc::channel::<(u64, String)>();
    let (answer, answers) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(mut ask) = asked.recv() {
            // the pointer has moved on from all but the last
            while let Ok(later) = asked.try_recv() {
                ask = later;
            }
            if answer.send((ask.0, lookup::find(&ask.1))).is_err() {
                return;
            }
        }
    });

    let hide = pictured("window-minimize-symbolic", "Thu gọn phụ đề tại chỗ");
    // the one button that says what it is on: an icon and the mode
    let mode = pictured("preferences-desktop-locale-symbolic", "Đổi giữa bản dịch, câu tiếng Nhật, hoặc cả hai");
    let mode_now = gtk4::Label::new(None);
    let both = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    both.append(&gtk4::Image::from_icon_name("preferences-desktop-locale-symbolic"));
    both.append(&mode_now);
    mode.set_child(Some(&both));

    let game = mem::name(pid);
    let look = saved_looks().get(&game).map(Look::from_json).unwrap_or_default();
    let ui = Ui { shown: Shown::default(), look, locked: true, game: None, placed: None, strip: (0, 0, 0), picking: false, reading, intro: !intro_read(&saved_looks()), selection: None, copied: None, hover: None, asked: None, lookups: 0, over_meaning: false, away: 0, history_open: false, expanded_height: 160, chrome_height: 0, icon_at: (0, 0) };
    let overlay = Rc::new(Overlay {
        ui: RefCell::new(ui),
        events,
        screen,
        display,
        pid,
        game,
        hub,
        pictures,
        settings,
        sizes,
        look: look_style,
        strip,
        stage,
        frame,
        content,
        speaker,
        japanese,
        translation,
        text_scroll,
        passage,
        source,
        service,
        intro,
        hinted,
        hint,
        dismiss,
        history,
        position,
        previous,
        next,
        latest,
        offer: Cell::new(None),
        move_area,
        title,
        handles,
        editor,
        done,
        opacity,
        opacity_now,
        automatic,
        reset,
        layout_sync: Cell::new(false),
        popup,
        meaning,
        scrolled,
        asks,
        answers,
        header,
        body,
        restore,
        icon_moved: Cell::new(false),
        history_summary,
        history_controls,
        empty,
        hide,
        mode,
        mode_now,
        speech,
        listen,
        speech_status,
        asking: Cell::new(false),
    });

    overlay.acting(&overlay.hide, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.locked = true;
        ui.icon_at = (ui.strip.0, ui.strip.1);
        ui.look.hidden = !ui.look.hidden;
        drop(ui);
        o.save();
    });
    overlay.acting(&overlay.mode, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.look.mode = ui.look.mode.next();
        drop(ui);
        o.save();
    });
    // The compact header stays with the text instead of floating at the game's corner.
    overlay.header.append(&overlay.mode);
    let listening = overlay.clone();
    overlay.listen.connect_clicked(move |_| {
        if listening.speech.status().busy() {
            listening.speech.stop();
        } else if listening.speech.settings().voice.is_empty() {
            listening.set_up(Tab::Speech);
        } else {
            let text = listening.ui.borrow().shown.texts(Mode::Japanese).0.unwrap_or_default();
            listening.speech.speak(&text);
        }
    });
    let more = gtk4::MenuButton::new();
    more.set_icon_name("view-more-symbolic");
    more.set_tooltip_text(Some("Thao tác khác: chỉnh bố cục, chép, OCR, dừng theo dõi"));
    more.set_popover(Some(&overlay.more_menu()));
    overlay.header.append(&more);
    overlay.header.append(&overlay.button("emblem-system-symbolic", "Thiết lập: hiển thị, dịch, từ điển, OCR", |o| o.set_up(Tab::Display)));
    overlay.header.append(&overlay.hide);
    overlay.acting(&overlay.restore, |o| {
        if o.icon_moved.replace(false) { return; }
        let mut ui = o.ui.borrow_mut();
        if let Some((dx, dy)) = ui.look.icon {
            let width = ui.look.place.map_or(ui.strip.2, |p| p.2);
            ui.look.place = Some((dx, dy, width));
        }
        ui.look.hidden = false;
        drop(ui);
        o.save();
    });
    overlay.acting(&overlay.history_summary, |o| o.ui.borrow_mut().history_open = true);
    overlay.acting(&overlay.dismiss, |o| o.ui.borrow_mut().shown.dismiss());
    overlay.acting(&overlay.previous, |o| {
        o.ui.borrow_mut().shown.step(-1);
        o.speech.suspend(true);
    });
    overlay.acting(&overlay.next, |o| {
        o.ui.borrow_mut().shown.step(1);
        o.speech.suspend(o.ui.borrow().shown.old().is_some());
    });
    overlay.acting(&overlay.latest, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.shown.latest();
        ui.history_open = false;
        drop(ui);
        o.speech.suspend(false);
    });
    let chooser = overlay.clone();
    overlay.service.connect_clicked(move |_| chooser.choose_service());
    let helped = overlay.clone();
    understood.connect_clicked(move |_| {
        helped.ui.borrow_mut().intro = false;
        save_intro_read();
        helped.render();
        helped.place();
    });
    let remedy = overlay.clone();
    overlay.hint.connect_clicked(move |_| {
        if let Some(action) = remedy.offer.get() {
            remedy.act(action);
        }
    });

    overlay.dragging(overlay.move_area.upcast_ref(), (0, 0));
    overlay.icon_drag();
    for (edges, handle) in &overlay.handles { overlay.dragging(handle.upcast_ref(), *edges); }
    let editing = overlay.clone();
    overlay.done.connect_clicked(move |_| editing.editing(false));
    let fading = overlay.clone();
    overlay.opacity.connect_value_changed(move |scale| {
        if !fading.layout_sync.get() { fading.restyle(|look| look.opacity = scale.value()); }
    });
    let sizing = overlay.clone();
    overlay.automatic.connect_toggled(move |automatic| {
        if sizing.layout_sync.get() { return; }
        let height = sizing.frame.height().max(SHORTEST);
        sizing.restyle(|look| look.height = if automatic.is_active() { None } else { Some(height) });
    });
    overlay.acting(&overlay.reset, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.look.place = None;
        ui.look.height = None;
        ui.look.icon = None;
        ui.look.opacity = OPACITY;
        drop(ui);
        o.save();
    });
    // the pointer on the meaning keeps it open, to be read and scrolled
    let on_meaning = gtk4::EventControllerMotion::new();
    let (entered, left) = (overlay.clone(), overlay.clone());
    on_meaning.connect_enter(move |_, _, _| { if let Ok(mut ui) = entered.ui.try_borrow_mut() { ui.over_meaning = true; } });
    on_meaning.connect_leave(move |_| { if let Ok(mut ui) = left.ui.try_borrow_mut() { ui.over_meaning = false; } });
    overlay.popup.add_controller(on_meaning);
    // the right button on the message opens a menu of what can be done with
    // it, before the label can open its own
    let right = gtk4::GestureClick::new();
    right.set_button(gdk::BUTTON_SECONDARY);
    right.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let clicked = overlay.clone();
    right.connect_pressed(move |gesture, _, x, y| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        clicked.text_menu(x, y);
    });
    overlay.japanese.add_controller(right);
    // the pointer over the message says which word to look up
    let pointer = gtk4::EventControllerMotion::new();
    let over = overlay.clone();
    pointer.connect_motion(move |_, x, y| {
        let label = &over.japanese;
        let (left, top) = label.layout_offsets();
        let scale = gtk4::pango::SCALE as f64;
        let (inside, index, _) = label.layout().xy_to_index(((x - left as f64) * scale) as i32, ((y - top as f64) * scale) as i32);
        let in_strip = label.compute_point(&over.frame, &gtk4::graphene::Point::new(x as f32, y as f32)).map_or(0, |p| p.x() as i32);
        if let Ok(mut ui) = over.ui.try_borrow_mut() { ui.hover = inside.then_some((index as usize, in_strip)); }
    });
    let left = overlay.clone();
    // Hiding the text during render can synchronously emit leave while Ui is borrowed.
    pointer.connect_leave(move |_| { if let Ok(mut ui) = left.ui.try_borrow_mut() { ui.hover = None; } });
    overlay.japanese.add_controller(pointer);
    // Long passages scroll separately; history only changes over its controls.
    let wheel = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
    let scrolled = overlay.clone();
    wheel.connect_scroll(move |_, _, dy| {
        let mut ui = scrolled.ui.borrow_mut();
        ui.shown.step(dy.signum() as isize);
        let history = ui.shown.old().is_some();
        drop(ui);
        scrolled.speech.suspend(history);
        scrolled.render();
        glib::Propagation::Stop
    });
    overlay.history.add_controller(wheel);

    overlay.render();
    Ok(overlay)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run only inside the private compositor created by tools/uiux-smoke.py.
    #[test]
    #[ignore]
    fn uiux_smoke() {
        assert_eq!(std::env::var("WAYLAND_DISPLAY").as_deref(), Ok("vntext-uiux-test"));
        assert_eq!(std::env::var("VNTEXT_UI_TEST").as_deref(), Ok("isolated"));
        struct Desktop;
        impl Screen for Desktop {
            fn window_of(&self, _: u32) -> Option<WindowId> { Some(WindowId::X11(1)) }
            fn placement(&self, _: &WindowId) -> Option<(i32, i32, i32, i32)> { Some((40, 24, 680, 370)) }
            fn active_is(&self, _: u32) -> bool { true }
            fn pointer(&self, _: &WindowId) -> Option<(f64, f64)> { None }
            fn grab(&self, _: &WindowId, _: Region) -> Option<screen::Picture> { None }
        }
        fn pump() {
            let context = glib::MainContext::default();
            let until = Instant::now() + Duration::from_millis(250);
            while Instant::now() < until {
                while context.pending() { context.iteration(false); }
                thread::sleep(Duration::from_millis(5));
            }
        }
        fn capture(window: &gtk4::Window, name: &str) {
            pump();
            assert!(window.is_mapped());
            let paintable = gtk4::WidgetPaintable::new(Some(window));
            let snapshot = gtk4::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            let node = snapshot.to_node().expect("Visible UI must render");
            let texture = window.renderer().unwrap().render_texture(&node, None);
            let directory = PathBuf::from(std::env::var_os("VNTEXT_UI_OUTPUT").unwrap());
            texture.save_to_png(directory.join(format!("{name}.png"))).unwrap();
        }
        fn descendants(widget: &gtk4::Widget) -> Vec<gtk4::Widget> {
            let mut found = vec![widget.clone()];
            let mut child = widget.first_child();
            while let Some(item) = child {
                found.extend(descendants(&item));
                child = item.next_sibling();
            }
            found
        }
        fn drag(overlay: &Overlay, widget: &gtk4::Widget, dx: f64, dy: f64) {
            let gesture = widget.observe_controllers().into_iter().flatten()
                .find_map(|c| c.downcast::<gtk4::GestureDrag>().ok()).unwrap();
            let origin = || {
                let p = widget.compute_point(&overlay.strip, &gtk4::graphene::Point::new(0.0, 0.0)).unwrap();
                (p.x() as f64, p.y() as f64)
            };
            let start = origin();
            gesture.emit_by_name::<()>("drag-begin", &[&4.0f64, &4.0f64]);
            gesture.emit_by_name::<()>("drag-update", &[&(dx / 2.0), &(dy / 2.0)]);
            pump();
            let moved = origin();
            let offset = (dx - (moved.0 - start.0), dy - (moved.1 - start.1));
            gesture.emit_by_name::<()>("drag-update", &[&offset.0, &offset.1]);
            pump();
            let resting = overlay.ui.borrow().look.clone();
            // Repeated updates while held at the same surface point must not drift.
            for _ in 0..3 {
                let moved = origin();
                let offset = (dx - (moved.0 - start.0), dy - (moved.1 - start.1));
                gesture.emit_by_name::<()>("drag-update", &[&offset.0, &offset.1]);
                overlay.place();
                pump();
                assert_eq!(overlay.ui.borrow().look, resting);
            }
            gesture.emit_by_name::<()>("drag-end", &[&offset.0, &offset.1]);
            pump();
            overlay.place();
        }
        let opts = crate::Opts {
            args: Vec::new(), min: 4, all: false, clip: false, raw: false,
            translate: false, to: "vi".to_string(), model: None, web: false,
            open: false, port: 0, debug: false, pick: false, overlay: true, via: None,
        };
        let hub = web::Hub::default();
        let speech = tts::start(&hub, std::process::id());
        let overlay = build(Box::new(Desktop), hub.subscribe(), hub.clone(), std::process::id(),
                            Arc::new(ocr::Control::default()), ocr::Settings::of(&opts), false, speech).unwrap();
        overlay.ui.borrow_mut().intro = false;
        overlay.ui.borrow_mut().look.mode = Mode::Both;
        overlay.render();
        overlay.tick();
        capture(&overlay.strip, "waiting");
        assert!(overlay.empty.is_mapped() && overlay.header.is_mapped());
        hub.publish(r#"{"type":"message","seq":1,"name":"例","lines":["こんにちは。今日はいい天気ですね。"]}"#.to_string());
        hub.publish(r#"{"type":"translation","seq":1,"tr":"Xin chào. Hôm nay trời đẹp nhỉ."}"#.to_string());
        hub.publish(r#"{"type":"message","seq":2,"lines":["一緒に散歩しましょう。"]}"#.to_string());
        overlay.tick();
        pump();
        overlay.japanese.select_region(0, 2);
        let selection = overlay.japanese.selection_bounds();
        assert!(selection.is_some());
        hub.publish(r#"{"type":"translation","seq":2,"tr":"Cùng đi dạo nhé."}"#.to_string());
        hub.publish(r#"{"type":"toast","text":"Đã lưu thiết lập."}"#.to_string());
        overlay.tick();
        assert_eq!(overlay.japanese.selection_bounds(), selection);
        assert!(overlay.ui.borrow().locked);
        assert!(overlay.japanese.is_selectable());
        assert!(!overlay.ui.borrow().look.copy);
        capture(&overlay.strip, "overlay");
        assert!(overlay.history_summary.is_mapped());
        assert!(!overlay.history_controls.is_mapped());
        overlay.history_summary.emit_clicked();
        pump();
        assert!(overlay.history_controls.is_mapped());
        overlay.previous.emit_clicked();
        hub.publish(r#"{"type":"message","seq":3,"lines":["また明日。"]}"#.to_string());
        overlay.tick();
        assert_eq!(overlay.ui.borrow().shown.messages[overlay.ui.borrow().shown.at].seq, 1);
        capture(&overlay.strip, "history");
        overlay.latest.emit_clicked();
        assert_eq!(overlay.ui.borrow().shown.messages[overlay.ui.borrow().shown.at].seq, 3);
        pump();
        overlay.place();
        let expanded = overlay.ui.borrow().strip;
        overlay.hide.emit_clicked();
        capture(&overlay.strip, "collapsed");
        assert!(overlay.strip.is_mapped() && overlay.restore.is_mapped());
        assert!(overlay.restore.child().unwrap().is::<gtk4::Image>());
        assert!(overlay.restore.label().is_none());
        assert!(!overlay.header.is_mapped() && !overlay.body.is_mapped());
        assert_eq!(overlay.ui.borrow().strip, expanded);
        assert!(overlay.frame.width() < expanded.2);
        if let Some(helper) = std::env::var_os("VNTEXT_UI_POINTER") {
            overlay.place();
            pump();
            // Optional real protocol input, restricted by the helper to the private compositor.
            let at = overlay.ui.borrow().icon_at;
            let mut pointer = Command::new(helper).args([(at.0 + 24).to_string(), (at.1 + 24).to_string(), "60".to_string(), "40".to_string()]).spawn().unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                pump();
                if let Some(status) = pointer.try_wait().unwrap() { assert!(status.success()); break; }
                if Instant::now() >= until { pointer.kill().unwrap(); pointer.wait().unwrap(); panic!("Private pointer helper timed out"); }
            }
            assert!(overlay.ui.borrow().look.hidden, "A real pointer drag must not activate restore");
            assert_eq!(overlay.ui.borrow().icon_at, (at.0 + 60, at.1 + 40), "The button must let its drag gesture move the icon");
            overlay.ui.borrow_mut().look.icon = None;
            overlay.ui.borrow_mut().icon_at = at;
            overlay.icon_moved.set(false);
            overlay.place();
        }
        overlay.restore.emit_clicked();
        pump();
        overlay.place();
        assert!(overlay.header.is_mapped() && overlay.body.is_mapped());
        assert!(!overlay.restore.is_mapped());
        assert_eq!(overlay.ui.borrow().strip, expanded);
        hub.publish(r#"{"type":"translation","seq":3,"tr":"Hẹn ngày mai nhé."}"#.to_string());
        overlay.ui.borrow_mut().look.mode = Mode::Translation;
        overlay.tick();
        pump();
        overlay.place();
        assert!(!overlay.japanese.is_visible());
        assert!(overlay.listen.is_mapped() && overlay.header.is_mapped());
        let input = overlay.input(true);
        for button in [overlay.header.upcast_ref::<gtk4::Widget>(), overlay.listen.upcast_ref(), overlay.history_summary.upcast_ref()] {
            let b = button.compute_bounds(&overlay.strip).unwrap();
            assert!(input.contains_point((b.x() + b.width() / 2.0) as i32, (b.y() + b.height() / 2.0) as i32));
        }
        let b = overlay.translation.compute_bounds(&overlay.strip).unwrap();
        assert!(!input.contains_point((b.x() + b.width() / 2.0) as i32, (b.y() + b.height() / 2.0) as i32));
        capture(&overlay.strip, "translation");
        overlay.ui.borrow_mut().look.mode = Mode::Both;
        overlay.render();
        overlay.place();
        let look = overlay.ui.borrow().look.clone();
        {
            let mut ui = overlay.ui.borrow_mut();
            ui.look.place = Some((10000, 10000, 200));
            ui.look.font = 72;
        }
        overlay.render();
        overlay.place();
        pump();
        overlay.place();
        capture(&overlay.strip, "narrow");
        let bounds = overlay.strip.monitor().unwrap().geometry();
        assert!(overlay.strip.width() <= bounds.width() && overlay.strip.height() <= bounds.height());
        assert!(overlay.strip.margin(Edge::Left) + overlay.strip.width() <= bounds.width());
        assert!(overlay.strip.margin(Edge::Top) + overlay.strip.height() <= bounds.height());
        assert!(overlay.hide.is_mapped() && overlay.listen.is_mapped());
        overlay.ui.borrow_mut().look = look;
        overlay.render();
        overlay.place();
        for (tab, id, _) in Tab::ALL {
            overlay.set_up(tab);
            pump();
            let window = gtk4::Window::list_toplevels().into_iter()
                .filter_map(|widget| widget.downcast::<gtk4::Window>().ok())
                .find(|window| window.has_css_class("vntext-ask") && window.is_visible()).unwrap();
            let close = descendants(window.upcast_ref()).into_iter()
                .find(|widget| widget.tooltip_text().as_deref() == Some("Đóng thiết lập (Esc)"))
                .unwrap().downcast::<gtk4::Button>().unwrap();
            assert!(close.is_mapped(), "Every settings tab must keep the close button visible");
            let monitor = overlay.display.monitors().item(0).unwrap().downcast::<gdk::Monitor>().unwrap().geometry();
            assert!(window.width() <= monitor.width() && window.height() <= monitor.height());
            capture(&window, &format!("settings-{id}"));
            close.emit_clicked();
            pump();
            assert!(!overlay.asking.get());
        }
        let buttons = overlay.header.clone().upcast::<gtk4::Widget>();
        assert!(descendants(&buttons).contains(&overlay.hide.clone().upcast::<gtk4::Widget>()));
        assert!(descendants(&buttons).contains(&overlay.mode.clone().upcast::<gtk4::Widget>()));
        assert!(descendants(overlay.strip.upcast_ref()).contains(&buttons));
        assert!(descendants(overlay.strip.upcast_ref()).contains(&overlay.listen.clone().upcast::<gtk4::Widget>()));
        assert!(!descendants(&buttons).contains(&overlay.listen.clone().upcast::<gtk4::Widget>()));
        let menu_button = descendants(&buttons).into_iter().find_map(|widget| widget.downcast::<gtk4::MenuButton>().ok()).unwrap();
        let menu = menu_button.popover().unwrap();
        menu_button.popup();
        pump();
        assert!(menu.is_mapped());
        let edit = descendants(menu.upcast_ref()).into_iter().filter_map(|w| w.downcast::<gtk4::Button>().ok())
            .find(|b| b.label().as_deref() == Some("Chỉnh bố cục…")).unwrap();
        edit.emit_clicked();
        pump();
        assert!(!overlay.ui.borrow().locked);
        assert!(overlay.editor.is_mapped() && overlay.handles.iter().all(|(_, h)| h.is_mapped()));
        // A press and tiny pointer jitter must not start moving the panel.
        let stationary = overlay.ui.borrow().look.clone();
        drag(&overlay, overlay.move_area.upcast_ref(), 0.0, 0.0);
        assert_eq!(overlay.ui.borrow().look, stationary);
        drag(&overlay, overlay.move_area.upcast_ref(), 1.0, 1.0);
        assert_eq!(overlay.ui.borrow().look, stationary);
        overlay.japanese.select_region(0, 2);
        let selection = overlay.japanese.selection_bounds();
        overlay.opacity.set_value(0.33);
        assert_eq!(overlay.ui.borrow().look.opacity, 0.33);
        assert_eq!(overlay.opacity_now.text(), "33%");
        assert_eq!(overlay.japanese.selection_bounds(), selection);
        overlay.restyle(|look| { look.place = Some((0, 0, 600)); look.height = Some(320); });
        pump();
        overlay.place();
        let corner = &overlay.handles.iter().find(|(edges, _)| *edges == (1, 1)).unwrap().1;
        drag(&overlay, corner.upcast_ref(), 80.0, 60.0);
        assert_eq!(overlay.ui.borrow().look.place, Some((0, 0, 680)));
        assert_eq!(overlay.ui.borrow().look.height, Some(380));
        assert_eq!(overlay.frame.height(), 380);
        let corner = &overlay.handles.iter().find(|(edges, _)| *edges == (-1, -1)).unwrap().1;
        drag(&overlay, corner.upcast_ref(), -10.0, -10.0);
        assert_eq!(overlay.ui.borrow().look.place, Some((-10, -10, 690)));
        assert_eq!(overlay.ui.borrow().look.height, Some(390));
        drag(&overlay, overlay.move_area.upcast_ref(), 20.0, 25.0);
        assert_eq!(overlay.ui.borrow().look.place, Some((10, 15, 690)));
        assert_eq!(overlay.frame.height(), 390);
        let panel = overlay.frame.compute_bounds(&overlay.strip).unwrap();
        let input = overlay.input(false);
        assert!(input.contains_point((panel.x() + 20.0) as i32, (panel.y() + 20.0) as i32));
        assert!(!input.contains_point(overlay.strip.width() - 1, overlay.strip.height() - 1), "Editing must leave the game clickable outside the panel");
        capture(&overlay.strip, "layout-editor");
        let long = "今日はいい天気ですね。一緒に散歩しましょう。".repeat(30);
        hub.publish(json!({"type": "message", "seq": 4, "lines": [long]}).to_string());
        overlay.tick();
        pump();
        assert_eq!(overlay.frame.height(), 390);
        let scroll = overlay.text_scroll.vadjustment();
        assert!(scroll.upper() > scroll.page_size());
        assert!(overlay.header.is_mapped() && overlay.done.is_mapped());
        overlay.done.emit_clicked();
        pump();
        assert_eq!(overlay.frame.height(), 390);
        assert!(!overlay.editor.is_mapped());
        assert_eq!(Look::from_json(&saved_looks()[&overlay.game]).height, Some(390));
        overlay.hide.emit_clicked();
        pump();
        assert_eq!(overlay.ui.borrow().look.height, Some(390));
        assert!(overlay.restore.is_mapped());
        overlay.restore.emit_clicked();
        pump();
        assert_eq!(overlay.frame.height(), 390);
        overlay.editing(true);
        pump();
        overlay.automatic.set_active(true);
        pump();
        let side = &overlay.handles.iter().find(|(edges, _)| *edges == (1, 0)).unwrap().1;
        drag(&overlay, side.upcast_ref(), -120.0, 0.0);
        assert!(overlay.ui.borrow().look.height.is_none());
        assert_eq!(overlay.ui.borrow().look.place.unwrap().2, 570);
        overlay.reset.emit_clicked();
        assert!(overlay.ui.borrow().look.place.is_none() && overlay.ui.borrow().look.height.is_none());
        assert_eq!(overlay.ui.borrow().look.opacity, OPACITY);
        overlay.done.emit_clicked();
        pump();
        assert!(overlay.ui.borrow().locked && overlay.japanese.is_selectable());
        assert!(!overlay.editor.is_mapped() && overlay.handles.iter().all(|(_, h)| !h.is_mapped()));
        let saved = saved_looks();
        assert_eq!(Look::from_json(&saved[&overlay.game]), overlay.ui.borrow().look);
        let look = overlay.ui.borrow().look.clone();
        drag(&overlay, overlay.move_area.upcast_ref(), 30.0, 30.0);
        assert_eq!(overlay.ui.borrow().look, look);
        // Wrapped dialogue and an error banner must not squeeze the viewport.
        hub.publish(json!({"type": "message", "seq": 5, "lines": ["今日はいい天気ですね。", "一緒に散歩しましょう。"]}).to_string());
        hub.publish(json!({"type": "translation", "seq": 5, "tr": "Hôm nay trời đẹp nhỉ.\nCùng đi dạo nhé."}).to_string());
        hub.publish(json!({"type": "notice", "text": "Có lỗi: HTTP 502"}).to_string());
        overlay.tick();
        pump();
        overlay.place();
        pump();
        let viewport = overlay.text_scroll.vadjustment();
        assert!(viewport.page_size() >= 100.0);
        assert!(viewport.upper() <= viewport.page_size() + 1.0, "Short Japanese and Vietnamese must both fit without clipping");
        for text in [&overlay.japanese, &overlay.translation] {
            let b = text.compute_bounds(&overlay.passage).unwrap();
            assert!(b.y() >= 0.0 && b.y() + b.height() <= viewport.page_size() as f32 + 1.0);
        }
        capture(&overlay.strip, "readable-error");
        overlay.restyle(|look| { look.place = Some((0, 0, 600)); look.height = Some(320); });
        pump();
        overlay.place();
        let expanded = overlay.ui.borrow().look.clone();
        overlay.hide.emit_clicked();
        pump();
        let still = overlay.ui.borrow().icon_at;
        drag(&overlay, overlay.restore.upcast_ref(), 0.0, 0.0);
        assert_eq!(overlay.ui.borrow().icon_at, still);
        drag(&overlay, overlay.restore.upcast_ref(), 100.0, 80.0);
        assert!(overlay.ui.borrow().look.hidden);
        assert_eq!(overlay.ui.borrow().icon_at, (still.0 + 100, still.1 + 80));
        assert_eq!(overlay.ui.borrow().look.place, expanded.place);
        assert_eq!(overlay.ui.borrow().look.height, expanded.height);
        assert_eq!(Look::from_json(&saved_looks()[&overlay.game]).icon, Some((100, 80)));
        capture(&overlay.strip, "moved-icon");
        let input = overlay.input(false);
        assert!(!input.contains_point(0, 0));
        // A click generated at the end of a drag must be ignored.
        overlay.restore.emit_clicked();
        assert!(overlay.ui.borrow().look.hidden);
        // An ordinary click still restores next to the icon, with the old size.
        overlay.restore.emit_clicked();
        pump();
        overlay.place();
        assert!(!overlay.ui.borrow().look.hidden);
        assert_eq!(overlay.ui.borrow().look.place, Some((100, 80, 600)));
        assert_eq!(overlay.frame.height(), 320);
        // Saved icon position is reused and clamped by its own compact footprint.
        overlay.hide.emit_clicked();
        pump();
        drag(&overlay, overlay.restore.upcast_ref(), 10000.0, 10000.0);
        let at = overlay.ui.borrow().icon_at;
        let bounds = overlay.strip.monitor().unwrap().geometry();
        assert_eq!(at.0 + overlay.frame.width(), bounds.x() + bounds.width());
        assert_eq!(at.1 + overlay.frame.height(), bounds.y() + bounds.height());
        overlay.icon_moved.set(false);
        overlay.restore.emit_clicked();
        pump();
        overlay.place();
        assert_eq!(overlay.ui.borrow().strip.0 + overlay.ui.borrow().strip.2, bounds.x() + bounds.width());
        assert!(overlay.ui.borrow().strip.1 + overlay.frame.height() <= bounds.y() + bounds.height());
        overlay.text_menu(4.0, 4.0);
        pump();
        assert!(descendants(overlay.japanese.upcast_ref()).into_iter()
            .any(|widget| widget.downcast::<gtk4::Popover>().is_ok_and(|menu| menu.is_mapped())));
        overlay.strip.destroy();
        overlay.popup.destroy();
        overlay.speech.shutdown();
    }

    #[test]
    fn panel_stays_within_its_monitor() {
        let screen = (0, 0, 1024, 768);
        assert_eq!(panel((40, 24, 680, 370), None, screen, 320, 150), (56, 400, 648));
        assert_eq!(panel((0, 0, 1024, 768), None, screen, 320, 150), (16, 612, 992));
        assert_eq!(panel((900, 650, 680, 370), Some((200, 100, 680)), screen, 320, 150), (344, 618, 680));
        assert_eq!(panel((40, 24, 680, 370), Some((0, 0, 200)), screen, 320, 150), (40, 24, 320));
        assert_eq!(panel((-1920, 0, 1920, 1080), None, (-1920, 0, 1920, 1080), 320, 150), (-1904, 924, 1888));
        assert_eq!(panel((0, 0, 800, 600), None, (0, 0, 280, 240), 320, 300), (0, 0, 280));
    }

    #[test]
    fn resize_keeps_opposite_edges_and_respects_bounds() {
        let rect = (100, 100, 500, 300);
        let screen = (0, 0, 1024, 768);
        assert_eq!(resized(rect, (40, 50), (1, 1), (300, 140), screen), (100, 100, 540, 350));
        assert_eq!(resized(rect, (40, 50), (-1, -1), (300, 140), screen), (140, 150, 460, 250));
        assert_eq!(resized(rect, (1000, 1000), (-1, -1), (300, 140), screen), (300, 260, 300, 140));
        assert_eq!(resized(rect, (-1000, -1000), (-1, -1), (300, 140), screen), (0, 0, 600, 400));
        assert_eq!(resized(rect, (1000, 1000), (1, 1), (300, 140), screen), (100, 100, 924, 668));
        assert_eq!(resized(rect, (-1000, 50), (1, 0), (300, 140), screen), (100, 100, 300, 300));
        assert_eq!(resized((-1800, 100, 500, 300), (-400, 0), (-1, 0), (300, 140), (-1920, 0, 1920, 1080)), (-1920, 100, 620, 300));
    }

    #[test]
    fn messages_and_translations() {
        let mut shown = Shown::default();
        shown.take(r#"{"type":"message","seq":3,"name":"誰","lines":["一","二"],"translating":false}"#);
        assert_eq!(shown.texts(Mode::Translation), (Some("一\n二".to_string()), None, false));
        assert_eq!(shown.speaker(), Some("誰"));
        assert_eq!(shown.source().as_deref(), Some("Trích text trực tiếp"));
        shown.take(r#"{"type":"message","seq":4,"lines":["三"],"translating":true}"#);
        assert_eq!(shown.texts(Mode::Translation), (None, Some(PENDING.to_string()), true));
        shown.take(r#"{"type":"translation","seq":3,"tr":"late"}"#);
        assert!(shown.texts(Mode::Translation).2);
        shown.take(r#"{"type":"translation","seq":4,"tr":"ba"}"#);
        assert_eq!(shown.texts(Mode::Translation), (None, Some("ba".to_string()), false));
        assert_eq!(shown.texts(Mode::Both), (Some("三".to_string()), Some("ba".to_string()), false));
        assert_eq!(shown.texts(Mode::Japanese), (Some("三".to_string()), None, false));
    }

    #[test]
    fn stepping_back() {
        let mut shown = Shown::default();
        for seq in 1..=3 {
            shown.take(&format!(r#"{{"type":"message","seq":{seq},"lines":["{seq}"]}}"#));
        }
        shown.step(-1);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("2"));
        assert_eq!(shown.old(), Some((2, 3)));
        assert_eq!(shown.source().as_deref(), Some("Trích text trực tiếp"));
        // New passages do not take the reader away from a passage in history.
        shown.take(r#"{"type":"message","seq":4,"lines":["4"]}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("2"));
        assert_eq!(shown.old(), Some((2, 4)));
        shown.latest();
        assert_eq!(shown.old(), None);
        assert_eq!(shown.source().as_deref(), Some("Trích text trực tiếp"));
        shown.step(9);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("4"));
        shown.step(-9);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("1"));
    }

    #[test]
    fn history_keeps_the_same_passage_when_the_buffer_fills() {
        let mut shown = Shown::default();
        let message = |seq| format!(r#"{{"type":"message","seq":{seq},"lines":["{seq}"]}}"#);
        for seq in 1..=KEPT {
            shown.take(&message(seq));
        }
        shown.step(-10);
        let selected = shown.messages[shown.at].seq;
        shown.take(&message(KEPT + 1));
        assert_eq!(shown.messages.len(), KEPT);
        assert_eq!(shown.messages[shown.at].seq, selected);
        shown.step(-999);
        shown.take(&message(KEPT + 2));
        assert_eq!(shown.messages[shown.at].seq, 3);
        shown.latest();
        shown.take(&message(KEPT + 3));
        assert_eq!(shown.messages[shown.at].seq, (KEPT + 3) as u64);
    }

    #[test]
    fn toasts_expire_without_losing_the_passage_or_translation() {
        let mut shown = Shown::default();
        shown.take(r#"{"type":"message","seq":1,"name":"例","lines":["こんにちは。"]}"#);
        shown.take(r#"{"type":"toast","text":"Đã lưu"}"#);
        let until = shown.expires.unwrap();
        shown.take(r#"{"type":"translation","seq":1,"tr":"Xin chào."}"#);
        assert_eq!(shown.offered(), Some(("Đã lưu".to_string(), None)));
        assert!(!shown.expire(until - Duration::from_millis(1)));
        assert!(shown.expire(until));
        assert_eq!(shown.offered(), None);
        assert_eq!(shown.copied(Mode::Both).as_deref(), Some("こんにちは。\nXin chào."));
        assert_eq!(shown.speaker(), Some("例"));
        shown.take(r#"{"type":"notice","text":"Thiếu key","action":"translate"}"#);
        assert!(!shown.expire(until + Duration::from_secs(60)));
        shown.dismiss();
        assert_eq!(shown.offered(), None);
    }

    #[test]
    fn notices() {
        let mut shown = Shown::default();
        shown.take(r#"{"type":"notice","text":"oops"}"#);
        assert_eq!(shown.texts(Mode::Japanese), (None, None, false));
        assert_eq!(shown.offered(), Some(("oops".to_string(), None)));
        shown.take(r#"{"type":"message","seq":1,"lines":["一"]}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("一"));
        shown.take(r#"{"type":"message","seq":2,"source":"ocr","lines":["二"]}"#);
        assert_eq!(shown.source().as_deref(), Some("Đọc từ ảnh"));
        shown.take(r#"{"type":"translation","seq":2,"tr":"hai","via":"google"}"#);
        assert!(shown.by_google());
        assert_eq!(shown.copied(Mode::Both).as_deref(), Some("二\nhai"));
        assert_eq!(shown.copied(Mode::Translation).as_deref(), Some("hai"));
    }

    #[test]
    fn looks_round_trip() {
        let look = Look { font: 30, opacity: 0.4, colour: "#a6e3a1".to_string(), mode: Mode::Both, place: Some((10, -20, 640)), height: Some(300), icon: Some((20, 30)), hidden: true, copy: true };
        assert_eq!(Look::from_json(&look.to_json()), look);
        assert_eq!(Look::from_json(&json!({"font": 500, "mode": "?", "place": [1, 2]})), Look::default());
    }

    #[test]
    fn looks_saved_before_opacity_and_colour() {
        // a file from before the two were there, and one with them out of range or not a colour
        let old = Look::from_json(&json!({"font": 30, "mode": "both", "place": [1, 2, 300], "hidden": true}));
        assert_eq!((old.font, old.opacity, old.colour.as_str()), (30, OPACITY, COLOUR));
        assert_eq!(old.place, Some((1, 2, 300)));
        assert!(old.height.is_none());
        assert!(old.icon.is_none());
        assert!(Look::from_json(&json!({"icon": [1]})).icon.is_none());
        assert!(Look::from_json(&json!({"height": 20})).height.is_none());
        let wrong = Look::from_json(&json!({"opacity": 0.0, "colour": "red; } * { display: none"}));
        assert_eq!((wrong.opacity, wrong.colour.as_str()), (OPACITY, COLOUR));
        assert_eq!(Look::from_json(&json!({"opacity": 1.0})).opacity, 1.0);
    }

    #[test]
    fn the_style_follows_the_look() {
        let look = Look { opacity: 0.5, colour: "#ffe08a".to_string(), font: 30, ..Look::default() };
        let (sizes, colour) = strip_style(&look);
        assert!(sizes.contains("rgba(0, 0, 0, 0.50)") && sizes.contains("font-size: 30px"));
        assert!(colour.contains("color: #ffe08a"));
        assert!(is_colour("#00ff7F") && !is_colour("00ff7f") && !is_colour("#00ff7") && !is_colour("#00ff7g"));
    }

    #[test]
    fn the_first_time_help_is_marked_once() {
        assert!(!intro_read(&json!({"game.exe": {}})));
        assert!(intro_read(&json!({"@intro": true, "game.exe": {}})));
    }

    #[test]
    fn hints_and_what_they_offer() {
        let mut shown = Shown::default();
        shown.take(r#"{"type":"message","seq":1,"lines":["一"],"translating":true}"#);
        shown.take(r#"{"type":"hint","text":"hết hạn mức","action":"google"}"#);
        // the message stays, with the hint beside it
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("一"));
        assert_eq!(shown.offered(), Some(("hết hạn mức".to_string(), Some(Action::Google))));
        shown.take(r#"{"type":"translation","seq":1,"tr":"một"}"#);
        assert_eq!(shown.offered(), None);
        // Notices also leave the Japanese and the existing translation in place.
        shown.take(r#"{"type":"notice","text":"key sai","action":"translate"}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("一"));
        assert_eq!(shown.texts(Mode::Translation).1.as_deref(), Some("một"));
        assert_eq!(shown.offered(), Some(("key sai".to_string(), Some(Action::Translate))));
        shown.take(r#"{"type":"notice","text":"mạng"}"#);
        assert_eq!(shown.offered(), Some(("mạng".to_string(), None)));
        shown.take(r#"{"type":"message","seq":2,"lines":["二"]}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("二"));
    }

    #[test]
    fn modes_have_names() {
        assert_eq!(Mode::Both.name(), "Cả hai");
        assert_eq!(Mode::ALL.map(|m| m.0.name()).len(), 3);
    }
}
