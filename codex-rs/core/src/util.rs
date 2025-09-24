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

/// Status returned by [`open_url`].
#[derive(Debug, Clone)]
pub enum OpenUrlStatus {
    /// URL was successfully opened.
    Opened,
    /// URL opening was suppressed due to environment constraints.
    Suppressed { reason: String },
}

/// Error returned by [`open_url`].
#[derive(Debug, thiserror::Error)]
pub enum OpenUrlError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command failed: {0}")]
    CommandFailed(String),
}

/// Opens the given URL using the appropriate method for the current environment.
///
/// Returns `OpenUrlStatus::Suppressed` for environments where automatic URL opening
/// is not desirable (SSH, containers) or not possible. Returns `OpenUrlError` only
/// for actual execution failures.
pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        });
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Check for Termux first
        if is_termux() {
            return run_command("termux-open-url", &[url]);
        }

        // Check for WSL
        if is_wsl() {
            // Try cmd.exe first, fallback to wslview
            match run_command("cmd.exe", &["/c", "start", url]) {
                Ok(status) => return Ok(status),
                Err(_) => {
                    return run_command("wslview", &[url]);
                }
            }
        }

        // Check for SSH/Container environments - suppress automatic opening
        if is_ssh() || is_container() {
            return Ok(OpenUrlStatus::Suppressed {
                reason: "Automatic URL opening is disabled in SSH/container environments. Please open the URL manually.".into(),
            });
        }

        // Linux desktop environment - try various methods
        if let Ok(browser) = std::env::var("BROWSER") {
            if let Ok(status) = run_command(&browser, &[url]) {
                return Ok(status);
            }
        }

        // Try common Linux URL openers
        let commands = [
            ("xdg-open", vec![url]),
            ("gio", vec!["open", url]),
            ("sensible-browser", vec![url]),
            ("firefox", vec![url]),
            ("google-chrome", vec![url]),
            ("chromium-browser", vec![url]),
            ("chromium", vec![url]),
        ];

        for (cmd, args) in commands {
            if let Ok(status) = run_command(cmd, &args) {
                return Ok(status);
            }
        }

        Err(OpenUrlError::CommandFailed("No suitable browser found".into()))
    }

    #[cfg(target_os = "macos")]
    {
        run_command("open", &[url])
    }

    #[cfg(target_os = "windows")]
    {
        run_command("cmd", &["/C", "start", "", url])
    }
}

/// Helper function to run a command and return appropriate status.
fn run_command(cmd: &str, args: &[&str]) -> Result<OpenUrlStatus, OpenUrlError> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(OpenUrlError::Io)?;

    if output.status.success() {
        Ok(OpenUrlStatus::Opened)
    } else {
        Err(OpenUrlError::CommandFailed(format!(
            "Command '{}' failed with exit code: {:?}",
            cmd, output.status.code()
        )))
    }
}

/// Check if we're running in Termux (Android).
fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

/// Check if we're running in WSL.
fn is_wsl() -> bool {
    std::env::var("WSL_DISTRO_NAME").is_ok() ||
    std::env::var("WSLENV").is_ok() ||
    std::fs::read_to_string("/proc/version")
        .map(|content| content.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

/// Check if we're running in an SSH session.
fn is_ssh() -> bool {
    std::env::var("SSH_CONNECTION").is_ok() ||
    std::env::var("SSH_CLIENT").is_ok() ||
    std::env::var("SSH_TTY").is_ok()
}

/// Check if we're running in a container.
fn is_container() -> bool {
    std::fs::read_to_string("/proc/1/cgroup")
        .map(|content| content.contains("docker") || content.contains("containerd") || content.contains("lxc"))
        .unwrap_or(false) ||
    std::path::Path::new("/.dockerenv").exists() ||
    std::env::var("container").is_ok()
}
