//! Errors as the reader should hear them: what went wrong in plain words,
//! and what can be done about it. The raw error still goes to the terminal.

use serde_json::json;

/// Something the reader can be offered to put right what went wrong.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// translate with Google from now on
    Google,
    /// open the settings where a translator is set up
    Translate,
    /// open the settings where dictionaries are
    Dictionary,
}

impl Action {
    const ALL: [(Action, &'static str, &'static str); 3] = [(Action::Google, "google", "Chuyển sang Google"), (Action::Translate, "translate", "Mở thiết lập dịch"), (Action::Dictionary, "dictionary", "Mở tab Từ điển")];

    pub fn id(self) -> &'static str {
        Action::ALL.iter().find(|a| a.0 == self).unwrap().1
    }

    pub fn named(id: &str) -> Option<Action> {
        Action::ALL.iter().find(|a| a.1 == id).map(|a| a.0)
    }

    pub fn label(self) -> &'static str {
        Action::ALL.iter().find(|a| a.0 == self).unwrap().2
    }
}

#[derive(Debug, PartialEq)]
pub struct Explained {
    pub text: String,
    pub action: Option<Action>,
}

/// Says in plain words what a raw error means. What is not known is said
/// as it is, so that nothing is hidden.
pub fn explain(raw: &str) -> Explained {
    let lower = raw.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let (text, action) = if not_authorized(raw) {
        ("KWin chưa cho vntext chụp cửa sổ game, nên OCR không đọc được. Mở cửa sổ chọn game (chạy vntext không tham số), bấm \"Thêm vào menu ứng dụng\", rồi đọc lại game.", None)
    } else if has(&["429", "rate limit", "quota", "resource_exhausted", "too many requests"]) {
        ("Dịch vụ dịch báo đã hết hạn mức (quá nhiều yêu cầu). Đợi một lúc, hoặc dịch bằng Google, không cần key.", Some(Action::Google))
    } else if has(&["401", "403", "unauthorized", "forbidden", "api key", "api_key", "permission_denied", "invalid_api"]) && !has(&["os error 13"]) {
        ("Key của dịch vụ dịch không đúng hoặc đã hết hiệu lực. Nhập lại key ở thiết lập dịch.", Some(Action::Translate))
    } else if has(&["no key", "needs its api key", "name a model", "chưa có api key"]) {
        ("Dịch vụ dịch đã chọn chưa có key hoặc chưa ghi tên model. Thiết lập ở tab Dịch.", Some(Action::Translate))
    } else if has(&["timed out", "timeout", "dns", "connection", "network", "error sending request", "unreachable", "resolve"]) && !has(&["127.0.0.1:19633", "yomitan"]) {
        ("Không nối được mạng, hoặc dịch vụ không trả lời kịp. Kiểm tra kết nối rồi thử lại; tool sẽ tự thử lại.", None)
    } else if has(&["yomitan", "127.0.0.1:19633"]) {
        ("Chưa hỏi được Yomitan. Mở Firefox và bật Yomitan API, hoặc dùng từ điển nhập vào tool.", Some(Action::Dictionary))
    } else if has(&["no x11 window", "x11 window", "cannot reach the x server", "no window that can be pictured"]) {
        ("Không tìm thấy cửa sổ của game. Nếu cửa sổ đang thu nhỏ thì mở nó lên. Game chạy bằng Wayland driver của Wine chỉ được hỗ trợ trên KDE; ở desktop khác hãy tắt driver đó (Faugus: bỏ tuỳ chọn Wayland).", None)
    } else if has(&["permission denied", "os error 13", "not allowed here", "ptrace"]) {
        ("Hệ thống chặn tool đọc bộ nhớ game. Mở cửa sổ chọn game: khung kiểm tra ở đầu có nút \"Cho phép\".", None)
    } else if has(&["target process is gone", "no process matching"]) {
        ("Game đã đóng.", None)
    } else {
        return Explained { text: format!("Có lỗi: {}", raw.trim()), action: None };
    };
    Explained { text: text.to_string(), action }
}

/// Whether an error is KWin refusing to let this program take pictures.
pub fn not_authorized(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("noauthorized") || lower.contains("not authorized")
}

/// The event to send to the strip for a raw error: a `notice`, which takes
/// the place of the message, or a `hint`, which goes beside it.
pub fn event(kind: &str, raw: &str) -> String {
    let said = explain(raw);
    json!({"type": kind, "text": said.text, "action": said.action.map(Action::id)}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(raw: &str) -> (Option<Action>, String) {
        let e = explain(raw);
        (e.action, e.text)
    }

    #[test]
    fn rate_limits_offer_google() {
        for raw in ["rate limit reached (HTTP 429)", "http status: 429", "RESOURCE_EXHAUSTED: quota"] {
            let (action, text) = said(raw);
            assert_eq!(action, Some(Action::Google), "{raw}");
            assert!(text.contains("hết hạn mức"));
        }
    }

    #[test]
    fn bad_keys_open_the_translation_settings() {
        for raw in ["http status: 401", "HTTP 403 Forbidden", "API key not valid. Please pass a valid API key.", "no key for Gemini", "name a model for OpenAI"] {
            assert_eq!(said(raw).0, Some(Action::Translate), "{raw}");
        }
    }

    #[test]
    fn a_lost_network_is_told_without_http() {
        for raw in ["io: timed out", "error sending request for url (https://x)", "dns error: failed to lookup address information", "Connection refused (os error 111)"] {
            let (action, text) = said(raw);
            assert_eq!(action, None, "{raw}");
            assert!(text.contains("mạng") && !text.contains("HTTP"), "{raw}");
        }
    }

    #[test]
    fn yomitan_opens_the_dictionaries() {
        assert_eq!(said("http://127.0.0.1:19633/termEntries: Connection Refused").0, Some(Action::Dictionary));
        assert_eq!(said("yomitan did not answer").0, Some(Action::Dictionary));
    }

    #[test]
    fn the_game_and_the_system() {
        assert!(said("the game has no X11 window").1.contains("cửa sổ của game"));
        assert!(said("OCR: the game has no window that can be pictured").1.contains("cửa sổ của game"));
        // KWin's refusal is not taken for a wrong key
        let refused = said("screenshot: org.kde.KWin.ScreenShot2.Error.NoAuthorized: The process is not authorized to take a screenshot");
        assert!(refused.0.is_none() && refused.1.contains("Thêm vào menu ứng dụng"));
        assert!(said("OCR: the game has no X11 window").1.contains("Wayland driver"));
        let memory = said("cannot open pid 12: Permission denied (os error 13)");
        assert!(memory.1.contains("Cho phép") && memory.0.is_none());
        assert_eq!(said("target process is gone").1, "Game đã đóng.");
    }

    #[test]
    fn what_is_not_known_is_not_hidden() {
        assert_eq!(said("something odd\n").1, "Có lỗi: something odd");
    }

    #[test]
    fn events_carry_the_action() {
        let event: serde_json::Value = serde_json::from_str(&event("hint", "HTTP 429")).unwrap();
        assert_eq!((event["type"].as_str(), event["action"].as_str()), (Some("hint"), Some("google")));
        assert_eq!(Action::named("google"), Some(Action::Google));
        assert_eq!(Action::named("?"), None);
        let plain: serde_json::Value = serde_json::from_str(&crate::explain::event("notice", "boom")).unwrap();
        assert!(plain["action"].is_null());
    }
}
