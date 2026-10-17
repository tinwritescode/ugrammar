use serde::{Deserialize, Serialize};
use ugrammar_core::diff::DiffOp;
use ugrammar_core::insights::Insight;
use ugrammar_core::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PopoverKind {
    Diff,
    Message,
    Loading,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
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
        }
    }

    pub fn text(kind: PopoverKind, message: &str) -> Self {
        PopoverPayload { kind, original: None, corrected: None, ops: None, message: Some(message.into()), app_name: None, notes: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Accept,
    Dismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayState {
    pub status: StatusKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub settings: Settings,
    pub hotkey: String,
    pub insights: Vec<Insight>,
    pub has_api_key: bool,
}
