use std::env;
#[cfg(any(target_os = "linux", target_family = "unix"))]
use std::fs;
use std::io;
#[cfg(target_family = "unix")]
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tracing::debug;

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

#[derive(Debug, Error)]
pub enum OpenUrlError {
    #[error("failed to launch `{command}`: {source}")]
    SpawnFailed {
        command: String,
        #[source]
        source: io::Error,
    },
    #[error("`{command}` exited with status {status}")]
    CommandExited { command: String, status: String },
    #[error("unsupported platform: {0}")]
    Unsupported(String),
}

pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".to_string(),
        });
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return open_url_linux(url);
    }

    #[cfg(target_os = "macos")]
    {
        return open_url_macos(url);
    }

    #[cfg(target_os = "windows")]
    {
        return open_url_windows(url);
    }

    #[allow(unreachable_code)]
    Err(OpenUrlError::Unsupported(env::consts::OS.to_string()))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_url_linux(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if is_termux() {
        return run_command("termux-open-url", vec![url.to_string()])
            .map(|_| OpenUrlStatus::Opened);
    }

    #[cfg(target_os = "linux")]
    if is_wsl() {
        return open_url_wsl(url);
    }

    if should_suppress_auto_open() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "Detected SSH or container session; not launching a browser automatically.".to_string(),
        });
    }

    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();
    if let Ok(browser) = env::var("BROWSER") {
        if let Some(mut parts) = shlex::split(&browser) {
            if let Some(program) = parts.first().cloned() {
                parts.push(url.to_string());
                candidates.push((program, parts));
            }
        }
    }

    candidates.push(("xdg-open".to_string(), vec![url.to_string()]));
    candidates.push(("gio".to_string(), vec!["open".to_string(), url.to_string()]));
    candidates.push(("sensible-browser".to_string(), vec![url.to_string()]));
    candidates.push(("firefox".to_string(), vec![url.to_string()]));
    candidates.push(("google-chrome".to_string(), vec![url.to_string()]));
    candidates.push(("chromium".to_string(), vec![url.to_string()]));

    let mut last_error: Option<OpenUrlError> = None;
    for (program, args) in candidates {
        match run_command(&program, args) {
            Ok(()) => return Ok(OpenUrlStatus::Opened),
            Err(err) => {
                debug!(%program, error = %err, "URL opener failed");
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| OpenUrlError::Unsupported("no known URL opener found".to_string())))
}

#[cfg(target_os = "linux")]
fn open_url_wsl(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    let primary = run_command(
        "cmd.exe",
        vec!["/c".to_string(), "start".to_string(), String::from(""), url.to_string()],
    );
    if primary.is_ok() {
        return Ok(OpenUrlStatus::Opened);
    }

    let fallback = run_command("wslview", vec![url.to_string()]);
    if fallback.is_ok() {
        return Ok(OpenUrlStatus::Opened);
    }

    Err(fallback.unwrap_err())
}

#[cfg(target_os = "macos")]
fn open_url_macos(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if should_suppress_auto_open() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "Detected SSH or container session; not launching a browser automatically.".to_string(),
        });
    }

    run_command("open", vec![url.to_string()]).map(|_| OpenUrlStatus::Opened)
}

#[cfg(target_os = "windows")]
fn open_url_windows(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    run_command(
        "cmd",
        vec!["/C".to_string(), "start".to_string(), String::from(""), url.to_string()],
    )
    .map(|_| OpenUrlStatus::Opened)
}

fn should_suppress_auto_open() -> bool {
    is_ssh_session() || is_container_environment()
}

fn run_command(program: &str, args: Vec<String>) -> Result<(), OpenUrlError> {
    let formatted = format_command(program, &args);
    debug!(command = %formatted, "Attempting to launch URL opener");

    let mut command = Command::new(program);
    command.args(&args);
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());

    let status = command.status().map_err(|source| OpenUrlError::SpawnFailed {
        command: formatted.clone(),
        source,
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(OpenUrlError::CommandExited {
            command: formatted,
            status: describe_status(status),
        })
    }
}

fn format_command(program: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(program.to_string());
    for arg in args {
        if arg.contains(' ') {
            parts.push(format!("\"{arg}\""));
        } else {
            parts.push(arg.clone());
        }
    }
    parts.join(" ")
}

fn describe_status(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => code.to_string(),
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    return format!("signal {signal}");
                }
            }
            "unknown".to_string()
        }
    }
}

fn is_ssh_session() -> bool {
    env::var("SSH_CONNECTION").is_ok()
        || env::var("SSH_CLIENT").is_ok()
        || env::var("SSH_TTY").is_ok()
}

fn is_container_environment() -> bool {
    if env::var("CONTAINER").is_ok()
        || env::var("KUBERNETES_SERVICE_HOST").is_ok()
        || env::var("DOCKER_CONTAINER").is_ok()
    {
        return true;
    }

    #[cfg(target_family = "unix")]
    {
        if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
            return true;
        }

        if let Ok(contents) = fs::read_to_string("/proc/1/cgroup") {
            if contents.contains("docker")
                || contents.contains("kubepods")
                || contents.contains("containerd")
            {
                return true;
            }
        }
    }

    false
}

#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    env::var("WSL_DISTRO_NAME").is_ok()
        || fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|release| release.to_lowercase().contains("microsoft"))
            .unwrap_or(false)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn is_termux() -> bool {
    env::var("TERMUX_VERSION").is_ok()
        || env::var("PREFIX")
            .map(|prefix| prefix.contains("com.termux"))
            .unwrap_or(false)
}
