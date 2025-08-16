// Parallel execution logic for handling multiple tool calls concurrently

use crate::models::ResponseItem;
use crate::custom_command::CustomCommand;
use crate::rate_limiter::{RateLimiter, RateLimitConfig};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;

lazy_static::lazy_static! {
    /// Global rate limiter for parallel execution
    static ref RATE_LIMITER: Arc<RwLock<RateLimiter>> = {
        Arc::new(RwLock::new(RateLimiter::new(RateLimitConfig::default())))
    };
}

/// Checks if a set of items can be executed in parallel
pub async fn can_execute_parallel(items: &[ResponseItem]) -> bool {
    // Check if rate limiter allows parallel execution
    let limiter = RATE_LIMITER.read().await;
    if !limiter.is_parallel_enabled() {
        return false;
    }
    
    // Check if we have multiple items
    if items.len() <= 1 {
        return false;
    }
    
    // Limit parallel execution to avoid overwhelming the API
    // Only parallelize if we have 2-3 items (conservative approach)
    if items.len() > limiter.config().max_concurrent_calls {
        return false;
    }
    
    // All items must be function calls
    let all_function_calls = items.iter().all(|item| {
        matches!(item, ResponseItem::FunctionCall { .. })
    });
    
    if !all_function_calls {
        return false;
    }
    
    // Check for read-only operations
    items.iter().all(|item| {
        match item {
            ResponseItem::FunctionCall { name, arguments, .. } => {
                if name == "shell" {
                    is_safe_shell_command(arguments)
                } else {
                    is_safe_for_parallel(name)
                }
            }
            _ => false,
        }
    })
}

/// Get the rate limiter for acquiring permits
pub async fn get_rate_limiter() -> Arc<RwLock<RateLimiter>> {
    RATE_LIMITER.clone()
}

/// Update rate limiter configuration
pub async fn update_rate_limit_config(config: RateLimitConfig) {
    let mut limiter = RATE_LIMITER.write().await;
    *limiter = RateLimiter::new(config);
}

/// Determines if a function is safe for parallel execution
pub fn is_safe_for_parallel(function_name: &str) -> bool {
    match function_name {
        // Read operations are generally safe
        "read_file" | "list_files" | "search_files" | "glob_files" => true,
        // MCP read operations
        name if name.starts_with("mcp__") => {
            name.contains("_read") || 
            name.contains("_get") || 
            name.contains("_list") ||
            name.contains("_search") ||
            name.contains("_status")
        }
        // Write operations are not safe for parallel execution
        // Note: "shell" safety is checked separately by is_safe_shell_command()
        // so we don't reject it here
        "container.exec" | "apply_patch" | "update_plan" => false,
        // For "shell", we need to check the actual command with is_safe_shell_command
        // This is handled in can_execute_parallel, so return true here to allow the check
        "shell" => true,
        _ => false,
    }
}

/// Determine if a shell tool call is read-only and thus safe to parallelize
pub fn is_safe_shell_command(arguments: &str) -> bool {
    // Expect JSON string with shape { "command": ["cmd", ...] }
    if let Ok(val) = serde_json::from_str::<Value>(arguments) {
        if let Some(first_cmd) = val
            .get("command")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|v| v.as_str())
        {
            return matches!(
                first_cmd,
                "cat" | "ls" | "grep" | "head" | "tail" | "wc" | "find" | "pwd" | "echo"
            );
        }
    }
    false
}

/// Check if a custom command is marked for parallel execution
pub fn is_custom_command_parallel(command: &CustomCommand) -> bool {
    command.parallel
}

/// Build dependency graph for custom commands with depends_on
pub fn resolve_command_dependencies(commands: &[CustomCommand]) -> Vec<Vec<&CustomCommand>> {
    let mut execution_groups = Vec::new();
    let mut executed: Vec<String> = Vec::new();
    let mut remaining: Vec<&CustomCommand> = commands.iter().collect();
    
    while !remaining.is_empty() {
        let mut current_group = Vec::new();
        let mut next_remaining = Vec::new();
        
        for cmd in remaining {
            // Check if all dependencies are satisfied
            let deps_satisfied = cmd.depends_on.iter().all(|dep| executed.contains(dep));
            
            if deps_satisfied {
                current_group.push(cmd);
            } else {
                next_remaining.push(cmd);
            }
        }
        
        if current_group.is_empty() && !next_remaining.is_empty() {
            // Circular dependency detected, execute remaining commands sequentially
            for cmd in next_remaining {
                execution_groups.push(vec![cmd]);
            }
            break;
        }
        
        // Mark current group as executed
        for cmd in &current_group {
            executed.push(cmd.name.clone());
        }
        
        if !current_group.is_empty() {
            execution_groups.push(current_group);
        }
        remaining = next_remaining;
    }
    
    execution_groups
}

/// Information about parallel execution
#[derive(Debug)]
pub struct ParallelExecutionInfo {
    pub total_items: usize,
    pub parallel_groups: usize,
    pub parallelizable_items: usize,
}

