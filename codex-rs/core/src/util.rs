use std::time::Duration;

use rand::Rng;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

#[derive(Debug, Clone)]
pub enum OpenUrlStatus {
    Opened,
    Suppressed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum OpenUrlError {
    #[error("Failed to execute command: {0}")]
    CommandError(String),
    #[error("Environment error: {0}")]
    EnvironmentError(String),
}

/// Open URL in the default browser, handling various environment constraints.
pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        });
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Termux detection
        if is_termux() {
            if let Ok(output) = std::process::Command::new("termux-open-url")
                .arg(url)
                .output()
            {
                if output.status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
            return Ok(OpenUrlStatus::Suppressed {
                reason: "Termux environment detected but termux-open-url is not available. Please install termux-api: pkg install termux-api".into(),
            });
        }

        // WSL detection
        if is_wsl() {
            // Try cmd.exe first
            if let Ok(output) = std::process::Command::new("cmd.exe")
                .args(["/c", "start", url])
                .output()
            {
                if output.status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
            // Fallback to wslview
            if let Ok(output) = std::process::Command::new("wslview").arg(url).output() {
                if output.status.success() {
                    return Ok(OpenUrlStatus::Opened);
                }
            }
            return Ok(OpenUrlStatus::Suppressed {
                reason: "WSL environment detected but unable to open URL. Please manually open the URL in your browser.".into(),
            });
        }

        // SSH/Container detection - suppress automatic opening
        if is_ssh() || is_container() {
            return Ok(OpenUrlStatus::Suppressed {
                reason: "SSH/Container environment detected. Please manually open the URL in your browser.".into(),
            });
        }

        // Linux desktop environment - try various browser opening methods
        let browser_commands = [
            // Environment variable BROWSER
            std::env::var("BROWSER").ok().map(|b| vec![b]),
            // Standard Linux commands
            Some(vec!["xdg-open".to_string()]),
            Some(vec!["gio".to_string(), "open".to_string()]),
            Some(vec!["sensible-browser".to_string()]),
            // Direct browser executables
            Some(vec!["firefox".to_string()]),
            Some(vec!["google-chrome".to_string()]),
            Some(vec!["chromium".to_string()]),
            Some(vec!["chromium-browser".to_string()]),
        ];

        for browser_cmd in browser_commands.into_iter().flatten() {
            if let Some(cmd) = browser_cmd.first() {
                let mut command = std::process::Command::new(cmd);
                for arg in &browser_cmd[1..] {
                    command.arg(arg);
                }
                command.arg(url);

                if let Ok(output) = command.output() {
                    if output.status.success() {
                        return Ok(OpenUrlStatus::Opened);
                    }
                }
            }
        }

        return Err(OpenUrlError::CommandError(
            "No suitable browser command found".into(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("open").arg(url).output() {
            if output.status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }
        return Err(OpenUrlError::CommandError("Failed to open URL on macOS".into()));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .output()
        {
            if output.status.success() {
                return Ok(OpenUrlStatus::Opened);
            }
        }
        return Err(OpenUrlError::CommandError("Failed to open URL on Windows".into()));
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "windows")))]
    {
        Ok(OpenUrlStatus::Suppressed {
            reason: "Unsupported operating system".into(),
        })
    }
}

/// Detect if running in Termux environment
fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

/// Detect if running in WSL environment
fn is_wsl() -> bool {
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        version.contains("Microsoft") || version.contains("WSL")
    } else {
        false
    }
}

/// Detect if running in SSH session
fn is_ssh() -> bool {
    std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok()
}

/// Detect if running in container environment
fn is_container() -> bool {
    // Check for Docker
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }

    // Check for other container indicators
    if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
        if cgroup.contains("docker") || cgroup.contains("lxc") || cgroup.contains("kubepods") {
            return true;
        }
    }

    false
}
