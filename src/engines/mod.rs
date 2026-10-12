//! Engine profiles. A profile knows one family of games well enough to read
//! the passage on screen exactly, instead of guessing from a memory scan.
//!
//! To add a family: write a module here that implements `Engine`, and list
//! its `detect` function in `DETECTORS`. Nothing outside this directory needs
//! to change.

use crate::mem::Proc;

pub mod ark;
pub mod bishop;
pub mod med;
pub mod shin;
pub mod spt;
pub mod yuris;

/// One unit of text as the game shows it: a few sentences, usually by one speaker.
pub struct Passage {
    /// absent for narration
    pub speaker: Option<String>,
    pub lines: Vec<String>,
    /// the text as the engine stores it, control codes included
    pub raw: String,
}

/// A place where the game keeps a reading position, as found by `Engine::locate`.
#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    pub addr: u64,
    /// The engine recognises this as the game's live state, so what it holds
    /// can be shown right away. Other cells merely look like one: games keep
    /// copies (the state stored in a save, for instance) that hold some
    /// unrelated passage. Those are only believed once they are seen to step
    /// through the script the way a reader does.
    pub trusted: bool,
}

#[derive(Clone, Default)]
pub struct Position {
    pub cells: Vec<Cell>,
}

pub trait Engine: Send + Sync {
    /// Short description for the status line, e.g. the family and variant.
    fn name(&self) -> String;

    /// Number of passages in the script. They are indexed in script order,
    /// so `index + 1` is normally what the reader sees next.
    fn len(&self) -> usize;

    fn passage(&self, index: usize) -> Passage;

    /// Searches the process for the game's reading position; empty when the
    /// game is not showing text. A quick search only looks where the live
    /// state is expected; a `thorough` one scans the game's whole memory and
    /// also reports cells that merely look right.
    fn locate(&self, p: &Proc, thorough: bool) -> Position;

    /// The passage the cell at `addr` points at. Fast: called for every cell
    /// many times per second.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize>;
}

type Detector = fn(&Proc) -> Option<Box<dyn Engine>>;

/// Tried in order; the ones that only look at the game's files come first.
const DETECTORS: &[Detector] = &[spt::detect, med::detect, ark::detect, yuris::detect, shin::detect, bishop::detect];

/// Returns the profile of the first engine recognised in the process.
pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    DETECTORS.iter().find_map(|detect| detect(p))
}
