//! Pictures of a part of a game's window, taken through X11 and made as
//! small as they can be while their text stays readable.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, ImageFormat, MapState, Window};
use x11rb::rust_connection::RustConnection;

/// Shades of grey kept: what four bits a pixel hold.
const SHADES: u32 = 16;
/// A picture wider than this is shrunk by a whole factor.
const MAX_WIDTH: usize = 1024;
/// How deep below the root a game's own window is looked for.
const DEPTH: u32 = 3;

pub struct Screen {
    conn: RustConnection,
    root: Window,
}

/// A part of a window, in fractions of its width and height.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Region {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// A grey picture, one shade below `SHADES` for each pixel.
#[derive(Clone, PartialEq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub shades: Vec<u8>,
}

impl Screen {
    pub fn open() -> Result<Screen, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("cannot reach the X server: {e}"))?;
        let root = conn.setup().roots[screen].root;
        Ok(Screen { conn, root })
    }

    /// The largest window on screen that belongs to the process.
    pub fn window_of(&self, pid: u32) -> Option<Window> {
        let mut best: Option<(u32, Window)> = None;
        let mut level = vec![self.root];
        for _ in 0..DEPTH {
            let mut next = Vec::new();
            for window in level {
                let Some(tree) = self.conn.query_tree(window).ok().and_then(|c| c.reply().ok()) else { continue };
                for child in tree.children {
                    next.push(child);
                    if self.number(child, b"_NET_WM_PID", AtomEnum::CARDINAL) != Some(pid) {
                        continue;
                    }
                    let shown = self.conn.get_window_attributes(child).ok().and_then(|c| c.reply().ok());
                    let Some((width, height)) = self.size(child) else { continue };
                    let area = width as u32 * height as u32;
                    if shown.is_some_and(|a| a.map_state == MapState::VIEWABLE) && best.is_none_or(|b| area > b.0) {
                        best = Some((area, child));
                    }
                }
            }
            level = next;
        }
        best.map(|b| b.1)
    }

    fn number(&self, window: Window, property: &[u8], kind: AtomEnum) -> Option<u32> {
        let property = self.conn.intern_atom(false, property).ok()?.reply().ok()?.atom;
        let value = self.conn.get_property(false, window, property, kind, 0, 1).ok()?.reply().ok()?;
        let number = value.value32()?.next();
        number
    }

    /// The process whose window has the keyboard, if it is an X11 window.
    pub fn active_pid(&self) -> Option<u32> {
        let active = self.number(self.root, b"_NET_ACTIVE_WINDOW", AtomEnum::WINDOW).filter(|w| *w != 0)?;
        self.number(active, b"_NET_WM_PID", AtomEnum::CARDINAL)
    }

    /// Where the window is on screen, as `x, y, width, height`; `None` while
    /// it is not shown.
    pub fn placement(&self, window: Window) -> Option<(i32, i32, i32, i32)> {
        let shown = self.conn.get_window_attributes(window).ok()?.reply().ok()?;
        if shown.map_state != MapState::VIEWABLE {
            return None;
        }
        let (width, height) = self.size(window)?;
        let corner = self.conn.translate_coordinates(window, self.root, 0, 0).ok()?.reply().ok()?;
        Some((corner.dst_x as i32, corner.dst_y as i32, width as i32, height as i32))
    }

    fn size(&self, window: Window) -> Option<(u16, u16)> {
        let geometry = self.conn.get_geometry(window).ok()?.reply().ok()?;
        Some((geometry.width, geometry.height))
    }

    /// Where the pointer is in the window, in fractions of its size.
    pub fn pointer(&self, window: Window) -> Option<(f64, f64)> {
        let pointer = self.conn.query_pointer(window).ok()?.reply().ok()?;
        let (width, height) = self.size(window)?;
        let (x, y) = (pointer.win_x as f64 / width as f64, pointer.win_y as f64 / height as f64);
        ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then_some((x, y))
    }

    /// The region of the window as it looks now. `None` while the window
    /// cannot be read, as when it is minimised.
    pub fn grab(&self, window: Window, region: Region) -> Option<Picture> {
        let (width, height) = self.size(window)?;
        let (x, y) = ((region.left * width as f64) as i16, (region.top * height as f64) as i16);
        let w = ((region.right - region.left) * width as f64) as u16;
        let h = ((region.bottom - region.top) * height as f64) as u16;
        if w == 0 || h == 0 {
            return None;
        }
        let image = self.conn.get_image(ImageFormat::Z_PIXMAP, window, x, y, w, h, !0).ok()?.reply().ok()?;
        let (w, h) = (w as usize, h as usize);
        // four bytes a pixel: blue, green, red and one unused
        if image.data.len() != w * h * 4 {
            return None;
        }
        let grey: Vec<u8> = image.data.chunks_exact(4).map(|p| ((p[2] as u32 * 77 + p[1] as u32 * 150 + p[0] as u32 * 29) >> 8) as u8).collect();
        Some(Picture::from_grey(&grey, w, h))
    }
}

