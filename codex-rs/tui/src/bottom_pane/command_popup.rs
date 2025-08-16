use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;
use crate::slash_command::SlashCommand;
use crate::slash_command::built_in_slash_commands;
use codex_common::fuzzy_match::fuzzy_match;
use codex_core::custom_command::CustomCommand;

/// Unified command type for display.
#[derive(Clone)]
pub(crate) enum CommandType<'a> {
    BuiltIn(&'a SlashCommand),
    Custom(&'a CustomCommand),
}

pub(crate) struct CommandPopup {
    command_filter: String,
    all_commands: Vec<(&'static str, SlashCommand)>,
    custom_commands: Vec<CustomCommand>,
    state: ScrollState,
}

impl CommandPopup {
    pub(crate) fn new(custom_commands: Vec<CustomCommand>) -> Self {
        Self {
            command_filter: String::new(),
            all_commands: built_in_slash_commands(),
            custom_commands,
            state: ScrollState::new(),
        }
    }

    /// Update the filter string based on the current composer text. The text
    /// passed in is expected to start with a leading '/'. Everything after the
    /// *first* '/" on the *first* line becomes the active filter that is used
    /// to narrow down the list of available commands.
    pub(crate) fn on_composer_text_change(&mut self, text: String) {
        let first_line = text.lines().next().unwrap_or("");

        if let Some(stripped) = first_line.strip_prefix('/') {
            // Extract the *first* token (sequence of non-whitespace
            // characters) after the slash so that `/clear something` still
            // shows the help for `/clear`.
            let token = stripped.trim_start();
            let cmd_token = token.split_whitespace().next().unwrap_or("");

            // Update the filter keeping the original case (commands are all
            // lower-case for now but this may change in the future).
            self.command_filter = cmd_token.to_string();
        } else {
            // The composer no longer starts with '/'. Reset the filter so the
            // popup shows the *full* command list if it is still displayed
            // for some reason.
            self.command_filter.clear();
        }

        // Reset or clamp selected index based on new filtered list.
        let matches_len = self.filtered_all().len();
        self.state.clamp_selection(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    /// Determine the preferred height of the popup. This is the number of
    /// rows required to show at most MAX_POPUP_ROWS commands.
    pub(crate) fn calculate_required_height(&self) -> u16 {
        self.filtered_all().len().clamp(1, MAX_POPUP_ROWS) as u16
    }

    /// Compute fuzzy-filtered matches paired with optional highlight indices and score.
    /// Sorted by ascending score, then by command name for stability.
    fn filtered(&self) -> Vec<(&SlashCommand, Option<Vec<usize>>, i32)> {
        let filter = self.command_filter.trim();
        let mut out: Vec<(&SlashCommand, Option<Vec<usize>>, i32)> = Vec::new();
        if filter.is_empty() {
            for (_, cmd) in self.all_commands.iter() {
                out.push((cmd, None, 0));
            }
        } else {
            for (_, cmd) in self.all_commands.iter() {
                if let Some((indices, score)) = fuzzy_match(cmd.command(), filter) {
                    out.push((cmd, Some(indices), score));
                }
            }
        }
        out.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.command().cmp(b.0.command())));
        out
    }

    /// Compute filtered custom commands.
    fn filtered_custom(&self) -> Vec<(&CustomCommand, Option<Vec<usize>>, i32)> {
        let filter = self.command_filter.trim();
        let mut out: Vec<(&CustomCommand, Option<Vec<usize>>, i32)> = Vec::new();
        if filter.is_empty() {
            for cmd in self.custom_commands.iter() {
                out.push((cmd, None, 0));
            }
        } else {
            for cmd in self.custom_commands.iter() {
                if let Some((indices, score)) = fuzzy_match(cmd.command(), filter) {
                    out.push((cmd, Some(indices), score));
                }
            }
        }
        out.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.command().cmp(b.0.command())));
        out
    }

    fn filtered_commands(&self) -> Vec<&SlashCommand> {
        self.filtered().into_iter().map(|(c, _, _)| c).collect()
    }


    /// Get all filtered commands (both built-in and custom) with match indices and scores.
    fn filtered_all_with_indices(&self) -> Vec<(CommandType, Option<Vec<usize>>, i32)> {
        let mut result = Vec::new();
        
        // Add built-in commands
        for (cmd, indices, score) in self.filtered() {
            result.push((CommandType::BuiltIn(cmd), indices, score));
        }
        
        // Add custom commands
        for (cmd, indices, score) in self.filtered_custom() {
            result.push((CommandType::Custom(cmd), indices, score));
        }
        
        result
    }

    /// Get all filtered commands (both built-in and custom).
    fn filtered_all(&self) -> Vec<CommandType> {
        self.filtered_all_with_indices()
            .into_iter()
            .map(|(cmd, _, _)| cmd)
            .collect()
    }

    /// Move the selection cursor one step up.
    pub(crate) fn move_up(&mut self) {
        let matches = self.filtered_all();
        let len = matches.len();
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    /// Move the selection cursor one step down.
    pub(crate) fn move_down(&mut self) {
        let matches = self.filtered_all();
        let matches_len = matches.len();
        self.state.move_down_wrap(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    /// Return currently selected command, if any.
    pub(crate) fn selected_command(&self) -> Option<CommandType> {
        let matches = self.filtered_all();
        self.state
            .selected_idx
            .and_then(|idx| matches.get(idx).cloned())
    }
}

impl WidgetRef for CommandPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let all_matches = self.filtered_all_with_indices();
        
        let mut rows_all: Vec<GenericDisplayRow> = Vec::new();
        
        for (cmd_type, indices, _) in all_matches {
            let (name, description) = match cmd_type {
                CommandType::BuiltIn(cmd) => {
                    (cmd.command(), cmd.description())
                }
                CommandType::Custom(cmd) => {
                    (cmd.command(), cmd.description())
                }
            };
            
            rows_all.push(GenericDisplayRow {
                name: format!("/{}", name),
                match_indices: indices.map(|v| v.into_iter().map(|i| i + 1).collect()),
                is_current: false,
                description: Some(description.to_string()),
            });
        }
        
        render_rows(area, buf, &rows_all, &self.state, MAX_POPUP_ROWS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_includes_init_when_typing_prefix() {
        let mut popup = CommandPopup::new(Vec::new());
        // Simulate the composer line starting with '/in' so the popup filters
        // matching commands by prefix.
        popup.on_composer_text_change("/in".to_string());

        // Access the filtered list via the selected command and ensure that
        // one of the matches is the new "init" command.
        let matches = popup.filtered_commands();
        assert!(
            matches.iter().any(|cmd| cmd.command() == "init"),
            "expected '/init' to appear among filtered commands"
        );
    }

    #[test]
    fn selecting_init_by_exact_match() {
        let mut popup = CommandPopup::new(Vec::new());
        popup.on_composer_text_change("/init".to_string());

        // When an exact match exists, the selected command should be that
        // command by default.
        let selected = popup.selected_command();
        match selected {
            Some(CommandType::BuiltIn(cmd)) => assert_eq!(cmd.command(), "init"),
            Some(CommandType::Custom(_)) => panic!("expected built-in command, got custom"),
            None => panic!("expected a selected command for exact match"),
        }
    }
}
