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

pub fn open_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check if running in Termux
    if std::env::var("TERMUX_VERSION").is_ok() {
        match Command::new("termux-open-url").arg(url).status() {
            Ok(status) if status.success() => return Ok(()),
            _ => {
                eprintln!("Failed to open URL with termux-open-url. Please open manually: {}", url);
                return Ok(());
            }
        }
    }
    
    // Check if running in WSL
    if is_wsl() {
        // Try cmd.exe first
        if let Ok(status) = Command::new("cmd.exe").args(["/c", "start", url]).status() {
            if status.success() {
                return Ok(());
            }
        }
        // Fallback to wslview
        if let Ok(status) = Command::new("wslview").arg(url).status() {
            if status.success() {
                return Ok(());
            }
        }
        eprintln!("Failed to open URL in WSL. Please open manually: {}", url);
        return Ok(());
    }
    
    // Check if running over SSH or in container
    if is_ssh() || is_container() {
        eprintln!("Running in SSH/container environment. Please open this URL manually: {}", url);
        return Ok(());
    }
    
    // OS-specific browser launch
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).status()?;
    }
    
    #[cfg(target_os = "linux")]
    {
        // Try xdg-open first
        if let Ok(status) = Command::new("xdg-open").arg(url).status() {
            if status.success() {
                return Ok(());
            }
        }
        // Fallback to specific browsers
        for browser in &["firefox", "chromium", "chrome", "google-chrome"] {
            if let Ok(status) = Command::new(browser).arg(url).status() {
                if status.success() {
                    return Ok(());
                }
            }
        }
        return Err("Failed to open browser on Linux".into());
    }
    
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/c", "start", url]).status()?;
    }
    
    Ok(())
}

fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|s| s.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn is_ssh() -> bool {
    std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok()
}

fn is_container() -> bool {
    std::path::Path::new("/.dockerenv").exists() || 
    std::fs::read_to_string("/proc/1/cgroup")
        .map(|s| s.contains("/docker") || s.contains("/containerd"))
        .unwrap_or(false)
}
