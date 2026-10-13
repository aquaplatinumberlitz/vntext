//! What the machine needs for vntext to work, checked: each item says
//! whether it is there, in a sentence, and how to put it right.

use std::process::Command;

use crate::dict::store::Store;
use crate::screen::X11;
use crate::{translate, yomitan};

/// The setting of the kernel that lets a program read the memory of another.
const PTRACE: &str = "/proc/sys/kernel/yama/ptrace_scope";
/// What the permission asks of the system: to have it from now on and at once.
const ALLOW: &str = "echo 'kernel.yama.ptrace_scope = 0' > /etc/sysctl.d/90-vntext-ptrace.conf && sysctl -w kernel.yama.ptrace_scope=0";

pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    /// what is the case, in a sentence
    pub said: String,
    /// how to put it right; nothing when it is right
    pub fix: String,
    /// a button can put it right
    pub allowable: bool,
}

/// What was found out about the machine; the checks only read this.
pub struct Env {
    /// `XDG_SESSION_TYPE`
    pub session: Option<String>,
    /// `XDG_CURRENT_DESKTOP`
    pub desktop: String,
    pub layer_shell: bool,
    pub x_server: bool,
    /// what `/proc/sys/kernel/yama/ptrace_scope` holds, if it is there
    pub ptrace: Option<String>,
    /// a game is running with Wine's Wayland driver, so its window is not an X11 one
    pub wayland_game: bool,
    pub wl_copy: bool,
    pub dictionaries: bool,
    pub yomitan: bool,
    /// the services that have a key
    pub keys: Vec<&'static str>,
}

impl Check {
    fn new(name: &'static str, ok: bool, said: &str, fix: &str) -> Check {
        Check { name, ok, said: said.to_string(), fix: if ok { String::new() } else { fix.to_string() }, allowable: false }
    }
}

pub fn checks(env: &Env) -> Vec<Check> {
    let wayland = env.session.as_deref() == Some("wayland");
    let gnome = env.desktop.split(':').any(|d| d.eq_ignore_ascii_case("gnome"));
    let strip = if env.layer_shell {
        Check::new("Dải nổi", true, "Desktop cho phép đặt dải nổi cạnh cửa sổ game.", "")
    } else if gnome {
        Check::new("Dải nổi", false, "GNOME không cho chương trình đặt cửa sổ nổi trên game (thiếu layer-shell), nên dải nổi không chạy được ở đó.", "Dùng KDE Plasma hoặc một desktop wlroots (Sway, Hyprland).")
    } else if !wayland {
        Check::new("Dải nổi", false, "Đây không phải phiên Wayland, mà dải nổi chỉ chạy trên Wayland có layer-shell.", "Đăng xuất rồi chọn phiên Wayland (ví dụ \"Plasma (Wayland)\") ở màn hình đăng nhập.")
    } else {
        Check::new("Dải nổi", false, "Desktop này không có layer-shell nên không đặt được dải nổi.", "Dùng KDE Plasma hoặc một desktop wlroots (Sway, Hyprland).")
    };
    let x = Check::new("Cửa sổ X11 (XWayland)", env.x_server, if env.x_server { "Tới được máy chủ X, nơi cửa sổ game Wine/Proton nằm." } else { "Không tới được máy chủ X, nên tool không tìm được cửa sổ game." }, "Cài và bật XWayland (gói xorg-xwayland trên Arch, xwayland trên Debian/Ubuntu); trong KDE nó bật sẵn.");
    let memory = match env.ptrace.as_deref().map(str::trim) {
        None | Some("0") => Check::new("Đọc bộ nhớ game", true, "Tool đọc được bộ nhớ của tiến trình khác cùng người dùng.", ""),
        Some(value) => {
            let mut memory = Check::new(
                "Đọc bộ nhớ game",
                false,
                &format!("Hệ thống đang cấm đọc bộ nhớ của tiến trình khác (kernel.yama.ptrace_scope = {value}), nên tool không lấy được text của game."),
                "Bấm \"Cho phép\" (hỏi mật khẩu), hoặc chạy: sudo sysctl kernel.yama.ptrace_scope=0 (giữ luôn sau khi khởi động lại: ghi dòng kernel.yama.ptrace_scope = 0 vào /etc/sysctl.d/90-vntext-ptrace.conf).",
            );
            memory.allowable = true;
            memory
        }
    };
    let kde = env.desktop.split(':').any(|d| d.eq_ignore_ascii_case("kde"));
    let window = if env.wayland_game && !kde {
        Check::new("Cửa sổ game Wayland", false, "Có game Wine đang chạy bằng Wayland driver, mà tool chỉ hỏi được cửa sổ Wayland qua KWin, nên ngoài KDE chưa hỗ trợ.", "Tắt Wayland driver của Wine cho game đó (Faugus: bỏ tuỳ chọn Wayland) để nó chạy qua XWayland.")
    } else if env.wayland_game {
        Check::new("Cửa sổ game Wayland", true, "Có game Wine chạy bằng Wayland driver; tool hỏi KWin về cửa sổ của nó.", "")
    } else {
        Check::new("Cửa sổ game Wayland", true, "Không thấy game nào chạy bằng Wayland driver của Wine.", "")
    };
    let copy = Check::new("Nút chép (wl-copy)", env.wl_copy, if env.wl_copy { "Có wl-copy để chép câu vào clipboard." } else { "Không có wl-copy nên nút chép và chép chữ đã chọn không chạy." }, "Cài gói wl-clipboard (Arch: sudo pacman -S wl-clipboard; Debian/Ubuntu: sudo apt install wl-clipboard).");
    let said = match (env.dictionaries, env.yomitan) {
        (true, true) => "Có từ điển đã nhập, và Yomitan cũng trả lời.",
        (true, false) => "Có từ điển đã nhập để tra từ.",
        (false, true) => "Chưa nhập từ điển nào, nhưng Yomitan của trình duyệt đang trả lời.",
        (false, false) => "Chưa có từ điển nào để tra từ: chưa nhập từ điển và Yomitan không trả lời.",
    };
    let lookup = Check::new("Tra từ", env.dictionaries || env.yomitan, said, "Mở hộp cài đặt (bánh răng trên dải nổi), tab \"Từ điển\": tải một từ điển của Yomitan rồi thêm vào. Hoặc bật Yomitan API trong Firefox (xem README).");
    let said = if env.keys.is_empty() { "Chưa có key nào; tool dịch bằng Google Dịch, không cần key.".to_string() } else { format!("Có key của: {}.", env.keys.join(", ")) };
    let translating = Check::new("Dịch", true, &said, "");
    vec![strip, x, memory, window, copy, lookup, translating]
}

