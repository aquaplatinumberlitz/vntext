//! Profile for Liar-soft's engine (`.gsc` scripts in `scr.xfl`): 信天翁航海録.
//!
//! Scripts are compiled files in the archive `scr.xfl` and are read from
//! there. In memory, the interpreter keeps one record for the script it is
//! running: a pointer to the script's code, the three lengths from its
//! header and a pointer to the instruction it runs next. While a message is
//! on screen that is the instruction right after the message's.
//!
//! The file formats and the table of instructions come from
//! Love-in-idleness's LiarsoftTool.

use std::collections::HashMap;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

const ARCHIVE: &str = "scr.xfl";
/// The interpreter's record, in 4-byte fields: the pointer to the code
/// first, the script's three lengths from field 10 and the pointer to the
/// next instruction in field 25.
const FIELDS: usize = 26;
const SHAPE: usize = 10;
const POSITION: usize = 25;

pub struct Game {
    /// for each script, by its shape: where each message's instruction ends in its code
    scripts: HashMap<text::Shape, HashMap<u32, usize>>,
    messages: Vec<String>,
}

fn field(fields: &[u8], k: usize) -> u32 {
    u32::from_le_bytes(fields[k * 4..k * 4 + 4].try_into().unwrap())
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let archive = std::fs::read(p.game_file(|name| name == ARCHIVE)?).ok()?;
        let mut game = Game { scripts: HashMap::new(), messages: Vec::new() };
        for script in text::scripts(&archive) {
            let Some((shape, messages)) = text::messages(script) else { continue };
            let starts = messages.iter().enumerate().map(|(k, m)| (m.end, game.messages.len() + k)).collect();
            game.messages.extend(messages.into_iter().map(|m| m.text));
            game.scripts.insert(shape, starts);
        }
        (!game.messages.is_empty()).then_some(game)
    }

    /// The message that these fields of the interpreter stand just past.
    fn standing(&self, fields: &[u8]) -> Option<Option<usize>> {
        let (code, position) = (field(fields, 0), field(fields, POSITION));
        let shape = [field(fields, SHAPE), field(fields, SHAPE + 1), field(fields, SHAPE + 2)];
        let at = position.checked_sub(code).filter(|at| code > 0x1_0000 && *at <= shape[0])?;
        Some(self.scripts.get(&shape)?.get(&at).copied())
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        "Liar-soft (gsc)".to_string()
    }

    fn len(&self) -> usize {
        self.messages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let raw = self.messages[index].clone();
        let (speaker, lines) = text::plain(&raw);
        Passage { speaker, lines, raw }
    }

    /// Finds the interpreter's record: three fields that are the lengths of
    /// one of the game's scripts, a pointer before them and, further on, a
    /// pointer into the code that the first one points at.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut cells = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(FIELDS * 4))).step_by(4) {
                if self.standing(&data[at..at + FIELDS * 4]).is_some() {
                    cells.push(Cell { addr: base + at as u64, trusted: true });
                }
            }
        });
        Position { cells }
    }

    /// The cell is the pointer to the code in the interpreter's record.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut fields = [0u8; FIELDS * 4];
        if p.read(addr, &mut fields) != fields.len() {
            return None;
        }
        self.standing(&fields)?
    }
}
