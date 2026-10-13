//! Profile for Ninetail's Unity (Mono, 32-bit) engine: the VenusBlood games.
//!
//! Each scene is a JSON text asset in a bundle of its own under
//! `Standalone/scenario`, a list of cuts; a cut that shows text has it in
//! `window.texts[0]`. They are read from there. In memory, the game's
//! `AdvDataControlUser` object holds the name of the scene being played, the
//! number of the cut on screen and the number of cuts in the scene.
//!
//! Everything here comes from the game's own code, decompiled from
//! `Assembly-CSharp.dll` with ILSpy.

use std::collections::HashMap;
use std::path::PathBuf;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

/// A 32-bit Mono string is `vtable, monitor, length: i32, UTF-16 units`.
const STRING_LENGTH_AT: usize = 8;
const STRING_CHARS_AT: usize = 12;
/// Scene names are short.
const MAX_NAME: usize = 24;
const NAME_UNITS: std::ops::RangeInclusive<usize> = 2..=MAX_NAME;
/// `AdvDataControlUser` from its `filename` field on, in 4-byte fields:
/// `filename, jumpfile, jumplabel` (strings), other references, then
/// `indexNum, indexMax`. Where the two numbers are depends on how many
/// references the build has, so they are looked for.
const FIELDS: usize = 16;
const NUMBERS_FROM: usize = 3;
/// The table of character names, in the game's `resources.assets`.
const CHARACTERS: &str = "advdb_dataset_jp";
/// Shown before each choice of a cut that asks for one.
const CHOICE_MARK: &str = "▶ ";

pub struct Game {
    title: String,
    /// by scene name in lower case: for each cut, its passage
    scenes: HashMap<String, Vec<Option<usize>>>,
    passages: Vec<text::Shown>,
}

/// The game's directory: the one with `Standalone/scenario` in it.
fn home(p: &Proc) -> Option<PathBuf> {
    p.game_dirs().into_iter().find(|dir| dir.join("Standalone/scenario").is_dir())
}

fn field(fields: &[u8], k: usize) -> u32 {
    u32::from_le_bytes(fields[k * 4..k * 4 + 4].try_into().unwrap())
}

/// The scene name in a Mono string at the start of `data`, in lower case.
fn name_at(data: &[u8]) -> Option<String> {
    let len = u32::from_le_bytes(data.get(STRING_LENGTH_AT..STRING_CHARS_AT)?.try_into().unwrap()) as usize;
    if !NAME_UNITS.contains(&len) {
        return None;
    }
    let units = data.get(STRING_CHARS_AT..STRING_CHARS_AT + len * 2)?;
    let plain = units.chunks_exact(2).all(|u| u[1] == 0 && (u[0].is_ascii_alphanumeric() || u[0] == b'_'));
    plain.then(|| units.iter().step_by(2).map(|b| b.to_ascii_lowercase() as char).collect())
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let home = home(p)?;
        // the name table sits in `<game>_Data/resources.assets`
        let data = std::fs::read_dir(&home).ok()?.flatten().map(|e| e.path().join("resources.assets")).find(|f| f.is_file());
        let table = data.and_then(|file| std::fs::read(file).ok());
        let table = table.as_deref().and_then(|assets| text::named(assets, CHARACTERS));
        let names = text::characters(std::str::from_utf8(table.unwrap_or_default()).unwrap_or_default());

        let mut files: Vec<PathBuf> = std::fs::read_dir(home.join("Standalone/scenario")).ok()?.flatten().map(|e| e.path()).collect();
        files.sort();
        let title = home.file_name()?.to_string_lossy().into_owned();
        let mut game = Game { title, scenes: HashMap::new(), passages: Vec::new() };
        for file in files {
            let Some(scene) = std::fs::read(&file).ok().and_then(|d| text::bundle(&d)).and_then(|a| text::scene(&a)) else { continue };
            let cuts = text::cuts(&scene, &names).into_iter().map(|shown| {
                shown.map(|shown| {
                    game.passages.push(shown);
                    game.passages.len() - 1
                })
            });
            let cuts = cuts.collect();
            game.scenes.insert(file.file_name()?.to_string_lossy().to_lowercase(), cuts);
        }
        (!game.passages.is_empty()).then_some(game)
    }

    /// What the object whose `filename` field starts `fields` stands on: the
    /// cuts of its scene and the number of the cut. The number is followed by
    /// the number of cuts in the scene, which is how it is recognised.
    fn standing(&self, p: &Proc, fields: &[u8]) -> Option<(&Vec<Option<usize>>, usize)> {
        let mut name = [0u8; STRING_CHARS_AT + MAX_NAME * 2];
        let n = p.read(field(fields, 0) as u64, &mut name);
        let cuts = self.scenes.get(&name_at(&name[..n])?)?;
        let at = (NUMBERS_FROM..FIELDS - 1).find(|k| field(fields, k + 1) as usize == cuts.len() && (field(fields, *k) as usize) < cuts.len())?;
        Some((cuts, field(fields, at) as usize))
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        format!("Ninetail ({})", self.title)
    }

    fn len(&self) -> usize {
        self.passages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let shown = &self.passages[index];
        // `;` breaks the line
        let mut lines: Vec<String> = shown.text.split(';').map(str::to_string).collect();
        lines.extend(shown.choices.iter().map(|choice| format!("{CHOICE_MARK}{choice}")));
        Passage { speaker: shown.speaker.clone(), lines, raw: shown.text.clone() }
    }

    /// Finds `AdvDataControlUser` objects: a field that points at a string
    /// naming a scene, followed by two more strings and, a few fields on, a
    /// number below the scene's count of cuts and that count.
    ///
    /// The game makes a new object when a save is loaded and the old one
    /// stays in memory, so when there are several none is trusted outright.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        // every string that names a scene
        let mut names: HashMap<u32, u32> = HashMap::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(STRING_CHARS_AT))).step_by(4) {
                let len = u32::from_le_bytes(data[at + STRING_LENGTH_AT..at + STRING_CHARS_AT].try_into().unwrap()) as usize;
                if NAME_UNITS.contains(&len) && name_at(&data[at..]).is_some_and(|name| self.scenes.contains_key(&name)) {
                    let vtable = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
                    names.insert((base + at as u64) as u32, vtable);
                }
            }
        });
        let (low, high) = (names.keys().min().copied().unwrap_or(1), names.keys().max().copied().unwrap_or(0));
        let mut cells = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(FIELDS * 4))).step_by(4) {
                let fields = &data[at..at + FIELDS * 4];
                let first = field(fields, 0);
                if first < low || first > high {
                    continue;
                }
                let Some(vtable) = names.get(&first) else { continue };
                // the two fields after it are strings as well
                let string = |k: usize| {
                    let mut head = [0u8; 4];
                    p.read(field(fields, k) as u64, &mut head) == 4 && u32::from_le_bytes(head) == *vtable
                };
                if string(1) && string(2) && self.standing(p, fields).is_some() {
                    cells.push(Cell { addr: base + at as u64, trusted: false });
                }
            }
        });
        if let [only] = cells.as_mut_slice() {
            only.trusted = true;
        }
        Position { cells }
    }

    /// The cell is the `filename` field of an `AdvDataControlUser`.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut fields = [0u8; FIELDS * 4];
        if p.read(addr, &mut fields) != fields.len() {
            return None;
        }
        let (cuts, cut) = self.standing(p, &fields)?;
        cuts[cut]
    }
}
