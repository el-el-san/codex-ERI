//! MCP server configuration management.
//! 
//! Provides functionality to update MCP server configurations in config.toml

use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use anyhow::{Result, Context};
use toml_edit::{DocumentMut, value, Item};
use crate::config_types::McpServerConfig;

/// Manages MCP server configuration persistence
pub struct McpConfigManager {
    config_path: PathBuf,
}

impl McpConfigManager {
    /// Create a new McpConfigManager with the given config path
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    /// Toggle the enabled state of a specific MCP server
    pub fn toggle_server(&self, server_name: &str) -> Result<bool> {
        let mut doc = self.load_config()?;
        
        // Navigate to mcp_servers section
        if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|item| item.as_table_mut()) {
            if let Some(server) = mcp_servers.get_mut(server_name).and_then(|item| item.as_table_mut()) {
                // Get current enabled state (default to true if not present)
                let current_enabled = server
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                
                // Toggle the state
                let new_enabled = !current_enabled;
                server["enabled"] = value(new_enabled);
                
                // Save the config
                self.save_config(&doc)?;
                
                return Ok(new_enabled);
            }
        }
        
        Err(anyhow::anyhow!("Server '{}' not found in config", server_name))
    }

    /// Set the enabled state of a specific MCP server
    pub fn set_server_enabled(&self, server_name: &str, enabled: bool) -> Result<()> {
        let mut doc = self.load_config()?;
        
        // Navigate to mcp_servers section
        if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|item| item.as_table_mut()) {
            if let Some(server) = mcp_servers.get_mut(server_name).and_then(|item| item.as_table_mut()) {
                server["enabled"] = value(enabled);
                
                // Save the config
                self.save_config(&doc)?;
                
                return Ok(());
            }
        }
        
        Err(anyhow::anyhow!("Server '{}' not found in config", server_name))
    }

    /// Get the current state of all MCP servers
    pub fn get_server_states(&self) -> Result<HashMap<String, bool>> {
        let doc = self.load_config()?;
        let mut states = HashMap::new();
        
        if let Some(mcp_servers) = doc.get("mcp_servers").and_then(|item| item.as_table()) {
            for (name, server) in mcp_servers.iter() {
                if let Some(server_table) = server.as_table() {
                    let enabled = server_table
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    states.insert(name.to_string(), enabled);
                }
            }
        }
        
        Ok(states)
    }

    /// Load the config.toml file
    fn load_config(&self) -> Result<DocumentMut> {
        let content = fs::read_to_string(&self.config_path)
            .with_context(|| format!("Failed to read config file: {:?}", self.config_path))?;
        
        content.parse::<DocumentMut>()
            .with_context(|| "Failed to parse config.toml")
    }

    /// Save the config.toml file
    fn save_config(&self, doc: &DocumentMut) -> Result<()> {
        let content = doc.to_string();
        fs::write(&self.config_path, content)
            .with_context(|| format!("Failed to write config file: {:?}", self.config_path))?;
        
        Ok(())
    }
}