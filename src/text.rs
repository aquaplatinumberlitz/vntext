//! Finding Japanese strings in raw memory, in UTF-8, UTF-16LE and Shift-JIS.

use encoding_rs::SHIFT_JIS;
use memchr::{memchr, memchr2, memrchr};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Enc {
    Utf8,
    Utf16,
    Sjis,
}

impl Enc {
    pub const ALL: [Enc; 3] = [Enc::Utf8, Enc::Utf16, Enc::Sjis];

    pub fn name(self) -> &'static str {
        match self {
            Enc::Utf8 => "utf8",
            Enc::Utf16 => "utf16",
            Enc::Sjis => "sjis",
        }
    }

    pub fn parse(s: &str) -> Option<Enc> {
        Enc::ALL.into_iter().find(|e| e.name() == s)
    }
}

pub struct Hit {
    pub addr: u64,
    pub enc: Enc,
    pub text: String,
}

#[derive(Clone, Copy)]
pub struct Cfg {
    pub min_chars: usize,
    pub min_kana: usize,
}

const BACK: usize = 2048;
const FWD: usize = 4096;
const MAX_CHARS: usize = 2000;

fn ok16(u: u16) -> bool {
    matches!(u,
        0x20..=0x7E | 0x0A | 0x0D
        | 0x2010..=0x203B | 0x2190..=0x21FF | 0x2500..=0x257F | 0x25A0..=0x266F
        | 0x3000..=0x30FF | 0x4E00..=0x9FFF
        | 0xF8F0 | 0xFF01..=0xFF5E | 0xFF61..=0xFF9F | 0xFFE5)
}

fn allowed(c: char) -> bool {
    let u = c as u32;
    u <= 0xFFFF && ok16(u as u16)
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}')
}

fn encodable(c: char) -> bool {
    let mut b = [0u8; 4];
    !SHIFT_JIS.encode(c.encode_utf8(&mut b)).2
}

/// Decides whether a decoded run is real Japanese text rather than binary data
/// that happens to decode into kana (pixel and audio data do this constantly).
pub fn is_japanese(text: &str, enc: Enc, cfg: &Cfg) -> bool {
    let (mut n, mut kana, mut jp, mut wide, mut doubled) = (0, 0, 0, 0, 0);
    let mut distinct: Vec<char> = Vec::new();
    let (mut prev_kana, mut pair) = (false, false);
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        n += 1;
        // half-width kana only count in UTF-8: in UTF-16 any xx FF pair looks like one
        let k = is_kana(c) || (enc == Enc::Utf8 && crate::clean::is_half_kana(c));
        // real sentences have kana next to kana; noise alternates blocks
        pair |= k && prev_kana;
        prev_kana = k || c == 'ー';
        kana += k as usize;
        jp += (c as u32 >= 0x80) as usize;
        wide += matches!(c, '\u{FF01}'..='\u{FF5E}') as usize;
        // 81 81 / 82 82 / 83 83: what runs of identical bytes decode to in Shift-JIS
        doubled += matches!(c, '＝' | 'ｂ' | 'ャ') as usize;
        if distinct.len() < 16 && !distinct.contains(&c) {
            distinct.push(c);
        }
        // kanji outside the Shift-JIS repertoire practically never occur in games
        if c as u32 >= 0x4E00 && !encodable(c) {
            return false;
        }
    }
    // UTF-8 validates itself; the other two need more characters to be believable
    n >= cfg.min_chars.max(if enc == Enc::Utf8 { 0 } else { 6 })
        && n <= MAX_CHARS
        && kana >= cfg.min_kana
        && (pair || cfg.min_kana == 0)
        // control codes can outweigh the text itself; valid UTF-8 is strong evidence already
        && jp * if enc == Enc::Utf8 { 3 } else { 2 } >= n
        && wide * 2 <= n
        && distinct.len() >= 3
        && distinct.len() * 4 >= n.min(40)
        // Shift-JIS has no structure to validate, so it gets a stricter test
        && (enc != Enc::Sjis || (jp * 3 >= n * 2 && doubled * 3 <= n))
}

fn unit(buf: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([buf[i], buf[i + 1]])
}

fn sjis_lead(b: u8) -> bool {
    matches!(b, 0x81..=0x9F | 0xE0..=0xFC)
}

