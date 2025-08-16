use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CustomCommand {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub command_type: CustomCommandType,
    pub content: String,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub accepts_args: bool,
    #[serde(default)]
    pub arg_placeholder: Option<String>,
    #[serde(default)]
    pub force_high_reasoning: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomCommandType {
    Shell,
    Prompt,
}

impl CustomCommand {
    pub fn command(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}