//! Persist Codex session rollouts (.jsonl) so sessions can be replayed or inspected later.

use std::fs::File;
use std::fs::{self};
use std::io::Error as IoError;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::FormatItem;
use time::macros::format_description;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::{self};
use tokio::sync::oneshot;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

use crate::config::Config;
use crate::git_info::GitInfo;
use crate::git_info::collect_git_info;
use crate::models::ResponseItem;
use crate::protocol::InputItem;

const SESSIONS_SUBDIR: &str = "sessions";

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SessionMeta {
    pub id: Uuid,
    pub timestamp: String,
    pub instructions: Option<String>,
}

#[derive(Serialize)]
struct SessionMetaWithGit {
    #[serde(flatten)]
    meta: SessionMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    git: Option<GitInfo>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SessionStateSnapshot {}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SavedSession {
    pub session: SessionMeta,
    #[serde(default)]
    pub items: Vec<ResponseItem>,
    #[serde(default)]
    pub state: SessionStateSnapshot,
    pub session_id: Uuid,
}

/// Records all [`ResponseItem`]s for a session and flushes them to disk after
/// every update.
///
/// Rollouts are recorded as JSONL and can be inspected with tools such as:
///
/// ```ignore
/// $ jq -C . ~/.codex/sessions/rollout-2025-05-07T17-24-21-5973b6c0-94b8-487b-a530-2aeb6098ae0e.jsonl
/// $ fx ~/.codex/sessions/rollout-2025-05-07T17-24-21-5973b6c0-94b8-487b-a530-2aeb6098ae0e.jsonl
/// ```
#[derive(Clone)]
pub(crate) struct RolloutRecorder {
    tx: Sender<RolloutCmd>,
}

enum RolloutCmd {
    AddItems(Vec<ResponseItem>),
    UpdateState(SessionStateSnapshot),
    Shutdown { ack: oneshot::Sender<()> },
}

impl RolloutRecorder {
    /// Attempt to create a new [`RolloutRecorder`]. If the sessions directory
    /// cannot be created or the rollout file cannot be opened we return the
    /// error so the caller can decide whether to disable persistence.
    pub async fn new(
        config: &Config,
        uuid: Uuid,
        instructions: Option<String>,
    ) -> std::io::Result<Self> {
        let LogFileInfo {
            file,
            session_id,
            timestamp,
        } = create_log_file(config, uuid)?;

        let timestamp_format: &[FormatItem] = format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        );
        let timestamp = timestamp
            .format(timestamp_format)
            .map_err(|e| IoError::other(format!("failed to format timestamp: {e}")))?;

        // Clone the cwd for the spawned task to collect git info asynchronously
        let cwd = config.cwd.clone();

        // A reasonably-sized bounded channel. If the buffer fills up the send
        // future will yield, which is fine – we only need to ensure we do not
        // perform *blocking* I/O on the caller's thread.
        let (tx, rx) = mpsc::channel::<RolloutCmd>(256);

        // Spawn a Tokio task that owns the file handle and performs async
        // writes. Using `tokio::fs::File` keeps everything on the async I/O
        // driver instead of blocking the runtime.
        tokio::task::spawn(rollout_writer(
            tokio::fs::File::from_std(file),
            rx,
            Some(SessionMeta {
                timestamp,
                id: session_id,
                instructions,
            }),
            cwd,
        ));

        Ok(Self { tx })
    }

    pub(crate) async fn record_items(&self, items: &[ResponseItem]) -> std::io::Result<()> {
        let mut filtered = Vec::new();
        for item in items {
            match item {
                // Note that function calls may look a bit strange if they are
                // "fully qualified MCP tool calls," so we could consider
                // reformatting them in that case.
                ResponseItem::Message { .. }
                | ResponseItem::LocalShellCall { .. }
                | ResponseItem::FunctionCall { .. }
                | ResponseItem::FunctionCallOutput { .. }
                | ResponseItem::Reasoning { .. } => filtered.push(item.clone()),
                ResponseItem::Other => {
                    // These should never be serialized.
                    continue;
                }
            }
        }
        if filtered.is_empty() {
            return Ok(());
        }
        self.tx
            .send(RolloutCmd::AddItems(filtered))
            .await
            .map_err(|e| IoError::other(format!("failed to queue rollout items: {e}")))
    }

