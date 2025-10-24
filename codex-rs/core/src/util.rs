use std::process::Command;
use std::time::Duration;

use rand::Rng;
use thiserror::Error;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

/// Status of URL opening attempt
#[derive(Debug, Clone)]
pub enum OpenUrlStatus {
    /// URL was successfully opened
    Opened,
    /// URL opening was suppressed due to environment constraints
    Suppressed { reason: String },
}

/// Error that occurred while attempting to open URL
#[derive(Debug, Clone, Error)]
pub enum OpenUrlError {
    #[error("Failed to execute command: {0}")]
    ExecutionFailed(String),
    #[error("No suitable browser command found")]
    NoBrowserFound,
}

/// Environment detection functions
#[allow(dead_code)]
fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok() || std::env::var("PREFIX").map_or(false, |p| p.contains("termux"))
}

#[allow(dead_code)]
fn is_wsl() -> bool {
    std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok()
}

#[allow(dead_code)]
fn is_ssh() -> bool {
    std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok()
}

#[allow(dead_code)]
fn is_container() -> bool {
    // Check for Docker, Kubernetes, and other container environments
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
        || std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
        || std::env::var("DOCKER_HOST").is_ok()
}

/// Open URL with appropriate command for the current environment
pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        });
    }

    #[cfg(target_os = "android")]
    {
        // Termux environment
        if is_termux() {
            return match Command::new("termux-open-url")
                .arg(url)
                .status()
            {
                Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
                _ => Ok(OpenUrlStatus::Suppressed {
                    reason: "termux-open-url failed or not available. Please open the URL manually: ".to_string() + url,
                }),
            };
        }
        // Non-Termux Android
        return Ok(OpenUrlStatus::Suppressed {
            reason: "URL opening not supported on this Android environment. Please open the URL manually: ".to_string() + url,
        });
    }

    #[cfg(target_os = "linux")]
    {
        // Termux on Linux (older versions or custom builds)
        if is_termux() {
            return match Command::new("termux-open-url")
                .arg(url)
                .status()
            {
                Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
                _ => Ok(OpenUrlStatus::Suppressed {
                    reason: "termux-open-url failed or not available. Please open the URL manually: ".to_string() + url,
                }),
            };
        }

        // WSL environment
        if is_wsl() {
            // Try cmd.exe first
            if let Ok(status) = Command::new("cmd.exe")
                .args(&["/c", "start", url])
                .status()
            {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }

            // Fallback to wslview
            if let Ok(status) = Command::new("wslview").arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }

            return Ok(OpenUrlStatus::Suppressed {
                reason: "Could not open browser in WSL. Please open the URL manually: ".to_string() + url,
            });
        }

        // SSH or container environment
        if is_ssh() || is_container() {
            return Ok(OpenUrlStatus::Suppressed {
                reason: "Running in SSH/container environment. Please open the URL manually: ".to_string() + url,
            });
        }

        // Linux desktop environment
        // Try BROWSER environment variable first
        if let Ok(browser) = std::env::var("BROWSER") {
            if let Ok(status) = Command::new(&browser).arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
        }

        // Try xdg-open
        if let Ok(status) = Command::new("xdg-open").arg(url).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        // Try gio open
        if let Ok(status) = Command::new("gio").args(&["open", url]).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        // Try sensible-browser
        if let Ok(status) = Command::new("sensible-browser").arg(url).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        // Try common browsers
        for browser in &["firefox", "google-chrome", "chromium", "chromium-browser"] {
            if let Ok(status) = Command::new(browser).arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
        }

        return Ok(OpenUrlStatus::Suppressed {
            reason: "No suitable browser found. Please open the URL manually: ".to_string() + url,
        });
    }

    #[cfg(target_os = "macos")]
    {
        match Command::new("open").arg(url).status() {
            Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
            _ => Ok(OpenUrlStatus::Suppressed {
                reason: "Failed to open URL with 'open' command. Please open manually: ".to_string() + url,
            }),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match Command::new("cmd")
            .args(&["/C", "start", url])
            .status()
        {
            Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
            _ => Ok(OpenUrlStatus::Suppressed {
                reason: "Failed to open URL. Please open manually: ".to_string() + url,
            }),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        Ok(OpenUrlStatus::Suppressed {
            reason: "URL opening not supported on this platform. Please open manually: ".to_string() + url,
        })
    }
}
