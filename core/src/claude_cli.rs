//! Grammar fixes through the local Claude Code CLI: `claude -p --output-format json`, prompt on stdin.

use std::ffi::OsStr;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::engine::{build_messages, finish_check, CheckRequest, CheckResult, ConnectionStatus, EngineError, EngineErrorKind};

/// The CLI starts a Node-free binary but still logs in and warms up, so it never gets less than this.
pub const MIN_TIMEOUT_MS: u64 = 30_000;

/// The instructions ride in the prompt (as for the HTTP engine); this only replaces Claude Code's coding-agent prompt.
const SYSTEM_PROMPT: &str = "You are a text-processing function. Follow the instructions in the user's message exactly and write only the requested output.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliConfig {
    pub program: PathBuf,
    pub model: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCall {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Runs `call` to exit. `Err` means the process could not be started or talked to.
pub trait Runner: Send + Sync {
    fn run(&self, call: CliCall) -> impl Future<Output = Result<CliOutput, String>> + Send;
}

impl<F, Fut> Runner for F
where
    F: Fn(CliCall) -> Fut + Send + Sync,
    Fut: Future<Output = Result<CliOutput, String>> + Send,
{
    fn run(&self, call: CliCall) -> impl Future<Output = Result<CliOutput, String>> + Send {
        self(call)
    }
}

#[derive(Default)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    async fn run(&self, call: CliCall) -> Result<CliOutput, String> {
        let mut child = tokio::process::Command::new(&call.program)
            .args(&call.args)
            // Away from any project, so no CLAUDE.md or project settings are picked up.
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Could not run {}: {e}", call.program.display()))?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin.write_all(call.stdin.as_bytes()).await.map_err(|e| e.to_string())?;
        drop(stdin);
        let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
        Ok(CliOutput { stdout: String::from_utf8_lossy(&out.stdout).into(), stderr: String::from_utf8_lossy(&out.stderr).into() })
    }
}

/// `setting` when set; else the first `claude` on `path_env`, then the installer's usual places. A GUI app's PATH
/// rarely includes `~/.local/bin`, so the bare name is only the last resort.
pub fn resolve_program(setting: &str, home: Option<&Path>, path_env: Option<&OsStr>, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if !setting.is_empty() {
        return match (setting.strip_prefix("~/"), home) {
            (Some(rest), Some(h)) => h.join(rest),
            _ => PathBuf::from(setting),
        };
    }
    let on_path = path_env.into_iter().flat_map(std::env::split_paths).map(|d| d.join("claude"));
    let installed = home.into_iter().flat_map(|h| [h.join(".local/bin/claude"), h.join(".claude/local/claude")]);
    let system = ["/opt/homebrew/bin/claude", "/usr/local/bin/claude"].map(PathBuf::from);
    on_path.chain(installed).chain(system).find(|p| exists(p)).unwrap_or_else(|| PathBuf::from("claude"))
}

pub fn args(model: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-p", "--output-format", "json", "--model", model, "--system-prompt", SYSTEM_PROMPT, "--tools", ""]
        .map(String::from)
        .into();
    // No tools, MCP servers, user/project settings, hooks, slash commands or saved sessions: a pure text call.
    a.extend(["--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands", "--no-session-persistence"].map(String::from));
    a
}

/// The model's text from `--output-format json`. A CLI-reported error becomes its message.
pub fn parse_output(out: &CliOutput) -> Result<String, EngineError> {
    let v: Option<Value> = serde_json::from_str(out.stdout.trim()).ok();
    let result = v.as_ref().and_then(|v| v.get("result")).and_then(Value::as_str);
    match (v.as_ref().and_then(|v| v.get("is_error")).and_then(Value::as_bool), result) {
        (Some(false), Some(text)) => Ok(text.to_string()),
        (Some(true), Some(msg)) => Err(EngineError::new(EngineErrorKind::Process, msg)),
        _ => {
            let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("no result");
            Err(EngineError::new(EngineErrorKind::Process, format!("claude CLI failed: {why}")))
        }
    }
}

pub struct ClaudeCliEngine<R = ProcessRunner> {
    cfg: ClaudeCliConfig,
    runner: R,
}

