use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::Snowflake;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Snowflake,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl User {
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.global_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.username)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guild {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub id: Snowflake,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub position: Option<i32>,
    #[serde(default)]
    pub recipients: Vec<User>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub last_message_id: Option<Snowflake>,
    #[serde(default)]
    pub nsfw: bool,
}

impl Channel {
    pub const GUILD_TEXT: u8 = 0;
    pub const DM: u8 = 1;
    pub const GUILD_VOICE: u8 = 2;
    pub const GROUP_DM: u8 = 3;
    pub const CATEGORY: u8 = 4;
    pub const ANNOUNCEMENT: u8 = 5;
    pub const STAGE: u8 = 13;
    pub const FORUM: u8 = 15;
    pub const MEDIA: u8 = 16;

    #[must_use]
    pub fn is_private(&self) -> bool {
        matches!(self.kind, Self::DM | Self::GROUP_DM)
    }

    #[must_use]
    pub fn holds_messages(&self) -> bool {
        matches!(
            self.kind,
            Self::GUILD_TEXT
                | Self::GUILD_VOICE
                | Self::ANNOUNCEMENT
                | Self::STAGE
                | Self::FORUM
                | Self::MEDIA
        )
    }

    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(name) = self.name.as_deref().filter(|name| !name.is_empty()) {
            return name.to_owned();
        }
        let people: Vec<&str> = self.recipients.iter().map(User::display_name).collect();
        if people.is_empty() {
            "Unnamed conversation".to_owned()
        } else {
            people.join(", ")
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub id: Snowflake,
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    pub author: User,
    #[serde(rename = "type", default)]
    pub kind: u16,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub embeds: Vec<Value>,
    #[serde(default)]
    pub sticker_items: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub message_snapshots: Vec<Value>,
    #[serde(default, skip_serializing)]
    pub hit: bool,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

#[must_use]
pub fn is_deletable_type(kind: u16) -> bool {
    !matches!(kind, 1..=5 | 14..=17 | 21)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Application {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub bot: Option<User>,
    #[serde(default)]
    pub interactions_endpoint_url: Option<String>,
    #[serde(default)]
    pub integration_types_config: Option<Map<String, Value>>,
    #[serde(default)]
    pub approximate_guild_count: Option<u64>,
    #[serde(default)]
    pub approximate_user_install_count: Option<u64>,
    #[serde(default)]
    pub flags: Option<u64>,
}

const CDN: &str = "https://cdn.discordapp.com";

#[must_use]
pub fn avatar_url(user_id: Snowflake, avatar: Option<&str>) -> String {
    match avatar {
        Some(hash) => format!("{CDN}/avatars/{user_id}/{hash}.png?size=96"),
        None => format!("{CDN}/embed/avatars/{}.png", (user_id.0 >> 22) % 6),
    }
}

#[must_use]
pub fn guild_icon_url(guild_id: Snowflake, icon: Option<&str>) -> Option<String> {
    icon.map(|hash| format!("{CDN}/icons/{guild_id}/{hash}.png?size=96"))
}

#[must_use]
pub fn channel_icon_url(channel_id: Snowflake, icon: Option<&str>) -> Option<String> {
    icon.map(|hash| format!("{CDN}/channel-icons/{channel_id}/{hash}.png?size=96"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_keeps_fields_disweep_does_not_model() {
        let original = serde_json::json!({
            "id": "5", "channel_id": "6", "type": 19, "content": "hi", "timestamp": "2026-01-01T00:00:00+00:00",
            "author": {"id": "7", "username": "kit"},
            "reactions": [{"count": 3}]
        });
        let message: Message = serde_json::from_value(original).unwrap();
        assert_eq!(message.kind, 19);
        let saved = serde_json::to_value(&message).unwrap();
        assert_eq!(saved["reactions"][0]["count"], 3);
    }

    #[test]
    fn follows_the_deletable_column() {
        for kind in [0, 6, 7, 18, 19, 20, 23, 46, 999] {
            assert!(is_deletable_type(kind), "type {kind} is deletable");
        }
        for kind in [1, 2, 3, 4, 5, 14, 15, 16, 17, 21] {
            assert!(!is_deletable_type(kind), "type {kind} is not deletable");
        }
    }

    #[test]
    fn dms_without_a_name_use_their_participants() {
        let channel: Channel = serde_json::from_value(serde_json::json!({
            "id": "1", "type": 3,
            "recipients": [{"id": "2", "username": "a", "global_name": "Ada"}, {"id": "3", "username": "bo"}]
        }))
        .unwrap();
        assert_eq!(channel.display_name(), "Ada, bo");
    }
}
