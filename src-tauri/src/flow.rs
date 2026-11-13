use crate::helper_client::{HelperError, Selection};
use crate::reactions::resting;
use crate::ipc::{PopoverPayload, Status, StatusKind};
use crate::popover::Answer;
use std::sync::atomic::{AtomicBool, Ordering};
use ugrammar_core::engine::Tone;
use ugrammar_core::i18n::{fill, Strings};
use ugrammar_core::settings::Settings;
use ugrammar_core::text::{is_js_space_char, js_trim};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixOutcome {
    Paused,
    Busy,
    Skipped,
    NoSelection,
    NeedsPermission,
    Error,
    Unchanged,
    TooShort,
    Dismissed,
    Applied,
    WriteFailed,
    /// "How does it read?" was answered; the text is untouched.
    Reacted,
}

pub trait FlowDeps {
    fn settings(&self) -> Settings;
    async fn read_selection(&self, excluded: &[String]) -> Result<Selection, HelperError>;
    async fn write_selection(&self, text: &str, pid: Option<i64>, method: Option<&str>) -> Result<(), HelperError>;
    /// Puts text on the system clipboard, for the user to paste themselves.
    async fn copy_text(&self, text: &str) -> Result<(), HelperError>;
    /// Engine call with the settings' dictionary and explain options; Err carries the message.
    /// `app` is the bundle id the text came from; it picks that app's writing style.
    async fn check(&self, text: &str, tone: Option<Tone>, app: Option<&str>) -> Result<(String, Vec<String>), String>;
    fn set_status(&self, s: Status);
    fn notify(&self, message: &str);
    /// Resolves when the user picks, or when superseded.
    async fn show_diff(&self, p: PopoverPayload) -> Answer;
    fn show_message(&self, message: &str);
    /// "How does it read?": the engine's take on the text; Err carries the message.
    async fn react(&self, text: &str) -> Result<String, String>;
    /// Shows a reaction until the user closes it.
    fn show_reaction(&self, reaction: &str);
    /// Immediate "Checking…" feedback; replaced by show_diff/show_message or removed by hide_loading.
    fn show_loading(&self);
    fn hide_loading(&self);
    /// Called after text was replaced (accept or quick fix); used for insights.
    fn on_applied(&self, original: &str, corrected: &str, notes: &[String]);
    /// A real action just found Accessibility untrusted; keeps the permissions screen's cache honest.
    fn note_accessibility_not_trusted(&self) {}
}

fn read_error(t: &Strings, e: &HelperError) -> String {
    match e {
        HelperError::ExcludedApp => t.skip_excluded.into(),
        HelperError::SecureField => t.skip_secure.into(),
        HelperError::NoSelection => t.select_first.into(),
        other => fill(t.read_failed, &[("code", &other.to_string())]),
    }
}

fn write_error(t: &Strings, e: &HelperError) -> String {
    match e {
        HelperError::FocusChanged => t.focus_changed.into(),
        other => fill(t.write_failed, &[("code", &other.to_string())]),
    }
}

/// The write failed: keep the fix on the clipboard so the wait is not lost, and say so only if that worked.
async fn write_failed<D: FlowDeps>(d: &D, t: &Strings, e: &HelperError, corrected: &str) -> FixOutcome {
    let msg = write_error(t, e);
    d.notify(&match d.copy_text(corrected).await {
        Ok(()) => format!("{msg} {}", t.fix_on_clipboard),
        Err(_) => msg,
    });
    FixOutcome::WriteFailed
}

struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// One hotkey press: read selection -> engine -> diff popover -> write. Original text is only touched on Accept.
#[derive(Default)]
pub struct FixFlow {
    busy: AtomicBool,
}

impl FixFlow {
    fn idle<D: FlowDeps>(d: &D) {
        d.set_status(resting(&d.settings()));
    }

