use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use crate::function_tool::FunctionCallError;
use crate::tools::ToolRouter;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub(crate) const PUBLIC_TOOL_NAME: &str = "exec";
pub(crate) const WAIT_TOOL_NAME: &str = "wait";
pub(crate) const DEFAULT_WAIT_YIELD_TIME_MS: u64 = 1_000;

const CODE_MODE_UNSUPPORTED_MESSAGE: &str = "code mode is disabled in Android builds";

pub(crate) struct CodeModeService;

impl CodeModeService {
    pub(crate) fn new(_js_repl_node_path: Option<PathBuf>) -> Self {
        Self
    }

    pub(crate) async fn stored_values(&self) -> HashMap<String, JsonValue> {
        HashMap::new()
    }

    pub(crate) async fn replace_stored_values(&self, _values: HashMap<String, JsonValue>) {}

    pub(crate) async fn start_turn_worker(
        &self,
        _session: &std::sync::Arc<crate::codex::Session>,
        _turn: &std::sync::Arc<crate::codex::TurnContext>,
        _router: std::sync::Arc<ToolRouter>,
        _tracker: SharedTurnDiffTracker,
    ) -> Option<()> {
        None
    }
}

pub(crate) struct CodeModeExecuteHandler;

#[async_trait]
impl ToolHandler for CodeModeExecuteHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, _invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        Err(FunctionCallError::RespondToModel(
            CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
        ))
    }
}

pub(crate) struct CodeModeWaitHandler;

#[async_trait]
impl ToolHandler for CodeModeWaitHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, _invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        Err(FunctionCallError::RespondToModel(
            CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
        ))
    }
}
