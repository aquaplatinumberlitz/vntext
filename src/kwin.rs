//! Windows of the desktop as KWin knows them, for those that are not X11
//! windows: where they are and which has the focus (told by a script that
//! KWin runs for us), and pictures of them (asked of KWin's screenshot
//! interface).

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value as Json;
use zbus::blocking::Connection;
use zbus::zvariant::{Fd, OwnedValue, Value};

use crate::screen::{Listed, Picture, Region};

const KWIN: &str = "org.kde.KWin";
const SHOT_PATH: &str = "/org/kde/KWin/ScreenShot2";
const SHOT: &str = "org.kde.KWin.ScreenShot2";

/// What KWin sends of a picture: its size and how its pixels are laid out;
/// the pixels themselves come through a pipe.
#[derive(Debug)]
pub struct Shot {
    pub width: usize,
    pub height: usize,
    /// bytes from the start of one row to the start of the next
    pub stride: usize,
    /// a `QImage::Format`
    pub format: u32,
    pub data: Vec<u8>,
}

fn number(reply: &HashMap<String, OwnedValue>, key: &str) -> Option<usize> {
    let value = reply.get(key)?;
    u32::try_from(value).ok().map(|n| n as usize).or_else(|| u64::try_from(value).ok().map(|n| n as usize)).or_else(|| i32::try_from(value).ok().and_then(|n| usize::try_from(n).ok()))
}

/// Has KWin take a picture of a window: of the one with the handle given,
/// or of the one with the focus if there is none. Without the cursor, the
/// frame and the shadow, and as large as the window really is.
pub fn capture(conn: &Connection, handle: Option<&str>) -> Result<Shot, String> {
    let mut ends = [0i32; 2];
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(format!("cannot make a pipe: {}", std::io::Error::last_os_error()));
    }
    let (mut read, write) = unsafe { (std::fs::File::from_raw_fd(ends[0]), std::fs::File::from_raw_fd(ends[1])) };
    // the picture comes through the pipe while KWin answers, so it is read at once
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        read.read_to_end(&mut data).map(|_| data)
    });
    let options: HashMap<&str, Value> = HashMap::from([("include-cursor", false.into()), ("include-decoration", false.into()), ("include-shadow", false.into()), ("native-resolution", true.into())]);
    let called = match handle {
        Some(handle) => conn.call_method(Some(KWIN), SHOT_PATH, Some(SHOT), "CaptureWindow", &(handle, &options, Fd::from(write.as_fd()))),
        None => conn.call_method(Some(KWIN), SHOT_PATH, Some(SHOT), "CaptureActiveWindow", &(&options, Fd::from(write.as_fd()))),
    };
    // the end that is written to is closed here, which is what lets the reader reach the end
    drop(write);
    let reply = called.map_err(|e| e.to_string());
    let data = reader.join().map_err(|_| "the picture was not read".to_string())?.map_err(|e| e.to_string())?;
    let reply: HashMap<String, OwnedValue> = reply?.body().deserialize().map_err(|e| e.to_string())?;
    let size = |key| number(&reply, key).ok_or_else(|| format!("KWin did not say the {key} of the picture"));
    Ok(Shot { width: size("width")?, height: size("height")?, stride: size("stride")?, format: size("format")? as u32, data })
}


/// Whether KWin lets this program take pictures of windows: `Err` with
/// KWin's words if it refuses. Asked by taking one of the window that has
/// the focus, which is thrown away.
pub fn allowed() -> Result<(), String> {
    let conn = Connection::session().map_err(|e| e.to_string())?;
    match capture(&conn, None) {
        Err(e) if crate::explain::not_authorized(&e) => Err(e),
        // anything else, as no window having the focus, is not a refusal
        _ => Ok(()),
    }
}

/// How the program that runs in KWin tells of the windows: it calls the
/// methods of this object, which keeps what it is told.
const PATH: &str = "/org/vntext/Kwin";
const INTERFACE: &str = "org.vntext.Kwin";
/// How far up the tree of processes a window's process is looked for.
const GENERATIONS: usize = 32;

