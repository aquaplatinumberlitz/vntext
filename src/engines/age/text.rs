//! Scripts of Eushully's AGE engine (`SYS4`): the archives they come in,
//! their instructions and the text in them. The formats and the table of
//! instructions are those of Kelebek1's Eushully-Decompiler.

use std::collections::HashMap;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

const X: u8 = u8::MAX;
/// The number of arguments of each instruction, by its code; `X` where there is none.
const ARGUMENTS: [u8; 924] = [
    X, 0, 0, 1, 2, 0, 2, 1, 1, 0, 2, 11, 0, 4, 12, 1, 4, 9, 1, 4, 0, 5, 2, 2,
    X, X, X, X, X, X, 8, 12, 6, 2, 2, 2, 2, 3, 4, 4, 4, X, 4, 5, 5, 12, 5, 4,
    5, 4, 10, 6, 12, 11, 3, 11, 12, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, 3, 3, 3, 3, 3, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
    2, 3, 3, 2, 2, 2, 3, 3, 3, 3, 3, 3, 2, 0, 2, 1, 5, 1, 1, 10, 1, 1, 1, 1,
    1, 3, 3, 2, 0, 2, 1, 1, 1, 1, 5, 3, 1, 0, 1, 0, 1, 4, 6, 1, 1, 2, 1, 1,
    7, 1, 2, 0, 0, 2, 0, 5, X, X, X, X, X, X, X, X, 3, 0, 2, 2, X, X, X, X,
    X, X, 2, 2, 9, 0, 0, 0, 1, 1, 2, 0, 2, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1,
    1, 0, 2, 1, 1, 2, 2, 2, 1, 0, 0, 1, 2, 0, 3, 0, 1, 0, 1, 0, 4, 1, 6, 1,
    2, 0, 6, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, 0, 2, 0, 2, 1, 0, 0, 0, 3, 1, 0, 1, 1, 2,
    1, 2, 2, 2, 2, 1, 2, 1, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, 5, 7, 8, 4, 1, 1, 1, 2, 3, 2, 2, 1,
    2, 3, 6, 7, 1, 3, 2, 3, 4, 1, 1, 0, 2, 1, 1, 6, 1, 1, 7, 1, 2, 6, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, 2, 2, 2, 3, 3, 3, 3, 1,
    3, 0, 1, 0, 0, 2, 2, 2, 9, 2, 1, 1, 2, 1, 2, 1, 0, 1, 1, 2, 3, 0, 3, 3,
    3, 1, 1, 0, 0, 1, 1, 1, 2, 2, 2, 1, 0, 1, 2, 0, 1, 3, 2, 2, 1, 4, 2, 1,
    2, 3, 1, 1, 1, 2, 1, 1, 3, 5, 2, 5, 4, 0, 2, 2, 3, 2, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, 0, 0, 0, 2,
    4, 3, 1, 8, 1, 4, 5, 4, 1, 1, 5, 4, 4, 6, 7, 8, 3, 5, 1, 7, 0, 1, 0, 3,
    1, 1, 2, 3, 2, 2, 2, 4, 4, 4, 4, 1, 0, 2, 6, 7, 6, 4, 2, 8, 0, 2, 5, 6,
    5, 5, 3, 4, 3, 5, 6, 5, 1, 4, 4, 5, 5, 5, 4, 2, 1, 6, 2, 7, 0, 0, 2, 2,
    4, 5, 2, 0, 0, 2, 2, 1, 1, 3, 3, X, X, 12, 1, 10, 10, 12, 1, 2, 5, X, 5, 5,
    2, 0, 1, 1, 8, 3, 5, 4, 4, 1, 1, 1, 5, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, 11, 1, 1, 3, 3, 1, 6, 2, 0, 2, 2, 4, 4, 3, X, X, 1, 1, 1, 1,
    3, 3, 3, 3, X, 2, X, 2, 3, 2, 8, 1, 1, 2, 2, 3, 3, 3, 3, 3, 3, 1, 2, 2,
    1, 1, 1, 1, 2, X, 1, 11, 9, 7, 6, 6, 3, 4, 1, 1, 2, 7, 1, 1, 5, 6, 1, 2,
    3, 1, 2, 3, 0, 0, 1, 1, 1, X, 2, X, 1, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, 10, 3, 4, 5, 0, 2, 4, 1, 3, 2, 1, 0, 6, 2, 11, 1,
    2, X, 4, X, 1, 4, X, 4, X, X, X, 4, X, 3, 5, 3, 1, 2, 1, X, 2, 3, X, X,
    X, 4, X, X, X, 6, 4, X, X, X, 3, 2, 2, X, X, X, 5, X, 5, 2, 2, 3, X, 3,
    3, 2, X, 3, 3, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,
    X, X, X, X, X, X, X, X, X, X, X, X, 3, X, 11, 8, 3, 6, X, X, X, X, X, 6,
    7, 2, 1, 6, X, X, 5, X, 3, 7, X, 5,
];

