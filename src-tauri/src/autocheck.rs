use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use ugrammar_core::sentence::{sentence_ending_at, SentenceRange, DEFAULT_MIN_WORDS};
use ugrammar_core::text::{to_u16, utf16_len as len16};

pub const POLL_MS: u64 = 250;
const STABLE_MS: u64 = 500;
/// A typed . ? ! is a clear "sentence done" signal: wait only long enough to skip "..." / "?!" mid-burst.
const PUNCT_STABLE_MS: u64 = 200;
const MAX_CACHE: usize = 200;
/// Results are reused across apps and fields for this long; kept in RAM only.
const CACHE_TTL_MS: u64 = 10 * 60_000;
/// A sentence whose check failed is sent again only after this long.
const RETRY_MS: u64 = 30_000;
/// Sentences waiting behind the one in flight; the oldest drops first.
const MAX_QUEUE: usize = 3;
/// A fix that came back after the user typed on opens once typing pauses this long.
const IDLE_MS: u64 = 1500;
/// Fixes not acted on stay in the menu-bar list this long.
const INBOX_TTL_MS: u64 = 10 * 60_000;
const MAX_INBOX: usize = 10;

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
    /// Frame of the focused window.
    pub window: Option<Rect>,
}

/// A checked sentence the engine corrected. `start` is UTF-16, into its field's text as last seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: u64,
    field: String,
    pub pid: Option<i64>,
    pub app_name: Option<String>,
    pub start: usize,
    pub sentence: String,
    pub corrected: String,
    pub notes: Vec<String>,
    at: u64,
}

/// One engine request; handed back to `on_check_done` with the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub offset: usize,
    pub sentence: String,
    field: String,
    pid: Option<i64>,
    app_name: Option<String>,
    /// Field text when the sentence was sent, if the caret sat at its end.
    before: Option<Vec<u16>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Check(Job),
    /// Open the popup for this fix.
    Show(Found),
    /// The user started a new sentence: close this popup; its fix already moved to the inbox.
    Hide(u64),
    /// Diagnostics: outcomes only, never text.
    Log(String),
}

/// How a shown or listed fix was settled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Applied,
    Dismissed,
    /// Closed without a decision: it waits in the inbox.
    Later,
}

struct Entry {
    sentence: String,
    /// None = the engine found nothing to fix.
    fix: Option<(String, Vec<String>)>,
    at: u64,
}

