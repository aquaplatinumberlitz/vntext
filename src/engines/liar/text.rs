//! Scripts of Liar-soft's engine: the `.xfl` archive they come in, their
//! instructions and the messages in them. The formats and the table of
//! instructions are those of Love-in-idleness's LiarsoftTool.

const X: u8 = u8::MAX;
/// How many bytes of operands each instruction has, by its code; `X` where
/// there is none. This is the tool's `RScript19` table: 信天翁航海録 does not
/// parse with its other ones.
const OPERANDS: [u8; 256] = [
    X, X, X, 4, 4, 4, X, X, 0, 2, 0, 0, 8, 4, 58, 48, 4, 0, 8, 4,
    8, 4, 16, 16, 8, 8, 0, 0, 12, 8, 24, X, 24, 20, 20, 8, 8, 8, 16, 12,
    8, 8, 8, 8, 4, 8, 4, 8, 8, 8, 0, 0, 0, 4, X, 0, 20, 0, 0, 16,
    12, 8, 4, 12, 4, 4, 16, 4, 0, 0, 16, 16, 4, 12, 4, 20, X, 16, X, X,
    4, 28, 24, 4, 8, X, X, X, X, X, 12, 20, 8, 8, 4, 8, 8, 8, 8, 12,
    12, 8, 4, 8, 16, 4, 20, 12, 16, X, 12, 12, 4, 8, 8, 8, 8, 8, X, X,
    8, 8, X, X, X, X, X, X, X, X, 16, 20, 8, X, 12, 20, 12, X, X, X,
    8, 8, 8, X, X, X, X, X, X, X, 8, 8, 8, 8, 8, 8, 12, 20, 8, 8,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    44, 20, 12, X, X, X, X, X, X, X, 8, 16, 4, 12, X, X, X, X, X, X,
    12, 8, 0, 8, X, 20, X, X, X, X, 4, 4, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, 20,
];

/// Codes with any of the top four bits set are arithmetic on variables:
/// two 16-bit operands when all four are set, three otherwise.
const ARITHMETIC: u16 = 0xF000;
const MESSAGE: u16 = 81;
/// Where the number of a message's text is among its operands, in bytes.
const MESSAGE_TEXT: usize = 20;
const HEADER: usize = 36;

