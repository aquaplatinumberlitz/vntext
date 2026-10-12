//! Profile for the MED engine as used by BLACKRAINBOW / 筆柿そふと from 2013
//! on (催眠術3, 催眠術4, 催眠術 RE, WIZARDLINKS), whose scripts are packed in
//! `md_scr.med`.
//!
//! The game splits a loaded script up in memory, so the scripts are read
//! from the game's directory. What is read from memory is the VM state: the
//! length of the running script's code, the program counter (the offset of
//! the instruction it will execute next) twice, and a pointer to the
//! script's name. While a text box is on screen the VM rests on the
//! instruction that clears it, so the counter points just past that one.
//!
//! The archive format is described by GARbro (ArcFormats/DxLib/ArcMED.cs);
//! key recovery and the script format come from a translation tool for
//! this engine (MED.py, SCE.py), author unknown.

use std::collections::HashMap;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

const NEWLINE: u8 = 0;
const PRINT_TEXT: u8 = 1;
const CLEAR_TEXT: u8 = 4;
const CHOICE: u8 = 10;
const PRINT_NAME: u8 = 13;
const CHOICES: u8 = 33;
/// Shown as the speaker of a passage that lists choices.
const CHOICE_LABEL: &str = "選択肢";
/// The VM state, in 32-bit cells: the length of the script's code at 0, the
/// program counter at 4 and again at 8, a pointer to the script's name at 9.
const COUNTER: usize = 4;
const COUNTER_COPY: usize = 8;
const NAME: usize = 9;
/// Script names are short; this much is read to get one.
const NAME_LEN: usize = 64;

struct Script {
    code_len: u32,
    /// offset of every instruction within the code, in order
    offsets: Vec<u32>,
    /// passage shown while the VM is at the instruction at this offset
    passages: HashMap<u32, usize>,
}

struct Block {
    speaker: Option<String>,
    /// one line, or for a menu of choices one line per choice
    text: String,
}

pub struct Archive {
    file: String,
    scripts: Vec<Script>,
    by_name: HashMap<Vec<u8>, usize>,
    /// code lengths of all scripts, to spot a VM state quickly
    code_lens: std::collections::HashSet<u32>,
    blocks: Vec<Block>,
}

fn word(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

fn half(data: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().unwrap()) as usize)
}

/// The files of an archive: `MD`, entry length and count at 4 and 6, then
/// entries of a NUL-padded name followed by `?, size, offset`.
fn entries(data: &[u8]) -> Option<Vec<(String, &[u8])>> {
    if !data.starts_with(b"MD") {
        return None;
    }
    let (length, count) = (half(data, 4)?, half(data, 6)?);
    let name_length = length.checked_sub(12)?;
    (0..count)
        .map(|i| {
            let entry = data.get(0x10 + i * length..0x10 + (i + 1) * length)?;
            let name = entry[..name_length].split(|b| *b == 0).next()?;
            let (size, offset) = (word(entry, name_length + 4)?, word(entry, name_length + 8)?);
            Some((String::from_utf8_lossy(name).into_owned(), data.get(offset..offset.checked_add(size)?)?))
        })
        .collect()
}

impl Archive {
    /// A decrypted script is `size, string offset, string count: u16, xref
    /// count: u16, line count`, then instructions of the form `source line:
    /// u16, length: u8, opcode, arguments` up to the string offset, a zero
    /// byte, the xref offsets and the NUL-terminated strings. Instructions
    /// that take a string carry its index as their first argument.
    ///
    /// What the reader sees as one text box is a run of instructions: maybe
    /// a name, then text and line breaks, ended by the one that clears the box.
    fn add_script(&mut self, name: &str, data: &[u8]) -> Option<()> {
        let (strings_at, string_count, xrefs) = (word(data, 4)? + text::CLEAR, half(data, 8)?, half(data, 10)?);
        let mut strings = Vec::with_capacity(string_count);
        let mut at = strings_at + 1 + 2 * xrefs;
        for _ in 0..string_count {
            let end = at + data.get(at..)?.iter().position(|b| *b == 0)?;
            strings.push(text::decode(&data[at..end]));
            at = end + 1;
        }

        let mut script = Script { code_len: (strings_at - text::CLEAR) as u32, offsets: Vec::new(), passages: HashMap::new() };
        let mut block = Block { speaker: None, text: String::new() };
        let mut pending: Vec<u32> = Vec::new();
        let mut at = text::CLEAR;
        while at + 1 < strings_at {
            let (length, opcode) = (*data.get(at + 2)? as usize, *data.get(at + 3)?);
            let offset = (at - text::CLEAR) as u32;
            let string = || half(data, at + 4).and_then(|i| strings.get(i));
            script.offsets.push(offset);
            match opcode {
                PRINT_NAME => block.speaker = string().map(|name| text::speaker(name)),
                // line breaks only wrap the text in its box
                PRINT_TEXT => block.text.push_str(string().map_or("", |s| s.trim_start_matches('\u{3000}'))),
                _ => {}
            }
            if matches!(opcode, PRINT_NAME | PRINT_TEXT | NEWLINE | CLEAR_TEXT) && !(pending.is_empty() && opcode == NEWLINE) {
                pending.push(offset);
            }
            // A menu of choices: `FF FF, count: u8`, then for each choice the
            // offset of its CHOICE instruction, which carries the text and is
            // where the VM jumps when it is picked. While the menu is up the
            // VM is on this instruction or the one after it.
            if opcode == CHOICES {
                let targets = (0..*data.get(at + 6)? as usize).filter_map(|i| half(data, at + 7 + 2 * i));
                let options: Vec<&str> = targets
                    .filter(|target| data.get(text::CLEAR + target + 3) == Some(&CHOICE))
                    .filter_map(|target| half(data, text::CLEAR + target + 4).and_then(|i| strings.get(i)))
                    .map(|option| option.trim())
                    // menus of the system scripts have placeholders for text
                    .filter(|option| option.chars().any(|c| !c.is_ascii()))
                    .collect();
                if !options.is_empty() {
                    let after = offset + 3 + length as u32;
                    script.passages.extend([(offset, self.blocks.len()), (after, self.blocks.len())]);
                    self.blocks.push(Block { speaker: Some(CHOICE_LABEL.to_string()), text: options.join("\n") });
                }
            }
            if opcode == CLEAR_TEXT {
                if !block.text.is_empty() {
                    script.passages.extend(pending.iter().map(|o| (*o, self.blocks.len())));
                    self.blocks.push(block);
                }
                block = Block { speaker: None, text: String::new() };
                pending.clear();
            }
            at += 3 + length;
        }
        self.code_lens.insert(script.code_len);
        self.by_name.insert(name.as_bytes().to_vec(), self.scripts.len());
        self.scripts.push(script);
        Some(())
    }

