//! Profile for the old BLACKRAINBOW engine (催眠術2, 催眠術2 URA), whose
//! scripts are `.spt` files packed into `sj_spt.dat` / `sjfd_spt.dat`.
//!
//! Unlike the other engines this one does not keep the script file in memory
//! (it turns each script into a linked list of commands), so the pack is read
//! from the game's directory. What it does keep, at a fixed place, is the
//! header of the running script followed by the index of the command being
//! executed.
//!
//! The pack and script formats come from an English patching tool for these
//! games (blackrb_spt.py, repack.py), author unknown.

use std::collections::HashMap;

use memchr::memmem;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

/// A script starts with its name, NUL-padded to this size, then the number
/// of commands and the size of what follows.
const HEADER: usize = 68;
/// Each pack entry: name padded to 32 bytes, then the script's size.
const ENTRY_NAME: usize = 0x20;
const MESSAGE: u32 = 0x08;
/// A menu of choices.
const MENU: u32 = 0x0E;
/// Shown before each choice of a menu.
const CHOICE_MARK: &str = "▶ ";
/// In the VM state, the command index sits this far after the script header
/// (count, size and a pointer come in between).
const INDEX_AT: usize = HEADER + 12;

struct Script {
    /// the first HEADER + 8 bytes, as the VM state repeats them
    head: Vec<u8>,
    /// passage index of each command, for the commands that show a message
    /// or a menu
    passages: Vec<Option<usize>>,
}

pub struct Pack {
    file: String,
    scripts: Vec<Script>,
    by_head: HashMap<Vec<u8>, usize>,
    /// every message and menu, in pack order
    messages: Vec<Message>,
}

enum Message {
    /// stored text, as the engine keeps it
    Text(Vec<u8>),
    /// the choices of a menu
    Menu(Vec<String>),
}

fn word(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

impl Pack {
    /// A pack is `version, ?, count, base`, then `count` offsets from `base`
    /// to the entries. A script is its header, then `count` commands of the
    /// form `command: u32, length: u32, arguments`. A message command's
    /// arguments are `flag, voice, n, channel length, text length` followed
    /// by the voice channel and the text.
    fn parse(file: String, data: &[u8]) -> Option<Pack> {
        let (count, base) = (word(data, 8)?, word(data, 12)?);
        let mut scripts = Vec::new();
        let mut messages = Vec::new();
        for i in 0..count {
            let entry = base + word(data, 16 + 4 * i)?;
            let size = word(data, entry + ENTRY_NAME)?;
            let script = data.get(entry + ENTRY_NAME + 4..entry + ENTRY_NAME + 4 + size)?;
            let commands = word(script, HEADER)?;
            let mut passages = Vec::with_capacity(commands);
            let mut at = HEADER + 8;
            for _ in 0..commands {
                let (command, length) = (word(script, at)? as u32, word(script, at + 4)?);
                let arguments = script.get(at + 8..at + 8 + length)?;
                at += 8 + length;
                if command == MENU {
                    // `?, ?, length` and then the choices, each ended by a
                    // backslash and a zero, in plain Shift-JIS
                    let listed = arguments.get(12..)?;
                    let choices = text::menu(listed);
                    passages.push((!choices.is_empty()).then_some(messages.len()));
                    if !choices.is_empty() {
                        messages.push(Message::Menu(choices));
                    }
                    continue;
                }
                if command != MESSAGE {
                    passages.push(None);
                    continue;
                }
                let (channel, length) = (word(arguments, 12)?, word(arguments, 16)?);
                passages.push(Some(messages.len()));
                messages.push(Message::Text(arguments.get(20 + channel..20 + channel + length)?.to_vec()));
            }
            scripts.push(Script { head: script[..HEADER + 8].to_vec(), passages });
        }
        let by_head = scripts.iter().enumerate().map(|(i, s)| (s.head.clone(), i)).collect();
        (!messages.is_empty()).then_some(Pack { file, scripts, by_head, messages })
    }

    /// Reads the script pack from the game's directory.
    fn find(p: &Proc) -> Option<Pack> {
        let path = p.game_file(|name| name.ends_with("_spt.dat"))?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        Pack::parse(name, &std::fs::read(path).ok()?)
    }

    /// The passage on screen according to a VM state: the script header,
    /// then `INDEX_AT` bytes in the index of the command being executed. The
    /// VM rests on a message command for as long as the message is shown.
    fn shown(&self, state: &[u8]) -> Option<usize> {
        let script = &self.scripts[*self.by_head.get(state.get(..HEADER + 8)?)?];
        *script.passages.get(word(state, INDEX_AT)?)?
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Pack::find(p).map(|pack| Box::new(pack) as Box<dyn Engine>)
}

impl Engine for Pack {
    fn name(&self) -> String {
        format!("BLACKRAINBOW spt ({})", self.file)
    }

    fn len(&self) -> usize {
        self.messages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        match &self.messages[index] {
            Message::Text(stored) => {
                let raw = text::decode(stored);
                let (speaker, line) = text::split(&raw);
                Passage { speaker, lines: vec![line], raw }
            }
            Message::Menu(choices) => {
                let lines: Vec<String> = choices.iter().map(|choice| format!("{CHOICE_MARK}{choice}")).collect();
                Passage { speaker: None, raw: lines.join("\n"), lines }
            }
        }
    }

    /// The VM state is recognised by a script's header, command count and
    /// size appearing together outside any file. It does not move, so a
    /// thorough search finds nothing more.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let finder = memmem::Finder::new(b".spt\0");
        let mut cells = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for hit in finder.find_iter(data).take_while(|hit| *hit < limit) {
                // the name starts after the NUL before it
                let start = memchr::memrchr(0, &data[..hit]).map_or(0, |nul| nul + 1);
                // The engine keeps the header in other places too. In the VM
                // state the command index follows, and after it the script's
                // name once more.
                let Some(state) = data.get(start..start + INDEX_AT + 4 + ENTRY_NAME) else { continue };
                let name = &state[..hit - start + 5];
                if self.by_head.contains_key(&state[..HEADER + 8]) && state[INDEX_AT + 4..].starts_with(name) {
                    cells.push(Cell { addr: base + start as u64, trusted: true });
                }
            }
        });
        Position { cells }
    }

    /// The cell is the script header in the VM state; the script may change
    /// under it, so both the header and the index are read every time.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut state = [0u8; INDEX_AT + 4];
        if p.read(addr, &mut state) != state.len() {
            return None;
        }
        self.shown(&state)
    }
}
