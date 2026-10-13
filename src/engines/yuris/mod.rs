//! Profile for the YU-RIS engine (聖奴隷学園 1 to 3 and others).
//!
//! Compiled scripts (`ystNNNNN.ybn`) are packed in the game's YPF archives
//! and are read from there. In memory the engine keeps, for every script
//! task it runs, a context with the number of the command it will execute
//! next, pointers into the loaded script and the script's number. While a
//! line of text is on screen, the scenario's task has just passed the
//! `RETURNCODE` command that ends the line.
//!
//! The file formats come from https://github.com/shimamura-sakura/yuri.

use std::collections::HashMap;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

use memchr::memmem;

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

/// In a task's context, in 32-bit cells from the pointer to the script's
/// data section: the pointer to the line numbers is one cell after it and
/// the script's number two. The next command's number comes before a run of
/// pointers that is three long in older versions of the engine (479) and
/// four long in newer ones (494), so it is four or five cells before.
const COUNTER_BEFORE: [u64; 2] = [4, 5];
const SCRIPT_AFTER: u64 = 2;
/// A context keeps its calls further on: a number (how deep it is, or how
/// much room there is) and then a pointer to a frame for each level. A frame
/// is `?, return: command number, script`. Where that is differs between
/// versions (0x110 after the context's cell in 488, 0x270 in 479), so it is
/// looked for in this stretch.
const CALLS_WITHIN: std::ops::Range<usize> = 0x40..0x400;
const MAX_COUNT: u32 = 0x1000;
/// Shown before each choice of a menu.
const CHOICE_MARK: &str = "▶ ";

struct Script {
    commands: u32,
    /// line shown after the command with this number has run
    lines: HashMap<u32, usize>,
    /// menu that is up while a call from this script is to return to the
    /// command with this number
    menus: HashMap<u32, usize>,
}

pub struct Game {
    scripts: HashMap<u32, Script>,
    lines: Vec<text::Line>,
}

/// The game's archives, oldest first: an update's files replace the ones
/// it has in common with what came before.
fn archives(p: &Proc) -> Vec<PathBuf> {
    for dir in p.game_dirs() {
        let mut found: Vec<PathBuf> = [dir.join("pac"), dir]
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flat_map(|entries| entries.flatten().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ypf")))
            .collect();
        if !found.is_empty() {
            let name = |path: &PathBuf| path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            found.sort_by_key(|path| (name(path).starts_with("update"), name(path)));
            return found;
        }
    }
    Vec::new()
}

/// The compiled scripts and the command list of an archive, by file name.
fn scripts_of(path: &PathBuf, into: &mut HashMap<String, Vec<u8>>) -> Option<()> {
    let file = File::open(path).ok()?;
    let mut head = [0u8; text::YPF_HEAD];
    file.read_exact_at(&mut head, 0).ok()?;
    let mut table = vec![0u8; text::ypf_table_len(&head)?];
    file.read_exact_at(&mut table, text::YPF_HEAD as u64).ok()?;
    for entry in text::ypf_entries(&head, &table)?.into_iter().filter(|e| e.name.ends_with(".ybn")) {
        let mut stored = vec![0u8; entry.size];
        file.read_exact_at(&mut stored, entry.at).ok()?;
        let data = if entry.compressed { text::inflate(&stored)? } else { stored };
        into.insert(entry.name.rsplit('\\').next().unwrap_or(&entry.name).to_string(), data);
    }
    Some(())
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let mut files = HashMap::new();
        for path in archives(p) {
            let _ = scripts_of(&path, &mut files);
        }
        let commands = text::commands(files.get("ysc.ybn")?)?;
        // scripts are named yst00000.ybn and up, by their number
        let numbered: Vec<(u32, &Vec<u8>)> = files
            .iter()
            .filter_map(|(name, data)| Some((name.strip_prefix("yst")?.strip_suffix(".ybn")?.parse().ok()?, data)))
            .collect();
        let parsed: Vec<(u32, text::Sections)> =
            numbered.iter().filter_map(|(number, data)| Some((*number, text::sections(data)?.0))).collect();
        let all: Vec<text::Sections> =
            parsed.iter().map(|(_, s)| text::Sections { count: s.count, commands: s.commands, arguments: s.arguments, data: s.data, lines: s.lines }).collect();
        let key = text::find_key(&all, commands.count)?;

        let mut game = Game { scripts: HashMap::new(), lines: Vec::new() };
        let mut parsed = parsed;
        parsed.sort_by_key(|(number, _)| *number);
        for (number, sections) in parsed {
            let mut script = Script { commands: sections.count as u32, lines: HashMap::new(), menus: HashMap::new() };
            // lines and menus, in the order the script has them; a menu is
            // kept as a line of its own, one choice to a row
            let mut menus = text::menus(&sections, key, &commands).into_iter().peekable();
            for line in text::lines(&sections, key, &commands) {
                while let Some(menu) = menus.next_if(|menu| menu.returns_to <= line.ends_at) {
                    script.menus.insert(menu.returns_to, game.lines.len());
                    game.lines.push(menu_line(menu));
                }
                script.lines.insert(line.ends_at, game.lines.len());
                game.lines.push(line);
            }
            for menu in menus {
                script.menus.insert(menu.returns_to, game.lines.len());
                game.lines.push(menu_line(menu));
            }
            // scripts with neither text nor menus are the engine's own; their
            // tasks are of no interest
            if !script.lines.is_empty() || !script.menus.is_empty() {
                game.scripts.insert(number, script);
            }
        }
        (!game.lines.is_empty()).then_some(game)
    }

    /// What a task context says: the next command's number and the script's.
    fn context(&self, p: &Proc, addr: u64) -> Option<(u32, &Script)> {
        let cell = |at: u64| {
            let mut value = [0u8; 4];
            (p.read(at, &mut value) == 4).then(|| u32::from_le_bytes(value))
        };
        let script = self.scripts.get(&cell(addr + 4 * SCRIPT_AFTER)?)?;
        // where the counter is not, there is a pointer: far too large a number
        let next = COUNTER_BEFORE.iter().filter_map(|before| cell(addr - 4 * before)).find(|next| *next <= script.commands)?;
        Some((next, script))
    }
}

