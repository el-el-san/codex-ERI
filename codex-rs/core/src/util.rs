use std::env;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use codex_protocol::ThreadId;
use rand::Rng;
use tracing::debug;
use tracing::error;

use crate::parse_command::shlex_join;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

/// Emit structured feedback metadata as key/value pairs.
///
/// This logs a tracing event with `target: "feedback_tags"`. If
/// `codex_feedback::CodexFeedback::metadata_layer()` is installed, these fields are captured and
/// later attached as tags when feedback is uploaded.
///
/// Values are wrapped with [`tracing::field::DebugValue`], so the expression only needs to
/// implement [`std::fmt::Debug`].
///
/// Example:
///
/// ```rust
/// codex_core::feedback_tags!(model = "gpt-5", cached = true);
/// codex_core::feedback_tags!(provider = provider_id, request_id = request_id);
/// ```
#[macro_export]
macro_rules! feedback_tags {
    ($( $key:ident = $value:expr ),+ $(,)?) => {
        ::tracing::info!(
            target: "feedback_tags",
            $( $key = ::tracing::field::debug(&$value) ),+
        );
    };
}

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

pub(crate) fn error_or_panic(message: impl std::string::ToString) {
    if cfg!(debug_assertions) {
        panic!("{}", message.to_string());
    } else {
        error!("{}", message.to_string());
    }
}

pub(crate) fn try_parse_error_message(text: &str) -> String {
    debug!("Parsing server error response: {}", text);
    let json = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
    if let Some(error) = json.get("error")
        && let Some(message) = error.get("message")
        && let Some(message_str) = message.as_str()
    {
        return message_str.to_string();
    }
    if text.is_empty() {
        return "Unknown error".to_string();
    }
    text.to_string()
}

pub fn resolve_path(base: &Path, path: &PathBuf) -> PathBuf {
    if path.is_absolute() {
        path.clone()
    } else {
        base.join(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenUrlStatus {
    Opened,
    Suppressed { reason: String },
}

#[derive(Debug)]
pub struct OpenUrlError {
    message: String,
}

impl OpenUrlError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OpenUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OpenUrlError {}

pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".to_string(),
        });
    }

    open_url_platform(url)
}

pub fn is_termux() -> bool {
    env::var_os("TERMUX_VERSION").is_some() || env::var_os("TERMUX_APP_PID").is_some()
}

pub fn is_wsl() -> bool {
    env::var_os("WSL_INTEROP").is_some()
        || env::var_os("WSL_DISTRO_NAME").is_some()
        || env::var_os("WSLENV").is_some()
}

pub fn is_ssh() -> bool {
    env::var_os("SSH_CONNECTION").is_some()
        || env::var_os("SSH_CLIENT").is_some()
        || env::var_os("SSH_TTY").is_some()
}

pub fn is_container() -> bool {
    env::var_os("container").is_some()
        || Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_url_platform(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if is_termux() {
        return open_url_termux(url);
    }
    if is_wsl() {
        return open_url_wsl(url);
    }
    if is_ssh() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "SSH session detected; open the URL manually.".to_string(),
        });
    }
    if is_container() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "Container environment detected; open the URL manually.".to_string(),
        });
    }

    open_url_linux(url)
}

#[cfg(target_os = "macos")]
fn open_url_platform(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    match run_command("open", &[url])? {
        CommandOutcome::Success => Ok(OpenUrlStatus::Opened),
        CommandOutcome::Failure(message) => Err(OpenUrlError::new(message)),
        CommandOutcome::NotFound => Err(OpenUrlError::new("open command not found")),
    }
}

