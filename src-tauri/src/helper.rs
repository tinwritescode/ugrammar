use crate::helper_client::HotkeySlot;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{timeout_at, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperEvent {
    Hotkey(HotkeySlot),
    Restarted,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// Talks JSON lines to the Swift helper. Respawns it if it dies.
#[derive(Clone)]
pub struct Helper {
    stdin: Arc<tokio::sync::Mutex<Option<ChildStdin>>>,
    up: Arc<watch::Sender<bool>>,
    pending: Pending,
    next_id: Arc<AtomicU64>,
}

/// `{"event":"hotkey","slot":N}`; anything else is ignored.
fn parse_event(msg: &Value) -> Option<HelperEvent> {
    (msg["event"] == "hotkey").then_some(())?;
    HotkeySlot::from_wire(msg["slot"].as_u64()?).map(HelperEvent::Hotkey)
}

fn fail(error: &str) -> Value {
    json!({ "ok": false, "error": error })
}

impl Helper {
    pub fn start(path: PathBuf, events: mpsc::UnboundedSender<HelperEvent>) -> Self {
        let h = Helper {
            stdin: Arc::default(),
            up: Arc::new(watch::Sender::new(false)),
            pending: Arc::default(),
            next_id: Arc::new(AtomicU64::new(1)),
        };
        let run = h.clone();
        tauri::async_runtime::spawn(async move {
            let mut first = true;
            loop {
                if !first {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                run.run_once(&path, &events, first).await;
                first = false;
            }
        });
        h
    }

    async fn run_once(&self, path: &PathBuf, events: &mpsc::UnboundedSender<HelperEvent>, first: bool) {
        let child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else { return };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
        *self.stdin.lock().await = Some(stdin);
        self.up.send_replace(true);
        if !first {
            let _ = events.send(HelperEvent::Restarted);
        }
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            self.on_line(&line, events);
        }
        self.up.send_replace(false);
        *self.stdin.lock().await = None;
        let _ = child.wait().await;
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(fail("helper_exited"));
        }
    }

    fn on_line(&self, line: &str, events: &mpsc::UnboundedSender<HelperEvent>) {
        let Ok(msg) = serde_json::from_str::<Value>(line) else { return };
        if msg.get("event").is_some() {
            if let Some(e) = parse_event(&msg) {
                let _ = events.send(e);
            }
            return;
        }
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(msg);
            }
        }
    }

    /// Never errors: failures come back as `{ok:false,error}` like the helper's own replies.
    /// Waits (within `timeout`) for a helper that is still spawning or respawning.
    pub async fn request(&self, cmd: &str, args: Value, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        let up = matches!(timeout_at(deadline, self.up.subscribe().wait_for(|u| *u)).await, Ok(Ok(_)));
        if !up {
            return fail("helper_not_running");
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = match args {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        msg.insert("id".into(), id.into());
        msg.insert("cmd".into(), cmd.into());
        let mut line = Value::Object(msg).to_string();
        line.push('\n');
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        {
            let mut stdin = self.stdin.lock().await;
            let Some(w) = stdin.as_mut() else {
                self.pending.lock().unwrap().remove(&id);
                return fail("helper_not_running");
            };
            if w.write_all(line.as_bytes()).await.is_err() {
                self.pending.lock().unwrap().remove(&id);
                return fail("helper_not_running");
            }
        }
        match timeout_at(deadline, rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => fail("helper_exited"),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                fail("helper_timeout")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn talks_to_the_real_helper() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../helper/.build/release/grammar-helper");
        let (tx, _rx) = mpsc::unbounded_channel();
        let h = Helper::start(path, tx);
        let r = h.request("status", json!({}), Duration::from_secs(5)).await;
        assert_eq!(r["ok"], true, "status reply: {r}");
        assert!(r.get("trusted").is_some());
        let bad = h.request("no_such_cmd", json!({}), Duration::from_secs(2)).await;
        assert_eq!(bad["ok"], false);
    }

    #[test]
    fn hotkey_events_name_their_slot() {
        assert_eq!(parse_event(&json!({ "event": "hotkey", "slot": 1 })), Some(HelperEvent::Hotkey(HotkeySlot::Fix)));
        assert_eq!(parse_event(&json!({ "event": "hotkey", "slot": 2 })), Some(HelperEvent::Hotkey(HotkeySlot::Tone)));
        assert_eq!(parse_event(&json!({ "event": "hotkey", "slot": 7 })), None);
        assert_eq!(parse_event(&json!({ "event": "hotkey" })), None);
        assert_eq!(parse_event(&json!({ "event": "ready" })), None);
    }

    #[tokio::test]
    async fn missing_binary_reports_not_running() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let h = Helper::start(PathBuf::from("/nonexistent/helper"), tx);
        let r = h.request("status", json!({}), Duration::from_millis(200)).await;
        assert_eq!(r["error"], "helper_not_running");
    }
}
