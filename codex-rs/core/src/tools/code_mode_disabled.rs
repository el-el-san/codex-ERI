use std::collections::HashMap;

use serde_json::Value as JsonValue;

use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::ToolRouter;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
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

const CODE_MODE_UNSUPPORTED_MESSAGE: &str = "code mode is disabled in Android builds";

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
        _session: &std::sync::Arc<Session>,
        _turn: &std::sync::Arc<TurnContext>,
        _router: std::sync::Arc<ToolRouter>,
        _tracker: SharedTurnDiffTracker,
    ) -> Option<()> {
        None
    }
}

pub(crate) struct CodeModeExecuteHandler;

impl CodeModeExecuteHandler {
    pub(crate) fn new(_spec: ToolSpec) -> Self {
        Self
    }
}

impl ToolHandler for CodeModeExecuteHandler {
    type Output = FunctionToolOutput;

    fn tool_name(&self) -> ToolName {
        ToolName::plain(PUBLIC_TOOL_NAME)
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Custom { .. })
    }

    fn handle(
        &self,
        _invocation: ToolInvocation,
    ) -> impl std::future::Future<Output = Result<Self::Output, FunctionCallError>> + Send {
        async {
            Err(FunctionCallError::RespondToModel(
                CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
            ))
        }
    }
}

pub(crate) struct CodeModeWaitHandler;

impl ToolHandler for CodeModeWaitHandler {
    type Output = FunctionToolOutput;

    fn tool_name(&self) -> ToolName {
        ToolName::plain(WAIT_TOOL_NAME)
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn handle(
        &self,
        _invocation: ToolInvocation,
    ) -> impl std::future::Future<Output = Result<Self::Output, FunctionCallError>> + Send {
        async {
            Err(FunctionCallError::RespondToModel(
                CODE_MODE_UNSUPPORTED_MESSAGE.to_string(),
            ))
        }
    }
}

pub(crate) mod execute_spec {
    use super::*;
    use std::collections::BTreeMap;

    pub(crate) fn create_code_mode_tool(
        _enabled_tools: &[CodeModeToolDefinition],
        _namespace_descriptions: &BTreeMap<String, ToolNamespaceDescription>,
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
    use std::collections::BTreeMap;

    pub(crate) fn create_wait_tool() -> ToolSpec {
        let properties = BTreeMap::from([
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
