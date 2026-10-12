//! Text of the MED engine: decryption of script files and the strings in them.

use encoding_rs::SHIFT_JIS;

/// Every script archive holds a file `_VIEW` whose first bytes are the same
/// in every game, which gives the game's key away.
pub const KNOWN_NAME: &str = "_VIEW";
const KNOWN_PLAIN: &[u8] = b"\0#RULE_VIEWER\0:VIEW_0\0{\0";
/// A script's first bytes (its sizes and counts) are stored in the clear.
pub const CLEAR: usize = 0x10;

/// Recovers the key from the encrypted `_VIEW` file: the cipher adds the key
/// to the text, repeating it, so known text minus stored text is the key.
pub fn recover_key(view: &[u8]) -> Option<Vec<u8>> {
    let stored = view.get(CLEAR..CLEAR + KNOWN_PLAIN.len())?;
    let stream: Vec<u8> = KNOWN_PLAIN.iter().zip(stored).map(|(plain, stored)| plain.wrapping_sub(*stored)).collect();
    // the shortest prefix that repeats into the whole stream
    (2..=stream.len()).map(|n| &stream[..n]).find(|key| stream.iter().zip(key.iter().cycle()).all(|(a, b)| a == b)).map(<[u8]>::to_vec)
}

pub fn decrypt(script: &mut [u8], key: &[u8]) {
    for (byte, key) in script.iter_mut().skip(CLEAR).zip(key.iter().cycle()) {
        *byte = byte.wrapping_add(*key);
    }
}

pub fn decode(stored: &[u8]) -> String {
    SHIFT_JIS.decode(stored).0.into_owned()
}

/// A speaker is written `【name】`.
pub fn speaker(name: &str) -> String {
    name.trim().trim_start_matches('【').trim_end_matches('】').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_recovery_and_decryption() {
        let key = b"tauromin";
        let mut view = vec![0u8; CLEAR];
        view.extend(KNOWN_PLAIN.iter().zip(key.iter().cycle()).map(|(p, k)| p.wrapping_sub(*k)));
        assert_eq!(recover_key(&view).unwrap(), key);
        decrypt(&mut view, key);
        assert_eq!(&view[CLEAR..], KNOWN_PLAIN);
        assert_eq!(speaker("【深町　悟】"), "深町　悟");
    }
}
