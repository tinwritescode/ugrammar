use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::text::js_trim;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mod {
    Cmd,
    Alt,
    Ctrl,
    Shift,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hotkey {
    pub key: String, // A-Z
    pub mods: Vec<Mod>,
}

/// Field order is the order `settings.json` is written in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub endpoint: String,
    pub model: String,
    pub timeout_ms: u64,
    pub dictionary: Vec<String>,
    pub excluded_apps: Vec<String>, // bundle ids
    pub hotkey: Hotkey,
    pub paused: bool,
    pub explain: bool, // short Vietnamese reason per fix (slower)
    pub auto_check: bool, // check each sentence when its end is typed (. ? ! Enter)
    pub quick_fix: bool, // hotkey replaces at once, no popover (⌘Z undoes)
    pub explain_lang: String, // language of the per-fix reasons
    pub first_run_acknowledged: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:20128/v1".into(),
            model: "xpiki/gpt-6.1-sol".into(),
            timeout_ms: 10000,
            dictionary: vec![],
            excluded_apps: [
                "com.1password.1password",
                "com.agilebits.onepassword7",
                "com.bitwarden.desktop",
                "com.apple.keychainaccess",
                "com.apple.Passwords",
                "com.apple.Terminal",
                "com.googlecode.iterm2",
                "com.mitchellh.ghostty",
                "dev.warp.Warp-Stable",
            ]
            .map(String::from)
            .into(),
            hotkey: Hotkey { key: "G".into(), mods: vec![Mod::Alt, Mod::Cmd] },
            paused: false,
            explain: true,
            auto_check: false,
            quick_fix: false,
            explain_lang: "Vietnamese".into(),
            first_run_acknowledged: false,
        }
    }
}

fn str_list(v: Option<&Value>) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    Some(
        v?.as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(js_trim)
            .filter(|w| !w.is_empty() && seen.insert(w.to_lowercase()))
            .map(String::from)
            .collect(),
    )
}

fn hotkey(v: Option<&Value>) -> Option<Hotkey> {
    let hk = v?;
    let key = hk.get("key")?.as_str()?;
    if key.len() != 1 || !key.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let mods = hk.get("mods")?.as_array()?;
    if mods.is_empty() {
        return None;
    }
    let mods = mods.iter().map(|m| Mod::deserialize(m).ok()).collect::<Option<Vec<_>>>()?;
    Some(Hotkey { key: key.to_ascii_uppercase(), mods })
}

pub fn normalize_settings(raw: &Value) -> Settings {
    let empty = Map::new();
    let r = raw.as_object().unwrap_or(&empty);
    let d = Settings::default();
    let str_or = |k: &str, f: String| r.get(k).and_then(Value::as_str).map(js_trim).filter(|s| !s.is_empty()).map_or(f, String::from);
    let is = |k: &str, v: bool| r.get(k) == Some(&Value::Bool(v));
    Settings {
        endpoint: str_or("endpoint", d.endpoint).trim_end_matches('/').to_string(),
        model: str_or("model", d.model),
        timeout_ms: r
            .get("timeoutMs")
            .and_then(Value::as_f64)
            .filter(|t| (1000.0..=120000.0).contains(t))
            .map_or(d.timeout_ms, |t| t.round() as u64),
        dictionary: str_list(r.get("dictionary")).unwrap_or_default(),
        excluded_apps: str_list(r.get("excludedApps")).unwrap_or(d.excluded_apps),
        hotkey: hotkey(r.get("hotkey")).unwrap_or(d.hotkey),
        paused: is("paused", true),
        explain: !is("explain", false),
        auto_check: is("autoCheck", true),
        quick_fix: is("quickFix", true),
        explain_lang: str_or("explainLang", d.explain_lang),
        first_run_acknowledged: is("firstRunAcknowledged", true),
    }
}

type Listener = Box<dyn Fn(&Settings) + Send + Sync>;

/// Persists to <dir>/settings.json. The API key is never part of Settings (it lives in the Keychain).
pub struct SettingsStore {
    dir: PathBuf,
    data: Settings,
    listeners: Vec<Listener>,
}

