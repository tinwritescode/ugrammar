//! `ugrammar://connect?key=ug_...` links from tyvn.online/account.
//! The endpoint is fixed so a link can never point the app at another server.

use tauri::{App, Url};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::app::ctx;

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

fn handle(url: &Url) {
    let Some(key) = license_from(url) else { return };
    tauri::async_runtime::spawn(async move {
        let c = ctx();
        c.update_settings(serde_json::json!({ "endpoint": ENDPOINT, "model": "ugrammar" }));
        c.set_api_key(&key).await;
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
}
