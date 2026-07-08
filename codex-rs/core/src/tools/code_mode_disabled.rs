use std::sync::Arc;

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::ToolRouter;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

pub(crate) const PUBLIC_TOOL_NAME: &str = codex_tools::CODE_MODE_PUBLIC_TOOL_NAME;
pub(crate) const WAIT_TOOL_NAME: &str = codex_tools::CODE_MODE_WAIT_TOOL_NAME;
pub(crate) const DEFAULT_WAIT_YIELD_TIME_MS: u64 = 10_000;

const CODE_MODE_UNSUPPORTED_MESSAGE: &str = "code mode is disabled in Android builds";

pub(crate) trait CodeModeSessionProvider: Send + Sync {}

pub(crate) struct InProcessCodeModeSessionProvider;

impl CodeModeSessionProvider for InProcessCodeModeSessionProvider {}

pub(crate) fn is_exec_tool_name(tool_name: &ToolName) -> bool {
    tool_name.namespace.is_none() && tool_name.name == PUBLIC_TOOL_NAME
}

pub(crate) struct CodeModeService {
    session_provider: Arc<dyn CodeModeSessionProvider>,
}

impl CodeModeService {
    pub(crate) fn new(session_provider: Arc<dyn CodeModeSessionProvider>) -> Self {
        Self { session_provider }
    }

    pub(crate) fn session_provider(&self) -> Arc<dyn CodeModeSessionProvider> {
        Arc::clone(&self.session_provider)
    }

    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }

    pub(crate) fn start_turn_worker(
        &self,
        _session: &Arc<Session>,
        _step_context: Arc<StepContext>,
        _router: Arc<ToolRouter>,
        _tracker: SharedTurnDiffTracker,
    ) -> Option<()> {
        None
    }
}

pub(crate) struct CodeModeExecuteHandler {
    spec: ToolSpec,
}

impl CodeModeExecuteHandler {
    pub(crate) fn new(spec: ToolSpec, _nested_tool_specs: Vec<ToolSpec>) -> Self {
        Self { spec }
    }
}

impl ToolExecutor<ToolInvocation> for CodeModeExecuteHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(PUBLIC_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    fn handle(&self, _invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            Err(FunctionCallError::RespondToModel(
                CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
            ))
        })
    }
}

impl CoreToolRuntime for CodeModeExecuteHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Custom { .. })
    }
}

pub(crate) struct CodeModeWaitHandler;

impl ToolExecutor<ToolInvocation> for CodeModeWaitHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(WAIT_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        execute_spec::create_wait_tool()
    }

    fn handle(&self, _invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            Err(FunctionCallError::RespondToModel(
                CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
            ))
        })
    }
}

impl CoreToolRuntime for CodeModeWaitHandler {}

pub(crate) mod execute_spec {
    use super::*;
    use std::collections::BTreeMap;

    pub(crate) fn create_code_mode_tool(
        enabled_tools: &[codex_tools::CodeModeToolDefinition],
        namespace_descriptions: &BTreeMap<String, codex_tools::ToolNamespaceDescription>,
        code_mode_only_enabled: bool,
        deferred_tools_available: bool,
    ) -> ToolSpec {
        codex_tools::create_code_mode_tool(
            enabled_tools,
            namespace_descriptions,
            code_mode_only_enabled,
            deferred_tools_available,
        )
    }

    pub(crate) fn create_wait_tool() -> ToolSpec {
        codex_tools::create_wait_tool()
    }
}