impl ParallelExecutionInfo {
    pub fn from_items(items: &[ResponseItem]) -> Self {
        let parallelizable = items
            .iter()
            .filter(|item| match item {
                ResponseItem::FunctionCall { name, arguments, .. } => {
                    if name == "shell" {
                        is_safe_shell_command(arguments)
                    } else {
                        is_safe_for_parallel(name)
                    }
                }
                _ => false,
            })
            .count();
        
        Self {
            total_items: items.len(),
            parallel_groups: if parallelizable > 1 { 1 } else { 0 },
            parallelizable_items: parallelizable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serde_json::json;
    
    #[tokio::test]
    async fn test_parallel_group_identification() {
        // Test with multiple read operations that can be parallelized
        let items = vec![
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: json!({"path": "file1.txt"}).to_string(),
                call_id: "1".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "list_files".to_string(),
                arguments: json!({"path": "/"}).to_string(),
                call_id: "2".to_string(),
            },
        ];
        
        assert!(can_execute_parallel(&items).await);
    }
    
    #[test]
    fn test_safe_for_parallel_detection() {
        // Test safe operations
        assert!(is_safe_for_parallel("read_file"));
        assert!(is_safe_for_parallel("list_files"));
        assert!(is_safe_for_parallel("search_files"));
        assert!(is_safe_for_parallel("glob_files"));
        
        // Test MCP operations
        assert!(is_safe_for_parallel("mcp__tool_read"));
        assert!(is_safe_for_parallel("mcp__tool_get"));
        assert!(is_safe_for_parallel("mcp__tool_list"));
        assert!(is_safe_for_parallel("mcp__tool_search"));
        assert!(is_safe_for_parallel("mcp__tool_status"));
        
        // Test unsafe operations
        assert!(!is_safe_for_parallel("shell"));
        assert!(!is_safe_for_parallel("container.exec"));
        assert!(!is_safe_for_parallel("apply_patch"));
        assert!(!is_safe_for_parallel("update_plan"));
    }

    #[test]
    fn test_is_safe_shell_command() {
        // Safe read-only shell commands
        let args = json!({"command": ["ls", "-la"]}).to_string();
        assert!(is_safe_shell_command(&args));

        let args = json!({"command": ["cat", "file.txt"]}).to_string();
        assert!(is_safe_shell_command(&args));

        // Unsafe shell command (rm)
        let args = json!({"command": ["rm", "-rf", "/tmp/x"]}).to_string();
        assert!(!is_safe_shell_command(&args));

        // Missing/invalid structure
        assert!(!is_safe_shell_command("{}"));
        assert!(!is_safe_shell_command("not json"));
    }
    
    #[test]
    fn test_dependency_resolution() {
        let commands = vec![
            CustomCommand {
                name: "build".to_string(),
                description: "Build the project".to_string(),
                command_type: crate::custom_command::CustomCommandType::Shell,
                content: "cargo build".to_string(),
                parallel: false,
                depends_on: vec![],
            },
            CustomCommand {
                name: "test".to_string(),
                description: "Run tests".to_string(),
                command_type: crate::custom_command::CustomCommandType::Shell,
                content: "cargo test".to_string(),
                parallel: true,
                depends_on: vec!["build".to_string()],
            },
            CustomCommand {
                name: "lint".to_string(),
                description: "Run linter".to_string(),
                command_type: crate::custom_command::CustomCommandType::Shell,
                content: "cargo clippy".to_string(),
                parallel: true,
                depends_on: vec!["build".to_string()],
            },
        ];
        
        let groups = resolve_command_dependencies(&commands);
        
        // First group should contain only "build"
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[0][0].name, "build");
        
        // Second group should contain "test" and "lint" (can run in parallel)
        assert_eq!(groups[1].len(), 2);
        let names: Vec<String> = groups[1].iter().map(|c| c.name.clone()).collect();
        assert!(names.contains(&"test".to_string()));
        assert!(names.contains(&"lint".to_string()));
    }
    
    #[test]
    fn test_custom_command_parallel() {
        let parallel_cmd = CustomCommand {
            name: "search".to_string(),
            description: "Search files".to_string(),
            command_type: crate::custom_command::CustomCommandType::Shell,
            content: "grep pattern".to_string(),
            parallel: true,
            depends_on: vec![],
        };
        
        let sequential_cmd = CustomCommand {
            name: "write".to_string(),
            description: "Write file".to_string(),
            command_type: crate::custom_command::CustomCommandType::Shell,
            content: "echo test > file.txt".to_string(),
            parallel: false,
            depends_on: vec![],
        };
        
        assert!(is_custom_command_parallel(&parallel_cmd));
        assert!(!is_custom_command_parallel(&sequential_cmd));
    }
    
    #[test]
    fn test_circular_dependency_handling() {
        let commands = vec![
            CustomCommand {
                name: "a".to_string(),
                description: "Command A".to_string(),
                command_type: crate::custom_command::CustomCommandType::Shell,
                content: "echo a".to_string(),
                parallel: false,
                depends_on: vec!["b".to_string()],
            },
            CustomCommand {
                name: "b".to_string(),
                description: "Command B".to_string(),
                command_type: crate::custom_command::CustomCommandType::Shell,
                content: "echo b".to_string(),
                parallel: false,
                depends_on: vec!["a".to_string()],
            },
        ];
        
        let groups = resolve_command_dependencies(&commands);
        
        // Circular dependencies should result in sequential execution
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[1].len(), 1);
    }
}
