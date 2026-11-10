//! Reading lengths of time people type, like `30d`.

use std::time::Duration;

const MAX_SECONDS: u64 = 50 * 365 * 86_400;

/// Parses a number and a unit: `m`, `h`, `d`, `w`, `mo` or `y`.
#[must_use]
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim().to_ascii_lowercase();
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().ok()?;
    let unit_seconds: u64 = match unit.trim() {
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600,
        "d" | "day" | "days" => 86_400,
        "w" | "week" | "weeks" => 604_800,
        "mo" | "month" | "months" => 2_592_000,
        "y" | "yr" | "year" | "years" => 31_536_000,
        _ => return None,
    };
    let seconds = number.checked_mul(unit_seconds)?;
    (1..=MAX_SECONDS)
        .contains(&seconds)
        .then(|| Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_common_forms() {
        for (text, seconds) in [
            ("30d", 2_592_000),
            ("2h", 7_200),
            ("90 min", 5_400),
            ("1w", 604_800),
            ("6mo", 15_552_000),
            ("1Y", 31_536_000),
            (" 45m ", 2_700),
        ] {
            assert_eq!(
                parse_duration(text),
                Some(Duration::from_secs(seconds)),
                "{text}"
            );
        }
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        for text in [
            "",
            "d",
            "30",
            "-5d",
            "1.5d",
            "3 parsecs",
            "0d",
            "99999999999y",
        ] {
            assert_eq!(parse_duration(text), None, "{text}");
        }
    }
}
