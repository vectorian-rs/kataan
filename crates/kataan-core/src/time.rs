//! Time vocabulary shared across the workspace.
//!
//! Kataan owns the *type* — what an instant is, what precisions exist, and that
//! a value must parse. It does not own the *policy*: which document types carry
//! which time fields, and what they mean, belongs to the vault's ontology.
//!
//! The governing rule is that precision is never widened. If a source says
//! `2006`, storing `2006-01-01` would assert a day we do not know, so year and
//! month precision are represented explicitly and round-trip unchanged.

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

/// How much of a timestamp the author actually knew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precision {
    /// `2006`
    Year,
    /// `2006-05`
    Month,
    /// `2006-05-18` — a calendar day, which is zone-relative and therefore not
    /// an instant.
    Day,
    /// `2006-05-18T09:30:00Z` — a fixed point on the timeline.
    Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimestampError {
    #[error("`{0}` is a Unix epoch; timestamps must be ISO-8601 (e.g. 2026-08-29T12:00:00Z)")]
    UnixEpoch(String),
    #[error("`{0}` has a time but no timezone; add `Z` or an offset like `+02:00`")]
    Zoneless(String),
    #[error("`{0}` is not a valid ISO-8601 timestamp")]
    Unparseable(String),
}

/// A validated timestamp that remembers how precise it is.
///
/// Serializes back to exactly the text it was parsed from, so a round trip
/// never rewrites an author's `2006` into `2006-01-01T00:00:00Z`.
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

        let precision = if raw.contains('T') {
            // Datetimes must pin a zone: without one the instant is ambiguous.
            if !(raw.ends_with('Z') || has_offset(raw)) {
                return Err(TimestampError::Zoneless(raw.to_owned()));
            }
            OffsetDateTime::parse(raw, &Rfc3339)
                .map_err(|_| TimestampError::Unparseable(raw.to_owned()))?;
            Precision::Instant
        } else {
            parse_civil(raw)?
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

/// Parse `YYYY`, `YYYY-MM`, or `YYYY-MM-DD`, validating the calendar so
/// `2026-02-30` and `2026-13-01` are rejected rather than stored.
fn parse_civil(raw: &str) -> Result<Precision, TimestampError> {
    let unparseable = || TimestampError::Unparseable(raw.to_owned());
    let mut parts = raw.split('-');
    let year: i32 = parts
        .next()
        .ok_or_else(unparseable)?
        .parse()
        .map_err(|_| unparseable())?;

    let Some(month) = parts.next() else {
        return Ok(Precision::Year);
    };
    let month: u8 = month.parse().map_err(|_| unparseable())?;
    let month = Month::try_from(month).map_err(|_| unparseable())?;

    let Some(day) = parts.next() else {
        return Ok(Precision::Month);
    };
    if parts.next().is_some() {
        return Err(unparseable());
    }
    let day: u8 = day.parse().map_err(|_| unparseable())?;
    Date::from_calendar_date(year, month, day).map_err(|_| unparseable())?;
    Ok(Precision::Day)
}

/// Whether a datetime carries a `+HH:MM` / `-HH:MM` offset. Only the time half
/// is inspected, since the date half is full of `-` separators.
fn has_offset(raw: &str) -> bool {
    raw.split_once('T')
        .is_some_and(|(_, clock)| clock.contains('+') || clock.contains('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_each_precision_and_remembers_it() {
        for (value, expected) in [
            ("2006", Precision::Year),
            ("2006-05", Precision::Month),
            ("2006-05-18", Precision::Day),
            ("2026-08-29T12:00:00Z", Precision::Instant),
            ("2026-08-29T12:00:00+02:00", Precision::Instant),
        ] {
            let parsed = Timestamp::parse(value).expect(value);
            assert_eq!(parsed.precision(), expected, "{value}");
            // Round-trips unchanged: precision is never widened on the way out.
            assert_eq!(parsed.as_str(), value);
        }
    }

    #[test]
    fn rejects_a_unix_epoch_distinctly() {
        // The exact value kataan.toml carried before this change.
        assert_eq!(
            Timestamp::parse("1788013953"),
            Err(TimestampError::UnixEpoch("1788013953".to_owned()))
        );
        // Four digits is a year, not an epoch.
        assert!(Timestamp::parse("2006").is_ok());
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
