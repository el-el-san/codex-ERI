use anyhow::Result;
use reqwest::ClientBuilder;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

pub(crate) fn create_env_for_mcp_server(
    extra_env: Option<HashMap<OsString, OsString>>,
    env_vars: &[String],
) -> HashMap<OsString, OsString> {
    DEFAULT_ENV_VARS
        .iter()
        .copied()
        .chain(env_vars.iter().map(String::as_str))
        .filter_map(|var| env::var_os(var).map(|value| (OsString::from(var), value)))
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

pub(crate) fn open_url(url: &str) -> Result<OpenUrlStatus> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".to_string(),
        });
    }

    open_url_platform(url).map_err(anyhow::Error::from)
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
        OpenUrlError::new(format!("Browser launch failed: {}", failures.join("; ")))
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
        Ok(status) if status.success() => Ok(CommandOutcome::Success),
        Ok(status) => Ok(CommandOutcome::Failure(format!(
            "{program} exited with {status}"
        ))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(CommandOutcome::NotFound),
        Err(err) => Err(OpenUrlError::new(format!("failed to run {program}: {err}"))),
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
    use pretty_assertions::assert_eq;

    use serial_test::serial;
    use std::ffi::OsStr;

    struct EnvVarGuard {
        key: String,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: impl AsRef<OsStr>) -> Self {
            let original = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value.as_ref());
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
        let expected = OsString::from(&value);
        let env = create_env_for_mcp_server(
            Some(HashMap::from([(OsString::from("TZ"), expected.clone())])),
            &[],
        );
        assert_eq!(env.get(OsStr::new("TZ")), Some(&expected));
    }

    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_includes_additional_whitelisted_variables() {
        let custom_var = "EXTRA_RMCP_ENV";
        let value = "from-env";
        let expected = OsString::from(value);
        let _guard = EnvVarGuard::set(custom_var, value);
        let env = create_env_for_mcp_server(/*extra_env*/ None, &[custom_var.to_string()]);
        assert_eq!(env.get(OsStr::new(custom_var)), Some(&expected));
    }

    #[cfg(unix)]
    #[test]
    #[serial(extra_rmcp_env)]
    fn create_env_preserves_path_when_it_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;

        let raw_path = std::ffi::OsStr::from_bytes(b"/tmp/codex-\xFF/bin");
        let expected = raw_path.to_os_string();
        let _guard = EnvVarGuard::set("PATH", raw_path);

        let env = create_env_for_mcp_server(/*extra_env*/ None, &[]);

        assert_eq!(env.get(OsStr::new("PATH")), Some(&expected));
    }
}