impl Picture {
    /// Shrinks a picture of 256 greys to at most `MAX_WIDTH` across, each
    /// pixel the average of a square of the original, and keeps `SHADES`.
    fn from_grey(grey: &[u8], width: usize, height: usize) -> Picture {
        let by = width.div_ceil(MAX_WIDTH).max(1);
        let (w, h) = (width / by, height / by);
        let mut shades = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let sum: u32 = (0..by).flat_map(|dy| (0..by).map(move |dx| (dy, dx))).map(|(dy, dx)| grey[(y * by + dy) * width + x * by + dx] as u32).sum();
                shades.push((sum / (by * by) as u32 * SHADES / 256) as u8);
            }
        }
        Picture { width: w, height: h, shades }
    }

    /// The share of pixels that are clearly not the same in the two pictures;
    /// 1 when the pictures are not of one size.
    pub fn difference(&self, other: &Picture) -> f64 {
        if (self.width, self.height) != (other.width, other.height) || self.shades.is_empty() {
            return 1.0;
        }
        let changed = self.shades.iter().zip(&other.shades).filter(|(a, b)| a.abs_diff(**b) >= 2).count();
        changed as f64 / self.shades.len() as f64
    }

    /// The picture as a PNG file of four bits a pixel.
    pub fn png(&self) -> Vec<u8> {
        let mut rows = Vec::with_capacity((self.width / 2 + 2) * self.height);
        for row in self.shades.chunks_exact(self.width) {
            // each row starts with its filter: none
            rows.push(0);
            rows.extend(row.chunks(2).map(|pair| pair[0] << 4 | pair.get(1).copied().unwrap_or(0)));
        }
        let mut header = Vec::new();
        header.extend((self.width as u32).to_be_bytes());
        header.extend((self.height as u32).to_be_bytes());
        // four bits deep, grey, and the only compression, filtering and interlacing there are
        header.extend([4, 0, 0, 0, 0]);
        let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut file, b"IHDR", &header);
        chunk(&mut file, b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&rows, 9));
        chunk(&mut file, b"IEND", &[]);
        file
    }
}

/// Adds a chunk of a PNG file: `length, kind, data, checksum of kind and data`.
fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend((data.len() as u32).to_be_bytes());
    let from = file.len();
    file.extend(kind);
    file.extend(data);
    let sum = crc32(&file[from..]);
    file.extend(sum.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut sum = !0u32;
    for byte in data {
        sum ^= *byte as u32;
        for _ in 0..8 {
            sum = sum >> 1 ^ if sum & 1 == 1 { 0xEDB8_8320 } else { 0 };
        }
    }
    !sum
}

pub fn base64(data: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let bits = (group[0] as u32) << 16 | (group.get(1).copied().unwrap_or(0) as u32) << 8 | group.get(2).copied().unwrap_or(0) as u32;
        for k in 0..4 {
            // a group of fewer than three bytes is filled up with `=`
            let digit = DIGITS[(bits >> (18 - 6 * k) & 63) as usize] as char;
            out.push(if k <= group.len() { digit } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_padding() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn crc_of_known_text() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn shrinking() {
        // 2050 across is shrunk by three; white stays the lightest shade
        let picture = Picture::from_grey(&vec![255; 2050 * 6], 2050, 6);
        assert_eq!((picture.width, picture.height), (683, 2));
        assert!(picture.shades.iter().all(|s| *s == 15));
    }

    #[test]
    fn png_layout() {
        let picture = Picture { width: 3, height: 2, shades: vec![0, 15, 7, 1, 2, 3] };
        let file = picture.png();
        assert_eq!(&file[12..16], b"IHDR");
        assert_eq!(&file[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
        let data = u32::from_be_bytes(file[33..37].try_into().unwrap()) as usize;
        let rows = miniz_oxide::inflate::decompress_to_vec_zlib(&file[41..41 + data]).unwrap();
        assert_eq!(rows, [0, 0x0F, 0x70, 0, 0x12, 0x30]);
    }

    #[test]
    fn differences() {
        let a = Picture { width: 2, height: 2, shades: vec![0, 0, 8, 8] };
        let b = Picture { width: 2, height: 2, shades: vec![1, 0, 8, 15] };
        assert_eq!(a.difference(&b), 0.25);
        assert_eq!(a.difference(&Picture { width: 4, height: 1, shades: vec![0; 4] }), 1.0);
    }
}
