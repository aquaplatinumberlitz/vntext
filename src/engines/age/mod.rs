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
/// The interpreter keeps a record for the running script and, when it calls
/// another script, pushes the caller's record into the slot after it. The
/// slots are this many bytes apart, and a call does not go deeper than this.
const SLOT: u64 = 0x168;
const CALL_DEPTH: u64 = 3;
/// A `call-script` and its argument.
const CALL_WORDS: u32 = 3;
/// Shown before each label of a menu.
const CHOICE_MARK: &str = "▶ ";
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
    /// a menu of choices rather than text
    menu: bool,
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
    for (k, count) in shape.iter_mut().take(6).enumerate() {
        *count = field(fields, COUNTS + k);
        if *count > MAX_COUNT {
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
    /// The passage that has this word of code in its range.
    fn at(script: &Script, word: u32) -> Option<usize> {
        let next = script.passages.partition_point(|(from, ..)| *from <= word);
        let (_, to, index) = *script.passages.get(next.checked_sub(1)?)?;
        (word <= to).then_some(index)
    }

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
            let menus = text::menus(script, code);
            if found.is_empty() && menus.is_empty() {
                continue;
            }
            let shape = text::shape(script)?;
            let end = text::HEADER + text::code_words(&shape) as usize * 4;
            let mut passages = Vec::new();
            for shown in found {
                passages.push((shown.from, shown.to, game.passages.len()));
                let speaker = shown.speaker.and_then(|who| naming.as_ref()?.name(&strings, who)).cloned();
                game.passages.push(Shown { speaker, text: shown.text, menu: false });
            }
            // a menu is a passage of its own; the interpreter stands on the
            // call (three words long) that draws it
            for menu in menus {
                passages.push((menu.call_at, menu.call_at + CALL_WORDS, game.passages.len()));
                let rows = menu.labels.iter().map(|label| format!("{CHOICE_MARK}{label}"));
                let text: Vec<String> = menu.question.into_iter().chain(rows).collect();
                game.passages.push(Shown { speaker: None, text: text.join("\n"), menu: true });
            }
            passages.sort_by_key(|(from, ..)| *from);
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
        Passage { speaker: shown.speaker.clone(), lines: shown.text.lines().map(str::to_string).collect(), raw: shown.text.clone() }
    }

    /// Finds the interpreter's records of running scripts that have text.
    /// The shape copied from the header is twelve numbers, so nothing else
    /// looks like one.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut cells = Vec::new();
        let mut callers = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(FIELDS * 4))).step_by(4) {
                let fields = &data[at..at + FIELDS * 4];
                if running(fields).is_some_and(|(shape, ..)| self.scripts.contains_key(&shape)) {
                    let Some((script, word)) = self.script(p, fields) else { continue };
                    let addr = base + at as u64;
                    // A record standing on the call that puts up a menu is
                    // the caller's, pushed out of the way: the one to follow
                    // is the running record, a slot before. A record left
                    // there after the menu is gone is of no use.
                    match Self::at(script, word).filter(|index| self.passages[*index].menu) {
                        Some(menu) => callers.push((addr, menu)),
                        None => cells.push(Cell { addr, trusted: true }),
                    }
                }
            }
        });
        for (addr, menu) in callers {
            let live = addr.wrapping_sub(SLOT);
            if self.read(p, live) == Some(menu) && !cells.iter().any(|c| c.addr == live) {
                cells.push(Cell { addr: live, trusted: true });
            }
        }
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
        if let Some(index) = self.script(p, &fields).and_then(|(script, word)| Self::at(script, word)) {
            return Some(index);
        }
        // Not on a passage: it may have called a script that draws a menu,
        // in which case the record of the caller is in the slots after this
        // one. It counts only when it is another script than the running one;
        // a record of the same script is left over from a call that is over.
        let running_code = field(&fields, 0);
        (1..=CALL_DEPTH).find_map(|k| {
            let mut caller = [0u8; FIELDS * 4];
            (p.read(addr + k * SLOT, &mut caller) == caller.len() && field(&caller, 0) != running_code).then_some(())?;
            let (script, word) = self.script(p, &caller)?;
            Self::at(script, word).filter(|index| self.passages[*index].menu)
        })
    }
}
