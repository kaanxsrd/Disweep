mod control;
mod pacing;
mod preview;
mod runner;
mod spec;
mod status;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub use control::{Control, Stopped};
pub use pacing::Pacer;
pub use spec::{ArchiveOptions, JobOptions, JobOrigin, JobSpec, Scope, Speed, Strategy, Target};
pub use status::{
    Account, ChannelEntry, ConversationEntry, Counters, DayCount, EngineEvent, FailureKind,
    FailureRecord, FeedItem, FeedOutcome, GuildEntry, Inventory, JobFailure, JobSnapshot, JobState,
    Preview, Rule, RuleDraft, SampleMessage, Stats, TargetPreview, TargetProgress, TargetState,
};

use crate::api::{
    AuthScheme, Channel, Discord, Transport, avatar_url, channel_icon_url, guild_icon_url,
};
use crate::archive::{Archive, safe_directory_name};
use crate::error::ApiError;
use crate::filter::{CompiledFilter, FilterError};
use crate::package::{self, PackageIndex};
use crate::store::{JobRecord, Store, StoreError};
use crate::{Secret, Snowflake, now_ms};
use preview::PreviewContext;
use runner::{JobRun, ThrottleInbox, Tracker, failure_from, observe_throttles};

const EVENT_BUFFER: usize = 1_024;
const MAX_TARGETS: usize = 2_000;
const RULE_CHECK_INTERVAL: Duration = Duration::from_secs(30);
const MIN_RULE_INTERVAL_SECS: u64 = 15 * 60;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error("That account isn't signed in.")]
    UnknownAccount,
    #[error("That sweep no longer exists.")]
    UnknownJob,
    #[error("That rule no longer exists.")]
    UnknownRule,
    #[error("Choose at least one conversation.")]
    NoTargets,
    #[error("That's too many conversations for one sweep. Choose {MAX_TARGETS} or fewer.")]
    TooManyTargets,
    #[error("This sweep has finished, so it can't be changed.")]
    NotActive,
    #[error("Only a sweep that was stopped or interrupted can be resumed.")]
    NotResumable,
    #[error("That token belongs to a bot. Sign in with a personal account here.")]
    BotAccount,
    #[error("No data package is imported for this account.")]
    NoPackage,
    #[error("This data package isn't confirmed to be this account's, so it can't be used here.")]
    PackageMismatch,
    #[error("{0}")]
    Package(String),
    #[error("A rule must run at most every 15 minutes and keep at least one minute of history.")]
    BadRuleTiming,
    #[error("The preview was cancelled.")]
    Cancelled,
    #[error("Something went wrong inside Disweep: {0}")]
    Internal(String),
}

#[derive(Clone)]
struct AccountRuntime {
    account: Account,
    client: Discord,
    queue: mpsc::UnboundedSender<String>,
}

#[derive(Clone)]
struct JobEntry {
    spec: Arc<JobSpec>,
    snapshot: Arc<RwLock<JobSnapshot>>,
    control: Arc<Control>,
}