    pub(crate) async fn record_state(&self, state: SessionStateSnapshot) -> std::io::Result<()> {
        self.tx
            .send(RolloutCmd::UpdateState(state))
            .await
            .map_err(|e| IoError::other(format!("failed to queue rollout state: {e}")))
    }

    pub async fn resume(
        path: &Path,
        cwd: std::path::PathBuf,
    ) -> std::io::Result<(Self, SavedSession)> {
        info!("Resuming rollout from {path:?}");
        let text = tokio::fs::read_to_string(path).await?;
        let mut lines = text.lines();
        let meta_line = lines
            .next()
            .ok_or_else(|| IoError::other("empty session file"))?;
        let session: SessionMeta = serde_json::from_str(meta_line)
            .map_err(|e| IoError::other(format!("failed to parse session meta: {e}")))?;
        let mut items = Vec::new();
        let mut state = SessionStateSnapshot::default();

        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("record_type")
                .and_then(|rt| rt.as_str())
                .map(|s| s == "state")
                .unwrap_or(false)
            {
                if let Ok(s) = serde_json::from_value::<SessionStateSnapshot>(v.clone()) {
                    state = s
                }
                continue;
            }
            match serde_json::from_value::<ResponseItem>(v.clone()) {
                Ok(item) => match item {
                    ResponseItem::Message { .. }
                    | ResponseItem::LocalShellCall { .. }
                    | ResponseItem::FunctionCall { .. }
                    | ResponseItem::FunctionCallOutput { .. }
                    | ResponseItem::Reasoning { .. } => items.push(item),
                    ResponseItem::Other => {}
                },
                Err(e) => {
                    warn!("failed to parse item: {v:?}, error: {e}");
                }
            }
        }

        let saved = SavedSession {
            session: session.clone(),
            items: items.clone(),
            state: state.clone(),
            session_id: session.id,
        };

        let file = std::fs::OpenOptions::new()
            .append(true)
            .read(true)
            .open(path)?;

        let (tx, rx) = mpsc::channel::<RolloutCmd>(256);
        tokio::task::spawn(rollout_writer(
            tokio::fs::File::from_std(file),
            rx,
            None,
            cwd,
        ));
        info!("Resumed rollout successfully from {path:?}");
        Ok((Self { tx }, saved))
    }

    pub async fn shutdown(&self) -> std::io::Result<()> {
        let (tx_done, rx_done) = oneshot::channel();
        match self.tx.send(RolloutCmd::Shutdown { ack: tx_done }).await {
            Ok(_) => rx_done
                .await
                .map_err(|e| IoError::other(format!("failed waiting for rollout shutdown: {e}"))),
            Err(e) => {
                warn!("failed to send rollout shutdown command: {e}");
                Err(IoError::other(format!(
                    "failed to send rollout shutdown command: {e}"
                )))
            }
        }
    }
}

struct LogFileInfo {
    /// Opened file handle to the rollout file.
    file: File,

    /// Session ID (also embedded in filename).
    session_id: Uuid,

    /// Timestamp for the start of the session.
    timestamp: OffsetDateTime,
}

fn create_log_file(config: &Config, session_id: Uuid) -> std::io::Result<LogFileInfo> {
    // Resolve ~/.codex/sessions/YYYY/MM/DD and create it if missing.
    let timestamp = OffsetDateTime::now_local()
        .map_err(|e| IoError::other(format!("failed to get local time: {e}")))?;
    let mut dir = config.codex_home.clone();
    dir.push(SESSIONS_SUBDIR);
    dir.push(timestamp.year().to_string());
    dir.push(format!("{:02}", u8::from(timestamp.month())));
    dir.push(format!("{:02}", timestamp.day()));
    fs::create_dir_all(&dir)?;

    // Custom format for YYYY-MM-DDThh-mm-ss. Use `-` instead of `:` for
    // compatibility with filesystems that do not allow colons in filenames.
    let format: &[FormatItem] =
        format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]");
    let date_str = timestamp
        .format(format)
        .map_err(|e| IoError::other(format!("failed to format timestamp: {e}")))?;

    let filename = format!("rollout-{date_str}-{session_id}.jsonl");

    let path = dir.join(filename);
    let file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)?;

    Ok(LogFileInfo {
        file,
        session_id,
        timestamp,
    })
}