impl ClaudeCliEngine {
    pub fn new(cfg: ClaudeCliConfig) -> Self {
        Self::with_runner(cfg, ProcessRunner)
    }
}

impl<R: Runner> ClaudeCliEngine<R> {
    pub fn with_runner(cfg: ClaudeCliConfig, runner: R) -> Self {
        Self { cfg, runner }
    }

    async fn ask(&self, prompt: String) -> Result<String, EngineError> {
        let call = CliCall { program: self.cfg.program.clone(), args: args(&self.cfg.model), stdin: prompt };
        let ms = self.cfg.timeout_ms.max(MIN_TIMEOUT_MS);
        match tokio::time::timeout(Duration::from_millis(ms), self.runner.run(call)).await {
            Err(_) => Err(EngineError::new(EngineErrorKind::Timeout, format!("Timed out after {}s", ms as f64 / 1000.0))),
            Ok(Err(msg)) => Err(EngineError::new(EngineErrorKind::Process, msg)),
            Ok(Ok(out)) => parse_output(&out),
        }
    }

    pub async fn check(&self, req: &CheckRequest) -> Result<CheckResult, EngineError> {
        let mut prompt = build_messages(&req.text, &req.dictionary, req.explain, req.explain_lang.as_deref(), req.tone);
        let raw = self.ask(prompt.remove(0).content).await?;
        finish_check(req, &raw)
    }

