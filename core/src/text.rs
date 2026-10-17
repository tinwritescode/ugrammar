//! Text offsets across this crate are UTF-16 code units: macOS Accessibility ranges are UTF-16 and the TS original used JS string indices.

pub fn to_u16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Lone surrogates become U+FFFD; offsets computed on the `&[u16]` stay exact.
pub fn from_u16(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// JavaScript's `\s`. Differs from `char::is_whitespace` on U+0085 (not JS space) and U+FEFF (JS space).
pub fn is_js_space(u: u16) -> bool {
    matches!(u, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}

pub fn is_js_space_char(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(is_js_space)
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space_char)
}

/// Regex class equal to JS `\s`; the regex crate's `\s` is Unicode White_Space.
pub(crate) const JS_SPACE: &str = r"[\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_space_matches_javascript() {
        for c in ['\t', '\n', '\u{0B}', '\u{0C}', '\r', ' ', '\u{A0}', '\u{1680}', '\u{2000}', '\u{200A}', '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}', '\u{FEFF}'] {
            assert!(is_js_space_char(c), "{:?} is JS whitespace", c);
        }
        for c in ['\u{85}', '\u{180E}', '\u{200B}', 'a', '😀'] {
            assert!(!is_js_space_char(c), "{:?} is not JS whitespace", c);
        }
        assert_eq!(js_trim("\u{FEFF} hi \u{85}"), "hi \u{85}");
    }

    #[test]
    fn js_space_class_agrees_with_predicate() {
        let re = regex::Regex::new(&format!("^{JS_SPACE}$")).unwrap();
        for u in 0..=0xFFFFu16 {
            if let Some(c) = char::from_u32(u32::from(u)) {
                assert_eq!(re.is_match(c.encode_utf8(&mut [0; 4])), is_js_space(u), "U+{u:04X}");
            }
        }
    }

    #[test]
    fn lengths_are_utf16_units() {
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len("Tiếng Việt"), 10);
        assert_eq!(from_u16(&to_u16("Xin chào 👋")), "Xin chào 👋");
    }
}
