use std::collections::BTreeMap;
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
use codex_features::Features;
use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

pub(crate) const PUBLIC_TOOL_NAME: &str = codex_tools::CODE_MODE_PUBLIC_TOOL_NAME;
pub(crate) const WAIT_TOOL_NAME: &str = codex_tools::CODE_MODE_WAIT_TOOL_NAME;
pub(crate) const DEFAULT_WAIT_YIELD_TIME_MS: u64 = 10_000;

const CODE_MODE_UNSUPPORTED_MESSAGE: &str = "code mode is disabled in Android builds";

pub(crate) trait CodeModeSessionProvider: Send + Sync {}

pub(crate) struct InProcessCodeModeSessionProvider;

impl CodeModeSessionProvider for InProcessCodeModeSessionProvider {}

pub(crate) fn default_exec_yield_time_override_ms(_features: &Features) -> Option<u64> {
    None
}

pub(crate) fn is_exec_tool_name(tool_name: &ToolName) -> bool {
    tool_name.namespace.is_none() && tool_name.name == PUBLIC_TOOL_NAME
}

pub(crate) struct CodeModeService {
    session_provider: Arc<dyn CodeModeSessionProvider>,
}

impl CodeModeService {
    pub(crate) fn new(
        session_provider: Arc<dyn CodeModeSessionProvider>,
        _features: &Features,
    ) -> Self {
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
        wait_spec::create_wait_tool()
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

    pub(crate) fn create_code_mode_tool(
        _enabled_tools: &[codex_tools::CodeModeToolDefinition],
        _deferred_tools: &[codex_tools::CodeModeToolDefinition],
        _namespace_descriptions: &BTreeMap<String, codex_tools::ToolNamespaceDescription>,
        _default_exec_yield_time_ms: u64,
        _code_mode_only: bool,
    ) -> ToolSpec {
        const CODE_MODE_FREEFORM_GRAMMAR: &str = r#"
start: pragma_source | plain_source
pragma_source: PRAGMA_LINE NEWLINE SOURCE
plain_source: SOURCE

PRAGMA_LINE: /[ \t]*\/\/ @exec:[^\r\n]*/
NEWLINE: /\r?\n/
SOURCE: /[\s\S]+/
"#;

        ToolSpec::Freeform(FreeformTool {
            name: PUBLIC_TOOL_NAME.to_string(),
            description: CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
            format: FreeformToolFormat {
                r#type: "grammar".to_string(),
                syntax: "lark".to_string(),
                definition: CODE_MODE_FREEFORM_GRAMMAR.to_string(),
            },
        })
    }
}

pub(crate) mod wait_spec {
    use super::*;

    pub(crate) fn create_wait_tool() -> ToolSpec {
        let properties = BTreeMap::from([
            (
                "cell_id".to_string(),
                JsonSchema::string(Some("Identifier of the running exec cell.".to_string())),
            ),
            (
                "yield_time_ms".to_string(),
                JsonSchema::number(Some(
                    "Wait before yielding more output. Defaults to 10000 ms.".to_string(),
                )),
            ),
            (
                "max_tokens".to_string(),
                JsonSchema::number(Some(
                    "Output token budget for this wait call. Defaults to 10000 tokens.".to_string(),
                )),
            ),
            (
                "terminate".to_string(),
                JsonSchema::boolean(Some(
                    "True stops the running exec cell; false or omitted waits for output."
                        .to_string(),
                )),
            ),
        ]);

        ToolSpec::Function(ResponsesApiTool {
            name: WAIT_TOOL_NAME.to_string(),
            description: CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
            strict: false,
            parameters: JsonSchema::object(
                properties,
                Some(vec!["cell_id".to_string()]),
                Some(false.into()),
            ),
            output_schema: None,
            defer_loading: None,
        })
    }
}
