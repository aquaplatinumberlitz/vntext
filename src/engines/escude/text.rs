//! Scripts of Escu:de's engine: the `ESC-ARC` archives, the compiled
//! scripts with their messages, and the tables of scripts and speakers.
//! The formats are those of Chenx221's EscudeTools; what the instructions
//! mean is from the engine's own source, which the game ships in its archive.

use std::collections::HashMap;

fn le32(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

fn string(data: &[u8], at: usize) -> Option<String> {
    let from = data.get(at..)?;
    Some(encoding_rs::SHIFT_JIS.decode(&from[..from.iter().position(|b| *b == 0)?]).0.into_owned())
}

/// The files of an archive by name, unpacked. The index is encrypted with a
/// running key that starts from a seed in the header: in version 2,
/// `offset of name, offset, length` for each file and the names after them;
/// in version 1, a fixed-width name and `offset, length`.
pub fn archive(data: &[u8]) -> Option<HashMap<String, Vec<u8>>> {
    const NAME_V1: usize = 0x80;
    if !data.starts_with(b"ESC-ARC") {
        return None;
    }
    let mut seed = le32(data, 8)? as u32;
    let mut key = || {
        seed ^= 0x65AC_9365;
        seed ^= (((seed >> 1) ^ seed) >> 3) ^ (((seed << 1) ^ seed) << 3);
        seed
    };
    let count = (le32(data, 12)? as u32 ^ key()) as usize;
    let decrypt = |stored: &[u8], key: &mut dyn FnMut() -> u32| -> Vec<u8> {
        stored.chunks_exact(4).flat_map(|w| (u32::from_le_bytes(w.try_into().unwrap()) ^ key()).to_le_bytes()).collect()
    };
    let mut files = HashMap::new();
    let mut add = |name: String, at: usize, len: usize| {
        files.extend(data.get(at..at + len).and_then(unpack).map(|file| (name.to_lowercase(), file)));
    };
    match data[7] {
        b'2' => {
            let names_len = (le32(data, 16)? as u32 ^ key()) as usize;
            let index = decrypt(data.get(20..20 + count * 12)?, &mut key);
            let names = data.get(20 + count * 12..20 + count * 12 + names_len)?;
            for entry in index.chunks_exact(12) {
                add(string(names, le32(entry, 0)?)?, le32(entry, 4)?, le32(entry, 8)?);
            }
        }
        b'1' => {
            let index = decrypt(data.get(16..16 + count * (NAME_V1 + 8))?, &mut key);
            for entry in index.chunks_exact(NAME_V1 + 8) {
                add(string(&[&entry[..NAME_V1], &[0]].concat(), 0)?, le32(entry, NAME_V1)?, le32(entry, NAME_V1 + 4)?);
            }
        }
        _ => return None,
    }
    Some(files)
}

/// A file as stored in an archive: as it is, or `acp`, the unpacked length
/// (big-endian) and LZW codes.
fn unpack(stored: &[u8]) -> Option<Vec<u8>> {
    if !stored.starts_with(b"acp\0") {
        return Some(stored.to_vec());
    }
    let size = u32::from_be_bytes(stored.get(4..8)?.try_into().unwrap()) as usize;
    lzw(&stored[8..], size)
}

/// LZW with codes read from the high bit down, 9 bits wide at first. Code
/// 0x100 ends the data, 0x101 widens the codes by a bit, 0x102 empties the
/// dictionary; an entry of the dictionary is where in the output a code
/// started, and stands for the bytes from there through the first byte of
/// the code after it.
fn lzw(codes: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut starts: Vec<usize> = Vec::new();
    let (mut width, mut at) = (9, 0);
    while out.len() < size {
        if at + width > codes.len() * 8 {
            return None;
        }
        let code = (at..at + width).fold(0, |code, bit| code << 1 | (codes[bit / 8] >> (7 - bit % 8) & 1) as usize);
        at += width;
        match code {
            0x100 => break,
            0x101 => width += 1,
            0x102 => {
                width = 9;
                starts.clear();
            }
            _ => {
                starts.push(out.len());
                if code < 0x100 {
                    out.push(code as u8);
                } else {
                    let from = *starts.get(code - 0x103)?;
                    let len = (starts.get(code - 0x102)? - from + 1).min(size - out.len());
                    for k in 0..len {
                        out.push(out[from + k]);
                    }
                }
            }
        }
    }
    Some(out)
}

/// The sheets of a database file by name: their rows, each a list of cells
/// with the text of those that hold text. A file is `mdb`, then for each
/// sheet three blocks, each with its length before it: the columns (`name,
/// count`, then `kind: u16, size: u16, name` for each), the rows, and the
/// text that names and text cells point into.
pub fn database(data: &[u8]) -> Option<HashMap<String, Vec<Vec<Option<String>>>>> {
    const TEXT: usize = 4;
    if !data.starts_with(b"mdb\0") {
        return None;
    }
    let mut sheets = HashMap::new();
    let mut at = 4;
    let block = |at: &mut usize| -> Option<&[u8]> {
        let len = le32(data, *at)?;
        *at += 4 + len;
        data.get(*at - len..*at)
    };
    while le32(data, at)? != 0 {
        let (columns, rows, text) = (block(&mut at)?, block(&mut at)?, block(&mut at)?);
        let kinds: Vec<(usize, usize)> = columns
            .get(8..8 + le32(columns, 4)? * 8)?
            .chunks_exact(8)
            .map(|c| (u16::from_le_bytes([c[0], c[1]]) as usize, u16::from_le_bytes([c[2], c[3]]) as usize))
            .collect();
        let width: usize = kinds.iter().map(|k| k.1).sum();
        let mut sheet = Vec::new();
        for row in rows.chunks_exact(width.max(1)) {
            let mut cells = Vec::new();
            let mut from = 0;
            for (kind, size) in &kinds {
                let value = row[from..from + size].iter().rev().fold(0u32, |n, b| n << 8 | *b as u32);
                cells.push((*kind == TEXT).then(|| string(text, value as usize).unwrap_or_default()));
                from += size;
            }
            sheet.push(cells);
        }
        sheets.insert(string(text, le32(columns, 0)?)?, sheet);
    }
    Some(sheets)
}

/// Instructions of the scripts, by their numbers in the engine's source.
pub const NAME: u8 = 40;
pub const TEXT: u8 = 41;
pub const PAGE: u8 = 42;
pub const OPTION: u8 = 43;

/// How many 4-byte parameters follow an instruction in the code.
pub fn parameters(instruction: u8) -> usize {
    match instruction {
        2 | 4 | 5 | 7..=13 | 15..=17 | 40 | 41 | 44 | 45 => 1,
        OPTION => 2,
        _ => 0,
    }
}

/// One page of text: what is shown until the reader clicks.
pub struct Page {
    /// the speaker's number in the table of speakers; 0 for narration
    pub speaker: u32,
    pub text: String,
    pub choices: Vec<String>,
    /// the bytes of code it spans, from its first instruction to past its last
    pub from: u32,
    pub to: u32,
}

pub struct Script {
    pub code: Vec<u8>,
    pub pages: Vec<Page>,
}

/// Reads a script and its messages. The script is `@code:__`, the lengths
/// `code, text count, text, message count`, and the code; the messages are
/// `@mess:__`, `count, length`, where each starts, and the messages with
/// every byte XORed with 0x55.
///
/// A page is `NAME` (not for narration), one or more `TEXT` and `PAGE`. A
/// menu is a run of `OPTION`s, each with the number of the message that is
/// its label; it is added to the page before it.
pub fn script(code_file: &[u8], messages: Option<&[u8]>) -> Option<Script> {
    if !code_file.starts_with(b"@code:__") {
        return None;
    }
    let code = code_file.get(24..24 + le32(code_file, 8)?)?.to_vec();
    let message = |number: usize| -> Option<String> {
        let file = messages.filter(|m| m.starts_with(b"@mess:__"))?;
        let (count, len) = (le32(file, 8)?, le32(file, 12)?);
        let plain: Vec<u8> = file.get(16 + count * 4..16 + count * 4 + len)?.iter().map(|b| b ^ 0x55).collect();
        string(&plain, le32(file, 16 + number * 4).filter(|_| number < count)?)
    };
    let mut pages: Vec<Page> = Vec::new();
    let mut open: Option<Page> = None;
    let mut speaker = 0;
    let mut at = 0;
    while at < code.len() {
        let instruction = code[at];
        let next = at + 1 + parameters(instruction) * 4;
        let parameter = le32(&code, at + 1).unwrap_or(0);
        match instruction {
            NAME => speaker = parameter as u32,
            TEXT => {
                let page = open.get_or_insert(Page { speaker, text: String::new(), choices: Vec::new(), from: at as u32, to: 0 });
                page.text.push_str(&message(parameter).unwrap_or_default());
            }
            PAGE => {
                pages.extend(open.take().map(|page| Page { to: next as u32, ..page }));
                speaker = 0;
            }
            OPTION => {
                if let (Some(page), Some(label)) = (pages.last_mut(), message(parameter)) {
                    page.choices.push(label);
                    page.to = next as u32;
                }
            }
            _ => {}
        }
        at = next;
    }
    Some(Script { code, pages })
}

/// Makes plain lines of a message: `<r>` breaks the line,
/// `<ruby text='reading'>word</ruby>` keeps the word, other tags are dropped.
pub fn plain(message: &str) -> Vec<String> {
    let mut text = String::new();
    let mut rest = message;
    while let Some(tag) = rest.find('<') {
        text.push_str(&rest[..tag]);
        let Some(end) = rest[tag..].find('>') else { break };
        if &rest[tag..tag + end + 1] == "<r>" {
            text.push('\n');
        }
        rest = &rest[tag + end + 1..];
    }
    text.push_str(rest);
    text.lines().map(|line| line.trim_matches('\u{3000}').to_string()).filter(|line| !line.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup() {
        assert_eq!(plain("「<ruby text='はいそん'>廃村</ruby>だ」<r>　次の行"), ["「廃村だ」", "次の行"]);
    }

    #[test]
    fn lzw_codes() {
        // 'a', 'b', then the first entry: "ab", and the end
        let bits = "001100001".to_string() + "001100010" + "100000011" + "100000000";
        let mut codes = vec![0u8; bits.len().div_ceil(8)];
        for (k, bit) in bits.bytes().enumerate() {
            codes[k / 8] |= (bit - b'0') << (7 - k % 8);
        }
        assert_eq!(lzw(&codes, 4).unwrap(), b"abab");
    }

    #[test]
    fn pages() {
        let hidden = |text: &str| -> Vec<u8> { encoding_rs::SHIFT_JIS.encode(text).0.iter().chain(&[0]).map(|b| b ^ 0x55).collect() };
        let (first, second, choice) = (hidden("やあ"), hidden("地の文"), hidden("はい"));
        let mut messages = b"@mess:__".to_vec();
        let lens = [0, first.len(), first.len() + second.len()];
        for n in [3, first.len() + second.len() + choice.len(), lens[0], lens[1], lens[2]] {
            messages.extend((n as u32).to_le_bytes());
        }
        messages.extend([first, second, choice].concat());
        let mut code = Vec::new();
        for (instruction, parameters) in [(NAME, &[7u32][..]), (TEXT, &[0]), (PAGE, &[]), (TEXT, &[1]), (PAGE, &[]), (OPTION, &[2, 99])] {
            code.push(instruction);
            code.extend(parameters.iter().flat_map(|p| p.to_le_bytes()));
        }
        let mut file = b"@code:__".to_vec();
        for n in [code.len(), 0, 0, 3] {
            file.extend((n as u32).to_le_bytes());
        }
        file.extend(&code);
        let script = script(&file, Some(&messages)).unwrap();
        assert_eq!(script.pages.len(), 2);
        assert_eq!((script.pages[0].speaker, script.pages[0].text.as_str(), script.pages[0].from, script.pages[0].to), (7, "やあ", 5, 11));
        assert_eq!((script.pages[1].speaker, script.pages[1].text.as_str()), (0, "地の文"));
        assert_eq!(script.pages[1].choices, ["はい"]);
        assert_eq!(script.pages[1].to, code.len() as u32);
    }
}