/// The script KWin runs. It tells `SERVICE` of every window there is and of
/// each change: a window appearing, going, moving or changing size, being
/// minimised, and which has the focus. It sends the area the program in the
/// window draws in, not its frame, so that it is the area a picture of the
/// window shows.
const SCRIPT: &str = r#"
const service = "SERVICE";
function tell(method, text) {
    callDBus(service, "PATH", "INTERFACE", method, text);
}
function send(w) {
    if (!w.internalId) return;
    const g = w.clientGeometry;
    tell("Window", JSON.stringify({id: String(w.internalId), pid: w.pid, title: String(w.caption || ""), x: g.x, y: g.y, w: g.width, h: g.height, minimized: w.minimized}));
}
function track(w) {
    send(w);
    for (const signal of ["frameGeometryChanged", "clientGeometryChanged", "minimizedChanged"]) {
        if (w[signal]) w[signal].connect(function () { send(w); });
    }
}
workspace.windowAdded.connect(track);
workspace.windowRemoved.connect(function (w) { tell("Gone", String(w.internalId)); });
workspace.windowActivated.connect(function (w) { tell("Active", w ? String(w.internalId) : ""); });
workspace.windowList().forEach(track);
tell("Active", workspace.activeWindow ? String(workspace.activeWindow.internalId) : "");
"#;

/// A window as the script told of it.
#[derive(Clone, PartialEq, Debug)]
pub struct Win {
    pub pid: u32,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub minimized: bool,
}

/// What the script has told so far.
#[derive(Default, Debug)]
pub struct State {
    windows: HashMap<String, Win>,
    active: Option<String>,
}

impl State {
    /// Takes what the script says of a window: `{id, pid, title, x, y, w, h, minimized}`.
    pub fn window(&mut self, json: &str) {
        let Ok(told) = serde_json::from_str::<Json>(json) else { return };
        let number = |key: &str| told[key].as_f64().map(|n| n.round() as i32);
        let (Some(id), Some(pid), Some(x), Some(y), Some(width), Some(height)) = (told["id"].as_str(), told["pid"].as_u64(), number("x"), number("y"), number("w"), number("h")) else { return };
        self.windows.insert(id.to_string(), Win { pid: pid as u32, title: told["title"].as_str().unwrap_or_default().to_string(), x, y, width, height, minimized: told["minimized"] == true });
    }

    pub fn gone(&mut self, id: &str) {
        self.windows.remove(id);
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
    }

    pub fn activate(&mut self, id: &str) {
        self.active = Some(id.to_string()).filter(|id| !id.is_empty());
    }

    /// The largest window that is shown and belongs to a process `belongs`
    /// says is the one wanted.
    pub fn of(&self, belongs: impl Fn(u32) -> bool) -> Option<String> {
        let shown = self.windows.iter().filter(|(_, w)| !w.minimized && w.width > 0 && w.height > 0 && belongs(w.pid));
        shown.max_by_key(|(id, w)| (w.width as i64 * w.height as i64, id.as_str())).map(|(id, _)| id.clone())
    }

    /// Where the window is, as `x, y, width, height`; `None` while it is
    /// not shown or not known.
    pub fn placement(&self, id: &str) -> Option<(i32, i32, i32, i32)> {
        let w = self.windows.get(id).filter(|w| !w.minimized)?;
        Some((w.x, w.y, w.width, w.height))
    }

    pub fn active_pid(&self) -> Option<u32> {
        self.windows.get(self.active.as_ref()?).map(|w| w.pid)
    }

    /// The windows shown of the processes `wanted` names, as the launcher
    /// lists windows.
    pub fn listed(&self, wanted: impl Fn(u32) -> bool) -> Vec<Listed> {
        let shown = self.windows.values().filter(|w| !w.minimized && w.width > 0 && w.height > 0 && wanted(w.pid));
        let mut listed: Vec<Listed> = shown.map(|w| Listed { window: 0, pid: w.pid, title: w.title.clone(), area: w.width as u32 * w.height as u32 }).collect();
        listed.sort_by(|a, b| (a.pid, &a.title).cmp(&(b.pid, &b.title)));
        listed
    }
}

