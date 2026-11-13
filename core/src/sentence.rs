use serde::Serialize;

use crate::text::{from_u16, is_js_space};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentenceRange {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

pub const DEFAULT_MIN_WORDS: usize = 3;

const NEWLINE: u16 = b'\n' as u16;

pub const TERMINATORS: [char; 3] = ['.', '?', '!'];

fn is_terminator(u: u16) -> bool {
    char::from_u32(u as u32).is_some_and(|c| TERMINATORS.contains(&c))
}

/// `s.trim().split(/\s+/).length`
fn word_count(s: &[u16]) -> usize {
    let a = s.iter().position(|&u| !is_js_space(u)).unwrap_or(s.len());
    let b = s.iter().rposition(|&u| !is_js_space(u)).map_or(a, |i| i + 1);
    1 + s[a..b].windows(2).filter(|w| !is_js_space(w[0]) && is_js_space(w[1])).count()
}

/// Indices are UTF-16 units, matching Accessibility ranges.
pub fn sentence_ending_at(text: &[u16], caret: usize, min_words: usize) -> Option<SentenceRange> {
    sentence_before(text, caret, min_words, true)
}

/// The sentence the caret ends, finished or still being written; an unfinished one only when
/// nothing but whitespace follows the caret on its line (no word in progress, not editing mid-line).
pub fn sentence_at_caret(text: &[u16], caret: usize, min_words: usize) -> Option<SentenceRange> {
    sentence_before(text, caret, min_words, false)
}

fn sentence_before(text: &[u16], caret: usize, min_words: usize, finished_only: bool) -> Option<SentenceRange> {
    if caret == 0 || caret > text.len() {
        return None;
    }
    // Terminator is the last non-space char before the caret, or a newline was just typed.
    let mut i = caret;
    let mut saw_newline = false;
    while i > 0 && matches!(text[i - 1], 0x20 | 0x09 | 0x0D | NEWLINE) {
        saw_newline |= text[i - 1] == NEWLINE;
        i -= 1;
    }
    if i == 0 {
        return None;
    }
    if !is_terminator(text[i - 1]) && !saw_newline {
        let line_rest_blank = text[caret..].iter().take_while(|&&u| u != NEWLINE).all(|&u| is_js_space(u));
        if finished_only || !line_rest_blank {
            return None;
        }
    }
    let end = i;
    // Walk back to the previous terminator followed by whitespace (or text start / newline).
    let mut start = (0..end - 1)
        .rev()
        .find(|&k| text[k] == NEWLINE || (is_terminator(text[k]) && is_js_space(text[k + 1])))
        .map_or(0, |k| k + 1);
    while start < end && is_js_space(text[start]) {
        start += 1;
    }
    let sentence = &text[start..end];
    if word_count(sentence) < min_words {
        return None;
    }
    Some(SentenceRange { start, end, text: from_u16(sentence) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::to_u16;

    fn at(t: &str, caret: usize) -> Option<SentenceRange> {
        sentence_ending_at(&to_u16(t), caret, DEFAULT_MIN_WORDS)
    }

    #[test]
    fn detects_a_just_finished_sentence() {
        let t = to_u16("Hello there. I has a cat.");
        let r = sentence_ending_at(&t, t.len(), DEFAULT_MIN_WORDS).unwrap();
        assert_eq!(r.text, "I has a cat.");
        assert_eq!(from_u16(&t[r.start..r.end]), r.text);
    }

    #[test]
    fn allows_trailing_space_and_newline() {
        assert_eq!(at("I has a cat. ", 13).unwrap().text, "I has a cat.");
        assert_eq!(at("I has a cat\n", 12).unwrap().text, "I has a cat");
    }

    #[test]
    fn no_trigger_mid_sentence_or_when_too_short() {
        assert_eq!(at("I has a cat", 11), None);
        assert_eq!(at("Hi there.", 9), None);
    }

    #[test]
    fn does_not_split_on_decimals() {
        let t = "The price is 3.5 dollars today.";
        assert_eq!(at(t, t.encode_utf16().count()).unwrap().text, t);
    }

    #[test]
    fn caret_out_of_range() {
        assert_eq!(at("I has a cat.", 0), None);
        assert_eq!(at("I has a cat.", 13), None);
    }

    fn open(t: &str, caret: usize) -> Option<String> {
        sentence_at_caret(&to_u16(t), caret, DEFAULT_MIN_WORDS).map(|r| r.text)
    }

    #[test]
    fn at_caret_takes_an_unfinished_sentence() {
        assert_eq!(open("Hello there. She dont like it", 29).as_deref(), Some("She dont like it"));
        assert_eq!(open("She dont like it ", 17).as_deref(), Some("She dont like it"));
        assert_eq!(open("She dont like\n\nBest, Tin", 13).as_deref(), Some("She dont like"), "text on later lines");
    }

    #[test]
    fn at_caret_skips_unfinished_text_with_words_after_the_caret() {
        assert_eq!(open("She dont like it", 10), None, "caret mid-word");
        assert_eq!(open("She dont like it", 8), None, "caret mid-line");
        assert_eq!(open("Hi there", 8), None, "too short");
    }

    #[test]
    fn at_caret_still_takes_finished_sentences() {
        assert_eq!(open("I has a cat. Then", 12).as_deref(), Some("I has a cat."));
    }

    #[test]
    fn offsets_are_utf16_units() {
        let t = to_u16("Xin chào 👋. Tôi có một con mèo.");
        let r = sentence_ending_at(&t, t.len(), DEFAULT_MIN_WORDS).unwrap();
        // JS: "Xin chào 👋. ".length === 13
        assert_eq!((r.start, r.end), (13, t.len()));
        assert_eq!(t.len(), 32);
        assert_eq!(r.text, "Tôi có một con mèo.");
    }
}
