use crate::JsonSchema;
use crate::ToolName;
use crate::ToolSpec;
use serde::Serialize;

pub const PUBLIC_TOOL_NAME: &str = "exec";
pub const WAIT_TOOL_NAME: &str = "wait";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum CodeModeToolKind {
    Function,
    Freeform,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolNamespaceDescription {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CodeModeToolDefinition {
    pub tool_name: ToolName,
    pub name: String,
    pub description: String,
    pub kind: CodeModeToolKind,
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<JsonSchema>,
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

pub fn code_mode_name_for_tool_name(tool_name: &ToolName) -> String {
    match tool_name.namespace.as_deref() {
        Some(namespace) if namespace.ends_with('_') || tool_name.name.starts_with('_') => {
            format!("{namespace}{}", tool_name.name)
        }
        Some(namespace) => format!("{namespace}__{}", tool_name.name),
        None => tool_name.name.clone(),
    }
}

pub fn is_code_mode_nested_tool(_name: &str) -> bool {
    false
}
