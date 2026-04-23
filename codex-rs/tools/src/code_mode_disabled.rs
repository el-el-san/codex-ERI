use crate::FreeformTool;
use crate::FreeformToolFormat;
use crate::JsonSchema;
use crate::ResponsesApiTool;
use crate::ToolName;
use crate::ToolSpec;
use serde::Serialize;
use std::collections::BTreeMap;

pub const PUBLIC_TOOL_NAME: &str = "exec";
pub const WAIT_TOOL_NAME: &str = "wait";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolNamespaceDescription {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CodeModeToolDefinition {
    pub tool_name: ToolName,
    pub name: String,
    pub description: String,
}

pub fn augment_tool_spec_for_code_mode(spec: ToolSpec) -> ToolSpec {
    spec
}

pub fn tool_spec_to_code_mode_tool_definition(_spec: &ToolSpec) -> Option<CodeModeToolDefinition> {
    None
}

pub fn collect_code_mode_tool_definitions<'a>(
    _specs: impl IntoIterator<Item = &'a ToolSpec>,
) -> Vec<CodeModeToolDefinition> {
    Vec::new()
}

pub fn collect_code_mode_exec_prompt_tool_definitions<'a>(
    _specs: impl IntoIterator<Item = &'a ToolSpec>,
) -> Vec<CodeModeToolDefinition> {
    Vec::new()
}

pub fn create_wait_tool() -> ToolSpec {
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

pub fn create_code_mode_tool(
    _enabled_tools: &[CodeModeToolDefinition],
    _namespace_descriptions: &BTreeMap<String, ToolNamespaceDescription>,
    _code_mode_only_enabled: bool,
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
