//! Profile for Entergram's "shin" engine (Umineko Saku, Higurashi Hou, ...).
//!
//! The engine keeps the whole scenario file (SNR) in memory and its script VM
//! keeps the offset of the instruction it is executing. With both located,
//! the current message is read straight from the scenario: exact, complete,
//! and without waiting for a memory rescan.
//!
//! Format knowledge comes from https://github.com/DCNick3/shin (shin-core).

use std::collections::HashMap;

use encoding_rs::SHIFT_JIS;
use memchr::memmem;

use super::{Cell, Engine, Passage, Position};
use crate::mem::{Mapping, Proc};
use text::clean;

mod text;

const MAGIC: &[u8] = b"SNR ";
const MSGSET: u8 = 0x86;
/// Offset of the position cell inside the page that holds a live VM.
const LIVE_CELL: u64 = 0x10;
/// Fewer messages than this and the "scenario" is something else.
const MIN_MESSAGES: usize = 50;

/// A MSGSET instruction is `86 | id: u24 | wait: u8 | ... | len: u16 | text`,
/// with `len` counting the terminating NUL. How many bytes come before the
/// text differs between versions of the engine; add a row for a new game.
struct Layout {
    game: &'static str,
    head_len: usize,
}

const LAYOUTS: &[Layout] = &[
    Layout { game: "Umineko Saku", head_len: 7 },
    Layout { game: "Higurashi Hou", head_len: 8 },
];

struct Message {
    /// offsets of the MSGSET instruction inside the scenario
    start: u32,
    end: u32,
    /// text as stored: `@` commands and half-width kana included
    text: String,
}

impl Message {
    /// Speaker name (absent for narration) and the readable text of each
    /// section; the game reveals one section per click (`@k`).
    fn parts(&self) -> (Option<String>, Vec<String>) {
        let (name, body) = self.text.split_once("@r").unwrap_or(("", &self.text));
        let name = clean(name);
        let sections = body.split("@k").map(clean).filter(|s| !s.is_empty()).collect();
        ((!name.is_empty()).then_some(name), sections)
    }
}

pub struct Script {
    messages: Vec<Message>,
    by_start: HashMap<u32, usize>,
    size: usize,
    layout: &'static Layout,
    /// device and inode of the memory object the scenario was found in
    home: (String, u64),
}

/// Message text is Shift-JIS in which single bytes A0..DF stand for the
/// engine's own shorthand; they are mapped to the code points that
/// `text::clean` expects (U+F8F0 and half-width katakana).
fn decode(bytes: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(bytes.len() * 2);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0..=0x7F => out.push(b as char),
            0xA0 => out.push('\u{f8f0}'),
            0xA1..=0xDF => out.push(char::from_u32(0xFF61 + (b - 0xA1) as u32)?),
            _ => {
                let pair = bytes.get(i..i + 2)?;
                out.push_str(&SHIFT_JIS.decode_without_bom_handling_and_without_replacement(pair)?);
                i += 1;
            }
        }
        i += 1;
    }
    Some(out)
}

impl Script {
    /// Picks the messages out of a scenario image, trying every known layout
    /// and keeping the one that reads the most messages.
    fn parse(data: &[u8]) -> Script {
        LAYOUTS.iter().map(|layout| Script::parse_with(data, layout)).max_by_key(|s| s.messages.len()).unwrap()
    }

    /// Message ids are consecutive, which is what separates real instructions
    /// from bytes that merely look like one.
    fn parse_with(data: &[u8], layout: &'static Layout) -> Script {
        let head_len = layout.head_len;
        let mut found: Vec<(u32, Message)> = Vec::new();
        let mut i = 0;
        while let Some(k) = memchr::memchr(MSGSET, &data[i..]) {
            let at = i + k;
            i = at + 1;
            let Some(head) = data.get(at..at + head_len) else { break };
            let id = u32::from_le_bytes([head[1], head[2], head[3], 0]);
            let len = u16::from_le_bytes([head[head_len - 2], head[head_len - 1]]) as usize;
            let text_at = at + head_len;
            let Some(body) = data.get(text_at..text_at + len) else { continue };
            if head[4] > 1 || len < 2 || body[len - 1] != 0 || body[..len - 1].contains(&0) {
                continue;
            }
            let Some(text) = decode(&body[..len - 1]) else { continue };
            found.push((id, Message { start: at as u32, end: (text_at + len) as u32, text }));
        }
        let consecutive: Vec<bool> = (0..found.len())
            .map(|k| (k > 0 && found[k - 1].0 + 1 == found[k].0) || (k + 1 < found.len() && found[k].0 + 1 == found[k + 1].0))
            .collect();
        let messages: Vec<Message> = found.into_iter().zip(consecutive).filter(|(_, ok)| *ok).map(|((_, m), _)| m).collect();
        let by_start = messages.iter().enumerate().map(|(k, m)| (m.start, k)).collect();
        Script { messages, by_start, size: data.len(), layout, home: Default::default() }
    }

