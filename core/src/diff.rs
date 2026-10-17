use serde::Serialize;

use crate::text::is_js_space_char;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffKind {
    Equal,
    Delete,
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffOp {
    #[serde(rename = "type")]
    pub kind: DiffKind,
    pub text: String,
}

// Tokens: runs of whitespace, or runs of non-whitespace. Whitespace is kept so ops reconstruct both strings exactly.
fn tokenize(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev = None;
    for (i, c) in s.char_indices() {
        let space = is_js_space_char(c);
        if prev.is_some_and(|p| p != space) {
            out.push(&s[start..i]);
            start = i;
        }
        prev = Some(space);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

pub fn diff_words(a: &str, b: &str) -> Vec<DiffOp> {
    let (x, y) = (tokenize(a), tokenize(b));
    let (n, m) = (x.len(), y.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if x[i] == y[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }

    let mut ops: Vec<DiffOp> = Vec::new();
    let mut push = |kind, text: &str| match ops.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(text),
        _ => ops.push(DiffOp { kind, text: text.to_string() }),
    };
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if x[i] == y[j] {
            push(DiffKind::Equal, x[i]);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            push(DiffKind::Delete, x[i]);
            i += 1;
        } else {
            push(DiffKind::Insert, y[j]);
            j += 1;
        }
    }
    for t in &x[i..] {
        push(DiffKind::Delete, t);
    }
    for t in &y[j..] {
        push(DiffKind::Insert, t);
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuild(ops: &[DiffOp], skip: DiffKind) -> String {
        ops.iter().filter(|o| o.kind != skip).map(|o| o.text.as_str()).collect()
    }

    #[test]
    fn identical_single_equal_op() {
        assert_eq!(diff_words("a b", "a b"), vec![DiffOp { kind: DiffKind::Equal, text: "a b".into() }]);
    }

    #[test]
    fn replace_word() {
        let d = diff_words("I has a apple.", "I have an apple.");
        let texts = |k| d.iter().filter(|o| o.kind == k).map(|o| o.text.trim()).collect::<Vec<_>>();
        assert_eq!(texts(DiffKind::Delete), ["has", "a"]);
        assert_eq!(texts(DiffKind::Insert), ["have", "an"]);
        assert_eq!(rebuild(&d, DiffKind::Insert), "I has a apple.");
        assert_eq!(rebuild(&d, DiffKind::Delete), "I have an apple.");
    }

    #[test]
    fn preserves_whitespace_and_newlines_on_reconstruct() {
        let (a, b) = ("Hi  there,\nhow r you?", "Hi  there,\nhow are you?");
        let d = diff_words(a, b);
        assert_eq!(rebuild(&d, DiffKind::Insert), a);
        assert_eq!(rebuild(&d, DiffKind::Delete), b);
    }

    #[test]
    fn js_whitespace_splits_tokens() {
        assert_eq!(tokenize("a\u{FEFF}b\u{85}c"), ["a", "\u{FEFF}", "b\u{85}c"]);
    }
}
