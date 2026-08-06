use std::collections::HashMap;
use std::time::{Duration, Instant};

/// In-memory per-email / per-IP windows for OTP send / validate.
/// Process-local only; restart clears all counters.
#[derive(Debug, Default)]
pub struct OtpRateLimiter {
    send_attempts: HashMap<String, Vec<Instant>>,
    validate_attempts: HashMap<String, Vec<Instant>>,
    send_by_ip: HashMap<String, Vec<Instant>>,
    validate_by_ip: HashMap<String, Vec<Instant>>,
}

impl OtpRateLimiter {
    pub const SEND_MAX: usize = 3;
    pub const SEND_WINDOW: Duration = Duration::from_secs(15 * 60);
    pub const VALIDATE_MAX: usize = 8;
    pub const VALIDATE_WINDOW: Duration = Duration::from_secs(15 * 60);
    /// Slightly looser than per-email send; blocks bulk abuse from one client.
    pub const SEND_IP_MAX: usize = 10;
    pub const VALIDATE_IP_MAX: usize = 20;

    pub fn new() -> Self {
        Self::default()
    }

    /// Returns Ok(()) if a send is allowed; does not consume quota.
    pub fn check_send(&mut self, email: &str) -> Result<(), ()> {
        Self::check(
            &mut self.send_attempts,
            email,
            Self::SEND_MAX,
            Self::SEND_WINDOW,
        )
    }

    /// Record a successful send against the email window.
    pub fn record_send(&mut self, email: &str) {
        Self::record(
            &mut self.send_attempts,
            email,
            Self::SEND_MAX,
            Self::SEND_WINDOW,
        );
    }

    /// Returns Ok(()) if a send is allowed for this IP; does not consume quota.
    /// Empty / missing IP skips the gate (Ok).
    pub fn check_send_ip(&mut self, ip: &str) -> Result<(), ()> {
        let key = ip.trim();
        if key.is_empty() {
            return Ok(());
        }
        Self::check(
            &mut self.send_by_ip,
            key,
            Self::SEND_IP_MAX,
            Self::SEND_WINDOW,
        )
    }

    /// Record a successful send against the IP window.
    pub fn record_send_ip(&mut self, ip: &str) {
        let key = ip.trim();
        if key.is_empty() {
            return;
        }
        Self::record(
            &mut self.send_by_ip,
            key,
            Self::SEND_IP_MAX,
            Self::SEND_WINDOW,
        );
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

    /// IP gate for validate (records on check). Empty IP skips.
    pub fn check_and_record_validate_ip(&mut self, ip: &str) -> Result<(), ()> {
        let key = ip.trim();
        if key.is_empty() {
            return Ok(());
        }
        Self::check_and_record(
            &mut self.validate_by_ip,
            key,
            Self::VALIDATE_IP_MAX,
            Self::VALIDATE_WINDOW,
        )
    }

    fn normalize_key(key: &str) -> Option<String> {
        let key = key.trim().to_lowercase();
        if key.is_empty() {
            None
        } else {
            Some(key)
        }
    }

    fn prune(entries: &mut Vec<Instant>, now: Instant, window: Duration) {
        entries.retain(|at| now.duration_since(*at) < window);
    }

    fn check(
        map: &mut HashMap<String, Vec<Instant>>,
        key: &str,
        max: usize,
        window: Duration,
    ) -> Result<(), ()> {
        let Some(key) = Self::normalize_key(key) else {
            return Err(());
        };

        let now = Instant::now();
        let entries = map.entry(key).or_default();
        Self::prune(entries, now, window);

        if entries.len() >= max {
            Err(())
        } else {
            Ok(())
        }
    }

    fn record(map: &mut HashMap<String, Vec<Instant>>, key: &str, max: usize, window: Duration) {
        let Some(key) = Self::normalize_key(key) else {
            return;
        };

        let now = Instant::now();
        let entries = map.entry(key).or_default();
        Self::prune(entries, now, window);

        // Cap growth if callers record without checking.
        if entries.len() < max {
            entries.push(now);
        }
    }

    fn check_and_record(
        map: &mut HashMap<String, Vec<Instant>>,
        key: &str,
        max: usize,
        window: Duration,
    ) -> Result<(), ()> {
        Self::check(map, key, max, window)?;
        Self::record(map, key, max, window);
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
            assert!(limiter.check_send(email).is_ok());
            limiter.record_send(email);
        }
        assert!(limiter.check_send(email).is_err());
        assert!(limiter.check_send("other@example.com").is_ok());
    }

    #[test]
    fn check_send_alone_does_not_burn_quota() {
        let mut limiter = OtpRateLimiter::new();
        let email = "user@example.com";

        for _ in 0..(OtpRateLimiter::SEND_MAX + 5) {
            assert!(limiter.check_send(email).is_ok());
        }
        limiter.record_send(email);
        assert!(limiter.check_send(email).is_ok());
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

    #[test]
    fn send_ip_window_is_independent_of_email() {
        let mut limiter = OtpRateLimiter::new();
        let ip = "203.0.113.10";

        for i in 0..OtpRateLimiter::SEND_IP_MAX {
            let email = format!("user{i}@example.com");
            assert!(limiter.check_send(&email).is_ok());
            assert!(limiter.check_send_ip(ip).is_ok());
            limiter.record_send(&email);
            limiter.record_send_ip(ip);
        }

        assert!(limiter.check_send("fresh@example.com").is_ok());
        assert!(limiter.check_send_ip(ip).is_err());
        assert!(limiter.check_send_ip("203.0.113.11").is_ok());
    }

    #[test]
    fn empty_ip_skips_ip_gate() {
        let mut limiter = OtpRateLimiter::new();
        assert!(limiter.check_send_ip("").is_ok());
        assert!(limiter.check_and_record_validate_ip("  ").is_ok());
    }
}
