//! Exponential backoff reconnection logic.

use std::time::Duration;

/// Configuration for exponential backoff reconnection.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Initial delay before first reconnection attempt.
    pub initial_delay: Duration,
    /// Maximum delay between reconnection attempts.
    pub max_delay: Duration,
    /// Multiplier for each successive attempt.
    pub multiplier: f64,
    /// Maximum number of attempts (None = infinite).
    pub max_attempts: Option<u32>,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            multiplier: 2.0,
            max_attempts: None,
        }
    }
}

/// Tracks reconnection state and calculates delays.
#[derive(Debug)]
pub struct ReconnectState {
    config: ReconnectConfig,
    attempts: u32,
    current_delay: Duration,
}

impl ReconnectState {
    pub fn new(config: ReconnectConfig) -> Self {
        let initial_delay = config.initial_delay;
        Self {
            config,
            attempts: 0,
            current_delay: initial_delay,
        }
    }

    /// Returns the next delay, or None if max attempts exceeded.
    pub fn next_delay(&mut self) -> Option<Duration> {
        if let Some(max) = self.config.max_attempts {
            if self.attempts >= max {
                return None;
            }
        }

        let delay = self.current_delay;
        self.attempts += 1;

        // Calculate next delay with exponential backoff
        let next = Duration::from_secs_f64(
            (self.current_delay.as_secs_f64() * self.config.multiplier)
                .min(self.config.max_delay.as_secs_f64()),
        );
        self.current_delay = next;

        Some(delay)
    }

    /// Reset state after successful connection.
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.current_delay = self.config.initial_delay;
    }

    /// Get current attempt count.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}