const SHOW_TEXT: u32 = 0x6E;
const WAIT_FOR_INPUT: u32 = 0x72;
const FURIGANA: u32 = 0x196;
const SET_STRING: u32 = 0x192;
const LOOKUP: u32 = 0x61;
const LOOKUP_2D: u32 = 0x12C;
const MOV: u32 = 0x55;
const SUB: u32 = 0x51;
const CALL: u32 = 0x8F;
/// The instruction whose second argument is an array in the data.
const ARRAY: u32 = 0x64;

/// Kinds of argument.
const NUMBER: u32 = 0;
const STRING: u32 = 2;
const GLOBAL: u32 = 3;
const GLOBAL_STRING: u32 = 5;
const LOCAL_STRING_POINTER: u32 = 0xE;

/// A script starts with `SYS4` and its version, then these numbers.
const SIGNATURE: &[u8] = b"SYS4";
pub const HEADER: usize = 0x3C;
const NUMBERS_AT: usize = 8;

fn le32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()))
}

/// What tells scripts apart once loaded: the six counts of local variables
/// and the length and place of the three tables after the code, all from
/// the header. The game keeps every one of them with a running script.
pub type Shape = [u32; 12];

pub fn shape(script: &[u8]) -> Option<Shape> {
    if !script.starts_with(SIGNATURE) {
        return None;
    }
    let n = |k: usize| le32(script, NUMBERS_AT + k * 4);
    // the seventh number is the length of the six that follow
    Some([n(0)?, n(1)?, n(2)?, n(3)?, n(4)?, n(5)?, n(7)?, n(8)?, n(9)?, n(10)?, n(11)?, n(12)?])
}

/// Where the code of a script of this shape ends, in 4-byte words from its start.
pub fn code_words(shape: &Shape) -> u32 {
    shape[7].min(shape[9]).min(shape[11])
}

pub struct Instruction {
    /// place in the code, in words
    pub at: u32,
    pub code: u32,
    /// kind and value of each argument
    pub arguments: Vec<(u32, u32)>,
}

/// The instructions of a script. Strings and arrays sit after the code and
/// before the tables, so the code ends at the first one that is referred to.
pub fn instructions(script: &[u8]) -> Option<Vec<Instruction>> {
    let mut end = code_words(&shape(script)?);
    let mut found = Vec::new();
    let mut at = 0;
    while at < end {
        let code = le32(script, HEADER + at as usize * 4)?;
        let count = *ARGUMENTS.get(code as usize).filter(|count| **count != X)? as u32;
        let mut arguments = Vec::with_capacity(count as usize);
        for k in 0..count {
            let place = HEADER + (at + 1 + k * 2) as usize * 4;
            let (kind, value) = (le32(script, place)?, le32(script, place + 4)?);
            if kind == STRING || (code == ARRAY && k == 1) {
                end = end.min(value);
            }
            arguments.push((kind, value));
        }
        found.push(Instruction { at, code, arguments });
        at += 1 + count * 2;
    }
    Some(found)
}

