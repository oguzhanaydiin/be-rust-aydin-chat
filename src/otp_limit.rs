use std::collections::HashMap;
use std::time::{Duration, Instant};

/// In-memory per-email windows for OTP send / validate.
#[derive(Debug, Default)]
pub struct OtpRateLimiter {
    send_attempts: HashMap<String, Vec<Instant>>,
    validate_attempts: HashMap<String, Vec<Instant>>,
}

impl OtpRateLimiter {
    pub const SEND_MAX: usize = 3;
    pub const SEND_WINDOW: Duration = Duration::from_secs(15 * 60);
    pub const VALIDATE_MAX: usize = 8;
    pub const VALIDATE_WINDOW: Duration = Duration::from_secs(15 * 60);

    pub fn new() -> Self {
        Self::default()
    }

    /// Returns Ok(()) if allowed and records the attempt; Err(()) if limited.
    pub fn check_and_record_send(&mut self, email: &str) -> Result<(), ()> {
        Self::check_and_record(
            &mut self.send_attempts,
            email,
            Self::SEND_MAX,
            Self::SEND_WINDOW,
        )
    }

    /// Returns Ok(()) if allowed and records the attempt; Err(()) if limited.
    pub fn check_and_record_validate(&mut self, email: &str) -> Result<(), ()> {
        Self::check_and_record(
            &mut self.validate_attempts,
            email,
            Self::VALIDATE_MAX,
            Self::VALIDATE_WINDOW,
        )
    }

    fn check_and_record(
        map: &mut HashMap<String, Vec<Instant>>,
        email: &str,
        max: usize,
        window: Duration,
    ) -> Result<(), ()> {
        let key = email.trim().to_lowercase();
        if key.is_empty() {
            return Err(());
        }

        let now = Instant::now();
        let entries = map.entry(key).or_default();
        entries.retain(|at| now.duration_since(*at) < window);

        if entries.len() >= max {
            return Err(());
        }

        entries.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_allows_up_to_max_then_blocks() {
        let mut limiter = OtpRateLimiter::new();
        let email = "user@example.com";

        for _ in 0..OtpRateLimiter::SEND_MAX {
            assert!(limiter.check_and_record_send(email).is_ok());
        }
        assert!(limiter.check_and_record_send(email).is_err());
        assert!(limiter.check_and_record_send("other@example.com").is_ok());
    }

    #[test]
    fn validate_allows_up_to_max_then_blocks() {
        let mut limiter = OtpRateLimiter::new();
        let email = "user@example.com";

        for _ in 0..OtpRateLimiter::VALIDATE_MAX {
            assert!(limiter.check_and_record_validate(email).is_ok());
        }
        assert!(limiter.check_and_record_validate(email).is_err());
    }
}
