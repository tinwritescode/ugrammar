use serde::Serialize;

use crate::diff::{diff_words, DiffKind};
use crate::text::{is_js_space_char, utf16_len};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// Offsets into the original text (UTF-16 units).
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub replacement: String,
}

/// One span per changed region: adjacent delete/insert pairs merge into a replacement.
pub fn diff_spans(original: &str, corrected: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let mut cur: Option<Span> = None;
    let open = |pos| Span { start: pos, end: pos, original: String::new(), replacement: String::new() };
    for op in diff_words(original, corrected) {
        match op.kind {
            DiffKind::Equal => {
                spans.extend(cur.take());
                pos += utf16_len(&op.text);
            }
            DiffKind::Delete => {
                let len = utf16_len(&op.text);
                let c = cur.get_or_insert_with(|| open(pos));
                c.original.push_str(&op.text);
                c.end = pos + len;
                pos += len;
            }
            DiffKind::Insert => cur.get_or_insert_with(|| open(pos)).replacement.push_str(&op.text),
        }
    }
    spans.extend(cur);
    spans.into_iter().map(trim_span).collect()
}

/// Drop shared leading/trailing whitespace so underlines hug the words.
fn trim_span(mut s: Span) -> Span {
    let lead = s.original.len() - s.original.trim_start_matches(is_js_space_char).len();
    if lead > 0 && s.replacement.starts_with(&s.original[..lead]) {
        s.start += utf16_len(&s.original[..lead]);
        s.original.drain(..lead);
        s.replacement.drain(..lead);
    }
    let trail = s.original.len() - s.original.trim_end_matches(is_js_space_char).len();
    let cut = s.original.len() - trail;
    if trail > 0 && s.replacement.ends_with(&s.original[cut..]) {
        s.end -= utf16_len(&s.original[cut..]);
        s.original.truncate(cut);
        s.replacement.truncate(s.replacement.len() - trail);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{from_u16, to_u16};

    fn slice(text: &str, s: &Span) -> String {
        from_u16(&to_u16(text)[s.start..s.end])
    }

    #[test]
    fn replacement_spans_point_into_the_original() {
        let a = "Hi team, I has attached the report. Please to review it before monday.";
        let b = "Hi team, I have attached the report. Please review it before Monday.";
        let spans = diff_spans(a, b);
        let pairs: Vec<_> = spans.iter().map(|s| (s.original.as_str(), s.replacement.as_str())).collect();
        assert_eq!(pairs, [("has", "have"), ("to ", ""), ("monday.", "Monday.")]);
        for s in &spans {
            assert_eq!(slice(a, s), s.original);
        }
    }

    #[test]
    fn identical_text_has_no_spans() {
        assert_eq!(diff_spans("a b c", "a b c"), vec![]);
    }

    #[test]
    fn offsets_are_utf16_units_with_emoji() {
        let a = "😀 I has a cat";
        let s = &diff_spans(a, "😀 I have a cat")[0];
        // JS: "😀 I has a cat".indexOf("has") === 5
        assert_eq!((s.start, s.end), (5, 8));
        assert_eq!(slice(a, s), "has");
    }

    #[test]
    fn offsets_are_utf16_units_with_vietnamese() {
        let a = "Tiếng Việt rất hay, nhưng tôi has lỗi ở đây.";
        let b = "Tiếng Việt rất hay, nhưng tôi have lỗi ở đây!";
        let spans = diff_spans(a, b);
        assert_eq!(spans.len(), 2);
        // JS: a.indexOf("has") === 30
        assert_eq!((spans[0].start, spans[0].end), (30, 33));
        for s in &spans {
            assert_eq!(slice(a, s), s.original);
        }
        assert_eq!((spans[1].start, spans[1].end, spans[1].original.as_str()), (40, 44, "đây."));
    }
}
