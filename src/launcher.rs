//! The window that opens when vntext is started without arguments: the
//! games that are running, what to do about the one chosen, and a button
//! that starts reading it.
//!
//! Reading itself is left to a child process, this program run again with
//! the arguments of `auto` or `ocr`: both go on until the game is gone and
//! end the process when they do.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::glib;
use serde_json::{json, Value};

use crate::screen::{by_process, Screen};
use crate::translate::{self, Service};
use crate::{doctor, engines, explain, install, mem, ocr};

const TICK: Duration = Duration::from_millis(200);
/// Ticks between two looks at the windows on screen.
const REFRESH: u32 = 10;
/// Languages to translate to: the code the translators take, and its name.
const LANGUAGES: [(&str, &str); 10] = [
    ("vi", "Tiếng Việt"),
    ("en", "English"),
    ("fr", "Français"),
    ("es", "Español"),
    ("pt", "Português"),
    ("de", "Deutsch"),
    ("ru", "Русский"),
    ("id", "Bahasa Indonesia"),
    ("zh", "中文"),
    ("ko", "한국어"),
];

/// How the reader wants one game read.
#[derive(Clone, PartialEq, Debug)]
struct Options {
    translate: bool,
    to: String,
    via: Service,
    /// serve the page for Yomitan as well
    web: bool,
}

impl Options {
    /// The options saved for a game; what is missing from them, or all of
    /// them for a game never opened, is filled in with `fresh`.
    fn from_json(saved: &Value, fresh: &Options) -> Options {
        Options {
            translate: saved["translate"].as_bool().unwrap_or(fresh.translate),
            to: saved["to"].as_str().filter(|to| LANGUAGES.iter().any(|l| l.0 == *to)).map_or_else(|| fresh.to.clone(), str::to_string),
            via: saved["via"].as_str().and_then(Service::named).unwrap_or(fresh.via),
            web: saved["web"].as_bool().unwrap_or(fresh.web),
        }
    }

    fn to_json(&self) -> Value {
        json!({"translate": self.translate, "to": self.to, "via": self.via.known().id, "web": self.web})
    }

    /// The arguments that have this program read the game `pid`: from its
    /// profile, or from pictures of the text box when it has none.
    fn command(&self, pid: u32, profile: bool) -> Vec<String> {
        let mut args = vec![if profile { "auto" } else { "ocr" }.to_string(), pid.to_string(), "--overlay".to_string()];
        if self.translate {
            args.extend(["--translate", "--to", &self.to, "--via", self.via.known().id].map(str::to_string));
        }
        if self.web {
            args.push("--web".to_string());
        }
        args
    }
}

/// What a game is read with unless the reader says otherwise: for one who
/// has no key to translate with, and has not chosen a service of their own
/// (`chosen`), translated by Google, which needs none.
fn fresh_for(chosen: Option<Service>, any_key: bool) -> Options {
    if !any_key && chosen != Some(Service::Local) {
        return Options { translate: true, to: "vi".to_string(), via: Service::Google, web: false };
    }
    Options { translate: false, to: "vi".to_string(), via: chosen.unwrap_or(Service::Gemini), web: false }
}

fn fresh() -> Options {
    let any_key = translate::SERVICES.iter().any(|k| k.service.keyed() && !translate::setup(k.service).key.is_empty());
    fresh_for(translate::explicit(), any_key)
}

/// The file of options: a JSON object with the options of each game, by the
/// name of its process, as the text boxes and looks are kept.
fn games_file() -> PathBuf {
    ocr::config_dir("XDG_CONFIG_HOME", ".config").join("games.json")
}

