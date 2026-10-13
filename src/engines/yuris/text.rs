//! File formats of the YU-RIS engine: YPF archives and compiled scripts
//! (YSTB), as described in https://github.com/shimamura-sakura/yuri (yuri.md).

use encoding_rs::SHIFT_JIS;

fn word(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

/// A file of an archive: its name and where its data is.
pub struct Entry {
    pub name: String,
    pub compressed: bool,
    pub size: usize,
    pub at: u64,
}

/// Size of the fixed part of an archive: `YPF\0`, version, file count and
/// the size of the entry table.
pub const YPF_HEAD: usize = 32;

/// How long the entry table of an archive is, from its fixed part. Since
/// version 300 the stored size includes the fixed part.
pub fn ypf_table_len(head: &[u8]) -> Option<usize> {
    if !head.starts_with(b"YPF\0") {
        return None;
    }
    let (version, size) = (word(head, 4)?, word(head, 12)?);
    if version >= 300 { size.checked_sub(YPF_HEAD) } else { Some(size) }
}

/// Reads an archive's entry table. An entry is `name hash, name length, name,
/// kind, compression, size, stored size, offset, data hash`. The name length
/// is inverted and passed through a table of swaps, the name is inverted,
/// and the offset is 64 bits wide since version 477.
pub fn ypf_entries(head: &[u8], table: &[u8]) -> Option<Vec<Entry>> {
    const SWAPS: [(u8, u8); 12] =
        [(3, 72), (17, 25), (46, 50), (6, 53), (9, 11), (12, 16), (13, 19), (21, 27), (28, 30), (32, 35), (38, 41), (44, 47)];
    let name_len = |stored: u8| {
        let n = !stored;
        SWAPS.iter().find_map(|(a, b)| if n == *a { Some(*b) } else if n == *b { Some(*a) } else { None }).unwrap_or(n) as usize
    };
    let (version, count) = (word(head, 4)?, word(head, 8)?);
    let offset_len = if version >= 477 { 8 } else { 4 };
    let mut entries = Vec::with_capacity(count);
    let mut at = 0;
    for _ in 0..count {
        let len = name_len(*table.get(at + 4)?);
        let name: Vec<u8> = table.get(at + 5..at + 5 + len)?.iter().map(|b| !b).collect();
        at += 5 + len;
        let compressed = *table.get(at + 1)? == 1;
        let size = word(table, at + 6)?;
        let offset = table.get(at + 10..at + 10 + offset_len)?.iter().rev().fold(0u64, |n, b| n << 8 | *b as u64);
        entries.push(Entry { name: SHIFT_JIS.decode(&name).0.to_lowercase(), compressed, size, at: offset });
        at += 10 + offset_len + 4;
    }
    Some(entries)
}

pub fn inflate(stored: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib(stored).ok()
}

/// The command numbers that matter, from the engine's command list
/// (`ysc.ybn`): `YSCM`, version, count, padding, then per command its name,
/// a parameter count and the parameters (a name and two bytes each).
pub struct Commands {
    pub count: usize,
    pub word: u8,
    pub return_code: u8,
    pub gosub: u8,
}

pub fn commands(data: &[u8]) -> Option<Commands> {
    if !data.starts_with(b"YSCM") {
        return None;
    }
    let count = word(data, 8)?;
    let mut names = Vec::with_capacity(count);
    let mut at = 16;
    let string = |at: usize| Some(at + data.get(at..)?.iter().position(|b| *b == 0)?);
    for _ in 0..count {
        let end = string(at)?;
        names.push(&data[at..end]);
        let parameters = *data.get(end + 1)? as usize;
        at = end + 2;
        for _ in 0..parameters {
            at = string(at)? + 3;
        }
    }
    let number = |name: &[u8]| names.iter().position(|n| *n == name).map(|i| i as u8);
    Some(Commands { count, word: number(b"WORD")?, return_code: number(b"RETURNCODE")?, gosub: number(b"GOSUB")? })
}

/// The sections of a compiled script, still encrypted: commands (4 bytes
/// each: number, argument count, ...), arguments (12 bytes each: id, type,
/// operator, data size, data offset), data, and source line numbers.
pub struct Sections<'a> {
    pub count: usize,
    pub commands: &'a [u8],
    pub arguments: &'a [u8],
    pub data: &'a [u8],
    pub lines: &'a [u8],
}

/// Size of a compiled script's header: `YSTB`, version, command count and
/// the sizes of the four sections.
pub const YSTB_HEAD: usize = 32;

