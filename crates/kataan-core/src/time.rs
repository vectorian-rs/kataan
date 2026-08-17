//! Small time helpers shared across the workspace.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch as a decimal string, or `"0"` if the system
/// clock is set before the epoch. Shared by the rebuild engine and the search
/// index so both stamp records the same way.
pub fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}