/// A menu as a passage: no speaker, a row for each choice.
fn menu_line(menu: text::Menu) -> text::Line {
    let rows: Vec<String> = menu.choices.iter().map(|choice| format!("{CHOICE_MARK}{choice}")).collect();
    text::Line { ends_at: menu.returns_to, speaker: None, text: rows.join("\n") }
}

impl Game {
    /// The menu that the task of this context has put up: its outermost call,
    /// the one from the story script, is to return to just after a menu.
    /// When the task runs in the library, that frame is always the call that
    /// brought it there, so the deeper ones need not be looked at.
    fn menu(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut block = vec![0u8; CALLS_WITHIN.end];
        let n = p.read(addr, &mut block);
        let cells: Vec<u32> = block[..n & !3].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let frame = |at: u32| -> Option<usize> {
            let mut frame = [0u8; 12];
            (p.read(at as u64, &mut frame) == 12).then_some(())?;
            let field = |k: usize| u32::from_le_bytes(frame[k * 4..k * 4 + 4].try_into().unwrap());
            self.scripts.get(&field(2))?.menus.get(&field(1)).copied()
        };
        // the first frame pointer is the one after a small number (not a pointer)
        (CALLS_WITHIN.start / 4..cells.len()).find_map(|k| {
            let before = cells[k];
            if before == 0 || before > MAX_COUNT {
                return None;
            }
            frame(*cells.get(k + 1)?)
        })
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        "YU-RIS".to_string()
    }

    fn len(&self) -> usize {
        self.lines.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let line = &self.lines[index];
        Passage { speaker: line.speaker.clone(), lines: line.text.lines().map(str::to_string).collect(), raw: line.text.clone() }
    }

    /// Finds the contexts of tasks that run a script with text. The engine
    /// keeps loaded scripts decrypted in memory, so first every loaded
    /// script is found by its header, then every pair of cells that point at
    /// one script's data and line number sections: that pair is in a context.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        // address of the data section -> address of the line number section
        let mut loaded: HashMap<u32, u32> = HashMap::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for hit in memmem::find_iter(data, b"YSTB").take_while(|hit| *hit < limit) {
                let Some(head) = data.get(hit..hit + text::YSTB_HEAD) else { continue };
                let field = |at: usize| u32::from_le_bytes(head[at..at + 4].try_into().unwrap()) as u64;
                let (count, commands, arguments, values, lines) = (field(8), field(12), field(16), field(20), field(24));
                if commands == count * 4 && lines == count * 4 && arguments % 12 == 0 {
                    let values_at = base + hit as u64 + text::YSTB_HEAD as u64 + commands + arguments;
                    loaded.insert(values_at as u32, (values_at + values) as u32);
                }
            }
        });
        let mut cells = Vec::new();
        if !loaded.is_empty() {
            let _ = p.scan(false, |base, data, limit, _| {
                let cell = |k: usize| u32::from_le_bytes(data[k * 4..k * 4 + 4].try_into().unwrap());
                for k in 0..(limit.min(data.len().saturating_sub(8))) / 4 {
                    if loaded.get(&cell(k)) == Some(&cell(k + 1)) {
                        cells.push(base + k as u64 * 4);
                    }
                }
            });
        }
        cells.retain(|addr| self.context(p, *addr).is_some() || self.menu(p, *addr).is_some());
        Position { cells: cells.into_iter().map(|addr| Cell { addr, trusted: true }).collect() }
    }

    /// The cell is a context's pointer to the script's data. The script's
    /// number is read every time: the same context goes from script to script.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        // While a menu is up the task runs the library's menu routine, a
        // script without text; the menu is told by where the call returns to.
        match self.context(p, addr) {
            Some((next, script)) => script.lines.get(&next.checked_sub(1)?).copied(),
            None => self.menu(p, addr),
        }
    }
}
