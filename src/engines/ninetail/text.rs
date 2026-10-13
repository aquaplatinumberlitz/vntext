//! Scripts of Ninetail's Unity engine: the asset bundles they come in, the
//! scenes in them and the table of character names.

use std::collections::HashMap;

use memchr::memmem;
use serde_json::Value;

/// Decompresses one LZ4 block of known unpacked size.
pub fn lz4(src: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut at = 0;
    // a length of 15 continues in the bytes that follow, until one is not 255
    let extend = |mut len: usize, at: &mut usize| -> Option<usize> {
        if len == 15 {
            loop {
                let more = *src.get(*at)?;
                *at += 1;
                len += more as usize;
                if more != 255 {
                    break;
                }
            }
        }
        Some(len)
    };
    while at < src.len() {
        let token = src[at];
        at += 1;
        let literals = extend((token >> 4) as usize, &mut at)?;
        out.extend_from_slice(src.get(at..at + literals)?);
        at += literals;
        if at >= src.len() {
            break;
        }
        let back = u16::from_le_bytes([src[at], *src.get(at + 1)?]) as usize;
        at += 2;
        let len = extend((token & 15) as usize, &mut at)? + 4;
        if back == 0 || back > out.len() {
            return None;
        }
        for _ in 0..len {
            out.push(out[out.len() - back]);
        }
    }
    (out.len() == size).then_some(out)
}

/// Decompresses an LZMA block: five bytes of properties and the stream,
/// without the size that an `.lzma` file would have in between.
fn lzma(stored: &[u8], size: usize) -> Option<Vec<u8>> {
    use lzma_rs::decompress::{Options, UnpackedSize};
    let options = Options { unpacked_size: UnpackedSize::UseProvided(Some(size as u64)), ..Default::default() };
    let mut out = Vec::with_capacity(size);
    lzma_rs::lzma_decompress_with_options(&mut &stored[..], &mut out, &options).ok()?;
    (out.len() == size).then_some(out)
}

const COMPRESSION: u32 = 0x3F;
const INFO_AT_END: u32 = 0x80;
const PADDED: u32 = 0x200;

fn unpack(stored: &[u8], size: usize, compression: u32) -> Option<Vec<u8>> {
    match compression {
        0 => Some(stored.to_vec()),
        1 => lzma(stored, size),
        2 | 3 => lz4(stored, size),
        _ => None,
    }
}

/// The contents of a `UnityFS` bundle: its data blocks, unpacked and joined.
/// The files in it are not told apart; what is wanted is found by content.
pub fn bundle(data: &[u8]) -> Option<Vec<u8>> {
    let be32 = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().unwrap()));
    if !data.starts_with(b"UnityFS\0") {
        return None;
    }
    let version = be32(8)?;
    // two version strings, then `size: u64, packed info: u32, info: u32, flags: u32`
    let mut at = 12;
    for _ in 0..2 {
        at += memchr::memchr(0, data.get(at..)?)? + 1;
    }
    let (packed, size, flags) = (be32(at + 8)? as usize, be32(at + 12)? as usize, be32(at + 16)?);
    at += 20;
    if version >= 7 {
        at = at.next_multiple_of(16);
    }
    let info_at = if flags & INFO_AT_END != 0 { data.len().checked_sub(packed)? } else { at };
    let info = unpack(data.get(info_at..info_at + packed)?, size, flags & COMPRESSION)?;
    if flags & INFO_AT_END == 0 {
        at += packed;
    }
    if flags & PADDED != 0 {
        at = at.next_multiple_of(16);
    }
    // a 16-byte hash, the block count, then `size: u32, packed: u32, flags: u16` each
    let count = u32::from_be_bytes(info.get(16..20)?.try_into().unwrap()) as usize;
    let mut out = Vec::new();
    for block in info.get(20..)?.chunks_exact(10).take(count) {
        let size = u32::from_be_bytes(block[0..4].try_into().unwrap()) as usize;
        let packed = u32::from_be_bytes(block[4..8].try_into().unwrap()) as usize;
        let flags = u16::from_be_bytes([block[8], block[9]]) as u32;
        out.extend(unpack(data.get(at..at + packed)?, size, flags & COMPRESSION)?);
        at += packed;
    }
    Some(out)
}

fn le32(data: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().unwrap()) as usize)
}

/// The scene in an unpacked scenario bundle. A text asset stores its text
/// as `length: u32` and the bytes, and a scene's text is JSON that starts
/// with its file name.
pub fn scene(assets: &[u8]) -> Option<Value> {
    memmem::find_iter(assets, b"{\"fileName\":").find_map(|at| {
        let len = le32(assets, at.checked_sub(4)?)?;
        serde_json::from_slice(assets.get(at..at + len)?).ok()
    })
}

