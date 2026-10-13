//! Profile for Escu:de's engine: 廃村少女 弐.
//!
//! Scripts are compiled files in the archive `script.bin`, each with a file
//! of its messages; the tables of scripts and of speakers are in a database
//! in `data.bin`. All are read from there. In memory, the interpreter has
//! one record of what it is running: the number of the script, where the
//! instruction it ran last starts and ends, and that instruction with its
//! parameters.
//!
//! The file formats come from Chenx221's EscudeTools. The interpreter's
//! record is `SCR_ENV` in the engine's own source, which the game ships in
//! its archive `haison2.bin`.

use super::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

mod text;

const SCRIPTS: &str = "script.bin";
const DATA: &str = "data.bin";
const DATABASE: &str = "db_scripts.bin";
/// The sheets of the database with the scripts (their file names in the
/// second column) and the speakers (their names in the first), both by number.
const SCRIPT_SHEET: &str = "スクリプト";
const SPEAKER_SHEET: &str = "登場人物";
/// `SCR_ENV`: seven 4-byte fields `id, ppc, pc, sp, bp, proc, line`, then the
/// instruction in one byte, its parameters and their count. The structure
/// is packed, so what follows the instruction is not aligned.
const INSTRUCTION: usize = 28;
const PARAMETERS: usize = 29;
const COUNT: usize = 61;
const RECORD: usize = 65;
/// The size of the interpreter's stack, and how many built-in routines it can have.
const STACK: u32 = 128;
const ROUTINES: u32 = 256;
/// Shown before each choice of a menu.
const CHOICE_MARK: &str = "▶ ";

pub struct Game {
    /// by number, as the database lists them; `None` for those without a file
    scripts: Vec<Option<text::Script>>,
    /// for each script, the number of its first page among all pages
    first: Vec<usize>,
    speakers: Vec<String>,
    pages: usize,
}

fn field(fields: &[u8], k: usize) -> u32 {
    u32::from_le_bytes(fields[k * 4..k * 4 + 4].try_into().unwrap())
}

impl Game {
    fn find(p: &Proc) -> Option<Game> {
        let files = text::archive(&std::fs::read(p.game_file(|name| name == SCRIPTS)?).ok()?)?;
        let data = text::archive(&std::fs::read(p.game_file(|name| name == DATA)?).ok()?)?;
        let sheets = text::database(data.get(DATABASE)?)?;
        let column = |sheet: &str, k: usize| -> Option<Vec<String>> {
            Some(sheets.get(sheet)?.iter().map(|row| row.get(k).cloned().flatten().unwrap_or_default()).collect())
        };
        let mut game = Game { scripts: Vec::new(), first: Vec::new(), speakers: column(SPEAKER_SHEET, 0)?, pages: 0 };
        for name in column(SCRIPT_SHEET, 1)? {
            let name = name.to_lowercase();
            let code = files.get(&format!("{name}.bin")).filter(|_| !name.is_empty());
            let script = code.and_then(|code| text::script(code, files.get(&format!("{name}.001")).map(Vec::as_slice)));
            game.first.push(game.pages);
            game.pages += script.as_ref().map_or(0, |s| s.pages.len());
            game.scripts.push(script);
        }
        (game.pages > 0).then_some(game)
    }

    /// The script and the end of the last instruction in these fields, when
    /// they are the interpreter's record: the instruction and the parameters
    /// it holds are then those found in the script at the place it gives.
    fn running(&self, fields: &[u8]) -> Option<(usize, u32)> {
        let (id, last, next) = (field(fields, 0) as usize, field(fields, 1) as usize, field(fields, 2) as usize);
        let script = self.scripts.get(id)?.as_ref()?;
        let instruction = *script.code.get(last)?;
        let parameters = text::parameters(instruction);
        let sane = field(fields, 3) <= STACK && field(fields, 4) <= STACK && field(fields, 5) < ROUTINES;
        let count = u32::from_le_bytes(fields[COUNT..COUNT + 4].try_into().unwrap()) as usize;
        if !sane || fields[INSTRUCTION] != instruction || next != last + 1 + parameters * 4 || count != parameters {
            return None;
        }
        let held = &fields[PARAMETERS..PARAMETERS + parameters * 4];
        (script.code.get(last + 1..next)? == held).then_some((id, next as u32))
    }

    /// The page that is up according to these fields: the one that the
    /// last instruction belongs to.
    fn shown(&self, fields: &[u8]) -> Option<usize> {
        let (id, next) = self.running(fields)?;
        let pages = &self.scripts[id].as_ref()?.pages;
        let after = pages.partition_point(|page| page.from < next);
        let page = pages.get(after.checked_sub(1)?)?;
        (next <= page.to).then_some(self.first[id] + after - 1)
    }

    fn page(&self, index: usize) -> &text::Page {
        let script = self.first.partition_point(|first| *first <= index) - 1;
        &self.scripts[script].as_ref().unwrap().pages[index - self.first[script]]
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Game::find(p).map(|game| Box::new(game) as Box<dyn Engine>)
}

impl Engine for Game {
    fn name(&self) -> String {
        "Escu:de".to_string()
    }

    fn len(&self) -> usize {
        self.pages
    }

    fn passage(&self, index: usize) -> Passage {
        let page = self.page(index);
        let speaker = self.speakers.get(page.speaker as usize).filter(|name| !name.is_empty()).cloned();
        let mut lines = text::plain(&page.text);
        lines.extend(page.choices.iter().flat_map(|choice| text::plain(choice)).map(|choice| format!("{CHOICE_MARK}{choice}")));
        Passage { speaker, lines, raw: page.text.clone() }
    }

    /// Finds the interpreter's record, while it stands on a page. The game
    /// keeps a copy of it with the state it would save, which is the same
    /// as the live one right after a load and falls behind from then on. So
    /// what is found is trusted when it all stands on the same page, and
    /// otherwise left to prove itself by moving.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut cells = Vec::new();
        let mut pages = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            // the record sits inside a packed structure, at any address
            for at in 0..limit.min(data.len().saturating_sub(RECORD)) {
                // most of memory is not a small number followed by two rising ones
                let id = field(&data[at..], 0) as usize;
                if id == 0 || id >= self.scripts.len() {
                    continue;
                }
                if let Some(page) = self.shown(&data[at..at + RECORD]) {
                    cells.push(Cell { addr: base + at as u64, trusted: false });
                    pages.push(page);
                }
            }
        });
        if pages.windows(2).all(|pair| pair[0] == pair[1]) {
            cells.iter_mut().for_each(|cell| cell.trusted = true);
        }
        Position { cells }
    }

    /// The cell is the start of the interpreter's record. A page is up when
    /// the last instruction is one of its own.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut fields = [0u8; RECORD];
        if p.read(addr, &mut fields) != fields.len() {
            return None;
        }
        self.shown(&fields)
    }
}