async fn rollout_writer(
    file: tokio::fs::File,
    mut rx: mpsc::Receiver<RolloutCmd>,
    mut meta: Option<SessionMeta>,
    cwd: std::path::PathBuf,
) -> std::io::Result<()> {
    let mut writer = JsonlWriter { file };

    // If we have a meta, collect git info asynchronously and write meta first
    if let Some(session_meta) = meta.take() {
        let git_info = collect_git_info(&cwd).await;
        let session_meta_with_git = SessionMetaWithGit {
            meta: session_meta,
            git: git_info,
        };

        // Write the SessionMeta as the first item in the file
        writer.write_line(&session_meta_with_git).await?;
    }

    // Process rollout commands
    while let Some(cmd) = rx.recv().await {
        match cmd {
            RolloutCmd::AddItems(items) => {
                for item in items {
                    match item {
                        ResponseItem::Message { .. }
                        | ResponseItem::LocalShellCall { .. }
                        | ResponseItem::FunctionCall { .. }
                        | ResponseItem::FunctionCallOutput { .. }
                        | ResponseItem::Reasoning { .. } => {
                            writer.write_line(&item).await?;
                        }
                        ResponseItem::Other => {}
                    }
                }
            }
            RolloutCmd::UpdateState(state) => {
                #[derive(Serialize)]
                struct StateLine<'a> {
                    record_type: &'static str,
                    #[serde(flatten)]
                    state: &'a SessionStateSnapshot,
                }
                writer
                    .write_line(&StateLine {
                        record_type: "state",
                        state: &state,
                    })
                    .await?;
            }
            RolloutCmd::Shutdown { ack } => {
                let _ = ack.send(());
            }
        }
    }

    Ok(())
}

struct JsonlWriter {
    file: tokio::fs::File,
}

impl JsonlWriter {
    async fn write_line(&mut self, item: &impl serde::Serialize) -> std::io::Result<()> {
        let mut json = serde_json::to_string(item)?;
        json.push('\n');
        let _ = self.file.write_all(json.as_bytes()).await;
        self.file.flush().await?;
        Ok(())
    }
}

/// Find the most recent rollout file in the sessions directory
pub async fn find_latest_rollout(config: &Config) -> std::io::Result<Option<PathBuf>> {
    let mut sessions_dir = config.codex_home.clone();
    sessions_dir.push(SESSIONS_SUBDIR);
    
    if !sessions_dir.exists() {
        return Ok(None);
    }
    
    // Find all rollout files recursively
    let mut latest_file: Option<(PathBuf, std::time::SystemTime)> = None;
    
    fn scan_dir(dir: &Path, latest: &mut Option<(PathBuf, std::time::SystemTime)>) -> std::io::Result<()> {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    scan_dir(&path, latest)?;
                } else if let Some(name) = path.file_name() {
                    if let Some(name_str) = name.to_str() {
                        if name_str.starts_with("rollout-") && name_str.ends_with(".jsonl") {
                            if let Ok(metadata) = entry.metadata() {
                                if let Ok(modified) = metadata.modified() {
                                    match latest {
                                        None => {
                                            *latest = Some((path, modified));
                                        }
                                        Some((_, last_time)) if modified > *last_time => {
                                            *latest = Some((path, modified));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    
    scan_dir(&sessions_dir, &mut latest_file)?;
    Ok(latest_file.map(|(path, _)| path))
}

/// Load a rollout file and extract conversation history
pub async fn load_rollout_conversation(path: &Path) -> std::io::Result<Vec<InputItem>> {
    use tokio::io::AsyncBufReadExt;
    use tokio::io::BufReader;
    
    let file = tokio::fs::File::open(path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    
    let mut conversation = Vec::new();
    
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        
        // Parse each JSON line
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            // Check role field for messages
            if let Some(role) = value.get("role") {
                let role_str = role.as_str().unwrap_or("");
                
                // Extract message content based on role
                if role_str == "user" {
                    if let Some(content_array) = value.get("content").and_then(|c| c.as_array()) {
                        for item in content_array {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                conversation.push(InputItem::Text { 
                                    text: format!("User: {}", text) 
                                });
                            }
                        }
                    }
                } else if role_str == "assistant" {
                    if let Some(content_array) = value.get("content").and_then(|c| c.as_array()) {
                        for item in content_array {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                conversation.push(InputItem::Text { 
                                    text: format!("Assistant: {}", text) 
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(conversation)
}
