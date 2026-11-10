use std::fmt;
use std::sync::Arc;

use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Secret(Arc<Zeroizing<String>>);

impl Secret {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(Arc::new(Zeroizing::new(value)))
    }

    pub fn from_pasted(raw: &str) -> Result<Self, InvalidToken> {
        let trimmed = raw
            .trim()
            .trim_matches(|c| matches!(c, '"' | '\'' | '`'))
            .trim();
        let token = trimmed.strip_prefix("Bot ").unwrap_or(trimmed).trim();
        let valid_character = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
        if token.len() < 20 || token.len() > 512 || !token.chars().all(valid_character) {
            return Err(InvalidToken);
        }
        Ok(Self::new(token.to_owned()))
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error)]
#[error("that doesn't look like a Discord token")]
pub struct InvalidToken;

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "MTA1NjQ3.GZk2Lw.abcdefghijklmnopqrstuvwxyz012345";

    #[test]
    fn strips_quotes_spaces_and_a_bot_prefix() {
        for pasted in [
            SAMPLE.to_owned(),
            format!("  {SAMPLE}\n"),
            format!("\"{SAMPLE}\""),
            format!("Bot {SAMPLE}"),
        ] {
            assert_eq!(Secret::from_pasted(&pasted).unwrap().expose(), SAMPLE);
        }
    }

    #[test]
    fn rejects_text_that_cannot_be_a_token() {
        for pasted in [
            "",
            "short",
            "has spaces in the middle of the text",
            "new\nline.abcdefghijklmnopqrst",
        ] {
            assert!(
                Secret::from_pasted(pasted).is_err(),
                "{pasted:?} should be rejected"
            );
        }
    }

    #[test]
    fn debug_output_hides_the_value() {
        let printed = format!("{:?}", Secret::new(SAMPLE.to_owned()));
        assert!(!printed.contains(SAMPLE));
    }
}