    /// Looks for a loaded scenario in the target process.
    fn find(p: &Proc) -> Option<Script> {
        let finder = memmem::Finder::new(MAGIC);
        let mut candidates = Vec::new();
        let mut collect = |base: u64, data: &[u8], limit: usize, m: &Mapping| {
            for off in finder.find_iter(data).take_while(|off| *off < limit) {
                if let Some(size) = data.get(off + 4..off + 8) {
                    let size = u32::from_le_bytes(size.try_into().unwrap()) as usize;
                    candidates.push((base + off as u64, size, (m.dev.clone(), m.inode)));
                }
            }
        };
        p.scan(false, &mut collect).ok()?;
        let resident = Script::from_candidates(p, candidates);
        if resident.as_ref().is_some_and(|script| script.home.1 != 0) {
            return resident;
        }
        let mut candidates = Vec::new();
        let _ = p.scan_shared(|_| true, |base, data, limit, m| {
            for off in finder.find_iter(data).take_while(|off| *off < limit) {
                if let Some(size) = data.get(off + 4..off + 8) {
                    let size = u32::from_le_bytes(size.try_into().unwrap()) as usize;
                    candidates.push((base + off as u64, size, (m.dev.clone(), m.inode)));
                }
            }
        });
        let Some(script) = Script::from_candidates(p, candidates) else { return resident };
        eprintln!("scenario recovered from allocated shared memory: {}", script.layout.game);
        Some(script)
    }

    fn from_candidates(p: &Proc, mut candidates: Vec<(u64, usize, (String, u64))>) -> Option<Script> {
        // A heap copy decodes correctly but cannot locate the VM in guest shared memory.
        candidates.sort_by_key(|(_, _, (_, inode))| *inode == 0);
        for (addr, size, home) in candidates {
            if !(0x1000..=512 << 20).contains(&size) {
                continue;
            }
            let mut data = vec![0u8; size];
            if p.read(addr, &mut data) != size {
                continue;
            }
            let script = Script::parse(&data);
            if script.messages.len() >= MIN_MESSAGES {
                return Some(Script { home, ..script });
            }
        }
        None
    }

    /// Finds the VM's position cells: a 32-bit cell holding the start offset
    /// of a MSGSET instruction with, 8 bytes before it, the offset just past
    /// that same instruction (the VM's program counter while it waits on the
    /// message). The pairing makes accidental matches practically impossible.
    ///
    /// Only the memory object that holds the scenario is searched, i.e. the
    /// game's own memory: an emulator keeps data of its own that matches too
    /// (buffered save files hold a VM state).
    fn locate_cursors(&self, p: &Proc) -> Vec<u64> {
        let mut is_start = vec![0u64; self.size / 64 + 1];
        for m in &self.messages {
            is_start[m.start as usize / 64] |= 1 << (m.start % 64);
        }
        let size = self.size as u32;
        let mut cursors = Vec::new();
        let _ = p.scan_where(false, |m| self.at_home(m), |base, data, limit, _| {
            for (k, cell) in data[..limit & !3].chunks_exact(4).enumerate().skip(2) {
                let v = u32::from_le_bytes(cell.try_into().unwrap());
                if v == 0 || v >= size || is_start[v as usize / 64] >> (v % 64) & 1 == 0 {
                    continue;
                }
                let pc = u32::from_le_bytes(data[k * 4 - 8..k * 4 - 4].try_into().unwrap());
                if pc == self.messages[self.by_start[&v]].end {
                    cursors.push(base + k as u64 * 4);
                }
            }
        });
        cursors
    }

    fn at_home(&self, m: &Mapping) -> bool {
        (m.dev.as_str(), m.inode) == (self.home.0.as_str(), self.home.1)
    }

    /// Finds live VMs the quick way. Every live VM seen so far sat at the
    /// start of a page, with its position cell at offset 0x10 (and so the
    /// program counter at 0x08); copies of a VM state sat elsewhere.
    fn locate_live(&self, p: &Proc) -> Vec<u64> {
        let mut cells = Vec::new();
        let _ = p.page_heads(|m| self.at_home(m), LIVE_CELL as usize + 4, |page, head| {
            if self.live_head(head) {
                cells.push(page + LIVE_CELL);
            }
        });
        if cells.is_empty() {
            // The scenario may be resident while the VM's aliases have lost their PTEs.
            // Search allocated backing extents with exactly the same live-state check.
            let _ = p.scan_shared(|m| self.at_home(m), |base, data, limit, _| {
                let first = ((4096 - base % 4096) % 4096) as usize;
                for offset in (first..limit).step_by(4096) {
                    if data.get(offset..offset + LIVE_CELL as usize + 4).is_some_and(|head| self.live_head(head)) {
                        cells.push(base + offset as u64 + LIVE_CELL);
                    }
                }
            });
        }
        cells
    }

