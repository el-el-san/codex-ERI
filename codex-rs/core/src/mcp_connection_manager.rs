//! Connection manager for Model Context Protocol (MCP) servers.
//!
//! The [`McpConnectionManager`] owns one [`codex_mcp_client::McpClient`] per
//! configured server (keyed by the *server name*). It offers convenience
//! helpers to query the available tools across *all* servers and returns them
//! in a single aggregated map using the fully-qualified tool name
//! `"<server><MCP_TOOL_NAME_DELIMITER><tool>"` as the key.

use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use codex_mcp_client::McpClient;
use codex_mcp_client::McpTransport;
use mcp_types::ClientCapabilities;
use mcp_types::Implementation;
use mcp_types::Tool;

use serde_json::json;
use sha1::Digest;
use sha1::Sha1;
use tokio::task::JoinSet;
use tracing::info;
use tracing::warn;

use crate::config_types::McpServerConfig;

/// Delimiter used to separate the server name from the tool name in a fully
/// qualified tool name.
///
/// OpenAI requires tool names to conform to `^[a-zA-Z0-9_-]+$`, so we must
/// choose a delimiter from this character set.
const MCP_TOOL_NAME_DELIMITER: &str = "__";
const MAX_TOOL_NAME_LENGTH: usize = 64;

/// Timeout for the `tools/list` request.
const LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum number of concurrent MCP server connections
const MAX_CONCURRENT_CONNECTIONS: usize = 5;

/// Timeout for MCP server initialization
const MCP_INIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Map that holds a startup error for every MCP server that could **not** be
/// spawned successfully.
pub type ClientStartErrors = HashMap<String, anyhow::Error>;

fn qualify_tools(tools: Vec<ToolInfo>) -> HashMap<String, ToolInfo> {
    let mut used_names = HashSet::new();
    let mut qualified_tools = HashMap::new();
    for tool in tools {
        let mut qualified_name = format!(
            "{}{}{}",
            tool.server_name, MCP_TOOL_NAME_DELIMITER, tool.tool_name
        );
        if qualified_name.len() > MAX_TOOL_NAME_LENGTH {
            let mut hasher = Sha1::new();
            hasher.update(qualified_name.as_bytes());
            let sha1 = hasher.finalize();
            let sha1_str = format!("{sha1:x}");

            // Truncate to make room for the hash suffix
            let prefix_len = MAX_TOOL_NAME_LENGTH - sha1_str.len();

            qualified_name = format!("{}{}", &qualified_name[..prefix_len], sha1_str);
        }

        if used_names.contains(&qualified_name) {
            warn!("skipping duplicated tool {}", qualified_name);
            continue;
        }

        used_names.insert(qualified_name.clone());
        qualified_tools.insert(qualified_name, tool);
    }

    qualified_tools
}

struct ToolInfo {
    server_name: String,
    tool_name: String,
    tool: Tool,
}

/// A thin wrapper around a set of running [`McpClient`] instances.
#[derive(Default)]
pub(crate) struct McpConnectionManager {
    /// Server-name -> client instance.
    ///
    /// The server name originates from the keys of the `mcp_servers` map in
    /// the user configuration.
    clients: HashMap<String, std::sync::Arc<McpClient>>,

    /// Fully qualified tool name -> tool instance.
    tools: HashMap<String, ToolInfo>,
    
    /// Server configuration for tracking enabled state
    server_configs: HashMap<String, McpServerConfig>,
}