/// Whether two processes belong together: one is the other, or above or
/// below it in the tree of processes, or both are Wine's for one prefix.
/// This matters because the process that owns a window need not be the one
/// whose memory is read.
pub fn related(a: u32, b: u32, parent: &dyn Fn(u32) -> Option<u32>, prefix: &dyn Fn(u32) -> Option<String>) -> bool {
    let above = |from: u32, to: u32| {
        let mut at = from;
        for _ in 0..GENERATIONS {
            if at == to {
                return true;
            }
            match parent(at) {
                Some(up) if up > 1 => at = up,
                _ => return false,
            }
        }
        false
    };
    a == b || above(a, b) || above(b, a) || prefix(a).zip(prefix(b)).is_some_and(|(x, y)| !x.is_empty() && x == y)
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // the name is in parentheses and may hold spaces: what follows the last one is fixed
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

fn prefix_of(pid: u32) -> Option<String> {
    let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    env.split(|b| *b == 0).find_map(|var| var.strip_prefix(b"WINEPREFIX=")).map(|p| String::from_utf8_lossy(p).into_owned())
}

/// The object the script calls.
struct Listener(Arc<Mutex<State>>);

#[zbus::interface(name = "org.vntext.Kwin")]
impl Listener {
    fn window(&self, json: String) {
        self.0.lock().unwrap().window(&json);
    }

    fn gone(&self, id: String) {
        self.0.lock().unwrap().gone(&id);
    }

    fn active(&self, id: String) {
        self.0.lock().unwrap().activate(&id);
    }
}

/// What the script was loaded as, and where it was written, to be undone.
struct Loaded {
    conn: Connection,
    plugin: String,
    file: PathBuf,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// Unloads the script from KWin and removes its file. Safe to call again,
/// and from wherever the program ends.
pub fn cleanup() {
    let Some(loaded) = LOADED.lock().unwrap().take() else { return };
    // On a thread of its own: at the end of the program the thread that
    // calls this may have lost what a call over the bus needs.
    let _ = std::thread::spawn(move || {
        unload(&loaded.conn, &loaded.plugin);
        let _ = std::fs::remove_file(&loaded.file);
    })
    .join();
}

fn unload(conn: &Connection, plugin: &str) {
    let _ = conn.call_method(Some(KWIN), "/Scripting", Some("org.kde.kwin.Scripting"), "unloadScript", &(plugin,));
}

fn plugin_name(pid: u32) -> String {
    format!("vntext-{pid}")
}

fn script_file(pid: u32) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("vntext-kwin-{pid}.js"))
}

/// Unloads the scripts that earlier runs left in KWin by dying: files of
/// ours in the runtime directory whose process is gone.
fn clear_leftovers(conn: &Connection) {
    let dir = script_file(0).parent().map(PathBuf::from).unwrap_or_default();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.strip_prefix("vntext-kwin-").and_then(|n| n.strip_suffix(".js")).and_then(|n| n.parse::<u32>().ok()) else { continue };
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            unload(conn, &plugin_name(pid));
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The write end of the pipe the signal handler tells the thread that cleans up through.
static SIGNALS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn signalled(signal: libc::c_int) {
    let fd = SIGNALS.load(std::sync::atomic::Ordering::Relaxed);
    let byte = signal as u8;
    // all a handler may do is write a byte to the pipe
    unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1) };
}

/// The last chance to unload the script. Whatever has ended the program
/// should have called `cleanup` already: by now the thread has lost what
/// much of the standard library needs, so the attempt may fail, and it must
/// not take the program down with it. A script left behind is unloaded by
/// the next run.
extern "C" fn at_exit() {
    let _ = std::panic::catch_unwind(cleanup);
}

/// Has the script unloaded however the program ends: by returning, by
/// `exit`, or by a signal (Ctrl+C, closing the terminal, being asked to stop).
fn unload_on_exit() {
    let mut ends = [0i32; 2];
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return;
    }
    SIGNALS.store(ends[1], std::sync::atomic::Ordering::Relaxed);
    let mut read = unsafe { std::fs::File::from_raw_fd(ends[0]) };
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        if read.read_exact(&mut byte).is_ok() {
            cleanup();
            std::process::exit(128 + byte[0] as i32);
        }
    });
    unsafe {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(signal, signalled as extern "C" fn(libc::c_int) as libc::sighandler_t);
        }
        libc::atexit(at_exit);
    }
}

pub struct Kwin {
    conn: Connection,
    state: Arc<Mutex<State>>,
    /// why the last picture asked for was not taken, if it was not
    refused: Mutex<Option<String>>,
}

