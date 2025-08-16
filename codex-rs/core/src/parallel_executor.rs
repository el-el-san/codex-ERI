// Executor for parallel tool calls with rate limiting and error handling

use crate::models::ResponseItem;
use crate::rate_limiter::{RateLimiter, retry_with_backoff};
use crate::protocol::{
    ParallelExecutionStartEvent, 
    ParallelExecutionProgressEvent,
    ParallelExecutionEndEvent,
    EventMsg,
    Event,
};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use futures::future::join_all;
use uuid::Uuid;

/// Result of parallel execution
#[derive(Debug)]
pub struct ParallelExecutionResult {
    pub successful: usize,
    pub failed: usize,
    pub duration_ms: u64,
    pub results: Vec<Result<serde_json::Value, String>>,
}

/// Execute multiple tool calls in parallel with rate limiting
pub async fn execute_parallel<F, Fut>(
    items: Vec<ResponseItem>,
    rate_limiter: Arc<RwLock<RateLimiter>>,
    execute_fn: F,
    event_sender: Option<tokio::sync::mpsc::Sender<Event>>,
) -> ParallelExecutionResult
where
    F: Fn(ResponseItem) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>> + Send,
{
    let start_time = Instant::now();
    let group_id = Uuid::new_v4().to_string();
    let total_count = items.len();
    
    // Send parallel execution start event
    if let Some(ref sender) = event_sender {
        let tool_names: Vec<String> = items.iter()
            .filter_map(|item| {
                if let ResponseItem::FunctionCall { name, .. } = item {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        
        let start_event = Event {
            id: Uuid::new_v4().to_string(),
            msg: EventMsg::ParallelExecutionStart(ParallelExecutionStartEvent {
                group_id: group_id.clone(),
                tools: tool_names,
                total_count,
            }),
        };
        
        let _ = sender.send(start_event).await;
    }
    
    // Execute tools with rate limiting
    let mut handles = vec![];
    let completed_count = Arc::new(tokio::sync::Mutex::new(0usize));
    
    for item in items {
        let limiter = rate_limiter.clone();
        let execute = execute_fn.clone();
        let sender = event_sender.clone();
        let group_id_clone = group_id.clone();
        let completed = completed_count.clone();
        
        let handle = tokio::spawn(async move {
            // Acquire rate limit permit
            let _permit = limiter.read().await.acquire().await;
            
            // Execute with retry logic
            let config = limiter.read().await.config().clone();
            let result = retry_with_backoff(
                || execute(item.clone()),
                &config,
            ).await;
            
            // Update progress
            let mut count = completed.lock().await;
            *count += 1;
            let current_count = *count;
            
            // Send progress event
            if let Some(sender) = sender {
                let tool_name = if let ResponseItem::FunctionCall { name, .. } = &item {
                    Some(name.clone())
                } else {
                    None
                };
                
                let progress_event = Event {
                    id: Uuid::new_v4().to_string(),
                    msg: EventMsg::ParallelExecutionProgress(ParallelExecutionProgressEvent {
                        group_id: group_id_clone,
                        completed: current_count,
                        total: total_count,
                        completed_tool: tool_name,
                    }),
                };
                
                let _ = sender.send(progress_event).await;
            }
            
            result
        });
        
        handles.push(handle);
    }
    
    // Wait for all executions to complete
    let results = join_all(handles).await;
    
    // Process results
    let mut successful = 0;
    let mut failed = 0;
    let mut final_results = vec![];
    
    for result in results {
        match result {
            Ok(Ok(value)) => {
                successful += 1;
                final_results.push(Ok(value));
            }
            Ok(Err(e)) => {
                failed += 1;
                final_results.push(Err(e));
            }
            Err(e) => {
                failed += 1;
                final_results.push(Err(format!("Task panic: {}", e)));
            }
        }
    }
    
    let duration_ms = start_time.elapsed().as_millis() as u64;
    
    // Send completion event
    if let Some(sender) = event_sender {
        let end_event = Event {
            id: Uuid::new_v4().to_string(),
            msg: EventMsg::ParallelExecutionEnd(ParallelExecutionEndEvent {
                group_id,
                successful,
                failed,
                duration_ms,
            }),
        };
        
        let _ = sender.send(end_event).await;
    }
    
    ParallelExecutionResult {
        successful,
        failed,
        duration_ms,
        results: final_results,
    }
}

/// Execute tools sequentially (fallback when parallel execution is not available)
pub async fn execute_sequential<F, Fut>(
    items: Vec<ResponseItem>,
    execute_fn: F,
) -> Vec<Result<serde_json::Value, String>>
where
    F: Fn(ResponseItem) -> Fut,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>>,
{
    let mut results = vec![];
    
    for item in items {
        let result = execute_fn(item).await;
        results.push(result);
    }
    
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limiter::RateLimitConfig;
    
    #[tokio::test]
    async fn test_sequential_execution() {
        let items = vec![
            ResponseItem::FunctionCall {
                id: None,
                name: "test1".to_string(),
                arguments: serde_json::json!({}).to_string(),
                call_id: "1".to_string(),
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "test2".to_string(),
                arguments: serde_json::json!({}).to_string(),
                call_id: "2".to_string(),
            },
        ];
        
        let results = execute_sequential(items, |item| async move {
            if let ResponseItem::FunctionCall { name, .. } = item {
                Ok(serde_json::json!({ "name": name }))
            } else {
                Err("Not a function call".to_string())
            }
        }).await;
        
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
    }
}
