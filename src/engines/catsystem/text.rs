//! Scripts of CatSystem2: the `KIF` archive `scene.int`, encrypted with a
//! key kept in the game's executable, and the `CatScene` scripts in it.
//! The formats are those of morkt's GARbro (`ArcINT.cs`) and
//! trigger-segfault's TriggersTools.CatSystem2 (`CstScene`).

use std::collections::HashMap;

use blowfish::cipher::{BlockDecrypt, KeyInit};
use blowfish::BlowfishLE;

fn le16(data: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().unwrap()) as usize)
}

fn le32(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

/// A resource of a Windows executable, found by the names of its type and its own.
fn resource(exe: &[u8], kind: &str, name: &str) -> Option<Vec<u8>> {
    let pe = le32(exe, 0x3c)?;
    if exe.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let sections = le16(exe, pe + 6)?;
    let optional = pe + 24;
    let directories = optional + if le16(exe, optional)? == 0x20b { 112 } else { 96 };
    let root_rva = le32(exe, directories + 2 * 8)?;
    let table = optional + le16(exe, pe + 20)?;
    let offset = |rva: usize| -> Option<usize> {
        (0..sections).find_map(|k| {
            let s = table + k * 40;
            let (start, len, raw) = (le32(exe, s + 12)?, le32(exe, s + 16)?.max(le32(exe, s + 8)?), le32(exe, s + 20)?);
            (start..start + len).contains(&rva).then(|| rva - start + raw)
        })
    };
    let root = offset(root_rva)?;
    // the entries of a directory with a name, or the first one when `wanted` is empty
    let entry = |dir: usize, wanted: &str| -> Option<usize> {
        let count = le16(exe, dir + 12)? + le16(exe, dir + 14)?;
        (0..count).find_map(|k| {
            let (id, to) = (le32(exe, dir + 16 + k * 8)?, le32(exe, dir + 20 + k * 8)?);
            let matches = wanted.is_empty() || {
                let at = root + (id & 0x7fff_ffff);
                let len = le16(exe, at)?;
                let units: Vec<u16> = (0..len).map(|i| le16(exe, at + 2 + i * 2).unwrap_or(0) as u16).collect();
                id & 0x8000_0000 != 0 && String::from_utf16_lossy(&units).eq_ignore_ascii_case(wanted)
            };
            matches.then_some(root + (to & 0x7fff_ffff))
        })
    };
    let data = entry(entry(entry(root, kind)?, name)?, "")?;
    let at = offset(le32(exe, data)?)?;
    Some(exe.get(at..at + le32(exe, data + 4)?)?.to_vec())
}

fn decipher(cipher: &BlowfishLE, data: &mut [u8]) {
    for block in data.chunks_exact_mut(8) {
        cipher.decrypt_block(block.into());
    }
}

/// The key of the archives: the passphrase in the executable's resource
/// `V_CODE2`, which is itself enciphered with the bytes of `KEY_CODE`
/// (each XORed with 0xCD), turned into a number by a CRC.
pub fn key(exe: &[u8]) -> Option<u32> {
    let mut code = resource(exe, "V_CODE2", "DATA")?;
    let pass = resource(exe, "KEY_CODE", "KEY").map_or(b"windmill".to_vec(), |k| k.iter().map(|b| b ^ 0xcd).collect());
    decipher(&BlowfishLE::new_from_slice(&pass).ok()?, &mut code);
    let phrase = &code[..code.iter().position(|b| *b == 0).unwrap_or(code.len())];
    let table: Vec<u32> = (0..256u32)
        .map(|i| (0..8).fold(i << 24, |c, _| if c & 0x8000_0000 != 0 { (c << 1) ^ 0x04c1_1db7 } else { c << 1 }))
        .collect();
    Some(phrase.iter().fold(u32::MAX, |key, c| !table[((key >> 24) ^ *c as u32) as usize] ^ (key << 8)))
}

/// The Mersenne twister as CatSystem2 has it: seeded the old way.
struct Twister {
    mt: [u32; 624],
    next: usize,
}

impl Twister {
    fn new(mut seed: u32) -> Twister {
        let mut mt = [0; 624];
        for cell in &mut mt {
            let upper = seed & 0xffff_0000;
            seed = seed.wrapping_mul(69069).wrapping_add(1);
            *cell = upper | (seed & 0xffff_0000) >> 16;
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        Twister { mt, next: 624 }
    }

    fn rand(&mut self) -> u32 {
        const MAG: [u32; 2] = [0, 0x9908_b0df];
        let mt = &mut self.mt;
        if self.next >= 624 {
            for k in 0..624 {
                let y = (mt[k] & 0x8000_0000) | (mt[(k + 1) % 624] & 0x7fff_ffff);
                mt[k] = mt[(k + 397) % 624] ^ (y >> 1) ^ MAG[(y & 1) as usize];
            }
            self.next = 0;
        }
        let mut y = mt[self.next];
        self.next += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }
}

/// A file name of the index, each letter shifted back along a reversed alphabet.
fn decipher_name(name: &[u8], key: u32) -> String {
    const ALPHABET: &[u8] = b"zyxwvutsrqponmlkjihgfedcbaZYXWVUTSRQPONMLKJIHGFEDCBA";
    let mut k = (key >> 24).wrapping_add(key >> 16).wrapping_add(key >> 8).wrapping_add(key) as u8 as usize;
    let mut out = Vec::new();
    for b in name.iter().take_while(|b| **b != 0) {
        out.push(match ALPHABET.iter().position(|a| a == b) {
            Some(j) => ALPHABET[0x33 - (j + 0x34 - k % 0x34) % 0x34],
            None => *b,
        });
        k += 1;
    }
    encoding_rs::SHIFT_JIS.decode(&out).0.into_owned()
}

/// The files of a `KIF` archive, by name and in the order of its index.
/// An archive whose first file is `__key__.dat` is encrypted: its index
/// with Blowfish under a key from that file's length and with `key`, its
/// files with the same Blowfish.
pub fn archive(data: &[u8], key: Option<u32>) -> Option<Vec<(String, Vec<u8>)>> {
    const ENTRY: usize = 0x48;
    if !data.starts_with(b"KIF\0") {
        return None;
    }
    let count = le32(data, 4)?;
    let entry = |k: usize| data.get(8 + k * ENTRY..8 + (k + 1) * ENTRY);
    let mut files = Vec::new();
    if entry(0)?.starts_with(b"__key__.dat\0") {
        let key = key?;
        let seed = le32(entry(0)?, 0x44)? as u32;
        let cipher = BlowfishLE::new_from_slice(&Twister::new(seed).rand().to_le_bytes()).ok()?;
        for k in 1..count {
            let e = entry(k)?;
            let mut place = [(le32(e, 0x40)? as u32).wrapping_add(k as u32).to_le_bytes(), (le32(e, 0x44)? as u32).to_le_bytes()].concat();
            decipher(&cipher, &mut place);
            let (at, len) = (le32(&place, 0)?, le32(&place, 4)?);
            let name = decipher_name(&e[..0x40], Twister::new(key.wrapping_add(k as u32)).rand());
            let mut file = data.get(at..at + len)?.to_vec();
            decipher(&cipher, &mut file[..len / 8 * 8]);
            files.push((name, file));
        }
    } else {
        for k in 0..count {
            let e = entry(k)?;
            let name = &e[..e[..0x40].iter().position(|b| *b == 0)?];
            let (at, len) = (le32(e, 0x40)?, le32(e, 0x44)?);
            files.push((encoding_rs::SHIFT_JIS.decode(name).0.into_owned(), data.get(at..at + len)?.to_vec()));
        }
    }
    Some(files)
}

/// What a line of a scene is: a message, or the speaker of the messages after it.
const MESSAGE: u16 = 0x2001;
const NAME: u16 = 0x2101;

/// A scene, cut where the reader clicks: what the interpreter calls an
/// input, and its position is the input and a line within it.
pub struct Scene {
    /// the header of the script, as the interpreter keeps it in memory
    pub header: [u8; 16],
    pub inputs: Vec<Input>,
}

#[derive(Default)]
pub struct Input {
    pub lines: usize,
    pub speaker: Option<String>,
    /// the messages, joined
    pub text: String,
}

/// Unpacks a `CatScene` or `FES` file: a signature of 8 or 4 bytes, the
/// packed and the unpacked lengths, and from byte 16 zlib data.
fn unpack(file: &[u8], signature: &[u8]) -> Option<Vec<u8>> {
    if !file.starts_with(signature) {
        return None;
    }
    miniz_oxide::inflate::decompress_to_vec_zlib(file.get(16..16 + le32(file, signature.len())?)?).ok()
}

/// A `CatScene` file. The script in it has a header of four lengths, a table
/// of the inputs (how many lines each has and where it starts), a table of
/// where each line starts and the lines, each a kind in two bytes and a string.
pub fn scene(file: &[u8]) -> Option<Scene> {
    let script = unpack(file, b"CatScene")?;
    let count = le32(&script, 4)?;
    let (table, strings) = (le32(&script, 8)? + 16, le32(&script, 12)? + 16);
    let line = |k: usize| -> Option<(u16, String)> {
        let start = strings + le32(&script, table + k * 4)?;
        let rest = script.get(start + 2..)?;
        let text = &rest[..rest.iter().position(|b| *b == 0)?];
        Some((le16(&script, start)? as u16, encoding_rs::SHIFT_JIS.decode(text).0.into_owned()))
    };
    let mut inputs = Vec::with_capacity(count);
    for k in 0..count {
        let (lines, first) = (le32(&script, 16 + k * 8)?, le32(&script, 20 + k * 8)?);
        let mut input = Input { lines, ..Input::default() };
        for j in first..first + lines {
            match line(j)? {
                (MESSAGE, text) => input.text += &text,
                (NAME, name) => input.speaker = Some(name),
                _ => {}
            }
        }
        inputs.push(input);
    }
    Some(Scene { header: script.get(..16)?.try_into().unwrap(), inputs })
}

/// The values the screens of `FES` files give string variables before the
/// player changes them, such as the hero's name, by number. The screens
/// name the variables, and list each name with its number.
pub fn defaults(files: &[(String, Vec<u8>)]) -> HashMap<u32, String> {
    let texts: Vec<String> =
        files.iter().filter_map(|(_, file)| unpack(file, b"FES\0")).map(|text| encoding_rs::SHIFT_JIS.decode(&text).0.into_owned()).collect();
    let mut numbers = HashMap::new();
    for line in texts.iter().flat_map(|text| text.lines()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        if let [name, number] = words[..] {
            if let Ok(number) = number.parse::<u32>() {
                numbers.insert(name.to_string(), number);
            }
        }
    }
    let mut values = HashMap::new();
    for line in texts.iter().flat_map(|text| text.lines()) {
        if let ["strvar", name, value] = line.split_whitespace().collect::<Vec<_>>()[..] {
            if let Some(number) = name.parse().ok().or_else(|| numbers.get(name).copied()) {
                values.entry(number).or_insert_with(|| value.to_string());
            }
        }
    }
    values
}

/// The lines of text in a message as the reader sees them: ruby
/// `[base/reading]` shows its base, `\n` breaks the line, other escapes
/// (`\@` to go on without a click, `\fss` and the like for the size of the
/// letters) show nothing, and `$strN` is the string variable.
pub fn plain(text: &str, var: impl Fn(u32) -> String) -> Vec<String> {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some(c) if c.is_ascii_alphabetic() => {
                    while chars.next_if(char::is_ascii_alphabetic).is_some() {}
                }
                _ => {}
            },
            '[' => {
                let ruby: String = chars.by_ref().take_while(|c| *c != ']').collect();
                out += ruby.split_once('/').map_or(ruby.as_str(), |(base, _)| base);
            }
            '$' if chars.clone().take(3).eq("str".chars()) => {
                chars.nth(2);
                let mut number = String::new();
                while let Some(d) = chars.next_if(char::is_ascii_digit) {
                    number.push(d);
                }
                out += &number.parse().map(&var).unwrap_or_default();
            }
            c => out.push(c),
        }
    }
    out.lines().map(|line| line.trim_end().to_string()).filter(|line| !line.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text() {
        let name = |key| if key == 20 { "勇希".to_string() } else { String::new() };
        assert_eq!(plain("１１月も『[夜間瀬町/よませまち]』は", name), ["１１月も『夜間瀬町』は"]);
        assert_eq!(plain("「えっと、あの、\\nこれをどうぞ……」\\@", name), ["「えっと、あの、", "これをどうぞ……」"]);
        assert_eq!(plain("\\fss「俺は$str10$str20です」", name), ["「俺は勇希です」"]);
    }
}