/// Finds out about this machine. `layer_shell` is the toolkit's answer, which
/// only the program that has started it can give.
pub fn gather(layer_shell: bool) -> Env {
    let keys = translate::SERVICES.iter().filter(|k| k.service.keyed() && !translate::setup(k.service).key.is_empty()).map(|k| k.label).collect();
    Env {
        session: std::env::var("XDG_SESSION_TYPE").ok(),
        desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        layer_shell,
        x_server: X11::open().is_ok(),
        ptrace: std::fs::read_to_string(PTRACE).ok(),
        wayland_game: wine_on_wayland(),
        wl_copy: on_path("wl-copy"),
        dictionaries: Store::open().is_ok_and(|store| store.ready()),
        yomitan: yomitan::lookup("あ").is_ok(),
        keys,
    }
}

/// Whether a process of this user has Wine's Wayland driver loaded, which
/// is what makes a game's window a Wayland one.
fn wine_on_wayland() -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else { return false };
    procs.flatten().filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit())).any(|e| std::fs::read_to_string(e.path().join("maps")).is_ok_and(|maps| maps.contains("winewayland.drv")))
}

/// Whether a program of that name is in one of the directories of `PATH`.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Allows reading the memory of other programs, now and from the next boot
/// on. The system asks the reader for the password.
pub fn allow_ptrace() -> Result<(), String> {
    let by_hand = "Chạy tay: sudo sysctl kernel.yama.ptrace_scope=0";
    let done = Command::new("pkexec").args(["sh", "-c", ALLOW]).status();
    match done {
        Ok(status) if status.success() => Ok(()),
        // what pkexec says when the window for the password is closed, and when the password is wrong
        Ok(status) if matches!(status.code(), Some(126 | 127)) => Err("Chưa được cấp quyền (đã huỷ hoặc nhập sai mật khẩu).".to_string()),
        Ok(status) => Err(format!("Không đặt được ({status}). {by_hand}")),
        Err(e) => Err(format!("Không chạy được pkexec: {e}. {by_hand}")),
    }
}