/// The text asset of the given name in a serialized assets file: `length:
/// u32`, the name, padding to four bytes, then the text stored the same way.
pub fn named<'a>(assets: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let mut stored = (name.len() as u32).to_le_bytes().to_vec();
    stored.extend_from_slice(name.as_bytes());
    memmem::find_iter(assets, &stored).find_map(|at| {
        let text = (at + stored.len()).next_multiple_of(4);
        let len = le32(assets, text)?;
        // nothing else with this name has text right behind it
        let found = assets.get(text + 4..text + 4 + len)?;
        std::str::from_utf8(found).is_ok().then_some(found)
    })
}

/// The names of the characters by their tags, from the `@char` part of the
/// `advdb_dataset` table: `tag, id, name, ...` on each line.
pub fn characters(table: &str) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let mut part = "";
    for line in table.lines() {
        let mut columns = line.split(',');
        let (Some(tag), _, Some(name)) = (columns.next(), columns.next(), columns.next()) else { continue };
        if tag.starts_with('@') {
            part = tag;
        } else if part == "@char" && !tag.is_empty() && !tag.starts_with('#') {
            names.insert(tag.to_string(), name.to_string());
        }
    }
    names
}

/// What one cut of a scene shows.
pub struct Shown {
    pub speaker: Option<String>,
    /// the text with `;` for line breaks, as stored
    pub text: String,
    pub choices: Vec<String>,
}

/// The cuts of a scene; `None` for those that show no text.
pub fn cuts(scene: &Value, names: &HashMap<String, String>) -> Vec<Option<Shown>> {
    let cuts = scene["cuts"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    cuts.iter()
        .map(|cut| {
            let window = &cut["window"];
            let text = window["texts"][0].as_str().filter(|t| !t.is_empty())?;
            if cut["window_enable"].as_bool() != Some(true) {
                return None;
            }
            // a name written in the cut wins over the one the tag stands for
            let name = window["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string);
            let speaker = name.or_else(|| names.get(window["tag"].as_str()?).cloned()).filter(|n| !n.is_empty());
            let mut choices = Vec::new();
            if cut["select_enable"].as_bool() == Some(true) {
                let listed = cut["select"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                choices.extend(listed.iter().filter_map(|c| c["text"].as_str()).filter(|t| !t.is_empty()).map(str::to_string));
            }
            Some(Shown { speaker, text: text.to_string(), choices })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_block() {
        // five literals, then a match that repeats the last two bytes six times
        let packed = [0x58, b'a', b'b', b'c', b'd', b'e', 2, 0, 0x10, b'!'];
        assert_eq!(lz4(&packed, 18).unwrap(), format!("abc{}!", "de".repeat(7)).as_bytes());
        assert!(lz4(&packed, 17).is_none());
    }

    #[test]
    fn scenes() {
        let json = r#"{"fileName":"a.xml","cuts":[{"window_enable":false,"window":null},{"window_enable":true,"select_enable":true,"select":[{"text":"はい"},{"text":"いいえ"}],"window":{"texts":["一行目;二行目","","","",""],"name":"","tag":"x"}},{"window_enable":true,"window":{"texts":["地の文"],"name":"誰か","tag":"x"}}]}"#;
        let mut assets = vec![0u8; 8];
        assets.extend((json.len() as u32).to_le_bytes());
        assets.extend(json.as_bytes());
        let names = characters("@char,id,name,advId,more\nx,X,太郎,1,\n@face,x,y,zoom,\ny,1,2,3,\n");
        assert_eq!(names.len(), 1);
        let cuts = cuts(&scene(&assets).unwrap(), &names);
        assert!(cuts[0].is_none());
        let shown = cuts[1].as_ref().unwrap();
        assert_eq!((shown.speaker.as_deref(), shown.text.as_str(), shown.choices.len()), (Some("太郎"), "一行目;二行目", 2));
        assert_eq!(cuts[2].as_ref().unwrap().speaker.as_deref(), Some("誰か"));
    }

    #[test]
    fn named_assets() {
        let mut assets = vec![7u8; 4];
        assets.extend(5u32.to_le_bytes());
        assets.extend(b"table\0\0\0");
        assets.extend(3u32.to_le_bytes());
        assets.extend(b"a,b");
        assert_eq!(named(&assets, "table"), Some(&b"a,b"[..]));
    }
}