    /// What every hotkey action starts with: not paused or busy, "Checking…" shown, and a selection of 3+ words.
    async fn start<D: FlowDeps>(&self, d: &D) -> Result<(Busy<'_>, Selection), FixOutcome> {
        let s = d.settings();
        let t = s.strings();
        if s.paused {
            return Err(FixOutcome::Paused);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(FixOutcome::Busy);
        }
        let busy = Busy(&self.busy);
        d.set_status(Status::of(StatusKind::Checking));
        d.show_loading();
        let sel = match d.read_selection(&s.excluded_apps).await {
            Ok(sel) => sel,
            Err(HelperError::AccessibilityNotTrusted) => {
                d.hide_loading();
                d.set_status(Status { kind: StatusKind::NeedsPermission, message: Some(t.grant_ax.into()) });
                d.notify(t.needs_ax);
                d.note_accessibility_not_trusted();
                return Err(FixOutcome::NeedsPermission);
            }
            Err(e) => {
                d.show_message(&read_error(t, &e));
                Self::idle(d);
                return Err(if e == HelperError::NoSelection { FixOutcome::NoSelection } else { FixOutcome::Skipped });
            }
        };
        if js_trim(&sel.text).split(is_js_space_char).filter(|w| !w.is_empty()).count() < 3 {
            d.show_message(t.too_short);
            Self::idle(d);
            return Err(FixOutcome::TooShort);
        }

        Ok((busy, sel))
    }

    fn engine_failed<D: FlowDeps>(d: &D, e: String) -> FixOutcome {
        let t = d.settings().strings();
        let msg = if e.is_empty() { t.engine_error.to_string() } else { e };
        d.hide_loading();
        d.set_status(Status { kind: StatusKind::Error, message: Some(msg.clone()) });
        d.notify(&fill(t.check_failed, &[("msg", &msg)]));
        FixOutcome::Error
    }

    /// "How does it read?" on the selection: shown, never written.
    pub async fn react<D: FlowDeps>(&self, d: &D) -> FixOutcome {
        let (_busy, sel) = match self.start(d).await {
            Ok(x) => x,
            Err(o) => return o,
        };
        match d.react(&sel.text).await {
            Ok(r) => {
                Self::idle(d);
                d.show_reaction(&r);
                FixOutcome::Reacted
            }
            Err(e) => Self::engine_failed(d, e),
        }
    }