impl Kwin {
    /// The one connection to KWin of this program, started the first time it is asked for.
    pub fn shared() -> Result<Arc<Kwin>, String> {
        static KWIN_AT_WORK: OnceLock<Result<Arc<Kwin>, String>> = OnceLock::new();
        KWIN_AT_WORK.get_or_init(|| Kwin::start().map(Arc::new)).clone()
    }

    /// Loads the script into KWin, which tells of the windows from then on.
    fn start() -> Result<Kwin, String> {
        let pid = std::process::id();
        let state: Arc<Mutex<State>> = Arc::default();
        let service = format!("org.vntext.Kwin{pid}");
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(service.as_str()))
            .and_then(|b| b.serve_at(PATH, Listener(state.clone())))
            .and_then(|b| b.build())
            .map_err(|e| format!("cannot reach the session bus: {e}"))?;
        clear_leftovers(&conn);
        let file = script_file(pid);
        std::fs::write(&file, SCRIPT.replace("SERVICE", &service).replace("PATH", PATH).replace("INTERFACE", INTERFACE)).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        let plugin = plugin_name(pid);
        let id = Kwin::load(&conn, &file, &plugin);
        if !id.as_ref().is_ok_and(|id| *id >= 0) {
            let _ = std::fs::remove_file(&file);
            return Err(id.err().unwrap_or_else(|| "KWin did not load the script".to_string()));
        }
        // from here on the script is in KWin, and is taken out when the program ends
        *LOADED.lock().unwrap() = Some(Loaded { conn: conn.clone(), plugin, file });
        unload_on_exit();
        let path = format!("/Scripting/Script{}", id.unwrap());
        if let Err(e) = conn.call_method(Some(KWIN), path.as_str(), Some("org.kde.kwin.Script"), "run", &()) {
            cleanup();
            return Err(format!("KWin did not run the script: {e}"));
        }
        Ok(Kwin { conn, state, refused: Mutex::new(None) })
    }

    /// Has KWin load the script in `file` as `plugin`; gives its number.
    fn load(conn: &Connection, file: &std::path::Path, plugin: &str) -> Result<i32, String> {
        let reply = conn.call_method(Some(KWIN), "/Scripting", Some("org.kde.kwin.Scripting"), "loadScript", &(file.to_string_lossy().as_ref(), plugin)).map_err(|e| format!("KWin: {e}"))?;
        reply.body().deserialize().map_err(|e| e.to_string())
    }

    /// The window of the process `pid`, or of one that belongs with it.
    pub fn window_of(&self, pid: u32) -> Option<String> {
        let state = self.state.lock().unwrap();
        state.of(|p| p == pid).or_else(|| state.of(|p| related(p, pid, &parent_of, &prefix_of)))
    }

    pub fn placement(&self, id: &str) -> Option<(i32, i32, i32, i32)> {
        self.state.lock().unwrap().placement(id)
    }

    pub fn active_pid(&self) -> Option<u32> {
        self.state.lock().unwrap().active_pid()
    }

    /// Whether the window with the focus is one of the process `pid`.
    pub fn active_is(&self, pid: u32) -> bool {
        self.active_pid().is_some_and(|active| related(active, pid, &parent_of, &prefix_of))
    }

    /// The windows shown of the processes named.
    pub fn listed(&self, pids: &[u32]) -> Vec<Listed> {
        self.state.lock().unwrap().listed(|pid| pids.contains(&pid))
    }

    /// Why the last picture asked for was not taken: KWin's own words.
    pub fn refused(&self) -> Option<String> {
        self.refused.lock().unwrap().clone()
    }

    pub fn grab(&self, id: &str, region: Region) -> Option<Picture> {
        let shot = capture(&self.conn, Some(id));
        *self.refused.lock().unwrap() = shot.as_ref().err().cloned();
        let shot = shot.ok()?;
        let (grey, width, height) = grey(&shot, region)?;
        Some(Picture::from_grey(&grey, width, height))
    }
}

/// How long a picture of a whole window takes to be made and sent, which
/// is more than a part of an X11 window: the reader asks less often.
pub const INTERVAL: Duration = Duration::from_millis(300);