fn saved_games() -> Value {
    let file = std::fs::read_to_string(games_file()).unwrap_or_default();
    serde_json::from_str(&file).ok().filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn options_of(game: &str) -> Options {
    Options::from_json(&saved_games()[game], &fresh())
}

fn save_options(game: &str, options: &Options) {
    let mut games = saved_games();
    games[game] = options.to_json();
    let file = games_file();
    let _ = std::fs::create_dir_all(file.parent().unwrap());
    if let Err(e) = std::fs::write(&file, format!("{games:#}\n")) {
        eprintln!("cannot save the options of the game to {}: {e}", file.display());
    }
}

/// What is known of the engine of a game.
#[derive(Clone, PartialEq, Debug)]
enum Kind {
    Looking,
    /// a profile reads it, named so
    Profile(String),
    Unknown,
    /// the process could not be looked into
    Failed(String),
}

impl Kind {
    fn said(&self) -> String {
        match self {
            Kind::Looking => "đang nhận…".to_string(),
            Kind::Profile(engine) => engine.clone(),
            Kind::Unknown => "chưa có profile".to_string(),
            Kind::Failed(e) => e.clone(),
        }
    }
}

/// What a game that cannot be looked into because of the system's permissions says.
const DENIED: &str = "bị từ chối đọc bộ nhớ game, xem mục kiểm tra ở trên";

fn recognise(pid: u32) -> Kind {
    match mem::Proc::open(pid) {
        Ok(p) => engines::detect(&p).map_or(Kind::Unknown, |engine| Kind::Profile(engine.name())),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Kind::Failed(DENIED.to_string()),
        Err(e) => Kind::Failed(format!("không mở được tiến trình: {e}")),
    }
}

/// The reading of a game has ended.
struct Exit {
    pid: u32,
    /// the last thing it said, if it ended in failure
    error: Option<String>,
}

/// A line of the child's output without the colours it was written in.
fn plain(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for c in chars.by_ref().skip(1) {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

struct Row {
    pid: u32,
    row: gtk4::ListBoxRow,
    title: gtk4::Label,
    detail: gtk4::Label,
    /// the name of the process, which is what a game is kept by
    game: String,
}

struct Launcher {
    window: gtk4::Window,
    screen: Screen,
    list: gtk4::ListBox,
    empty: gtk4::Label,
    rows: RefCell<Vec<Row>>,
    kinds: RefCell<HashMap<u32, Kind>>,
    found: (Sender<(u32, Kind)>, Receiver<(u32, Kind)>),
    ended: (Sender<Exit>, Receiver<Exit>),
    /// what is wrong with the machine, one block for each thing
    checks: gtk4::Box,
    checked: (Sender<doctor::Env>, Receiver<doctor::Env>),
    allowed: (Sender<Result<(), String>>, Receiver<Result<(), String>>),
    options: gtk4::Box,
    translate: gtk4::Switch,
    language: gtk4::DropDown,
    service: gtk4::DropDown,
    web: gtk4::Switch,
    start: gtk4::Button,
    note: gtk4::Label,
    status: gtk4::Label,
    /// the options are being put into the widgets, not chosen
    loading: Cell<bool>,
    ticks: Cell<u32>,
}

fn label(text: &str, classes: &[&str]) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    classes.iter().for_each(|class| label.add_css_class(class));
    label
}

fn margins(widget: &impl IsA<gtk4::Widget>, vertical: i32, horizontal: i32) {
    widget.set_margin_top(vertical);
    widget.set_margin_bottom(vertical);
    widget.set_margin_start(horizontal);
    widget.set_margin_end(horizontal);
}

/// A row of the options: what it is for, and the widget that sets it.
fn setting(name: &str, widget: &impl IsA<gtk4::Widget>) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let named = label(name, &[]);
    named.set_hexpand(true);
    named.set_valign(gtk4::Align::Center);
    widget.set_valign(gtk4::Align::Center);
    row.append(&named);
    row.append(widget);
    row
}

impl Launcher {
    fn selected(&self) -> Option<u32> {
        self.list.selected_row().and_then(|row| row.widget_name().parse().ok())
    }

    fn kind_of(&self, pid: u32) -> Kind {
        self.kinds.borrow().get(&pid).cloned().unwrap_or(Kind::Looking)
    }

    /// Lists the windows on screen anew: games that have appeared get a
    /// row, and a look into their engine; rows of games that are gone go.
    fn refresh(self: &Rc<Self>) {
        let now = by_process(self.screen.windows(), std::process::id());
        self.rows.borrow_mut().retain(|row| {
            let there = now.iter().any(|w| w.pid == row.pid);
            if !there {
                self.list.remove(&row.row);
                self.kinds.borrow_mut().remove(&row.pid);
            }
            there
        });
        for window in &now {
            let known = self.rows.borrow().iter().any(|r| r.pid == window.pid);
            if !known {
                self.add(window.pid, &window.title);
            }
            if let Some(row) = self.rows.borrow().iter().find(|r| r.pid == window.pid) {
                if row.title.text() != window.title {
                    row.title.set_text(&window.title);
                }
            }
        }
        self.empty.set_visible(now.is_empty());
        if self.selected().is_none() {
            if let Some(first) = self.list.row_at_index(0) {
                self.list.select_row(Some(&first));
            }
        }
        self.show_options();
    }

    fn add(self: &Rc<Self>, pid: u32, title: &str) {
        let game = mem::name(pid);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let (title, detail) = (label(title, &["heading"]), label("", &["dim-label"]));
        content.append(&title);
        content.append(&detail);
        margins(&content, 8, 12);
        let row = gtk4::ListBoxRow::new();
        row.set_child(Some(&content));
        row.set_widget_name(&pid.to_string());
        self.list.append(&row);
        detail.set_text(&format!("{game} · {}", Kind::Looking.said()));
        self.rows.borrow_mut().push(Row { pid, row, title, detail, game });
        // looking into a process takes seconds, so it is done off this thread
        let found = self.found.0.clone();
        thread::spawn(move || {
            let _ = found.send((pid, recognise(pid)));
        });
    }

    /// Takes what the lookups and the readings have to tell.
    fn tick(self: &Rc<Self>) {
        let looked: Vec<(u32, Kind)> = self.found.1.try_iter().collect();
        for (pid, kind) in looked {
            if let Some(row) = self.rows.borrow().iter().find(|r| r.pid == pid) {
                row.detail.set_text(&format!("{} · {}", row.game, kind.said()));
                self.kinds.borrow_mut().insert(pid, kind);
            }
        }
        if let Some(env) = self.checked.1.try_iter().last() {
            self.show_checks(&env);
        }
        if let Some(allowed) = self.allowed.1.try_iter().last() {
            match allowed {
                Ok(()) => {
                    self.status.set_text("Đã cho phép đọc bộ nhớ game.");
                    self.recognise_again();
                }
                Err(e) => self.status.set_text(&e),
            }
            self.check();
        }
        let ended: Vec<Exit> = self.ended.1.try_iter().collect();
        for exit in ended {
            self.returned(exit);
        }
        if self.window.is_visible() {
            let ticks = self.ticks.get();
            self.ticks.set(ticks + 1);
            if ticks % REFRESH == 0 {
                self.refresh();
            }
            self.show_options();
        }
    }

    /// Checks the machine, off this thread: the answer comes back to `tick`.
    fn check(self: &Rc<Self>) {
        let layer_shell = gtk4_layer_shell::is_supported();
        let found = self.checked.0.clone();
        thread::spawn(move || {
            let _ = found.send(doctor::gather(layer_shell));
        });
    }

    /// Lists what is wrong with the machine, how to put it right, and a
    /// button where one can.
    fn show_checks(self: &Rc<Self>, env: &doctor::Env) {
        while let Some(old) = self.checks.first_child() {
            self.checks.remove(&old);
        }
        let wrong: Vec<doctor::Check> = doctor::checks(env).into_iter().filter(|c| !c.ok).collect();
        self.checks.set_visible(!wrong.is_empty());
        for check in wrong {
            let block = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
            block.add_css_class("card");
            margins(&block, 8, 12);
            block.append(&label(check.name, &["heading"]));
            block.append(&label(&check.said, &[]));
            block.append(&label(&check.fix, &["dim-label"]));
            if check.allowable {
                let allow = gtk4::Button::with_label("Cho phép");
                allow.set_halign(gtk4::Align::Start);
                let launcher = self.clone();
                allow.connect_clicked(move |_| launcher.allow());
                block.append(&allow);
            }
            self.checks.append(&block);
        }
    }

    /// Explains what allowing the reading of memory means, and if the reader
    /// agrees, has the system do it (it asks for the password).
    fn allow(self: &Rc<Self>) {
        let dialog = gtk4::AlertDialog::builder()
            .message("Cho phép tool đọc bộ nhớ của game?")
            .detail("Thiết lập này cho mọi chương trình của cùng người dùng đọc bộ nhớ của nhau; trên Arch nó là mặc định. Tool sẽ ghi kernel.yama.ptrace_scope = 0 vào /etc/sysctl.d/90-vntext-ptrace.conf và áp dụng ngay. Hệ thống sẽ hỏi mật khẩu.")
            .buttons(["Huỷ", "Cho phép"])
            .cancel_button(0)
            .default_button(1)
            .modal(true)
            .build();
        let answered = self.allowed.0.clone();
        dialog.choose(Some(&self.window), gtk4::gio::Cancellable::NONE, move |chosen| {
            if chosen.is_ok_and(|button| button == 1) {
                thread::spawn(move || {
                    let _ = answered.send(doctor::allow_ptrace());
                });
            }
        });
    }

    /// Looks into the games that could not be looked into before.
    fn recognise_again(self: &Rc<Self>) {
        let pids: Vec<u32> = self.rows.borrow().iter().map(|r| r.pid).filter(|pid| matches!(self.kind_of(*pid), Kind::Failed(_))).collect();
        for pid in pids {
            self.kinds.borrow_mut().remove(&pid);
            if let Some(row) = self.rows.borrow().iter().find(|r| r.pid == pid) {
                row.detail.set_text(&format!("{} · {}", row.game, Kind::Looking.said()));
            }
            let found = self.found.0.clone();
            thread::spawn(move || {
                let _ = found.send((pid, recognise(pid)));
            });
        }
    }

    /// Puts into the widgets what is saved for the game selected, and says
    /// what the main button does for it.
    fn show_options(self: &Rc<Self>) {
        let rows = self.rows.borrow();
        let Some(row) = self.selected().and_then(|pid| rows.iter().find(|r| r.pid == pid)) else {
            self.options.set_sensitive(false);
            self.start.set_label("Đọc game này");
            self.note.set_visible(false);
            return;
        };
        self.options.set_sensitive(true);
        let kind = self.kind_of(row.pid);
        let (text, note) = match &kind {
            Kind::Profile(_) => ("Đọc game này", None),
            Kind::Looking => ("Đang nhận engine…", None),
            _ => ("Đọc bằng OCR", Some("Game này chưa có profile nên tool đọc khung thoại từ ảnh chụp. Cách này cần key Gemini (nhập ở bánh răng trên dải nổi), và lần đầu sẽ hỏi bạn khoanh khung thoại.")),
        };
        self.start.set_label(text);
        self.start.set_sensitive(kind != Kind::Looking);
        self.note.set_visible(note.is_some());
        self.note.set_text(note.unwrap_or_default());
        // the widgets are filled only when the game changes: the reader may be in the middle of choosing
        if self.options.widget_name() == row.game {
            return;
        }
        self.options.set_widget_name(&row.game);
        self.loading.set(true);
        let options = options_of(&row.game);
        self.translate.set_active(options.translate);
        self.web.set_active(options.web);
        self.language.set_selected(LANGUAGES.iter().position(|l| l.0 == options.to).unwrap_or(0) as u32);
        self.service.set_selected(translate::SERVICES.iter().position(|k| k.service == options.via).unwrap_or(0) as u32);
        self.loading.set(false);
        self.translated();
    }

    /// The services as the reader can choose them; one without the key it
    /// needs says so.
    fn services(&self) {
        let names: Vec<String> = translate::SERVICES
            .iter()
            .map(|k| if k.service.keyed() && translate::setup(k.service).key.is_empty() { format!("{} (chưa có key)", k.label) } else { k.label.to_string() })
            .collect();
        let at = self.service.selected();
        self.service.set_model(Some(&gtk4::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>())));
        self.service.set_selected(at);
    }

    /// Greys out the language and the service while nothing is translated.
    fn translated(&self) {
        self.language.set_sensitive(self.translate.is_active());
        self.service.set_sensitive(self.translate.is_active());
    }

    /// The options as the widgets have them now.
    fn chosen(&self) -> Options {
        let language = LANGUAGES.get(self.language.selected() as usize).unwrap_or(&LANGUAGES[0]);
        let service = translate::SERVICES.get(self.service.selected() as usize).map_or_else(translate::chosen, |k| k.service);
        Options { translate: self.translate.is_active(), to: language.0.to_string(), via: service, web: self.web.is_active() }
    }

    /// Keeps what the reader has chosen for the game selected.
    fn keep(self: &Rc<Self>) {
        self.translated();
        if self.loading.get() {
            return;
        }
        let rows = self.rows.borrow();
        if let Some(row) = self.selected().and_then(|pid| rows.iter().find(|r| r.pid == pid)) {
            save_options(&row.game, &self.chosen());
        }
    }

    /// Runs this program again to read the game selected, and gives way to
    /// its overlay until it ends.
    fn read(self: &Rc<Self>) {
        let Some(pid) = self.selected() else { return };
        let kind = self.kind_of(pid);
        if kind == Kind::Looking {
            return;
        }
        let args = self.chosen().command(pid, matches!(kind, Kind::Profile(_)));
        let spawned = std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| {
            let mut command = Command::new(exe);
            command.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
            // the reader goes when the launcher does, however that happens
            unsafe {
                command.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
            command.spawn().map_err(|e| e.to_string())
        });
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => return self.status.set_text(&format!("Không chạy được phần đọc: {e}")),
        };
        self.status.set_text("");
        self.window.set_visible(false);
        let ended = self.ended.0.clone();
        let output = child.stderr.take();
        thread::spawn(move || {
            let mut last = String::new();
            for line in BufReader::new(output.unwrap()).lines().map_while(Result::ok) {
                eprintln!("{line}");
                let line = plain(&line);
                if !line.is_empty() {
                    last = line;
                }
            }
            let failed = child.wait().map_or(true, |status| !status.success());
            let _ = ended.send(Exit { pid, error: failed.then_some(last) });
        });
    }

    /// The reading ended: the window comes back, and says why if it was not
    /// because the game was closed.
    fn returned(self: &Rc<Self>, exit: Exit) {
        let alive = std::path::Path::new(&format!("/proc/{}", exit.pid)).exists();
        match exit.error.filter(|_| alive) {
            Some(error) => self.status.set_text(&format!("Phần đọc dừng lại. {}", explain::explain(&error).text)),
            None if alive => self.status.set_text("Phần đọc đã dừng."),
            None => self.status.set_text("Game đã đóng."),
        }
        self.services();
        self.window.present();
        self.refresh();
        self.check();
    }
}

