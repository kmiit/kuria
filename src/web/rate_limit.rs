use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const LOGIN_MAX_FAILURES: u32 = 5;
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
struct FailureRecord {
    count: u32,
    first_failure: Instant,
}

#[derive(Debug, Default)]
pub struct LoginRateLimiter {
    failures: Mutex<HashMap<String, FailureRecord>>,
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_limited(&self, key: &str) -> bool {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        prune_expired(&mut failures);
        failures
            .get(key)
            .is_some_and(|record| record.count >= LOGIN_MAX_FAILURES)
    }

    pub fn record_failure(&self, key: &str) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        prune_expired(&mut failures);

        failures
            .entry(key.to_string())
            .and_modify(|record| record.count = record.count.saturating_add(1))
            .or_insert_with(|| FailureRecord {
                count: 1,
                first_failure: Instant::now(),
            });
    }

    pub fn record_success(&self, key: &str) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.remove(key);
    }
}

fn prune_expired(failures: &mut HashMap<String, FailureRecord>) {
    let now = Instant::now();
    failures.retain(|_, record| now.duration_since(record.first_failure) <= LOGIN_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_rate_limiter_blocks_after_repeated_failures() {
        let limiter = LoginRateLimiter::new();
        for _ in 0..LOGIN_MAX_FAILURES {
            assert!(!limiter.is_limited("user@example.com"));
            limiter.record_failure("user@example.com");
        }

        assert!(limiter.is_limited("user@example.com"));
    }

    #[test]
    fn login_rate_limiter_success_clears_failures() {
        let limiter = LoginRateLimiter::new();
        for _ in 0..LOGIN_MAX_FAILURES {
            limiter.record_failure("user@example.com");
        }
        assert!(limiter.is_limited("user@example.com"));

        limiter.record_success("user@example.com");
        assert!(!limiter.is_limited("user@example.com"));
    }
}
