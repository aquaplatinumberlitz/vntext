//! Message text of the BLACKRAINBOW `.spt` engine.

use encoding_rs::SHIFT_JIS;

/// Messages are Shift-JIS with the lowest bit of every fourth byte flipped.
pub fn decode(stored: &[u8]) -> String {
    let mut bytes = stored.to_vec();
    bytes.iter_mut().step_by(4).for_each(|b| *b ^= 1);
    SHIFT_JIS.decode(&bytes).0.into_owned()
}

/// Splits a message into speaker and text. A spoken line starts with
/// `【name】` on a line of its own; the other line breaks only wrap the text
/// in its box, so they are dropped.
pub fn split(message: &str) -> (Option<String>, String) {
    let (speaker, body) = match message.strip_prefix('【').and_then(|rest| rest.split_once("】\r\n")) {
        Some((name, body)) => (Some(name.to_string()), body),
        None => (None, message),
    };
    (speaker, body.replace("\r\n", ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_and_split() {
        // 「おは」 with bytes 0 and 4 flipped
        assert_eq!(decode(&[0x80, 0x75, 0x82, 0xA8, 0x83, 0xCD]), "「おは");
        assert_eq!(split("【桜井　佐和】\r\n「おはよう、進」"), (Some("桜井　佐和".to_string()), "「おはよう、進」".to_string()));
        assert_eq!(split("何かに反抗したり、行動を\r\nすることなど考えたこともない。"), (None, "何かに反抗したり、行動をすることなど考えたこともない。".to_string()));
    }
}
