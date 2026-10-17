//! Profile for CatSystem2: アマカノ.
//!
//! Scenes are `CatScene` files in the archive `scene.int`, which is
//! encrypted with a key from the game's executable; they are read from
//! there. The interpreter runs a scene one input at a time, an input being
//! the lines up to where the reader clicks. In memory it keeps a record of
//! where it is: a pointer to the slot holding the loaded scene, the scene's
//! name in 32 bytes, the input and the line within it. While text waits for
//! a click, the line is the input's last.
//!
//! The hero's name is a string variable (`$str20`), which the game keeps
//! with the others in a `std::map<int, std::string>`; until the player
//! names him, the screens of `fes.int` give the default.
//!
//! The file formats come from morkt's GARbro and trigger-segfault's
//! TriggersTools.CatSystem2; the records in memory were found in the game.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

const SCENES: &str = "scene.int";
const SCREENS: &str = "fes.int";
/// The interpreter's record, from the scene's name: the name, then the
/// input and the line in 4 bytes each. The pointer to the slot is before it.
const NAME: usize = 32;
const RECORD: usize = NAME + 8;
/// No scene has more inputs, nor an input more lines, than this.
const MOST: u32 = 0x1_0000;
/// The loaded scene, which the slot points to, keeps a pointer to the
/// script's header here.
const HEADER: u64 = 12;
/// A node of the map of string variables (MSVC, 32-bit): `left, parent,
/// right`, a byte for its colour and one that is set on the map's head, the
/// number of the variable and its `std::string` (16 bytes of text or a
/// pointer to it, the length, the capacity).
const NODE: usize = 44;
const IS_HEAD: usize = 13;
const KEY: usize = 16;
const STRING: usize = 20;
const LENGTH: usize = 36;
const CAPACITY: usize = 40;
/// How deep the map's tree can be, and how often the variables are read again.
const DEPTH: usize = 64;
const REFRESH: Duration = Duration::from_millis(500);

struct Scene {
    header: [u8; 16],
    /// for each input, the number of its passage, if it shows text
    passages: Vec<Option<usize>>,
    /// how many lines each input has
    lines: Vec<u32>,
}

struct Shown {
    speaker: Option<String>,
    text: String,
}

#[derive(Default)]
struct Vars {
    /// the head of the map in memory, once found
    head: Option<u64>,
    values: HashMap<u32, String>,
    read: Option<Instant>,
}

