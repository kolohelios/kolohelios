//! Human-readable metrics intervals, with the existing unit aliases and lengths.

use fundu::{CustomDurationParserBuilder, CustomTimeUnit, Multiplier, TimeUnit};
use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("expected an unsigned duration with a unit"))]
    Syntax,
    #[snafu(display("invalid duration: {source}"))]
    Parse { source: fundu::ParseError },
    #[snafu(display("duration exceeds the supported timestamp range"))]
    Range,
}

pub fn parse(value: &str) -> Result<time::Duration, Error> {
    let value = value.trim();
    if value != "0" && !has_explicit_units(value) {
        return Err(Error::Syntax);
    }
    use TimeUnit::*;
    let parser = CustomDurationParserBuilder::new()
        .time_units(&[
            CustomTimeUnit::with_default(NanoSecond, &["nanos", "nsec", "ns"]),
            CustomTimeUnit::with_default(MicroSecond, &["usec", "us", "µs"]),
            CustomTimeUnit::with_default(MilliSecond, &["millis", "msec", "ms"]),
            CustomTimeUnit::with_default(Second, &["seconds", "second", "secs", "sec", "s"]),
            CustomTimeUnit::with_default(Minute, &["minutes", "minute", "min", "mins", "m"]),
            CustomTimeUnit::with_default(Hour, &["hours", "hour", "hr", "hrs", "h"]),
            CustomTimeUnit::with_default(Day, &["days", "day", "d"]),
            CustomTimeUnit::with_default(Week, &["weeks", "week", "wk", "wks", "w"]),
            // Retain the existing fixed 30.44-day month, not a calendar month
            // or fundu's default month length.
            CustomTimeUnit::new(
                Second,
                &["months", "month", "M"],
                Some(Multiplier(2_630_016, 0)),
            ),
            CustomTimeUnit::with_default(Year, &["years", "year", "yr", "yrs", "y"]),
        ])
        .allow_time_unit_delimiter()
        .parse_multiple(None)
        .disable_exponent()
        .disable_infinity()
        .build();
    let duration = parser
        .parse(value)
        .map_err(|source| Error::Parse { source })?;
    let duration: std::time::Duration = duration.try_into().map_err(|_| Error::Range)?;
    time::Duration::try_from(duration).map_err(|_| Error::Range)
}

// fundu's compound parser can otherwise interpret a unitless term as seconds,
// including treating `1.2.3h` as two terms. Retain explicit units on every term.
fn has_explicit_units(value: &str) -> bool {
    let mut chars = value.chars().peekable();
    if chars.peek().is_none() {
        return false;
    }
    while chars.peek().is_some() {
        if !chars.peek().is_some_and(char::is_ascii_digit) {
            return false;
        }
        let mut dots = 0;
        while let Some(c) = chars.next_if(|c| c.is_ascii_digit() || *c == '.') {
            dots += usize::from(c == '.');
        }
        if dots > 1 {
            return false;
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.next_if(|c| c.is_alphabetic()).is_none() {
            return false;
        }
        while chars.next_if(|c| c.is_alphabetic()).is_some() {}
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_intervals_and_existing_aliases_keep_their_lengths() {
        for (value, seconds) in [
            ("7d", 604_800),
            ("1w", 604_800),
            ("24h", 86_400),
            ("1hour 12min 5s", 4_325),
            ("1h30m", 5_400),
            ("1 day 2 hrs", 93_600),
            ("1wk 1min", 604_860),
            ("1month", 2_630_016),
            ("1M", 2_630_016),
            ("1year", 31_557_600),
            ("0", 0),
            (" 7d ", 604_800),
        ] {
            assert_eq!(
                parse(value).unwrap(),
                time::Duration::seconds(seconds),
                "{value}"
            );
        }
        for value in ["1nanos", "1nsec", "1ns"] {
            assert_eq!(parse(value).unwrap(), time::Duration::nanoseconds(1));
        }
        for value in ["1usec", "1us", "1µs"] {
            assert_eq!(parse(value).unwrap(), time::Duration::microseconds(1));
        }
        for value in ["1millis", "1msec", "1ms"] {
            assert_eq!(parse(value).unwrap(), time::Duration::milliseconds(1));
        }
        assert_eq!(parse("4.2s").unwrap(), time::Duration::milliseconds(4_200));
        assert_eq!(parse("0.5h").unwrap(), time::Duration::minutes(30));
    }

    #[test]
    fn invalid_and_overflowing_intervals_are_errors() {
        for value in [
            "",
            "123",
            "1s 2",
            "12 3s",
            "1s +2s",
            "1s2.3.4h",
            "1.s.2h",
            "-1d",
            "+1d",
            "1fortnight",
            "NaN",
            "inf",
            "infinity",
            "1e3s",
            "1.2.3h",
            "999999999999999999999999999999999d",
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
    }
}