    pub async fn run<D: FlowDeps>(&self, d: &D, tone: Option<Tone>) -> FixOutcome {
        let (_busy, sel) = match self.start(d).await {
            Ok(x) => x,
            Err(o) => return o,
        };
        let s = d.settings();
        let t = s.strings();
        let text = sel.text.as_str();
        let (corrected, notes) = match d.check(text, tone, sel.bundle_id.as_deref()).await {
            Ok(r) => r,
            Err(e) => return Self::engine_failed(d, e),
        };

        if corrected == text {
            d.show_message(t.no_errors);
            Self::idle(d);
            return FixOutcome::Unchanged;
        }

        Self::idle(d);
        if s.quick_fix && tone.is_none() {
            if let Err(e) = d.write_selection(&corrected, sel.pid, sel.method.as_deref()).await {
                d.hide_loading();
                return write_failed(d, t, &e, &corrected).await;
            }
            d.on_applied(text, &corrected, &notes);
            d.show_message(t.applied);
            return FixOutcome::Applied;
        }
        let Answer::Accept(written) = d.show_diff(PopoverPayload::diff(text, &corrected, sel.app_name.clone(), notes.clone())).await else {
            return FixOutcome::Dismissed;
        };

        if let Err(e) = d.write_selection(&written, sel.pid, sel.method.as_deref()).await {
            return write_failed(d, t, &e, &written).await;
        }
        // Insights count repeated grammar mistakes; a translation is not one.
        if tone != Some(Tone::Translate) {
            d.on_applied(text, &written, &notes);
        }
        FixOutcome::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use tokio::sync::oneshot;
    use ugrammar_core::i18n::Lang;

    enum Engine {
        Fix,
        Same,
        Fail(&'static str),
    }

    struct Fake {
        paused: bool,
        quick_fix: bool,
        language: Option<Lang>,
        sel: Result<Selection, HelperError>,
        write_ok: Option<&'static str>,
        copy_fails: bool,
        copies: RefCell<Vec<String>>,
        engine: Engine,
        diff: Answer,
        gate: RefCell<Option<oneshot::Receiver<()>>>,
        status: RefCell<Vec<Status>>,
        notes: RefCell<Vec<String>>,
        msgs: RefCell<Vec<String>>,
        writes: RefCell<Vec<String>>,
        applied: RefCell<Vec<String>>,
        excluded: RefCell<Vec<String>>,
        checked_app: RefCell<Option<String>>,
        reactions: RefCell<Vec<String>>,
        engine_calls: Cell<usize>,
        diffs: Cell<usize>,
        loading: Cell<usize>,
        hidden: Cell<usize>,
        ax_lost: Cell<usize>,
    }

    fn sel(text: &str) -> Result<Selection, HelperError> {
        Ok(Selection { text: text.into(), method: Some("ax".into()), pid: Some(7), bundle_id: Some("com.apple.Notes".into()), app_name: Some("Notes".into()) })
    }

    fn sel_err(code: &str) -> Result<Selection, HelperError> {
        Err(code.to_string().into())
    }

    fn fake() -> Fake {
        Fake {
            paused: false,
            quick_fix: false,
            language: None,
            sel: sel("I has a apple."),
            write_ok: None,
            copy_fails: false,
            copies: RefCell::default(),
            engine: Engine::Fix,
            diff: Answer::Accept("I have an apple.".into()),
            gate: RefCell::new(None),
            status: RefCell::default(),
            notes: RefCell::default(),
            msgs: RefCell::default(),
            writes: RefCell::default(),
            applied: RefCell::default(),
            excluded: RefCell::default(),
            checked_app: RefCell::default(),
            reactions: RefCell::default(),
            engine_calls: Cell::new(0),
            diffs: Cell::new(0),
            loading: Cell::new(0),
            hidden: Cell::new(0),
            ax_lost: Cell::new(0),
        }
    }

    impl FlowDeps for Fake {
        fn settings(&self) -> Settings {
            Settings { paused: self.paused, quick_fix: self.quick_fix, dictionary: vec!["Xpiki".into()], language: self.language, ..Settings::default() }
        }
        async fn read_selection(&self, excluded: &[String]) -> Result<Selection, HelperError> {
            *self.excluded.borrow_mut() = excluded.to_vec();
            let gate = self.gate.borrow_mut().take();
            if let Some(g) = gate {
                let _ = g.await;
            }
            self.sel.clone()
        }
        async fn write_selection(&self, text: &str, _pid: Option<i64>, _method: Option<&str>) -> Result<(), HelperError> {
            if let Some(e) = self.write_ok {
                return Err(e.to_string().into());
            }
            self.writes.borrow_mut().push(text.into());
            Ok(())
        }
        async fn copy_text(&self, text: &str) -> Result<(), HelperError> {
            if self.copy_fails {
                return Err("boom".to_string().into());
            }
            self.copies.borrow_mut().push(text.into());
            Ok(())
        }
        async fn check(&self, text: &str, _tone: Option<Tone>, app: Option<&str>) -> Result<(String, Vec<String>), String> {
            *self.checked_app.borrow_mut() = app.map(String::from);
            match self.engine {
                Engine::Fix => {
                    self.engine_calls.set(self.engine_calls.get() + 1);
                    Ok(("I have an apple.".into(), vec![]))
                }
                Engine::Same => Ok((text.into(), vec![])),
                Engine::Fail(m) => Err(m.into()),
            }
        }
        fn set_status(&self, s: Status) {
            self.status.borrow_mut().push(s);
        }
        fn notify(&self, m: &str) {
            self.notes.borrow_mut().push(m.into());
        }
        async fn show_diff(&self, _p: PopoverPayload) -> Answer {
            assert!(!matches!(self.engine, Engine::Same), "should not show");
            self.diffs.set(self.diffs.get() + 1);
            self.diff.clone()
        }
        fn show_message(&self, m: &str) {
            self.msgs.borrow_mut().push(m.into());
        }
        async fn react(&self, _text: &str) -> Result<String, String> {
            self.engine_calls.set(self.engine_calls.get() + 1);
            match self.engine {
                Engine::Fail(m) => Err(m.into()),
                _ => Ok("Blunt\n- Reads as an order.".into()),
            }
        }
        fn show_reaction(&self, r: &str) {
            self.reactions.borrow_mut().push(r.into());
        }
        fn show_loading(&self) {
            self.loading.set(self.loading.get() + 1);
        }
        fn hide_loading(&self) {
            self.hidden.set(self.hidden.get() + 1);
        }
        fn on_applied(&self, _o: &str, c: &str, _n: &[String]) {
            self.applied.borrow_mut().push(c.into());
        }
        fn note_accessibility_not_trusted(&self) {
            self.ax_lost.set(self.ax_lost.get() + 1);
        }
    }

    async fn run(f: &Fake) -> FixOutcome {
        FixFlow::default().run(f, None).await
    }

    #[tokio::test]
    async fn quick_fix_writes_without_showing_the_diff() {
        let f = Fake { quick_fix: true, diff: Answer::Dismiss, ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Applied);
        assert_eq!(f.diffs.get(), 0);
        assert_eq!(*f.writes.borrow(), ["I have an apple."]);
        assert_eq!(*f.applied.borrow(), ["I have an apple."]);
    }

    #[tokio::test]
    async fn tone_always_shows_the_diff_even_with_quick_fix() {
        let f = Fake { quick_fix: true, diff: Answer::Dismiss, ..fake() };
        assert_eq!(FixFlow::default().run(&f, Some(Tone::Formal)).await, FixOutcome::Dismissed);
        assert_eq!(f.diffs.get(), 1);
    }

    #[tokio::test]
    async fn an_accepted_translation_is_written_but_not_recorded_for_insights() {
        let f = fake();
        assert_eq!(FixFlow::default().run(&f, Some(Tone::Translate)).await, FixOutcome::Applied);
        assert_eq!(*f.writes.borrow(), ["I have an apple."]);
        assert!(f.applied.borrow().is_empty());
        let g = fake();
        FixFlow::default().run(&g, Some(Tone::Formal)).await;
        assert_eq!(g.applied.borrow().len(), 1, "other tones still count");
    }

    #[tokio::test]
    async fn accept_writes_corrected_text() {
        let f = fake();
        assert_eq!(run(&f).await, FixOutcome::Applied);
        assert_eq!(*f.writes.borrow(), ["I have an apple."]);
    }

    #[tokio::test]
    async fn accept_writes_and_records_only_the_fixes_left_on() {
        let f = Fake { diff: Answer::Accept("I have a apple.".into()), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Applied);
        assert_eq!(*f.writes.borrow(), ["I have a apple."]);
        assert_eq!(*f.applied.borrow(), ["I have a apple."]);
    }

    #[tokio::test]
    async fn checks_with_the_selected_apps_style() {
        let f = fake();
        run(&f).await;
        assert_eq!(f.checked_app.borrow().as_deref(), Some("com.apple.Notes"));
    }

    #[tokio::test]
    async fn react_shows_the_reaction_and_never_writes() {
        let f = fake();
        assert_eq!(FixFlow::default().react(&f).await, FixOutcome::Reacted);
        assert_eq!(*f.reactions.borrow(), ["Blunt\n- Reads as an order."]);
        assert!(f.writes.borrow().is_empty() && f.applied.borrow().is_empty());
    }

    #[tokio::test]
    async fn react_shares_the_fix_guards_and_errors() {
        let f = Fake { sel: sel("Hi there"), ..fake() };
        assert_eq!(FixFlow::default().react(&f).await, FixOutcome::TooShort);
        let f = Fake { paused: true, ..fake() };
        assert_eq!(FixFlow::default().react(&f).await, FixOutcome::Paused);
        let f = Fake { engine: Engine::Fail("Timed out"), ..fake() };
        assert_eq!(FixFlow::default().react(&f).await, FixOutcome::Error);
        assert_eq!(f.hidden.get(), 1);
        assert!(f.reactions.borrow().is_empty());
    }

    #[tokio::test]
    async fn shows_loading_before_the_engine_call_and_hides_it_on_error() {
        let f = Fake { engine: Engine::Fail("Timed out"), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Error);
        assert_eq!(f.loading.get(), 1);
        assert_eq!(f.hidden.get(), 1);
    }

    #[tokio::test]
    async fn under_3_words_skips_the_engine() {
        let f = Fake { sel: sel("Hi there"), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::TooShort);
        assert_eq!(f.engine_calls.get(), 0);
    }

    #[tokio::test]
    async fn dismiss_never_writes() {
        let f = Fake { diff: Answer::Dismiss, ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Dismissed);
        assert!(f.writes.borrow().is_empty());
    }

    #[tokio::test]
    async fn paused_does_nothing() {
        let f = Fake { paused: true, ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Paused);
        assert_eq!(f.engine_calls.get(), 0);
    }

    #[tokio::test]
    async fn engine_error_no_write_error_status_and_notification() {
        let f = Fake { engine: Engine::Fail("Timed out after 10s"), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Error);
        assert!(f.writes.borrow().is_empty());
        assert_eq!(f.status.borrow().last().unwrap().kind, StatusKind::Error);
        assert!(f.notes.borrow()[0].contains("not changed"));
    }

    #[tokio::test]
    async fn secure_field_skipped_before_engine() {
        let f = Fake { sel: sel_err("secure_field"), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Skipped);
        assert_eq!(f.engine_calls.get(), 0);
        assert_eq!(f.msgs.borrow()[0], "Skipped: password or secure field.");
    }

    #[tokio::test]
    async fn messages_follow_the_picked_language() {
        let f = Fake { sel: sel_err("secure_field"), language: Some(Lang::Vi), ..fake() };
        run(&f).await;
        let f2 = Fake { engine: Engine::Same, language: Some(Lang::Vi), ..fake() };
        run(&f2).await;
        assert_eq!(*f.msgs.borrow(), ["Bỏ qua: ô mật khẩu hoặc ô bảo mật."]);
        assert_eq!(*f2.msgs.borrow(), ["Không có lỗi nào ✓"]);
    }

    #[tokio::test]
    async fn excluded_apps_list_passed_to_helper() {
        let f = Fake { sel: sel_err("excluded_app"), ..fake() };
        run(&f).await;
        assert!(f.excluded.borrow().iter().any(|a| a == "com.apple.Terminal"));
    }

    #[tokio::test]
    async fn needs_permission() {
        let f = Fake { sel: sel_err("accessibility_not_trusted"), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::NeedsPermission);
        assert_eq!(f.status.borrow().last().unwrap().kind, StatusKind::NeedsPermission);
        assert_eq!(f.ax_lost.get(), 1);
    }

    #[tokio::test]
    async fn other_errors_do_not_report_accessibility_lost() {
        let f = Fake { sel: sel_err("secure_field"), ..fake() };
        run(&f).await;
        assert_eq!(f.ax_lost.get(), 0);
    }

    #[tokio::test]
    async fn unchanged_text_shows_message_no_popover_or_write() {
        let f = Fake { engine: Engine::Same, ..fake() };
        assert_eq!(run(&f).await, FixOutcome::Unchanged);
        assert!(f.writes.borrow().is_empty());
    }

    #[tokio::test]
    async fn write_failure_notifies() {
        let f = Fake { write_ok: Some("focus_changed"), ..fake() };
        assert_eq!(run(&f).await, FixOutcome::WriteFailed);
        assert!(f.notes.borrow()[0].contains("Focus changed"));
    }

    #[tokio::test]
    async fn write_failure_copies_the_fix_and_says_so_in_both_paths() {
        for quick_fix in [false, true] {
            let f = Fake { quick_fix, write_ok: Some("focus_changed"), ..fake() };
            assert_eq!(run(&f).await, FixOutcome::WriteFailed);
            assert_eq!(*f.copies.borrow(), ["I have an apple."]);
            let n = f.notes.borrow()[0].clone();
            assert!(n.contains("Focus changed") && n.contains("clipboard"), "{n}");
        }
    }

    #[tokio::test]
    async fn write_failure_with_failed_copy_claims_nothing_about_the_clipboard() {
        for quick_fix in [false, true] {
            let f = Fake { quick_fix, write_ok: Some("focus_changed"), copy_fails: true, ..fake() };
            assert_eq!(run(&f).await, FixOutcome::WriteFailed);
            assert_eq!(*f.notes.borrow(), [write_error(ugrammar_core::i18n::strings(Lang::En), &HelperError::FocusChanged)]);
        }
    }

    #[tokio::test]
    async fn second_press_while_busy_ignored() {
        let (release, gate) = oneshot::channel();
        let f = Fake { sel: sel_err("no_selection"), gate: RefCell::new(Some(gate)), ..fake() };
        let flow = FixFlow::default();
        let first = flow.run(&f, None);
        let second = async {
            let r = flow.run(&f, None).await;
            release.send(()).unwrap();
            r
        };
        let (a, b) = tokio::join!(first, second);
        assert_eq!(b, FixOutcome::Busy);
        assert_eq!(a, FixOutcome::NoSelection);
    }
}