/// Splits a compiled script of the newer layout (version 300 on) into its
/// sections; returns the offsets of the data and line number sections too.
pub fn sections(script: &[u8]) -> Option<(Sections<'_>, usize, usize)> {
    if !script.starts_with(b"YSTB") || word(script, 4)? < 300 {
        return None;
    }
    let (count, commands, arguments, data, lines) =
        (word(script, 8)?, word(script, 12)?, word(script, 16)?, word(script, 20)?, word(script, 24)?);
    if commands != count * 4 || lines != count * 4 {
        return None;
    }
    let at = [YSTB_HEAD, YSTB_HEAD + commands, YSTB_HEAD + commands + arguments, YSTB_HEAD + commands + arguments + data];
    let sections = Sections {
        count,
        commands: script.get(at[0]..at[1])?,
        arguments: script.get(at[1]..at[2])?,
        data: script.get(at[2]..at[3])?,
        lines: script.get(at[3]..at[3] + lines)?,
    };
    Some((sections, at[2], at[3]))
}

/// Each section is XORed with the same four-byte key, restarting at its
/// beginning.
fn clear(section: &[u8], key: [u8; 4]) -> Vec<u8> {
    section.iter().zip(key.iter().cycle()).map(|(b, k)| b ^ k).collect()
}

/// Works out the key shared by a game's scripts. Source line numbers are
/// small, so the upper three bytes of the first one are the key's. The
/// lowest byte is the one for which, in every script, line numbers never
/// decrease and every command number exists.
pub fn find_key(scripts: &[Sections], commands: usize) -> Option<[u8; 4]> {
    let first = scripts.iter().find(|s| s.count >= 4)?;
    let upper = [first.lines[1], first.lines[2], first.lines[3]];
    let fits = |low: u8| {
        scripts.iter().all(|s| {
            let numbers = s.lines.chunks_exact(4).map(|n| u32::from_le_bytes([n[0] ^ low, n[1] ^ upper[0], n[2] ^ upper[1], n[3] ^ upper[2]]));
            numbers.clone().zip(numbers.skip(1)).all(|(a, b)| a <= b) && s.commands.chunks_exact(4).all(|c| ((c[0] ^ low) as usize) < commands)
        })
    };
    let mut lows = (0..=255u8).filter(|low| fits(*low));
    let low = lows.next()?;
    lows.next().is_none().then_some([low, upper[0], upper[1], upper[2]])
}

/// A line of text: the command that ends it, and what it says.
pub struct Line {
    pub ends_at: u32,
    pub speaker: Option<String>,
    pub text: String,
}

/// Collects a script's text. Text is `WORD` commands, whose one argument's
/// data is the raw string, up to a `RETURNCODE`, which is where the script
/// waits for the reader. A spoken line starts with `【name】`.
pub fn lines(script: &Sections, key: [u8; 4], commands: &Commands) -> Vec<Line> {
    let (list, arguments, data) = (clear(script.commands, key), clear(script.arguments, key), clear(script.data, key));
    let mut found = Vec::new();
    let mut said = Vec::new();
    let mut argument = 0;
    for (number, command) in list.chunks_exact(4).enumerate() {
        if command[0] == commands.word && command[1] >= 1 {
            let (size, at) = (word(&arguments, argument * 12 + 4), word(&arguments, argument * 12 + 8));
            if let Some(raw) = size.zip(at).and_then(|(size, at)| data.get(at..at + size)) {
                said.extend_from_slice(raw);
            }
        } else if command[0] == commands.return_code && !said.is_empty() {
            let whole = SHIFT_JIS.decode(&said).0;
            let whole = whole.trim_start_matches('\u{3000}');
            let (speaker, text) = match whole.strip_prefix('【').and_then(|rest| rest.split_once('】')) {
                Some((name, text)) => (Some(name.to_string()), text),
                None => (None, whole),
            };
            found.push(Line { ends_at: number as u32, speaker, text: text.to_string() });
            said.clear();
        }
        argument += command[1] as usize;
    }
    found
}

/// A menu of choices.
pub struct Menu {
    /// the number of the command after the one that puts it up
    pub returns_to: u32,
    pub choices: Vec<String>,
}

/// The routine of the engine's script library that puts up a menu. It is
/// called with `GOSUB`: its name, then a label for each choice.
const MENU_ROUTINE: &[u8] = b"ES.SEL.SET";

/// A string as an argument holds it: `M`, its length in two bytes and the
/// string in quotes of one kind or another, sometimes nested.
fn literal(argument: &[u8]) -> Option<String> {
    let quoted = argument.strip_prefix(b"M")?.get(2..)?;
    let text = SHIFT_JIS.decode(quoted).0;
    Some(text.trim_matches(|c| matches!(c, '"' | '\'' | '`')).to_string())
}