    /// Reads the script archives from the game's directory: `md_scr.med`,
    /// and after it any `md_scr2.med` and so on, which a patch adds. A later
    /// archive's scripts replace earlier ones of the same name, as they do
    /// in the game. Only the first archive is sure to hold `_VIEW`, so its
    /// key serves them all.
    fn find(p: &Proc) -> Option<Archive> {
        let is_archive = |name: &str| {
            name.strip_suffix(".med").and_then(|stem| stem.trim_end_matches(|c: char| c.is_ascii_digit()).strip_suffix("_scr")).is_some()
        };
        let paths = p.game_files(is_archive);
        let file = paths.first()?.file_name()?.to_string_lossy().into_owned();
        let mut archive =
            Archive { file, scripts: Vec::new(), by_name: HashMap::new(), code_lens: Default::default(), blocks: Vec::new() };
        let mut key: Option<Vec<u8>> = None;
        for path in paths {
            let Ok(data) = std::fs::read(&path) else { continue };
            let Some(files) = entries(&data) else { continue };
            let own = files.iter().find(|(name, _)| name.starts_with(text::KNOWN_NAME)).and_then(|(_, view)| text::recover_key(view));
            let Some(key) = own.or(key.clone()).inspect(|k| key = Some(k.clone())) else { continue };
            for (name, stored) in files {
                let mut script = stored.to_vec();
                text::decrypt(&mut script, &key);
                // a file that does not parse is not a script; the rest still are
                let _ = archive.add_script(&name, &script);
            }
        }
        (!archive.blocks.is_empty()).then_some(archive)
    }

    /// The script a VM state belongs to: the one its name pointer names,
    /// provided the code length next to it agrees.
    fn script_of(&self, p: &Proc, code_len: u32, name_pointer: u32) -> Option<&Script> {
        let mut name = [0u8; NAME_LEN];
        let n = p.read(name_pointer as u64, &mut name);
        let name = name[..n].split(|b| *b == 0).next()?;
        let script = &self.scripts[*self.by_name.get(name)?];
        (script.code_len == code_len).then_some(script)
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Archive::find(p).map(|archive| Box::new(archive) as Box<dyn Engine>)
}

impl Engine for Archive {
    fn name(&self) -> String {
        format!("MED ({})", self.file)
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let block = &self.blocks[index];
        Passage { speaker: block.speaker.clone(), lines: block.text.lines().map(str::to_string).collect(), raw: block.text.clone() }
    }

    /// Scans for the VM state. It is a fixed part of the program's data, so
    /// a thorough search has nothing more to find.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut candidates: Vec<(u64, u32, u32)> = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            let cell = |k: usize| u32::from_le_bytes(data[k * 4..k * 4 + 4].try_into().unwrap());
            for k in 0..(limit.min(data.len().saturating_sub(4 * (NAME + 1)))) / 4 {
                let code_len = cell(k);
                if self.code_lens.contains(&code_len) && cell(k + COUNTER) == cell(k + COUNTER_COPY) && cell(k + COUNTER) < code_len {
                    candidates.push((base + ((k + COUNTER) * 4) as u64, code_len, cell(k + NAME)));
                }
            }
        });
        let found = candidates.into_iter().filter(|(_, code_len, name)| self.script_of(p, *code_len, *name).is_some());
        Position { cells: found.map(|(addr, ..)| Cell { addr, trusted: true }).collect() }
    }

    /// The cell is the program counter inside the VM state; the instruction
    /// before the one it points at is the one being executed. The script is
    /// looked up every time, since the same state serves every script.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let from = addr - (COUNTER * 4) as u64;
        let mut state = [0u8; (NAME + 1) * 4];
        if p.read(from, &mut state) != state.len() {
            return None;
        }
        let cell = |k: usize| u32::from_le_bytes(state[k * 4..k * 4 + 4].try_into().unwrap());
        let script = self.script_of(p, cell(0), cell(NAME))?;
        let next = script.offsets.binary_search(&cell(COUNTER)).ok()?;
        script.passages.get(script.offsets.get(next.checked_sub(1)?)?).copied()
    }
}
