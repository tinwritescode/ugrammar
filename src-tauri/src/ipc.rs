use serde::{Deserialize, Serialize};
use ugrammar_core::diff::DiffOp;
use ugrammar_core::i18n::Lang;
use ugrammar_core::insights::Insight;
use ugrammar_core::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum PopoverKind {
    Diff,
    Message,
    Loading,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
pub struct PopoverPayload {
    pub kind: PopoverKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ops: Option<Vec<DiffOp>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
    /// Esc dismisses it; auto-check popups leave Esc to the app being typed in.
    pub esc: bool,
    /// UI language; `Ctx::open_popover` sets it from settings.
    pub lang: Lang,
}

impl PopoverPayload {
    pub fn diff(original: &str, corrected: &str, app_name: Option<String>, notes: Vec<String>) -> Self {
        PopoverPayload {
            kind: PopoverKind::Diff,
            original: Some(original.into()),
            corrected: Some(corrected.into()),
            ops: Some(ugrammar_core::diff::diff_words(original, corrected)),
            message: None,
            app_name,
            notes: (!notes.is_empty()).then_some(notes),
            esc: true,
            lang: Lang::En,
        }
    }

    pub fn text(kind: PopoverKind, message: &str) -> Self {
        PopoverPayload { kind, original: None, corrected: None, ops: None, message: Some(message.into()), app_name: None, notes: None, esc: false, lang: Lang::En }
    }
}

/// An excluded app as the tray lists it; `name`/`icon` are absent when the app is not installed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
pub struct AppInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Base64 PNG.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Choice {
    Accept,
    Dismiss,
    /// Closed without a decision (timed out, or another popover took its place).
    Later,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum StatusKind {
    Idle,
    Checking,
    Paused,
    Error,
    NeedsPermission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub kind: StatusKind,
    pub message: Option<String>,
}

impl Status {
    pub fn of(kind: StatusKind) -> Self {
        Status { kind, message: None }
    }
}

/// A macOS permission ugrammar needs. Only Accessibility is used today; the list stays an enum
/// (rather than a bare bool) so a future permission slots in without changing the wire shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum PermissionId {
    Accessibility,
}

/// Live status of one permission, for the tray's permissions screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PermissionStatus {
    pub id: PermissionId,
    pub granted: bool,
    /// Was granted before but macOS no longer trusts this build (common after a rebuild/re-sign);
    /// System Settings still lists the app, so re-opening Settings alone won't fix it.
    pub stale: bool,
}

/// Result of the last manual "check for updates", shown under its button until the tray closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum UpdateNote {
    Checking,
    Downloading,
    UpToDate,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
pub struct TrayState {
    pub status: StatusKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub settings: Settings,
    pub hotkey: String,
    pub insights: Vec<Insight>,
    pub has_api_key: bool,
    /// Version installed in place, waiting for a restart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_note: Option<UpdateNote>,
    /// Auto-check fixes not acted on yet, oldest first.
    pub pending: Vec<PendingFix>,
    pub permissions: Vec<PermissionStatus>,
    /// Read from the macOS login item, not settings.json.
    pub launch_at_login: bool,
}

/// An auto-check fix waiting in the menu-bar list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(optional_fields))]
pub struct PendingFix {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    pub ops: Vec<DiffOp>,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use ts_rs::{Config, TS};
    use ugrammar_core::engine::{ConnectionStatus, Tone};

    fn read_dir(dir: &Path) -> BTreeMap<String, String> {
        let entries = fs::read_dir(dir).into_iter().flatten().flatten();
        entries.filter_map(|e| Some((e.file_name().into_string().ok()?, fs::read_to_string(e.path()).ok()?))).collect()
    }

    /// The web UI's copies of these types are generated from them: src/shared/bindings. A stale copy is rewritten
    /// and fails this test once, so the change shows up in the diff.
    #[test]
    fn typescript_bindings_are_current() {
        let tmp = std::env::temp_dir().join(format!("ugrammar-ts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let cfg = Config::new().with_out_dir(&tmp).with_large_int("number");
        TrayState::export_all(&cfg).unwrap();
        PopoverPayload::export_all(&cfg).unwrap();
        AppInfo::export_all(&cfg).unwrap();
        Choice::export_all(&cfg).unwrap();
        Tone::export_all(&cfg).unwrap();
        ConnectionStatus::export_all(&cfg).unwrap();
        let fresh = read_dir(&tmp);
        let _ = fs::remove_dir_all(&tmp);

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/shared/bindings");
        let old = read_dir(&dir);
        if old == fresh {
            return;
        }
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in &fresh {
            fs::write(dir.join(name), body).unwrap();
        }
        let changed: std::collections::BTreeSet<_> = fresh.keys().chain(old.keys()).filter(|n| old.get(*n) != fresh.get(*n)).collect();
        panic!("src/shared/bindings was stale and is rewritten now: {changed:?}");
    }
}