/// Collects a script's menus.
pub fn menus(script: &Sections, key: [u8; 4], commands: &Commands) -> Vec<Menu> {
    let (list, arguments, data) = (clear(script.commands, key), clear(script.arguments, key), clear(script.data, key));
    let value = |argument: usize| {
        let (size, at) = (word(&arguments, argument * 12 + 4)?, word(&arguments, argument * 12 + 8)?);
        data.get(at..at + size)
    };
    let mut found = Vec::new();
    let mut argument = 0;
    for (number, command) in list.chunks_exact(4).enumerate() {
        let count = command[1] as usize;
        let routine = value(argument).and_then(literal).filter(|_| command[0] == commands.gosub && count >= 1);
        if routine.is_some_and(|name| name.as_bytes().eq_ignore_ascii_case(MENU_ROUTINE)) {
            let labels = (1..count).filter_map(|k| value(argument + k).and_then(literal));
            let choices: Vec<String> = labels.filter(|label| !label.is_empty()).collect();
            if !choices.is_empty() {
                found.push(Menu { returns_to: number as u32 + 1, choices });
            }
        }
        argument += count;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_text_and_key() {
        let key = [0x60, 0x5E, 0x41, 0x4A];
        let commands = Commands { count: 119, word: 106, return_code: 79, gosub: 29 };
        let said = SHIFT_JIS.encode("【獅堂】「やっと昼休みか」").0.into_owned();
        // WORD with one argument, RETURNCODE, two more commands; lines 3, 3, 4, 9
        let list = [106u8, 1, 0, 0, 79, 0, 0, 0, 5, 0, 0, 0, 7, 0, 0, 0];
        let mut arguments = vec![0u8; 12];
        arguments[4..8].copy_from_slice(&(said.len() as u32).to_le_bytes());
        let numbers: Vec<u8> = [3u32, 3, 4, 9].iter().flat_map(|n| n.to_le_bytes()).collect();
        let (list, arguments, data, numbers) = (clear(&list, key), clear(&arguments, key), clear(&said, key), clear(&numbers, key));
        let script = Sections { count: 4, commands: &list, arguments: &arguments, data: &data, lines: &numbers };
        // a script whose line numbers take every low byte pins the key down
        let plain: Vec<u8> = (0..256u32).flat_map(|n| n.to_le_bytes()).collect();
        let (long_list, long_numbers) = (clear(&[5u8, 0, 0, 0].repeat(256), key), clear(&plain, key));
        let long = Sections { count: 256, commands: &long_list, arguments: &[], data: &[], lines: &long_numbers };
        assert_eq!(find_key(&[long], commands.count), Some(key));
        let found = lines(&script, key, &commands);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].ends_at, found[0].speaker.as_deref(), found[0].text.as_str()), (1, Some("獅堂"), "「やっと昼休みか」"));
    }

    #[test]
    fn menu() {
        let key = [1, 2, 3, 4];
        let commands = Commands { count: 119, word: 106, return_code: 79, gosub: 29 };
        // some command, then GOSUB with the routine, two labels and an empty one
        let values: [&[u8]; 4] = [b"M\x0c\x00\"ES.SEL.SET\"", b"M\x06\x00`\"Go\"`", b"M\x06\x00\"Stay\"", b"M\x02\x00''"];
        let mut data = Vec::new();
        let mut arguments = Vec::new();
        for value in values {
            arguments.extend([0u8; 4]);
            arguments.extend((value.len() as u32).to_le_bytes());
            arguments.extend((data.len() as u32).to_le_bytes());
            data.extend_from_slice(value);
        }
        let list = [5u8, 0, 0, 0, 29, 4, 0, 0];
        let (list, arguments, data) = (clear(&list, key), clear(&arguments, key), clear(&data, key));
        let script = Sections { count: 2, commands: &list, arguments: &arguments, data: &data, lines: &[] };
        let found = menus(&script, key, &commands);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].returns_to, found[0].choices.clone()), (2, vec!["Go".to_string(), "Stay".to_string()]));
    }

    #[test]
    fn archive_name_lengths() {
        // a table with one entry named "a.ybn"
        let mut table = vec![0u8; 4];
        table.push(!5u8);
        table.extend(b"a.ybn".iter().map(|b| !b));
        table.extend([0, 1]);
        table.extend(7u32.to_le_bytes());
        table.extend(5u32.to_le_bytes());
        table.extend(0x1234u64.to_le_bytes());
        table.extend([0u8; 4]);
        let mut head = b"YPF\0".to_vec();
        head.extend(494u32.to_le_bytes());
        head.extend(1u32.to_le_bytes());
        head.extend(((YPF_HEAD + table.len()) as u32).to_le_bytes());
        head.extend([0u8; 16]);
        assert_eq!(ypf_table_len(&head), Some(table.len()));
        let entries = ypf_entries(&head, &table).unwrap();
        assert_eq!((entries[0].name.as_str(), entries[0].compressed, entries[0].size, entries[0].at), ("a.ybn", true, 5, 0x1234));
    }
}
