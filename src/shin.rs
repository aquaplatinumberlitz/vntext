//! Engine profile for Entergram's "shin" engine (Umineko Saku, Higurashi Hou, ...).
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

use crate::clean::clean;
use crate::mem::Proc;

const MAGIC: &[u8] = b"SNR ";
const MSGSET: u8 = 0x86;
/// Fewer messages than this and the "scenario" is something else.
const MIN_MESSAGES: usize = 50;

pub struct Message {
    /// offsets of the MSGSET instruction inside the scenario
    pub start: u32,
    pub end: u32,
    /// text as stored: `@` commands and half-width kana included
    pub text: String,
}

impl Message {
    /// Speaker name (absent for narration) and the readable text of each
    /// section; the game reveals one section per click (`@k`).
    pub fn parts(&self) -> (Option<String>, Vec<String>) {
        let (name, body) = self.text.split_once("@r").unwrap_or(("", &self.text));
        let name = clean(name);
        let sections = body.split("@k").map(clean).filter(|s| !s.is_empty()).collect();
        ((!name.is_empty()).then_some(name), sections)
    }
}

pub struct Script {
    pub messages: Vec<Message>,
    by_start: HashMap<u32, usize>,
    size: usize,
}

/// Message text is Shift-JIS in which single bytes A0..DF stand for the
/// engine's own shorthand; they are mapped to the code points that
/// `clean::clean` expects (U+F8F0 and half-width katakana).
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
    /// Picks the messages out of a scenario image. A MSGSET instruction is
    /// `86 | id: u24 | wait: u8 | len: u16 | text` with `len` counting the
    /// terminating NUL. Message ids are consecutive, which is what separates
    /// real instructions from bytes that merely look like one.
    fn parse(data: &[u8]) -> Script {
        let mut found: Vec<(u32, Message)> = Vec::new();
        let mut i = 0;
        while let Some(k) = memchr::memchr(MSGSET, &data[i..]) {
            let at = i + k;
            i = at + 1;
            let Some(head) = data.get(at..at + 7) else { break };
            let id = u32::from_le_bytes([head[1], head[2], head[3], 0]);
            let len = u16::from_le_bytes([head[5], head[6]]) as usize;
            let Some(body) = data.get(at + 7..at + 7 + len) else { continue };
            if head[4] > 1 || len < 2 || body[len - 1] != 0 || body[..len - 1].contains(&0) {
                continue;
            }
            let Some(text) = decode(&body[..len - 1]) else { continue };
            found.push((id, Message { start: at as u32, end: (at + 7 + len) as u32, text }));
        }
        let consecutive: Vec<bool> = (0..found.len())
            .map(|k| (k > 0 && found[k - 1].0 + 1 == found[k].0) || (k + 1 < found.len() && found[k].0 + 1 == found[k + 1].0))
            .collect();
        let messages: Vec<Message> = found.into_iter().zip(consecutive).filter(|(_, ok)| *ok).map(|((_, m), _)| m).collect();
        let by_start = messages.iter().enumerate().map(|(k, m)| (m.start, k)).collect();
        Script { messages, by_start, size: data.len() }
    }

    /// Looks for a loaded scenario in the target process.
    pub fn find(p: &Proc) -> Option<Script> {
        let finder = memmem::Finder::new(MAGIC);
        let mut candidates = Vec::new();
        p.scan(false, |base, data, limit, _| {
            for off in finder.find_iter(data).take_while(|off| *off < limit) {
                if let Some(size) = data.get(off + 4..off + 8) {
                    candidates.push((base + off as u64, u32::from_le_bytes(size.try_into().unwrap()) as usize));
                }
            }
        })
        .ok()?;
        for (addr, size) in candidates {
            if !(0x1000..=512 << 20).contains(&size) {
                continue;
            }
            let mut data = vec![0u8; size];
            if p.read(addr, &mut data) != size {
                continue;
            }
            let script = Script::parse(&data);
            if script.messages.len() >= MIN_MESSAGES {
                return Some(script);
            }
        }
        None
    }

    /// Finds the VM's position cells: a 32-bit cell holding the start offset
    /// of a MSGSET instruction with, 8 bytes before it, the offset just past
    /// that same instruction (the VM's program counter while it waits on the
    /// message). The pairing makes accidental matches practically impossible.
    pub fn locate_cursors(&self, p: &Proc) -> Vec<u64> {
        let mut is_start = vec![0u64; self.size / 64 + 1];
        for m in &self.messages {
            is_start[m.start as usize / 64] |= 1 << (m.start % 64);
        }
        let size = self.size as u32;
        let mut cursors = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
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

    /// The message a position cell currently points at, if any.
    pub fn message_at(&self, p: &Proc, cursor: u64) -> Option<usize> {
        let mut cell = [0u8; 4];
        if p.read(cursor, &mut cell) != 4 {
            return None;
        }
        self.by_start.get(&u32::from_le_bytes(cell)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
