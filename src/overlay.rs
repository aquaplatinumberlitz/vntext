//! A strip that floats by the game's window and shows the message the game
//! is on: its translation if there is one, otherwise the message itself.
//!
//! It is a layer-shell surface, the only kind of window that a Wayland
//! compositor lets a program place and keep above the others. Clicks go
//! through it to whatever is below.

use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{cairo, gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde_json::Value;

use crate::screen::Screen;

const TICK: Duration = Duration::from_millis(100);
/// Between the game's window and the strip.
const GAP: i32 = 6;
/// Shown while a message waits for its translation.
const PENDING: &str = "…";
const STYLE: &str = "
window.vntext { background: none; }
.vntext label {
    background: rgba(0, 0, 0, 0.72);
    color: white;
    font-size: 22px;
    padding: 10px 18px;
    border-radius: 10px;
    text-shadow: 0 0 3px black, 0 0 3px black;
}
.vntext label.pending { color: rgba(255, 255, 255, 0.5); }
";

/// Starts the strip for the game `pid`. `events` are those of the web page:
/// `message` with a number, a name and lines, and `translation` for a number.
pub fn start(events: Receiver<String>, pid: u32) {
    thread::spawn(move || {
        if let Err(e) = run(events, pid) {
            eprintln!("\x1b[33mno overlay: {e}\x1b[0m");
        }
    });
}

/// What the strip says: the message the game is on.
#[derive(Default)]
struct Shown {
    seq: u64,
    text: String,
    pending: bool,
}

impl Shown {
    fn take(&mut self, event: &str) {
        let Ok(event) = serde_json::from_str::<Value>(event) else { return };
        let seq = event["seq"].as_u64().unwrap_or(0);
        match event["type"].as_str() {
            Some("message") => {
                let lines: Vec<&str> = event["lines"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let pending = event["translating"].as_bool() == Some(true);
                *self = Shown { seq, text: if pending { PENDING.to_string() } else { lines.join("\n") }, pending };
            }
            // a translation that comes late is for a message the game has left
            Some("translation") if seq == self.seq => {
                (self.text, self.pending) = (event["tr"].as_str().unwrap_or_default().to_string(), false);
            }
            _ => {}
        }
    }
}

fn run(events: Receiver<String>, pid: u32) -> Result<(), String> {
    let screen = Screen::open()?;
    gtk4::init().map_err(|e| e.to_string())?;
    if !gtk4_layer_shell::is_supported() {
        return Err("the compositor has no layer shell".to_string());
    }
    let display = gdk::Display::default().ok_or("no display")?;
    let style = gtk4::CssProvider::new();
    style.load_from_data(STYLE);
    gtk4::style_context_add_provider_for_display(&display, &style, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

    let window = gtk4::Window::new();
    window.add_css_class("vntext");
    window.init_layer_shell();
    window.set_namespace(Some("vntext"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    // placed by its distance from the top left corner of the screen, panels or not
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Left, true);
    window.set_exclusive_zone(-1);
    let label = gtk4::Label::new(None);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    window.set_child(Some(&label));

    let mut shown = Shown::default();
    let mut game = None;
    glib::timeout_add_local(TICK, move || {
        let mut changed = false;
        for event in events.try_iter() {
            shown.take(&event);
            changed = true;
        }
        if changed {
            label.set_text(&shown.text);
            if shown.pending {
                label.add_css_class("pending");
            } else {
                label.remove_css_class("pending");
            }
        }
        game = game.or_else(|| screen.window_of(pid));
        let place = game.and_then(|window| screen.placement(window)).filter(|_| screen.active_pid() == Some(pid));
        let Some((x, y, width, height)) = place.filter(|_| !shown.text.is_empty()) else {
            // the window may have been closed and opened anew
            game = None;
            window.set_visible(false);
            return glib::ControlFlow::Continue;
        };
        // as wide as the game, and as tall as its text needs
        label.set_size_request(width, -1);
        window.set_default_size(width, -1);
        let tall = window.height().max(label.measure(gtk4::Orientation::Vertical, width).1);
        let bottom = display.monitors().into_iter().flatten().filter_map(|m| m.downcast::<gdk::Monitor>().ok()).map(|m| m.geometry().y() + m.geometry().height()).max();
        // below the game's window, or at the top of it when there is no room below
        let below = y + height + GAP;
        let top = if bottom.is_none_or(|bottom| below + tall <= bottom) { below } else { y + GAP };
        window.set_margin(Edge::Left, x);
        window.set_margin(Edge::Top, top);
        window.set_visible(true);
        // nothing of the strip takes the pointer
        if let Some(surface) = window.surface() {
            surface.set_input_region(Some(&cairo::Region::create()));
        }
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
        shown.take(r#"{"type":"message","seq":3,"name":null,"lines":["一","二"],"translating":false}"#);
        assert_eq!((shown.text.as_str(), shown.pending), ("一\n二", false));
        shown.take(r#"{"type":"message","seq":4,"lines":["三"],"translating":true}"#);
        assert_eq!((shown.text.as_str(), shown.pending), (PENDING, true));
        shown.take(r#"{"type":"translation","seq":3,"tr":"late"}"#);
        assert!(shown.pending);
        shown.take(r#"{"type":"translation","seq":4,"tr":"ba"}"#);
        assert_eq!((shown.text.as_str(), shown.pending), ("ba", false));
    }
}
