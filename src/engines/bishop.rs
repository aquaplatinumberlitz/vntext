//! Profile for BISHOP's own engine (牝教師, 淫獄の放課後, 隷従の制服, ...),
//! whose scripts are "BSXScript" 3.0 to 3.3 files (`bsxx.dat`).
//!
//! The game loads the script file whole and keeps its VM state in one fixed
//! block: a pointer to the script followed by its size, with the program
//! counter and the address of the instruction being executed close by.
//!
//! The file format and the instruction lengths come from
//! https://github.com/crskycode/BSXScript_Tool (BSXScript.cs).

use std::collections::HashMap;

use memchr::memmem;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

const SIGNATURE: &[u8] = b"BSXScript 3.";
const SHOW_MESSAGE: u8 = 0x1D;
/// Fewer messages than this and the "script" is something else.
const MIN_MESSAGES: usize = 50;
/// The VM state is searched this far before and after the script pointer.
const VM_BEFORE: u64 = 0x40;
const VM_AFTER: u64 = 0x200;

/// One "show message" instruction. Offsets are relative to the code block,
/// which is how the VM stores them.
struct Show {
    start: u32,
    end: u32,
    message: usize,
    speaker: Option<usize>,
}

pub struct Script {
    version: String,
    data: Vec<u8>,
    shows: Vec<Show>,
    by_start: HashMap<u32, usize>,
    names: Vec<usize>,
    messages: Vec<usize>,
    /// address of the cell that points at the script, inside the VM state
    vm: u64,
    /// the protagonist's family and given name (`\A` and `\B` in the text)
    family: String,
    given: String,
}

/// Length of every instruction but "show message", by opcode.
fn fixed_length(op: u8) -> Option<usize> {
    Some(match op {
        0x00..=0x02 | 0x0A | 0x0B | 0x1E..=0x32 | 0x35 | 0x37 | 0x39 | 0x3F..=0x41 => 1,
        0x03..=0x09 | 0x34 | 0x36 | 0x38 => 5,
        0x0C..=0x19 | 0x3A..=0x3C => 18,
        0x1A..=0x1C | 0x3D => 12,
        0x33 => 13,
        _ => return None,
    })
}

fn word(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

impl Script {
    fn parse(data: Vec<u8>) -> Option<Script> {
        if !data.starts_with(SIGNATURE) {
            return None;
        }
        let version = String::from_utf8_lossy(&data[..16]).trim_end_matches('\0').to_string();
        let (code_at, code_len) = (word(&data, 0x2C)?, word(&data, 0x30)?);
        let code = data.get(code_at..code_at.checked_add(code_len)?)?;

        // A table of string offsets, counted in UTF-16 units from the start of a block.
        let table = |list_at: usize, bytes: usize, block: usize| -> Option<Vec<usize>> {
            let (list, count, block) = (word(&data, list_at)?, word(&data, bytes)? / 4, word(&data, block)?);
            (0..count).map(|i| Some(block + word(&data, list + 4 * i)? * 2)).collect()
        };
        let names = table(0x88, 0x8C, 0x90)?;
        let messages = table(0x98, 0x9C, 0xA0)?;

        let mut shows = Vec::new();
        let mut at = 0;
        while at < code.len() {
            let op = code[at];
            if op == SHOW_MESSAGE {
                // 1D | kind: u8 | message: u32 | speaker: u32 (kind > 0) | ...
                let kind = *code.get(at + 1)? as usize;
                let extra = if kind > 1 { 4 * word(code, at + 10)? } else { 0 };
                let len = extra + 4 * kind + 6;
                let message = word(code, at + 2)?;
                let speaker = if kind > 0 { Some(word(code, at + 6)?) } else { None };
                if message < messages.len() && speaker.is_none_or(|s| s < names.len()) {
                    shows.push(Show { start: at as u32, end: (at + len) as u32, message, speaker });
                }
                at += len;
            } else if op == 0x3E {
                at += 5 + 4 * word(code, at + 1)?;
            } else {
                at += fixed_length(op)?;
            }
        }
        if shows.len() < MIN_MESSAGES {
            return None;
        }
        let by_start = shows.iter().enumerate().map(|(i, s)| (s.start, i)).collect();
        Some(Script { version, data, shows, by_start, names, messages, vm: 0, family: String::new(), given: String::new() })
    }

    /// Looks for a loaded script: the signature, then a cell that points at
    /// it and is followed by a plausible size.
    fn find(p: &Proc) -> Option<Script> {
        let finder = memmem::Finder::new(SIGNATURE);
        let mut bases: Vec<u64> = Vec::new();
        p.scan(false, |base, data, limit, _| {
            bases.extend(finder.find_iter(data).take_while(|off| *off < limit).map(|off| base + off as u64));
        })
        .ok()?;
        if bases.is_empty() {
            return None;
        }
        let mut owners: Vec<(u64, u64)> = Vec::new();
        p.scan(false, |base, data, limit, _| {
            for (k, cell) in data[..limit & !3].chunks_exact(4).enumerate() {
                let value = u32::from_le_bytes(cell.try_into().unwrap()) as u64;
                if bases.contains(&value) {
                    owners.push((base + k as u64 * 4, value));
                }
            }
        })
        .ok()?;
        for (cell, base) in owners {
            let mut size = [0u8; 4];
            p.read(cell + 4, &mut size);
            let size = u32::from_le_bytes(size) as usize;
            if !(0x1_0000..=64 << 20).contains(&size) {
                continue;
            }
            let mut data = vec![0u8; size];
            if p.read(base, &mut data) != size {
                continue;
            }
            if let Some(script) = Script::parse(data) {
                let (family, given) = protagonist(p).unwrap_or_else(|| ("主人公".to_string(), "主人公".to_string()));
                return Some(Script { vm: cell, family, given, ..script });
            }
        }
        None
    }

    fn string(&self, at: usize) -> String {
        let units: Vec<u16> = self.data[at.min(self.data.len())..]
            .chunks_exact(2)
            .map(|u| u16::from_le_bytes([u[0], u[1]]))
            .take_while(|u| *u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    /// Turns stored text into what the game shows. `\n` breaks the line for
    /// the text box only, so it is dropped along with the indent after it;
    /// `\b` and `\s` change the text size.
    fn readable(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some('n') => while chars.next_if_eq(&'\u{3000}').is_some() {},
                    Some('b' | 's') => {}
                    Some('A') => out.push_str(&self.family),
                    Some('B') => out.push_str(&self.given),
                    Some(other) => out.extend(['\\', other]),
                    None => out.push('\\'),
                },
                '\u{E000}' => out.push('♥'),
                // Glyphs of the game's own font. Going by where they occur
                // (U+E042 for あ, U+E0A2 for ア, ...) they are kana with a
                // voicing mark; this is inferred, not documented.
                '\u{E001}'..='\u{E0FF}' => {
                    out.extend(char::from_u32(0x3000 + (c as u32 & 0xFF)));
                    out.push('゛');
                }
                _ => out.push(c),
            }
        }
        out
    }
}

