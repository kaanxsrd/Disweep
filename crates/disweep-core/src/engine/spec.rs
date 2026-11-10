use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Snowflake;
use crate::filter::Filters;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Scope {
    Private {
        channel_id: Snowflake,
    },

    Channel {
        guild_id: Snowflake,
        channel_id: Snowflake,
    },

    Guild {
        guild_id: Snowflake,
    },

    Package {
        channel_id: Snowflake,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Target {
    pub scope: Scope,
    pub label: String,

    #[serde(default)]
    pub expected: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Strategy {
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

    pub thorough: bool,
    pub speed: Speed,

    pub limit: Option<u64>,
    pub archive: ArchiveOptions,
}

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
    #[must_use]
    pub fn label(&self) -> String {
        match self.targets.as_slice() {
            [only] => only.label.clone(),
            [first, rest @ ..] => format!("{} and {} more", first.label, rest.len()),
            [] => "Nothing".to_owned(),
        }
    }
}