struct Inner {
    store: Store,
    transport: Arc<dyn Transport>,
    events: broadcast::Sender<EngineEvent>,
    accounts: RwLock<HashMap<Snowflake, AccountRuntime>>,
    jobs: RwLock<HashMap<String, JobEntry>>,
    archive_root: RwLock<PathBuf>,
    pace_scale: RwLock<f64>,
    scheduler_running: AtomicBool,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    #[must_use]
    pub fn new(store: Store, transport: Arc<dyn Transport>, archive_root: PathBuf) -> Self {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            inner: Arc::new(Inner {
                store,
                transport,
                events,
                accounts: RwLock::new(HashMap::new()),
                jobs: RwLock::new(HashMap::new()),
                archive_root: RwLock::new(archive_root),
                pace_scale: RwLock::new(1.0),
                scheduler_running: AtomicBool::new(false),
            }),
        }
    }

    pub fn set_archive_root(&self, root: PathBuf) {
        *self.inner.archive_root.write() = root;
    }

    #[must_use]
    pub fn archive_root(&self) -> PathBuf {
        self.inner.archive_root.read().clone()
    }

    pub fn speed_up_for_demo(&self, scale: f64) {
        if self.inner.transport.is_simulated() {
            *self.inner.pace_scale.write() = scale;
        }
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    #[must_use]
    pub fn transport(&self) -> Arc<dyn Transport> {
        self.inner.transport.clone()
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    pub async fn seed_demo_history(&self, account: Snowflake) -> Result<(), EngineError> {
        if !self.inner.transport.is_simulated() {
            return Ok(());
        }
        const SAMPLE: [(i64, u64); 12] = [
            (27, 124),
            (25, 342),
            (24, 88),
            (20, 510),
            (17, 64),
            (13, 233),
            (12, 401),
            (9, 156),
            (6, 289),
            (4, 97),
            (2, 318),
            (1, 72),
        ];
        let today = chrono::Utc::now().date_naive();
        for (days_ago, count) in SAMPLE {
            let day = (today - chrono::Duration::days(days_ago))
                .format("%Y-%m-%d")
                .to_string();
            self.inner.store.add_deleted_on(account, day, count).await?;
        }
        Ok(())
    }

    pub async fn recover(&self) -> Result<u32, EngineError> {
        let unfinished = self.inner.store.unfinished_jobs().await?;
        let count = unfinished.len() as u32;
        for mut record in unfinished {
            record.snapshot.state = JobState::Interrupted;
            record.snapshot.finished_at = Some(now_ms());
            record.snapshot.current_target = None;
            record.snapshot.rate_limited_until = None;
            for target in &mut record.snapshot.targets {
                if target.state == TargetState::Active {
                    target.state = TargetState::Pending;
                }
            }
            self.inner.store.save_job(record).await?;
        }
        Ok(count)
    }

    pub async fn sign_in(&self, token: &Secret) -> Result<Account, EngineError> {
        let client = Discord::new(self.inner.transport.clone(), token, AuthScheme::User);
        let user = client.current_user().await?;
        if user.bot {
            return Err(EngineError::BotAccount);
        }
        let account = Account {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name().to_owned(),
            avatar_url: (!self.inner.transport.is_simulated())
                .then(|| avatar_url(user.id, user.avatar.as_deref())),
        };
        self.inner.store.save_account(account.clone()).await?;
        self.attach_client(account.clone(), client);
        Ok(account)
    }

    pub fn attach(&self, account: Account, token: &Secret) {
        let client = Discord::new(self.inner.transport.clone(), token, AuthScheme::User);
        self.attach_client(account, client);
    }

    fn attach_client(&self, account: Account, client: Discord) {
        let mut accounts = self.inner.accounts.write();
        if let Some(existing) = accounts.get_mut(&account.id) {
            existing.account = account;
            existing.client = client;
            return;
        }
        let (queue, receiver) = mpsc::unbounded_channel();
        let id = account.id;
        accounts.insert(
            id,
            AccountRuntime {
                account,
                client,
                queue,
            },
        );
        tokio::spawn(run_worker(self.inner.clone(), id, receiver));
    }

    pub async fn sign_out(&self, id: Snowflake) -> Result<(), EngineError> {
        for entry in self.inner.jobs.read().values() {
            if entry.spec.account_id == id {
                entry.control.stop();
            }
        }
        self.inner.accounts.write().remove(&id);
        self.inner.store.remove_account(id).await?;
        Ok(())
    }

    #[must_use]
    pub fn accounts(&self) -> Vec<Account> {
        let mut accounts: Vec<Account> = self
            .inner
            .accounts
            .read()
            .values()
            .map(|runtime| runtime.account.clone())
            .collect();
        accounts.sort_by_key(|account| account.id);
        accounts
    }

    #[must_use]
    pub fn client(&self, id: Snowflake) -> Option<Discord> {
        self.inner
            .accounts
            .read()
            .get(&id)
            .map(|runtime| runtime.client.clone())
    }

    fn require_client(&self, id: Snowflake) -> Result<Discord, EngineError> {
        self.client(id).ok_or(EngineError::UnknownAccount)
    }

    pub async fn inventory(&self, account: Snowflake) -> Result<Inventory, EngineError> {
        let client = self.require_client(account)?;
        let simulated = self.inner.transport.is_simulated();
        let (guilds, channels) = tokio::try_join!(client.guilds(), client.dm_channels())?;

        let guilds = guilds
            .into_iter()
            .map(|guild| GuildEntry {
                id: guild.id,
                icon_url: if simulated {
                    None
                } else {
                    guild_icon_url(guild.id, guild.icon.as_deref())
                },
                name: guild.name,
            })
            .collect();

        let mut conversations: Vec<ConversationEntry> = channels
            .into_iter()
            .filter(Channel::is_private)
            .map(|channel| {
                let icon_url = if simulated {
                    None
                } else if channel.kind == Channel::GROUP_DM {
                    channel_icon_url(channel.id, channel.icon.as_deref())
                } else {
                    channel
                        .recipients
                        .first()
                        .map(|person| avatar_url(person.id, person.avatar.as_deref()))
                };
                ConversationEntry {
                    id: channel.id,
                    is_group: channel.kind == Channel::GROUP_DM,
                    name: channel.display_name(),
                    icon_url,
                    last_message_at: channel.last_message_id.map(Snowflake::timestamp_ms),
                }
            })
            .collect();
        conversations.sort_by_key(|item| std::cmp::Reverse(item.last_message_at));

        Ok(Inventory {
            guilds,
            conversations,
        })
    }

    pub async fn guild_channels(
        &self,
        account: Snowflake,
        guild: Snowflake,
    ) -> Result<Vec<ChannelEntry>, EngineError> {
        let client = self.require_client(account)?;
        let channels = client.guild_channels(guild).await?;

        let categories: HashMap<Snowflake, (i32, String)> = channels
            .iter()
            .filter(|channel| channel.kind == Channel::CATEGORY)
            .map(|channel| {
                (
                    channel.id,
                    (channel.position.unwrap_or(0), channel.display_name()),
                )
            })
            .collect();

        let mut listed: Vec<&Channel> = channels
            .iter()
            .filter(|channel| channel.holds_messages())
            .collect();
        listed.sort_by_key(|channel| {
            let category_position = channel
                .parent_id
                .and_then(|id| categories.get(&id))
                .map_or(-1, |(position, _)| *position);
            (category_position, channel.position.unwrap_or(0), channel.id)
        });

        Ok(listed
            .into_iter()
            .map(|channel| ChannelEntry {
                id: channel.id,
                name: channel.display_name(),
                kind: channel.kind,
                category: channel
                    .parent_id
                    .and_then(|id| categories.get(&id))
                    .map(|(_, name)| name.clone()),
                nsfw: channel.nsfw,
            })
            .collect())
    }

    fn validate(spec: &JobSpec) -> Result<CompiledFilter, EngineError> {
        if spec.targets.is_empty() {
            return Err(EngineError::NoTargets);
        }
        if spec.targets.len() > MAX_TARGETS {
            return Err(EngineError::TooManyTargets);
        }
        Ok(CompiledFilter::new(&spec.filters, spec.account_id)?)
    }

    async fn package_for(&self, spec: &JobSpec) -> Result<Option<PackageIndex>, EngineError> {
        if !spec
            .targets
            .iter()
            .any(|target| matches!(target.scope, Scope::Package { .. }))
        {
            return Ok(None);
        }
        let index = self
            .inner
            .store
            .package(spec.account_id)
            .await?
            .ok_or(EngineError::NoPackage)?;

        if index.owner_id != Some(spec.account_id) {
            return Err(EngineError::PackageMismatch);
        }
        Ok(Some(index))
    }

    pub async fn preview(
        &self,
        preview_id: String,
        spec: JobSpec,
        cancel: CancellationToken,
    ) -> Result<Preview, EngineError> {
        let filter = Self::validate(&spec)?;
        let client = self.require_client(spec.account_id)?;
        let package = self.package_for(&spec).await?;
        let context = PreviewContext {
            client: &client,
            filter: &filter,
            package: package.as_ref(),
            account: spec.account_id,
            cancel: &cancel,
        };

        let total = spec.targets.len() as u32;
        let mut results = Vec::with_capacity(spec.targets.len());
        for (done, target) in spec.targets.iter().enumerate() {
            let _ = self.inner.events.send(EngineEvent::PreviewProgress {
                preview_id: preview_id.clone(),
                done: done as u32,
                total,
                label: target.label.clone(),
            });
            results.push(context.target(target).await?);
        }
        Ok(preview::assemble(&spec, results))
    }

    pub async fn start(&self, spec: JobSpec) -> Result<JobSnapshot, EngineError> {
        Self::validate(&spec)?;
        self.package_for(&spec).await?;
        let runtime = self.runtime(spec.account_id)?;

        let snapshot = JobSnapshot {
            id: Uuid::new_v4().to_string(),
            account_id: spec.account_id,
            label: spec.label(),
            origin: spec.origin.clone(),
            state: JobState::Queued,
            created_at: now_ms(),
            started_at: None,
            finished_at: None,
            counters: Counters::default(),
            targets: spec
                .targets
                .iter()
                .map(|target| TargetProgress {
                    label: target.label.clone(),
                    state: TargetState::Pending,
                    deleted: 0,
                    processed: 0,
                    estimate: target.expected,
                    note: None,
                    phase: 0,
                    cursor: None,
                })
                .collect(),
            current_target: None,
            total_estimate: Some(
                spec.targets
                    .iter()
                    .filter_map(|target| target.expected)
                    .sum(),
            ),
            rate_per_minute: 0.0,
            eta_ms: None,
            pacing_ms: 0,
            rate_limited_until: None,
            failure: None,
            archive_path: None,
            notes: Vec::new(),
        };
        self.enqueue(runtime, spec, snapshot.clone()).await?;
        Ok(snapshot)
    }

    fn runtime(&self, account: Snowflake) -> Result<AccountRuntime, EngineError> {
        self.inner
            .accounts
            .read()
            .get(&account)
            .cloned()
            .ok_or(EngineError::UnknownAccount)
    }

    async fn enqueue(
        &self,
        runtime: AccountRuntime,
        spec: JobSpec,
        snapshot: JobSnapshot,
    ) -> Result<(), EngineError> {
        let id = snapshot.id.clone();
        self.inner
            .store
            .save_job(JobRecord {
                spec: spec.clone(),
                snapshot: snapshot.clone(),
            })
            .await?;
        let _ = self.inner.events.send(EngineEvent::Job {
            job: Box::new(snapshot.clone()),
        });
        self.inner.jobs.write().insert(
            id.clone(),
            JobEntry {
                spec: Arc::new(spec),
                snapshot: Arc::new(RwLock::new(snapshot)),
                control: Arc::new(Control::new()),
            },
        );
        runtime
            .queue
            .send(id)
            .map_err(|error| EngineError::Internal(error.to_string()))
    }

    fn entry(&self, id: &str) -> Result<JobEntry, EngineError> {
        self.inner
            .jobs
            .read()
            .get(id)
            .cloned()
            .ok_or(EngineError::UnknownJob)
    }

    fn publish(&self, snapshot: JobSnapshot) {
        let _ = self.inner.events.send(EngineEvent::Job {
            job: Box::new(snapshot),
        });
    }

    pub fn pause(&self, id: &str) -> Result<JobSnapshot, EngineError> {
        let entry = self.entry(id)?;
        if !entry.snapshot.read().state.is_active() {
            return Err(EngineError::NotActive);
        }
        entry.control.pause();
        let snapshot = {
            let mut snapshot = entry.snapshot.write();
            if snapshot.state == JobState::Running {
                snapshot.state = JobState::Paused;
            }
            snapshot.clone()
        };
        self.publish(snapshot.clone());
        Ok(snapshot)
    }

    pub fn resume(&self, id: &str) -> Result<JobSnapshot, EngineError> {
        let entry = self.entry(id)?;
        if !entry.snapshot.read().state.is_active() {
            return Err(EngineError::NotActive);
        }
        entry.control.resume();
        let snapshot = {
            let mut snapshot = entry.snapshot.write();
            if snapshot.state == JobState::Paused {
                snapshot.state = JobState::Running;
            }
            snapshot.clone()
        };
        self.publish(snapshot.clone());
        Ok(snapshot)
    }

    pub fn stop(&self, id: &str) -> Result<(), EngineError> {
        let entry = self.entry(id)?;
        if !entry.snapshot.read().state.is_active() {
            return Err(EngineError::NotActive);
        }
        entry.control.stop();
        Ok(())
    }

    pub fn stop_all(&self) {
        for entry in self.inner.jobs.read().values() {
            if entry.snapshot.read().state.is_active() {
                entry.control.stop();
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self, id: &str) -> Option<JobSnapshot> {
        self.inner
            .jobs
            .read()
            .get(id)
            .map(|entry| entry.snapshot.read().clone())
    }

    #[must_use]
    pub fn active_jobs(&self) -> Vec<JobSnapshot> {
        let mut active: Vec<JobSnapshot> = self
            .inner
            .jobs
            .read()
            .values()
            .map(|entry| entry.snapshot.read().clone())
            .filter(|snapshot| snapshot.state.is_active())
            .collect();
        active.sort_by_key(|snapshot| snapshot.created_at);
        active
    }

    pub async fn history(&self, limit: u32) -> Result<Vec<JobSnapshot>, EngineError> {
        let mut jobs = self.inner.store.recent_jobs(limit).await?;

        for job in &mut jobs {
            if let Some(live) = self.snapshot(&job.id) {
                *job = live;
            }
        }
        Ok(jobs)
    }

    pub async fn failures(&self, job: &str) -> Result<Vec<FailureRecord>, EngineError> {
        Ok(self.inner.store.failures(job.to_owned()).await?)
    }

    pub async fn forget_job(&self, id: &str) -> Result<(), EngineError> {
        if self
            .snapshot(id)
            .is_some_and(|snapshot| snapshot.state.is_active())
        {
            return Err(EngineError::NotActive);
        }
        self.inner.jobs.write().remove(id);
        self.inner.store.delete_job(id.to_owned()).await?;
        Ok(())
    }

    pub async fn resume_finished(&self, id: &str) -> Result<JobSnapshot, EngineError> {
        let record = self
            .inner
            .store
            .job(id.to_owned())
            .await?
            .ok_or(EngineError::UnknownJob)?;
        if !matches!(
            record.snapshot.state,
            JobState::Interrupted | JobState::Stopped | JobState::Failed
        ) {
            return Err(EngineError::NotResumable);
        }
        let runtime = self.runtime(record.spec.account_id)?;
        let mut snapshot = record.snapshot;
        snapshot.state = JobState::Queued;
        snapshot.finished_at = None;
        snapshot.failure = None;
        self.enqueue(runtime, record.spec, snapshot.clone()).await?;
        Ok(snapshot)
    }

    pub async fn stats(&self, account: Snowflake) -> Result<Stats, EngineError> {
        Ok(self.inner.store.stats(account).await?)
    }

    pub async fn import_package(
        &self,
        account: Snowflake,
        path: PathBuf,
    ) -> Result<PackageIndex, EngineError> {
        let index = tokio::task::spawn_blocking(move || package::read_index(&path))
            .await
            .map_err(|error| EngineError::Internal(error.to_string()))?
            .map_err(|error| EngineError::Package(error.to_string()))?;
        if index.owner_id != Some(account) {
            return Err(EngineError::PackageMismatch);
        }
        self.inner
            .store
            .save_package(account, index.clone())
            .await?;
        Ok(index)
    }

    pub async fn package(&self, account: Snowflake) -> Result<Option<PackageIndex>, EngineError> {
        Ok(self.inner.store.package(account).await?)
    }

    pub async fn forget_package(&self, account: Snowflake) -> Result<(), EngineError> {
        Ok(self.inner.store.remove_package(account).await?)
    }

    pub async fn rules(&self) -> Result<Vec<Rule>, EngineError> {
        Ok(self.inner.store.rules().await?)
    }

    pub async fn add_rule(&self, draft: RuleDraft) -> Result<Rule, EngineError> {
        if draft.targets.is_empty() {
            return Err(EngineError::NoTargets);
        }
        if draft.every_secs < MIN_RULE_INTERVAL_SECS || draft.older_than_secs < 60 {
            return Err(EngineError::BadRuleTiming);
        }
        CompiledFilter::new(&draft.filters, draft.account_id)?;
        self.require_client(draft.account_id)?;

        let now = now_ms();
        let rule = Rule {
            id: Uuid::new_v4().to_string(),
            account_id: draft.account_id,
            label: draft.label,
            targets: draft.targets,
            older_than_secs: draft.older_than_secs,
            every_secs: draft.every_secs,
            filters: draft.filters,
            enabled: true,
            created_at: now,
            next_run_at: now,
            last_run_at: None,
            last_job_id: None,
        };
        self.inner.store.save_rule(rule.clone()).await?;
        Ok(rule)
    }

    pub async fn set_rule_enabled(&self, id: &str, enabled: bool) -> Result<Rule, EngineError> {
        let mut rule = self.rule(id).await?;
        rule.enabled = enabled;
        if enabled {
            rule.next_run_at = rule.next_run_at.max(now_ms());
        }
        self.inner.store.save_rule(rule.clone()).await?;
        Ok(rule)
    }

    pub async fn delete_rule(&self, id: &str) -> Result<(), EngineError> {
        Ok(self.inner.store.delete_rule(id.to_owned()).await?)
    }

    async fn rule(&self, id: &str) -> Result<Rule, EngineError> {
        self.inner
            .store
            .rules()
            .await?
            .into_iter()
            .find(|rule| rule.id == id)
            .ok_or(EngineError::UnknownRule)
    }

    pub async fn run_rule(&self, id: &str) -> Result<JobSnapshot, EngineError> {
        let rule = self.rule(id).await?;
        self.launch_rule(rule, now_ms()).await
    }

    async fn launch_rule(&self, mut rule: Rule, now: i64) -> Result<JobSnapshot, EngineError> {
        let mut filters = rule.filters.clone();
        let cutoff = now - (rule.older_than_secs as i64) * 1000;
        filters.before = Some(filters.before.map_or(cutoff, |before| before.min(cutoff)));
        let spec = JobSpec {
            account_id: rule.account_id,
            targets: rule.targets.clone(),
            filters,
            options: JobOptions::default(),
            origin: JobOrigin::Rule {
                rule_id: rule.id.clone(),
            },
        };
        let snapshot = self.start(spec).await?;
        rule.last_run_at = Some(now);
        rule.last_job_id = Some(snapshot.id.clone());
        rule.next_run_at = now + (rule.every_secs as i64) * 1000;
        self.inner.store.save_rule(rule).await?;
        Ok(snapshot)
    }

    pub async fn run_due_rules(&self, now: i64) -> Result<u32, EngineError> {
        let attached: HashSet<Snowflake> = self.inner.accounts.read().keys().copied().collect();
        let mut started = 0;
        for rule in self.inner.store.rules().await? {
            if !rule.enabled || rule.next_run_at > now || !attached.contains(&rule.account_id) {
                continue;
            }
            let still_running = rule
                .last_job_id
                .as_deref()
                .and_then(|id| self.snapshot(id))
                .is_some_and(|snapshot| snapshot.state.is_active());
            if still_running {
                continue;
            }
            match self.launch_rule(rule, now).await {
                Ok(_) => started += 1,
                Err(error) => tracing::warn!(%error, "couldn't start a scheduled rule"),
            }
        }
        Ok(started)
    }

    pub fn start_scheduler(&self) {
        if self.inner.scheduler_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let engine = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(RULE_CHECK_INTERVAL).await;
                if let Err(error) = engine.run_due_rules(now_ms()).await {
                    tracing::warn!(%error, "couldn't check scheduled rules");
                }
            }
        });
    }
}