fn build() -> Option<gtk4::Window> {
    let screen = match Screen::open() {
        Ok(screen) => screen,
        Err(e) => {
            eprintln!("{e}");
            return None;
        }
    };
    let window = gtk4::Window::new();
    window.set_title(Some("vntext"));
    window.set_default_size(460, 600);
    let page = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    margins(&page, 14, 14);
    page.append(&label("Chọn game đang chạy để đọc", &["title-3"]));
    let checks = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    checks.set_visible(false);
    page.append(&checks);
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Browse);
    list.add_css_class("boxed-list");
    let empty = label("Chưa thấy cửa sổ game nào. Mở game lên, danh sách sẽ tự cập nhật.", &["dim-label"]);
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_vexpand(true);
    scroll.set_min_content_height(160);
    page.append(&scroll);
    page.append(&empty);

    let options = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    let translate_on = gtk4::Switch::new();
    let language = gtk4::DropDown::from_strings(&LANGUAGES.map(|l| l.1));
    let service = gtk4::DropDown::from_strings(&[]);
    let web = gtk4::Switch::new();
    options.append(&setting("Dịch", &translate_on));
    options.append(&setting("Dịch sang", &language));
    options.append(&setting("Dịch bằng", &service));
    options.append(&setting("Trang web cho Yomitan (127.0.0.1:8777)", &web));
    page.append(&options);

    let start = gtk4::Button::with_label("Đọc game này");
    start.add_css_class("suggested-action");
    start.add_css_class("pill");
    let note = label("", &["dim-label"]);
    let status = label("", &[]);
    let menu = gtk4::Button::with_label("Thêm vào menu ứng dụng");
    menu.set_halign(gtk4::Align::Start);
    page.append(&start);
    page.append(&note);
    page.append(&status);
    page.append(&menu);
    window.set_child(Some(&page));

    let launcher = Rc::new(Launcher {
        window: window.clone(),
        screen,
        list: list.clone(),
        empty,
        rows: RefCell::default(),
        kinds: RefCell::default(),
        found: mpsc::channel(),
        ended: mpsc::channel(),
        checks,
        checked: mpsc::channel(),
        allowed: mpsc::channel(),
        options,
        translate: translate_on.clone(),
        language: language.clone(),
        service: service.clone(),
        web: web.clone(),
        start: start.clone(),
        note,
        status: status.clone(),
        loading: Cell::new(false),
        ticks: Cell::new(0),
    });
    launcher.services();
    let (kept, shown) = (launcher.clone(), launcher.clone());
    list.connect_row_selected(move |_, _| shown.show_options());
    translate_on.connect_active_notify({
        let kept = kept.clone();
        move |_| kept.keep()
    });
    web.connect_active_notify({
        let kept = kept.clone();
        move |_| kept.keep()
    });
    language.connect_selected_notify({
        let kept = kept.clone();
        move |_| kept.keep()
    });
    service.connect_selected_notify(move |_| kept.keep());
    let reading = launcher.clone();
    start.connect_clicked(move |_| reading.read());
    menu.connect_clicked(move |_| match install::all() {
        Ok(done) => {
            let link = match done.link {
                Some(Ok(link)) => format!(" Lệnh vntext cũng có ở {}.", link.display()),
                Some(Err(e)) => format!(" {e}"),
                None => String::new(),
            };
            status.set_text(&format!("Đã thêm vào menu ứng dụng ({}).{link}", done.entry.display()));
        }
        Err(e) => status.set_text(&e),
    });

    launcher.refresh();
    launcher.check();
    let ticking = launcher.clone();
    glib::timeout_add_local(TICK, move || {
        ticking.tick();
        glib::ControlFlow::Continue
    });
    window.present();
    Some(window)
}