/// The engine's own glyphs, `F040` to `F043` in Shift-JIS, which decode to
/// private-use characters: a dash, and the start, middle and end of a long
/// bar that stretches a vowel.
const GLYPHS: [(char, char); 4] = [('\u{E000}', '―'), ('\u{E001}', 'ー'), ('\u{E002}', 'ー'), ('\u{E003}', 'ー')];

/// A string of a script: Shift-JIS with every byte inverted, up to a `FF`.
fn string(script: &[u8], word: u32) -> Option<String> {
    let stored = script.get(HEADER + word as usize * 4..)?;
    let plain: Vec<u8> = stored.iter().take_while(|b| **b != 0xFF).map(|b| !b).collect();
    let text = encoding_rs::SHIFT_JIS.decode(&plain).0;
    Some(text.chars().map(|c| GLYPHS.iter().find(|(code, _)| *code == c).map_or(c, |(_, shown)| *shown)).collect())
}

/// How the game's scripts name the speaker, read off the routine that
/// draws the name: it looks the name up in a list of global strings by the
/// number in one global variable, or, when a second variable is not zero,
/// in a table with `width` names for each speaker.
#[derive(Clone, Copy)]
pub struct Naming {
    names: u32,
    speaker: u32,
    variants: u32,
    width: u32,
    variant: u32,
}

/// Finds that routine: `lookup-array` of a string by a global, and a little
/// later `lookup-array-2d` of a string by the same global and another.
pub fn naming(code: &[Instruction]) -> Option<Naming> {
    let kinds = |i: &Instruction, wanted: &[u32]| i.arguments.iter().map(|a| a.0).eq(wanted.iter().copied());
    code.iter().enumerate().find_map(|(k, wide)| {
        if wide.code != LOOKUP_2D || !kinds(wide, &[LOCAL_STRING_POINTER, GLOBAL_STRING, GLOBAL, NUMBER, GLOBAL]) {
            return None;
        }
        let speaker = wide.arguments[2].1;
        let plain = code[k.saturating_sub(8)..k].iter().rev().find(|i| {
            i.code == LOOKUP && kinds(i, &[LOCAL_STRING_POINTER, GLOBAL_STRING, GLOBAL]) && i.arguments[2].1 == speaker
        })?;
        Some(Naming {
            names: plain.arguments[1].1,
            speaker,
            variants: wide.arguments[1].1,
            width: wide.arguments[3].1,
            variant: wide.arguments[4].1,
        })
    })
}

/// The global strings a script sets to fixed text, by their numbers.
pub fn strings(script: &[u8], code: &[Instruction], into: &mut HashMap<u32, String>) {
    for set in code.iter().filter(|i| i.code == SET_STRING) {
        if let [(GLOBAL_STRING, to), (STRING, from)] = set.arguments[..] {
            into.extend(string(script, from).map(|text| (to, text)));
        }
    }
}

/// One stretch of text that the game shows before waiting for the reader.
pub struct Shown {
    /// the speaker's number and which of their names is used
    pub speaker: Option<(u32, u32)>,
    pub text: String,
    /// the words of code it spans, up to just past the wait
    pub from: u32,
    pub to: u32,
}

/// Instructions that call another script (`call-script`), and those that end
/// a stretch of straight code.
const CALL_SCRIPT: u32 = 0x03;
const ENDS_CODE: [u32; 6] = [0x02, 0x05, 0x8C, 0x8F, 0xA0, CALL_SCRIPT];

/// Whether some Japanese is in it: kana, or a character of the CJK block.
fn japanese(text: &str) -> bool {
    text.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c) || ('\u{4E00}'..='\u{9FFF}').contains(&c))
}