pub struct Game {
    /// by name, lowercased and without the extension
    scenes: HashMap<String, Scene>,
    passages: Vec<Shown>,
    /// the string variables the passages use, and their values before the player changes them
    used: Vec<u32>,
    defaults: HashMap<u32, String>,
    vars: Mutex<Vars>,
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn read_u32(p: &Proc, addr: u64) -> Option<u64> {
    let mut word = [0u8; 4];
    (p.read(addr, &mut word) == 4).then(|| u32::from_le_bytes(word) as u64)
}

/// The numbers of the string variables a text uses.
fn variables(text: &str) -> impl Iterator<Item = u32> + '_ {
    text.split("$str").skip(1).filter_map(|rest| rest[..rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len())].parse().ok())
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let archive = std::fs::read(p.game_file(|name| name == SCENES)?).ok()?;
        // the executables mapped into the process; the game's has the key
        let maps = crate::mem::read_maps(p.pid).unwrap_or_default();
        let key = maps.iter().filter(|m| m.path.to_lowercase().ends_with(".exe")).find_map(|m| text::key(&std::fs::read(&m.path).ok()?));
        let files = text::archive(&archive, key)?;
        let screens = p.game_file(|name| name == SCREENS).and_then(|f| std::fs::read(f).ok());
        let defaults = screens.and_then(|data| text::archive(&data, key)).map(|files| text::defaults(&files)).unwrap_or_default();
        let mut game = Game { scenes: HashMap::new(), passages: Vec::new(), used: Vec::new(), defaults, vars: Mutex::default() };
        for (name, file) in &files {
            let Some(name) = name.to_lowercase().strip_suffix(".cst").map(str::to_string) else { continue };
            let Some(scene) = text::scene(file) else { continue };
            let mut passages = Vec::new();
            for input in &scene.inputs {
                let shows = !input.text.trim().is_empty();
                passages.push(shows.then_some(game.passages.len()));
                if shows {
                    game.used.extend(variables(&input.text).chain(input.speaker.iter().flat_map(|s| variables(s))));
                    game.passages.push(Shown { speaker: input.speaker.clone(), text: input.text.clone() });
                }
            }
            let lines = scene.inputs.iter().map(|input| input.lines as u32).collect();
            game.scenes.insert(name, Scene { header: scene.header, passages, lines });
        }
        game.used.sort();
        game.used.dedup();
        (!game.passages.is_empty()).then_some(game)
    }

    /// The scene, input and line in a record, when it looks like the interpreter's.
    fn position(&self, record: &[u8]) -> Option<(&Scene, usize)> {
        let (input, line) = (u32_at(record, NAME), u32_at(record, NAME + 4));
        if input >= MOST || line >= MOST || !record[0].is_ascii_alphanumeric() {
            return None;
        }
        let name = &record[..record[..NAME].iter().position(|b| *b == 0)?];
        let scene = self.scenes.get(&String::from_utf8_lossy(name).to_lowercase())?;
        (line <= *scene.lines.get(input as usize)?).then_some((scene, input as usize))
    }

    /// Whether the slot before the record at `addr` holds the scene it names:
    /// what tells the live record from copies of it.
    fn loaded(p: &Proc, addr: u64, scene: &Scene) -> bool {
        let header = (|| read_u32(p, read_u32(p, read_u32(p, addr - 4)?)? + HEADER))();
        let mut held = [0u8; 16];
        header.is_some_and(|h| p.read(h, &mut held) == held.len() && held == scene.header)
    }

    /// The variable of a node that could be one of a variable the passages use.
    fn looks_like_node(&self, node: &[u8]) -> Option<u32> {
        let key = u32_at(node, KEY);
        let (len, capacity) = (u32_at(node, LENGTH) as usize, u32_at(node, CAPACITY) as usize);
        let links = (0..3).all(|k| u32_at(node, k * 4) % 4 == 0 && u32_at(node, k * 4) >= 0x1_0000);
        let shape = node[IS_HEAD] == 0 && node[IS_HEAD - 1] <= 1 && len <= capacity && (15..0x1000).contains(&capacity);
        let ends = || capacity > 15 || node[STRING + len] == 0;
        (links && shape && ends() && self.used.binary_search(&key).is_ok()).then_some(key)
    }

    /// Reads the node at `addr` of the map of string variables.
    fn node(p: &Proc, addr: u64) -> Option<[u8; NODE]> {
        let mut node = [0u8; NODE];
        (p.read(addr, &mut node) == NODE).then_some(node)
    }

    /// The text of the variable in a node.
    fn string(p: &Proc, node: &[u8]) -> Option<String> {
        let (len, capacity) = (u32_at(node, LENGTH) as usize, u32_at(node, CAPACITY) as usize);
        if len > capacity || capacity >= MOST as usize {
            return None;
        }
        let text = if capacity < 16 {
            node.get(STRING..STRING + len)?.to_vec()
        } else {
            let mut text = vec![0u8; len];
            (p.read(u32_at(node, STRING) as u64, &mut text) == len).then_some(text)?
        };
        Some(encoding_rs::SHIFT_JIS.decode(&text).0.into_owned())
    }

    /// The node of variable `key` in the map with this head.
    fn lookup(p: &Proc, head: u64, key: u32) -> Option<(u64, [u8; NODE])> {
        let mut at = read_u32(p, head + 4)?;
        for _ in 0..DEPTH {
            let node = Self::node(p, at)?;
            if at == head || node[IS_HEAD] != 0 {
                return None;
            }
            let here = u32_at(&node, KEY);
            if here == key {
                return Some((at, node));
            }
            at = u32_at(&node, if key < here { 0 } else { 8 }) as u64;
        }
        None
    }

    /// The head of the map that the node at `addr` belongs to, if it is a
    /// node of variable `key`: climbing to the head and looking the variable
    /// up from there must lead back to it.
    fn head_of(p: &Proc, addr: u64, key: u32) -> Option<u64> {
        let mut at = addr;
        for _ in 0..DEPTH {
            let node = Self::node(p, at)?;
            if node[IS_HEAD] != 0 {
                return (Self::lookup(p, at, key)?.0 == addr).then_some(at);
            }
            at = u32_at(&node, 4) as u64;
        }
        None
    }

    /// Reads the variables the passages use, at most every `REFRESH`.
    fn refresh(&self, p: &Proc) {
        let mut vars = self.vars.lock().unwrap();
        let Some(head) = vars.head else { return };
        if vars.read.is_some_and(|read| read.elapsed() < REFRESH) {
            return;
        }
        vars.read = Some(Instant::now());
        for key in &self.used {
            match Self::lookup(p, head, *key).and_then(|(_, node)| Self::string(p, &node)) {
                Some(value) => vars.values.insert(*key, value),
                None => vars.values.remove(key),
            };
        }
    }

    fn expand(&self, text: &str) -> Vec<String> {
        let vars = self.vars.lock().unwrap();
        text::plain(text, |key| {
            let set = vars.values.get(&key).filter(|value| !value.is_empty());
            set.or_else(|| self.defaults.get(&key)).cloned().unwrap_or_default()
        })
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        "CatSystem2".to_string()
    }

    fn len(&self) -> usize {
        self.passages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let shown = &self.passages[index];
        let speaker = shown.speaker.as_deref().map(|name| self.expand(name).concat()).filter(|name| !name.is_empty());
        Passage { speaker, lines: self.expand(&shown.text), raw: shown.text.clone() }
    }

    /// Finds the interpreter's record, and the map of string variables. The
    /// game keeps copies of the record, which name a scene that is no longer
    /// loaded or stand still; the live one is trusted, being the one whose
    /// slot holds the scene it names.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut cells = Vec::new();
        let mut nodes = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(NODE.max(RECORD)))).step_by(4) {
                if let Some((scene, _)) = self.position(&data[at..at + RECORD]) {
                    cells.push((base + at as u64, scene));
                }
                if let Some(key) = self.looks_like_node(&data[at..at + NODE]) {
                    nodes.push((base + at as u64, key));
                }
            }
        });
        let mut vars = self.vars.lock().unwrap();
        if vars.head.is_none_or(|head| Self::node(p, head).is_none_or(|node| node[IS_HEAD] == 0)) {
            vars.head = nodes.iter().find_map(|(addr, key)| Self::head_of(p, *addr, *key));
            vars.read = None;
        }
        drop(vars);
        self.refresh(p);
        Position { cells: cells.into_iter().map(|(addr, scene)| Cell { addr, trusted: Self::loaded(p, addr, scene) }).collect() }
    }

    /// The cell is the scene's name in the interpreter's record.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut record = [0u8; RECORD];
        if p.read(addr, &mut record) != RECORD {
            return None;
        }
        self.refresh(p);
        let (scene, input) = self.position(&record)?;
        scene.passages[input]
    }
}
