use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::spec::{JobOrigin, Target};
use crate::Snowflake;
use crate::filter::Filters;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Account {
    pub id: Snowflake,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Completed,
    Stopped,
    Failed,

    Interrupted,
}

impl JobState {
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Paused)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Counters {
    pub deleted: u64,

    pub already_gone: u64,

    pub skipped: u64,
    pub failed: u64,

    pub examined: u64,
    pub throttle_count: u64,
    pub throttled_ms: u64,
}

impl Counters {
    #[must_use]
    pub fn processed(&self) -> u64 {
        self.deleted + self.already_gone + self.skipped + self.failed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TargetState {
    Pending,
    Active,
    Done,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TargetProgress {
    pub label: String,
    pub state: TargetState,
    pub deleted: u64,
    pub processed: u64,

    pub estimate: Option<u64>,
    pub note: Option<String>,

    pub phase: u32,

    pub cursor: Option<Snowflake>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FailureKind {
    Login,
    Network,
    RateLimit,
    Archive,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobFailure {
    pub kind: FailureKind,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobSnapshot {
    pub id: String,
    pub account_id: Snowflake,
    pub label: String,
    pub origin: JobOrigin,
    pub state: JobState,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub counters: Counters,
    pub targets: Vec<TargetProgress>,
    pub current_target: Option<u32>,
    pub total_estimate: Option<u64>,
    pub rate_per_minute: f64,
    pub eta_ms: Option<u64>,
    pub pacing_ms: u32,
    pub rate_limited_until: Option<i64>,
    pub failure: Option<JobFailure>,
    pub archive_path: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FeedOutcome {
    Deleted,
    AlreadyGone,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FeedItem {
    pub message_id: Snowflake,
    pub conversation: String,
    pub sent_at: i64,
    pub snippet: String,
    pub attachments: u32,
    pub outcome: FeedOutcome,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum EngineEvent {
    Job {
        job: Box<JobSnapshot>,
    },
    Feed {
        job_id: String,
        items: Vec<FeedItem>,
    },
    PreviewProgress {
        preview_id: String,
        done: u32,
        total: u32,
        label: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FailureRecord {
    pub channel_id: Snowflake,
    pub message_id: Snowflake,
    pub conversation: String,
    pub code: Option<u32>,
    pub reason: String,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DayCount {
    pub day: String,
    pub deleted: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Stats {
    pub total_deleted: u64,
    pub jobs_finished: u64,
    pub days: Vec<DayCount>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SampleMessage {
    pub id: Snowflake,
    pub conversation: String,
    pub sent_at: i64,
    pub content: String,
    pub attachments: u32,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TargetPreview {
    pub label: String,
    pub count: u64,
    pub exact: bool,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Preview {
    pub targets: Vec<TargetPreview>,
    pub total: u64,
    pub exact: bool,
    pub estimated_ms: u64,
    pub sample: Vec<SampleMessage>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GuildEntry {
    pub id: Snowflake,
    pub name: String,
    pub icon_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConversationEntry {
    pub id: Snowflake,
    pub is_group: bool,
    pub name: String,
    pub icon_url: Option<String>,
    pub last_message_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChannelEntry {
    pub id: Snowflake,
    pub name: String,
    pub kind: u8,
    pub category: Option<String>,
    pub nsfw: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Inventory {
    pub guilds: Vec<GuildEntry>,
    pub conversations: Vec<ConversationEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rule {
    pub id: String,
    pub account_id: Snowflake,
    pub label: String,
    pub targets: Vec<Target>,

    pub older_than_secs: u64,
    pub every_secs: u64,
    pub filters: Filters,
    pub enabled: bool,
    pub created_at: i64,
    pub next_run_at: i64,
    pub last_run_at: Option<i64>,
    pub last_job_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RuleDraft {
    pub account_id: Snowflake,
    pub label: String,
    pub targets: Vec<Target>,
    pub older_than_secs: u64,
    pub every_secs: u64,
    #[serde(default)]
    pub filters: Filters,
}
