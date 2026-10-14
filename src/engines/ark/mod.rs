//! Profile for Astronauts' ArkEngine, a Unity (Mono) engine: the
//! 極限痴漢特異点 series.
//!
//! Scripts are text files packed and encrypted in
//! `<game>_Data/StreamingAssets/scripts.bin`, and are read from there. In
//! memory, the game's `ScriptParser` object holds the name of the script it
//! is running, the text it is showing and the number of the line it stands
//! on. All three are found by what they contain rather than by their place
//! in the object, which differs between builds of the game.
//!
//! Everything here comes from the game's own code, decompiled from
//! `Assembly-CSharp.dll` with ILSpy.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use memchr::memmem;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

/// `ScriptParser` keeps these one-character strings in consecutive fields
/// (its separators for splitting script lines), which is how it is found.
const SEPARATORS: [u16; 5] = [b'\n' as u16, b'\r' as u16, b' ' as u16, b'[' as u16, b'/' as u16];
/// A Mono string is `vtable, monitor, length: i32, UTF-16 units`.
const STRING_LENGTH_AT: u64 = 16;
const STRING_CHARS_AT: u64 = 20;
const MAX_STRING: usize = 2000;
/// How much of the `ScriptParser` object is examined, before and after the
/// first separator field.
const BEFORE: u64 = 0x40;
const AFTER: u64 = 0x400;

struct Script {
    /// the script's text boxes by what they say, without whitespace
    by_text: HashMap<String, Vec<usize>>,
    menus: Vec<usize>,
}


pub struct Game {
    scripts: HashMap<String, Script>,
    blocks: Vec<text::Block>,
    /// the object as last read, and what it meant
    last: Mutex<(Vec<u8>, Option<usize>)>,
}

/// The game's script pack: `scripts.idx` and `scripts.bin` under
/// `<name>_Data/StreamingAssets` next to the executable.
fn pack(p: &Proc) -> Option<(PathBuf, PathBuf)> {
    for dir in p.game_dirs() {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let assets = entry.path().join("StreamingAssets");
            let (idx, bin) = (assets.join("scripts.idx"), assets.join("scripts.bin"));
            if entry.file_name().to_string_lossy().ends_with("_Data") && idx.is_file() && bin.is_file() {
                return Some((idx, bin));
            }
        }
    }
    None
}

fn mono_string(p: &Proc, at: u64) -> Option<String> {
    let mut len = [0u8; 4];
    if at < 0x1_0000 || !at.is_multiple_of(8) || p.read(at + STRING_LENGTH_AT, &mut len) != 4 {
        return None;
    }
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_STRING {
        return None;
    }
    let mut units = vec![0u8; len * 2];
    if p.read(at + STRING_CHARS_AT, &mut units) != units.len() {
        return None;
    }
    String::from_utf16(&units.chunks_exact(2).map(|u| u16::from_le_bytes([u[0], u[1]])).collect::<Vec<_>>()).ok()
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let (idx, bin) = pack(p)?;
        let (index, bin) = (text::index(&std::fs::read(idx).ok()?)?, std::fs::read(bin).ok()?);
        let mut game = Game { scripts: HashMap::new(), blocks: Vec::new(), last: Mutex::new((Vec::new(), None)) };
        for entry in index {
            // each file is one string, packed and encrypted
            let Some(stored) = bin.get(entry.at..entry.at + entry.size).and_then(text::decrypt) else { continue };
            let Some(source) = text::Pack(&stored).string() else { continue };
            let mut script = Script { by_text: HashMap::new(), menus: Vec::new() };
            for block in text::blocks(source) {
                match block.menu {
                    true => script.menus.push(game.blocks.len()),
                    false => script.by_text.entry(text::essence(&block.text)).or_default().push(game.blocks.len()),
                }
                game.blocks.push(block);
            }
            let name = entry.name.rsplit('/').next().unwrap_or(&entry.name).to_string();
            game.scripts.insert(name, script);
        }
        (!game.blocks.is_empty()).then_some(game)
    }

    /// Works out what is on screen from the `ScriptParser` object: one of
    /// its string fields names a script, another holds a text box of that
    /// script, and its line counter holds the number of the line after that
    /// box.
    ///
    /// A menu leaves no text of its own in the object: the text is still
    /// that of the box before it, while the line counter has moved on into
    /// the menu. So when the box's own line number is gone from the object
    /// and a number inside the menu that follows it is there, the menu is up.
    fn shown(&self, p: &Proc, object: &[u8]) -> Option<usize> {
        let strings: Vec<String> = object
            .chunks_exact(8)
            .filter_map(|field| mono_string(p, u64::from_le_bytes(field.try_into().unwrap())))
            .collect();
        let numbers: HashSet<u32> = object.chunks_exact(4).map(|n| u32::from_le_bytes(n.try_into().unwrap())).collect();
        let mut found = None;
        for script in strings.iter().filter_map(|name| self.scripts.get(name)) {
            for boxes in strings.iter().filter_map(|shown| script.by_text.get(&text::essence(shown))) {
                // the script may say the same thing more than once
                if let Some(exact) = boxes.iter().find(|b| numbers.contains(&self.blocks[**b].line)) {
                    return Some(*exact);
                }
                let menu = boxes.iter().map(|b| b + 1).find(|next| {
                    script.menus.contains(next) && (self.blocks[*next].first..=self.blocks[*next].line).any(|l| numbers.contains(&l))
                });
                found = menu.or(found).or(boxes.first().copied());
            }
        }
        found
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        "ArkEngine".to_string()
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let block = &self.blocks[index];
        Passage { speaker: block.speaker.clone(), lines: block.text.lines().map(str::to_string).collect(), raw: block.text.clone() }
    }

    /// Finds `ScriptParser` objects: five consecutive fields pointing at the
    /// strings "\n", "\r", " ", "[" and "/". The object does not move (Unity's
    /// Mono collector never moves anything), so a thorough search finds
    /// nothing more.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        // every one-character string that is one of the separators
        let mut strings: [HashSet<u64>; 5] = Default::default();
        let _ = p.scan(false, |base, data, limit, _| {
            for (which, unit) in SEPARATORS.iter().enumerate() {
                let [low, high] = unit.to_le_bytes();
                let stored = [1, 0, 0, 0, low, high, 0, 0];
                for hit in memmem::find_iter(data, &stored).take_while(|hit| *hit < limit) {
                    let at = (base + hit as u64).wrapping_sub(STRING_LENGTH_AT);
                    if at.is_multiple_of(8) {
                        strings[which].insert(at);
                    }
                }
            }
        });
        let mut cells = Vec::new();
        if strings.iter().all(|set| !set.is_empty()) {
            let _ = p.scan(false, |base, data, limit, _| {
                let field = |k: usize| u64::from_le_bytes(data[k * 8..k * 8 + 8].try_into().unwrap());
                for k in 0..(limit.min(data.len().saturating_sub(8 * strings.len()))) / 8 {
                    if strings.iter().enumerate().all(|(i, set)| set.contains(&field(k + i))) {
                        cells.push(Cell { addr: base + k as u64 * 8, trusted: true });
                    }
                }
            });
        }
        Position { cells }
    }

    /// The cell is the first separator field of a `ScriptParser`. Its
    /// strings are only followed again when something in the object changed.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut object = vec![0u8; (BEFORE + AFTER) as usize];
        let n = p.read(addr - BEFORE, &mut object);
        object.truncate(n & !7);
        let mut last = self.last.lock().unwrap();
        if last.0 != object {
            *last = (object.clone(), self.shown(p, &object));
        }
        last.1
    }
}