    pub async fn test_connection(&self) -> ConnectionStatus {
        match self.ask("Reply with the single word: pong".into()).await {
            Ok(_) => ConnectionStatus { ok: true, message: format!("Connected (claude CLI, {})", self.cfg.model) },
            Err(e) => ConnectionStatus { ok: false, message: e.message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn cfg() -> ClaudeCliConfig {
        ClaudeCliConfig { program: "/bin/claude".into(), model: "haiku".into(), timeout_ms: 1000 }
    }

    fn reply(result: &str, is_error: bool) -> Result<CliOutput, String> {
        Ok(CliOutput { stdout: json!({ "type": "result", "is_error": is_error, "result": result }).to_string(), stderr: String::new() })
    }

    fn req(text: &str) -> CheckRequest {
        CheckRequest { text: text.into(), ..Default::default() }
    }

    #[tokio::test]
    async fn check_sends_the_prompt_on_stdin_and_returns_the_corrected_text() {
        let seen = Arc::new(Mutex::new(None));
        let rec = seen.clone();
        let e = ClaudeCliEngine::with_runner(cfg(), move |c: CliCall| {
            *rec.lock().unwrap() = Some(c);
            async { reply("<corrected>I have an apple.</corrected>", false) }
        });
        let r = e.check(&CheckRequest { dictionary: vec!["Xpiki".into()], ..req("I has a apple.") }).await.unwrap();
        assert_eq!((r.original.as_str(), r.corrected.as_str()), ("I has a apple.", "I have an apple."));
        let call = seen.lock().unwrap().clone().unwrap();
        assert_eq!(call.program, PathBuf::from("/bin/claude"));
        assert_eq!(call.stdin, build_messages("I has a apple.", &["Xpiki".into()], false, None, None)[0].content);
        assert_eq!(call.args[..5], ["-p", "--output-format", "json", "--model", "haiku"]);
        let tools = call.args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(call.args[tools + 1], "", "no tools: the text is never instructions to act on");
    }

    #[tokio::test]
    async fn check_shares_the_http_engines_output_rules() {
        let e = ClaudeCliEngine::with_runner(cfg(), |_| async { reply("<corrected>I have a cat.</corrected>\n<notes>\n- has → have: chủ ngữ\n</notes>", false) });
        let r = e.check(&CheckRequest { explain: true, ..req("I has a cat.") }).await.unwrap();
        assert_eq!(r.notes, Some(vec!["has → have: chủ ngữ".to_string()]));
        let e = ClaudeCliEngine::with_runner(cfg(), |_| async { reply("   ", false) });
        assert_eq!(e.check(&req("a")).await.unwrap_err(), EngineError::new(EngineErrorKind::BadResponse, "Empty response"));
    }

    #[tokio::test]
    async fn cli_reported_error_surfaces_its_message() {
        let e = ClaudeCliEngine::with_runner(cfg(), |_| async { reply("There's an issue with the selected model (x).", true) });
        assert_eq!(e.check(&req("a")).await.unwrap_err(), EngineError::new(EngineErrorKind::Process, "There's an issue with the selected model (x)."));
    }

    #[tokio::test]
    async fn non_json_output_reports_the_last_stderr_line() {
        let e = ClaudeCliEngine::with_runner(cfg(), |_| async { Ok(CliOutput { stdout: "".into(), stderr: "boot\nInvalid API key · Please run /login\n\n".into() }) });
        assert_eq!(e.check(&req("a")).await.unwrap_err().message, "claude CLI failed: Invalid API key · Please run /login");
        let e = ClaudeCliEngine::with_runner(cfg(), |_| async { Ok(CliOutput { stdout: "{}".into(), stderr: "".into() }) });
        assert_eq!(e.check(&req("a")).await.unwrap_err().message, "claude CLI failed: no result");
    }

    #[tokio::test]
    async fn spawn_failure_is_a_process_error() {
        let e = ClaudeCliEngine::with_runner(cfg(), |_| async { Err::<CliOutput, _>("Could not run /bin/claude: No such file".to_string()) });
        assert_eq!(e.check(&req("a")).await.unwrap_err(), EngineError::new(EngineErrorKind::Process, "Could not run /bin/claude: No such file"));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_never_drops_below_the_cli_floor() {
        let e = ClaudeCliEngine::with_runner(cfg(), |_| std::future::pending());
        assert_eq!(e.check(&req("a")).await.unwrap_err(), EngineError::new(EngineErrorKind::Timeout, "Timed out after 30s"));
    }

    #[tokio::test]
    async fn test_connection_reports_model_or_error() {
        let good = ClaudeCliEngine::with_runner(cfg(), |_| async { reply("pong", false) });
        assert_eq!(good.test_connection().await, ConnectionStatus { ok: true, message: "Connected (claude CLI, haiku)".into() });
        let bad = ClaudeCliEngine::with_runner(cfg(), |_| async { reply("Not logged in", true) });
        assert_eq!(bad.test_connection().await, ConnectionStatus { ok: false, message: "Not logged in".into() });
    }

    #[test]
    fn resolve_prefers_setting_then_path_then_install_places() {
        let home = Path::new("/Users/u");
        let none = |_: &Path| false;
        assert_eq!(resolve_program("/x/claude", Some(home), None, none), PathBuf::from("/x/claude"));
        assert_eq!(resolve_program("~/bin/claude", Some(home), None, none), PathBuf::from("/Users/u/bin/claude"));
        let path = OsStr::new("/a:/b");
        assert_eq!(resolve_program("", Some(home), Some(path), |p| p == Path::new("/b/claude")), PathBuf::from("/b/claude"));
        let local = |p: &Path| p == Path::new("/Users/u/.local/bin/claude") || p == Path::new("/opt/homebrew/bin/claude");
        assert_eq!(resolve_program("", Some(home), Some(path), local), PathBuf::from("/Users/u/.local/bin/claude"));
        assert_eq!(resolve_program("", Some(home), Some(path), none), PathBuf::from("claude"));
    }

    /// Talks to the real CLI: `UGRAMMAR_LIVE_CLAUDE=1 cargo test live_claude -- --nocapture`.
    #[tokio::test]
    async fn live_claude_fixes_grammar() {
        if std::env::var_os("UGRAMMAR_LIVE_CLAUDE").is_none() {
            return;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let program = resolve_program("", home.as_deref(), std::env::var_os("PATH").as_deref(), Path::exists);
        let e = ClaudeCliEngine::new(ClaudeCliConfig { program, model: "haiku".into(), timeout_ms: 60_000 });
        let r = e.check(&req("I has a apple.")).await.unwrap();
        println!("{r:?}");
        assert_eq!(r.corrected, "I have an apple.");
    }
}
