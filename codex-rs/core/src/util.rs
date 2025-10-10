use std::time::Duration;
use std::process::Command;

use rand::Rng;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

/// Status returned by `open_url` to indicate the result of the operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenUrlStatus {
    /// The URL was successfully opened.
    Opened,
    /// Opening the URL was suppressed due to environment constraints.
    Suppressed { reason: String },
}

/// Error type for `open_url` function.
#[derive(Debug, thiserror::Error)]
pub enum OpenUrlError {
    #[error("Failed to execute command: {0}")]
    ExecutionFailed(String),
    #[error("No suitable browser command found")]
    NoBrowserFound,
}

/// Opens a URL in the default browser, with environment-specific handling.
///
/// This function detects the current environment (Termux, WSL, SSH, Container, etc.)
/// and chooses the appropriate method to open the URL.
///
/// Returns:
/// - `Ok(OpenUrlStatus::Opened)` if the URL was successfully opened
/// - `Ok(OpenUrlStatus::Suppressed)` if opening was skipped due to environment constraints
/// - `Err(OpenUrlError)` if an error occurred
pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        });
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Termux: termux-open-url
        if is_termux() {
            return match Command::new("termux-open-url").arg(url).status() {
                Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
                Ok(_) => Err(OpenUrlError::ExecutionFailed(
                    "termux-open-url command failed. Make sure termux-api is installed (pkg install termux-api)".into()
                )),
                Err(e) => Err(OpenUrlError::ExecutionFailed(format!(
                    "Failed to run termux-open-url: {}. Install it with: pkg install termux-api", e
                ))),
            };
        }

        // WSL: cmd.exe /c start → 失敗時 wslview
        if is_wsl() {
            if let Ok(status) = Command::new("cmd.exe")
                .args(&["/c", "start", url])
                .status()
            {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
            // Fallback to wslview
            return match Command::new("wslview").arg(url).status() {
                Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
                _ => Err(OpenUrlError::ExecutionFailed(
                    "Both cmd.exe and wslview failed to open URL".into(),
                )),
            };
        }

        // SSH/Container: Suppress automatic opening
        if is_ssh() || is_container() {
            return Ok(OpenUrlStatus::Suppressed {
                reason: format!(
                    "Running in {} environment. Please open the URL manually: {}",
                    if is_ssh() { "SSH" } else { "container" },
                    url
                ),
            });
        }

        // Linux desktop: Try various methods
        // 1. Check BROWSER environment variable
        if let Ok(browser) = std::env::var("BROWSER") {
            if !browser.is_empty() {
                if let Ok(status) = Command::new(&browser).arg(url).status() {
                    if status.success() {
                        return Ok(OpenUrlStatus::Opened);
                    }
                }
            }
        }

        // 2. Try xdg-open
        if let Ok(status) = Command::new("xdg-open").arg(url).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        // 3. Try gio open
        if let Ok(status) = Command::new("gio").args(&["open", url]).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        // 4. Try sensible-browser
        if let Ok(status) = Command::new("sensible-browser").arg(url).status() {
            if status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }

        // 5. Try common browsers directly
        for browser in &["firefox", "google-chrome", "chromium", "chromium-browser"] {
            if let Ok(status) = Command::new(browser).arg(url).status() {
                if status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
        }

        Err(OpenUrlError::NoBrowserFound)
    }

    #[cfg(target_os = "macos")]
    {
        match Command::new("open").arg(url).status() {
            Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
            Ok(_) => Err(OpenUrlError::ExecutionFailed("open command failed".into())),
            Err(e) => Err(OpenUrlError::ExecutionFailed(format!("Failed to run open: {}", e))),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match Command::new("cmd")
            .args(&["/C", "start", url])
            .status()
        {
            Ok(status) if status.success() => Ok(OpenUrlStatus::Opened),
            Ok(_) => Err(OpenUrlError::ExecutionFailed("cmd /C start failed".into())),
            Err(e) => Err(OpenUrlError::ExecutionFailed(format!("Failed to run cmd: {}", e))),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "windows")))]
    {
        Ok(OpenUrlStatus::Suppressed {
            reason: format!("Unsupported platform. Please open the URL manually: {}", url),
        })
    }
}

/// Checks if the current environment is Termux (Android terminal emulator).
fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok() || std::env::var("PREFIX").map_or(false, |p| p.contains("com.termux"))
}

/// Checks if the current environment is WSL (Windows Subsystem for Linux).
fn is_wsl() -> bool {
    if let Ok(contents) = std::fs::read_to_string("/proc/version") {
        return contents.to_lowercase().contains("microsoft") || contents.to_lowercase().contains("wsl");
    }
    false
}

/// Checks if the current environment is an SSH session.
fn is_ssh() -> bool {
    std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_CLIENT").is_ok()
}

/// Checks if the current environment is a container (Docker, etc.).
fn is_container() -> bool {
    // Check for Docker
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }
    // Check for other container indicators
    if let Ok(contents) = std::fs::read_to_string("/proc/1/cgroup") {
        if contents.contains("docker") || contents.contains("lxc") || contents.contains("kubepods") {
            return true;
        }
    }
    false
}
