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

/// Open a URL in the browser, with platform-specific handling
pub fn open_url(url: &str) -> Result<(), String> {
    // Check for Termux environment
    if std::env::var("PREFIX").ok().as_deref() == Some("/data/data/com.termux/files/usr") {
        // Termux environment detected
        match Command::new("termux-open-url").arg(url).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Failed to open URL with termux-open-url: {}", e);
                eprintln!("Please open the following URL manually: {}", url);
                return Err(format!("Failed to open URL: {}", e));
            }
        }
    }
    
    // Check for WSL environment
    if let Ok(wsl_distro) = std::env::var("WSL_DISTRO_NAME") {
        // WSL environment detected
        if let Ok(_) = Command::new("cmd.exe").args(&["/c", "start", url]).spawn() {
            return Ok(());
        }
        if let Ok(_) = Command::new("wslview").arg(url).spawn() {
            return Ok(());
        }
        eprintln!("WSL detected ({}), but unable to open browser", wsl_distro);
        eprintln!("Please open the following URL manually: {}", url);
        return Err("Failed to open URL in WSL".to_string());
    }
    
    // Check for SSH connection
    if std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_CLIENT").is_ok() {
        eprintln!("SSH session detected. Please open the following URL manually:");
        eprintln!("{}", url);
        return Err("Cannot open browser in SSH session".to_string());
    }
    
    // Check for container environment
    if Path::new("/.dockerenv").exists() || std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
        eprintln!("Container environment detected. Please open the following URL manually:");
        eprintln!("{}", url);
        return Err("Cannot open browser in container".to_string());
    }
    
    // Try platform-specific methods
    #[cfg(target_os = "macos")]
    {
        if let Ok(_) = Command::new("open").arg(url).spawn() {
            return Ok(());
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        // Try xdg-open first (most common)
        if let Ok(_) = Command::new("xdg-open").arg(url).spawn() {
            return Ok(());
        }
        // Try other common Linux browsers
        for browser in &["firefox", "chromium", "google-chrome", "brave-browser"] {
            if let Ok(_) = Command::new(browser).arg(url).spawn() {
                return Ok(());
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        if let Ok(_) = Command::new("cmd").args(&["/c", "start", url]).spawn() {
            return Ok(());
        }
    }
    
    // Fallback to webbrowser crate if available
    #[cfg(feature = "webbrowser")]
    {
        if webbrowser::open(url).is_ok() {
            return Ok(());
        }
    }
    
    // If all else fails, just print the URL
    eprintln!("Unable to open browser automatically. Please open the following URL manually:");
    eprintln!("{}", url);
    Err("Failed to open URL".to_string())
}
