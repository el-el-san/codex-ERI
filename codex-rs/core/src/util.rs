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

// --- Cross‑platform URL opener and environment detection helpers ---

/// Attempt to open a URL in the default browser with environment‑aware behavior.
///
/// - Termux: uses `termux-open-url` when available.
/// - WSL: prefers `cmd.exe /C start` (Windows default browser), falls back to `wslview`.
/// - SSH/Container: does not auto‑open; logs the URL so the user can open manually.
/// - macOS: uses `open`.
/// - Linux: uses `xdg-open`, falling back to common browsers if necessary.
/// - Windows: uses `cmd /C start`.
///
/// Returns `Ok(())` if the request was handled (including the SSH/Container case
/// where we intentionally avoid auto‑opening). Returns `Err` only for genuine
/// failures where an auto‑open should have succeeded for the current platform.
pub fn open_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::process::Command;
    use tracing::{info, warn};

    if is_termux() {
        if which::which("termux-open-url").is_ok() {
            let status = Command::new("termux-open-url").arg(url).status();
            if matches!(status, Ok(s) if s.success()) {
                return Ok(());
            }
        }
        // Fall through to Linux handlers below if termux tool is unavailable.
    }

    if is_wsl() {
        // Prefer opening in the Windows host browser.
        let status = Command::new("cmd.exe")
            .args(["/C", "start", "", url])
            .status();
        if matches!(status, Ok(s) if s.success()) {
            return Ok(());
        }
        // Fallback: wslview if present
        if which::which("wslview").is_ok() {
            let status = Command::new("wslview").arg(url).status();
            if matches!(status, Ok(s) if s.success()) {
                return Ok(());
            }
        }
        return Err("failed to open URL via Windows host from WSL".into());
    }

    if is_ssh() || is_container() {
        // Avoid surprising behavior; just print the URL.
        warn!("Running over SSH or inside a container; not auto‑opening browser");
        info!("Open this URL in your browser: {url}");
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open").arg(url).status();
        if matches!(status, Ok(s) if s.success()) {
            return Ok(());
        }
        return Err("failed to open URL via macOS 'open'".into());
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd")
            .args(["/C", "start", "", url])
            .status();
        if matches!(status, Ok(s) if s.success()) {
            return Ok(());
        }
        return Err("failed to open URL via 'start'".into());
    }

    #[cfg(target_os = "linux")]
    {
        // First try xdg-open
        if which::which("xdg-open").is_ok() {
            let status = Command::new("xdg-open").arg(url).status();
            if matches!(status, Ok(s) if s.success()) {
                return Ok(());
            }
        }
        // Try a few common browsers as a fallback.
        for bin in ["gio", "xdg-open", "firefox", "google-chrome", "chromium", "brave-browser"] {
            if which::which(bin).is_ok() {
                let status = if bin == "gio" { // gio open <url>
                    Command::new("gio").args(["open", url]).status()
                } else {
                    Command::new(bin).arg(url).status()
                };
                if matches!(status, Ok(s) if s.success()) {
                    return Ok(());
                }
            }
        }
        return Err("failed to open URL on Linux".into());
    }

    #[allow(unreachable_code)]
    Err("unsupported target OS for open_url".into())
}

pub fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || std::env::var("PREFIX").map(|p| p.contains("com.termux")).unwrap_or(false)
        || std::env::var("ANDROID_ROOT").is_ok()
}

pub fn is_wsl() -> bool {
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        return true;
    }
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        return release.contains("Microsoft") || release.contains("microsoft");
    }
    false
}

pub fn is_ssh() -> bool {
    std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_TTY").is_ok()
}

pub fn is_container() -> bool {
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }
    if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
        let s = cgroup.to_ascii_lowercase();
        return s.contains("docker") || s.contains("kubepods") || s.contains("containerd");
    }
    false
}
