// Rate limiter for parallel execution to avoid API rate limits

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::sleep;

/// Configuration for rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of concurrent API calls
    pub max_concurrent_calls: usize,
    /// Minimum delay between API calls in milliseconds
    pub min_delay_ms: u64,
    /// Whether parallel execution is enabled
    pub parallel_enabled: bool,
    /// Exponential backoff multiplier for retries
    pub backoff_multiplier: f64,
    /// Maximum retry attempts
    pub max_retries: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_concurrent_calls: 5,  // Increased for better parallelism
            min_delay_ms: 100,       // 100ms between calls
            parallel_enabled: true,   // Enable by default but with limits
            backoff_multiplier: 2.0,
            max_retries: 5,
        }
    }
}

/// Rate limiter for controlling API call frequency
pub struct RateLimiter {
    semaphore: Arc<Semaphore>,
    last_call_time: Arc<Mutex<Instant>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_calls)),
            last_call_time: Arc::new(Mutex::new(Instant::now())),
            config,
        }
    }

    /// Acquire a permit for making an API call
    pub async fn acquire(&self) -> RateLimitPermit {
        // Wait for semaphore permit
        let permit = self.semaphore.clone().acquire_owned().await
            .expect("Failed to acquire semaphore permit");
        
        // Enforce minimum delay between calls
        let mut last_time = self.last_call_time.lock().await;
        let elapsed = last_time.elapsed();
        let min_delay = Duration::from_millis(self.config.min_delay_ms);
        
        if elapsed < min_delay {
            let wait_time = min_delay - elapsed;
            sleep(wait_time).await;
        }
        
        *last_time = Instant::now();
        
        RateLimitPermit {
            _permit: permit,
        }
    }

    /// Check if parallel execution is enabled
    pub fn is_parallel_enabled(&self) -> bool {
        self.config.parallel_enabled && self.config.max_concurrent_calls > 1
    }

    /// Get the configuration
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

/// Permit for making an API call
pub struct RateLimitPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
}

/// Retry logic with exponential backoff
pub async fn retry_with_backoff<F, Fut, T, E>(
    f: F,
    config: &RateLimitConfig,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    let mut delay_ms = config.min_delay_ms;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if attempt < config.max_retries => {
                attempt += 1;
                let delay = Duration::from_millis(delay_ms);
                
                tracing::warn!(
                    "Attempt {} failed: {}. Retrying in {:?}...",
                    attempt, e, delay
                );
                
                sleep(delay).await;
                
                // Exponential backoff
                delay_ms = ((delay_ms as f64) * config.backoff_multiplier) as u64;
                delay_ms = delay_ms.min(60000); // Cap at 60 seconds
            }
            Err(e) => {
                tracing::error!("All {} retry attempts failed: {}", config.max_retries, e);
                return Err(e);
            }
        }
    }
}

/// Check if an error is a rate limit error
pub fn is_rate_limit_error(error_msg: &str) -> bool {
    error_msg.contains("rate limit") || 
    error_msg.contains("Rate limit") ||
    error_msg.contains("429") ||
    error_msg.contains("too many requests") ||
    error_msg.contains("Too Many Requests")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_delays() {
        let config = RateLimitConfig {
            max_concurrent_calls: 1,
            min_delay_ms: 50,
            ..Default::default()
        };
        
        let limiter = RateLimiter::new(config);
        
        let start = Instant::now();
        let _permit1 = limiter.acquire().await;
        let _permit2 = limiter.acquire().await;
        let elapsed = start.elapsed();
        
        // Should have at least min_delay_ms between calls
        assert!(elapsed >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_concurrent_limit() {
        let config = RateLimitConfig {
            max_concurrent_calls: 2,
            min_delay_ms: 0,
            ..Default::default()
        };
        
        let limiter = Arc::new(RateLimiter::new(config));
        
        // Try to acquire 3 permits concurrently (but limit is 2)
        let limiter1 = limiter.clone();
        let limiter2 = limiter.clone();
        let limiter3 = limiter.clone();
        
        let handle1 = tokio::spawn(async move {
            let _permit = limiter1.acquire().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        
        let handle2 = tokio::spawn(async move {
            let _permit = limiter2.acquire().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        
        let start = Instant::now();
        let handle3 = tokio::spawn(async move {
            // This should wait until one of the first two completes
            let _permit = limiter3.acquire().await;
        });
        
        handle1.await.unwrap();
        handle2.await.unwrap();
        handle3.await.unwrap();
        
        let elapsed = start.elapsed();
        // Third permit should have waited
        assert!(elapsed >= Duration::from_millis(100));
    }
}