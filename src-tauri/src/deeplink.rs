//! `ugrammar://connect?key=ug_...` links from tyvn.online/account.
//! The endpoint is fixed so a link can never point the app at another server.

use tauri::{App, Url};
use tauri_plugin_deep_link::DeepLinkExt;
use tokio::sync::oneshot;
use ugrammar_core::i18n::fill;

use crate::app::{ctx, Ctx};

pub const ENDPOINT: &str = "https://tyvn.online/api/public/ugrammar/v1";

/// The license key in a valid connect link, or None.
pub fn license_from(url: &Url) -> Option<String> {
    if url.scheme() != "ugrammar" || url.host_str() != Some("connect") {
        return None;
    }
    let key = url.query_pairs().find(|(k, _)| k == "key")?.1.to_string();
    let ok = key.len() == 67 && key.starts_with("ug_") && key[3..].bytes().all(|b| b.is_ascii_hexdigit());
    ok.then_some(key)
}

pub trait ConnectDeps {
    /// Asks the user; any web page can open a connect link.
    async fn confirm(&self) -> bool;
    async fn set_api_key(&self, key: &str) -> Result<(), String>;
    fn use_license_endpoint(&self);
}

#[derive(Debug, PartialEq, Eq)]
pub enum Connect {
    Cancelled,
    KeyFailed(String),
    Connected,
}

/// The key is saved before the endpoint switches, so the old key is never sent to the license server.
pub async fn connect<D: ConnectDeps>(d: &D, key: &str) -> Connect {
    if !d.confirm().await {
        return Connect::Cancelled;
    }
    if let Err(e) = d.set_api_key(key).await {
        return Connect::KeyFailed(e);
    }
    d.use_license_endpoint();
    Connect::Connected
}

impl ConnectDeps for Ctx {
    async fn confirm(&self) -> bool {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        let t = self.settings().strings();
        let (tx, rx) = oneshot::channel();
        self.app
            .dialog()
            .message(fill(t.connect_body, &[("endpoint", ENDPOINT)]))
            .title(t.connect_title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(t.connect_ok.into(), t.cancel.into()))
            .show(move |ok| {
                let _ = tx.send(ok);
            });
        rx.await.unwrap_or(false)
    }

    async fn set_api_key(&self, key: &str) -> Result<(), String> {
        Ctx::set_api_key(self, key).await
    }

    fn use_license_endpoint(&self) {
        ctx().update_settings(serde_json::json!({ "provider": "openai", "endpoint": ENDPOINT, "model": "ugrammar" }));
    }
}

fn handle(url: &Url) {
    let Some(key) = license_from(url) else { return };
    tauri::async_runtime::spawn(async move {
        let c = ctx();
        match connect(c, &key).await {
            Connect::Cancelled => return,
            Connect::KeyFailed(e) => c.notify(&fill(c.settings().strings().key_save_failed, &[("error", &e)])),
            Connect::Connected => {}
        }
        c.refresh_tray();
        c.open_panel().await;
    });
}

/// Call after the app context exists.
pub fn register(app: &App) {
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        urls.iter().for_each(handle);
    }
    app.deep_link().on_open_url(|event| event.urls().iter().for_each(handle));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn key() -> String {
        format!("ug_{}", "a1".repeat(32))
    }

    #[test]
    fn accepts_connect_link() {
        let u = Url::parse(&format!("ugrammar://connect?key={}", key())).unwrap();
        assert_eq!(license_from(&u), Some(key()));
    }

    #[test]
    fn rejects_other_hosts_schemes_and_bad_keys() {
        assert_eq!(license_from(&Url::parse(&format!("ugrammar://other?key={}", key())).unwrap()), None);
        assert_eq!(license_from(&Url::parse(&format!("https://connect?key={}", key())).unwrap()), None);
        assert_eq!(license_from(&Url::parse("ugrammar://connect?key=ug_short").unwrap()), None);
        assert_eq!(license_from(&Url::parse("ugrammar://connect").unwrap()), None);
    }

    struct Fake {
        confirm: bool,
        key_err: Option<&'static str>,
        log: RefCell<Vec<String>>,
    }

    fn fake(confirm: bool, key_err: Option<&'static str>) -> Fake {
        Fake { confirm, key_err, log: RefCell::default() }
    }

    impl ConnectDeps for Fake {
        async fn confirm(&self) -> bool {
            self.log.borrow_mut().push("confirm".into());
            self.confirm
        }
        async fn set_api_key(&self, key: &str) -> Result<(), String> {
            self.log.borrow_mut().push(format!("key {key}"));
            self.key_err.map_or(Ok(()), |e| Err(e.into()))
        }
        fn use_license_endpoint(&self) {
            self.log.borrow_mut().push("endpoint".into());
        }
    }

    #[tokio::test]
    async fn cancel_changes_nothing() {
        let f = fake(false, None);
        assert_eq!(connect(&f, &key()).await, Connect::Cancelled);
        assert_eq!(*f.log.borrow(), ["confirm"]);
    }

    #[tokio::test]
    async fn key_is_saved_before_the_endpoint_switches() {
        let f = fake(true, None);
        assert_eq!(connect(&f, &key()).await, Connect::Connected);
        assert_eq!(*f.log.borrow(), ["confirm".to_string(), format!("key {}", key()), "endpoint".into()]);
    }

    #[tokio::test]
    async fn failed_key_save_keeps_the_old_endpoint() {
        let f = fake(true, Some("keychain_status_-25308"));
        assert_eq!(connect(&f, &key()).await, Connect::KeyFailed("keychain_status_-25308".into()));
        assert!(!f.log.borrow().iter().any(|l| l == "endpoint"));
    }
}
