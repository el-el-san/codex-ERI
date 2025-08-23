use serde::Deserialize;
use serde::Serialize;

use crate::exec_command::session_id::SessionId;

#[derive(Debug, Clone, Deserialize)]
pub struct ExecCommandParams {
    pub cmd: String,

    #[serde(default = "default_yield_time")]
    pub yield_time_ms: u64,

    #[serde(default = "max_output_tokens")]
    pub max_output_tokens: u64,

    #[serde(default = "default_shell")]
    pub shell: String,

    #[serde(default = "default_login")]
    pub login: bool,
}

fn default_yield_time() -> u64 {
    10_000
}

fn max_output_tokens() -> u64 {
    10_000
}

fn default_login() -> bool {
    true
}

fn default_shell() -> String {
    "/bin/bash".to_string()
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WriteStdinParams {
    pub session_id: SessionId,
    pub chars: String,

    #[serde(default = "write_stdin_default_yield_time_ms")]
    pub yield_time_ms: u64,

    #[serde(default = "write_stdin_default_max_output_tokens")]
    pub max_output_tokens: u64,
}

fn write_stdin_default_yield_time_ms() -> u64 {
    250
}

fn write_stdin_default_max_output_tokens() -> u64 {
    10_000
}