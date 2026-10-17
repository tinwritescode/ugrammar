use std::fs;
use std::io;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::spans::diff_spans;
use crate::text::{js_trim, utf16_len};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Insight {
    pub from: String,
    pub to: String,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

const MAX_ENTRIES: usize = 500;

/// Local-only counters of fixes the user accepted; never leaves the machine. Only short word-level pairs are kept, not sentences.
pub struct InsightStore {
    dir: PathBuf,
    // Insertion-ordered like the JS object it replaces, so ties in top() and eviction resolve the same way.
    data: IndexMap<String, Insight>,
}

impl InsightStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let data = fs::read(dir.join("insights.json"))
            .ok()
            .and_then(|b| serde_json::from_str(&String::from_utf8_lossy(&b)).ok())
            .unwrap_or_default();
        Self { dir, data }
    }

    pub fn record(&mut self, original: &str, corrected: &str, notes: &[String]) {
        for sp in diff_spans(original, corrected) {
            let from = js_trim(&sp.original);
            let to = js_trim(&sp.replacement);
            if from.is_empty() && to.is_empty() {
                continue;
            }
            if utf16_len(from) > 40 || utf16_len(to) > 40 {
                continue;
            }
            let from_lower = from.to_lowercase();
            let note = notes.iter().find(|n| !from.is_empty() && n.to_lowercase().starts_with(&from_lower));
            let cur = self
                .data
                .entry(format!("{from_lower}→{}", to.to_lowercase()))
                .or_insert_with(|| Insight { from: from.into(), to: to.into(), count: 0, note: None });
            cur.count += 1;
            if let Some(n) = note {
                cur.note = Some(n.clone());
            }
        }
        if self.data.len() > MAX_ENTRIES {
            let mut keys: Vec<_> = self.data.iter().map(|(k, v)| (v.count, k.clone())).collect();
            keys.sort_by_key(|(count, _)| *count);
            for (_, k) in keys.into_iter().take(self.data.len() - MAX_ENTRIES) {
                self.data.shift_remove(&k);
            }
        }
        let _ = self.save();
    }

    pub fn top(&self, n: usize) -> Vec<Insight> {
        let mut out: Vec<_> = self.data.values().filter(|i| i.count >= 2).cloned().collect();
        out.sort_by_key(|i| std::cmp::Reverse(i.count));
        out.truncate(n);
        out
    }

    pub fn clear(&mut self) {
        self.data.clear();
        let _ = self.save();
    }

    fn save(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join("insights.json.tmp");
        fs::write(&tmp, serde_json::to_string(&self.data)?)?;
        fs::rename(tmp, self.dir.join("insights.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn counts_repeated_fixes_hides_singles_persists() {
        let dir = TempDir::new();
        let mut s = InsightStore::new(dir.path());
        s.record("I has a cat", "I have a cat", &["has → have: subject-verb agreement".into()]);
        assert_eq!(s.top(10), vec![]);
        s.record("She has two dogs", "She has two dogs", &[]);
        s.record("We has fun", "We have fun", &[]);
        let top = InsightStore::new(dir.path()).top(10);
        assert_eq!(top.len(), 1);
        assert_eq!((top[0].from.as_str(), top[0].to.as_str(), top[0].count), ("has", "have", 2));
        assert!(top[0].note.as_deref().unwrap().contains("agreement"));
    }

    #[test]
    fn loads_and_writes_the_existing_file_format() {
        let dir = TempDir::new();
        let file = dir.path().join("insights.json");
        fs::write(&file, r#"{"has→have":{"from":"has","to":"have","count":3,"note":"has → have: x"},"a→an":{"from":"a","to":"an","count":1}}"#).unwrap();
        let mut s = InsightStore::new(dir.path());
        s.record("a apple", "an apple", &[]);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            r#"{"has→have":{"from":"has","to":"have","count":3,"note":"has → have: x"},"a→an":{"from":"a","to":"an","count":2}}"#
        );
        s.clear();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{}");
    }

    #[test]
    fn length_limit_counts_utf16_units() {
        let dir = TempDir::new();
        let mut s = InsightStore::new(dir.path());
        let (ok, long) = ("😀".repeat(20), "😀".repeat(21)); // 40 and 42 UTF-16 units
        for _ in 0..2 {
            s.record(&format!("x {ok} y"), "x z y", &[]);
            s.record(&format!("x {long} y"), "x w y", &[]);
        }
        let top = s.top(10);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].from, ok);
    }

    #[test]
    fn evicts_lowest_counts_past_max_entries() {
        let dir = TempDir::new();
        let mut s = InsightStore::new(dir.path());
        s.record("keep", "kept", &[]);
        s.record("keep", "kept", &[]);
        for i in 0..MAX_ENTRIES {
            s.record(&format!("w{i}"), &format!("v{i}"), &[]);
        }
        assert_eq!(s.data.len(), MAX_ENTRIES);
        assert!(!s.data.contains_key("w0→v0"));
        assert_eq!(s.top(10)[0].from, "keep");
    }
}
