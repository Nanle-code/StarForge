//! # Time Controller
//!
//! Provides ledger time manipulation for testing – advance, freeze, rewind,
//! and jump to specific timestamps.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents the simulated ledger time.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LedgerTime {
    /// Current ledger sequence number.
    pub sequence: u32,
    /// Unix timestamp (seconds) of the current ledger close time.
    pub timestamp: i64,
    /// Average close time between ledgers (seconds).
    pub close_seconds: u64,
    /// Whether time advancement is frozen.
    pub frozen: bool,
}

impl LedgerTime {
    /// Genesis ledger: sequence 1, timestamp = now.
    pub fn genesis() -> Self {
        Self {
            sequence: 1,
            timestamp: Utc::now().timestamp(),
            close_seconds: 5,
            frozen: false,
        }
    }

    /// Genesis ledger with a specific starting timestamp.
    pub fn genesis_at(timestamp: i64) -> Self {
        Self {
            sequence: 1,
            timestamp,
            close_seconds: 5,
            frozen: false,
        }
    }

    /// Advance by one ledger close.
    pub fn tick(&mut self) {
        if !self.frozen {
            self.sequence += 1;
            self.timestamp += self.close_seconds as i64;
        }
    }

    /// Advance by `n` ledger closes.
    pub fn advance(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Jump to a specific ledger sequence number.
    pub fn jump_to_sequence(&mut self, target: u32) {
        if target > self.sequence && !self.frozen {
            let delta = target - self.sequence;
            self.timestamp += delta as i64 * self.close_seconds as i64;
            self.sequence = target;
        }
    }

    /// Jump to a specific Unix timestamp.
    pub fn jump_to_timestamp(&mut self, target: i64) {
        if target > self.timestamp && !self.frozen {
            let delta_secs = target - self.timestamp;
            let ledgers = (delta_secs / self.close_seconds as i64).max(1) as u32;
            self.sequence += ledgers;
            self.timestamp = target;
        }
    }

    /// Set the close time interval between ledgers.
    pub fn set_close_seconds(&mut self, secs: u64) {
        self.close_seconds = secs;
    }

    /// Freeze time – prevents further advancement.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// Unfreeze time.
    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }

    /// Reset back to genesis.
    pub fn reset(&mut self) {
        *self = Self::genesis();
    }
}

// ── Time Controller ───────────────────────────────────────────────────────────

/// High-level controller for manipulating simulation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeController {
    /// Current ledger time state.
    pub ledger_time: LedgerTime,
    /// Saved time points that can be jumped back to.
    save_points: Vec<(String, LedgerTime)>,
}

impl TimeController {
    pub fn new() -> Self {
        Self {
            ledger_time: LedgerTime::genesis(),
            save_points: Vec::new(),
        }
    }

    pub fn with_genesis_timestamp(timestamp: i64) -> Self {
        Self {
            ledger_time: LedgerTime::genesis_at(timestamp),
            save_points: Vec::new(),
        }
    }

    /// Advance time by one ledger close.
    pub fn tick(&mut self) {
        self.ledger_time.tick();
    }

    /// Advance time by `n` ledger closes.
    pub fn advance(&mut self, n: u32) {
        self.ledger_time.advance(n);
    }

    /// Jump to a specific sequence number.
    pub fn jump_to_sequence(&mut self, target: u32) {
        self.ledger_time.jump_to_sequence(target);
    }

    /// Jump to a specific Unix timestamp.
    pub fn jump_to_timestamp(&mut self, target: i64) {
        self.ledger_time.jump_to_timestamp(target);
    }

    /// Freeze time.
    pub fn freeze(&mut self) {
        self.ledger_time.freeze();
    }

    /// Unfreeze time.
    pub fn unfreeze(&mut self) {
        self.ledger_time.unfreeze();
    }

    /// Set the ledger close interval in seconds.
    pub fn set_close_seconds(&mut self, secs: u64) {
        self.ledger_time.set_close_seconds(secs);
    }

    /// Save the current time point with a label.
    pub fn save_point(&mut self, label: &str) {
        self.save_points.push((label.to_string(), self.ledger_time));
    }

    /// Restore time to a previously saved point by label.
    pub fn restore_point(&mut self, label: &str) -> Option<()> {
        let idx = self.save_points.iter().position(|(l, _)| l == label)?;
        self.ledger_time = self.save_points[idx].1;
        Some(())
    }

    /// List all saved time points.
    pub fn list_save_points(&self) -> &[(String, LedgerTime)] {
        &self.save_points
    }

    /// Reset to genesis.
    pub fn reset(&mut self) {
        self.ledger_time = LedgerTime::genesis();
        self.save_points.clear();
    }

    /// Get the current clock time as a human-readable string.
    pub fn current_time_string(&self) -> String {
        let dt = DateTime::from_timestamp(self.ledger_time.timestamp, 0).unwrap_or_default();
        dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
    }
}

impl Default for TimeController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_sequence_is_one() {
        let tc = TimeController::new();
        assert_eq!(tc.ledger_time.sequence, 1);
    }

    #[test]
    fn tick_advances_sequence() {
        let mut tc = TimeController::new();
        let seq_before = tc.ledger_time.sequence;
        tc.tick();
        assert_eq!(tc.ledger_time.sequence, seq_before + 1);
    }

    #[test]
    fn advance_multiple_ledgers() {
        let mut tc = TimeController::new();
        tc.advance(10);
        assert_eq!(tc.ledger_time.sequence, 11);
    }

    #[test]
    fn frozen_time_does_not_advance() {
        let mut tc = TimeController::new();
        tc.freeze();
        tc.tick();
        tc.tick();
        assert_eq!(tc.ledger_time.sequence, 1);
    }

    #[test]
    fn unfreeze_resumes_advancement() {
        let mut tc = TimeController::new();
        tc.freeze();
        tc.advance(5);
        tc.unfreeze();
        tc.tick();
        assert_eq!(tc.ledger_time.sequence, 2);
    }

    #[test]
    fn jump_to_sequence() {
        let mut tc = TimeController::new();
        tc.jump_to_sequence(100);
        assert_eq!(tc.ledger_time.sequence, 100);
    }

    #[test]
    fn save_and_restore_point() {
        let mut tc = TimeController::new();
        tc.advance(50);
        tc.save_point("after_50");
        tc.advance(30);
        assert_eq!(tc.ledger_time.sequence, 81);
        tc.restore_point("after_50");
        assert_eq!(tc.ledger_time.sequence, 51);
    }

    #[test]
    fn restore_nonexistent_point_returns_none() {
        let mut tc = TimeController::new();
        assert!(tc.restore_point("nope").is_none());
    }

    #[test]
    fn set_close_seconds() {
        let mut tc = TimeController::new();
        tc.set_close_seconds(10);
        assert_eq!(tc.ledger_time.close_seconds, 10);
        let ts_before = tc.ledger_time.timestamp;
        tc.tick();
        assert_eq!(tc.ledger_time.timestamp - ts_before, 10);
    }

    #[test]
    fn reset_returns_to_genesis() {
        let mut tc = TimeController::new();
        tc.advance(100);
        tc.save_point("p1");
        tc.reset();
        assert_eq!(tc.ledger_time.sequence, 1);
        assert!(tc.list_save_points().is_empty());
    }

    #[test]
    fn current_time_string_is_formatted() {
        let tc = TimeController::new();
        let s = tc.current_time_string();
        assert!(s.contains("UTC"));
    }
}
