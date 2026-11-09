//! What a settings change sets in motion. Every path that saves settings (the settings view, the tray's
//! pause item, a connect link) goes through `Ctx::update_settings`, which applies these.

use crate::ipc::{Status, StatusKind};
use ugrammar_core::engine::EngineSetup;
use ugrammar_core::settings::Settings;

#[derive(Debug, Clone, PartialEq)]
pub struct Reactions {
    /// Cached auto-check answers came from another engine setup.
    pub clear_cache: bool,
    /// Whether the auto-check loop should be running.
    pub auto_check: bool,
    pub register_hotkey: bool,
    /// The status to show now; None keeps what is shown (a check running, an error).
    pub status: Option<Status>,
}

pub fn react(before: &Settings, after: &Settings, shown: &Status) -> Reactions {
    Reactions {
        clear_cache: EngineSetup::from_settings(before) != EngineSetup::from_settings(after),
        auto_check: after.auto_check && !after.paused,
        register_hotkey: before.hotkey != after.hotkey,
        status: matches!(shown.kind, StatusKind::Paused | StatusKind::Idle).then(|| resting(after)),
    }
}

/// The status when nothing is going on.
pub fn resting(s: &Settings) -> Status {
    Status::of(if s.paused { StatusKind::Paused } else { StatusKind::Idle })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugrammar_core::settings::Hotkey;

    fn idle() -> Status {
        Status::of(StatusKind::Idle)
    }

    #[test]
    fn nothing_changed_nothing_to_do() {
        let s = Settings::default();
        let r = react(&s, &s, &idle());
        assert!(!r.clear_cache && !r.register_hotkey);
        assert_eq!(r.auto_check, s.auto_check && !s.paused);
    }

    #[test]
    fn pausing_stops_auto_check_and_shows_paused() {
        let before = Settings { auto_check: true, ..Settings::default() };
        let after = Settings { paused: true, ..before.clone() };
        let r = react(&before, &after, &idle());
        assert!(!r.auto_check);
        assert_eq!(r.status, Some(Status::of(StatusKind::Paused)));
        let r = react(&after, &before, &Status::of(StatusKind::Paused));
        assert!(r.auto_check);
        assert_eq!(r.status, Some(idle()));
    }

    #[test]
    fn a_running_check_or_error_stays_shown() {
        let s = Settings::default();
        let paused = Settings { paused: true, ..s.clone() };
        for kind in [StatusKind::Checking, StatusKind::Error, StatusKind::NeedsPermission] {
            assert_eq!(react(&s, &paused, &Status::of(kind)).status, None);
        }
    }

    #[test]
    fn engine_changes_clear_the_cache_and_hotkey_changes_reregister() {
        let s = Settings::default();
        let model = Settings { model: format!("{}-x", s.model), ..s.clone() };
        assert!(react(&s, &model, &idle()).clear_cache);
        let hotkey = Settings { hotkey: Hotkey { key: "K".into(), ..s.hotkey.clone() }, ..s.clone() };
        let r = react(&s, &hotkey, &idle());
        assert!(r.register_hotkey && !r.clear_cache);
    }
}
