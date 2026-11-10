//! Deciding which messages a sweep may delete.
//!
//! Discord's search narrows things down, but its results are never trusted on their own. Every
//! candidate passes through [`CompiledFilter::evaluate`] immediately before deletion, whichever
//! scanner found it. That is what guarantees Disweep only ever deletes your own messages: a search
//! can return a bot's reply to a command you ran, and it is rejected here.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Snowflake;
use crate::api::{Attachment, Message, SearchQuery, is_deletable_type};

/// A kind of content a message can contain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HasKind {
    Link,
    File,
    Image,
    Video,
    Sound,
    Embed,
    Sticker,
    Poll,
    Snapshot,
}

impl HasKind {
    #[must_use]
    pub fn search_value(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::File => "file",
            Self::Image => "image",
            Self::Video => "video",
            Self::Sound => "sound",
            Self::Embed => "embed",
            Self::Sticker => "sticker",
            Self::Poll => "poll",
            Self::Snapshot => "snapshot",
        }
    }
}

/// What the user asked to sweep.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Filters {
    /// Only messages sent at or after this moment, in Unix milliseconds.
    pub after: Option<i64>,
    /// Only messages sent before this moment, in Unix milliseconds.
    pub before: Option<i64>,
    /// Only this message and newer ones.
    pub after_message: Option<Snowflake>,
    /// Only this message and older ones.
    pub before_message: Option<Snowflake>,
    /// Only messages containing this text, ignoring case.
    pub contains: Option<String>,
    /// Messages containing any of these are kept, ignoring case.
    pub keep_if_contains: Vec<String>,
    /// Only messages matching this regular expression, ignoring case.
    pub pattern: Option<String>,
    /// Only messages with at least one of these.
    pub has: Vec<HasKind>,
    pub include_pinned: bool,
    pub include_nsfw: bool,
}

/// The filters don't make sense.
#[derive(Clone, Debug, thiserror::Error)]
pub enum FilterError {
    #[error("That regular expression isn't valid: {0}")]
    BadPattern(String),
    #[error("The start of the date range is after its end.")]
    EmptyRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Delete,
    Skip(SkipReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    NotYours,
    Pinned,
    SystemMessage,
    NoMatch,
}

/// Checks a pattern without building a job, for live feedback while typing.
pub fn validate_pattern(pattern: &str) -> Result<(), FilterError> {
    compile_pattern(pattern).map(|_| ())
}

fn compile_pattern(pattern: &str) -> Result<Regex, FilterError> {
    // The regex crate runs in linear time. These limits cap memory for a pathological pattern.
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 21)
        .build()
        .map_err(|error| FilterError::BadPattern(first_line(&error.to_string())))
}

fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim();
    line.strip_prefix("error: ").unwrap_or(line).to_owned()
}

/// Filters prepared for one account.
#[derive(Clone, Debug)]
pub struct CompiledFilter {
    author: Snowflake,
    lowest: Option<u64>,
    highest: Option<u64>,
    contains: Option<String>,
    keep: Vec<String>,
    pattern: Option<Regex>,
    has: Vec<HasKind>,
    include_pinned: bool,
    search_contains: Option<String>,
    include_nsfw: bool,
}

impl CompiledFilter {
    pub fn new(filters: &Filters, author: Snowflake) -> Result<Self, FilterError> {
        let mut lowest = filters.after.map(|ms| Snowflake::from_timestamp_ms(ms).0);
        let mut highest = filters
            .before
            .map(|ms| Snowflake::from_timestamp_ms(ms).0.saturating_sub(1));
        if let Some(message) = filters.after_message {
            lowest = Some(lowest.map_or(message.0, |low| low.max(message.0)));
        }
        if let Some(message) = filters.before_message {
            highest = Some(highest.map_or(message.0, |high| high.min(message.0)));
        }
        if let (Some(low), Some(high)) = (lowest, highest)
            && low > high
        {
            return Err(FilterError::EmptyRange);
        }

        let text = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let search_contains = text(&filters.contains);
        let pattern = text(&filters.pattern)
            .map(|pattern| compile_pattern(&pattern))
            .transpose()?;
        Ok(Self {
            author,
            lowest,
            highest,
            contains: search_contains.as_deref().map(str::to_lowercase),
            keep: filters
                .keep_if_contains
                .iter()
                .map(|word| word.trim().to_lowercase())
                .filter(|word| !word.is_empty())
                .collect(),
            pattern,
            has: filters.has.clone(),
            include_pinned: filters.include_pinned,
            search_contains,
            include_nsfw: filters.include_nsfw,
        })
    }