#[cfg(target_os = "windows")]
fn open_url_platform(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    match run_command("cmd", &["/C", "start", "", url])? {
        CommandOutcome::Success => Ok(OpenUrlStatus::Opened),
        CommandOutcome::Failure(message) => Err(OpenUrlError::new(message)),
        CommandOutcome::NotFound => Err(OpenUrlError::new("cmd command not found")),
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "windows"
)))]
fn open_url_platform(_url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    Ok(OpenUrlStatus::Suppressed {
        reason: "Unsupported platform; open the URL manually.".to_string(),
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_url_termux(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    match run_command("termux-open-url", &[url])? {
        CommandOutcome::Success => Ok(OpenUrlStatus::Opened),
        CommandOutcome::Failure(message) => Err(OpenUrlError::new(message)),
        CommandOutcome::NotFound => Err(OpenUrlError::new(
            "termux-open-url not found; install termux-api",
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_url_wsl(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    let mut failures = Vec::new();
    if let Some(status) = record_outcome(
        "cmd.exe",
        run_command("cmd.exe", &["/c", "start", "", url])?,
        &mut failures,
    ) {
        return Ok(status);
    }
    if let Some(status) = record_outcome("wslview", run_command("wslview", &[url])?, &mut failures)
    {
        return Ok(status);
    }

    Err(open_url_failed(failures))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_url_linux(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    let mut failures = Vec::new();

    for candidate in browser_env_candidates() {
        let program = &candidate[0];
        if let Some(status) = record_outcome(
            program,
            run_command_with_url(program, &candidate[1..], url)?,
            &mut failures,
        ) {
            return Ok(status);
        }
    }

    if let Some(status) =
        record_outcome("xdg-open", run_command("xdg-open", &[url])?, &mut failures)
    {
        return Ok(status);
    }
    if let Some(status) = record_outcome("gio", run_command("gio", &["open", url])?, &mut failures)
    {
        return Ok(status);
    }
    if let Some(status) = record_outcome(
        "sensible-browser",
        run_command("sensible-browser", &[url])?,
        &mut failures,
    ) {
        return Ok(status);
    }

    for browser in ["firefox", "google-chrome", "chromium", "chromium-browser"] {
        if let Some(status) = record_outcome(browser, run_command(browser, &[url])?, &mut failures)
        {
            return Ok(status);
        }
    }

    Err(open_url_failed(failures))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn browser_env_candidates() -> Vec<Vec<String>> {
    let Ok(value) = env::var("BROWSER") else {
        return Vec::new();
    };

    value
        .split(':')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            match shlex::split(entry) {
                Some(parts) if !parts.is_empty() => Some(parts),
                _ => Some(vec![entry.to_string()]),
            }
        })
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn run_command_with_url(
    program: &str,
    args: &[String],
    url: &str,
) -> Result<CommandOutcome, OpenUrlError> {
    let mut command = Command::new(program);
    command.args(args);
    command.arg(url);
    run_command_status(program, command)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_url_failed(failures: Vec<String>) -> OpenUrlError {
    if failures.is_empty() {
        OpenUrlError::new("No supported browser launcher found")
    } else {
        let failures = failures.join("; ");
        OpenUrlError::new(format!("Browser launch failed: {failures}"))
    }
}

enum CommandOutcome {
    Success,
    Failure(String),
    NotFound,
}

fn run_command(program: &str, args: &[&str]) -> Result<CommandOutcome, OpenUrlError> {
    let mut command = Command::new(program);
    command.args(args);
    run_command_status(program, command)
}

fn run_command_status(program: &str, mut command: Command) -> Result<CommandOutcome, OpenUrlError> {
    match command.status() {
        Ok(status) => {
            if status.success() {
                Ok(CommandOutcome::Success)
            } else {
                Ok(CommandOutcome::Failure(format!(
                    "{program} exited with {status}"
                )))
            }
        }
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                Ok(CommandOutcome::NotFound)
            } else {
                Err(OpenUrlError::new(format!("failed to run {program}: {err}")))
            }
        }
    }
}

fn record_outcome(
    program: &str,
    outcome: CommandOutcome,
    failures: &mut Vec<String>,
) -> Option<OpenUrlStatus> {
    match outcome {
        CommandOutcome::Success => Some(OpenUrlStatus::Opened),
        CommandOutcome::Failure(message) => {
            failures.push(message);
            None
        }
        CommandOutcome::NotFound => {
            failures.push(format!("{program} not found"));
            None
        }
    }
}

/// Trim a thread name and return `None` if it is empty after trimming.
pub fn normalize_thread_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn resume_command(thread_name: Option<&str>, thread_id: Option<ThreadId>) -> Option<String> {
    let resume_target = thread_name
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| thread_id.map(|thread_id| thread_id.to_string()));
    resume_target.map(|target| {
        let needs_double_dash = target.starts_with('-');
        let escaped = shlex_join(&[target]);
        if needs_double_dash {
            format!("codex resume -- {escaped}")
        } else {
            format!("codex resume {escaped}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_error_message() {
        let text = r#"{
  "error": {
    "message": "Your refresh token has already been used to generate a new access token. Please try signing in again.",
    "type": "invalid_request_error",
    "param": null,
    "code": "refresh_token_reused"
  }
}"#;
        let message = try_parse_error_message(text);
        assert_eq!(
            message,
            "Your refresh token has already been used to generate a new access token. Please try signing in again."
        );
    }

    #[test]
    fn test_try_parse_error_message_no_error() {
        let text = r#"{"message": "test"}"#;
        let message = try_parse_error_message(text);
        assert_eq!(message, r#"{"message": "test"}"#);
    }

    #[test]
    fn feedback_tags_macro_compiles() {
        #[derive(Debug)]
        struct OnlyDebug;

        feedback_tags!(model = "gpt-5", cached = true, debug_only = OnlyDebug);
    }

    #[test]
    fn normalize_thread_name_trims_and_rejects_empty() {
        assert_eq!(normalize_thread_name("   "), None);
        assert_eq!(
            normalize_thread_name("  my thread  "),
            Some("my thread".to_string())
        );
    }

    #[test]
    fn resume_command_prefers_name_over_id() {
        let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let command = resume_command(Some("my-thread"), Some(thread_id));
        assert_eq!(command, Some("codex resume my-thread".to_string()));
    }

    #[test]
    fn resume_command_with_only_id() {
        let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let command = resume_command(None, Some(thread_id));
        assert_eq!(
            command,
            Some("codex resume 123e4567-e89b-12d3-a456-426614174000".to_string())
        );
    }

    #[test]
    fn resume_command_with_no_name_or_id() {
        let command = resume_command(None, None);
        assert_eq!(command, None);
    }

    #[test]
    fn resume_command_quotes_thread_name_when_needed() {
        let command = resume_command(Some("-starts-with-dash"), None);
        assert_eq!(
            command,
            Some("codex resume -- -starts-with-dash".to_string())
        );

        let command = resume_command(Some("two words"), None);
        assert_eq!(command, Some("codex resume 'two words'".to_string()));

        let command = resume_command(Some("quote'case"), None);
        assert_eq!(command, Some("codex resume \"quote'case\"".to_string()));
    }
}
