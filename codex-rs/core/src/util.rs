use std::path::Path;
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

/// Return `true` if the project folder specified by the `Config` is inside a
/// Git repository.
///
/// The check walks up the directory hierarchy looking for a `.git` file or
/// directory (note `.git` can be a file that contains a `gitdir` entry). This
/// approach does **not** require the `git` binary or the `git2` crate and is
/// therefore fairly lightweight.
///
/// Note that this does **not** detect *work‑trees* created with
/// `git worktree add` where the checkout lives outside the main repository
/// directory. If you need Codex to work from such a checkout simply pass the
/// `--allow-no-git-exec` CLI flag that disables the repo requirement.
pub fn is_inside_git_repo(base_dir: &Path) -> bool {
    let mut dir = base_dir.to_path_buf();

    loop {
        if dir.join(".git").exists() {
            return true;
        }

        // Pop one component (go up one directory).  `pop` returns false when
        // we have reached the filesystem root.
        if !dir.pop() {
            break;
        }
    }

    false
}

/// Opens a URL in the default browser with environment-aware logic
pub fn open_url(url: &str) -> Result<(), String> {
    // Check if running in Termux
    if std::env::var("TERMUX_VERSION").is_ok() {
        return Command::new("termux-open-url")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open URL in Termux: {}", e));
    }

    // Check if running in WSL
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        // Try cmd.exe first
        if Command::new("cmd.exe")
            .args(&["/c", "start", "", url])
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
        // Fallback to wslview
        return Command::new("wslview")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open URL in WSL: {}", e));
    }

    // Check if running in SSH or container (no display)
    if std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok() {
        return Err(format!("Running in SSH session. Please open the URL manually: {}", url));
    }
    
    if std::env::var("DISPLAY").is_err() && !cfg!(target_os = "macos") && !cfg!(target_os = "windows") {
        return Err(format!("No display detected (container/headless). Please open the URL manually: {}", url));
    }

    // OS-specific browser launch
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open URL on macOS: {}", e))
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(&["/c", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open URL on Windows: {}", e))
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        // Linux and other Unix-like systems
        let browsers = ["xdg-open", "firefox", "chromium", "chrome", "sensible-browser"];
        for browser in &browsers {
            if Command::new(browser).arg(url).spawn().is_ok() {
                return Ok(());
            }
        }
        Err(format!("Failed to open URL. Please open manually: {}", url))
    }
}
