//! What a sweep is asked to do.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Snowflake;
use crate::filter::Filters;

/// Where to look for messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Scope {
    /// A DM or group DM.
    Private { channel_id: Snowflake },
    /// One channel of a server.
    Channel {
        guild_id: Snowflake,
        channel_id: Snowflake,
    },
    /// A whole server, including threads.
    Guild { guild_id: Snowflake },
    /// A conversation listed in an imported data package.
    Package { channel_id: Snowflake },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Target {
    pub scope: Scope,
    pub label: String,
    /// The count from the preview, used for progress before a target is reached.
    #[serde(default)]
    pub expected: Option<u64>,
}

/// How candidates are found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Strategy {
    /// Full history for DMs unless a filter makes search cheaper, search for servers.
    #[default]
    Auto,
    Search,
    History,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Speed {
    Careful,
    #[default]
    Balanced,
    Fast,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ArchiveOptions {
    pub enabled: bool,
    pub attachments: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct JobOptions {
    pub strategy: Strategy,
    /// After searching, also walk full history to catch anything search didn't return.
    pub thorough: bool,
    pub speed: Speed,
    /// Stop after deleting this many messages.
    pub limit: Option<u64>,
    pub archive: ArchiveOptions,
}

/// What started a job.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum JobOrigin {
    #[default]
    App,
    Bot,
    Rule {
        rule_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobSpec {
    pub account_id: Snowflake,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub filters: Filters,
    #[serde(default)]
    pub options: JobOptions,
    #[serde(default)]
    pub origin: JobOrigin,
}

impl JobSpec {
    /// A short description for lists and notifications.
    #[must_use]
    pub fn label(&self) -> String {
        match self.targets.as_slice() {
            [only] => only.label.clone(),
            [first, rest @ ..] => format!("{} and {} more", first.label, rest.len()),
            [] => "Nothing".to_owned(),
        }
    }
}
