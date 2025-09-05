use std::time::Duration;
use std::process::Command;
use std::env;

use rand::Rng;
use anyhow::Result;

const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;

pub(crate) fn backoff(attempt: u64) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    let jitter = rand::rng().random_range(0.9..1.1);
    Duration::from_millis((base as f64 * jitter) as u64)
}

pub fn open_url(url: &str) -> Result<()> {
    if env::var("TERMUX_VERSION").is_ok() {
        Command::new("termux-open-url")
            .arg(url)
            .spawn()
            .map(|_| ())
            .or_else(|_| {
                eprintln!("Failed to open URL with termux-open-url. Please install Termux:API and run: pkg install termux-api");
                eprintln!("Alternatively, manually open: {}", url);
                Ok(())
            })
    } else if env::var("WSL_DISTRO_NAME").is_ok() || env::var("WSL_INTEROP").is_ok() {
        Command::new("cmd.exe")
            .args(["/c", "start", url])
            .spawn()
            .or_else(|_| Command::new("wslview").arg(url).spawn())
            .map(|_| ())
            .or_else(|_| {
                eprintln!("Failed to open URL in WSL. Manually open: {}", url);
                Ok(())
            })
    } else if env::var("SSH_CONNECTION").is_ok() || env::var("SSH_CLIENT").is_ok() {
        eprintln!("Running in SSH session. Please manually open: {}", url);
        Ok(())
    } else if env::var("KUBERNETES_SERVICE_HOST").is_ok() 
        || std::path::Path::new("/.dockerenv").exists() 
        || env::var("CONTAINER").is_ok() {
        eprintln!("Running in container. Please manually open: {}", url);
        Ok(())
    } else {
        #[cfg(target_os = "macos")]
        {
            Command::new("open").arg(url).spawn().map(|_| ()).or_else(|e| {
                eprintln!("Failed to open URL: {}", e);
                Err(anyhow::anyhow!("Failed to open URL"))
            })
        }
        
        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open")
                .arg(url)
                .spawn()
                .or_else(|_| Command::new("gnome-open").arg(url).spawn())
                .or_else(|_| Command::new("kde-open").arg(url).spawn())
                .or_else(|_| Command::new("firefox").arg(url).spawn())
                .or_else(|_| Command::new("chromium").arg(url).spawn())
                .or_else(|_| Command::new("google-chrome").arg(url).spawn())
                .map(|_| ())
                .or_else(|e| {
                    eprintln!("Failed to open URL: {}", e);
                    Err(anyhow::anyhow!("Failed to open URL"))
                })
        }
        
        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .args(["/c", "start", url])
                .spawn()
                .map(|_| ())
                .or_else(|e| {
                    eprintln!("Failed to open URL: {}", e);
                    Err(anyhow::anyhow!("Failed to open URL"))
                })
        }
        
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            eprintln!("Unsupported platform. Please manually open: {}", url);
            Ok(())
        }
    }
}
