use std::collections::HashMap;
use std::env;
use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use mcp_types::CallToolResult;
use reqwest::ClientBuilder;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use rmcp::model::CallToolResult as RmcpCallToolResult;
use rmcp::service::ServiceError;
use serde_json::Value;
use tokio::time;

pub(crate) async fn run_with_timeout<F, T>(
    fut: F,
    timeout: Option<Duration>,
    label: &str,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T, ServiceError>>,
{
    if let Some(duration) = timeout {
        let result = time::timeout(duration, fut)
            .await
            .with_context(|| anyhow!("timed out awaiting {label} after {duration:?}"))?;
        result.map_err(|err| anyhow!("{label} failed: {err}"))
    } else {
        fut.await.map_err(|err| anyhow!("{label} failed: {err}"))
    }
}

pub(crate) fn convert_call_tool_result(result: RmcpCallToolResult) -> Result<CallToolResult> {
    let mut value = serde_json::to_value(result)?;
    if let Some(obj) = value.as_object_mut()
        && (obj.get("content").is_none()
            || obj.get("content").is_some_and(serde_json::Value::is_null))
    {
        obj.insert("content".to_string(), Value::Array(Vec::new()));
    }
    serde_json::from_value(value).context("failed to convert call tool result")
}

/// Convert from mcp-types to Rust SDK types.
///
/// The Rust SDK types are the same as our mcp-types crate because they are both
/// derived from the same MCP specification.
/// As a result, it should be safe to convert directly from one to the other.
pub(crate) fn convert_to_rmcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

/// Convert from Rust SDK types to mcp-types.
///
/// The Rust SDK types are the same as our mcp-types crate because they are both
/// derived from the same MCP specification.
/// As a result, it should be safe to convert directly from one to the other.
pub(crate) fn convert_to_mcp<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value)?;
    serde_json::from_value(json).map_err(|err| anyhow!(err))
}

pub(crate) fn create_env_for_mcp_server(
    extra_env: Option<HashMap<String, String>>,
    env_vars: &[String],
) -> HashMap<String, String> {
    DEFAULT_ENV_VARS
        .iter()
        .copied()
        .chain(env_vars.iter().map(String::as_str))
        .filter_map(|var| env::var(var).ok().map(|value| (var.to_string(), value)))
        .chain(extra_env.unwrap_or_default())
        .collect()
}

pub(crate) fn build_default_headers(
    http_headers: Option<HashMap<String, String>>,
    env_http_headers: Option<HashMap<String, String>>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();

    if let Some(static_headers) = http_headers {
        for (name, value) in static_headers {
            let header_name = match HeaderName::from_bytes(name.as_bytes()) {
                Ok(name) => name,
                Err(err) => {
                    tracing::warn!("invalid HTTP header name `{name}`: {err}");
                    continue;
                }
            };
            let header_value = match HeaderValue::from_str(value.as_str()) {
                Ok(value) => value,
                Err(err) => {
                    tracing::warn!("invalid HTTP header value for `{name}`: {err}");
                    continue;
                }
            };
            headers.insert(header_name, header_value);
        }
    }

    if let Some(env_headers) = env_http_headers {
        for (name, env_var) in env_headers {
            if let Ok(value) = env::var(&env_var) {
                if value.trim().is_empty() {
                    continue;
                }

                let header_name = match HeaderName::from_bytes(name.as_bytes()) {
                    Ok(name) => name,
                    Err(err) => {
                        tracing::warn!("invalid HTTP header name `{name}`: {err}");
                        continue;
                    }
                };

                let header_value = match HeaderValue::from_str(value.as_str()) {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::warn!(
                            "invalid HTTP header value read from {env_var} for `{name}`: {err}"
                        );
                        continue;
                    }
                };
                headers.insert(header_name, header_value);
            }
        }
    }

    Ok(headers)
}

