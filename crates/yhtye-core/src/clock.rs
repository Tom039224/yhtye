//! The wall clock, in one place.

/// Milliseconds since the Unix epoch: 0 for a clock set before it, and
/// saturating (not wrapping) for one too far ahead.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
