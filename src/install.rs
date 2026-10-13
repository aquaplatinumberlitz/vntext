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

/// Whether `dir` is one of the directories of the path `var`.
fn on_path(dir: &Path, var: &std::ffi::OsStr) -> bool {
    std::env::split_paths(var).any(|d| d == dir)
}

/// What `all` did.
pub struct Installed {
    pub entry: PathBuf,
    /// where the command was linked, if `~/.local/bin` is on the path; why
    /// it was not, if it could not be
    pub link: Option<Result<PathBuf, String>>,
}

impl std::fmt::Display for Installed {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "wrote {}", self.entry.display())?;
        match &self.link {
            Some(Ok(link)) => write!(f, "\nlinked {}", link.display()),
            Some(Err(e)) => write!(f, "\n{e}"),
            None => Ok(()),
        }
    }
}

/// Makes `dir/vntext` a link to the program at `exe`, unless something else
/// that is not a link is there.
fn link(exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    let link = dir.join("vntext");
    match std::fs::symlink_metadata(&link) {
        Ok(known) if !known.file_type().is_symlink() => return Err(format!("{} đã có một file khác nên không đặt liên kết.", link.display())),
        Ok(_) => std::fs::remove_file(&link).map_err(|e| format!("cannot replace {}: {e}", link.display()))?,
        Err(_) => {}
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    std::os::unix::fs::symlink(exe, &link).map_err(|e| format!("cannot link {}: {e}", link.display()))?;
    Ok(link)
}

/// Puts vntext in the application menu and, if `~/.local/bin` is on the
/// path, there as a command.
pub fn all() -> Result<Installed, String> {
    let entry = menu()?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot tell where vntext is: {e}"))?;
    let bin = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin"));
    let path = std::env::var_os("PATH").unwrap_or_default();
    // a link that cannot be made does not undo the menu entry
    let link = bin.filter(|bin| on_path(bin, &path)).map(|bin| link(&exe, &bin));
    Ok(Installed { entry, link })
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
    fn the_command_is_a_link() {
        let dir = std::env::temp_dir().join(format!("vntext-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        let (one, two) = (dir.join("one"), dir.join("two"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&one, "").unwrap();
        std::fs::write(&two, "").unwrap();
        assert_eq!(link(&one, &bin).unwrap(), bin.join("vntext"));
        assert_eq!(std::fs::read_link(bin.join("vntext")).unwrap(), one);
        // another run points the link at the program that is running now
        link(&two, &bin).unwrap();
        assert_eq!(std::fs::read_link(bin.join("vntext")).unwrap(), two);
        // what is not a link is not touched
        std::fs::remove_file(bin.join("vntext")).unwrap();
        std::fs::write(bin.join("vntext"), "mine").unwrap();
        assert!(link(&one, &bin).is_err());
        assert_eq!(std::fs::read_to_string(bin.join("vntext")).unwrap(), "mine");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_path_is_searched() {
        let var = std::ffi::OsString::from("/usr/bin:/home/a/.local/bin");
        assert!(on_path(Path::new("/home/a/.local/bin"), &var));
        assert!(!on_path(Path::new("/home/b/.local/bin"), &var));
    }

    #[test]
    fn the_entry_asks_for_pictures_of_windows() {
        let text = entry(Path::new("/opt/vntext"));
        assert!(text.lines().any(|l| l == "Exec=\"/opt/vntext\""));
        assert!(text.lines().any(|l| l == "X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"));
        assert!(text.lines().any(|l| l == "Categories=Game;Utility;"));
    }
}
