//! The parts of an interaction payload the bot reads.
//!
//! Everything optional stays optional, so a field Discord adds or drops can't stop the bot from
//! understanding a command. None of these types implement `Debug`, because they hold the
//! interaction's token.

use std::collections::HashMap;

use disweep_core::Snowflake;
use serde::Deserialize;
use serde_json::Value;

pub const KIND_COMMAND: u8 = 2;
pub const KIND_COMPONENT: u8 = 3;

pub const CONTEXT_GUILD: u8 = 0;

const MANAGE_MESSAGES: u64 = 1 << 13;
const READ_MESSAGE_HISTORY: u64 = 1 << 16;

#[derive(Clone, Deserialize)]
pub struct Interaction {
    pub id: Snowflake,
    pub application_id: Snowflake,
    #[serde(rename = "type")]
    pub kind: u8,
    pub token: String,
    #[serde(default)]
    pub data: Option<Data>,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    #[serde(default)]
    pub channel: Option<PartialChannel>,
    #[serde(default)]
    pub channel_id: Option<Snowflake>,
    #[serde(default)]
    pub member: Option<Member>,
    #[serde(default)]
    pub user: Option<PartialUser>,
    #[serde(default)]
    pub app_permissions: Option<String>,
    #[serde(default)]
    pub context: Option<u8>,
}

#[derive(Clone, Deserialize)]
pub struct PartialChannel {
    pub id: Snowflake,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub recipients: Vec<PartialUser>,
}

#[derive(Clone, Deserialize)]
pub struct PartialUser {
    pub id: Snowflake,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
}

impl PartialUser {
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.global_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.username)
    }
}

#[derive(Clone, Deserialize)]
pub struct Member {
    #[serde(default)]
    pub user: Option<PartialUser>,
    #[serde(default)]
    pub permissions: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub options: Vec<CommandOption>,
    #[serde(default)]
    pub resolved: Option<Resolved>,
    #[serde(default)]
    pub target_id: Option<Snowflake>,
    #[serde(default)]
    pub custom_id: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct CommandOption {
    pub name: String,
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default)]
    pub options: Vec<CommandOption>,
    #[serde(rename = "type", default)]
    pub kind: u8,
}

#[derive(Clone, Default, Deserialize)]
pub struct Resolved {
    #[serde(default)]
    pub users: HashMap<String, PartialUser>,
}

const OPTION_SUBCOMMAND: u8 = 1;
const OPTION_GROUP: u8 = 2;

impl Interaction {
    /// Who ran this, which Discord attests to and can't be forged by the caller.
    #[must_use]
    pub fn invoker(&self) -> Option<&PartialUser> {
        self.member
            .as_ref()
            .and_then(|member| member.user.as_ref())
            .or(self.user.as_ref())
    }

    #[must_use]
    pub fn channel_id(&self) -> Option<Snowflake> {
        self.channel
            .as_ref()
            .map(|channel| channel.id)
            .or(self.channel_id)
    }

    #[must_use]
    pub fn is_in_guild(&self) -> bool {
        self.guild_id.is_some() || self.context == Some(CONTEXT_GUILD)
    }

    /// The invoked subcommand's name and its options, or the command's own options if it has none.
    #[must_use]
    pub fn subcommand(&self) -> Option<(&str, &[CommandOption])> {
        let data = self.data.as_ref()?;
        match data.options.first() {
            Some(first) if first.kind == OPTION_GROUP => {
                let inner = first.options.first()?;
                Some((inner.name.as_str(), inner.options.as_slice()))
            }
            Some(first) if first.kind == OPTION_SUBCOMMAND => {
                Some((first.name.as_str(), first.options.as_slice()))
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn top_level_options(&self) -> &[CommandOption] {
        self.data
            .as_ref()
            .map_or(&[], |data| data.options.as_slice())
    }

    fn permission(text: Option<&String>, bit: u64) -> bool {
        text.and_then(|bits| bits.parse::<u64>().ok())
            .is_some_and(|bits| bits & bit == bit)
    }

    /// Whether the person running this may moderate messages here.
    #[must_use]
    pub fn invoker_can_manage_messages(&self) -> bool {
        Self::permission(
            self.member
                .as_ref()
                .and_then(|member| member.permissions.as_ref()),
            MANAGE_MESSAGES,
        )
    }

    /// Whether the bot itself can read and remove messages here.
    #[must_use]
    pub fn bot_can_purge(&self) -> bool {
        Self::permission(
            self.app_permissions.as_ref(),
            MANAGE_MESSAGES | READ_MESSAGE_HISTORY,
        )
    }
}

/// Reading named values out of a command's options.
pub trait OptionsExt {
    fn string(&self, name: &str) -> Option<&str>;
    fn integer(&self, name: &str) -> Option<i64>;
    fn boolean(&self, name: &str) -> Option<bool>;
    fn snowflake(&self, name: &str) -> Option<Snowflake>;
}

fn find<'a>(options: &'a [CommandOption], name: &str) -> Option<&'a Value> {
    options
        .iter()
        .find(|option| option.name == name)
        .and_then(|option| option.value.as_ref())
}

impl OptionsExt for [CommandOption] {
    fn string(&self, name: &str) -> Option<&str> {
        find(self, name)?.as_str()
    }

    fn integer(&self, name: &str) -> Option<i64> {
        find(self, name)?.as_i64()
    }

    fn boolean(&self, name: &str) -> Option<bool> {
        find(self, name)?.as_bool()
    }

    fn snowflake(&self, name: &str) -> Option<Snowflake> {
        find(self, name)?.as_str()?.parse().ok()
    }
}