async fn run_worker(
    inner: Arc<Inner>,
    account: Snowflake,
    mut queue: mpsc::UnboundedReceiver<String>,
) {
    while let Some(job_id) = queue.recv().await {
        let Some(entry) = inner.jobs.read().get(&job_id).cloned() else {
            continue;
        };
        let Some(client) = inner
            .accounts
            .read()
            .get(&account)
            .map(|runtime| runtime.client.clone())
        else {
            continue;
        };
        execute(&inner, entry, client).await;
    }
}

async fn execute(inner: &Arc<Inner>, entry: JobEntry, client: Discord) {
    let spec = (*entry.spec).clone();
    let mut tracker = Tracker::new(entry.snapshot.clone(), inner.events.clone());

    match prepare(inner, &spec, &entry, &client).await {
        Ok(preparation) => {
            let inbox = Arc::new(ThrottleInbox::default());
            observe_throttles(
                &client,
                entry.snapshot.clone(),
                inner.events.clone(),
                inbox.clone(),
            );
            let pace_scale = *inner.pace_scale.read();
            let run = JobRun {
                pacer: Pacer::scaled(spec.options.speed, pace_scale),
                pace_scale,
                spec,
                client,
                store: inner.store.clone(),
                control: entry.control.clone(),
                tracker,
                filter: preparation.filter,
                archive: preparation.archive,
                throttles: inbox,
                package_index: preparation.package,
                denied_channels: HashSet::new(),
                refused_messages: HashSet::new(),
                unsaved_deletes: 0,
                last_persist: None,
            };
            run.run().await;
        }
        Err(failure) => {
            tracker.fail_before_start(failure);
            let snapshot = entry.snapshot.read().clone();
            if let Err(error) = inner.store.save_job(JobRecord { spec, snapshot }).await {
                tracing::warn!(%error, "couldn't save job state");
            }
        }
    }
}

