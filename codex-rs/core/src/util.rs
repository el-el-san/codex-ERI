use std::process::Command;
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tracing::debug;
use tracing::error;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

/// Status of URL opening attempt.
#[derive(Debug, Clone)]
pub enum OpenUrlStatus {
    /// URL was successfully opened.
    Opened,
    /// URL opening was suppressed due to environment constraints.
    Suppressed { reason: String },
}

/// Error that occurred while attempting to open URL.
#[derive(Debug, Clone, Error)]
pub enum OpenUrlError {
    #[error("Failed to execute command: {0}")]
    ExecutionFailed(String),
    #[error("No suitable browser command found")]
    NoBrowserFound,
}

/// Detect whether we are running under Termux.
#[allow(dead_code)]
fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || std::env::var("PREFIX").map_or(false, |p| p.contains("termux"))
}

/// Detect whether we are running under WSL.
#[allow(dead_code)]
fn is_wsl() -> bool {
    std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok()
}

/// Detect whether we are running under SSH.
#[allow(dead_code)]
fn is_ssh() -> bool {
    std::env::var("SSH_CONNECTION").is_ok()
        || std::env::var("SSH_CLIENT").is_ok()
        || std::env::var("SSH_TTY").is_ok()
}

/// Detect whether we are running inside a container.
#[allow(dead_code)]
fn is_container() -> bool {
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
        || std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
        || std::env::var("DOCKER_HOST").is_ok()
}

/// Open URL with appropriate command for the current environment.
pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        });
    }

    #[cfg(target_os = "android")]
    {
        if is_termux() {
            return match Command::new("termux-open-url").arg(url).status() {
                Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
                Ok(_) | Err(_) => Ok(OpenUrlStatus::Suppressed {
                    reason: format!(
                        "termux-open-url failed or not available. Please open the URL manually: {url}"
                    ),
                }),
            };
        }

        return Ok(OpenUrlStatus::Suppressed {
            reason: format!(
                "URL opening not supported on this Android environment. Please open the URL manually: {url}"
            ),
        });
    }

    #[cfg(target_os = "linux")]
    {
        if is_termux() {
            return match Command::new("termux-open-url").arg(url).status() {
                Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
                Ok(_) | Err(_) => Ok(OpenUrlStatus::Suppressed {
                    reason: format!(
                        "termux-open-url failed or not available. Please open the URL manually: {url}"
                    ),
                }),
            };
        }

        if is_wsl() {
            if let Ok(status) = Command::new("cmd.exe").args(["/c", "start", url]).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }

            if let Ok(status) = Command::new("wslview").arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }

            return Ok(OpenUrlStatus::Suppressed {
                reason: format!(
                    "Could not open browser in WSL. Please open the URL manually: {url}"
                ),
            });
        }

        if is_ssh() || is_container() {
            return Ok(OpenUrlStatus::Suppressed {
                reason: format!(
                    "Running in SSH/container environment. Please open the URL manually: {url}"
                ),
            });
        }

        if let Ok(browser) = std::env::var("BROWSER") {
            if let Ok(status) = Command::new(&browser).arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
        }

        if let Ok(status) = Command::new("xdg-open").arg(url).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        if let Ok(status) = Command::new("gio").args(["open", url]).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        if let Ok(status) = Command::new("sensible-browser").arg(url).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        for browser in ["firefox", "google-chrome", "chromium", "chromium-browser"] {
            if let Ok(status) = Command::new(browser).arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
        }

        return Ok(OpenUrlStatus::Suppressed {
            reason: format!("No suitable browser found. Please open the URL manually: {url}"),
        });
    }

    #[cfg(target_os = "macos")]
    {
        return match Command::new("open").arg(url).status() {
            Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
            Ok(_) | Err(_) => Ok(OpenUrlStatus::Suppressed {
                reason: format!(
                    "Failed to open URL with 'open' command. Please open manually: {url}"
                ),
            }),
        };
    }

    #[cfg(target_os = "windows")]
    {
        return match Command::new("cmd").args(["/C", "start", url]).status() {
            Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
            Ok(_) | Err(_) => Ok(OpenUrlStatus::Suppressed {
                reason: format!("Failed to open URL. Please open manually: {url}"),
            }),
        };
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        return Ok(OpenUrlStatus::Suppressed {
            reason: format!(
                "URL opening not supported on this platform. Please open manually: {url}"
            ),
        });
    }
}

pub(crate) fn error_or_panic(message: String) {
    if cfg!(debug_assertions) || env!("CARGO_PKG_VERSION").contains("alpha") {
        panic!("{message}");
    } else {
        error!("{message}");
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
}
