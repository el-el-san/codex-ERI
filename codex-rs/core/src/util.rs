use std::time::Duration;
use std::process::Command;
use std::env;

use rand::Rng;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

/// Open a URL in the default browser, handling various environments
pub fn open_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check for Termux environment
    if env::var("TERMUX_VERSION").is_ok() || env::var("PREFIX").unwrap_or_default().contains("/com.termux/") {
        // Use termux-open-url command
        let result = Command::new("termux-open-url")
            .arg(url)
            .spawn();
        
        if result.is_ok() {
            return Ok(());
        }
        // If termux-open-url fails, fall through to other methods
    }
    
    // Check for WSL environment
    if is_wsl() {
        // Try cmd.exe /c start
        let result = Command::new("cmd.exe")
            .args(["/c", "start", url])
            .spawn();
        
        if result.is_ok() {
            return Ok(());
        }
        
        // Try wslview as fallback
        let result = Command::new("wslview")
            .arg(url)
            .spawn();
        
        if result.is_ok() {
            return Ok(());
        }
    }
    
    // Check for SSH or container environment
    if is_ssh_or_container() {
        // Don't try to open browser automatically
        eprintln!("Running in SSH or container environment. Please open the following URL manually:");
        eprintln!("{}", url);
        return Ok(());
    }
    
    // Platform-specific browser opening
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()?;
        return Ok(());
    }
    
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()?;
        return Ok(());
    }
    
    #[cfg(target_os = "linux")]
    {
        // Try xdg-open
        let result = Command::new("xdg-open")
            .arg(url)
            .spawn();
        
        if result.is_ok() {
            return Ok(());
        }
        
        // Try common browsers as fallback
        for browser in &["firefox", "chromium", "google-chrome", "brave", "vivaldi"] {
            let result = Command::new(browser)
                .arg(url)
                .spawn();
            
            if result.is_ok() {
                return Ok(());
            }
        }
        
        return Err("Failed to open browser".into());
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("Unsupported platform for opening URLs".into())
    }
}

fn is_wsl() -> bool {
    if let Ok(kernel) = std::fs::read_to_string("/proc/version") {
        return kernel.to_lowercase().contains("microsoft");
    }
    false
}

fn is_ssh_or_container() -> bool {
    // Check if SSH_CONNECTION or SSH_CLIENT is set
    if env::var("SSH_CONNECTION").is_ok() || env::var("SSH_CLIENT").is_ok() {
        return true;
    }
    
    // Check if running in a container
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }
    
    // Check for container indicators in cgroup
    if let Ok(cgroup) = std::fs::read_to_string("/proc/self/cgroup") {
        if cgroup.contains("docker") || cgroup.contains("lxc") || cgroup.contains("containerd") {
            return true;
        }
    }
    
    false
}