struct Preparation {
    filter: CompiledFilter,
    archive: Option<Archive>,
    package: Option<PackageIndex>,
}

async fn prepare(
    inner: &Arc<Inner>,
    spec: &JobSpec,
    entry: &JobEntry,
    client: &Discord,
) -> Result<Preparation, JobFailure> {
    let other = |message: String| JobFailure {
        kind: FailureKind::Other,
        message,
    };
    let filter = CompiledFilter::new(&spec.filters, spec.account_id)
        .map_err(|error| other(error.to_string()))?;

    let package = if spec
        .targets
        .iter()
        .any(|target| matches!(target.scope, Scope::Package { .. }))
    {
        inner
            .store
            .package(spec.account_id)
            .await
            .map_err(|error| other(error.to_string()))?
    } else {
        None
    };

    let mut archive = None;
    if spec.options.archive.enabled {
        let existing = entry
            .snapshot
            .read()
            .archive_path
            .clone()
            .map(PathBuf::from);
        let root = inner.archive_root.read().clone();
        let directory = existing.unwrap_or_else(|| archive_folder(&root, &spec.label()));
        let downloader = spec
            .options
            .archive
            .attachments
            .then(|| client.transport().clone());
        let opened = Archive::open(&directory, downloader).map_err(|error| JobFailure {
            kind: FailureKind::Archive,
            message: error.to_string(),
        })?;
        entry.snapshot.write().archive_path = Some(directory.to_string_lossy().into_owned());
        archive = Some(opened);
    }

    Ok(Preparation {
        filter,
        archive,
        package,
    })
}

fn archive_folder(root: &Path, label: &str) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M%S");
    root.join(format!("{stamp} {}", safe_directory_name(label)))
}

#[must_use]
pub fn describe_failure(error: &ApiError) -> JobFailure {
    failure_from(error)
}
