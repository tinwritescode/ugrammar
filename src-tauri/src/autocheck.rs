use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ugrammar_core::sentence::{sentence_ending_at, SentenceRange, DEFAULT_MIN_WORDS};
use ugrammar_core::spans::diff_spans;
use ugrammar_core::text::{js_trim, to_u16, utf16_len as len16};

pub const POLL_MS: u64 = 250;
const STABLE_MS: u64 = 500;
/// A typed . ? ! is a clear "sentence done" signal: wait only long enough to skip "..." / "?!" mid-burst.
const PUNCT_STABLE_MS: u64 = 200;
const MAX_CACHE: usize = 200;

/// A fix inside some text; offsets are UTF-16 units.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub replacement: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub id: u64,
    pub fix: Fix,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub ok: bool,
    pub error: Option<String>,
    pub reason: Option<serde_json::Value>,
    pub visible: Option<bool>,
    pub text: Option<String>,
    pub caret: Option<usize>,
    pub sel_len: Option<usize>,
    pub pid: Option<i64>,
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub bounds: Option<Vec<Option<Rect>>>,
}

/// A checked sentence with fixes. Offsets are UTF-16, into the field text.
#[derive(Debug, Clone, PartialEq)]
pub struct SentenceFix {
    pub start: usize,
    pub original: String,
    pub corrected: String,
    pub notes: Vec<String>,
    /// Issue the popup anchors to.
    pub first_id: u64,
}

/// One engine request; handed back to `on_check_done` with the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub offset: usize,
    pub sentence: String,
    field: String,
    before: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Show (start, length) / hide (None) a "Checking…" hint at the sentence while the request runs.
    Loading(Option<(usize, usize)>),
    Check(Job),
    /// Fires only while the user has not typed past the sentence.
    Sentence(SentenceFix),
    /// Diagnostics: outcomes only, never text.
    Log(String),
}

fn slice_eq(text: &[u16], start: usize, end: usize, s: &str) -> bool {
    text.get(start..end).is_some_and(|t| t.iter().copied().eq(s.encode_utf16()))
}