/// A menu of choices: the labels are put into a list of strings one after
/// the other, then a script of the system is called to draw the menu and
/// wait. The question, if there is one, is put into a string of its own just
/// before.
pub struct Menu {
    pub question: Option<String>,
    pub labels: Vec<String>,
    /// the place of the `call-script`, in words; the interpreter stands on
    /// it while the menu is up
    pub call_at: u32,
}

/// Finds the menus of a script: a `call-script` with two or more labels
/// assigned in the straight code before it.
pub fn menus(script: &[u8], code: &[Instruction]) -> Vec<Menu> {
    let mut found = Vec::new();
    for (k, call) in code.iter().enumerate().filter(|(_, i)| i.code == CALL_SCRIPT) {
        let mut labels = Vec::new();
        let mut question = None;
        for before in code[..k].iter().rev().take(60) {
            if ENDS_CODE.contains(&before.code) {
                break;
            }
            if let (SET_STRING, [(kind, _), (STRING, text)]) = (before.code, &before.arguments[..]) {
                let Some(text) = string(script, *text).filter(|t| japanese(t)) else { continue };
                if *kind == LOCAL_STRING_POINTER {
                    labels.push(text);
                } else if !labels.is_empty() && question.is_none() {
                    question = Some(text);
                }
            }
        }
        if labels.len() >= 2 {
            labels.reverse();
            found.push(Menu { question, labels, call_at: call.at });
        }
    }
    found
}

/// The text of a script. A passage is every `show-text` of fixed text up to
/// a `wait-for-input`; `display-furigana` in between carries a word and
/// its reading, of which the word is kept.
///
/// The speaker is the last number put into the speaker variable before the
/// text. Narration calls a routine that puts minus one there, so a call of
/// such a routine clears the speaker.
pub fn passages(script: &[u8], code: &[Instruction], naming: Option<&Naming>) -> Vec<Shown> {
    let clears = |i: &Instruction, n: &Naming| i.code == SUB && i.arguments[..] == [(GLOBAL, n.speaker), (NUMBER, 0), (NUMBER, 1)];
    let place: HashMap<u32, usize> = code.iter().enumerate().map(|(k, i)| (i.at, k)).collect();
    let mut found = Vec::new();
    let (mut speaker, mut variant) = (None, 0);
    let mut open: Option<(u32, String)> = None;
    for i in code {
        let text = match i.code {
            SHOW_TEXT => i.arguments.get(1),
            FURIGANA => i.arguments.get(1),
            _ => None,
        };
        if let Some((STRING, word)) = text {
            let (_, so_far) = open.get_or_insert((i.at, String::new()));
            so_far.push_str(&string(script, *word).unwrap_or_default());
        } else if i.code == WAIT_FOR_INPUT {
            if let Some((from, text)) = open.take().filter(|(_, text)| !text.is_empty()) {
                found.push(Shown { speaker: speaker.map(|id| (id, variant)), text, from, to: i.at + 1 + i.arguments.len() as u32 * 2 });
            }
        } else if let Some(n) = naming {
            match (i.code, &i.arguments[..]) {
                (MOV, [(GLOBAL, to), (NUMBER, value)]) if *to == n.speaker => speaker = Some(*value).filter(|v| *v < i32::MAX as u32),
                (MOV, [(GLOBAL, to), (NUMBER, value)]) if *to == n.variant => variant = *value,
                (CALL, [(NUMBER, target)]) if place.get(target).is_some_and(|k| clears(&code[*k], n)) => speaker = None,
                _ if clears(i, n) => speaker = None,
                _ => {}
            }
        }
    }
    found
}

impl Naming {
    /// The name that goes with a speaker, from the global strings.
    pub fn name<'a>(&self, strings: &'a HashMap<u32, String>, (speaker, variant): (u32, u32)) -> Option<&'a String> {
        let at = match variant {
            0 => self.names + speaker,
            v if v < self.width => self.variants + speaker * self.width + v,
            _ => return None,
        };
        strings.get(&at).filter(|name| !name.is_empty())
    }
}