impl McpConnectionManager {
    /// Spawn a [`McpClient`] for each configured server.
    ///
    /// * `mcp_servers` – Map loaded from the user configuration where *keys*
    ///   are human-readable server identifiers and *values* are the spawn
    ///   instructions.
    ///
    /// Servers that fail to start are reported in `ClientStartErrors`: the
    /// user should be informed about these errors.
    pub async fn new(
        mcp_servers: HashMap<String, McpServerConfig>,
    ) -> Result<(Self, ClientStartErrors)> {
        // Early exit if no servers are configured.
        if mcp_servers.is_empty() {
            return Ok((Self::default(), ClientStartErrors::default()));
        }

        let mut errors = ClientStartErrors::new();
        let mut clients: HashMap<String, std::sync::Arc<McpClient>> = HashMap::new();
        
        // Process servers in batches to avoid overwhelming the system
        let servers: Vec<_> = mcp_servers.into_iter().collect();
        let total_servers = servers.len();
        
        info!("Starting {} MCP servers in batches of {}", total_servers, MAX_CONCURRENT_CONNECTIONS);
        
        for (batch_idx, batch) in servers.chunks(MAX_CONCURRENT_CONNECTIONS).enumerate() {
            let batch_start = batch_idx * MAX_CONCURRENT_CONNECTIONS;
            let batch_end = std::cmp::min(batch_start + batch.len(), total_servers);
            info!("Processing batch {}/{}: servers {}-{} of {}", 
                batch_idx + 1, 
                (total_servers + MAX_CONCURRENT_CONNECTIONS - 1) / MAX_CONCURRENT_CONNECTIONS,
                batch_start + 1, 
                batch_end, 
                total_servers
            );

            let mut join_set = JoinSet::new();
            
            for (server_name, cfg) in batch {
            // Skip disabled servers
            if !cfg.is_enabled() {
                info!("Skipping disabled MCP server: {}", server_name);
                continue;
            }
            
            // Validate server name before spawning
            if !is_valid_mcp_server_name(&server_name) {
                let error = anyhow::anyhow!(
                    "invalid server name '{}': must match pattern ^[a-zA-Z0-9_-]+$",
                    server_name
                );
                errors.insert(server_name.to_string(), error);
                continue;
            }
            
            let server_name = server_name.clone();
            let cfg = cfg.clone();

            join_set.spawn(async move {
                let (transport, args, env) = match cfg {
                    McpServerConfig::Stdio { command, args, env } => {
                        let mut all_args = vec![OsString::from(command)];
                        all_args.extend(args.into_iter().map(OsString::from));
                        (McpTransport::Stdio, all_args, env)
                    }
                    McpServerConfig::Http { url, env } => {
                        (McpTransport::Http { url }, vec![], env)
                    }
                };
                
                info!("Connecting to MCP server: {}", server_name.clone());
                
                let client_res = McpClient::new(transport, args, env).await;
                match client_res {
                    Ok(client) => {
                        // Initialize the client.
                        let params = mcp_types::InitializeRequestParams {
                            capabilities: ClientCapabilities {
                                experimental: None,
                                roots: None,
                                sampling: None,
                                // https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation#capabilities
                                // indicates this should be an empty object.
                                elicitation: Some(json!({})),
                            },
                            client_info: Implementation {
                                name: "codex-mcp-client".to_owned(),
                                version: env!("CARGO_PKG_VERSION").to_owned(),
                                title: Some("Codex".into()),
                            },
                            protocol_version: mcp_types::MCP_SCHEMA_VERSION.to_owned(),
                        };
                        let initialize_notification_params = None;
                        // Use extended timeout for MCP server initialization
                        let timeout = Some(MCP_INIT_TIMEOUT);
                        match client
                            .initialize(params, initialize_notification_params, timeout)
                            .await
                        {
                            Ok(_response) => (server_name.clone(), Ok(client)),
                            Err(e) => (server_name.clone(), Err(e)),
                        }
                    }
                    Err(e) => (server_name.clone(), Err(e.into())),
                }
            });
            }

            // Process batch results
            let mut batch_success = 0;
            let mut batch_failed = 0;
            
            while let Some(res) = join_set.join_next().await {
                let (server_name, client_res) = res?; // JoinError propagation

                match client_res {
                    Ok(client) => {
                        info!("✓ Successfully connected to MCP server: {}", server_name);
                        clients.insert(server_name, std::sync::Arc::new(client));
                        batch_success += 1;
                    }
                    Err(e) => {
                        warn!("✗ Failed to connect to MCP server {}: {:#}", server_name, e);
                        errors.insert(server_name, e);
                        batch_failed += 1;
                    }
                }
            }
            
            info!("Batch {} complete: {} successful, {} failed", 
                batch_idx + 1, batch_success, batch_failed);
            
            // Add a small delay between batches to avoid overwhelming the system
            if batch_idx < servers.chunks(MAX_CONCURRENT_CONNECTIONS).count() - 1 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        
        info!("MCP server initialization complete: {} connected, {} failed", 
            clients.len(), errors.len());

        let all_tools = list_all_tools(&clients).await?;

        let tools = qualify_tools(all_tools);
        
        // Store server configs for later reference
        let server_configs = servers.into_iter()
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect();

        Ok((Self { clients, tools, server_configs }, errors))
    }

    /// Returns a single map that contains **all** tools. Each key is the
    /// fully-qualified name for the tool.
    pub fn list_all_tools(&self) -> HashMap<String, Tool> {
        let all_tools: HashMap<String, Tool> = self.tools
            .iter()
            .map(|(name, tool)| (name.clone(), tool.tool.clone()))
            .collect();
        
        // Debug logging for MCP tools
        info!("=== MCP Tools Debug ===");
        info!("Total MCP tools loaded: {} tools", all_tools.len());
        
        // Show size of first 5 tools as samples
        for (name, tool) in all_tools.iter().take(5) {
            let tool_json = serde_json::to_string(&tool).unwrap_or_default();
            info!("  Tool '{}': {} bytes", name, tool_json.len());
        }
        
        // Calculate total size
        let total_json = serde_json::to_string(&all_tools).unwrap_or_default();
        info!("Total MCP tools JSON size: {} bytes (~{} KB)", 
            total_json.len(), 
            total_json.len() / 1024);
        info!("=======================");
        
        all_tools
    }

    /// Invoke the tool indicated by the (server, tool) pair.
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<serde_json::Value>,
        timeout: Option<Duration>,
    ) -> Result<mcp_types::CallToolResult> {
        let client = self
            .clients
            .get(server)
            .ok_or_else(|| anyhow!("unknown MCP server '{server}'"))?
            .clone();

        client
            .call_tool(tool.to_string(), arguments, timeout)
            .await
            .with_context(|| format!("tool call failed for `{server}/{tool}`"))
    }

