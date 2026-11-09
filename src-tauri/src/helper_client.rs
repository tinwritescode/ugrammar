//! The Swift helper's protocol, typed: one call per command. Command names, argument shapes, reply shapes,
//! timeouts, error codes, hotkey slots and the Keychain item all live here and in `main.swift`'s dispatch, nowhere else.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use ugrammar_core::settings::Hotkey;

use crate::helper::Helper;
pub use crate::placement::Rect;
use crate::ipc::AppInfo;

/// Sends one command and resolves with its reply object. Never fails: transport trouble comes back as
/// `{ok:false,error}` like the helper's own errors.
pub trait Transport: Send + Sync {
    fn request(&self, cmd: &'static str, args: Value, timeout: Duration) -> impl Future<Output = Value> + Send;
}

impl Transport for Helper {
    fn request(&self, cmd: &'static str, args: Value, timeout: Duration) -> impl Future<Output = Value> + Send {
        Helper::request(self, cmd, args, timeout)
    }
}

const KEY_SERVICE: &str = "Proofy";
const KEY_ACCOUNT: &str = "api-key";

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// Error codes from the helper and its transport.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub enum HelperError {
    AccessibilityNotTrusted,
    ExcludedApp,
    SecureField,
    NoSelection,
    FocusChanged,
    /// The helper process is not up (yet).
    NotRunning,
    Timeout,
    /// A reply that does not match the protocol.
    BadReply,
    Other(String),
}

impl From<String> for HelperError {
    fn from(code: String) -> Self {
        match code.as_str() {
            "accessibility_not_trusted" => Self::AccessibilityNotTrusted,
            "excluded_app" => Self::ExcludedApp,
            "secure_field" => Self::SecureField,
            "no_selection" => Self::NoSelection,
            "focus_changed" => Self::FocusChanged,
            "helper_not_running" | "helper_exited" => Self::NotRunning,
            "helper_timeout" => Self::Timeout,
            _ => Self::Other(code),
        }
    }
}

impl fmt::Display for HelperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AccessibilityNotTrusted => "accessibility_not_trusted",
            Self::ExcludedApp => "excluded_app",
            Self::SecureField => "secure_field",
            Self::NoSelection => "no_selection",
            Self::FocusChanged => "focus_changed",
            Self::NotRunning => "helper_not_running",
            Self::Timeout => "helper_timeout",
            Self::BadReply => "bad_reply",
            Self::Other(code) => code,
        })
    }
}

/// The two global hotkeys the helper registers; its `hotkey` events name the slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySlot {
    /// Check the selection (user-set, ⌥⌘G by default).
    Fix,
    /// Tone menu, fixed ⌥⌘T.
    Tone,
}

impl HotkeySlot {
    fn wire(self) -> u64 {
        match self {
            Self::Fix => 1,
            Self::Tone => 2,
        }
    }

    pub fn from_wire(n: u64) -> Option<Self> {
        [Self::Fix, Self::Tone].into_iter().find(|s| s.wire() == n)
    }
}

/// The selected text and the app it is in.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub text: String,
    /// How it was read, and so how to write it back: "ax" or "clipboard".
    pub method: Option<String>,
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub pid: Option<i64>,
}

/// Where the caret or selection is on screen; `precise` when the app reported the selection's own bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub rect: Rect,
    pub precise: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub ok: bool,
    pub error: Option<String>,
    pub reason: Option<Value>,
    pub visible: Option<bool>,
    pub text: Option<String>,
    pub caret: Option<usize>,
    pub sel_len: Option<usize>,
    pub pid: Option<i64>,
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub bounds: Option<Vec<Option<Rect>>>,
    /// Frame of the focused field; sent with bounds requests only.
    pub field: Option<Rect>,
    /// Frame of the focused window; sent with bounds requests only.
    pub window: Option<Rect>,
}

/// Replace `len` UTF-16 units at `loc` with `text`, only if they still read `expect`.
pub struct RangeEdit<'a> {
    pub loc: usize,
    pub len: usize,
    pub text: &'a str,
    pub expect: &'a str,
    pub pid: Option<i64>,
}

#[derive(Deserialize)]
struct Written {
    method: Option<String>,
}

#[derive(Clone)]
pub struct HelperClient<T = Helper> {
    t: T,
}

impl<T: Transport> HelperClient<T> {
    pub fn new(t: T) -> Self {
        Self { t }
    }

    async fn raw(&self, cmd: &'static str, args: Value, timeout: Duration) -> Result<Value, HelperError> {
        let r = self.t.request(cmd, args, timeout).await;
        if r["ok"] == true {
            return Ok(r);
        }
        Err(r["error"].as_str().map_or(HelperError::BadReply, |e| HelperError::from(e.to_string())))
    }

    async fn call<R: DeserializeOwned>(&self, cmd: &'static str, args: Value, timeout: Duration) -> Result<R, HelperError> {
        serde_json::from_value(self.raw(cmd, args, timeout).await?).map_err(|_| HelperError::BadReply)
    }

