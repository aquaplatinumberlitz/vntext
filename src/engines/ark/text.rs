//! Scripts of Astronauts' ArkEngine: the encrypted pack they come in and the
//! `\text` blocks in them. Key, cipher and pack layout are those of the
//! game's own `DataEncrypt` class.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

const KEY: &[u8; 32] = b"c6eahbq9sjuawhvdr9kvhpsm5qv393ga";
const IV: &[u8; 16] = b"ARC-PACKPASSWORD";

/// Everything in a pack is AES-256 in CBC mode with PKCS#7 padding.
pub fn decrypt(stored: &[u8]) -> Option<Vec<u8>> {
    let mut data = stored.to_vec();
    let len = cbc::Decryptor::<aes::Aes256>::new(KEY.into(), IV.into()).decrypt_padded_mut::<Pkcs7>(&mut data).ok()?.len();
    data.truncate(len);
    Some(data)
}

/// The little MessagePack these files use: strings, and maps of strings to
/// strings or non-negative integers.
pub struct Pack<'a>(pub &'a [u8]);

impl<'a> Pack<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = rest;
        Some(head)
    }

    fn number(&mut self, bytes: usize) -> Option<usize> {
        Some(self.take(bytes)?.iter().fold(0, |n, b| n << 8 | *b as usize))
    }

    pub fn string(&mut self) -> Option<&'a str> {
        let len = match self.take(1)?[0] {
            t @ 0xA0..=0xBF => (t & 0x1F) as usize,
            0xD9 => self.number(1)?,
            0xDA => self.number(2)?,
            0xDB => self.number(4)?,
            _ => return None,
        };
        std::str::from_utf8(self.take(len)?).ok()
    }

    pub fn integer(&mut self) -> Option<usize> {
        match self.take(1)?[0] {
            t @ 0x00..=0x7F => Some(t as usize),
            0xCC | 0xD0 => self.number(1),
            0xCD | 0xD1 => self.number(2),
            0xCE | 0xD2 => self.number(4),
            _ => None,
        }
    }

    pub fn map_len(&mut self) -> Option<usize> {
        match self.take(1)?[0] {
            t @ 0x80..=0x8F => Some((t & 0x0F) as usize),
            0xDE => self.number(2),
            _ => None,
        }
    }
}

/// One entry of an index file: where a file sits in the pack.
pub struct Entry {
    pub name: String,
    pub at: usize,
    pub size: usize,
}

/// An index is a run of `length: i32` and that many encrypted bytes, each a
/// map with `fileName`, `size` and `index`.
pub fn index(data: &[u8]) -> Option<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let len = u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize;
        let record = decrypt(data.get(at + 4..at + 4 + len)?)?;
        let mut pack = Pack(&record);
        let mut entry = Entry { name: String::new(), at: 0, size: 0 };
        for _ in 0..pack.map_len()? {
            match pack.string()? {
                "fileName" => entry.name = pack.string()?.to_string(),
                "size" => entry.size = pack.integer()?,
                "index" => entry.at = pack.integer()?,
                _ => return None,
            }
        }
        entries.push(entry);
        at += 4 + len;
    }
    Some(entries)
}

/// A text box or a menu of choices, as written in a script.
pub struct Block {
    pub speaker: Option<String>,
    /// the text; for a menu, one line per choice
    pub text: String,
    pub menu: bool,
    /// The parser's line counter while this is on screen: the number of the
    /// line after `\endtext`, or for a menu anything from `first` to `line`
    /// (the lines after its first `\MENU` and after its `\MENUEND`).
    pub first: u32,
    pub line: u32,
}

/// Shown as the speaker of a menu of choices.
const MENU_LABEL: &str = "選択肢";

/// Picks the text boxes and menus out of a script. A text box is `\text` or
/// `\text("speaker",...)`, the lines of text, then `\endtext`; the lines
/// only wrap the text in its box, so they are joined. A menu is a run of
/// `\MENU(number,"choice",...)` closed by `\MENUEND`.
pub fn blocks(script: &str) -> Vec<Block> {
    let mut found = Vec::new();
    let mut open: Option<Block> = None;
    let mut menu: Option<Block> = None;
    for (number, line) in script.lines().enumerate() {
        let (line, after) = (line.trim(), number as u32 + 1);
        if line.starts_with("\\endtext") {
            if let Some(block) = open.take().filter(|b| !b.text.is_empty()) {
                found.push(Block { first: after, line: after, ..block });
            }
        } else if let Some(arguments) = line.strip_prefix("\\text") {
            let speaker = arguments.strip_prefix('(').and_then(|a| a.split(',').next()).map(|s| s.trim().trim_matches('"'));
            let speaker = speaker.filter(|s| !s.is_empty()).map(str::to_string);
            open = Some(Block { speaker, text: String::new(), menu: false, first: 0, line: 0 });
        } else if line.starts_with("\\MENUEND") {
            found.extend(menu.take().map(|block| Block { line: after, ..block }));
        } else if let Some(arguments) = line.strip_prefix("\\MENU(") {
            let choice = arguments.split('"').nth(1).unwrap_or("").trim();
            let block = menu.get_or_insert_with(|| Block {
                speaker: Some(MENU_LABEL.to_string()),
                text: String::new(),
                menu: true,
                first: after,
                line: after,
            });
            if !choice.is_empty() {
                block.text.push_str(choice);
                block.text.push('\n');
            }
        } else if let Some(block) = open.as_mut() {
            if !line.starts_with("//") {
                block.text.push_str(line.trim_start_matches('\u{3000}'));
            }
        }
    }
    found
}

/// What two renderings of a text box have in common: the game shows the
/// lines with breaks and indents that the script does not have.
pub fn essence(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_blocks() {
        let script = "\\CG(\"SpB\",1)\r\n\\text(\"白衣の男\",,,)\r\n「一式・蟲指。指先を\r\n刺激する技です」\r\n\\endtext\r\n\r\n\\text\r\n夜の公園。\r\n\\endtext\r\n";
        let found = blocks(script);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].speaker.as_deref(), Some("白衣の男"));
        assert_eq!(found[0].text, "「一式・蟲指。指先を刺激する技です」");
        assert_eq!(found[0].line, 5);
        assert_eq!((found[1].speaker.as_deref(), found[1].text.as_str(), found[1].line), (None, "夜の公園。", 9));
        assert_eq!(found[0].first, 5);
        assert_eq!(essence("「一式・蟲指。指先を\n　刺激する技です」"), essence(&found[0].text));

        let menus = blocks("\\text\r\n選んで。\r\n\\endtext\r\n//選択肢\r\n\\MENU(1,\"制服\",@a,,)\r\n\\MENU(2,\"体操服\",@b,$x,)\r\n\\MENUEND(0)\r\n");
        let menu = &menus[1];
        assert!(menu.menu && !menus[0].menu);
        assert_eq!(menu.text, "制服\n体操服\n");
        assert_eq!((menu.first, menu.line), (5, 7));
    }

    #[test]
    fn message_pack() {
        let mut pack = Pack(&[0x83, 0xA8, b'f', b'i', b'l', b'e', b'N', b'a', b'm', b'e', 0xA3, b'a', b'.', b's', 0xCD, 0x01, 0x02, 0x7F]);
        assert_eq!(pack.map_len(), Some(3));
        assert_eq!(pack.string(), Some("fileName"));
        assert_eq!(pack.string(), Some("a.s"));
        assert_eq!(pack.integer(), Some(0x102));
        assert_eq!(pack.integer(), Some(0x7F));
    }
}
