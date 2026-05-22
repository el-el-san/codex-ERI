use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::ToolRouter;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_tools::CodeModeToolDefinition;
use codex_tools::FreeformTool;
use codex_tools::FreeformToolFormat;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolNamespaceDescription;
use codex_tools::ToolSpec;

pub(crate) const PUBLIC_TOOL_NAME: &str = "exec";
pub(crate) const WAIT_TOOL_NAME: &str = "wait";
pub(crate) const DEFAULT_WAIT_YIELD_TIME_MS: u64 = 1000;

const CODE_MODE_UNSUPPORTED_MESSAGE: &str = "code mode is disabled in Android builds";

pub(crate) fn is_exec_tool_name(tool_name: &ToolName) -> bool {
    tool_name.namespace.is_none() && tool_name.name == PUBLIC_TOOL_NAME
}

pub(crate) fn is_code_mode_nested_tool(_name: &str) -> bool {
    false
}

pub(crate) struct CodeModeService;

impl CodeModeService {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn stored_values(&self) -> HashMap<String, JsonValue> {
        HashMap::new()
    }

    pub(crate) async fn replace_stored_values(&self, _values: HashMap<String, JsonValue>) {}

    pub(crate) async fn start_turn_worker(
        &self,
        _session: &Arc<Session>,
        _turn: &Arc<TurnContext>,
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

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for CodeModeExecuteHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(PUBLIC_TOOL_NAME)
    }

    fn spec(&self) -> Option<ToolSpec> {
        Some(self.spec.clone())
    }

    async fn handle(
        &self,
        _invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        Err(FunctionCallError::RespondToModel(
            CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
        ))
    }
}

impl CoreToolRuntime for CodeModeExecuteHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Custom { .. })
    }
}

pub(crate) struct CodeModeWaitHandler;

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for CodeModeWaitHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(WAIT_TOOL_NAME)
    }

    fn spec(&self) -> Option<ToolSpec> {
        Some(wait_spec::create_wait_tool())
    }

    async fn handle(
        &self,
        _invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        Err(FunctionCallError::RespondToModel(
            CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
        ))
    }
}

impl CoreToolRuntime for CodeModeWaitHandler {}

pub(crate) mod execute_spec {
    use super::*;

    pub(crate) fn create_code_mode_tool(
        _enabled_tools: &[CodeModeToolDefinition],
        _namespace_descriptions: &std::collections::BTreeMap<String, ToolNamespaceDescription>,
        _code_mode_only: bool,
        _deferred_tools_available: bool,
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
            description: "Execute JavaScript source in code mode.".to_string(),
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
        let properties = std::collections::BTreeMap::from([
            (
                "cell_id".to_string(),
                JsonSchema::string(Some("Identifier of the running exec cell.".to_string())),
            ),
            (
                "yield_time_ms".to_string(),
                JsonSchema::number(Some(
                    "How long to wait (in milliseconds) for more output before yielding again."
                        .to_string(),
                )),
            ),
            (
                "max_tokens".to_string(),
                JsonSchema::number(Some(
                    "Maximum number of output tokens to return for this wait call.".to_string(),
                )),
            ),
            (
                "terminate".to_string(),
                JsonSchema::boolean(Some(
                    "Whether to terminate the running exec cell.".to_string(),
                )),
            ),
        ]);

        ToolSpec::Function(ResponsesApiTool {
            name: WAIT_TOOL_NAME.to_string(),
            description: "Waits on a yielded exec cell.".to_string(),
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
