//! Puts vntext in the application menu, so that it opens without a terminal.

use std::path::{Path, PathBuf};

const FILE: &str = "vntext.desktop";

/// A path as the `Exec` key of a desktop entry takes it: in quotes, with
/// the characters that mean something inside them escaped.
fn quoted(path: &Path) -> String {
    let mut out = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            // a percent sign would start a field code
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The desktop entry for the program at `exe`. KWin lets a program take
/// pictures of windows only if an entry like this one asks for it and
/// points `Exec` at that very file.
fn entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]
Type=Application
Name=vntext
Comment=Đọc và dịch text của visual novel
Exec={}
Icon=accessories-dictionary
Terminal=false
Categories=Game;Utility;
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
",
        quoted(exe)
    )
}

fn applications_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from);
    data.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))).unwrap_or_else(|| PathBuf::from(".")).join("applications")
}

/// Writes the entry for the running program and returns where it went.
pub fn menu() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot tell where vntext is: {e}"))?;
    let file = applications_dir().join(FILE);
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| format!("cannot create {}: {e}", file.parent().unwrap().display()))?;
    std::fs::write(&file, entry(&exe)).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_quoted() {
        assert_eq!(quoted(Path::new("/home/a/vntext")), "\"/home/a/vntext\"");
        assert_eq!(quoted(Path::new("/home/my games/v$n\"t%")), "\"/home/my games/v\\$n\\\"t%%\"");
    }

    #[test]
    fn the_entry_asks_for_pictures_of_windows() {
        let text = entry(Path::new("/opt/vntext"));
        assert!(text.lines().any(|l| l == "Exec=\"/opt/vntext\""));
        assert!(text.lines().any(|l| l == "X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"));
        assert!(text.lines().any(|l| l == "Categories=Game;Utility;"));
    }
}
