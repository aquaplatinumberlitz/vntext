//! What floats by the game's window: a strip with the message the game is
//! on, and a bar of buttons to set the strip up and to have the game's text
//! box read from pictures.
//!
//! Both are layer-shell surfaces, the only kind of window that a Wayland
//! compositor lets a program place and keep above the others. The strip
//! lets clicks through to whatever is below until it is unlocked.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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
use crate::{mem, ocr, web, yomitan};

const TICK: Duration = Duration::from_millis(100);
/// Between the game's window and what floats by it.
const GAP: i32 = 6;
/// How wide the bar of buttons is taken to be when a place is found for it.
const BAR: i32 = 48;
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
.vntext .bar { background: rgba(0, 0, 0, 0.72); border-radius: 8px; padding: 4px; opacity: 0.5; transition: opacity 150ms; }
.vntext .bar:hover { opacity: 1; }
.vntext .bar button { min-height: 0; min-width: 0; padding: 4px 6px; margin: 1px; font-size: 11px;
    color: white; background: rgba(255, 255, 255, 0.12); border: none; box-shadow: none; }
.vntext .bar button:hover { background: rgba(255, 255, 255, 0.3); }
.vntext .bar button.on { background: #e0a458; color: black; }
.vntext .bar.menu { opacity: 1; background: rgba(24, 24, 24, 0.97); }
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
";

/// Starts the overlay for the game `pid`. It shows the messages and notices
/// sent to the page from now on. With `reading` it has the text box read
/// from pictures at once, asking where the box is if it was never told.
pub fn start(hub: web::Hub, pid: u32, pictures: Arc<ocr::Control>, settings: ocr::Settings, reading: bool) {
    let events = hub.subscribe();
    thread::spawn(move || {
        if let Err(e) = run(events, hub, pid, pictures, settings, reading) {
            eprintln!("\x1b[33mno overlay: {e}\x1b[0m");
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
    const ALL: [(Mode, &'static str, &'static str); 3] = [(Mode::Translation, "translation", "Dịch"), (Mode::Japanese, "japanese", "Nhật"), (Mode::Both, "both", "2")];

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
    hidden: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look { font: FONT, opacity: OPACITY, colour: COLOUR.to_string(), mode: Mode::Translation, place: None, hidden: false }
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
            hidden: saved["hidden"] == true,
        }
    }

    fn to_json(&self) -> Value {
        let mode = Mode::ALL.iter().find(|m| m.0 == self.mode).unwrap().1;
        json!({"font": self.font, "opacity": self.opacity, "colour": self.colour, "mode": mode, "place": self.place.map(|p| [p.0, p.1, p.2]), "hidden": self.hidden})
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
    /// said in place of a message until the next one comes
    notice: Option<Said>,
    /// said beside the message, which is still shown
    hint: Option<Said>,
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
                let lines = event["lines"].as_array().into_iter().flatten().filter_map(text).collect();
                let pending = event["translating"] == true;
                let pictured = event["source"] == "ocr";
                self.messages.push(Message { seq, name: text(&event["name"]), lines, translation: None, pending, pictured, google: false });
                if self.messages.len() > KEPT {
                    self.messages.remove(0);
                    }
                // a new message brings the strip back from the old ones
                self.at = self.messages.len() - 1;
                self.notice = None;
                self.hint = None;
            }
            Some("translation") => {
                if let Some(message) = self.messages.iter_mut().rev().find(|m| m.seq == seq) {
                    (message.translation, message.pending) = (text(&event["tr"]), false);
                    message.google = event["via"] == "google";
                }
                // what failed before has been made good
                self.notice = None;
                self.hint = None;
            }
            Some(kind @ ("notice" | "hint")) => {
                let said = text(&event["text"]).map(|text| Said { text, action: event["action"].as_str().and_then(Action::named) });
                if kind == "notice" {
                    self.notice = said;
                } else {
                    self.hint = said;
                }
            }
            _ => {}
        }
    }

    /// Who says the message shown.
    fn speaker(&self) -> Option<&str> {
        self.messages.get(self.at).filter(|_| self.notice.is_none())?.name.as_deref()
    }

    /// Where the message shown comes from, in a word for the strip.
    fn source(&self) -> Option<String> {
        let message = self.messages.get(self.at).filter(|_| self.notice.is_none())?;
        let text = if message.pictured { "OCR" } else { "ENGINE" };
        Some(match self.old() {
            Some((at, of)) => format!("{text} · {at}/{of}"),
            None => text.to_string(),
        })
    }

    /// The message shown among those there are, while it is an old one.
    fn old(&self) -> Option<(usize, usize)> {
        (self.at + 1 < self.messages.len()).then_some((self.at + 1, self.messages.len()))
    }

    /// What is said beside the message: a hint, or the button of a notice,
    /// and what the button does.
    fn offered(&self) -> Option<(String, Option<Action>)> {
        match (&self.notice, &self.hint) {
            (Some(Said { action: Some(action), .. }), _) => Some((String::new(), Some(*action))),
            (Some(_), _) => None,
            (None, Some(hint)) => Some((hint.text.clone(), hint.action)),
            (None, None) => None,
        }
    }

    /// Whether Google Translate made the translation shown.
    fn by_google(&self) -> bool {
        self.messages.get(self.at).is_some_and(|m| m.google && self.notice.is_none())
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
        if let Some(notice) = &self.notice {
            return (None, Some(notice.text.clone()), true);
        }
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
    /// locked, the strip lets clicks through; unlocked, it can be moved,
    /// made wider and its text selected
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
    content: gtk4::Box,
    /// the name of who speaks, set apart from what is said
    speaker: gtk4::Label,
    japanese: gtk4::Label,
    translation: gtk4::Label,
    /// says in small letters where the message comes from
    source: gtk4::Label,
    /// says who translates, and lets another be chosen
    service: gtk4::Button,
    /// the help for the first time
    intro: gtk4::Box,
    /// what is said beside the message, and the button that does something about it
    hinted: gtk4::Label,
    hint: gtk4::Button,
    offer: Cell<Option<Action>>,
    grips: [gtk4::Label; 2],
    /// the meaning of the word under the pointer, from the dictionaries
    popup: gtk4::Window,
    meaning: gtk4::Box,
    scrolled: gtk4::ScrolledWindow,
    asks: Sender<(u64, String)>,
    answers: Receiver<(u64, Result<yomitan::Found, Trouble>)>,
    bar: gtk4::Window,
    hide: gtk4::Button,
    edit: gtk4::Button,
    mode: gtk4::Button,
    /// says on the button which of a message is shown
    mode_now: gtk4::Label,
    read: gtk4::Button,
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
}

impl Tab {
    const ALL: [(Tab, &'static str, &'static str); 4] = [(Tab::Display, "display", "Hiển thị"), (Tab::Translate, "translate", "Dịch"), (Tab::Dictionary, "dictionary", "Từ điển"), (Tab::Ocr, "ocr", "OCR")];

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
                said.extend(failed.drain(..));
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
        self.japanese.set_text(japanese.as_deref().unwrap_or_default());
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
        if source.is_some_and(|s| s.starts_with("OCR")) {
            self.source.add_css_class("pictures");
        } else {
            self.source.remove_css_class("pictures");
        }
        (ui.selection, ui.copied) = (None, None);
        // the text is another: what was found in the old one is gone
        (ui.hover, ui.asked) = (None, None);
        self.japanese.set_attributes(None);
        self.popup.set_visible(false);
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
        if let Some(action) = self.offer.get() {
            self.hint.set_label(action.label());
        }
        self.japanese.set_selectable(!ui.locked);
        self.grips.iter().for_each(|grip| grip.set_visible(!ui.locked));
        self.mode_now.set_text(ui.look.mode.label());
        self.hide.set_icon_name(if ui.look.hidden { "view-conceal-symbolic" } else { "view-reveal-symbolic" });
        switch(&self.hide, ui.look.hidden);
        switch(&self.edit, !ui.locked);
    }

    /// Puts the strip and the bar where they belong by the game's window,
    /// or out of sight while the game is not the window being played.
    fn place(&self) {
        let mut ui = self.ui.borrow_mut();
        let Some((x, y, width, height)) = ui.placed.filter(|_| !ui.picking) else {
            self.strip.set_visible(false);
            self.popup.set_visible(false);
            self.bar.set_visible(false);
            return;
        };
        let monitors = self.display.monitors();
        let screens: Vec<gdk::Rectangle> = monitors.into_iter().flatten().filter_map(|m| m.downcast::<gdk::Monitor>().ok()).map(|m| m.geometry()).collect();
        let right = screens.iter().map(|s| s.x() + s.width()).max();
        let bottom = screens.iter().map(|s| s.y() + s.height()).max();

        let strip = match ui.look.place {
            Some((dx, dy, wide)) => (x + dx, y + dy, wide),
            None => {
                // below the game's window, or at the top of it when there is no room below
                let tall = self.strip.height().max(self.content.measure(gtk4::Orientation::Vertical, width).1);
                let below = y + height + GAP;
                (x, if bottom.is_none_or(|bottom| below + tall <= bottom) { below } else { y + GAP }, width)
            }
        };
        ui.strip = strip;
        self.content.set_size_request(strip.2, -1);
        self.strip.set_default_size(strip.2, -1);
        self.strip.set_margin(Edge::Left, strip.0.max(0));
        self.strip.set_margin(Edge::Top, strip.1.max(0));
        let (japanese, translation, _) = ui.shown.texts(ui.look.mode);
        self.strip.set_visible(!ui.look.hidden && (japanese.is_some() || translation.is_some() || ui.intro));
        if let Some(surface) = self.strip.surface() {
            // Locked, the strip lets the pointer through, unless it shows
            // Japanese: then words are looked up under the pointer.
            let all = cairo::RectangleInt::new(0, 0, self.strip.width(), self.strip.height());
            let through = ui.locked && japanese.is_none() && !ui.intro;
            // ...save for the buttons that say who translates and offer a remedy
            let taking = if through {
                let taking = cairo::Region::create();
                for button in [&self.service, &self.hint] {
                    if let Some(b) = button.compute_bounds(&self.strip).filter(|_| button.is_visible()) {
                        let _ = taking.union_rectangle(&cairo::RectangleInt::new(b.x() as i32, b.y() as i32, b.width().ceil() as i32, b.height().ceil() as i32));
                    }
                }
                taking
            } else {
                cairo::Region::create_rectangle(&all)
            };
            surface.set_input_region(Some(&taking));
        }

        // right of the game's window, or along its right edge when there is no room
        let beside = x + width + GAP;
        let bar = if right.is_none_or(|right| beside + BAR <= right) { beside } else { x + width - BAR - GAP };
        self.bar.set_margin(Edge::Left, bar.max(0));
        self.bar.set_margin(Edge::Top, y.max(0));
        self.bar.set_visible(true);
    }

    fn tick(self: &Rc<Self>) {
        let mut changed = false;
        for event in self.events.try_iter() {
            self.ui.borrow_mut().shown.take(&event);
            changed = true;
        }
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
        let copy = selection.filter(|s| ui.selection == Some(*s) && ui.copied != Some(*s));
        ui.selection = selection;
        if let Some((from, to)) = copy {
            ui.copied = copy;
            let selected: String = self.japanese.text().chars().skip(from as usize).take((to - from) as usize).collect();
            let _ = Command::new("wl-copy").arg("--").arg(selected).spawn();
        }
        self.look_up(&mut ui);
        drop(ui);
        switch(&self.read, self.pictures.on());
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
    }

    /// Looks up the word under the pointer, and shows what was found by the
    /// strip, the word marked in the message.
    fn look_up(self: &Rc<Self>, ui: &mut Ui) {
        // The meaning stays while the pointer is on its word or on the
        // meaning itself, and a moment longer: the way from one to the
        // other leads over neither.
        let at = ui.hover.map(|h| h.0);
        ui.away = if at.is_some() || ui.over_meaning { 0 } else { ui.away + 1 };
        if at.is_some() && at != ui.asked {
            ui.asked = at;
            ui.lookups += 1;
            let text = self.japanese.text();
            let from: String = text.get(at.unwrap()..).unwrap_or_default().chars().take_while(|c| *c != '\n').take(LOOKED_UP).collect();
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
        let bottom = self.display.monitors().into_iter().flatten().filter_map(|m| m.downcast::<gdk::Monitor>().ok()).map(|m| m.geometry().y() + m.geometry().height()).max();
        let below = ui.strip.1 + self.strip.height() + 2;
        let top = if bottom.is_none_or(|bottom| below + tall <= bottom) { below } else { ui.strip.1 - tall - 2 };
        // the least may never be above the most, so the most is let go of first
        self.scrolled.set_max_content_height(-1);
        self.scrolled.set_min_content_height(tall - 20);
        self.scrolled.set_max_content_height(tall - 20);
        self.popup.set_margin(Edge::Left, (ui.strip.0 + x - 24).max(0));
        self.popup.set_margin(Edge::Top, top.max(0));
        self.popup.set_default_size(-1, -1);
        self.popup.set_visible(true);
    }

    /// The menu of the message's text, in place of the toolkit's own, most
    /// of which (cut, paste, delete) can do nothing to text that is only read.
    fn text_menu(self: &Rc<Self>, x: f64, y: f64) {
        let menu = floating("vntext");
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        list.add_css_class("bar");
        list.add_css_class("menu");
        let selected = self.japanese.selection_bounds().filter(|(from, to)| from != to);
        let item = |text: &str, act: Box<dyn Fn(&Overlay)>| {
            let choice = gtk4::Button::with_label(text);
            choice.set_focusable(false);
            let (overlay, menu) = (self.clone(), menu.clone());
            choice.connect_clicked(move |_| {
                act(&overlay);
                menu.destroy();
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
        // text can be selected only while the strip is unlocked
        if self.japanese.is_selectable() {
            item("Chọn tất cả", Box::new(|o| o.japanese.select_region(0, -1)));
        }
        menu.set_child(Some(&list));
        // the menu goes away when the pointer leaves it
        let pointer = gtk4::EventControllerMotion::new();
        let closing = menu.clone();
        pointer.connect_leave(move |_| closing.destroy());
        menu.add_controller(pointer);
        // under the pointer, so that it is on the menu as it opens
        let strip = self.ui.borrow().strip;
        let at = self.japanese.compute_point(&self.strip, &gtk4::graphene::Point::new(x as f32, y as f32));
        let (x, y) = at.map_or((0, 0), |p| (p.x() as i32 - 20, p.y() as i32 - 10));
        menu.set_margin(Edge::Left, (strip.0 + x).max(0));
        menu.set_margin(Edge::Top, (strip.1 + y).max(0));
        menu.set_visible(true);
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
                let told = match translate::choose(Service::Google) {
                    Ok(()) => "Từ câu sau dịch bằng Google Dịch.".to_string(),
                    Err(e) => e,
                };
                self.hub.publish(json!({"type": "notice", "text": told}).to_string());
            }
            Action::Translate => self.set_up(Tab::Translate),
            Action::Dictionary => self.set_up(Tab::Dictionary),
        }
    }

    /// The tab of how the strip looks: the size and colour of its text, how
    /// opaque it is, which of a message it shows, and where it is.
    fn display_page(self: &Rc<Self>) -> gtk4::Box {
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
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
        font.set_value(look.font as f64);
        let overlay = self.clone();
        font.connect_value_changed(move |font| overlay.restyle(|l| l.font = font.value() as i32));
        row("Cỡ chữ", font.upcast_ref());

        let opacity = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, *OPACITIES.start(), *OPACITIES.end(), 0.05);
        opacity.set_value(look.opacity);
        opacity.set_width_request(180);
        let overlay = self.clone();
        opacity.connect_value_changed(move |opacity| overlay.restyle(|l| l.opacity = opacity.value().clamp(*OPACITIES.start(), *OPACITIES.end())));
        row("Độ mờ nền", opacity.upcast_ref());

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

        let reset = gtk4::Button::with_label("Đặt lại vị trí và bề rộng");
        reset.set_halign(gtk4::Align::Start);
        let overlay = self.clone();
        reset.connect_clicked(move |_| overlay.restyle(|l| l.place = None));
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
        let menu = floating("vntext");
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        list.add_css_class("bar");
        list.add_css_class("menu");
        for known in &translate::SERVICES {
            let ready = translate::setup(known.service);
            let lacking = known.service.keyed() && ready.key.is_empty();
            let choice = gtk4::Button::with_label(&if lacking { format!("{} (chưa có key)", known.label) } else { known.label.to_string() });
            choice.set_focusable(false);
            switch(&choice, translate::at_work() == Some(known.service));
            let (overlay, menu, service) = (self.clone(), menu.clone(), known.service);
            choice.connect_clicked(move |_| {
                let told = match translate::choose(service) {
                    Ok(()) => format!("Từ câu sau dịch bằng {}.", service.known().label),
                    Err(e) => e,
                };
                overlay.hub.publish(json!({"type": "notice", "text": told}).to_string());
                menu.destroy();
            });
            list.append(&choice);
        }
        menu.set_child(Some(&list));
        // the list goes away when the pointer leaves it
        let pointer = gtk4::EventControllerMotion::new();
        let closing = menu.clone();
        pointer.connect_leave(move |_| closing.destroy());
        menu.add_controller(pointer);
        // over the button, so that the pointer is on the list as it opens
        let strip = self.ui.borrow().strip;
        let at = self.service.compute_bounds(&self.strip);
        let (x, y) = at.map_or((strip.2 - 140, 0), |b| (b.x() as i32 - 40, b.y() as i32 - 4));
        menu.set_margin(Edge::Left, (strip.0 + x).max(0));
        menu.set_margin(Edge::Top, (strip.1 + y).max(0));
        menu.set_visible(true);
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

    /// Lets the reader drag `grip` to change where the strip is, by `moved`,
    /// which is given how far the pointer went since it was last told.
    fn dragging(self: &Rc<Self>, grip: &gtk4::Label, moved: fn(&mut (i32, i32, i32), i32, i32)) {
        let drag = gtk4::GestureDrag::new();
        let overlay = self.clone();
        drag.connect_drag_begin(move |_, _, _| {
            let mut ui = overlay.ui.borrow_mut();
            // from now on the strip is where the reader puts it
            if let (None, Some((x, y, _, _))) = (ui.look.place, ui.placed) {
                ui.look.place = Some((ui.strip.0 - x, ui.strip.1 - y, ui.strip.2));
            }
        });
        let overlay = self.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            // The grip goes along with the strip, so the pointer is back
            // where it took hold after each step, and how far it is from
            // there is what it has moved since.
            if let Some(place) = overlay.ui.borrow_mut().look.place.as_mut() {
                moved(place, dx.round() as i32, dy.round() as i32);
                place.2 = place.2.max(NARROWEST);
            }
            overlay.place();
        });
        let overlay = self.clone();
        drag.connect_drag_end(move |_, _, _| overlay.save());
        grip.add_controller(drag);
    }

    /// A button of the bar that changes the state and shows it anew.
    fn button(self: &Rc<Self>, icon: &str, tip: &str, act: impl Fn(&Rc<Overlay>) + 'static) -> gtk4::Button {
        let button = pictured(icon, tip);
        self.acting(&button, act);
        button
    }

    /// The settings of the strip, in a small window over the game: who
    /// translates, set up on the first tab (the service shown when it is
    /// saved is the one that translates from then on), and the dictionaries.
    fn set_up(self: &Rc<Self>, tab: Tab) {
        let Some((x, y, width, height)) = self.ui.borrow().placed else { return };
        if self.asking.replace(true) {
            return;
        }
        let window = floating("vntext-ask");
        let overlay = self.clone();
        window.connect_destroy(move |_| overlay.asking.set(false));
        // typing needs the keyboard, which a floating surface otherwise never takes
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let tabs = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        tabs.set_hexpand(true);
        let close = pictured("window-close-symbolic", "Đóng (Esc)");
        close.add_css_class("icon");
        let closing = window.clone();
        close.connect_clicked(move |_| closing.destroy());
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        top.append(&tabs);
        top.append(&close);
        content.append(&top);
        let asked = gtk4::Label::new(Some("Nguồn dịch · Enter để lưu · Esc để đóng"));
        asked.set_xalign(0.0);
        content.append(&asked);
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
        key.set_width_request(360);
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
        let save = gtk4::Button::with_label("Lưu");
        save.set_halign(gtk4::Align::End);
        content.append(&save);
        let pages = gtk4::Stack::new();
        pages.add_named(&self.display_page(), Some(Tab::Display.id()));
        pages.add_named(&content, Some(Tab::Translate.id()));
        pages.add_named(&dictionaries(&window), Some(Tab::Dictionary.id()));
        pages.add_named(&self.ocr_page(&window), Some(Tab::Ocr.id()));
        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        outer.add_css_class("box");
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
        outer.append(&pages);
        window.set_child(Some(&outer));
        pages.set_visible_child_name(tab.id());
        buttons.borrow().iter().for_each(|(t, b)| switch(b, *t == tab));
        window.set_margin(Edge::Left, (x + width / 2 - 230).max(0));
        window.set_margin(Edge::Top, (y + height / 2 - 200).max(0));

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
            tabs.append(&tab);
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
                let told = match translate::save_setup(&setup) {
                    Ok(()) => format!("Đã lưu: dịch bằng {}.", setup.service.known().label),
                    Err(e) => format!("Không lưu được thiết lập dịch: {e}"),
                };
                overlay.hub.publish(json!({"type": "notice", "text": told}).to_string());
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
        key.grab_focus();
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

fn run(events: Receiver<String>, hub: web::Hub, pid: u32, pictures: Arc<ocr::Control>, settings: ocr::Settings, reading: bool) -> Result<(), String> {
    let screen = screen::open()?;
    gtk4::init().map_err(|e| e.to_string())?;
    if !gtk4_layer_shell::is_supported() {
        return Err("the compositor has no layer shell".to_string());
    }
    let display = gdk::Display::default().ok_or("no display")?;
    let style = gtk4::CssProvider::new();
    let swatches: String = COLOURS.iter().enumerate().map(|(at, c)| format!(".vntext-ask button.swatch{at} {{ background: {c}; }}\n")).collect();
    style.load_from_data(&format!("{STYLE}{swatches}"));
    let look_style = gtk4::CssProvider::new();
    let sizes = gtk4::CssProvider::new();
    gtk4::style_context_add_provider_for_display(&display, &sizes, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    gtk4::style_context_add_provider_for_display(&display, &style, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    gtk4::style_context_add_provider_for_display(&display, &look_style, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

    // the strip: a grip to move it by, the text, a grip to make it wider
    let strip = floating("vntext");
    let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    content.add_css_class("strip");
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    text.set_hexpand(true);
    let (speaker, japanese, translation) = (label("speaker"), label("japanese"), label("translation"));
    text.append(&speaker);
    text.append(&japanese);
    text.append(&translation);
    // what is said beside the message, with a button to do something about it
    let (hinted, hint) = (label("hinted"), gtk4::Button::new());
    hint.add_css_class("hint");
    hint.set_focusable(false);
    let hints = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hints.append(&hinted);
    hints.append(&hint);
    text.append(&hints);
    // the first time: three lines of help in place of the message
    let intro = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    for line in ["Rê chuột lên chữ Nhật để tra nghĩa của từ.", "Bấm bánh răng ở cụm nút bên phải cửa sổ game để chỉnh dải, nguồn dịch và từ điển.", "Kéo tay nắm ⠿ để dời dải (mở khoá dải bằng nút bút chì trước)."] {
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
    let grips = [label("grip"), label("grip")];
    grips[0].set_text("⠿");
    grips[0].set_tooltip_text(Some("Kéo để dời dải"));
    grips[1].set_text("↔");
    grips[1].set_tooltip_text(Some("Kéo để đổi bề rộng"));
    // not a label that wraps: there is never room for it, and it would be broken letter by letter
    let source = gtk4::Label::new(None);
    source.add_css_class("source");
    source.set_justify(gtk4::Justification::Right);
    source.set_valign(gtk4::Align::Start);
    content.append(&grips[0]);
    content.append(&text);
    let service = gtk4::Button::new();
    service.add_css_class("service");
    service.set_focusable(false);
    service.set_halign(gtk4::Align::End);
    service.set_tooltip_text(Some("Nguồn đang dịch. Bấm để đổi sang nguồn khác"));
    let side = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    side.set_valign(gtk4::Align::Start);
    side.append(&source);
    side.append(&service);
    content.append(&side);
    content.append(&grips[1]);
    strip.set_child(Some(&content));

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

    let bar = floating("vntext");
    let buttons = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    buttons.add_css_class("bar");
    bar.set_child(Some(&buttons));
    let hide = pictured("view-reveal-symbolic", "Ẩn hoặc hiện dải phụ đề");
    let edit = pictured("document-edit-symbolic", "Mở khoá dải: kéo ⠿ để dời, kéo ↔ để đổi bề rộng, lăn chuột để xem câu cũ, quét chữ Nhật để chép");
    // the one button that says what it is on: an icon and the mode
    let mode = pictured("preferences-desktop-locale-symbolic", "Đổi giữa bản dịch, câu tiếng Nhật, hoặc cả hai");
    let mode_now = gtk4::Label::new(None);
    let both = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    both.append(&gtk4::Image::from_icon_name("preferences-desktop-locale-symbolic"));
    both.append(&mode_now);
    mode.set_child(Some(&both));
    let read = pictured("", "Đọc khung thoại bằng ảnh chụp (dùng khi game không có profile)");
    read.set_label("OCR");

    let game = mem::name(pid);
    let look = saved_looks().get(&game).map(Look::from_json).unwrap_or_default();
    let ui = Ui { shown: Shown::default(), look, locked: true, game: None, placed: None, strip: (0, 0, 0), picking: false, reading, intro: !intro_read(&saved_looks()), selection: None, copied: None, hover: None, asked: None, lookups: 0, over_meaning: false, away: 0 };
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
        content,
        speaker,
        japanese,
        translation,
        source,
        service,
        intro,
        hinted,
        hint,
        offer: Cell::new(None),
        grips,
        popup,
        meaning,
        scrolled,
        asks,
        answers,
        bar,
        hide,
        edit,
        mode,
        mode_now,
        read,
        asking: Cell::new(false),
    });

    overlay.acting(&overlay.hide, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.look.hidden = !ui.look.hidden;
        drop(ui);
        o.save();
    });
    overlay.acting(&overlay.edit, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.locked = !ui.locked;
    });
    overlay.acting(&overlay.mode, |o| {
        let mut ui = o.ui.borrow_mut();
        ui.look.mode = ui.look.mode.next();
        drop(ui);
        o.save();
    });
    overlay.acting(&overlay.read, |o| {
        if o.pictures.on() {
            o.pictures.stop();
        } else {
            o.read_pictures();
        }
    });
    // only what is pressed often is on the bar: the rest is in the settings
    buttons.append(&overlay.hide);
    buttons.append(&overlay.edit);
    buttons.append(&overlay.button("edit-copy-symbolic", "Chép câu đang hiện trên dải vào clipboard (theo chế độ: bản dịch, câu Nhật, hoặc cả hai)", |o| {
        let ui = o.ui.borrow();
        if let Some(text) = ui.shown.copied(ui.look.mode) {
            let _ = Command::new("wl-copy").arg("--").arg(text).spawn();
        }
    }));
    buttons.append(&overlay.mode);
    buttons.append(&overlay.read);
    buttons.append(&overlay.button("emblem-system-symbolic", "Thiết lập: hiển thị, dịch, từ điển, OCR", |o| o.set_up(Tab::Display)));
    // The strip has no window to close, and a reader who came by the
    // launcher has no terminal to stop it from: this is the way back.
    buttons.append(&overlay.button("window-close-symbolic", "Dừng đọc game này: tắt dải nổi, game vẫn chạy. Mở từ cửa sổ chọn game thì quay về đó.", |_| crate::leave(0)));
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

    overlay.dragging(&overlay.grips[0], |place, dx, dy| (place.0, place.1) = (place.0 + dx, place.1 + dy));
    overlay.dragging(&overlay.grips[1], |place, dx, _| place.2 += dx);
    // the pointer on the meaning keeps it open, to be read and scrolled
    let on_meaning = gtk4::EventControllerMotion::new();
    let (entered, left) = (overlay.clone(), overlay.clone());
    on_meaning.connect_enter(move |_, _, _| entered.ui.borrow_mut().over_meaning = true);
    on_meaning.connect_leave(move |_| left.ui.borrow_mut().over_meaning = false);
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
        let in_strip = label.compute_point(&over.strip, &gtk4::graphene::Point::new(x as f32, y as f32)).map_or(0, |p| p.x() as i32);
        over.ui.borrow_mut().hover = inside.then_some((index as usize, in_strip));
    });
    let left = overlay.clone();
    pointer.connect_leave(move |_| left.ui.borrow_mut().hover = None);
    overlay.japanese.add_controller(pointer);
    // the wheel over the strip steps back through the messages and forward again
    let wheel = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
    let scrolled = overlay.clone();
    wheel.connect_scroll(move |_, _, dy| {
        let mut ui = scrolled.ui.borrow_mut();
        ui.shown.step(dy.signum() as isize);
        drop(ui);
        scrolled.render();
        glib::Propagation::Stop
    });
    overlay.content.add_controller(wheel);

    overlay.render();
    glib::timeout_add_local(TICK, move || {
        overlay.tick();
        glib::ControlFlow::Continue
    });
    glib::MainLoop::new(None, false).run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_and_translations() {
        let mut shown = Shown::default();
        shown.take(r#"{"type":"message","seq":3,"name":"誰","lines":["一","二"],"translating":false}"#);
        assert_eq!(shown.texts(Mode::Translation), (Some("一\n二".to_string()), None, false));
        assert_eq!(shown.speaker(), Some("誰"));
        assert_eq!(shown.source().as_deref(), Some("ENGINE"));
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
        assert_eq!(shown.source().as_deref(), Some("ENGINE · 2/3"));
        // a new message brings the reader back to the newest
        shown.take(r#"{"type":"message","seq":4,"lines":["4"]}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("4"));
        assert_eq!(shown.old(), None);
        assert_eq!(shown.source().as_deref(), Some("ENGINE"));
        shown.step(9);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("4"));
        shown.step(-9);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("1"));
    }

    #[test]
    fn notices() {
        let mut shown = Shown::default();
        shown.take(r#"{"type":"notice","text":"oops"}"#);
        assert_eq!(shown.texts(Mode::Japanese), (None, Some("oops".to_string()), true));
        shown.take(r#"{"type":"message","seq":1,"lines":["一"]}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("一"));
        shown.take(r#"{"type":"message","seq":2,"source":"ocr","lines":["二"]}"#);
        assert_eq!(shown.source().as_deref(), Some("OCR"));
        shown.take(r#"{"type":"translation","seq":2,"tr":"hai","via":"google"}"#);
        assert!(shown.by_google());
        assert_eq!(shown.copied(Mode::Both).as_deref(), Some("二\nhai"));
        assert_eq!(shown.copied(Mode::Translation).as_deref(), Some("hai"));
    }

    #[test]
    fn looks_round_trip() {
        let look = Look { font: 30, opacity: 0.4, colour: "#a6e3a1".to_string(), mode: Mode::Both, place: Some((10, -20, 640)), hidden: true };
        assert_eq!(Look::from_json(&look.to_json()), look);
        assert_eq!(Look::from_json(&json!({"font": 500, "mode": "?", "place": [1, 2]})), Look::default());
    }

    #[test]
    fn looks_saved_before_opacity_and_colour() {
        // a file from before the two were there, and one with them out of range or not a colour
        let old = Look::from_json(&json!({"font": 30, "mode": "both", "place": [1, 2, 300], "hidden": true}));
        assert_eq!((old.font, old.opacity, old.colour.as_str()), (30, OPACITY, COLOUR));
        assert_eq!(old.place, Some((1, 2, 300)));
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
        // a notice takes the place of the message, and only its button is beside it
        shown.take(r#"{"type":"notice","text":"key sai","action":"translate"}"#);
        assert_eq!(shown.texts(Mode::Japanese), (None, Some("key sai".to_string()), true));
        assert_eq!(shown.offered(), Some((String::new(), Some(Action::Translate))));
        shown.take(r#"{"type":"notice","text":"mạng"}"#);
        assert_eq!(shown.offered(), None);
        shown.take(r#"{"type":"message","seq":2,"lines":["二"]}"#);
        assert_eq!(shown.texts(Mode::Japanese).0.as_deref(), Some("二"));
    }

    #[test]
    fn modes_have_names() {
        assert_eq!(Mode::Both.name(), "Cả hai");
        assert_eq!(Mode::ALL.map(|m| m.0.name()).len(), 3);
    }
}
