//! Profile for Eushully's AGE engine, `SYS4` scripts: 封緘のグラセスタ.
//!
//! Scripts are compiled files, most of them inside the `.ALF` archives
//! listed by `SYS4INI.BIN` and the `APPENDnn.AAI` of the append discs, some
//! loose in the game's directory. They are read from there. In memory, the
//! interpreter keeps for each running script a pointer to its code, a
//! pointer to the instruction it stands on and, copied from the script's
//! header, the counts and table sizes that tell which script it is.
//!
//! The file formats and the table of instructions come from
//! Kelebek1's Eushully-Decompiler.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Mutex;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

/// The archive index that every game of the engine has.
const INDEX: &str = "SYS4INI.BIN";
/// A running script, in 4-byte fields: `code, position` (pointers), the six
/// counts of local variables, eight fields of the interpreter's own, then
/// `length, pointer` for each of the three tables after the code.
const FIELDS: usize = 22;
const COUNTS: usize = 2;
const TABLES: usize = 16;
/// No script has more local variables of a kind than this.
const MAX_COUNT: u32 = 0x1_0000;
struct Script {
    /// of its code, to tell apart scripts of the same shape
    hash: u64,
    /// its passages in order: the words of code each spans, and its number
    passages: Vec<(u32, u32, usize)>,
}

struct Shown {
    speaker: Option<String>,
    text: String,
}

pub struct Game {
    scripts: HashMap<text::Shape, Vec<Script>>,
    passages: Vec<Shown>,
    /// which script of its shape the code at an address was found to be
    known: Mutex<HashMap<(u32, text::Shape), usize>>,
}

fn hash(code: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

fn field(fields: &[u8], k: usize) -> u32 {
    u32::from_le_bytes(fields[k * 4..k * 4 + 4].try_into().unwrap())
}

/// The shape of the script that these fields of the interpreter describe,
/// its code and how many words into it the interpreter stands.
fn running(fields: &[u8]) -> Option<(text::Shape, u32, u32)> {
    let (code, position) = (field(fields, 0), field(fields, 1));
    if code < 0x1_0000 || position < code || (position - code) % 4 != 0 {
        return None;
    }
    let mut shape = [0; 12];
    for k in 0..6 {
        shape[k] = field(fields, COUNTS + k);
        if shape[k] > MAX_COUNT {
            return None;
        }
    }
    for k in 0..3 {
        // a table is given by its length and where it is, counted from the code
        let table = field(fields, TABLES + k * 2 + 1).checked_sub(code).filter(|bytes| bytes % 4 == 0)?;
        shape[6 + k * 2] = field(fields, TABLES + k * 2);
        shape[7 + k * 2] = table / 4;
    }
    Some((shape, code, (position - code) / 4))
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let index = p.game_file(|name| name == INDEX.to_lowercase())?;
        let dir = index.parent()?.to_path_buf();
        // later scripts set names over earlier ones: the game, its append
        // discs in order, then the loose files, which are patches
        let mut scripts: Vec<Vec<u8>> = Vec::new();
        let mut appended: Vec<PathBuf> = p.game_files(|name| name.ends_with(".aai"));
        appended.sort();
        for index in std::iter::once(index.clone()).chain(appended) {
            text::archived(&index, |script| scripts.push(script));
        }
        let mut loose: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).collect();
        loose.retain(|file| file != &index && file.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin")));
        loose.sort();
        scripts.extend(loose.iter().filter_map(|file| std::fs::read(file).ok()));

        let parsed: Vec<(Vec<u8>, Vec<text::Instruction>)> =
            scripts.into_iter().filter_map(|script| text::instructions(&script).map(|code| (script, code))).collect();
        let naming = parsed.iter().find_map(|(_, code)| text::naming(code));
        let mut strings = HashMap::new();
        for (script, code) in &parsed {
            text::strings(script, code, &mut strings);
        }

        let mut game = Game { scripts: HashMap::new(), passages: Vec::new(), known: Mutex::new(HashMap::new()) };
        for (script, code) in &parsed {
            let found = text::passages(script, code, naming.as_ref());
            if found.is_empty() {
                continue;
            }
            let shape = text::shape(script)?;
            let end = text::HEADER + text::code_words(&shape) as usize * 4;
            let mut passages = Vec::new();
            for shown in found {
                passages.push((shown.from, shown.to, game.passages.len()));
                let speaker = shown.speaker.and_then(|who| naming.as_ref()?.name(&strings, who)).cloned();
                game.passages.push(Shown { speaker, text: shown.text });
            }
            game.scripts.entry(shape).or_default().push(Script { hash: hash(script.get(text::HEADER..end)?), passages });
        }
        (!game.passages.is_empty()).then_some(game)
    }

    /// The script with text that these fields of the interpreter describe,
    /// and the word of its code that the interpreter stands on. When several
    /// scripts have the shape, the code in memory tells which it is.
    fn script(&self, p: &Proc, fields: &[u8]) -> Option<(&Script, u32)> {
        let (shape, code, word) = running(fields)?;
        let scripts = self.scripts.get(&shape)?;
        if let [only] = scripts.as_slice() {
            return Some((only, word));
        }
        let mut known = self.known.lock().unwrap();
        let which = match known.get(&(code, shape)) {
            Some(which) => *which,
            None => {
                let mut loaded = vec![0u8; text::code_words(&shape) as usize * 4];
                if p.read(code as u64, &mut loaded) != loaded.len() {
                    return None;
                }
                let which = scripts.iter().position(|s| s.hash == hash(&loaded))?;
                known.insert((code, shape), which);
                which
            }
        };
        Some((&scripts[which], word))
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        "AGE (SYS4)".to_string()
    }

    fn len(&self) -> usize {
        self.passages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let shown = &self.passages[index];
        Passage { speaker: shown.speaker.clone(), lines: vec![shown.text.clone()], raw: shown.text.clone() }
    }

    /// Finds the interpreter's records of running scripts that have text.
    /// The shape copied from the header is twelve numbers, so nothing else
    /// looks like one.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut cells = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(FIELDS * 4))).step_by(4) {
                let fields = &data[at..at + FIELDS * 4];
                if running(fields).is_some_and(|(shape, ..)| self.scripts.contains_key(&shape)) && self.script(p, fields).is_some() {
                    cells.push(Cell { addr: base + at as u64, trusted: true });
                }
            }
        });
        Position { cells }
    }

    /// The cell is the pointer to the code in a running script's record.
    /// The interpreter stands just past the wait while a passage is up; it
    /// is elsewhere, in a routine, while one is being drawn.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut fields = [0u8; FIELDS * 4];
        if p.read(addr, &mut fields) != fields.len() {
            return None;
        }
        let (script, word) = self.script(p, &fields)?;
        let next = script.passages.partition_point(|(from, ..)| *from <= word);
        let (_, to, index) = *script.passages.get(next.checked_sub(1)?)?;
        (word <= to).then_some(index)
    }
}