fn le32(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

const ARCHIVE: u32 = 0x0001_424C;
const ENTRY: usize = 40;
const ENTRY_NAME: usize = 32;

/// The `.gsc` files of an `.xfl` archive: `signature, table length, count`,
/// then for each file its name and `offset, length` into what follows the table.
pub fn scripts(archive: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    if le32(archive, 0) != Some(ARCHIVE as usize) {
        return found;
    }
    let (Some(table), Some(count)) = (le32(archive, 4), le32(archive, 8)) else { return found };
    for k in 0..count {
        let Some(entry) = archive.get(12 + k * ENTRY..12 + (k + 1) * ENTRY) else { break };
        let name = &entry[..entry[..ENTRY_NAME].iter().position(|b| *b == 0).unwrap_or(ENTRY_NAME)];
        let (at, len) = (12 + table + le32(entry, ENTRY_NAME).unwrap(), le32(entry, ENTRY_NAME + 4).unwrap());
        if name.to_ascii_lowercase().ends_with(b".gsc") {
            found.extend(archive.get(at..at + len));
        }
    }
    found
}

/// What the engine keeps of a script's header once it is loaded, and what
/// tells scripts apart: the length of the code, the number of strings and
/// the length of the strings.
pub type Shape = [u32; 3];

pub struct Message {
    /// where its instruction ends in the code
    pub end: u32,
    pub text: String,
}

/// The messages of a script. A script is a header of nine numbers (`file
/// length, header length, code length, string table length, strings
/// length`, ...), the code, a table of where each string starts, and the
/// strings, in Shift-JIS and ended by a zero.
pub fn messages(script: &[u8]) -> Option<(Shape, Vec<Message>)> {
    if le32(script, 4)? != HEADER {
        return None;
    }
    let (code_len, table_len, strings_len) = (le32(script, 8)?, le32(script, 12)?, le32(script, 16)?);
    let code = script.get(HEADER..HEADER + code_len)?;
    let table = script.get(HEADER + code_len..HEADER + code_len + table_len)?;
    let strings = script.get(HEADER + code_len + table_len..)?.get(..strings_len)?;
    let string = |number: usize| -> Option<String> {
        let from = strings.get(le32(table, number * 4)?..)?;
        let stored = &from[..from.iter().position(|b| *b == 0)?];
        Some(encoding_rs::SHIFT_JIS.decode(stored).0.into_owned())
    };
    let mut found = Vec::new();
    let mut at = 0;
    while at + 2 <= code.len() {
        let op = u16::from_le_bytes([code[at], code[at + 1]]);
        let operands = match op & ARITHMETIC {
            0 => *OPERANDS.get(op as usize).filter(|size| **size != X)? as usize,
            ARITHMETIC => 4,
            _ => 6,
        };
        let next = at + 2 + operands;
        if op == MESSAGE {
            let text = string(le32(code, at + 2 + MESSAGE_TEXT)?).unwrap_or_default();
            found.push(Message { end: next as u32, text });
        }
        at = next;
    }
    Some(([code_len as u32, table_len as u32 / 4, strings_len as u32], found))
}

/// Splits a message into its speaker and its lines. The text starts with
/// `【speaker】` when someone speaks; `^n` breaks the line, other `^` codes
/// are dropped; `|word[reading]` or `word[reading]` gives a reading, of
/// which the word is kept.
pub fn plain(message: &str) -> (Option<String>, Vec<String>) {
    let mut speaker = None;
    let mut rest = message;
    if let Some(named) = message.strip_prefix('【') {
        if let Some((name, after)) = named.split_once('】') {
            (speaker, rest) = (Some(name.to_string()), after);
        }
    }
    let mut text = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '^' => {
                if chars.next() == Some('n') {
                    text.push('\n');
                }
            }
            '[' => {
                chars.by_ref().take_while(|c| *c != ']').for_each(drop);
            }
            '|' => {}
            _ => text.push(c),
        }
    }
    let lines = text.lines().map(|line| line.trim_matches('\u{3000}').to_string()).filter(|line| !line.is_empty()).collect();
    (speaker, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup() {
        let (speaker, lines) = plain("【船長】^n「|阿呆[あほう]の船[ふね]だ」^n　次の行^d");
        assert_eq!(speaker.as_deref(), Some("船長"));
        assert_eq!(lines, ["「阿呆の船だ」", "次の行"]);
        assert_eq!(plain("　^n　地の文").1, ["地の文"]);
    }

    #[test]
    fn script() {
        // an instruction without operands (26), a message of string 1, arithmetic
        let mut code = vec![26, 0];
        code.extend([MESSAGE as u8, 0]);
        code.extend([0u8; 20]);
        code.extend(1u32.to_le_bytes());
        code.extend(1u32.to_le_bytes());
        code.extend([0x00, 0xF1, 1, 0, 2, 0]);
        let strings = [&[0u8][..], &encoding_rs::SHIFT_JIS.encode("やあ").0, &[0]].concat();
        let mut file = Vec::new();
        for n in [0, HEADER, code.len(), 8, strings.len(), 0, 0, 0, 0] {
            file.extend((n as u32).to_le_bytes());
        }
        file.extend(&code);
        file.extend(0u32.to_le_bytes());
        file.extend(1u32.to_le_bytes());
        file.extend(&strings);
        let (shape, found) = messages(&file).unwrap();
        assert_eq!(shape, [code.len() as u32, 2, strings.len() as u32]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].end, found[0].text.as_str()), (32, "やあ"));
    }
}
