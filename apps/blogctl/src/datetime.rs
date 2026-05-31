//! Shared timestamp-flag parser. `--published-at` (on `import`) and
//! `--sampled-at` (on `metrics update`) both want the same shape:
//! accept full RFC 3339, but also accept a bare `YYYY-MM-DD` (treated
//! as `T00:00:00Z`) since backfill from URLs like LinkedIn's typically
//! carries only the date.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time};

use crate::error::{Error, Result};

/// Which CLI flag we're parsing for — used to route to the right
/// `Error::Invalid*` variant so the user sees the actual flag name in
/// the error.
#[derive(Debug, Clone, Copy)]
pub enum Flag {
    PublishedAt,
    SampledAt,
}

/// Parse a timestamp string. Accepts:
///   - full RFC 3339 (`2026-05-09T14:32:00Z`, `2026-05-09T14:32:00-07:00`)
///   - bare date (`2026-05-09`), interpreted as `2026-05-09T00:00:00Z`
///
/// Anything else returns the matching `Error::Invalid*` variant with
/// the original input and the RFC 3339 parse error as the source — the
/// date-only path falls through to RFC 3339, so the surfaced error is
/// always the RFC 3339 parser's diagnostic.
pub fn parse_timestamp(value: &str, flag: Flag) -> Result<OffsetDateTime> {
    if let Ok(date) = Date::parse(value, &format_description!("[year]-[month]-[day]")) {
        return Ok(PrimitiveDateTime::new(date, Time::MIDNIGHT).assume_utc());
    }
    OffsetDateTime::parse(value, &Rfc3339).map_err(|source| match flag {
        Flag::PublishedAt => Error::InvalidPublishedAt {
            value: value.to_string(),
            source,
        },
        Flag::SampledAt => Error::InvalidSampledAt {
            value: value.to_string(),
            source,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn date_only_is_midnight_utc() {
        let parsed = parse_timestamp("2026-05-09", Flag::PublishedAt).unwrap();
        assert_eq!(parsed, datetime!(2026-05-09 00:00:00 UTC));
    }

    #[test]
    fn full_rfc3339_utc_parses() {
        let parsed = parse_timestamp("2026-05-09T14:32:00Z", Flag::PublishedAt).unwrap();
        assert_eq!(parsed, datetime!(2026-05-09 14:32:00 UTC));
    }

    #[test]
    fn full_rfc3339_with_offset_parses() {
        let parsed = parse_timestamp("2026-05-09T14:32:00-07:00", Flag::SampledAt).unwrap();
        assert_eq!(parsed, datetime!(2026-05-09 14:32:00 -7:00));
    }

    #[test]
    fn natural_language_rejected_as_published_at() {
        let err = parse_timestamp("yesterday", Flag::PublishedAt).unwrap_err();
        assert!(
            matches!(err, Error::InvalidPublishedAt { ref value, .. } if value == "yesterday"),
            "got: {err:?}"
        );
    }

    #[test]
    fn natural_language_rejected_as_sampled_at() {
        let err = parse_timestamp("yesterday", Flag::SampledAt).unwrap_err();
        assert!(
            matches!(err, Error::InvalidSampledAt { ref value, .. } if value == "yesterday"),
            "got: {err:?}"
        );
    }

    #[test]
    fn out_of_range_date_rejected() {
        // 2026-13-40 isn't a real calendar date — the date-only path
        // fails and the RFC 3339 fallback also rejects it.
        let err = parse_timestamp("2026-13-40", Flag::PublishedAt).unwrap_err();
        assert!(
            matches!(err, Error::InvalidPublishedAt { ref value, .. } if value == "2026-13-40"),
            "got: {err:?}"
        );
    }

    #[test]
    fn partial_date_rejected() {
        // Year-month with no day — neither parser accepts it.
        let err = parse_timestamp("2026-05", Flag::PublishedAt).unwrap_err();
        assert!(
            matches!(err, Error::InvalidPublishedAt { ref value, .. } if value == "2026-05"),
            "got: {err:?}"
        );
    }
}
