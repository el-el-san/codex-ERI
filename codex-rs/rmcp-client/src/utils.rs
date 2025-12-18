use std::collections::HashMap;
use std::env;
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

#[derive(Debug, Clone)]
pub(crate) enum OpenUrlStatus {
    Opened,
    Suppressed { reason: String },
}

#[allow(dead_code)]
fn is_termux() -> bool {
    env::var("TERMUX_VERSION").is_ok() || env::var("PREFIX").map_or(false, |p| p.contains("termux"))
}

#[allow(dead_code)]
fn is_wsl() -> bool {
    env::var("WSL_DISTRO_NAME").is_ok() || env::var("WSL_INTEROP").is_ok()
}

#[allow(dead_code)]
fn is_ssh() -> bool {
    env::var("SSH_CONNECTION").is_ok()
        || env::var("SSH_CLIENT").is_ok()
        || env::var("SSH_TTY").is_ok()
}

#[allow(dead_code)]
fn is_container() -> bool {
    Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || env::var("KUBERNETES_SERVICE_HOST").is_ok()
        || env::var("DOCKER_HOST").is_ok()
}

pub(crate) fn open_url(url: &str) -> OpenUrlStatus {
    if url.is_empty() {
        return OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        };
    }

    #[cfg(target_os = "android")]
    {
        if is_termux() {
            return match Command::new("termux-open-url").arg(url).status() {
                Ok(status) if status.success() => OpenUrlStatus::Opened,
                Ok(_) | Err(_) => OpenUrlStatus::Suppressed {
                    reason: "termux-open-url failed or not available".into(),
                },
            };
        }

        return OpenUrlStatus::Suppressed {
            reason: "URL opening not supported on this Android environment".into(),
        };
    }

    #[cfg(target_os = "linux")]
    {
        if is_termux() {
            return match Command::new("termux-open-url").arg(url).status() {
                Ok(status) if status.success() => OpenUrlStatus::Opened,
                Ok(_) | Err(_) => OpenUrlStatus::Suppressed {
                    reason: "termux-open-url failed or not available".into(),
                },
            };
        }

        if is_wsl() {
            if let Ok(status) = Command::new("cmd.exe").args(["/c", "start", url]).status()
                && status.success()
            {
                return OpenUrlStatus::Opened;
            }

            if let Ok(status) = Command::new("wslview").arg(url).status()
                && status.success()
            {
                return OpenUrlStatus::Opened;
            }

            return OpenUrlStatus::Suppressed {
                reason: "Could not open browser in WSL".into(),
            };
        }

        if is_ssh() || is_container() {
            return OpenUrlStatus::Suppressed {
                reason: "Running in SSH/container environment".into(),
            };
        }

        if let Ok(browser) = env::var("BROWSER") {
            if let Ok(status) = Command::new(&browser).arg(url).status()
                && status.success()
            {
                return OpenUrlStatus::Opened;
            }
        }

        if let Ok(status) = Command::new("xdg-open").arg(url).status()
            && status.success()
        {
            return OpenUrlStatus::Opened;
        }

        if let Ok(status) = Command::new("gio").args(["open", url]).status()
            && status.success()
        {
            return OpenUrlStatus::Opened;
        }

        if let Ok(status) = Command::new("sensible-browser").arg(url).status()
            && status.success()
        {
            return OpenUrlStatus::Opened;
        }

        for browser in ["firefox", "google-chrome", "chromium", "chromium-browser"] {
            if let Ok(status) = Command::new(browser).arg(url).status()
                && status.success()
            {
                return OpenUrlStatus::Opened;
            }
        }

        return OpenUrlStatus::Suppressed {
            reason: "No suitable browser found".into(),
        };
    }

    #[cfg(target_os = "macos")]
    {
        return match Command::new("open").arg(url).status() {
            Ok(status) if status.success() => OpenUrlStatus::Opened,
            Ok(_) | Err(_) => OpenUrlStatus::Suppressed {
                reason: "Failed to open URL with 'open' command".into(),
            },
        };
    }

    #[cfg(target_os = "windows")]
    {
        return match Command::new("cmd").args(["/C", "start", url]).status() {
            Ok(status) if status.success() => OpenUrlStatus::Opened,
            Ok(_) | Err(_) => OpenUrlStatus::Suppressed {
                reason: "Failed to open URL".into(),
            },
        };
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        return OpenUrlStatus::Suppressed {
            reason: "URL opening not supported on this platform".into(),
        };
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
    // Termux/Android-specific environment variables
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