    /// The final word on whether this message may be deleted.
    #[must_use]
    pub fn evaluate(&self, message: &Message) -> Verdict {
        if message.author.id != self.author {
            return Verdict::Skip(SkipReason::NotYours);
        }
        if !is_deletable_type(message.kind) {
            return Verdict::Skip(SkipReason::SystemMessage);
        }
        if message.pinned && !self.include_pinned {
            return Verdict::Skip(SkipReason::Pinned);
        }
        if self.matches(message) {
            Verdict::Delete
        } else {
            Verdict::Skip(SkipReason::NoMatch)
        }
    }

    fn matches(&self, message: &Message) -> bool {
        if self.lowest.is_some_and(|low| message.id.0 < low)
            || self.highest.is_some_and(|high| message.id.0 > high)
        {
            return false;
        }
        let needs_lowercase = self.contains.is_some() || !self.keep.is_empty();
        let lowered = if needs_lowercase {
            message.content.to_lowercase()
        } else {
            String::new()
        };
        if self
            .contains
            .as_deref()
            .is_some_and(|needle| !lowered.contains(needle))
        {
            return false;
        }
        if self.keep.iter().any(|word| lowered.contains(word)) {
            return false;
        }
        if self
            .pattern
            .as_ref()
            .is_some_and(|pattern| !pattern.is_match(&message.content))
        {
            return false;
        }
        self.has.is_empty() || self.has.iter().any(|kind| message_has(message, *kind))
    }

    /// True when some rule can only be applied here, so a search's count overstates the matches.
    #[must_use]
    pub fn needs_local_evaluation(&self) -> bool {
        self.pattern.is_some() || !self.keep.is_empty() || self.has.len() > 1
    }

    /// True when search can find matches far more cheaply than walking a whole conversation.
    #[must_use]
    pub fn is_selective_for_search(&self) -> bool {
        self.contains.is_some() || self.has.len() == 1
    }

    #[must_use]
    pub fn includes_pinned(&self) -> bool {
        self.include_pinned
    }

    /// The newest ID that can match, as a `before` cursor for channel history.
    #[must_use]
    pub fn history_start(&self) -> Option<Snowflake> {
        self.highest.map(|high| Snowflake(high.saturating_add(1)))
    }

    /// The oldest ID that can match, where walking history can stop.
    #[must_use]
    pub fn history_floor(&self) -> Option<Snowflake> {
        self.lowest.map(Snowflake)
    }

    /// The query that narrows candidates for this author.
    ///
    /// Discord's `min_id` and `max_id` exclude the ID given. Several `has` values are left out
    /// because their combined meaning isn't documented; they are applied in [`Self::evaluate`].
    #[must_use]
    pub fn search_query(&self, guild_scoped: bool) -> SearchQuery {
        SearchQuery {
            author_id: Some(self.author),
            channel_ids: Vec::new(),
            min_id: self
                .lowest
                .filter(|low| *low > 0)
                .map(|low| Snowflake(low - 1)),
            max_id: self.highest.map(|high| Snowflake(high.saturating_add(1))),
            content: self.search_contains.clone(),
            has: if let [only] = self.has.as_slice() {
                vec![only.search_value().to_owned()]
            } else {
                Vec::new()
            },
            // Only the guild endpoint documents this parameter. DMs filter locally.
            pinned: (guild_scoped && !self.include_pinned).then_some(false),
            include_nsfw: self.include_nsfw,
        }
    }
}