    pub fn parse_tool_name(&self, tool_name: &str) -> Option<(String, String)> {
        self.tools
            .get(tool_name)
            .map(|tool| (tool.server_name.clone(), tool.tool_name.clone()))
    }
    
    /// Get information about all MCP servers and their status
    pub fn get_server_info(&self) -> Vec<(String, McpServerConfig, bool, usize)> {
        let mut info = Vec::new();
        
        for (name, config) in &self.server_configs {
            let is_connected = self.clients.contains_key(name);
            let tool_count = self.tools
                .values()
                .filter(|tool| tool.server_name == *name)
                .count();
            
            info.push((name.clone(), config.clone(), is_connected, tool_count));
        }
        
        info.sort_by(|a, b| a.0.cmp(&b.0));
        info
    }
}

/// Query every server for its available tools and return a single map that
/// contains **all** tools. Each key is the fully-qualified name for the tool.
async fn list_all_tools(
    clients: &HashMap<String, std::sync::Arc<McpClient>>,
) -> Result<Vec<ToolInfo>> {
    let mut join_set = JoinSet::new();

    // Spawn one task per server so we can query them concurrently. This
    // keeps the overall latency roughly at the slowest server instead of
    // the cumulative latency.
    for (server_name, client) in clients {
        let server_name_cloned = server_name.clone();
        let client_clone = client.clone();
        join_set.spawn(async move {
            let res = client_clone
                .list_tools(None, Some(LIST_TOOLS_TIMEOUT))
                .await;
            (server_name_cloned, res)
        });
    }

    let mut aggregated: Vec<ToolInfo> = Vec::with_capacity(join_set.len());

    while let Some(join_res) = join_set.join_next().await {
        let (server_name, list_result) = join_res?;
        
        // Skip servers that don't support tools/list or have errors
        match list_result {
            Ok(list_result) => {
                for tool in list_result.tools {
                    let tool_info = ToolInfo {
                        server_name: server_name.clone(),
                        tool_name: tool.name.clone(),
                        tool,
                    };
                    aggregated.push(tool_info);
                }
            }
            Err(e) => {
                // Log warning but continue with other servers
                tracing::warn!("Server '{}' failed to list tools: {}", server_name, e);
            }
        }
    }

    info!(
        "aggregated {} tools from {} servers",
        aggregated.len(),
        clients.len()
    );

    Ok(aggregated)
}

fn is_valid_mcp_server_name(server_name: &str) -> bool {
    !server_name.is_empty()
        && server_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mcp_types::ToolInputSchema;

    fn create_test_tool(server_name: &str, tool_name: &str) -> ToolInfo {
        ToolInfo {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            tool: Tool {
                annotations: None,
                description: Some(format!("Test tool: {tool_name}")),
                input_schema: ToolInputSchema {
                    properties: None,
                    required: None,
                    r#type: "object".to_string(),
                },
                name: tool_name.to_string(),
                output_schema: None,
                title: None,
            },
        }
    }

    #[test]
    fn test_qualify_tools_short_non_duplicated_names() {
        let tools = vec![
            create_test_tool("server1", "tool1"),
            create_test_tool("server1", "tool2"),
        ];

        let qualified_tools = qualify_tools(tools);

        assert_eq!(qualified_tools.len(), 2);
        assert!(qualified_tools.contains_key("server1__tool1"));
        assert!(qualified_tools.contains_key("server1__tool2"));
    }

    #[test]
    fn test_qualify_tools_duplicated_names_skipped() {
        let tools = vec![
            create_test_tool("server1", "duplicate_tool"),
            create_test_tool("server1", "duplicate_tool"),
        ];

        let qualified_tools = qualify_tools(tools);

        // Only the first tool should remain, the second is skipped
        assert_eq!(qualified_tools.len(), 1);
        assert!(qualified_tools.contains_key("server1__duplicate_tool"));
    }

    #[test]
    fn test_qualify_tools_long_names_same_server() {
        let server_name = "my_server";

        let tools = vec![
            create_test_tool(
                server_name,
                "extremely_lengthy_function_name_that_absolutely_surpasses_all_reasonable_limits",
            ),
            create_test_tool(
                server_name,
                "yet_another_extremely_lengthy_function_name_that_absolutely_surpasses_all_reasonable_limits",
            ),
        ];

        let qualified_tools = qualify_tools(tools);

        assert_eq!(qualified_tools.len(), 2);

        let mut keys: Vec<_> = qualified_tools.keys().cloned().collect();
        keys.sort();

        assert_eq!(keys[0].len(), 64);
        assert_eq!(
            keys[0],
            "my_server__extremely_lena02e507efc5a9de88637e436690364fd4219e4ef"
        );

        assert_eq!(keys[1].len(), 64);
        assert_eq!(
            keys[1],
            "my_server__yet_another_e1c3987bd9c50b826cbe1687966f79f0c602d19ca"
        );
    }
}