/// What `vntext doctor` prints, and whether everything is in order.
pub fn report(env: &Env) -> (String, bool) {
    let checks = checks(env);
    let mut out = String::new();
    for check in &checks {
        out.push_str(&format!("[{}] {}: {}\n", if check.ok { "đạt" } else { "KHÔNG ĐẠT" }, check.name, check.said));
        if !check.fix.is_empty() {
            out.push_str(&format!("      Cách sửa: {}\n", check.fix));
        }
    }
    (out, checks.iter().all(|c| c.ok))
}

pub fn run() {
    // the toolkit can only say whether there is a layer shell once it has started
    let layer_shell = gtk4::init().is_ok() && gtk4_layer_shell::is_supported();
    let (text, ok) = report(&gather(layer_shell));
    print!("{text}");
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fine() -> Env {
        Env { session: Some("wayland".to_string()), desktop: "KDE".to_string(), layer_shell: true, x_server: true, ptrace: Some("0\n".to_string()), wayland_game: false, wl_copy: true, dictionaries: true, yomitan: false, keys: Vec::new() }
    }

    fn failing(env: &Env) -> Vec<&'static str> {
        checks(env).into_iter().filter(|c| !c.ok).map(|c| c.name).collect()
    }

    #[test]
    fn a_machine_that_has_everything() {
        let (text, ok) = report(&fine());
        assert!(ok, "{text}");
        assert!(!text.contains("Cách sửa"));
    }

    #[test]
    fn gnome_cannot_have_the_strip() {
        let env = Env { desktop: "ubuntu:GNOME".to_string(), layer_shell: false, ..fine() };
        let strip = &checks(&env)[0];
        assert!(!strip.ok && strip.said.contains("GNOME") && strip.fix.contains("KDE"));
        // another desktop without it, and a session that is not Wayland, are told apart
        let other = Env { desktop: "XFCE".to_string(), session: Some("x11".to_string()), layer_shell: false, ..fine() };
        assert!(checks(&other)[0].fix.contains("Wayland"));
        let wayland = Env { desktop: "weston".to_string(), layer_shell: false, ..fine() };
        assert!(checks(&wayland)[0].said.contains("layer-shell"));
    }

    #[test]
    fn memory_is_readable_only_at_zero() {
        for (value, ok) in [(Some("0\n"), true), (None, true), (Some("1\n"), false), (Some("2"), false), (Some("3"), false)] {
            let env = Env { ptrace: value.map(str::to_string), ..fine() };
            let memory = &checks(&env)[2];
            assert_eq!((memory.ok, memory.allowable), (ok, !ok), "{value:?}");
        }
        let denied = Env { ptrace: Some("1".to_string()), ..fine() };
        assert!(checks(&denied)[2].said.contains("ptrace_scope = 1"));
        assert!(checks(&denied)[2].fix.contains("Cho phép"));
    }

    #[test]
    fn what_is_missing_is_said() {
        let env = Env { x_server: false, wl_copy: false, dictionaries: false, ..fine() };
        assert_eq!(failing(&env), ["Cửa sổ X11 (XWayland)", "Nút chép (wl-copy)", "Tra từ"]);
        // Yomitan alone is enough to look words up
        assert_eq!(failing(&Env { dictionaries: false, yomitan: true, ..fine() }), Vec::<&str>::new());
        let (text, ok) = report(&env);
        assert!(!ok && text.contains("[KHÔNG ĐẠT] Nút chép (wl-copy)") && text.contains("wl-clipboard"));
    }

    #[test]
    fn wayland_games_need_kwin() {
        let on_kde = Env { wayland_game: true, ..fine() };
        assert!(checks(&on_kde)[3].ok && checks(&on_kde)[3].said.contains("KWin"));
        let elsewhere = Env { wayland_game: true, desktop: "sway".to_string(), ..fine() };
        let window = &checks(&elsewhere)[3];
        assert!(!window.ok && window.fix.contains("Wayland driver"));
        assert_eq!(failing(&elsewhere), ["Cửa sổ game Wayland"]);
        assert!(checks(&fine())[3].ok);
    }

    #[test]
    fn translating_never_fails_for_want_of_a_key() {
        assert!(checks(&fine())[6].said.contains("Google"));
        let keyed = Env { keys: vec!["Gemini", "Claude"], ..fine() };
        assert!(checks(&keyed)[6].ok && checks(&keyed)[6].said.contains("Gemini, Claude"));
    }

    #[test]
    fn programs_on_the_path() {
        assert!(on_path("sh"));
        assert!(!on_path("no-such-program-vntext"));
    }
}