/// The region of a picture as 256 greys, and its size. `None` for a format
/// that is not known or a picture that is not as large as it says.
pub fn grey(shot: &Shot, region: Region) -> Option<(Vec<u8>, usize, usize)> {
    // QImage formats of four bytes a pixel: blue, green, red and alpha (or unused) in that
    // order as little-endian words, or red, green, blue and alpha in memory order
    const BGRA: [u32; 3] = [4, 5, 6];
    const RGBA: [u32; 3] = [16, 17, 18];
    let (red, blue) = if BGRA.contains(&shot.format) {
        (2, 0)
    } else if RGBA.contains(&shot.format) {
        (0, 2)
    } else {
        return None;
    };
    if shot.stride < shot.width * 4 || shot.data.len() < shot.stride * shot.height {
        return None;
    }
    let from = |share: f64, of: usize| ((share.clamp(0.0, 1.0) * of as f64) as usize).min(of);
    let (left, top) = (from(region.left, shot.width), from(region.top, shot.height));
    let (width, height) = (from(region.right, shot.width).checked_sub(left)?, from(region.bottom, shot.height).checked_sub(top)?);
    if width == 0 || height == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(width * height);
    for row in top..top + height {
        let line = &shot.data[row * shot.stride + left * 4..row * shot.stride + (left + width) * 4];
        out.extend(line.chunks_exact(4).map(|p| ((p[red] as u32 * 77 + p[1] as u32 * 150 + p[blue] as u32 * 29) >> 8) as u8));
    }
    Some((out, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn told(id: &str, pid: u32, size: (i32, i32), minimized: bool) -> String {
        format!(r#"{{"id":"{id}","pid":{pid},"x":10,"y":20,"w":{},"h":{},"minimized":{minimized}}}"#, size.0, size.1)
    }

    #[test]
    fn windows_come_move_and_go() {
        let mut state = State::default();
        state.window(&told("a", 7, (800, 600), false));
        state.window(&told("b", 7, (100, 100), false));
        state.window(&told("c", 8, (1920, 1080), false));
        // the largest of the process's windows
        assert_eq!(state.of(|p| p == 7).as_deref(), Some("a"));
        assert_eq!(state.placement("a"), Some((10, 20, 800, 600)));
        // it moves and is resized: what is told last is true
        state.window(r#"{"id":"a","pid":7,"x":-5,"y":0.4,"w":640.0,"h":480,"minimized":false}"#);
        assert_eq!(state.placement("a"), Some((-5, 0, 640, 480)));
        // minimised, it is nowhere, and the next window of the process is the one wanted
        state.window(&told("a", 7, (640, 480), true));
        assert_eq!(state.placement("a"), None);
        assert_eq!(state.of(|p| p == 7).as_deref(), Some("b"));
        state.gone("b");
        assert_eq!(state.of(|p| p == 7), None);
        assert_eq!(state.placement("b"), None);
    }

    #[test]
    fn windows_as_the_launcher_lists_them() {
        let mut state = State::default();
        state.window(r#"{"id":"a","pid":7,"title":"Game","x":0,"y":0,"w":800,"h":600,"minimized":false}"#);
        state.window(r#"{"id":"b","pid":8,"title":"Browser","x":0,"y":0,"w":1920,"h":1080,"minimized":false}"#);
        state.window(r#"{"id":"c","pid":9,"title":"Hidden","x":0,"y":0,"w":800,"h":600,"minimized":true}"#);
        let listed = state.listed(|pid| pid == 7 || pid == 9);
        assert_eq!(listed, [Listed { window: 0, pid: 7, title: "Game".to_string(), area: 480000 }]);
        // a script that tells no title still tells of the window
        state.window(&told("d", 7, (10, 10), false));
        assert_eq!(state.listed(|pid| pid == 7).len(), 2);
    }

    #[test]
    fn what_is_not_a_window_is_not_taken() {
        let mut state = State::default();
        for bad in ["", "not json", "{}", r#"{"id":"a","pid":1}"#, r#"{"id":"a","pid":"x","x":0,"y":0,"w":1,"h":1}"#] {
            state.window(bad);
        }
        assert_eq!(state.of(|_| true), None);
        // a window with no size is not one to follow
        state.window(&told("z", 1, (0, 0), false));
        assert_eq!(state.of(|_| true), None);
    }

    #[test]
    fn the_focus() {
        let mut state = State::default();
        state.window(&told("a", 7, (1, 1), false));
        assert_eq!(state.active_pid(), None);
        state.activate("a");
        assert_eq!(state.active_pid(), Some(7));
        // no window has it: the script says so with nothing
        state.activate("");
        assert_eq!(state.active_pid(), None);
        state.activate("a");
        state.gone("a");
        assert_eq!(state.active_pid(), None);
        // a window not told of yet
        state.activate("unknown");
        assert_eq!(state.active_pid(), None);
    }

    #[test]
    fn processes_that_belong_together() {
        // 1 is init; 10 -> 11 -> 12; 20 is on its own; 30 and 31 are elsewhere in the tree
        let parent = |pid: u32| match pid {
            11 => Some(10),
            12 => Some(11),
            10 | 20 | 30 | 31 => Some(1),
            _ => None,
        };
        let prefix = |pid: u32| match pid {
            30 | 31 => Some("/p".to_string()),
            20 => Some(String::new()),
            _ => None,
        };
        assert!(related(12, 12, &parent, &prefix));
        assert!(related(12, 10, &parent, &prefix) && related(10, 12, &parent, &prefix));
        assert!(!related(12, 20, &parent, &prefix));
        // init is not a family
        assert!(!related(10, 20, &parent, &prefix));
        // the same Wine prefix, but not an empty one
        assert!(related(30, 31, &parent, &prefix));
        assert!(!related(20, 21, &parent, &prefix));
    }

    #[test]
    fn the_parent_in_a_stat_line() {
        let own = parent_of(std::process::id()).unwrap();
        assert_eq!(own, std::os::unix::process::parent_id());
    }

    fn shot(format: u32, width: usize, height: usize, stride: usize, pixel: impl Fn(usize, usize) -> [u8; 4]) -> Shot {
        let mut data = vec![0u8; stride * height];
        for y in 0..height {
            for x in 0..width {
                data[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&pixel(x, y));
            }
        }
        Shot { width, height, stride, format, data }
    }

    #[test]
    fn a_region_in_grey() {
        // blue, green, red, alpha; rows are padded to 24 bytes
        let blue_red = shot(6, 4, 3, 24, |x, _| if x < 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
        let whole = Region { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };
        let (all, width, height) = grey(&blue_red, whole).unwrap();
        assert_eq!((width, height, all.len()), (4, 3, 12));
        // blue is dark and red lighter, by the weights of the X11 path
        assert_eq!((all[0], all[3]), (28, 76));
        // the right half of the middle row
        let part = Region { left: 0.5, top: 1.0 / 3.0, right: 1.0, bottom: 2.0 / 3.0 };
        let (right, width, height) = grey(&blue_red, part).unwrap();
        assert_eq!((width, height, right), (2, 1, vec![76, 76]));
        // red first in memory is another format with the same colours
        let rgba = shot(17, 2, 1, 8, |x, _| if x == 0 { [255, 0, 0, 255] } else { [0, 255, 0, 255] });
        assert_eq!(grey(&rgba, whole).unwrap().0, [76, 149]);
        // white is as light as the X11 path makes it
        let white = shot(4, 1, 1, 4, |_, _| [255, 255, 255, 0]);
        assert_eq!(grey(&white, whole).unwrap().0, [255]);
    }

    #[test]
    fn pictures_that_cannot_be_read() {
        let whole = Region { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };
        // a format that is not of four bytes a pixel
        assert!(grey(&shot(24, 2, 2, 8, |_, _| [0; 4]), whole).is_none());
        // less data than the size says
        let mut short = shot(6, 2, 2, 8, |_, _| [0; 4]);
        short.data.truncate(10);
        assert!(grey(&short, whole).is_none());
        // a region with nothing in it, or turned inside out
        let ok = shot(6, 2, 2, 8, |_, _| [0; 4]);
        assert!(grey(&ok, Region { left: 0.5, top: 0.0, right: 0.5, bottom: 1.0 }).is_none());
        assert!(grey(&ok, Region { left: 0.8, top: 0.0, right: 0.2, bottom: 1.0 }).is_none());
    }

    #[test]
    fn the_script_is_filled_in() {
        let text = SCRIPT.replace("SERVICE", "org.vntext.Kwin5").replace("PATH", PATH).replace("INTERFACE", INTERFACE);
        assert!(text.contains(r#"const service = "org.vntext.Kwin5";"#) && text.contains(r#"callDBus(service, "/org/vntext/Kwin", "org.vntext.Kwin", method, text)"#));
        assert!(!text.contains("SERVICE") && !text.contains("INTERFACE"));
    }
}
