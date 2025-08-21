use std::collections::HashMap;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;
use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;

/// MCP server information for display
#[derive(Clone, Debug)]
pub struct McpServerInfo {
    pub name: String,
    pub url_or_cmd: String,
    pub enabled: bool,
    pub connected: bool,
    pub tool_count: usize,
}

/// Popup for managing MCP server connections
pub struct McpPopup {
    servers: Vec<McpServerInfo>,
    state: ScrollState,
}

impl McpPopup {
    /// Create a new MCP popup with server information
    pub fn new(servers: Vec<McpServerInfo>) -> Self {
        let mut state = ScrollState::new();
        if !servers.is_empty() {
            state.selected_idx = Some(0);
        }
        Self { servers, state }
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        
        match self.state.selected_idx {
            Some(idx) if idx > 0 => {
                self.state.selected_idx = Some(idx - 1);
            }
            Some(0) => {
                // Wrap to bottom
                self.state.selected_idx = Some(self.servers.len() - 1);
            }
            None => {
                self.state.selected_idx = Some(0);
            }
            _ => {}
        }
        
        // Ensure visible
        let visible_rows = MAX_POPUP_ROWS.min(self.servers.len());
        self.state.ensure_visible(self.servers.len(), visible_rows);
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        
        let max_idx = self.servers.len() - 1;
        match self.state.selected_idx {
            Some(idx) if idx < max_idx => {
                self.state.selected_idx = Some(idx + 1);
            }
            Some(idx) if idx == max_idx => {
                // Wrap to top
                self.state.selected_idx = Some(0);
            }
            None => {
                self.state.selected_idx = Some(0);
            }
            _ => {}
        }
        
        // Ensure visible
        let visible_rows = MAX_POPUP_ROWS.min(self.servers.len());
        self.state.ensure_visible(self.servers.len(), visible_rows);
    }

    /// Get the currently selected server
    pub fn selected_server(&self) -> Option<&McpServerInfo> {
        self.state.selected_idx.and_then(|idx| self.servers.get(idx))
    }

    /// Update server information
    pub fn update_servers(&mut self, servers: Vec<McpServerInfo>) {
        self.servers = servers;
        // Clamp selection if needed
        if let Some(idx) = self.state.selected_idx {
            if idx >= self.servers.len() && !self.servers.is_empty() {
                self.state.selected_idx = Some(self.servers.len() - 1);
            }
        }
    }

    /// Toggle the selected server's enabled state
    pub fn toggle_selected(&mut self) -> Option<(String, bool)> {
        if let Some(idx) = self.state.selected_idx {
            if let Some(server) = self.servers.get_mut(idx) {
                server.enabled = !server.enabled;
                return Some((server.name.clone(), server.enabled));
            }
        }
        None
    }

    /// Calculate required height for the popup
    pub fn calculate_required_height(&self) -> u16 {
        self.servers.len().clamp(1, MAX_POPUP_ROWS) as u16
    }

    /// Convert servers to display rows
    fn to_display_rows(&self) -> Vec<GenericDisplayRow> {
        self.servers
            .iter()
            .enumerate()
            .map(|(idx, server)| {
                let status = if !server.enabled {
                    "[OFF]"
                } else if server.connected {
                    "[ON] "
                } else {
                    "[...]"  // Connecting
                };
                
                let name = format!("{} {}", status, server.name);
                
                let description = if server.enabled && server.connected {
                    Some(format!("{} tools", server.tool_count))
                } else {
                    Some(server.url_or_cmd.clone())
                };
                
                GenericDisplayRow {
                    name,
                    match_indices: None,
                    is_current: false,
                    description,
                }
            })
            .collect()
    }
}

impl WidgetRef for McpPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let rows = self.to_display_rows();
        let max_results = MAX_POPUP_ROWS.min(self.servers.len());
        render_rows(area, buf, &rows, &self.state, max_results);
    }
}