    fn live_head(&self, head: &[u8]) -> bool {
        let word = |at: usize| u32::from_le_bytes(head[at..at + 4].try_into().unwrap());
        let waiting_on = self.by_start.get(&word(LIVE_CELL as usize)).map(|i| self.messages[*i].end);
        waiting_on == Some(word(LIVE_CELL as usize - 8))
    }

    /// The message a position cell currently points at, if any.
    fn message_at(&self, p: &Proc, cursor: u64) -> Option<usize> {
        let mut cell = [0u8; 4];
        if p.read(cursor, &mut cell) != 4 {
            return None;
        }
        self.by_start.get(&u32::from_le_bytes(cell)).copied()
    }
}

pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    Script::find(p).map(|script| Box::new(script) as Box<dyn Engine>)
}

impl Engine for Script {
    fn name(&self) -> String {
        format!("shin ({} layout)", self.layout.game)
    }

    fn len(&self) -> usize {
        self.messages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let message = &self.messages[index];
        let (speaker, lines) = message.parts();
        Passage { speaker, lines, raw: message.text.clone() }
    }

    fn locate(&self, p: &Proc, thorough: bool) -> Position {
        let mut cells: Vec<Cell> = self.locate_live(p).into_iter().map(|addr| Cell { addr, trusted: true }).collect();
        if thorough {
            let other = self.locate_cursors(p).into_iter().filter(|a| a & 0xFFF != LIVE_CELL);
            cells.extend(other.map(|addr| Cell { addr, trusted: false }));
        }
        Position { cells }
    }

    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        self.message_at(p, addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{FileExt, MetadataExt};

    #[test]
    fn guest_scenario_precedes_a_valid_anonymous_copy() {
        let mut guest = b"SNR \0\0\0\0".to_vec();
        for id in 0..MIN_MESSAGES as u32 {
            guest.extend_from_slice(&[MSGSET, id as u8, (id >> 8) as u8, (id >> 16) as u8, 0, 7, 0]);
            guest.extend_from_slice(b"@rtest\0");
        }
        guest.resize(4096, 0);
        let size = guest.len();
        guest[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        let copy = guest.clone();
        let p = Proc::open(std::process::id()).unwrap();
        let guest_home = ("00:1a".to_string(), 42);
        let candidate = || (copy.as_ptr() as u64, size, ("00:00".to_string(), 0));
        let script = Script::from_candidates(&p, vec![candidate(), (guest.as_ptr() as u64, size, guest_home.clone())]).unwrap();
        assert_eq!(script.home, guest_home, "A native heap copy cannot be used as the VM's home");
        assert_eq!(script.len(), MIN_MESSAGES);
        assert_eq!(Script::from_candidates(&p, vec![candidate()]).unwrap().home.1, 0, "Standalone processes still need the anonymous-memory path");
    }

    #[test]
    fn reopening_recovers_current_position_without_resident_aliases() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = format!("/dev/shm/vntext-shin-test-{}-{unique}", std::process::id());
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let length = 16 * 4096;
        file.set_len(length as u64).unwrap();
        let write = |page: u64, start: u32, end: u32| {
            file.write_at(&end.to_le_bytes(), page + LIVE_CELL - 8).unwrap();
            file.write_at(&start.to_le_bytes(), page + LIVE_CELL).unwrap();
        };
        write(2 * 4096, 0x80, 0x90);
        write(9 * 4096, 0x80, 0x91); // A copy with the wrong program counter must not be trusted.
        let meta = file.metadata().unwrap();
        let address = unsafe { libc::mmap(std::ptr::null_mut(), length, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, file.as_raw_fd(), 0) };
        assert_ne!(address, libc::MAP_FAILED);
        struct Mapped(*mut libc::c_void, usize);
        impl Drop for Mapped { fn drop(&mut self) { unsafe { libc::munmap(self.0, self.1); } } }
        let _mapped = Mapped(address, length);
        let script = Script {
            messages: vec![Message { start: 0x80, end: 0x90, text: "@r例一".to_string() }, Message { start: 0x100, end: 0x110, text: "@r例二".to_string() }],
            by_start: HashMap::from([(0x80, 0), (0x100, 1)]), size: 4096, layout: &LAYOUTS[0],
            home: (format!("{:02x}:{:02x}", libc::major(meta.dev()), libc::minor(meta.dev())), meta.ino()),
        };
        let expected = address as u64 + 2 * 4096 + LIVE_CELL;
        for index in 0..2 {
            let p = Proc::open(std::process::id()).unwrap();
            let mut resident = 0;
            p.page_heads(|m| script.at_home(m), 20, |_, _| resident += 1).unwrap();
            assert_eq!(resident, 0, "Reopening must not depend on the game's mappings having PTEs");
            let found = script.locate(&p, false);
            assert_eq!(found.cells.len(), 1);
            assert_eq!(found.cells[0].addr, expected);
            assert!(found.cells[0].trusted);
            assert_eq!(script.read(&p, expected), Some(index));
            assert_eq!(file.metadata().unwrap().blocks(), meta.blocks(), "The sparse holes must stay unallocated");
            write(2 * 4096, 0x100, 0x110);
            assert_eq!(unsafe { libc::madvise(address, length, libc::MADV_DONTNEED) }, 0);
        }
    }

    /// Optional read-only diagnosis; reports positions, never game dialogue.
    #[test]
    #[ignore]
    fn live_position() {
        let pid: u32 = std::env::var("VNTEXT_SHIN_PID").expect("Set the running game's PID explicitly").parse().unwrap();
        let p = Proc::open(pid).unwrap();
        let script = Script::find(&p).expect("A loaded shin scenario is required");
        eprintln!("scenario_home={}:{} size={}", script.home.0, script.home.1, script.size);
        for m in crate::mem::read_maps(pid).unwrap().iter().filter(|m| script.at_home(m)).take(4) {
            eprintln!("home_mapping={:x}-{:x} {} offset={} path={}", m.start, m.end, m.perms, m.offset, m.path);
        }
        let mut resident = 0;
        p.page_heads(|m| script.at_home(m), LIVE_CELL as usize + 4, |_, head| {
            resident += usize::from(script.live_head(head));
        }).unwrap();
        let started = std::time::Instant::now();
        let positions = script.locate(&p, false);
        eprintln!("profile={} passages={} resident_live={} recovered_live={} locate_ms={}",
                  script.layout.game, script.len(), resident, positions.cells.len(), started.elapsed().as_millis());
        for cell in positions.cells {
            eprintln!("position={:x} trusted={} readable={}", cell.addr, cell.trusted, script.read(&p, cell.addr).is_some());
        }
        let finder = memmem::Finder::new(MAGIC);
        let mut candidates = Vec::new();
        let stats = p.scan_shared(|_| true, |base, data, limit, m| {
            for off in finder.find_iter(data).take_while(|off| *off < limit) {
                if let Some(bytes) = data.get(off + 4..off + 8) {
                    candidates.push((base + off as u64, u32::from_le_bytes(bytes.try_into().unwrap()) as usize, (m.dev.clone(), m.inode)));
                }
            }
        }).unwrap();
        eprintln!("shared_scan_maps={} shared_scan_bytes={} candidates={}", stats.maps, stats.bytes, candidates.len());
        if let Some(shared) = Script::from_candidates(&p, candidates) {
            eprintln!("shared_scenario_home={}:{} size={} passages={} layout={}", shared.home.0, shared.home.1, shared.size, shared.len(), shared.layout.game);
            let found = shared.locate(&p, true);
            eprintln!("shared_positions={} trusted={}", found.cells.len(), found.cells.iter().filter(|c| c.trusted).count());
            for cell in found.cells.iter().take(8) {
                eprintln!("shared_position={:x} trusted={} index={:?}", cell.addr, cell.trusted, shared.read(&p, cell.addr));
            }
        }
    }

    #[test]
    fn message_parts() {
        // 右代宮　金蔵@r@v01/11500003.｢我が友ﾖ｡@k@v01/11500005.･･･源次｣
        let raw = b"\x89\x45\x91\xe3\x8b\x7b\x81\x40\x8b\xe0\x91\xa0@r@v01/11500003.\xa2\x89\xe4\x82\xaa\x97\x46\xd6\xa1@k@v01/11500005.\xa5\xa5\xa5\x8c\xb9\x8e\x9f\xa3";
        let message = Message { start: 0, end: 0, text: decode(raw).unwrap() };
        let (name, lines) = message.parts();
        assert_eq!(name.as_deref(), Some("右代宮　金蔵"));
        assert_eq!(lines, ["「我が友よ。", "………源次」"]);

        let narration = Message { start: 0, end: 0, text: "@r室内ｦ満ﾀｼﾃｲﾙｿﾉ匂ｲ｡".to_string() };
        assert_eq!(narration.parts(), (None, vec!["室内を満たしているその匂い。".to_string()]));
    }
}
