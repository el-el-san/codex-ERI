// Batching logic for parallel execution

use std::time::{Duration, Instant};
use tracing::debug;

use crate::models::ResponseItem;

/// Manages batching of response items for parallel execution
pub struct ParallelBatcher {
    items: Vec<ResponseItem>,
    batch_timeout: Duration,
    last_item_time: Option<Instant>,
    max_batch_size: usize,
}

impl ParallelBatcher {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            batch_timeout: Duration::from_millis(100), // Wait 100ms for more items
            last_item_time: None,
            max_batch_size: 10, // Process at most 10 items in parallel
        }
    }
    
    /// Add an item to the batch
    pub fn add_item(&mut self, item: ResponseItem) {
        self.items.push(item);
        self.last_item_time = Some(Instant::now());
    }
    
    /// Check if the batch should be processed
    pub fn should_process_batch(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        
        // Process if we've reached max batch size
        if self.items.len() >= self.max_batch_size {
            debug!("Batch ready: reached max size of {}", self.max_batch_size);
            return true;
        }
        
        // Process if timeout has elapsed since last item
        if let Some(last_time) = self.last_item_time {
            if last_time.elapsed() >= self.batch_timeout {
                debug!("Batch ready: timeout elapsed");
                return true;
            }
        }
        
        false
    }
    
    /// Check if the batch contains only parallelizable items
    pub fn is_parallelizable(&self) -> bool {
        if self.items.len() <= 1 {
            return false;
        }
        
        // Check if all items are function calls
        self.items.iter().all(|item| {
            matches!(item, ResponseItem::FunctionCall { .. })
        })
    }
    
    /// Take all items from the batch
    pub fn take_items(&mut self) -> Vec<ResponseItem> {
        self.last_item_time = None;
        std::mem::take(&mut self.items)
    }
    
    /// Get the number of items in the batch
    pub fn len(&self) -> usize {
        self.items.len()
    }
    
    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}