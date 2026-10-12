//! Turning message text of BISHOP's engine into what the game shows.

/// `\n` breaks the line for the text box only, so it is dropped along with
/// the indent after it; `\b` and `\s` change the text size. `\A` and `\B`
/// stand for the protagonist's family and given name.
pub fn readable(text: &str, family: &str, given: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => while chars.next_if_eq(&'\u{3000}').is_some() {},
                Some('b' | 's') => {}
                Some('A') => out.push_str(family),
                Some('B') => out.push_str(given),
                Some(other) => out.extend(['\\', other]),
                None => out.push('\\'),
            },
            '\u{E000}' => out.push('♥'),
            // Glyphs of the game's own font. Going by where they occur
            // (U+E042 for あ, U+E0A2 for ア, ...) they are kana with a
            // voicing mark; this is inferred, not documented.
            '\u{E001}'..='\u{E0FF}' => {
                out.extend(char::from_u32(0x3000 + (c as u32 & 0xFF)));
                out.push('゛');
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::readable;

    #[test]
    fn readable_text() {
        let read = |text| readable(text, "金本", "一也");
        assert_eq!(read("「あひィインッ\u{E000}　何これぇっ\u{E000}\\n　気持ちいい」"), "「あひィインッ♥　何これぇっ♥気持ちいい」");
        assert_eq!(read("\\b「\\A\\B君、\\n　ちょっといいかしら？」"), "「金本一也君、ちょっといいかしら？」");
        assert_eq!(read("\\B"), "一也");
        assert_eq!(read("\u{E042}あ"), "あ゛あ");
    }
}