    /// Whether ugrammar has Accessibility permission.
    pub async fn trusted(&self) -> bool {
        self.raw("status", json!({}), secs(10)).await.is_ok_and(|r| r["trusted"] == true)
    }

    /// Shows macOS's Accessibility prompt.
    pub async fn prompt_ax(&self) {
        let _ = self.raw("prompt_ax", json!({}), secs(10)).await;
    }

    pub async fn set_hotkey(&self, slot: HotkeySlot, h: &Hotkey) -> Result<(), HelperError> {
        self.raw("set_hotkey", json!({ "key": h.key, "mods": h.mods, "slot": slot.wire() }), secs(10)).await.map(drop)
    }

    pub async fn read(&self, excluded: &[String]) -> Result<Selection, HelperError> {
        let sel: Selection = self.call("read", json!({ "excluded": excluded }), secs(8)).await?;
        if sel.text.is_empty() {
            return Err(HelperError::NoSelection);
        }
        Ok(sel)
    }

    /// Replaces the selection; Ok carries the method used.
    pub async fn write(&self, text: &str, pid: Option<i64>, method: Option<&str>) -> Result<Option<String>, HelperError> {
        let w: Written = self.call("write", json!({ "text": text, "pid": pid, "method": method }), secs(8)).await?;
        Ok(w.method)
    }

    /// Under the selection, else the focused field; None when there is nothing visible to anchor to.
    pub async fn caret(&self, excluded: &[String]) -> Option<Caret> {
        let r = self.raw("caret", json!({ "excluded": excluded, "deep": true }), Duration::from_millis(1500)).await.ok()?;
        if r["visible"] != true {
            return None;
        }
        let rect = Rect { x: r["x"].as_f64()?, y: r["y"].as_f64()?, w: r["w"].as_f64()?, h: r["h"].as_f64()? };
        Some(Caret { rect, precise: r["precise"] == true })
    }

    /// The focused field's text and caret, with screen bounds for each (loc, len) range asked for.
    pub async fn snapshot(&self, excluded: &[String], ranges: &[(usize, usize)]) -> Snapshot {
        let ranges: Vec<_> = ranges.iter().map(|(loc, len)| json!({ "loc": loc, "len": len })).collect();
        let r = self.t.request("snapshot", json!({ "excluded": excluded, "ranges": ranges }), secs(3)).await;
        serde_json::from_value(r).unwrap_or_default()
    }

    /// Ok carries the method used.
    pub async fn replace_range(&self, e: RangeEdit<'_>) -> Result<Option<String>, HelperError> {
        let args = json!({ "loc": e.loc, "len": e.len, "text": e.text, "expect": e.expect, "pid": e.pid });
        let w: Written = self.call("replace_range", args, secs(5)).await?;
        Ok(w.method)
    }

    /// Brings the app with this pid forward.
    pub async fn activate(&self, pid: i64) {
        let _ = self.raw("activate", json!({ "pid": pid }), secs(3)).await;
    }

    pub async fn app_info(&self, ids: &[String]) -> Vec<AppInfo> {
        #[derive(Deserialize)]
        struct Apps {
            apps: Vec<AppInfo>,
        }
        self.call::<Apps>("app_info", json!({ "ids": ids }), secs(10)).await.map(|a| a.apps).unwrap_or_default()
    }

    /// The saved API key; Ok(None) when there is none.
    pub async fn api_key(&self) -> Result<Option<String>, HelperError> {
        let r = self.raw("keychain_get", json!({ "service": KEY_SERVICE, "account": KEY_ACCOUNT }), secs(10)).await?;
        Ok(r["value"].as_str().map(String::from))
    }