/// Unpacks LZSS as these archives use it: a 4096-byte window that starts
/// empty and is written from 0xFEE, flag bits from the low end, a set bit
/// for a literal.
fn lzss(stored: &[u8], size: usize) -> Vec<u8> {
    let mut window = [0u8; 4096];
    let (mut write, mut at) = (0xFEE, 0);
    let mut out = Vec::with_capacity(size);
    'all: while at < stored.len() {
        let flags = stored[at];
        at += 1;
        for bit in 0..8 {
            if out.len() >= size || at >= stored.len() {
                break 'all;
            }
            if flags >> bit & 1 == 1 {
                out.push(stored[at]);
                window[write] = stored[at];
                write = (write + 1) & 0xFFF;
                at += 1;
            } else {
                let Some(&[low, high]) = stored.get(at..at + 2).map(|pair| <&[u8; 2]>::try_from(pair).unwrap()) else { break 'all };
                at += 2;
                let from = low as usize | (high as usize & 0xF0) << 4;
                for k in 0..(high as usize & 0xF) + 3 {
                    let byte = window[(from + k) & 0xFFF];
                    out.push(byte);
                    window[write] = byte;
                    write = (write + 1) & 0xFFF;
                }
            }
        }
    }
    out
}

/// Sizes in an archive index: the names of the archives and of the files.
const ARCHIVE_NAME: usize = 0x100;
const ENTRY: usize = 0x50;
const ENTRY_NAME: usize = 0x40;
/// The packed index starts somewhere in here, after a title; where differs
/// between the main index and those of the append discs.
const INDEX_FROM: std::ops::Range<usize> = 0x100..0x240;

fn name(field: &[u8]) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    encoding_rs::SHIFT_JIS.decode(&field[..end]).0.into_owned()
}