/// The protagonist's (family, given) name, which the player can change. The
/// game keeps it in `bsxx.ini` in its directory: NUL-separated UTF-16 strings,
/// the third being the given name and the fourth the family name.
fn protagonist(p: &Proc) -> Option<(String, String)> {
    let dir = std::fs::read_link(format!("/proc/{}/cwd", p.pid)).ok()?;
    let raw = std::fs::read(dir.join("bsxx.ini")).ok()?;
    let units: Vec<u16> = raw.chunks_exact(2).map(|u| u16::from_le_bytes([u[0], u[1]])).collect();
    let mut strings = units.split(|u| *u == 0).map(String::from_utf16_lossy);
    let given = strings.nth(2).filter(|s| !s.is_empty())?;
    let family = strings.next().filter(|s| !s.is_empty())?;
    Some((family, given))
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Script::find(p).map(|script| Box::new(script) as Box<dyn Engine>)
}

impl Engine for Script {
    fn name(&self) -> String {
        format!("BISHOP ({})", self.version)
    }

    fn len(&self) -> usize {
        self.shows.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let show = &self.shows[index];
        let raw = self.string(self.messages[show.message]);
        let speaker = show.speaker.map(|s| self.readable(&self.string(self.names[s]))).filter(|s| !s.is_empty());
        Passage { speaker, lines: vec![self.readable(&raw)], raw }
    }

    /// The VM state does not move, so there is nothing more to find by
    /// searching thoroughly. A cell qualifies while the VM waits on a
    /// message: it holds the start of a "show message" instruction and the
    /// program counter next to it holds that instruction's end.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let from = self.vm - VM_BEFORE;
        let mut block = vec![0u8; (VM_BEFORE + VM_AFTER) as usize];
        let n = p.read(from, &mut block);
        let cells: Vec<u32> = block[..n & !3].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let found = cells.iter().enumerate().filter(|(_, value)| {
            self.by_start.get(value).is_some_and(|i| cells.contains(&self.shows[*i].end))
        });
        Position { cells: found.map(|(k, _)| Cell { addr: from + k as u64 * 4, trusted: true }).collect() }
    }

    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut cell = [0u8; 4];
        if p.read(addr, &mut cell) != 4 {
            return None;
        }
        self.by_start.get(&u32::from_le_bytes(cell)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script() -> Script {
        Script {
            version: String::new(),
            data: Vec::new(),
            shows: Vec::new(),
            by_start: HashMap::new(),
            names: Vec::new(),
            messages: Vec::new(),
            vm: 0,
            family: "金本".to_string(),
            given: "一也".to_string(),
        }
    }

    #[test]
    fn readable_text() {
        let s = script();
        assert_eq!(s.readable("「あひィインッ\u{E000}　何これぇっ\u{E000}\\n　気持ちいい」"), "「あひィインッ♥　何これぇっ♥気持ちいい」");
        assert_eq!(s.readable("\\b「\\A\\B君、\\n　ちょっといいかしら？」"), "「金本一也君、ちょっといいかしら？」");
        assert_eq!(s.readable("\\B"), "一也");
        assert_eq!(s.readable("\u{E042}あ"), "あ゛あ");
    }
}