impl SettingsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let data = fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_str::<Value>(&String::from_utf8_lossy(&b)).ok())
            .map_or_else(|| normalize_settings(&Value::Null), |v| normalize_settings(&v));
        Self { dir, data, listeners: vec![] }
    }

    pub fn get(&self) -> Settings {
        self.data.clone()
    }

    /// `patch` is merged over the current settings JSON (camelCase keys) and normalized; unknown keys are dropped.
    pub fn update(&mut self, patch: Value) -> io::Result<Settings> {
        let mut merged = serde_json::to_value(&self.data)?;
        if let (Value::Object(m), Value::Object(p)) = (&mut merged, patch) {
            m.extend(p);
        }
        self.data = normalize_settings(&merged);
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join("settings.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.data)? + "\n")?;
        fs::rename(tmp, self.dir.join("settings.json"))?;
        for l in &self.listeners {
            l(&self.data);
        }
        Ok(self.get())
    }

    pub fn on_change(&mut self, f: impl Fn(&Settings) + Send + Sync + 'static) {
        self.listeners.push(Box::new(f));
    }

    pub fn add_word(&mut self, word: &str) -> io::Result<()> {
        let mut dictionary = self.data.dictionary.clone();
        dictionary.push(word.into());
        self.update(serde_json::json!({ "dictionary": dictionary })).map(drop)
    }

    pub fn remove_word(&mut self, word: &str) -> io::Result<()> {
        let w = js_trim(word).to_lowercase();
        let dictionary: Vec<_> = self.data.dictionary.iter().filter(|x| x.to_lowercase() != w).collect();
        self.update(serde_json::json!({ "dictionary": dictionary })).map(drop)
    }

    pub fn add_excluded_app(&mut self, id: &str) -> io::Result<()> {
        let mut apps = self.data.excluded_apps.clone();
        apps.push(id.into());
        self.update(serde_json::json!({ "excludedApps": apps })).map(drop)
    }

    pub fn remove_excluded_app(&mut self, id: &str) -> io::Result<()> {
        let id = js_trim(id);
        let apps: Vec<_> = self.data.excluded_apps.iter().filter(|x| *x != id).collect();
        self.update(serde_json::json!({ "excludedApps": apps })).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn defaults_when_no_file() {
        let dir = TempDir::new();
        let s = SettingsStore::new(dir.path());
        assert_eq!(s.get().endpoint, "http://localhost:20128/v1");
        assert_eq!(s.get().model, "xpiki/gpt-6.1-sol");
        assert_eq!(s.get().hotkey, Hotkey { key: "G".into(), mods: vec![Mod::Alt, Mod::Cmd] });
    }

    #[test]
    fn persists_to_settings_json_and_reloads() {
        let dir = TempDir::new();
        let mut s = SettingsStore::new(dir.path());
        s.update(json!({ "endpoint": "http://x/v1", "model": "m2" })).unwrap();
        assert!(dir.path().join("settings.json").exists());
        let s2 = SettingsStore::new(dir.path());
        assert_eq!(s2.get().endpoint, "http://x/v1");
        assert_eq!(s2.get().model, "m2");
    }

    #[test]
    fn never_stores_api_key_in_json() {
        let dir = TempDir::new();
        let mut s = SettingsStore::new(dir.path());
        s.update(json!({ "endpoint": "http://x/v1", "apiKey": "secret" })).unwrap();
        assert!(!fs::read_to_string(dir.path().join("settings.json")).unwrap().contains("secret"));
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = TempDir::new();
        fs::write(dir.path().join("settings.json"), "{not json").unwrap();
        assert_eq!(SettingsStore::new(dir.path()).get(), Settings::default());
    }

    #[test]
    fn dictionary_add_dedupes_case_insensitively_trims_removes() {
        let dir = TempDir::new();
        let mut s = SettingsStore::new(dir.path());
        s.add_word(" Xpiki ").unwrap();
        s.add_word("xpiki").unwrap();
        s.add_word("").unwrap();
        assert_eq!(s.get().dictionary, ["Xpiki"]);
        s.remove_word("XPIKI").unwrap();
        assert!(s.get().dictionary.is_empty());
    }

    #[test]
    fn excluded_apps_add_remove() {
        let dir = TempDir::new();
        let mut s = SettingsStore::new(dir.path());
        let base = s.get().excluded_apps.len();
        s.add_excluded_app("com.foo.bar").unwrap();
        s.add_excluded_app("com.foo.bar").unwrap();
        assert_eq!(s.get().excluded_apps.len(), base + 1);
        s.remove_excluded_app("com.foo.bar").unwrap();
        assert_eq!(s.get().excluded_apps.len(), base);
    }

    #[test]
    fn defaults_exclude_password_managers_and_terminals() {
        let ex = Settings::default().excluded_apps;
        assert!(ex.contains(&"com.apple.Terminal".into()));
        assert!(ex.contains(&"com.1password.1password".into()));
    }

    #[test]
    fn notifies_listeners_on_change() {
        let dir = TempDir::new();
        let mut s = SettingsStore::new(dir.path());
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        s.on_change(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        s.update(json!({ "model": "z" })).unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn writes_pretty_camel_case_json_with_trailing_newline() {
        let dir = TempDir::new();
        let mut s = SettingsStore::new(dir.path());
        s.update(json!({ "excludedApps": [], "hotkey": { "key": "k", "mods": ["cmd"] } })).unwrap();
        let expected = "{\n  \"endpoint\": \"http://localhost:20128/v1\",\n  \"model\": \"xpiki/gpt-6.1-sol\",\n  \"timeoutMs\": 10000,\n  \"dictionary\": [],\n  \"excludedApps\": [],\n  \"hotkey\": {\n    \"key\": \"K\",\n    \"mods\": [\n      \"cmd\"\n    ]\n  },\n  \"paused\": false,\n  \"explain\": true,\n  \"autoCheck\": false,\n  \"quickFix\": false,\n  \"explainLang\": \"Vietnamese\",\n  \"firstRunAcknowledged\": false\n}\n";
        assert_eq!(fs::read_to_string(dir.path().join("settings.json")).unwrap(), expected);
        assert!(!dir.path().join("settings.json.tmp").exists());
    }

    #[test]
    fn rejects_bad_types_keeps_valid_fields_strips_trailing_slash() {
        let n = normalize_settings(&json!({ "endpoint": "http://a/v1/", "model": 5, "dictionary": ["a", 3, "a"], "paused": "yes", "timeoutMs": -1 }));
        assert_eq!(n.endpoint, "http://a/v1");
        assert_eq!(n.model, Settings::default().model);
        assert_eq!(n.dictionary, ["a"]);
        assert!(!n.paused);
        assert_eq!(n.timeout_ms, 10000);
    }

    #[test]
    fn invalid_hotkey_falls_back() {
        assert_eq!(normalize_settings(&json!({ "hotkey": { "key": "??", "mods": [] } })).hotkey, Settings::default().hotkey);
        assert_eq!(normalize_settings(&json!({ "hotkey": { "key": "a", "mods": ["cmd", "meta"] } })).hotkey, Settings::default().hotkey);
    }

    #[test]
    fn empty_object_normalizes_to_defaults() {
        assert_eq!(normalize_settings(&json!({})), Settings::default());
        assert_eq!(normalize_settings(&json!([1, 2])), Settings::default());
    }
}
