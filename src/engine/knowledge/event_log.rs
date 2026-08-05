//! Bounded in-memory event log for engine activity.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::RwLock;

const MAX_LOG_ENTRIES: usize = 1000;

/// A single entry in the event log.
#[derive(Clone, Debug)]
pub struct EventLogEntry {
    /// When the event occurred.
    pub timestamp: SystemTime,
    /// Category or type label for the event.
    pub event_type: String,
    /// Human-readable description of the event.
    pub details: String,
}

/// Bounded in-memory event log (FIFO, up to 1000 entries).
#[derive(Clone, Debug)]
pub struct EventLog {
    entries: Arc<RwLock<VecDeque<EventLogEntry>>>,
}

impl EventLog {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_LOG_ENTRIES))),
        }
    }

    pub async fn push(&self, event_type: impl Into<String>, details: impl Into<String>) {
        let mut entries = self.entries.write().await;
        if entries.len() >= MAX_LOG_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(EventLogEntry {
            timestamp: SystemTime::now(),
            event_type: event_type.into(),
            details: details.into(),
        });
    }

    pub async fn recent_entries(&self, n: usize) -> Vec<EventLogEntry> {
        let entries = self.entries.read().await;
        let len = entries.len();
        let start = len.saturating_sub(n);
        entries.range(start..).cloned().collect()
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}