/// Cache key: trimmed, inner whitespace collapsed, case kept.
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_from(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() || from > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// Where `s` is now: still at `start`, or the nearest copy within 200 units of it.
pub fn locate(text: &[u16], start: usize, s: &str) -> Option<usize> {
    let needle = to_u16(s);
    if text.get(start..start + needle.len()) == Some(&needle[..]) {
        return Some(start);
    }
    let mut best: Option<usize> = None;
    let mut i = find_from(text, &needle, 0);
    while let Some(at) = i {
        if best.is_none_or(|b| at.abs_diff(start) < b.abs_diff(start)) {
            best = Some(at);
        }
        i = find_from(text, &needle, at + 1);
    }
    best.filter(|b| b.abs_diff(start) <= 200)
}

/// Watches the focused field; when a sentence ends and the text settles, checks only that sentence.
/// Pure state: the driver feeds snapshots and engine results in and carries the effects out.
#[derive(Default)]
pub struct AutoChecker {
    next_id: u64,
    field: String,
    pid: Option<i64>,
    app_name: Option<String>,
    last_text: Vec<u16>,
    caret: usize,
    logged_text: Vec<u16>,
    changed_at: u64,
    /// Sentence seen ending at the caret; checked even if the user types past it before the text settles.
    candidate: Option<SentenceRange>,
    /// Key of the sentence in flight.
    inflight: Option<String>,
    queue: VecDeque<SentenceRange>,
    cache: IndexMap<String, Entry>,
    /// Key → when its check failed.
    failed: HashMap<String, u64>,
    /// Keys dismissed this session; never shown again.
    dismissed: HashSet<String>,
    showing: Option<Found>,
    /// Fixes for this field that came back while the user typed; one opens per typing pause.
    waiting: VecDeque<Found>,
    /// Fixes not acted on, any app: the menu-bar list.
    inbox: Vec<Found>,
}

impl AutoChecker {
    pub fn new() -> Self {
        AutoChecker { next_id: 1, ..Default::default() }
    }

    /// Auto-check stopped: forget the field; the cache and the inbox stay.
    pub fn reset(&mut self) {
        self.leave_field();
        self.field.clear();
    }

    fn leave_field(&mut self) {
        self.last_text.clear();
        self.candidate = None;
        self.queue.clear();
        for f in std::mem::take(&mut self.waiting) {
            self.push_inbox(f);
        }
    }

    /// Engine settings changed: old answers no longer hold.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.failed.clear();
    }

    pub fn busy(&self) -> bool {
        self.inflight.is_some()
    }

    pub fn inbox(&self) -> &[Found] {
        &self.inbox
    }

    pub fn inbox_item(&self, id: u64) -> Option<&Found> {
        self.inbox.iter().find(|f| f.id == id)
    }

    /// Settles the shown popup or an inbox entry.
    pub fn resolve(&mut self, id: u64, outcome: Outcome, now: u64) {
        let (f, shown) = if self.showing.as_ref().is_some_and(|f| f.id == id) {
            (self.showing.take(), true)
        } else {
            (self.inbox.iter().position(|f| f.id == id).map(|i| self.inbox.remove(i)), false)
        };
        let Some(f) = f else { return };
        match outcome {
            // The accepted text must not be sent back for another pass.
            Outcome::Applied => self.cache_put(Entry { sentence: f.corrected, fix: None, at: now }),
            Outcome::Dismissed => {
                self.dismissed.insert(norm(&f.sentence));
            }
            Outcome::Later if shown => self.push_inbox(f),
            Outcome::Later => self.inbox.push(f),
        }
    }

    fn push_inbox(&mut self, f: Found) {
        if self.inbox.iter().any(|g| g.field == f.field && g.sentence == f.sentence) {
            return;
        }
        self.inbox.push(f);
        if self.inbox.len() > MAX_INBOX {
            self.inbox.remove(0);
        }
    }

    fn cache_put(&mut self, e: Entry) {
        self.cache.insert(norm(&e.sentence), e);
        if self.cache.len() > MAX_CACHE {
            self.cache.shift_remove_index(0);
        }
    }

    fn expire(&mut self, now: u64) {
        self.cache.retain(|_, e| now.saturating_sub(e.at) < CACHE_TTL_MS);
        self.failed.retain(|_, at| now.saturating_sub(*at) < RETRY_MS);
        self.inbox.retain(|f| now.saturating_sub(f.at) < INBOX_TTL_MS);
    }

    pub fn on_snapshot(&mut self, snap: Snapshot, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        self.expire(now);
        let (true, Some(true), Some(text)) = (snap.ok, snap.visible, snap.text.as_deref()) else {
            // Focus often blips to a button/toolbar for one poll: keep the field, so it comes back intact.
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
            self.leave_field();
            self.field = field;
        }
        let sent = sentence_ending_at(&text, caret, DEFAULT_MIN_WORDS);
        let changed = text != self.last_text;
        self.caret = caret;
        if changed {
            self.last_text = text.clone();
            self.changed_at = now;
            let field = &self.field;
            self.inbox.retain_mut(|f| {
                if f.field != *field {
                    return true;
                }
                locate(&text, f.start, &f.sentence).map(|s| f.start = s).is_some()
            });
            self.hide_if_moved_on(&text, caret, &mut out);
        }
        if let Some(p) = self.candidate.take() {
            if sent.as_ref().map(|s| &s.text) == Some(&p.text) {
                self.candidate = Some(p);
            } else if locate(&text, p.start, &p.text) == Some(p.start) {
                // Typed past it: the sentence is done, no need to wait for the text to settle.
                self.consider(p, false, &mut out);
            }
        }
        if changed && sent.is_some() {
            self.candidate = sent.clone();
        }
        if snap.sel_len.unwrap_or(0) > 0 {
            return out;
        }
        let punct = sent.as_ref().is_some_and(|s| s.text.ends_with(['.', '?', '!']));
        if now.saturating_sub(self.changed_at) >= if punct { PUNCT_STABLE_MS } else { STABLE_MS } {
            match sent {
                Some(s) => self.consider(s, true, &mut out),
                None if text != self.logged_text => {
                    out.push(Effect::Log(format!("no sentence app={:?} len={} caret={caret}", snap.bundle_id, text.len())));
                    self.logged_text = text;
                }
                None => {}
            }
        }
        self.next_job(&mut out);
        if now.saturating_sub(self.changed_at) >= IDLE_MS {
            self.show_waiting(&mut out);
        }
        out
    }

    /// Text typed after the shown sentence means the user moved on.
    fn hide_if_moved_on(&mut self, text: &[u16], caret: usize, out: &mut Vec<Effect>) {
        let Some(f) = self.showing.as_ref().filter(|f| f.field == self.field) else { return };
        let Some(start) = locate(text, f.start, &f.sentence) else { return };
        let end = start + len16(&f.sentence);
        if caret > end && text[end..caret].iter().any(|&u| !char::from_u32(u as u32).is_some_and(char::is_whitespace)) {
            let f = self.showing.take().unwrap();
            out.push(Effect::Hide(f.id));
            self.push_inbox(f);
        }
    }

    /// A finished sentence: reuse its cached answer, or queue it for the engine.
    fn consider(&mut self, s: SentenceRange, at_caret: bool, out: &mut Vec<Effect>) {
        let key = norm(&s.text);
        if self.dismissed.contains(&key) || self.inflight.as_ref() == Some(&key) || self.queue.iter().any(|q| norm(&q.text) == key) {
            return;
        }
        if let Some(e) = self.cache.get(&key) {
            // Fix offsets only hold for the exact text checked.
            if let (Some((corrected, notes)), true) = (e.fix.clone(), e.sentence == s.text) {
                let f = self.found(s.start, s.text, corrected, notes, self.field.clone(), self.pid, self.app_name.clone(), e.at);
                self.deliver(f, at_caret, out);
            }
            return;
        }
        if self.failed.contains_key(&key) {
            return;
        }
        self.queue.push_back(s);
        if self.queue.len() > MAX_QUEUE {
            self.queue.pop_front();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn found(&mut self, start: usize, sentence: String, corrected: String, notes: Vec<String>, field: String, pid: Option<i64>, app_name: Option<String>, at: u64) -> Found {
        let id = self.next_id;
        self.next_id += 1;
        Found { id, field, pid, app_name, start, sentence, corrected, notes, at }
    }

    /// Opens it now if the user is still at it, else after the next typing pause; another field's goes to the inbox.
    fn deliver(&mut self, f: Found, now: bool, out: &mut Vec<Effect>) {
        let dup = |g: &Found| g.field == f.field && g.sentence == f.sentence;
        if self.showing.iter().chain(&self.waiting).chain(&self.inbox).any(dup) {
            return;
        }
        if f.field != self.field {
            self.push_inbox(f);
        } else if now && self.showing.is_none() {
            self.showing = Some(f.clone());
            out.push(Effect::Show(f));
        } else {
            self.waiting.push_back(f);
        }
    }

    fn show_waiting(&mut self, out: &mut Vec<Effect>) {
        while self.showing.is_none() {
            let Some(mut f) = self.waiting.pop_front() else { return };
            if let Some(s) = locate(&self.last_text, f.start, &f.sentence) {
                f.start = s;
                self.showing = Some(f.clone());
                out.push(Effect::Show(f));
            }
        }
    }

    fn next_job(&mut self, out: &mut Vec<Effect>) {
        while self.inflight.is_none() {
            let Some(s) = self.queue.pop_front() else { return };
            let Some(offset) = locate(&self.last_text, s.start, &s.text) else { continue };
            out.push(Effect::Log(format!("check start app={:?} len={}", self.app_name, s.end - s.start)));
            let at_caret = sentence_ending_at(&self.last_text, self.caret, DEFAULT_MIN_WORDS).is_some_and(|c| c.start == offset && c.text == s.text);
            self.inflight = Some(norm(&s.text));
            out.push(Effect::Check(Job {
                offset,
                sentence: s.text,
                field: self.field.clone(),
                pid: self.pid,
                app_name: self.app_name.clone(),
                before: at_caret.then(|| self.last_text.clone()),
            }));
        }
    }

    /// `result` = the engine's (corrected, notes), or its error message.
    pub fn on_check_done(&mut self, job: Job, result: Result<(String, Vec<String>), String>, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        self.inflight = None;
        match result {
            // Errors are silent in the background and never cached; the manual hotkey still reports them.
            Err(e) => {
                out.push(Effect::Log(format!("check error {e}")));
                self.failed.insert(norm(&job.sentence), now);
            }
            Ok((corrected, notes)) => {
                let changed = corrected != job.sentence;
                out.push(Effect::Log(format!("check done changed={changed} stale={}", job.field != self.field)));
                let fix = changed.then(|| (corrected.clone(), notes.clone()));
                self.cache_put(Entry { sentence: job.sentence.clone(), fix, at: now });
                let here = job.field == self.field;
                let start = if here { locate(&self.last_text, job.offset, &job.sentence) } else { Some(job.offset) };
                if let (true, Some(start)) = (changed, start) {
                    let at_it = here && job.before.as_ref() == Some(&self.last_text);
                    let f = self.found(start, job.sentence, corrected, notes, job.field, job.pid, job.app_name, now);
                    self.deliver(f, at_it, &mut out);
                }
            }
        }
        self.next_job(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "She dont like it.";
    const FIXED: &str = "She doesn't like it.";
    const MIN: u64 = 60_000;

    fn fix(s: &str) -> String {
        s.replace("dont", "doesn't")
    }

    #[test]
    fn locate_keeps_shifts_drops() {
        assert_eq!(locate(&to_u16("I has a cat"), 2, "has"), Some(2));
        assert_eq!(locate(&to_u16("Oh I has a cat"), 2, "has"), Some(5));
        assert_eq!(locate(&to_u16("I have a cat"), 2, "has"), None);
        assert_eq!(locate(&to_u16("😀 I has a cat"), 4, "has"), Some(5), "UTF-16 offsets");
    }

    struct Rig {
        auto: AutoChecker,
        snap: Snapshot,
        /// Engine answers at once; else jobs wait for `finish`.
        instant: bool,
        fail: bool,
        clock: u64,
        calls: Vec<String>,
        jobs: VecDeque<Job>,
        shown: Vec<Found>,
        hidden: Vec<u64>,
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
            app_name: Some("Slack".into()),
            ..Default::default()
        }
    }

    fn rig(instant: bool) -> Rig {
        Rig { auto: AutoChecker::new(), snap: snap(S), instant, fail: false, clock: 0, calls: vec![], jobs: VecDeque::new(), shown: vec![], hidden: vec![] }
    }

    impl Rig {
        fn tick(&mut self) {
            let fx = self.auto.on_snapshot(self.snap.clone(), self.clock);
            self.handle(fx);
        }

        fn handle(&mut self, fx: Vec<Effect>) {
            for e in fx {
                match e {
                    Effect::Show(f) => self.shown.push(f),
                    Effect::Hide(id) => self.hidden.push(id),
                    Effect::Log(_) => {}
                    Effect::Check(job) => {
                        self.calls.push(job.sentence.clone());
                        self.jobs.push_back(job);
                        if self.instant {
                            self.finish();
                        }
                    }
                }
            }
        }

        fn finish(&mut self) {
            let job = self.jobs.pop_front().expect("a job in flight");
            let r = if self.fail { Err("timeout".into()) } else { Ok((fix(&job.sentence), vec![])) };
            let fx = self.auto.on_check_done(job, r, self.clock);
            self.handle(fx);
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

        fn type_(&mut self, text: &str) {
            self.snap = snap(text);
            self.step(100);
        }

        fn last(&self) -> u64 {
            self.shown.last().expect("a popup").id
        }
    }

    #[test]
    fn sentence_end_opens_popup_with_fix() {
        let mut r = rig(true);
        r.steps(3);
        let got: Vec<_> = r.shown.iter().map(|f| (f.start, f.sentence.as_str(), f.corrected.as_str(), f.app_name.as_deref())).collect();
        assert_eq!(got, [(0, S, FIXED, Some("Slack"))]);
        assert_eq!(r.calls.len(), 1);
    }

    #[test]
    fn typed_period_is_checked_within_one_fast_poll() {
        let mut r = rig(true);
        r.step(250);
        r.step(250);
        assert_eq!(r.calls.len(), 1);
    }

    #[test]
    fn sentence_typed_past_before_it_settled_is_still_checked() {
        let mut r = rig(true);
        r.step(100);
        r.type_(&format!("{S} Next"));
        assert_eq!(r.calls, [S]);
    }

    #[test]
    fn busy_while_in_flight() {
        let mut r = rig(false);
        r.steps(2);
        assert!(r.auto.busy());
        r.finish();
        assert!(!r.auto.busy());
    }

    #[test]
    fn sentences_typed_during_a_check_queue_up_to_three() {
        let mut r = rig(false);
        let ss = ["She dont like it.", "He dont care much.", "We dont go there.", "They dont swim well.", "You dont run fast."];
        let mut text = String::new();
        for s in ss {
            text = if text.is_empty() { s.into() } else { format!("{text} {s}") };
            r.type_(&text);
            r.steps(1);
        }
        assert_eq!(r.calls, [ss[0]]);
        while !r.jobs.is_empty() {
            r.finish();
        }
        assert_eq!(r.calls, [ss[0], ss[2], ss[3], ss[4]], "oldest queued sentence drops");
    }

    #[test]
    fn cache_is_shared_across_fields() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Later, r.clock);
        r.snap = Snapshot { pid: Some(2), bundle_id: Some("com.apple.mail".into()), ..snap(S) };
        r.steps(3);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.shown.len(), 2, "cached fix opens in the other field");
    }

    #[test]
    fn cache_key_ignores_extra_whitespace() {
        let mut r = rig(true);
        r.snap = snap("She dont  like it.");
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Later, r.clock);
        r.type_("");
        r.type_(S);
        r.steps(3);
        assert_eq!(r.calls.len(), 1);
    }

    #[test]
    fn cache_expires_after_ten_minutes() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Applied, r.clock);
        r.type_("");
        r.step(11 * MIN);
        r.type_(S);
        r.steps(3);
        assert_eq!(r.calls.len(), 2);
    }

    #[test]
    fn failed_check_is_not_cached_and_retries_after_30s() {
        let mut r = rig(true);
        r.fail = true;
        r.steps(3);
        assert_eq!(r.calls.len(), 1);
        assert!(r.shown.is_empty());
        r.steps(40); // 24 s
        assert_eq!(r.calls.len(), 1);
        r.fail = false;
        r.steps(15);
        assert_eq!(r.calls.len(), 2);
        assert_eq!(r.shown.len(), 1);
    }

    #[test]
    fn clear_cache_checks_again() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Applied, r.clock);
        r.auto.clear_cache();
        r.type_("");
        r.type_(S);
        r.steps(3);
        assert_eq!(r.calls.len(), 2);
    }

    #[test]
    fn dismissed_sentence_never_returns() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Dismissed, r.clock);
        r.type_("");
        r.step(11 * MIN);
        r.type_(S);
        r.steps(3);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.shown.len(), 1);
    }

    #[test]
    fn accepted_sentence_is_not_checked_again() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Applied, r.clock);
        r.snap = snap(FIXED);
        r.steps(3);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.shown.len(), 1);
    }

    #[test]
    fn result_after_typing_on_opens_once_typing_pauses() {
        let mut r = rig(false);
        r.steps(1);
        r.type_(&format!("{S} Next"));
        r.finish();
        assert!(r.shown.is_empty());
        r.step(1000);
        assert!(r.shown.is_empty());
        r.step(600);
        assert_eq!(r.shown.len(), 1);
        assert_eq!(r.shown[0].sentence, S);
    }

    #[test]
    fn waiting_fix_moves_to_inbox_when_field_changes() {
        let mut r = rig(false);
        r.steps(1);
        r.type_(&format!("{S} Next"));
        r.finish();
        r.snap = Snapshot { pid: Some(2), bundle_id: Some("com.apple.mail".into()), ..snap("") };
        r.steps(5);
        assert!(r.shown.is_empty());
        assert_eq!(r.auto.inbox().len(), 1);
    }

    #[test]
    fn waiting_fixes_open_oldest_first_one_at_a_time() {
        let mut r = rig(false);
        let (a, b) = ("She dont like it.", "He dont care much.");
        r.steps(1);
        r.type_(&format!("{a} {b}"));
        r.steps(1);
        r.type_(&format!("{a} {b} Next"));
        r.finish();
        r.finish();
        r.steps(3);
        assert_eq!(r.shown.iter().map(|f| f.sentence.as_str()).collect::<Vec<_>>(), [a]);
        r.auto.resolve(r.last(), Outcome::Dismissed, r.clock);
        r.steps(1);
        assert_eq!(r.shown.iter().map(|f| f.sentence.as_str()).collect::<Vec<_>>(), [a, b]);
    }

    #[test]
    fn starting_a_new_sentence_hides_popup_into_inbox() {
        let mut r = rig(true);
        r.steps(3);
        let id = r.last();
        r.type_(&format!("{S} "));
        assert!(r.hidden.is_empty(), "a space is not a new sentence yet");
        r.type_(&format!("{S} He"));
        assert_eq!(r.hidden, [id]);
        assert_eq!(r.auto.inbox().iter().map(|f| f.id).collect::<Vec<_>>(), [id]);
        r.auto.resolve(id, Outcome::Later, r.clock); // the driver's close is a no-op now
        assert_eq!(r.auto.inbox().len(), 1);
    }

    #[test]
    fn inbox_entry_follows_text_and_drops_when_sentence_is_gone() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Later, r.clock);
        r.type_(&format!("Hi. {S}"));
        assert_eq!(r.auto.inbox()[0].start, 4);
        r.type_("Hi.");
        assert!(r.auto.inbox().is_empty());
    }

    #[test]
    fn inbox_entry_expires_after_ten_minutes() {
        let mut r = rig(true);
        r.steps(3);
        r.auto.resolve(r.last(), Outcome::Later, r.clock);
        r.snap = Snapshot { pid: Some(2), bundle_id: Some("com.apple.mail".into()), ..snap("") };
        r.step(9 * MIN);
        assert_eq!(r.auto.inbox().len(), 1);
        r.step(2 * MIN);
        assert!(r.auto.inbox().is_empty());
    }

    #[test]
    fn inbox_keeps_ten_newest() {
        let mut r = rig(true);
        for i in 0..12 {
            r.snap = Snapshot { pid: Some(10 + i), ..snap(S) };
            r.steps(3);
            r.auto.resolve(r.last(), Outcome::Later, r.clock);
        }
        assert_eq!(r.auto.inbox().len(), 10);
        assert_eq!(r.auto.inbox()[0].pid, Some(12));
    }

    #[test]
    fn applied_inbox_entry_leaves_it() {
        let mut r = rig(true);
        r.steps(3);
        let id = r.last();
        r.auto.resolve(id, Outcome::Later, r.clock);
        r.auto.resolve(id, Outcome::Applied, r.clock);
        assert!(r.auto.inbox().is_empty());
    }
}