    /// Saves the API key; an empty key deletes it.
    pub async fn set_api_key(&self, key: &str) -> Result<(), HelperError> {
        let item = json!({ "service": KEY_SERVICE, "account": KEY_ACCOUNT });
        let r = if key.is_empty() {
            self.raw("keychain_delete", item, secs(10)).await
        } else {
            self.raw("keychain_set", json!({ "service": KEY_SERVICE, "account": KEY_ACCOUNT, "value": key }), secs(10)).await
        };
        r.map(drop)
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::{Arc, Mutex};

    type Reply = dyn Fn(&str, &Value) -> Value + Send + Sync;

    /// In-memory helper: answers each command from `reply` and records what was sent.
    #[derive(Clone)]
    pub struct FakeHelper {
        pub sent: Arc<Mutex<Vec<(String, Value)>>>,
        reply: Arc<Reply>,
    }

    impl FakeHelper {
        pub fn new(reply: impl Fn(&str, &Value) -> Value + Send + Sync + 'static) -> Self {
            Self { sent: Arc::default(), reply: Arc::new(reply) }
        }
    }

    impl Transport for FakeHelper {
        fn request(&self, cmd: &'static str, args: Value, _timeout: Duration) -> impl Future<Output = Value> + Send {
            let r = (self.reply)(cmd, &args);
            self.sent.lock().unwrap().push((cmd.into(), args));
            async move { r }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHelper;
    use super::*;
    use ugrammar_core::settings::Mod;

    fn client(reply: impl Fn(&str, &Value) -> Value + Send + Sync + 'static) -> (HelperClient<FakeHelper>, FakeHelper) {
        let f = FakeHelper::new(reply);
        (HelperClient::new(f.clone()), f)
    }

    #[tokio::test]
    async fn read_returns_the_selection_or_a_typed_error() {
        let (c, f) = client(|_, _| json!({ "ok": true, "text": "I has", "method": "ax", "pid": 7, "appName": "Notes" }));
        let sel = c.read(&["com.x".into()]).await.unwrap();
        assert_eq!((sel.text.as_str(), sel.pid, sel.app_name.as_deref()), ("I has", Some(7), Some("Notes")));
        assert_eq!(f.sent.lock().unwrap()[0], ("read".into(), json!({ "excluded": ["com.x"] })));

        for (code, err) in [
            ("secure_field", HelperError::SecureField),
            ("excluded_app", HelperError::ExcludedApp),
            ("accessibility_not_trusted", HelperError::AccessibilityNotTrusted),
            ("helper_exited", HelperError::NotRunning),
            ("weird", HelperError::Other("weird".into())),
        ] {
            let (c, _) = client(move |_, _| json!({ "ok": false, "error": code }));
            assert_eq!(c.read(&[]).await.unwrap_err(), err);
        }
        let (c, _) = client(|_, _| json!({ "ok": true, "text": "" }));
        assert_eq!(c.read(&[]).await.unwrap_err(), HelperError::NoSelection);
    }

    #[tokio::test]
    async fn malformed_replies_are_bad_replies() {
        let (c, _) = client(|_, _| json!({ "ok": true }));
        assert_eq!(c.read(&[]).await.unwrap_err(), HelperError::BadReply);
        let (c, _) = client(|_, _| json!({ "ok": false }));
        assert_eq!(c.write("x", None, None).await.unwrap_err(), HelperError::BadReply);
    }

    #[tokio::test]
    async fn api_key_round_trip_and_delete_on_empty() {
        let (c, f) = client(|cmd, _| match cmd {
            "keychain_get" => json!({ "ok": true, "value": null }),
            _ => json!({ "ok": true }),
        });
        assert_eq!(c.api_key().await, Ok(None));
        c.set_api_key("k").await.unwrap();
        c.set_api_key("").await.unwrap();
        let sent = f.sent.lock().unwrap();
        assert_eq!(sent[1], ("keychain_set".into(), json!({ "service": "Proofy", "account": "api-key", "value": "k" })));
        assert_eq!(sent[2], ("keychain_delete".into(), json!({ "service": "Proofy", "account": "api-key" })));
    }

    #[tokio::test]
    async fn keychain_failure_is_an_error_not_a_missing_key() {
        let (c, _) = client(|_, _| json!({ "ok": false, "error": "keychain_status_-25308" }));
        assert_eq!(c.api_key().await, Err(HelperError::Other("keychain_status_-25308".into())));
    }

    #[tokio::test]
    async fn hotkey_slots_go_on_the_wire_as_numbers() {
        let (c, f) = client(|_, _| json!({ "ok": true }));
        c.set_hotkey(HotkeySlot::Tone, &Hotkey { key: "T".into(), mods: vec![Mod::Alt, Mod::Cmd] }).await.unwrap();
        assert_eq!(f.sent.lock().unwrap()[0].1, json!({ "key": "T", "mods": ["alt", "cmd"], "slot": 2 }));
        assert_eq!(HotkeySlot::from_wire(1), Some(HotkeySlot::Fix));
        assert_eq!(HotkeySlot::from_wire(2), Some(HotkeySlot::Tone));
        assert_eq!(HotkeySlot::from_wire(9), None);
    }

    #[tokio::test]
    async fn caret_needs_a_visible_rect() {
        let (c, _) = client(|_, _| json!({ "ok": true, "visible": true, "precise": true, "x": 1, "y": 2, "w": 3, "h": 4 }));
        assert_eq!(c.caret(&[]).await, Some(Caret { rect: Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 }, precise: true }));
        let (c, _) = client(|_, _| json!({ "ok": true, "visible": false }));
        assert_eq!(c.caret(&[]).await, None);
    }

    #[tokio::test]
    async fn snapshot_sends_ranges_and_survives_transport_errors() {
        let (c, f) = client(|_, _| json!({ "ok": false, "error": "helper_timeout" }));
        let s = c.snapshot(&[], &[(3, 1), (9, 1)]).await;
        assert!(!s.ok);
        assert_eq!(f.sent.lock().unwrap()[0].1["ranges"], json!([{ "loc": 3, "len": 1 }, { "loc": 9, "len": 1 }]));
    }

    #[tokio::test]
    async fn replace_range_sends_the_expected_text() {
        let (c, f) = client(|_, _| json!({ "ok": true, "method": "ax" }));
        let m = c.replace_range(RangeEdit { loc: 4, len: 5, text: "have", expect: "has", pid: Some(3) }).await.unwrap();
        assert_eq!(m.as_deref(), Some("ax"));
        assert_eq!(f.sent.lock().unwrap()[0].1, json!({ "loc": 4, "len": 5, "text": "have", "expect": "has", "pid": 3 }));
    }
}
