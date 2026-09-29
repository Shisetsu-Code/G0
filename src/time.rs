#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockKind {
    /// Suitable for deadlines, timeouts and durations. Never moves backward.
    Monotonic,
    /// Civil/UTC time. May jump because of synchronization or administrative change.
    Wall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationNs(pub u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonotonicInstant(pub u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnixTimeNs(pub i128);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUse {
    Deadline,
    Timeout,
    Benchmark,
    Timestamp,
    Expiration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeIssue {
    WallClockUsedForDurationSensitiveOperation,
    MonotonicClockUsedAsCivilTimestamp,
}

pub fn validate_clock_use(
    clock: ClockKind,
    usage: TimeUse,
) -> Result<(), TimeIssue> {
    match (clock, usage) {
        (
            ClockKind::Wall,
            TimeUse::Deadline | TimeUse::Timeout | TimeUse::Benchmark,
        ) => Err(TimeIssue::WallClockUsedForDurationSensitiveOperation),
        (ClockKind::Monotonic, TimeUse::Timestamp | TimeUse::Expiration) => {
            Err(TimeIssue::MonotonicClockUsedAsCivilTimestamp)
        }
        _ => Ok(()),
    }
}

pub fn elapsed(
    start: MonotonicInstant,
    end: MonotonicInstant,
) -> Option<DurationNs> {
    end.0.checked_sub(start.0).map(DurationNs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeouts_require_monotonic_time() {
        assert_eq!(
            validate_clock_use(ClockKind::Wall, TimeUse::Timeout),
            Err(TimeIssue::WallClockUsedForDurationSensitiveOperation)
        );
        assert!(
            validate_clock_use(ClockKind::Monotonic, TimeUse::Timeout).is_ok()
        );
    }

    #[test]
    fn timestamps_require_wall_time() {
        assert_eq!(
            validate_clock_use(ClockKind::Monotonic, TimeUse::Timestamp),
            Err(TimeIssue::MonotonicClockUsedAsCivilTimestamp)
        );
    }

    #[test]
    fn elapsed_never_wraps() {
        assert_eq!(
            elapsed(MonotonicInstant(100), MonotonicInstant(50)),
            None
        );
    }
}
