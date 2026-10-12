//! Turning raw in-memory text into readable lines. The rules here are the
//! first "engine profile": the Entergram "shin" engine (Umineko Saku,
//! Higurashi Hou, ...). The fixup table and the command grammar are taken
//! from DCNick3's reimplementation, https://github.com/DCNick3/shin
//! (shin-core/src/format/text/mod.rs and shin-core/src/layout/parser.rs).
//! They are harmless on text that does not use this syntax.

/// The engine stores common characters as half-width katakana, which take a
/// single byte in Shift-JIS. Same position in both strings = same character.
const FIXUP_ENCODED: &str = "｢｣ｧｨｩｪｫｬｭｮｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜｦﾝｰｯ､ﾟﾞ･\u{f8f0}｡";
const FIXUP_DECODED: &str = "「」ぁぃぅぇぉゃゅょあいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをんーっ、？！…　。";

pub fn is_half_kana(c: char) -> bool {
    matches!(c, '\u{FF61}'..='\u{FF9F}')
}

fn fixup(c: char) -> char {
    if !is_half_kana(c) && c != '\u{f8f0}' {
        return c;
    }
    FIXUP_ENCODED.chars().zip(FIXUP_DECODED.chars()).find(|(e, _)| *e == c).map_or(c, |(_, d)| d)
}

/// Splits off a command argument, which runs up to the next dot.
fn argument<'a>(rest: &mut &'a str) -> &'a str {
    let Some(end) = rest.find('.') else { return "" };
    let arg = &rest[..end];
    *rest = &rest[end + 1..];
    arg
}

fn int_argument(rest: &mut &str) -> u32 {
    let arg = argument(rest);
    match arg.strip_prefix('$') {
        Some(hex) => u32::from_str_radix(hex, 16).unwrap_or(0),
        None => arg.parse().unwrap_or(0),
    }
}

/// Interprets the `@` layout commands, keeping only what a reader sees:
/// `@r` is a line break, `@k` a click wait (also shown as a line break) and
/// `@U<code>.` a literal character. Ruby is written `@b<reading>.@<base@>`
/// and only the base text is kept. Everything else (voice, colour, size,
/// timing) is dropped together with its argument.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        if c != '@' {
            out.push(fixup(c));
            continue;
        }
        // a trailing `@` ends the message
        let Some(cmd) = rest.chars().next() else { break };
        rest = &rest[cmd.len_utf8()..];
        match cmd {
            'r' | 'k' => out.push('\n'),
            'U' => out.extend(char::from_u32(int_argument(&mut rest))),
            'b' | 'v' | 'a' | 'c' | 'o' | 's' | 'w' | 'x' | 'z' => {
                argument(&mut rest);
            }
            '+' | '-' | '/' | '<' | '>' | 'e' | 't' | 'y' | '|' | '[' | ']' | '{' | '}' => {}
            other => out.push(fixup(other)),
        }
    }
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn dialogue_block() {
        let raw = "\u{f8f0}金蔵@r@v01/11500003.｢忠告ﾉ気持ﾁだｹﾊｱﾘがﾀｸｲﾀだｲﾃｵｸ｡@k@v01/11500004.我が友ﾖ｡@k@v01/11500005.･･･源次｡";
        assert_eq!(clean(raw), "金蔵\n「忠告の気持ちだけはありがたくいただいておく。\n我が友よ。\n………源次。");
    }

    #[test]
    fn ruby_colour_and_escapes() {
        assert_eq!(clean("@c649.内臓を引き@bず.@<摺@>り出された@c999.@|ﾞ@U$3042."), "内臓を引き摺り出された！あ");
    }
}
