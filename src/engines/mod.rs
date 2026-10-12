//! Engine profiles. A profile knows one family of games well enough to read
//! the passage on screen exactly, instead of guessing from a memory scan.
//!
//! To add a family: write a module here that implements `Engine`, and list
//! its `detect` function in `DETECTORS`. Nothing outside this directory needs
//! to change.

use crate::mem::Proc;

pub mod shin;

/// One unit of text as the game shows it: a few sentences, usually by one speaker.
pub struct Passage {
    /// absent for narration
    pub speaker: Option<String>,
    pub lines: Vec<String>,
    /// the text as the engine stores it, control codes included
    pub raw: String,
}

/// Places where the game keeps a reading position, as found by
/// `Engine::locate`, most trustworthy first. There can be several: besides
/// the live one, games keep copies (the state of a loaded save, for
/// instance) that look the same but never move.
#[derive(Clone, PartialEq, Default)]
pub struct Position {
    pub cells: Vec<u64>,
}

pub trait Engine: Send + Sync {
    /// Short description for the status line, e.g. the family and variant.
    fn name(&self) -> String;

    /// Number of passages in the script. They are indexed in script order,
    /// so `index + 1` is normally what the reader sees next.
    fn len(&self) -> usize;

    fn passage(&self, index: usize) -> Passage;

    /// Searches the process for the game's reading position. Slow (a full
    /// memory scan); empty when the game is not showing text yet.
    fn locate(&self, p: &Proc) -> Position;

    /// The passage one located cell points at. Fast: called for every cell
    /// many times per second.
    fn read(&self, p: &Proc, cell: u64) -> Option<usize>;
}

type Detector = fn(&Proc) -> Option<Box<dyn Engine>>;

const DETECTORS: &[Detector] = &[shin::detect];

/// Returns the profile of the first engine recognised in the process.
pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    DETECTORS.iter().find_map(|detect| detect(p))
}