fn message_has(message: &Message, kind: HasKind) -> bool {
    let embed_type = |wanted: &[&str]| {
        message.embeds.iter().any(|embed| {
            embed
                .get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| wanted.contains(&t))
        })
    };
    match kind {
        HasKind::Link => {
            let lowered = message.content.to_ascii_lowercase();
            lowered.contains("http://") || lowered.contains("https://")
        }
        HasKind::File => !message.attachments.is_empty(),
        HasKind::Image => {
            message.attachments.iter().any(|a| {
                attachment_is(
                    a,
                    "image",
                    &["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp"],
                )
            }) || embed_type(&["image", "gifv"])
        }
        HasKind::Video => {
            message
                .attachments
                .iter()
                .any(|a| attachment_is(a, "video", &["mp4", "mov", "webm", "mkv", "avi"]))
                || embed_type(&["video", "gifv"])
        }
        HasKind::Sound => message
            .attachments
            .iter()
            .any(|a| attachment_is(a, "audio", &["mp3", "ogg", "wav", "flac", "m4a"])),
        HasKind::Embed => !message.embeds.is_empty(),
        HasKind::Sticker => !message.sticker_items.is_empty(),
        HasKind::Poll => message.poll.is_some(),
        HasKind::Snapshot => !message.message_snapshots.is_empty(),
    }
}