pub fn run() {
    // the name the desktop knows the window by: the one of the entry in the menu
    glib::set_prgname(Some("vntext"));
    if let Err(e) = gtk4::init() {
        eprintln!("{e}");
        return;
    }
    let Some(window) = build() else { return };
    let main = glib::MainLoop::new(None, false);
    let closing = main.clone();
    window.connect_close_request(move |_| {
        closing.quit();
        glib::Propagation::Proceed
    });
    main.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_read_and_how() {
        let mut options = Options { translate: false, to: "vi".to_string(), via: Service::Google, web: false };
        assert_eq!(options.command(42, true), ["auto", "42", "--overlay"]);
        assert_eq!(options.command(42, false)[0], "ocr");
        options.translate = true;
        options.web = true;
        assert_eq!(options.command(7, true), ["auto", "7", "--overlay", "--translate", "--to", "vi", "--via", "google", "--web"]);
    }

    #[test]
    fn options_round_trip_and_gaps() {
        let options = Options { translate: true, to: "fr".to_string(), via: Service::Claude, web: true };
        let fresh = Options { translate: false, to: "vi".to_string(), via: Service::Gemini, web: false };
        assert_eq!(Options::from_json(&options.to_json(), &fresh), options);
        // a game never opened, or a file with parts missing or wrong
        assert_eq!(Options::from_json(&Value::Null, &fresh), fresh);
        assert_eq!(Options::from_json(&json!({"translate": true, "to": "xx", "via": "nobody"}), &fresh), Options { translate: true, ..fresh });
    }

    #[test]
    fn those_with_no_key_translate_with_google() {
        let google = Options { translate: true, to: "vi".to_string(), via: Service::Google, web: false };
        assert_eq!(fresh_for(None, false), google);
        // one who set up a service without a key is no different
        assert_eq!(fresh_for(Some(Service::Claude), false), google);
        // one who has a key, or runs a server of their own, is left as they were
        assert_eq!(fresh_for(Some(Service::Claude), true), Options { translate: false, to: "vi".to_string(), via: Service::Claude, web: false });
        assert_eq!(fresh_for(None, true).via, Service::Gemini);
        assert_eq!(fresh_for(Some(Service::Local), false), Options { translate: false, to: "vi".to_string(), via: Service::Local, web: false });
    }

    #[test]
    fn colours_are_taken_out() {
        assert_eq!(plain("\x1b[33mno overlay: x\x1b[0m"), "no overlay: x");
        assert_eq!(plain("\x1b[2m[#12]\x1b[0m  "), "[#12]");
        assert_eq!(plain(""), "");
    }

    #[test]
    fn engines_are_said() {
        assert_eq!(Kind::Looking.said(), "đang nhận…");
        assert_eq!(Kind::Profile("Escude".to_string()).said(), "Escude");
        assert_eq!(Kind::Unknown.said(), "chưa có profile");
    }
}
