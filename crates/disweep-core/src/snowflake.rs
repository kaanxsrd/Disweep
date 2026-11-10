use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

pub const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
#[ts(export, type = "string")]
pub struct Snowflake(pub u64);

impl Snowflake {
    #[must_use]
    pub fn from_timestamp_ms(unix_ms: i64) -> Self {
        let since_epoch = unix_ms.saturating_sub(DISCORD_EPOCH_MS).max(0);
        Self((since_epoch as u64) << 22)
    }

    #[must_use]
    pub fn timestamp_ms(self) -> i64 {
        (self.0 >> 22) as i64 + DISCORD_EPOCH_MS
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("not a valid Discord ID")]
pub struct InvalidSnowflake;

impl FromStr for Snowflake {
    type Err = InvalidSnowflake;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(InvalidSnowflake);
        }
        text.parse().map(Self).map_err(|_| InvalidSnowflake)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SnowflakeVisitor;

        impl Visitor<'_> for SnowflakeVisitor {
            type Value = Snowflake;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a Discord ID as a string or integer")
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(value))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Snowflake, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_any(SnowflakeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_to_and_from_a_timestamp() {
        let unix_ms = 1_700_000_000_123;
        assert_eq!(
            Snowflake::from_timestamp_ms(unix_ms).timestamp_ms(),
            unix_ms
        );
    }

    #[test]
    fn timestamps_before_the_discord_epoch_clamp_to_zero() {
        assert_eq!(Snowflake::from_timestamp_ms(0), Snowflake(0));
    }

    #[test]
    fn only_digits_parse() {
        assert!("175928847299117063".parse::<Snowflake>().is_ok());
        for bad in [
            "",
            "12a",
            "../messages",
            "1/2",
            "-5",
            " 7",
            "99999999999999999999999",
        ] {
            assert!(
                bad.parse::<Snowflake>().is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn reads_strings_and_bare_integers_and_writes_strings() {
        let from_text: Snowflake = serde_json::from_str("\"922337203685477580\"").unwrap();
        let from_number: Snowflake = serde_json::from_str("922337203685477580").unwrap();
        assert_eq!(from_text, from_number);
        assert_eq!(
            serde_json::to_string(&from_text).unwrap(),
            "\"922337203685477580\""
        );
    }
}