pub(crate) fn apply_default_headers(
    builder: ClientBuilder,
    default_headers: &HeaderMap,
) -> ClientBuilder {
    if default_headers.is_empty() {
        builder
    } else {
        builder.default_headers(default_headers.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenUrlStatus {
    Opened,
    Suppressed { reason: String },
}

#[derive(Debug)]
pub(crate) struct OpenUrlError {
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

pub(crate) fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".to_string(),
        });
    }

    open_url_platform(url)
}

fn is_termux() -> bool {
    env::var_os("TERMUX_VERSION").is_some() || env::var_os("TERMUX_APP_PID").is_some()
}

fn is_wsl() -> bool {
    env::var_os("WSL_INTEROP").is_some()
        || env::var_os("WSL_DISTRO_NAME").is_some()
        || env::var_os("WSLENV").is_some()
}

fn is_ssh() -> bool {
    env::var_os("SSH_CONNECTION").is_some()
        || env::var_os("SSH_CLIENT").is_some()
        || env::var_os("SSH_TTY").is_some()
}

fn is_container() -> bool {
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

#[cfg(unix)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] = &[
    "HOME",
    "LOGNAME",
    "PATH",
    "SHELL",
    "USER",
    "__CF_USER_TEXT_ENCODING",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "TZ",
    "TERMUX_VERSION",
    "PREFIX",
    "TERMUX_APK_RELEASE",
    "TERMUX_APP_PID",
    "ANDROID_ROOT",
    "ANDROID_DATA",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
];

#[cfg(windows)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] = &[
    // Core path resolution
    "PATH",
    "PATHEXT",
    // Shell and system roots
    "COMSPEC",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    // User context and profiles
    "USERNAME",
    "USERDOMAIN",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    // Program locations
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "PROGRAMDATA",
    // App data and caches
    "LOCALAPPDATA",
    "APPDATA",
    // Temp locations
    "TEMP",
    "TMP",
    // Common shells/pwsh hints
    "POWERSHELL",
    "PWSH",
];

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_types::ContentBlock;
    use pretty_assertions::assert_eq;
    use rmcp::model::CallToolResult as RmcpCallToolResult;
    use serde_json::json;

    use serial_test::serial;
    use std::ffi::OsString;

    struct EnvVarGuard {
        key: String,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let original = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                unsafe {
                    std::env::set_var(&self.key, value);
                }
            } else {
                unsafe {
                    std::env::remove_var(&self.key);
                }
            }
        }
    }

    #[tokio::test]
    async fn create_env_honors_overrides() {
        let value = "custom".to_string();
        let env =
            create_env_for_mcp_server(Some(HashMap::from([("TZ".into(), value.clone())])), &[]);
        assert_eq!(env.get("TZ"), Some(&value));
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_includes_additional_whitelisted_variables() {
        let custom_var = "EXTRA_RMCP_ENV";
        let value = "from-env";
        let _guard = EnvVarGuard::set(custom_var, value);
        let env = create_env_for_mcp_server(None, &[custom_var.to_string()]);
        assert_eq!(env.get(custom_var), Some(&value.to_string()));
    }

    #[test]
    fn convert_call_tool_result_defaults_missing_content() -> Result<()> {
        let structured_content = json!({ "key": "value" });
        let rmcp_result = RmcpCallToolResult {
            content: vec![],
            structured_content: Some(structured_content.clone()),
            is_error: Some(true),
            meta: None,
        };

        let result = convert_call_tool_result(rmcp_result)?;

        assert!(result.content.is_empty());
        assert_eq!(result.structured_content, Some(structured_content));
        assert_eq!(result.is_error, Some(true));

        Ok(())
    }

    #[test]
    fn convert_call_tool_result_preserves_existing_content() -> Result<()> {
        let rmcp_result = RmcpCallToolResult::success(vec![rmcp::model::Content::text("hello")]);

        let result = convert_call_tool_result(rmcp_result)?;

        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            ContentBlock::TextContent(text_content) => {
                assert_eq!(text_content.text, "hello");
                assert_eq!(text_content.r#type, "text");
            }
            other => panic!("expected text content got {other:?}"),
        }
        assert_eq!(result.structured_content, None);
        assert_eq!(result.is_error, Some(false));

        Ok(())
    }
}