fn sjis_trail(b: u8) -> bool {
    matches!(b, 0x40..=0x7E | 0x80..=0xFC)
}

type Chars = Vec<(usize, char)>;

fn dec_utf8(span: &[u8], v: &mut Chars) {
    let mut off = 0;
    for chunk in span.utf8_chunks() {
        v.extend(chunk.valid().char_indices().map(|(i, c)| (off + i, c)));
        off += chunk.valid().len();
        if !chunk.invalid().is_empty() {
            v.push((off, '\u{FFFD}'));
            off += chunk.invalid().len();
        }
    }
}

fn dec_utf16(span: &[u8], v: &mut Chars) {
    for (i, u) in span.chunks_exact(2).enumerate() {
        let u = u16::from_le_bytes([u[0], u[1]]);
        v.push((i * 2, char::from_u32(u as u32).unwrap_or('\u{FFFD}')));
    }
}

fn dec_sjis(span: &[u8], v: &mut Chars) {
    let mut j = 0;
    while j < span.len() {
        let b = span[j];
        if b < 0x80 {
            v.push((j, b as char));
            j += 1;
        } else if sjis_lead(b) && j + 1 < span.len() && sjis_trail(span[j + 1]) {
            let c = SHIFT_JIS
                .decode_without_bom_handling_and_without_replacement(&span[j..j + 2])
                .and_then(|s| s.chars().next())
                .unwrap_or('\u{FFFD}');
            v.push((j, c));
            j += 2;
        } else {
            // includes half-width katakana: too common in binary noise to accept
            v.push((j, '\u{FFFD}'));
            j += 1;
        }
    }
}

fn decode(span: &[u8], enc: Enc, v: &mut Chars) {
    v.clear();
    match enc {
        Enc::Utf8 => dec_utf8(span, v),
        Enc::Utf16 => dec_utf16(span, v),
        Enc::Sjis => dec_sjis(span, v),
    }
}

/// NUL-bounded span around `p`, not reaching back before `floor`.
fn nul_span(buf: &[u8], p: usize, floor: usize) -> (usize, usize) {
    let lo = p.saturating_sub(BACK).max(floor);
    let hi = buf.len().min(p + FWD);
    let s = memrchr(0, &buf[lo..p]).map_or(lo, |i| lo + i + 1);
    let e = memchr(0, &buf[p..hi]).map_or(hi, |i| p + i);
    (s, e)
}

struct Sink<'a> {
    base: u64,
    limit: usize,
    cfg: &'a Cfg,
    out: &'a mut Vec<Hit>,
    /// byte ranges already claimed by a UTF-8 / UTF-16 string
    cov: Vec<(usize, usize)>,
}

impl Sink<'_> {
    /// Splits decoded chars (offsets relative to span start `s`) into runs of
    /// allowed characters and reports the ones that look Japanese.
    fn runs(&mut self, chars: &Chars, s: usize, span_len: usize, enc: Enc) {
        let n = chars.len();
        let mut a = 0;
        while a < n {
            if !allowed(chars[a].1) || chars[a].1.is_whitespace() {
                a += 1;
                continue;
            }
            let mut b = a;
            while b < n && allowed(chars[b].1) {
                b += 1;
            }
            let mut last = b;
            while chars[last - 1].1.is_whitespace() {
                last -= 1;
            }
            let text: String = chars[a..last].iter().map(|c| c.1).collect();
            if is_japanese(&text, enc, self.cfg) {
                let start = s + chars[a].0;
                let end = s + if b < n { chars[b].0 } else { span_len };
                if start < self.limit {
                    self.out.push(Hit { addr: self.base + start as u64, enc, text });
                }
                if enc != Enc::Sjis {
                    self.cov.push((start, end));
                }
            }
            a = b;
        }
    }

    fn covered(&self, p: usize) -> Option<usize> {
        let i = self.cov.partition_point(|r| r.0 <= p);
        (i > 0 && p < self.cov[i - 1].1).then(|| self.cov[i - 1].1)
    }
}

