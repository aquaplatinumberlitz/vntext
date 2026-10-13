//! The icon in the system tray: a way back to the window that lists games
//! while it is hidden, and a way to stop reading or leave without it.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};

/// The picture of the program, for the menu and the tray.
pub const ICON: &str = include_str!("vntext.svg");
/// The picture as pixels, four bytes each of red, green, blue and alpha,
/// in two sizes. The tray is given these and no name: a name it has to look
/// up in the icon themes, where a picture just installed is often not found
/// yet, and then it shows a question mark. Made from the picture with:
///   rsvg-convert -w 64 -h 64 src/vntext.svg | magick png:- -depth 8 rgba:src/vntext-64.rgba
const PIXELS: [(i32, &[u8]); 2] = [(32, include_bytes!("vntext-32.rgba")), (64, include_bytes!("vntext-64.rgba"))];
/// The name the picture goes by in an icon theme.
pub const ICON_NAME: &str = "vntext";

/// What the reader asks for through the tray.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Asked {
    /// to see the window that lists games
    Show,
    /// that the game being read is read no more
    Stop,
    Quit,
}

pub struct Tray {
    tell: Sender<Asked>,
    /// a game is being read
    pub reading: bool,
}

/// Pixels as the tray takes them: alpha first, then red, green and blue.
fn alpha_first(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect()
}

/// Where a theme under `themes` keeps the picture.
pub fn icon_file(themes: &std::path::Path) -> PathBuf {
    themes.join("hicolor/scalable/apps").join(format!("{ICON_NAME}.svg"))
}

/// Writes the picture into the theme under `themes`.
pub fn write_icon(themes: &std::path::Path) -> std::io::Result<PathBuf> {
    let file = icon_file(themes);
    std::fs::create_dir_all(file.parent().unwrap())?;
    std::fs::write(&file, ICON)?;
    Ok(file)
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        ICON_NAME.to_string()
    }

    fn title(&self) -> String {
        "vntext".to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        PIXELS.iter().map(|(size, rgba)| ksni::Icon { width: *size, height: *size, data: alpha_first(rgba) }).collect()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = if self.reading { "Đang đọc một game" } else { "Chưa đọc game nào" };
        ksni::ToolTip { title: "vntext".to_string(), description: description.to_string(), ..Default::default() }
    }

    /// A click on the icon.
    fn activate(&mut self, _: i32, _: i32) {
        let _ = self.tell.send(Asked::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let item = |label: &str, icon: &str, enabled: bool, asked: Asked| {
            StandardItem { label: label.to_string(), icon_name: icon.to_string(), enabled, activate: Box::new(move |tray: &mut Tray| {
                    let _ = tray.tell.send(asked);
                }), ..Default::default() }.into()
        };
        vec![
            item("Chọn game để đọc", "view-list-symbolic", true, Asked::Show),
            item("Dừng đọc game", "media-playback-stop-symbolic", self.reading, Asked::Stop),
            ksni::MenuItem::Separator,
            item("Thoát vntext", "application-exit-symbolic", true, Asked::Quit),
        ]
    }
}

const NAME: &str = "org.vntext.Launcher";
const PATH: &str = "/org/vntext/Launcher";

/// What a second run of the program asks of the first.
struct First(Sender<Asked>);

#[zbus::interface(name = "org.vntext.Launcher")]
impl First {
    fn show(&self) {
        let _ = self.0.send(Asked::Show);
    }
}

/// Makes this the one run that lists games: a window put away in the tray
/// is easily forgotten, and starting the program again should bring it
/// back, not open a second. `None` if another run is there already, which
/// has then been asked to show its window. The connection is to be kept
/// for as long as this run lasts.
pub fn only(tell: Sender<Asked>) -> Option<Option<zbus::blocking::Connection>> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    // the name is asked for outright: a run that waited in line for it would think itself the first
    let ours = zbus::blocking::connection::Builder::session().and_then(|b| b.serve_at(PATH, First(tell))).and_then(|b| b.build()).and_then(|conn| {
        match conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())? {
            RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(conn),
            _ => Err(zbus::Error::NameTaken),
        }
    });
    match ours {
        Ok(conn) => Some(Some(conn)),
        Err(zbus::Error::NameTaken) => {
            let first = zbus::blocking::Connection::session().and_then(|conn| conn.call_method(Some(NAME), PATH, Some(NAME), "Show", &()));
            match first {
                Ok(_) => None,
                // the other run does not answer: better two windows than none
                Err(_) => Some(None),
            }
        }
        // without a session bus there is no telling, and no harm in running
        Err(_) => Some(None),
    }
}

/// Puts the icon in the tray. `None` where the desktop has no tray.
pub fn start(tell: Sender<Asked>) -> Option<Handle<Tray>> {
    match (Tray { tell, reading: false }).spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("no icon in the tray: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_written_where_a_theme_keeps_it() {
        let themes = std::env::temp_dir().join(format!("vntext-test-icons-{}", std::process::id()));
        let file = write_icon(&themes).unwrap();
        assert!(file.ends_with("hicolor/scalable/apps/vntext.svg"));
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("<svg"));
        let _ = std::fs::remove_dir_all(&themes);
    }

    #[test]
    fn the_pixels_are_whole_pictures() {
        use ksni::Tray as _;
        let (tell, _asked) = std::sync::mpsc::channel();
        let icons = Tray { tell, reading: false }.icon_pixmap();
        assert_eq!(icons.len(), 2);
        for icon in &icons {
            assert_eq!(icon.data.len() as i32, icon.width * icon.height * 4);
            // a corner is clear, the middle of the T is solid amber: alpha, red, green, blue
            assert_eq!(icon.data[0], 0);
            let middle = ((icon.height * 5 / 16 * icon.width + icon.width / 2) * 4) as usize;
            assert_eq!(icon.data[middle..middle + 4], [0xff, 0xe0, 0xa4, 0x58]);
        }
        assert_eq!(alpha_first(&[1, 2, 3, 4]), [4, 1, 2, 3]);
    }

    #[test]
    fn stopping_is_offered_only_while_reading() {
        use ksni::Tray as _;
        let (tell, asked) = std::sync::mpsc::channel();
        let mut tray = Tray { tell, reading: false };
        let enabled = |tray: &Tray| tray.menu().iter().filter_map(|item| if let ksni::MenuItem::Standard(item) = item { Some(item.enabled) } else { None }).collect::<Vec<_>>();
        assert_eq!(enabled(&tray), [true, false, true]);
        tray.reading = true;
        assert_eq!(enabled(&tray), [true, true, true]);
        tray.activate(0, 0);
        assert_eq!(asked.try_recv(), Ok(Asked::Show));
    }
}