fn find_from(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() || from > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// Keep issues whose text is still at (or has shifted near) its old place; drop the rest.
pub fn revalidate(issues: Vec<Issue>, text: &[u16]) -> Vec<Issue> {
    let mut out = Vec::new();
    for mut it in issues {
        if slice_eq(text, it.fix.start, it.fix.end, &it.fix.original) {
            out.push(it);
            continue;
        }
        let needle = to_u16(&it.fix.original);
        let mut best: Option<usize> = None;
        let mut i = find_from(text, &needle, 0);
        while let Some(at) = i {
            if best.is_none_or(|b| at.abs_diff(it.fix.start) < b.abs_diff(it.fix.start)) {
                best = Some(at);
            }
            i = find_from(text, &needle, at + 1);
        }
        if let Some(b) = best.filter(|b| b.abs_diff(it.fix.start) <= 200) {
            it.fix.start = b;
            it.fix.end = b + needle.len();
            out.push(it);
        }
    }
    out
}

/// Watches the focused field; when a sentence just ended and the text settled, checks only that sentence.
/// Pure state: the driver feeds snapshots and engine results in and carries the effects out.
#[derive(Default)]
pub struct AutoChecker {
    checking: bool,
    loading: bool,
    next_id: u64,
    field: String,
    last_text: Vec<u16>,
    logged_text: Vec<u16>,
    changed_at: u64,
    /// Sentence seen ending at the caret; checked even if the user types past it before the text settles.
    pending: Option<SentenceRange>,
    /// Sentence → its fixes (offsets within the sentence); None while in flight or after a failure.
    checked: IndexMap<String, Option<Vec<Fix>>>,
    pub issues: Vec<Issue>,
    pub pid: Option<i64>,
    pub app_name: Option<String>,
}

impl AutoChecker {
    pub fn new() -> Self {
        AutoChecker { next_id: 1, ..Default::default() }
    }

    pub fn reset(&mut self) {
        self.issues.clear();
        self.last_text.clear();
        self.field.clear();
        self.pending = None;
    }

    pub fn issue(&self, id: u64) -> Option<&Issue> {
        self.issues.iter().find(|i| i.id == id)
    }

    pub fn dismiss(&mut self, id: u64) {
        let Some(pos) = self.issues.iter().position(|i| i.id == id) else { return };
        let gone = self.issues.remove(pos).fix;
        // A dismissed fix must not come back from the cache when the sentence is seen again.
        for fixes in self.checked.values_mut().flatten() {
            fixes.retain(|f| f.original != gone.original || f.replacement != gone.replacement);
        }
    }

    /// Popup closed: `corrected` = the sentence was rewritten to it; None = dismissed, its fixes stay hidden.
    pub fn resolve_sentence(&mut self, start: usize, length: usize, corrected: Option<&str>) {
        let inside: Vec<u64> = self.issues.iter().filter(|i| i.fix.start >= start && i.fix.end <= start + length).map(|i| i.id).collect();
        for id in inside {
            self.dismiss(id);
        }
        let Some(corrected) = corrected else { return };
        self.cache_clean(corrected, vec![]); // the accepted text must not be sent back for another pass
        self.last_text.clear();
    }

    fn cache_put(&mut self, sentence: String, fixes: Option<Vec<Fix>>) {
        self.checked.insert(sentence, fixes);
        if self.checked.len() > MAX_CACHE {
            self.checked.shift_remove_index(0);
        }
    }

    fn cache_clean(&mut self, sentence: &str, fixes: Vec<Fix>) {
        self.cache_put(js_trim(sentence).to_string(), Some(fixes)); // sentence_ending_at never includes edge whitespace
    }

    pub fn on_snapshot(&mut self, snap: Snapshot, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        let (true, Some(true), Some(text)) = (snap.ok, snap.visible, snap.text.as_deref()) else {
            // Focus often blips to a button/toolbar for one poll: keep issues, so the field comes back intact.
            out.push(Effect::Log(format!(
                "snap ok={} visible={:?} err={} why={}",
                snap.ok,
                snap.visible,
                snap.error.as_deref().unwrap_or("-"),
                snap.reason.as_ref().map_or("-".into(), |r| r.to_string())
            )));
            return out;
        };
        let text = to_u16(text);
        let caret = snap.caret.unwrap_or(0);
        let field = format!("{:?}:{:?}", snap.pid, snap.bundle_id);
        self.pid = snap.pid;
        self.app_name = snap.app_name.clone();
        if field != self.field {
            self.reset();
            self.field = field.clone();
            self.checked.clear();
        }
        if text != self.last_text {
            self.last_text = text.clone();
            self.changed_at = now;
            self.set_loading(None, &mut out); // typing on: the popup will not open, so drop its hint
            self.issues = revalidate(std::mem::take(&mut self.issues), &text);
            if let Some(s) = sentence_ending_at(&text, caret, DEFAULT_MIN_WORDS) {
                self.pending = Some(s);
            }
        }
        if self.checking || snap.sel_len.unwrap_or(0) > 0 {
            return out;
        }
        let sent = sentence_ending_at(&text, caret, DEFAULT_MIN_WORDS);
        if let Some(p) = self.pending.clone().filter(|p| Some(&p.text) != sent.as_ref().map(|s| &s.text)) {
            // Typed past it: the sentence is done, no need to wait for the text to settle.
            self.pending = None;
            if slice_eq(&text, p.start, p.end, &p.text) && !self.checked.contains_key(&p.text) {
                self.begin(p, snap.bundle_id.as_deref(), field, &mut out);
                return out;
            }
        }
        let punct = sent.as_ref().is_some_and(|s| s.text.ends_with(['.', '?', '!']));
        if now.saturating_sub(self.changed_at) < if punct { PUNCT_STABLE_MS } else { STABLE_MS } {
            return out;
        }
        let Some(sent) = sent else {
            if text != self.logged_text {
                out.push(Effect::Log(format!("no sentence app={:?} len={} caret={caret}", snap.bundle_id, text.len())));
                self.logged_text = text;
            }
            return out;
        };
        if self.checked.contains_key(&sent.text) {
            self.restore(sent.start, &sent.text);
            return out;
        }
        self.begin(sent, snap.bundle_id.as_deref(), field, &mut out);
        out
    }

    fn begin(&mut self, sent: SentenceRange, app: Option<&str>, field: String, out: &mut Vec<Effect>) {
        if self.pending.as_ref().is_some_and(|p| p.text == sent.text) {
            self.pending = None;
        }
        self.cache_put(sent.text.clone(), None);
        out.push(Effect::Log(format!("check start app={app:?} len={}", sent.end - sent.start)));
        self.checking = true;
        self.set_loading(Some((sent.start, sent.end - sent.start)), out);
        out.push(Effect::Check(Job { offset: sent.start, sentence: sent.text, field, before: self.last_text.clone() }));
    }

    /// Same sentence typed again (or retyped elsewhere in the field): re-add its cached issues without a new request.
    fn restore(&mut self, offset: usize, sentence: &str) {
        let Some(Some(fixes)) = self.checked.get(sentence).cloned() else { return };
        if fixes.is_empty() {
            return;
        }
        let end = offset + len16(sentence);
        if self.issues.iter().any(|i| i.fix.start >= offset && i.fix.end <= end) {
            return;
        }
        self.merge(offset, sentence, fixes);
    }

    fn merge(&mut self, offset: usize, sentence: &str, fixes: Vec<Fix>) {
        let end = offset + len16(sentence);
        let mut all: Vec<Issue> = std::mem::take(&mut self.issues).into_iter().filter(|i| i.fix.end <= offset || i.fix.start >= end).collect();
        for f in fixes {
            let id = self.next_id;
            self.next_id += 1;
            all.push(Issue { id, fix: Fix { start: offset + f.start, end: offset + f.end, ..f } });
        }
        self.issues = revalidate(all, &self.last_text);
    }

    /// `result` = the engine's (corrected, notes), or its error message.
    pub fn on_check_done(&mut self, job: Job, result: Result<(String, Vec<String>), String>) -> Vec<Effect> {
        let mut out = Vec::new();
        match result {
            // network/engine errors are silent in the background (sentence stays cached: no retry loop); the manual hotkey still reports them
            Err(e) => out.push(Effect::Log(format!("check error {e}"))),
            Ok((corrected, notes)) => self.apply_result(&job, &corrected, notes, &mut out),
        }
        self.checking = false;
        self.set_loading(None, &mut out);
        out
    }

    fn apply_result(&mut self, job: &Job, corrected: &str, notes: Vec<String>, out: &mut Vec<Effect>) {
        let stale = job.field != self.field;
        out.push(Effect::Log(format!("check done changed={} stale={stale}", corrected != job.sentence)));
        if stale || corrected == job.sentence {
            return;
        }
        let fixes: Vec<Fix> = diff_spans(&job.sentence, corrected)
            .into_iter()
            .filter(|sp| !js_trim(&sp.original).is_empty())
            .enumerate()
            .map(|(i, sp)| {
                let w = js_trim(&sp.original).to_lowercase();
                let reason = notes
                    .iter()
                    .find(|n| n.to_lowercase().starts_with(&w))
                    .or(if notes.len() == 1 { notes.first() } else { notes.get(i) })
                    .cloned();
                Fix { start: sp.start, end: sp.end, original: sp.original, replacement: sp.replacement, reason }
            })
            .collect();
        out.push(Effect::Log(format!("issues={}", fixes.len())));
        if self.checked.contains_key(&job.sentence) {
            self.checked.insert(job.sentence.clone(), Some(fixes.clone()));
        }
        self.merge(job.offset, &job.sentence, fixes);
        let end = job.offset + len16(&job.sentence);
        let first = self.issues.iter().find(|i| i.fix.start >= job.offset && i.fix.end <= end).map(|i| i.id);
        if let (Some(first_id), true) = (first, self.last_text == job.before) {
            self.loading = false; // the popup takes over the hint's place
            out.push(Effect::Sentence(SentenceFix { start: job.offset, original: job.sentence.clone(), corrected: corrected.into(), notes, first_id }));
        }
    }

    fn set_loading(&mut self, at: Option<(usize, usize)>, out: &mut Vec<Effect>) {
        if at.is_none() && !self.loading {
            return;
        }
        self.loading = at.is_some();
        out.push(Effect::Loading(at));
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "She dont like it.";
    const FIXED: &str = "She doesn't like it.";

    fn issue(start: usize, original: &str) -> Issue {
        Issue { id: 1, fix: Fix { start, end: start + len16(original), original: original.into(), replacement: "x".into(), reason: None } }
    }

    #[test]
    fn revalidate_keeps_shifts_drops() {
        assert_eq!(revalidate(vec![issue(2, "has")], &to_u16("I has a cat")), vec![issue(2, "has")]);
        assert_eq!(revalidate(vec![issue(2, "has")], &to_u16("Oh I has a cat"))[0].fix.start, 5);
        assert_eq!(revalidate(vec![issue(2, "has")], &to_u16("I have a cat")), vec![]);
        assert_eq!(revalidate(vec![issue(4, "has")], &to_u16("😀 I has a cat"))[0].fix.start, 5, "UTF-16 offsets");
    }

    struct Rig {
        auto: AutoChecker,
        snap: Snapshot,
        type_during_check: bool,
        clock: u64,
        calls: usize,
        popups: Vec<SentenceFix>,
        loading: Vec<Option<(usize, usize)>>,
    }

    fn snap(text: &str) -> Snapshot {
        Snapshot {
            ok: true,
            visible: Some(true),
            text: Some(text.into()),
            caret: Some(len16(text)),
            sel_len: Some(0),
            pid: Some(1),
            bundle_id: Some("com.tinyspeck.slackmacgap".into()),
            ..Default::default()
        }
    }

    fn rig(type_during_check: bool) -> Rig {
        Rig {
            auto: AutoChecker::new(),
            snap: snap(S),
            type_during_check,
            clock: 0,
            calls: 0,
            popups: vec![],
            loading: vec![],
        }
    }

    impl Rig {
        fn tick(&mut self) {
            let fx = self.auto.on_snapshot(self.snap.clone(), self.clock);
            self.handle(fx);
        }

        fn handle(&mut self, fx: Vec<Effect>) {
            for e in fx {
                match e {
                    Effect::Loading(at) => self.loading.push(at),
                    Effect::Sentence(f) => self.popups.push(f),
                    Effect::Log(_) => {}
                    Effect::Check(job) => {
                        self.calls += 1;
                        if self.type_during_check {
                            self.snap = snap(&format!("{S} Next"));
                            self.tick();
                        }
                        let done = self.auto.on_check_done(job, Ok((FIXED.into(), vec![])));
                        self.handle(done);
                    }
                }
            }
        }

        fn step(&mut self, ms: u64) {
            self.clock += ms;
            self.tick();
        }

        fn steps(&mut self, n: usize) {
            for _ in 0..n {
                self.step(600);
            }
        }

        fn originals(&self) -> Vec<&str> {
            self.auto.issues.iter().map(|i| i.fix.original.as_str()).collect()
        }
    }

    #[test]
    fn sentence_end_issue_found() {
        let mut r = rig(false);
        r.steps(3);
        assert_eq!(r.originals(), ["dont"]);
    }

    #[test]
    fn one_poll_focus_blip_keeps_issues() {
        let mut r = rig(false);
        r.steps(3);
        r.snap.visible = Some(false);
        r.steps(1);
        r.snap.visible = Some(true);
        r.steps(1);
        assert_eq!(r.auto.issues.len(), 1);
    }

    #[test]
    fn retyped_sentence_reuses_cache_dismissed_fix_stays_gone() {
        let mut r = rig(false);
        r.steps(3);
        r.snap = snap("");
        r.steps(2);
        assert!(r.auto.issues.is_empty());
        r.snap = snap(S);
        r.steps(2);
        assert_eq!(r.auto.issues.len(), 1);
        assert_eq!(r.calls, 1);
        let id = r.auto.issues[0].id;
        r.auto.dismiss(id);
        r.steps(2);
        assert!(r.auto.issues.is_empty());
    }

    #[test]
    fn fix_with_reasons_opens_once_check_returns() {
        let mut r = rig(false);
        r.steps(3);
        let got: Vec<_> = r.popups.iter().map(|p| (p.start, p.original.as_str(), p.corrected.as_str())).collect();
        assert_eq!(got, [(0, S, FIXED)]);
        r.auto.resolve_sentence(0, len16(S), None);
        r.steps(1);
        assert!(r.auto.issues.is_empty());
    }

    #[test]
    fn accepted_sentence_is_not_checked_again() {
        let mut r = rig(false);
        r.steps(3);
        r.snap = snap(FIXED);
        r.auto.resolve_sentence(0, len16(S), Some(FIXED));
        r.steps(3);
        assert_eq!(r.calls, 1);
        assert_eq!(r.popups.len(), 1);
    }

    #[test]
    fn typed_past_sentence_keeps_issue_without_popup() {
        let mut r = rig(true);
        r.steps(3);
        assert!(r.popups.is_empty());
        assert_eq!(r.auto.issues.len(), 1);
    }

    #[test]
    fn loading_shows_then_popup_takes_its_place() {
        let mut r = rig(false);
        r.steps(3);
        assert_eq!(r.loading, [Some((0, len16(S)))]);
        assert_eq!(r.popups.len(), 1);
    }

    #[test]
    fn typing_on_hides_loading_and_skips_popup() {
        let mut r = rig(true);
        r.steps(3);
        assert_eq!(r.loading, [Some((0, len16(S))), None]);
    }

    #[test]
    fn typed_period_is_checked_within_one_fast_poll() {
        let mut r = rig(false);
        r.step(250);
        r.step(250);
        assert_eq!(r.calls, 1);
    }

    #[test]
    fn sentence_typed_past_before_it_settled_is_still_checked() {
        let mut r = rig(false);
        r.step(100);
        r.snap = snap(&format!("{S} Next"));
        r.step(100);
        assert_eq!(r.calls, 1);
        assert_eq!(r.originals(), ["dont"]);
    }
}
