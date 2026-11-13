//! The one popover: what it shows, who waits on its answer, which auto-check fix holds it, and where it sits.
//! Pure state; `Ctx` does the window, shortcut and timer work around it.

use crate::ipc::{Choice, PopoverPayload};
use crate::placement::Anchor;
use crate::placement::Rect;
use std::collections::BTreeSet;
use tokio::sync::oneshot;
use ugrammar_core::diff::apply_fixes;

pub const POPOVER_W: f64 = 460.0;
const MAX_H: f64 = 600.0;
/// Height to measure a fresh popover at, before the page reports its own.
const FIRST_H: f64 = 200.0;

/// Names one showing of the popover; stale once it closes or another opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket(u64);

/// What a diff popover's waiter gets back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The text to write: the original with the fixes the user left on.
    Accept(String),
    Dismiss,
    /// Closed without a decision (timed out, or another popover took its place).
    Later,
}

/// What closing left to do: answer whoever waited on the closed popover.
#[must_use]
pub struct Closed {
    waiter: Option<oneshot::Sender<Answer>>,
    /// What Accept writes; None when no fix is left on.
    accepted: Option<String>,
}

impl Closed {
    pub fn answer(self, choice: Choice) {
        let answer = match (choice, self.accepted) {
            (Choice::Accept, Some(text)) => Answer::Accept(text),
            (Choice::Accept | Choice::Dismiss, _) => Answer::Dismiss,
            (Choice::Later, _) => Answer::Later,
        };
        if let Some(tx) = self.waiter {
            let _ = tx.send(answer);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    /// That popover is gone already.
    Stale,
    /// The mouse is over it: wait again.
    Keep,
    Close,
}

#[derive(Default)]
pub struct Session {
    generation: u64,
    resolve: Option<oneshot::Sender<Answer>>,
    /// Fixes of the showing diff the user turned off, numbered as `apply_fixes` counts them.
    skipped: BTreeSet<usize>,
    /// Fix id of the auto-check popup, from the claim until its answer is handled.
    auto: Option<u64>,
    payload: Option<PopoverPayload>,
    anchor: Option<Anchor>,
    size: (f64, f64),
    /// Where it was last placed.
    rect: Option<Rect>,
}

impl Session {
    pub fn close(&mut self) -> Closed {
        self.generation += 1;
        let skipped = std::mem::take(&mut self.skipped);
        let accepted = self.payload.take().and_then(|p| {
            let text = apply_fixes(p.ops.as_deref()?, &skipped);
            (p.original.as_deref() != Some(text.as_str())).then_some(text)
        });
        Closed { waiter: self.resolve.take(), accepted }
    }

    /// The diff's fixes the user turned off; the page sends the whole list on every toggle.
    pub fn set_skipped(&mut self, fixes: Vec<usize>) {
        self.skipped = fixes.into_iter().collect();
    }

    /// Replaces whatever is open; the payload is measured at full width until the page says otherwise. `waiter` gets the
    /// user's choice, or Later when replaced or closed for them.
    pub fn open(&mut self, payload: PopoverPayload, anchor: Anchor, waiter: Option<oneshot::Sender<Answer>>) -> (Ticket, Closed) {
        let closed = self.close();
        self.resolve = waiter;
        self.payload = Some(payload);
        self.anchor = Some(anchor);
        self.size = (POPOVER_W, if self.size.1 == 0.0 { FIRST_H } else { self.size.1 });
        (Ticket(self.generation), closed)
    }

    pub fn is_current(&self, t: Ticket) -> bool {
        t.0 == self.generation && self.payload.is_some()
    }

    pub fn payload(&self) -> Option<PopoverPayload> {
        self.payload.clone()
    }

    /// For an auto-check fix: only when nothing is showing and no other fix holds the popover.
    pub fn claim_auto(&mut self, id: u64) -> bool {
        if self.payload.is_some() || self.auto.is_some() {
            return false;
        }
        self.auto = Some(id);
        true
    }

    pub fn holds_auto(&self, id: u64) -> bool {
        self.auto == Some(id)
    }

    /// True when `id` held it.
    pub fn release_auto(&mut self, id: u64) -> bool {
        self.auto.take_if(|a| *a == id).is_some()
    }

    /// The size the page asked for, capped; None for a bogus height.
    pub fn resize(&mut self, h: f64, w: Option<f64>) -> Option<(f64, f64)> {
        if h <= 0.0 {
            return None;
        }
        self.size = (w.map_or(POPOVER_W, |w| w.ceil().min(POPOVER_W)), h.ceil().min(MAX_H));
        Some(self.size)
    }

    pub fn size(&self) -> (f64, f64) {
        self.size
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor
    }

    pub fn placed(&mut self, rect: Rect) {
        self.rect = Some(rect);
    }

    /// An untouched auto-check popup closes once its wait runs out, unless the mouse is over it.
    pub fn expiry(&self, t: Ticket, mouse: (f64, f64)) -> Expiry {
        if !self.is_current(t) {
            Expiry::Stale
        } else if self.rect.is_some_and(|r| r.contains(mouse.0, mouse.1)) {
            Expiry::Keep
        } else {
            Expiry::Close
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::PopoverKind;

    fn msg() -> PopoverPayload {
        PopoverPayload::text(PopoverKind::Message, "hi")
    }

    const AT: Anchor = Anchor::Corner { x: 0.0, y: 0.0 };

    fn waiting(s: &mut Session) -> (Ticket, oneshot::Receiver<Answer>) {
        let (tx, rx) = oneshot::channel();
        let (t, closed) = s.open(msg(), AT, Some(tx));
        closed.answer(Choice::Dismiss); // nothing was open: no one to answer
        (t, rx)
    }

    #[test]
    fn replacing_a_waiting_popover_answers_it_later() {
        let mut s = Session::default();
        let (_, mut rx) = waiting(&mut s);
        let (_, closed) = s.open(msg(), AT, None);
        closed.answer(Choice::Later);
        assert_eq!(rx.try_recv(), Ok(Answer::Later));
    }

    #[test]
    fn closing_answers_with_the_users_choice() {
        let mut s = Session::default();
        let (t, mut rx) = waiting(&mut s);
        assert!(s.is_current(t));
        s.close().answer(Choice::Dismiss);
        assert_eq!(rx.try_recv(), Ok(Answer::Dismiss));
        assert!(!s.is_current(t));
        assert_eq!(s.payload(), None);
    }

    fn diff_waiting(s: &mut Session) -> oneshot::Receiver<Answer> {
        let (tx, rx) = oneshot::channel();
        let _ = s.open(PopoverPayload::diff("I has a apple.", "I have an apple.", None, vec![]), AT, Some(tx));
        rx
    }

    #[test]
    fn accept_writes_only_the_fixes_left_on() {
        let mut s = Session::default();
        let mut rx = diff_waiting(&mut s);
        s.set_skipped(vec![1]);
        s.close().answer(Choice::Accept);
        assert_eq!(rx.try_recv(), Ok(Answer::Accept("I have a apple.".into())));
    }

    #[test]
    fn accept_with_every_fix_skipped_is_a_dismiss() {
        let mut s = Session::default();
        let mut rx = diff_waiting(&mut s);
        s.set_skipped(vec![0, 1]);
        s.close().answer(Choice::Accept);
        assert_eq!(rx.try_recv(), Ok(Answer::Dismiss));
    }

    #[test]
    fn a_new_popover_starts_with_every_fix_on() {
        let mut s = Session::default();
        let _ = diff_waiting(&mut s);
        s.set_skipped(vec![0]);
        let mut rx = diff_waiting(&mut s);
        s.close().answer(Choice::Accept);
        assert_eq!(rx.try_recv(), Ok(Answer::Accept("I have an apple.".into())));
    }

    #[test]
    fn a_ticket_goes_stale_when_another_popover_opens() {
        let mut s = Session::default();
        let (old, _) = s.open(msg(), AT, None);
        let (new, _) = s.open(msg(), AT, None);
        assert!(!s.is_current(old) && s.is_current(new));
    }

    #[test]
    fn auto_popups_wait_for_a_free_popover_and_one_fix_at_a_time() {
        let mut s = Session::default();
        assert!(s.claim_auto(1));
        assert!(!s.claim_auto(2));
        assert!(!s.release_auto(2));
        assert!(s.release_auto(1) && !s.holds_auto(1));
        let _ = s.open(msg(), AT, None);
        assert!(!s.claim_auto(3), "a message is showing");
    }

    #[test]
    fn expiry_waits_while_hovered() {
        let mut s = Session::default();
        let (t, _) = s.open(msg(), AT, None);
        s.placed(Rect { x: 10.0, y: 10.0, w: 100.0, h: 50.0 });
        assert_eq!(s.expiry(t, (50.0, 30.0)), Expiry::Keep);
        assert_eq!(s.expiry(t, (500.0, 30.0)), Expiry::Close);
        let _ = s.close();
        assert_eq!(s.expiry(t, (50.0, 30.0)), Expiry::Stale);
    }

    #[test]
    fn resize_caps_and_rejects_empty_heights() {
        let mut s = Session::default();
        let _ = s.open(msg(), AT, None);
        assert_eq!(s.size(), (POPOVER_W, FIRST_H));
        assert_eq!(s.resize(0.0, None), None);
        assert_eq!(s.resize(900.2, Some(120.4)), Some((121.0, MAX_H)));
        let _ = s.open(msg(), AT, None);
        assert_eq!(s.size(), (POPOVER_W, MAX_H), "a new popover measures at full width, last height");
    }
}
