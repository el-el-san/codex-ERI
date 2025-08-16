// Parallel tool execution utilities

use crate::models::ResponseItem;

/// Represents a group of tools that can be executed in parallel
#[derive(Debug, Clone)]
pub struct ParallelToolGroup {
    pub items: Vec<ResponseItem>,
}

/// Analyzes a list of ResponseItems to identify groups that can be executed in parallel
pub fn identify_parallel_groups(items: Vec<ResponseItem>) -> Vec<ParallelToolGroup> {
    let mut groups = Vec::new();
    let mut current_group = Vec::new();
    
    for item in items {
        match &item {
            ResponseItem::FunctionCall { .. } => {
                // Check if this tool can be executed in parallel with current group
                if can_execute_in_parallel(&item, &current_group) {
                    current_group.push(item);
                } else {
                    // Start a new group
                    if !current_group.is_empty() {
                        groups.push(ParallelToolGroup { 
                            items: current_group.clone() 
                        });
                        current_group.clear();
                    }
                    current_group.push(item);
                }
            }
            ResponseItem::LocalShellCall { .. } => {
                // Shell calls generally cannot be parallelized due to side effects
                if !current_group.is_empty() {
                    groups.push(ParallelToolGroup { 
                        items: current_group.clone() 
                    });
                    current_group.clear();
                }
                groups.push(ParallelToolGroup { 
                    items: vec![item] 
                });
            }
            _ => {
                // Other items are processed sequentially
                if !current_group.is_empty() {
                    groups.push(ParallelToolGroup { 
                        items: current_group.clone() 
                    });
                    current_group.clear();
                }
                groups.push(ParallelToolGroup { 
                    items: vec![item] 
                });
            }
        }
    }
    
    // Add remaining items
    if !current_group.is_empty() {
        groups.push(ParallelToolGroup { 
            items: current_group 
        });
    }
    
    groups
}

/// Determines if a tool can be executed in parallel with existing group
fn can_execute_in_parallel(item: &ResponseItem, group: &[ResponseItem]) -> bool {
    if group.is_empty() {
        return true;
    }
    
    match item {
        ResponseItem::FunctionCall { name, .. } => {
            // Check if this is a read-only operation
            if is_read_only_tool(name) {
                // Can parallelize with other read-only tools
                group.iter().all(|g| match g {
                    ResponseItem::FunctionCall { name: g_name, .. } => {
                        is_read_only_tool(g_name)
                    }
                    _ => false,
                })
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Identifies read-only tools that can be safely parallelized
fn is_read_only_tool(name: &str) -> bool {
    match name {
        // File system read operations
        "read_file" | "list_files" | "search_files" | "glob_files" => true,
        // MCP tools - check if they start with read-only prefixes
        tool if tool.starts_with("mcp__") => {
            tool.contains("_read") || 
            tool.contains("_get") || 
            tool.contains("_list") ||
            tool.contains("_search")
        }
        _ => false,
    }
}

/// Information about parallel execution results
#[derive(Debug)]
pub struct ParallelExecutionResult {
    pub successful: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
}

impl ParallelExecutionResult {
    pub fn new() -> Self {
        Self {
            successful: 0,
            failed: 0,
            total_duration_ms: 0,
        }
    }
    
    pub fn record_success(&mut self) {
        self.successful += 1;
    }
    
    pub fn record_failure(&mut self) {
        self.failed += 1;
    }
}