fn attachment_is(attachment: &Attachment, mime_family: &str, extensions: &[&str]) -> bool {
    if let Some(content_type) = &attachment.content_type {
        return content_type.starts_with(&format!("{mime_family}/"));
    }
    attachment
        .filename
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extensions.contains(&extension.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ME: Snowflake = Snowflake(1);

    fn message(id: u64, content: &str) -> Message {
        serde_json::from_value(json!({
            "id": id.to_string(), "channel_id": "9", "content": content, "type": 0,
            "author": {"id": "1", "username": "me"}
        }))
        .unwrap()
    }

    fn with(mut base: Message, patch: serde_json::Value) -> Message {
        let mut value = serde_json::to_value(&base).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        base = serde_json::from_value(value).unwrap();
        base
    }

    fn compile(filters: Filters) -> CompiledFilter {
        CompiledFilter::new(&filters, ME).unwrap()
    }

    fn deletes(filter: &CompiledFilter, message: &Message) -> bool {
        filter.evaluate(message) == Verdict::Delete
    }

    #[test]
    fn never_deletes_someone_elses_message() {
        let bot_reply = with(
            message(5, "here you go"),
            json!({"author": {"id": "77", "username": "MEE6", "bot": true}}),
        );
        assert_eq!(
            compile(Filters::default()).evaluate(&bot_reply),
            Verdict::Skip(SkipReason::NotYours)
        );
    }

    #[test]
    fn system_messages_are_skipped() {
        let call = with(message(5, ""), json!({"type": 3}));
        assert_eq!(
            compile(Filters::default()).evaluate(&call),
            Verdict::Skip(SkipReason::SystemMessage)
        );
    }

    #[test]
    fn pinned_messages_are_kept_unless_asked() {
        let pinned = with(message(5, "important"), json!({"pinned": true}));
        assert_eq!(
            compile(Filters::default()).evaluate(&pinned),
            Verdict::Skip(SkipReason::Pinned)
        );
        assert!(deletes(
            &compile(Filters {
                include_pinned: true,
                ..Default::default()
            }),
            &pinned
        ));
    }

    #[test]
    fn dates_bound_the_range() {
        let moment = 1_700_000_000_000;
        let at = |ms: i64| message(Snowflake::from_timestamp_ms(ms).0, "x");
        let filter = compile(Filters {
            after: Some(moment),
            before: Some(moment + 1000),
            ..Default::default()
        });
        assert!(!deletes(&filter, &at(moment - 1)));
        assert!(deletes(&filter, &at(moment)));
        assert!(deletes(&filter, &at(moment + 999)));
        assert!(!deletes(&filter, &at(moment + 1000)));
    }

    #[test]
    fn message_bounds_include_the_picked_message() {
        let filter = compile(Filters {
            after_message: Some(Snowflake(10)),
            before_message: Some(Snowflake(20)),
            ..Default::default()
        });
        assert!(!deletes(&filter, &message(9, "x")));
        assert!(deletes(&filter, &message(10, "x")));
        assert!(deletes(&filter, &message(20, "x")));
        assert!(!deletes(&filter, &message(21, "x")));
    }

    #[test]
    fn an_inverted_range_is_refused() {
        let filters = Filters {
            after: Some(2_000_000_000_000),
            before: Some(1_500_000_000_000),
            ..Default::default()
        };
        assert!(matches!(
            CompiledFilter::new(&filters, ME),
            Err(FilterError::EmptyRange)
        ));
    }

    #[test]
    fn contains_and_keep_ignore_case() {
        let filter = compile(Filters {
            contains: Some("Secret".to_owned()),
            keep_if_contains: vec!["KEEP".to_owned()],
            ..Default::default()
        });
        assert!(deletes(&filter, &message(1, "my SECRET plan")));
        assert!(!deletes(&filter, &message(1, "nothing here")));
        assert!(!deletes(&filter, &message(1, "secret, but keep it")));
    }

    #[test]
    fn blank_text_filters_do_nothing() {
        let filter = compile(Filters {
            contains: Some("   ".to_owned()),
            pattern: Some(String::new()),
            ..Default::default()
        });
        assert!(deletes(&filter, &message(1, "anything")));
        assert!(!filter.is_selective_for_search());
    }

    #[test]
    fn patterns_ignore_case_and_bad_ones_are_reported() {
        let filter = compile(Filters {
            pattern: Some(r"^order #\d+$".to_owned()),
            ..Default::default()
        });
        assert!(deletes(&filter, &message(1, "ORDER #42")));
        assert!(!deletes(&filter, &message(1, "order #x")));
        let error = validate_pattern("(unclosed").unwrap_err().to_string();
        assert!(error.ends_with("isn't valid: unclosed group"), "{error}");
    }

    #[test]
    fn a_pathological_pattern_is_rejected_rather_than_run() {
        assert!(validate_pattern(&"(a{1000}){1000}".repeat(10)).is_err());
    }

    #[test]
    fn has_means_any_of_the_selected_kinds() {
        let filter = compile(Filters {
            has: vec![HasKind::Link, HasKind::Sticker],
            ..Default::default()
        });
        assert!(deletes(&filter, &message(1, "see https://example.com")));
        assert!(deletes(
            &filter,
            &with(message(1, ""), json!({"sticker_items": [{"id": "1"}]}))
        ));
        assert!(!deletes(&filter, &message(1, "plain")));
    }

    #[test]
    fn attachment_kinds_use_content_type_then_extension() {
        let filter = compile(Filters {
            has: vec![HasKind::Image],
            ..Default::default()
        });
        let attachment = |fields: serde_json::Value| {
            let mut value = json!({"id": "1", "filename": "a.bin", "url": "u"});
            value
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            with(message(1, ""), json!({"attachments": [value]}))
        };
        assert!(deletes(
            &filter,
            &attachment(json!({"content_type": "image/png"}))
        ));
        assert!(!deletes(
            &filter,
            &attachment(json!({"content_type": "video/mp4"}))
        ));
        assert!(deletes(
            &filter,
            &attachment(json!({"filename": "photo.JPG"}))
        ));
    }

    #[test]
    fn search_ids_are_widened_because_discord_excludes_them() {
        let filter = compile(Filters {
            after_message: Some(Snowflake(10)),
            before_message: Some(Snowflake(20)),
            ..Default::default()
        });
        let query = filter.search_query(true);
        assert_eq!(query.min_id, Some(Snowflake(9)));
        assert_eq!(query.max_id, Some(Snowflake(21)));
    }

    #[test]
    fn several_has_kinds_are_applied_locally() {
        let one = compile(Filters {
            has: vec![HasKind::File],
            ..Default::default()
        });
        assert_eq!(one.search_query(true).has, vec!["file"]);
        assert!(!one.needs_local_evaluation());

        let two = compile(Filters {
            has: vec![HasKind::File, HasKind::Link],
            ..Default::default()
        });
        assert!(two.search_query(true).has.is_empty());
        assert!(two.needs_local_evaluation());
    }

    #[test]
    fn only_guild_search_gets_the_documented_pinned_parameter() {
        let filter = compile(Filters::default());
        assert_eq!(filter.search_query(true).pinned, Some(false));
        assert_eq!(filter.search_query(false).pinned, None);
    }
}
