//! Time vocabulary shared across the workspace.
//!
//! Kataan owns the *type* — what an instant is, what precisions exist, and that
//! a value must parse. It does not own the *policy*: which document types carry
//! which time fields, and what they mean, belongs to the vault's ontology.
//!
//! Timestamps are RFC 3339, and only RFC 3339: either a `full-date`
//! (`2026-08-29`) or a `date-time` (`2026-08-29T12:00:00Z`). One grammar, no
//! dialect.
//!
//! Reduced precision — a bare `2006` or `2006-05` — is ISO 8601 but *not*
//! RFC 3339, and is rejected. A source that only knows the year cannot be
//! recorded as a date; leave the field unset and carry the imprecision
//! elsewhere rather than inventing a day.

use std::time::{SystemTime, UNIX_EPOCH};

use time::{format_description::well_known::Rfc3339, Date, Month, OffsetDateTime};

/// Seconds since the Unix epoch as a decimal string, or `"0"` if the system
/// clock is set before the epoch.
///
/// Retained for the search index, whose `indexed_at` is an internal cache stamp
/// rather than vault data. Vault-facing timestamps use [`iso8601_utc_now`].
pub fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

/// The current instant as ISO-8601 UTC with a `Z` suffix, e.g.
/// `2026-08-29T18:30:00Z`. This is the form every vault-facing timestamp takes.
pub fn iso8601_utc_now() -> String {
    OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

/// Which RFC 3339 production a timestamp is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// RFC 3339 `full-date`: `2006-05-18`. A calendar day is zone-relative and
    /// therefore not a point on the timeline.
    Day,
    /// RFC 3339 `date-time`: `2006-05-18T09:30:00Z`. A fixed point.
    Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimestampError {
    #[error("`{0}` is a Unix epoch; timestamps must be ISO-8601 (e.g. 2026-08-29T12:00:00Z)")]
    UnixEpoch(String),
    #[error("`{0}` has a time but no timezone; add `Z` or an offset like `+02:00`")]
    Zoneless(String),
    #[error("`{0}` is not a valid RFC 3339 date or date-time")]
    Unparseable(String),
}

/// A validated RFC 3339 timestamp that remembers which production it is.
///
/// Serializes back to exactly the text it was parsed from, so a round trip
/// never rewrites a `full-date` into a `date-time` with an invented clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    raw: String,
    precision: Precision,
}

impl Timestamp {
    pub fn parse(value: &str) -> Result<Self, TimestampError> {
        let raw = value.trim();
        if raw.is_empty() {
            return Err(TimestampError::Unparseable(value.to_owned()));
        }

        // A bare integer is almost always a Unix epoch, which the old
        // `unix_timestamp_string` wrote. Name it rather than calling it
        // unparseable, so the fix is obvious.
        if raw.chars().all(|character| character.is_ascii_digit()) && raw.len() != 4 {
            return Err(TimestampError::UnixEpoch(raw.to_owned()));
        }

        let precision = match raw.split_once('T') {
            // Datetimes must pin a zone: without one the instant is ambiguous.
            // Only the clock half is inspected, since the date half is full of
            // `-` separators.
            Some((_, clock)) => {
                if !(clock.ends_with('Z') || clock.contains('+') || clock.contains('-')) {
                    return Err(TimestampError::Zoneless(raw.to_owned()));
                }
                OffsetDateTime::parse(raw, &Rfc3339)
                    .map_err(|_| TimestampError::Unparseable(raw.to_owned()))?;
                Precision::Instant
            }
            None => parse_full_date(raw)?,
        };

        Ok(Self {
            raw: raw.to_owned(),
            precision,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn precision(&self) -> Precision {
        self.precision
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.raw)
    }
}

/// Parse an RFC 3339 `full-date` — `YYYY-MM-DD`, nothing shorter — validating
/// the calendar so `2026-02-30` and `2026-13-01` are rejected rather than
/// stored.
fn parse_full_date(raw: &str) -> Result<Precision, TimestampError> {
    let unparseable = || TimestampError::Unparseable(raw.to_owned());
    let parts: Vec<&str> = raw.split('-').collect();
    // `full-date` is exactly three fixed-width components. A bare `2006` or
    // `2006-05` is ISO 8601 reduced precision, which RFC 3339 does not define.
    let [year, month, day] = parts[..] else {
        return Err(unparseable());
    };
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return Err(unparseable());
    }
    let year: i32 = year.parse().map_err(|_| unparseable())?;
    let month = Month::try_from(month.parse::<u8>().map_err(|_| unparseable())?)
        .map_err(|_| unparseable())?;
    let day: u8 = day.parse().map_err(|_| unparseable())?;
    Date::from_calendar_date(year, month, day).map_err(|_| unparseable())?;
    Ok(Precision::Day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_both_rfc_3339_productions_and_remembers_which() {
        for (value, expected) in [
            ("2006-05-18", Precision::Day),
            ("2026-08-29T12:00:00Z", Precision::Instant),
            ("2026-08-29T12:00:00+02:00", Precision::Instant),
        ] {
            let parsed = Timestamp::parse(value).expect(value);
            assert_eq!(parsed.precision(), expected, "{value}");
            // Round-trips unchanged: a full-date never gains an invented clock.
            assert_eq!(parsed.as_str(), value);
        }
    }

    #[test]
    fn rejects_iso_8601_reduced_precision() {
        // Valid ISO 8601, not valid RFC 3339. Recording "the year is 2006" as a
        // date would have to invent a month and a day.
        for value in ["2006", "2006-05", "2006-5-18", "206-05-18"] {
            assert!(
                matches!(Timestamp::parse(value), Err(TimestampError::Unparseable(_))),
                "`{value}` should be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_unix_epoch_distinctly() {
        // The exact value kataan.toml carried before this change.
        assert_eq!(
            Timestamp::parse("1788013953"),
            Err(TimestampError::UnixEpoch("1788013953".to_owned()))
        );
        // Four digits is not an epoch — but it is not RFC 3339 either, so it
        // is rejected as unparseable rather than as an epoch.
        assert!(matches!(
            Timestamp::parse("2006"),
            Err(TimestampError::Unparseable(_))
        ));
    }

    #[test]
    fn rejects_a_datetime_without_a_zone() {
        assert_eq!(
            Timestamp::parse("2026-08-29T12:00:00"),
            Err(TimestampError::Zoneless("2026-08-29T12:00:00".to_owned()))
        );
    }

    #[test]
    fn rejects_impossible_calendar_dates() {
        for value in [
            "2026-02-30",
            "2026-13-01",
            "2026-00-01",
            "not_applicable",
            "",
        ] {
            assert!(
                matches!(Timestamp::parse(value), Err(TimestampError::Unparseable(_))),
                "`{value}` should be unparseable"
            );
        }
        // Leap years are real years.
        assert!(Timestamp::parse("2024-02-29").is_ok());
        assert!(Timestamp::parse("2026-02-29").is_err());
    }

    #[test]
    fn iso_now_is_parseable_and_utc() {
        let now = iso8601_utc_now();
        assert!(now.ends_with('Z'), "{now}");
        assert_eq!(
            Timestamp::parse(&now).unwrap().precision(),
            Precision::Instant
        );
    }
}
