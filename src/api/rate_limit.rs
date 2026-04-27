use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::Mutex;

use crate::error::AppError;

/// Simple token-bucket rate limiter keyed by API key.
///
/// Each API key gets `max_tokens` tokens that refill at a rate of
/// `max_tokens` per `window` (default: 10 per 60 seconds).
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<String, Bucket>>>,
    max_tokens: u32,
    window_secs: f64,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_tokens: requests_per_minute,
            window_secs: 60.0,
        }
    }

    /// Try to consume one token for the given key.
    /// Returns `true` if the request is allowed, `false` if rate-limited.
    async fn try_acquire(&self, key: &str) -> bool {
        let mut map = self.inner.lock().await;
        let now = Instant::now();

        // Evict stale entries (idle for > 5 minutes) to prevent unbounded growth
        if map.len() > 100 {
            let stale_cutoff = std::time::Duration::from_secs(300);
            map.retain(|_, b| now.duration_since(b.last_refill) < stale_cutoff);
        }

        let bucket = map.entry(key.to_string()).or_insert(Bucket {
            tokens: self.max_tokens as f64,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        let refill = elapsed * (self.max_tokens as f64 / self.window_secs);
        bucket.tokens = (bucket.tokens + refill).min(self.max_tokens as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Middleware that rate-limits requests based on the X-API-Key header.
///
/// This should be applied AFTER auth middleware so the API key header is present.
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<RateLimiter>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    // Extract API key from header (already validated by auth middleware)
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    if !limiter.try_acquire(&api_key).await {
        return Err(AppError::RateLimited);
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new(5);
        for _ in 0..5 {
            assert!(limiter.try_acquire("test-key").await);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(3);
        // Consume all tokens
        for _ in 0..3 {
            assert!(limiter.try_acquire("test-key").await);
        }
        // Next request should be blocked
        assert!(!limiter.try_acquire("test-key").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_separate_keys() {
        let limiter = RateLimiter::new(2);
        assert!(limiter.try_acquire("key-a").await);
        assert!(limiter.try_acquire("key-a").await);
        assert!(!limiter.try_acquire("key-a").await);
        // Different key should still have tokens
        assert!(limiter.try_acquire("key-b").await);
    }
}