/// Appends every Japanese-looking string in `buf` that starts before `limit`.
pub fn scan(buf: &[u8], base: u64, limit: usize, cfg: &Cfg, out: &mut Vec<Hit>) {
    let mut sink = Sink { base, limit, cfg, out, cov: Vec::new() };
    let mut chars = Chars::new();
    let len = buf.len();

    // UTF-8: kana are E3 81..83 xx
    let mut i = 0;
    while let Some(k) = memchr(0xE3, &buf[i..]) {
        let p = i + k;
        if p >= limit {
            break;
        }
        if p + 2 < len && matches!(buf[p + 1], 0x81..=0x83) && matches!(buf[p + 2], 0x80..=0xBF) {
            let (s, e) = nul_span(buf, p, i);
            decode(&buf[s..e], Enc::Utf8, &mut chars);
            sink.runs(&chars, s, e - s, Enc::Utf8);
            i = e.max(p + 1);
        } else {
            i = p + 1;
        }
    }

    // UTF-16LE: kana are xx 30
    let mut i = 0;
    while let Some(k) = memchr(0x30, &buf[i..]) {
        let p = i + k;
        if p >= limit + 1 {
            break;
        }
        let jp = |at: usize| at + 1 < len && unit(buf, at) >= 0x3000 && ok16(unit(buf, at));
        if p > i && (base as usize + p) % 2 == 1 && matches!(buf[p - 1], 0x41..=0x96 | 0xA1..=0xFA) && (jp(p + 1) || (p >= i + 3 && jp(p - 3))) {
            let u = p - 1;
            let (mut s, mut e) = (u, u + 2);
            while s >= i + 2 && u - s < BACK && ok16(unit(buf, s - 2)) {
                s -= 2;
            }
            while e + 2 <= len && e - u < FWD && ok16(unit(buf, e)) {
                e += 2;
            }
            decode(&buf[s..e], Enc::Utf16, &mut chars);
            sink.runs(&chars, s, e - s, Enc::Utf16);
            i = e;
        } else {
            i = p + 1;
        }
    }

    // Shift-JIS: hiragana 82 9F..F1, katakana 83 40..96
    sink.cov.sort_unstable();
    let mut i = 0;
    while let Some(k) = memchr2(0x82, 0x83, &buf[i..]) {
        let p = i + k;
        if p >= limit {
            break;
        }
        let kana = p + 3 < len
            && match buf[p] {
                0x82 => matches!(buf[p + 1], 0x9F..=0xF1),
                _ => matches!(buf[p + 1], 0x40..=0x96),
            }
            && sjis_lead(buf[p + 2])
            && sjis_trail(buf[p + 3]);
        if !kana {
            i = p + 1;
            continue;
        }
        if let Some(end) = sink.covered(p) {
            i = end.max(p + 1);
            continue;
        }
        let (mut s, e) = nul_span(buf, p, i);
        // Without a NUL in reach the window may start in the middle of a
        // two-byte character and decode as garbage (large preloaded scripts).
        // Bytes below 0x40 are never trail bytes, so resync after the first one.
        if s > 0 && buf[s - 1] != 0 {
            s = buf[s..p].iter().position(|&b| b < 0x40).map_or(p, |k| s + k + 1);
        }
        // UTF-8 text read as Shift-JIS decodes into plausible-looking mojibake.
        if std::str::from_utf8(&buf[s..e]).is_err() {
            decode(&buf[s..e], Enc::Sjis, &mut chars);
            sink.runs(&chars, s, e - s, Enc::Sjis);
        }
        i = e.max(p + 1);
    }
}

/// Decodes the string that starts at the beginning of `buf` (as read from a known address).
pub fn decode_at(buf: &[u8], enc: Enc) -> Option<String> {
    let end = match enc {
        Enc::Utf16 => buf.chunks_exact(2).position(|u| u == [0, 0]).map_or(buf.len() & !1, |i| i * 2),
        _ => memchr(0, buf).unwrap_or(buf.len()),
    };
    let mut chars = Chars::new();
    decode(&buf[..end], enc, &mut chars);
    let text: String = chars.iter().map(|c| c.1).take_while(|c| allowed(*c)).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub fn encode(text: &str, enc: Enc) -> Option<Vec<u8>> {
    match enc {
        Enc::Utf8 => Some(text.as_bytes().to_vec()),
        Enc::Utf16 => Some(text.encode_utf16().flat_map(u16::to_le_bytes).collect()),
        Enc::Sjis => {
            let (bytes, _, bad) = SHIFT_JIS.encode(text);
            (!bad).then(|| bytes.into_owned())
        }
    }
}