/// The scripts listed in an archive index (`SYS4INI.BIN` or `APPENDnn.AAI`),
/// read from the archives next to it. The index is `size: u32` twice, the
/// packed length, and the packed list: the archives, then for each file
/// its name, `archive, number, offset, length`.
pub fn archived(index: &Path, mut each: impl FnMut(Vec<u8>)) -> Option<()> {
    let data = std::fs::read(index).ok()?;
    let at = INDEX_FROM.step_by(4).find(|at| {
        let n = |k: usize| le32(&data, at + k * 4).unwrap_or(0) as usize;
        n(0) == n(1) && n(0) > 0 && n(2) == data.len() - at - 12
    })?;
    let list = lzss(&data[at + 12..], le32(&data, at)? as usize);
    let archives = le32(&list, 0)? as usize;
    let dir = index.parent()?;
    let files: Vec<Option<File>> = (0..archives)
        .map(|k| File::open(dir.join(name(list.get(4 + k * ARCHIVE_NAME..4 + (k + 1) * ARCHIVE_NAME)?))).ok())
        .collect();
    let entries = 4 + archives * ARCHIVE_NAME;
    for k in 0..le32(&list, entries)? as usize {
        let Some(entry) = list.get(entries + 4 + k * ENTRY..entries + 4 + (k + 1) * ENTRY) else { break };
        if !name(&entry[..ENTRY_NAME]).to_uppercase().ends_with(".BIN") {
            continue;
        }
        let n = |k: usize| le32(entry, ENTRY_NAME + k * 4).unwrap_or(0) as usize;
        let Some(file) = files.get(n(0)).and_then(Option::as_ref) else { continue };
        let mut script = vec![0u8; n(3)];
        if file.read_exact_at(&mut script, n(2) as u64).is_ok() {
            each(script);
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn script() {
        // mov speaker 7; show-text; furigana; show-text; wait; then narration
        let code = [
            MOV, GLOBAL, 0x10, NUMBER, 7,
            SHOW_TEXT, NUMBER, 0, STRING, 35,
            FURIGANA, NUMBER, 0, STRING, 38, STRING, 39,
            WAIT_FOR_INPUT, NUMBER, 0,
            SUB, GLOBAL, 0x10, NUMBER, 0, NUMBER, 1,
            SHOW_TEXT, NUMBER, 0, STRING, 35,
            WAIT_FOR_INPUT, NUMBER, 0,
        ];
        assert_eq!(code.len(), 35);
        let mut script = b"SYS4472 ".to_vec();
        // six counts, the length of the rest, three tables right after the strings
        script.extend(words(&[1, 2, 3, 4, 5, 6, 0x1C, 0, 44, 0, 44, 0, 44]));
        assert_eq!(script.len(), HEADER);
        script.extend(words(&code));
        let inverted = |text: &str| -> Vec<u8> { encoding_rs::SHIFT_JIS.encode(text).0.iter().map(|b| !b).chain([0xFF]).collect() };
        // the text, then the engine's glyphs for the start and end of a long bar
        let mut said = inverted("「やあ");
        said.splice(said.len() - 1.., [!0xF0, !0x41, !0xF0, !0x43, 0xFF]);
        script.extend(said); // 10 bytes and the end: words 35 to 37
        script.resize(HEADER + 38 * 4, 0);
        script.extend(inverted("君")); // word 38
        script.resize(HEADER + 39 * 4, 0);
        script.extend(inverted("きみ"));
        script.resize(HEADER + 44 * 4, 0xFF);

        assert_eq!(shape(&script).unwrap(), [1, 2, 3, 4, 5, 6, 0, 44, 0, 44, 0, 44]);
        let code = instructions(&script).unwrap();
        assert_eq!(code.len(), 7);
        let naming = Naming { names: 100, speaker: 0x10, variants: 200, width: 6, variant: 0x11 };
        let found = passages(&script, &code, Some(&naming));
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].text.as_str(), found[0].speaker, found[0].from, found[0].to), ("「やあーー君", Some((7, 0)), 5, 20));
        assert_eq!((found[1].text.as_str(), found[1].speaker), ("「やあーー", None));
        let strings = HashMap::from([(107, "太郎".to_string()), (200 + 7 * 6 + 2, "謎の男".to_string())]);
        assert_eq!(naming.name(&strings, (7, 0)).unwrap(), "太郎");
        assert_eq!(naming.name(&strings, (7, 2)).unwrap(), "謎の男");
    }

    #[test]
    fn menu_of_labels() {
        let inverted = |text: &str| -> Vec<u8> { encoding_rs::SHIFT_JIS.encode(text).0.iter().map(|b| !b).chain([0xFF]).collect() };
        // question, two labels, the call; strings follow the code
        let code = [
            SET_STRING, GLOBAL_STRING, 9, STRING, 18,
            SET_STRING, LOCAL_STRING_POINTER, 0, STRING, 26,
            SET_STRING, LOCAL_STRING_POINTER, 0, STRING, 30,
            CALL_SCRIPT, NUMBER, 7,
        ];
        let mut script = b"SYS4472 ".to_vec();
        script.extend(words(&[1, 2, 3, 4, 5, 6, 0x1C, 0, 18, 0, 18, 0, 18]));
        script.extend(words(&code));
        for (at, text) in [(18, "開けますか？"), (26, "はい"), (30, "いいえ")] {
            script.resize(HEADER + at * 4, 0);
            script.extend(inverted(text));
        }
        let code = instructions(&script).unwrap();
        let found = menus(&script, &code);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].question.as_deref(), found[0].labels.clone(), found[0].call_at), (Some("開けますか？"), vec!["はい".to_string(), "いいえ".to_string()], 15));
    }

    #[test]
    fn lzss_window() {
        // two literals, then a copy of five bytes from where they were written
        assert_eq!(lzss(&[0b011, b'a', b'b', 0xEE, 0xF2], 7), b"abababa");
    }
}
