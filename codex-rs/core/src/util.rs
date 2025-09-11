use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
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

/// Try to open a URL in the default browser with environment-aware behavior.
///
/// - Termux(Android): uses `termux-open-url`
/// - WSL: `cmd.exe /C start` (fallback to `wslview`)
/// - SSH/Container: do not auto-open (return an error so callers can ignore)
/// - macOS: `open`
/// - Linux: `xdg-open`
/// - Windows: `cmd /C start`
///
/// Callers that don't want failures to be fatal (e.g. login flow) can safely
/// ignore the `Result` to allow manual opening.
pub fn open_url(url: &str) -> io::Result<()> {
    if is_termux() {
        return Command::new("termux-open-url")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(io::Error::other);
    }

    if is_wsl() {
        // Prefer Windows default handler via cmd.exe; if that fails, try wslview
        let cmd_res = Command::new("cmd.exe")
            .args(["/C", "start", "", url])
            .spawn();
        return match cmd_res {
            Ok(_) => Ok(()),
            Err(_) => Command::new("wslview")
                .arg(url)
                .spawn()
                .map(|_| ())
                .map_err(io::Error::other),
        };
    }

    if is_ssh() || is_container() {
        // Suppress auto-open; let the caller handle informing the user.
        return Err(io::Error::other(
            "auto-open suppressed in SSH/container environment",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        return Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(io::Error::other);
    }

    #[cfg(target_os = "linux")]
    {
        return Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(io::Error::other);
    }

    #[cfg(target_os = "windows")]
    {
        return Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(io::Error::other);
    }

    #[allow(unreachable_code)]
    Err(io::Error::other("unsupported platform for open_url"))
}

/// Heuristic check for running inside WSL.
pub fn is_wsl() -> bool {
    if std::env::var("WSL_DISTRO_NAME").is_ok() {
        return true;
    }
    if let Ok(release) = fs::read_to_string("/proc/sys/kernel/osrelease") {
        let lower = release.to_ascii_lowercase();
        return lower.contains("microsoft") || lower.contains("wsl");
    }
    false
}

/// Heuristic check for Android/Termux environment.
pub fn is_termux() -> bool {
    if std::env::var("TERMUX_VERSION").is_ok() {
        return true;
    }
    if let Ok(prefix) = std::env::var("PREFIX") {
        return prefix.contains("/data/data/com.termux");
    }
    false
}

/// Heuristic check for SSH session.
pub fn is_ssh() -> bool {
    std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_TTY").is_ok()
}

/// Heuristic check for common container environments.
pub fn is_container() -> bool {
    ["/.dockerenv", "/run/.containerenv"]
        .into_iter()
        .any(|p| Path::new(p).exists())
        || std::env::var("container").is_ok()
        || fs::read_to_string("/proc/1/cgroup")
            .map(|s| {
                let ls = s.to_ascii_lowercase();
                ls.contains("docker") || ls.contains("kubepods") || ls.contains("containerd")
            })
            .unwrap_or(false)
}
