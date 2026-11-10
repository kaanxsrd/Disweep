use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use tokio::sync::broadcast;
use tokio::time::Instant;

use super::control::Control;
use super::pacing::Pacer;
use super::spec::{JobSpec, Scope, Strategy, Target};
use super::status::{
    EngineEvent, FailureKind, FailureRecord, FeedItem, FeedOutcome, JobFailure, JobSnapshot,
    JobState, TargetState,
};
use crate::api::{DeleteOutcome, Discord, Message, SearchScope, Throttle};
use crate::archive::Archive;
use crate::error::ApiError;
use crate::filter::{CompiledFilter, SkipReason, Verdict};
use crate::package::{self, PackageIndex};
use crate::scan::{HistoryScanner, PackageScanner, Scanner, SearchScanner};
use crate::store::{JobRecord, Store};
use crate::{Snowflake, now_ms};

const EVENT_INTERVAL: Duration = Duration::from_millis(200);
const PERSIST_INTERVAL: Duration = Duration::from_secs(3);
const SNIPPET_CHARACTERS: usize = 240;
const RATE_WINDOW: usize = 60;
const MAX_NOTES: usize = 200;

enum Halt {
    Stopped,
    LimitReached,
    SkipTarget(String),
    Failed(JobFailure),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Search,
    History,
    Package,
}

#[derive(Default)]
pub(super) struct ThrottleInbox(Mutex<Vec<Throttle>>);

pub(super) struct Tracker {
    snapshot: Arc<RwLock<JobSnapshot>>,
    events: broadcast::Sender<EngineEvent>,
    feed: Vec<FeedItem>,
    last_event: Option<Instant>,
    delete_times: VecDeque<Instant>,

    page_anchor: u64,
}

impl Tracker {
    pub(super) fn new(
        snapshot: Arc<RwLock<JobSnapshot>>,
        events: broadcast::Sender<EngineEvent>,
    ) -> Self {
        Self {
            snapshot,
            events,
            feed: Vec::new(),
            last_event: None,
            delete_times: VecDeque::new(),
            page_anchor: 0,
        }
    }

    fn publish_now(&mut self) {
        self.recompute();
        let snapshot = self.snapshot.read().clone();
        if !self.feed.is_empty() {
            let items = std::mem::take(&mut self.feed);
            let _ = self.events.send(EngineEvent::Feed {
                job_id: snapshot.id.clone(),
                items,
            });
        }
        let _ = self.events.send(EngineEvent::Job {
            job: Box::new(snapshot),
        });
        self.last_event = Some(Instant::now());
    }

    fn publish_soon(&mut self) {
        if self
            .last_event
            .is_none_or(|last| last.elapsed() >= EVENT_INTERVAL)
        {
            self.publish_now();
        }
    }

    fn recompute(&self) {
        let mut guard = self.snapshot.write();
        let snapshot = &mut *guard;

        let mut total = 0;
        for target in &snapshot.targets {
            total += match target.state {
                TargetState::Done | TargetState::Skipped => target.processed,
                TargetState::Active => target.estimate.unwrap_or(0).max(target.processed),
                TargetState::Pending => target.estimate.unwrap_or(0),
            };
        }
        snapshot.total_estimate = Some(total);

        snapshot.rate_per_minute = match (self.delete_times.front(), self.delete_times.back()) {
            (Some(first), Some(last)) if self.delete_times.len() >= 5 && last > first => {
                (self.delete_times.len() - 1) as f64 / last.duration_since(*first).as_secs_f64()
                    * 60.0
            }
            _ => 0.0,
        };

        let remaining = total.saturating_sub(snapshot.counters.processed());
        let per_message_ms = if snapshot.rate_per_minute > 0.0 {
            60_000.0 / snapshot.rate_per_minute
        } else {
            f64::from(snapshot.pacing_ms)
        };
        snapshot.eta_ms = Some((remaining as f64 * per_message_ms) as u64);

        if snapshot
            .rate_limited_until
            .is_some_and(|until| until <= now_ms())
        {
            snapshot.rate_limited_until = None;
        }
    }

    fn note(&mut self, text: String) {
        let mut snapshot = self.snapshot.write();
        if snapshot.notes.len() < MAX_NOTES && !snapshot.notes.contains(&text) {
            snapshot.notes.push(text);
        }
    }

    fn set_state(&mut self, state: JobState) {
        self.snapshot.write().state = state;
        self.publish_now();
    }

    fn set_pacing(&self, pacing: Duration) {
        self.snapshot.write().pacing_ms = pacing.as_millis() as u32;
    }

    fn enter_target(&mut self, index: usize) {
        {
            let mut snapshot = self.snapshot.write();
            snapshot.current_target = Some(index as u32);
            snapshot.targets[index].state = TargetState::Active;
        }
        self.publish_now();
    }

    fn finish_target(&mut self, index: usize, state: TargetState, note: Option<String>) {
        {
            let mut snapshot = self.snapshot.write();
            let target = &mut snapshot.targets[index];
            target.state = state;
            if note.is_some() {
                target.note = note;
            }
        }
        self.publish_now();
    }

    fn begin_page(&mut self, index: usize, remaining: Option<u64>) {
        let mut snapshot = self.snapshot.write();
        let target = &mut snapshot.targets[index];
        self.page_anchor = target.processed;
        if let Some(remaining) = remaining {
            target.estimate = Some(target.processed + remaining);
        }
    }

    fn checkpoint(&self, index: usize, phase: u32, cursor: Option<Snowflake>) {
        let mut snapshot = self.snapshot.write();
        let target = &mut snapshot.targets[index];
        target.phase = phase;
        target.cursor = cursor;
    }

    fn examined(&self) {
        self.snapshot.write().counters.examined += 1;
    }

    fn deleted_so_far(&self) -> u64 {
        self.snapshot.read().counters.deleted
    }

    fn record(
        &mut self,
        index: usize,
        message: &Message,
        conversation: &str,
        outcome: FeedOutcome,
        note: Option<String>,
    ) {
        {
            let mut snapshot = self.snapshot.write();
            match outcome {
                FeedOutcome::Deleted => snapshot.counters.deleted += 1,
                FeedOutcome::AlreadyGone => snapshot.counters.already_gone += 1,
                FeedOutcome::Skipped => snapshot.counters.skipped += 1,
                FeedOutcome::Failed => snapshot.counters.failed += 1,
            }
            let target = &mut snapshot.targets[index];
            target.processed += 1;
            if outcome == FeedOutcome::Deleted {
                target.deleted += 1;
            }
        }
        if outcome == FeedOutcome::Deleted {
            self.delete_times.push_back(Instant::now());
            if self.delete_times.len() > RATE_WINDOW {
                self.delete_times.pop_front();
            }
        }
        self.feed.push(FeedItem {
            message_id: message.id,
            conversation: conversation.to_owned(),
            sent_at: message.id.timestamp_ms(),
            snippet: message.content.chars().take(SNIPPET_CHARACTERS).collect(),
            attachments: message.attachments.len() as u32,
            outcome,
            note,
        });
        self.publish_soon();
    }

    pub(super) fn fail_before_start(&mut self, failure: JobFailure) {
        self.close(JobState::Failed, Some(failure), None);
    }

    fn close(
        &mut self,
        state: JobState,
        failure: Option<JobFailure>,
        archive_path: Option<String>,
    ) {
        {
            let mut snapshot = self.snapshot.write();
            snapshot.state = state;
            snapshot.failure = failure;
            snapshot.finished_at = Some(now_ms());
            snapshot.current_target = None;
            snapshot.rate_limited_until = None;
            snapshot.eta_ms = None;
            if archive_path.is_some() {
                snapshot.archive_path = archive_path;
            }
        }
        self.publish_now();
    }
}

pub(super) fn observe_throttles(
    client: &Discord,
    snapshot: Arc<RwLock<JobSnapshot>>,
    events: broadcast::Sender<EngineEvent>,
    inbox: Arc<ThrottleInbox>,
) {
    client.set_observer(Arc::new(move |throttle: Throttle| {
        inbox.0.lock().push(throttle);
        let job = {
            let mut snapshot = snapshot.write();
            snapshot.counters.throttle_count += 1;
            snapshot.counters.throttled_ms += throttle.wait.as_millis() as u64;
            snapshot.rate_limited_until = Some(now_ms() + throttle.wait.as_millis() as i64);
            snapshot.clone()
        };
        let _ = events.send(EngineEvent::Job { job: Box::new(job) });
    }));
}

pub(super) struct JobRun {
    pub(super) spec: JobSpec,
    pub(super) client: Discord,
    pub(super) store: Store,
    pub(super) control: Arc<Control>,
    pub(super) tracker: Tracker,
    pub(super) filter: CompiledFilter,
    pub(super) pacer: Pacer,
    pub(super) archive: Option<Archive>,
    pub(super) throttles: Arc<ThrottleInbox>,
    pub(super) package_index: Option<PackageIndex>,
    pub(super) pace_scale: f64,
    pub(super) denied_channels: HashSet<Snowflake>,
    pub(super) refused_messages: HashSet<Snowflake>,
    pub(super) unsaved_deletes: u64,
    pub(super) last_persist: Option<Instant>,
}

impl JobRun {
    pub(super) async fn run(mut self) {
        {
            let mut snapshot = self.tracker.snapshot.write();
            snapshot.started_at.get_or_insert_with(now_ms);
            snapshot.pacing_ms = self.pacer.interval().as_millis() as u32;
        }

        let state = if self.control.is_paused() {
            JobState::Paused
        } else {
            JobState::Running
        };
        self.tracker.set_state(state);
        self.persist().await;

        let result = self.run_targets().await;
        let (state, failure) = match result {
            Ok(()) | Err(Halt::LimitReached) => (JobState::Completed, None),
            Err(Halt::Stopped) => (JobState::Stopped, None),
            Err(Halt::Failed(failure)) => (JobState::Failed, Some(failure)),

            Err(Halt::SkipTarget(reason)) => (
                JobState::Failed,
                Some(JobFailure {
                    kind: FailureKind::Other,
                    message: reason,
                }),
            ),
        };

        let mut archive_path = None;
        if let Some(archive) = self.archive.take() {
            match tokio::task::spawn_blocking(move || archive.finish()).await {
                Ok(Ok(directory)) => archive_path = Some(directory.to_string_lossy().into_owned()),
                Ok(Err(error)) => self.tracker.note(error.to_string()),
                Err(error) => self.tracker.note(error.to_string()),
            }
        }

        self.tracker.close(state, failure, archive_path);
        self.persist().await;
    }

    async fn run_targets(&mut self) -> Result<(), Halt> {
        let targets = self.spec.targets.clone();
        for (index, target) in targets.iter().enumerate() {
            let state = self.tracker.snapshot.read().targets[index].state;
            if matches!(state, TargetState::Done | TargetState::Skipped) {
                continue;
            }
            self.tracker.enter_target(index);
            match self.sweep_target(index, target).await {
                Ok(()) => self.tracker.finish_target(index, TargetState::Done, None),
                Err(Halt::SkipTarget(reason)) => {
                    self.tracker
                        .finish_target(index, TargetState::Skipped, Some(reason))
                }
                Err(Halt::LimitReached) => {
                    self.tracker.finish_target(index, TargetState::Done, None);
                    return Err(Halt::LimitReached);
                }
                Err(other) => return Err(other),
            }
            self.persist().await;
        }
        Ok(())
    }

    fn plan(&self, target: &Target) -> Vec<Phase> {
        let options = &self.spec.options;
        let searched = |also_history: bool| {
            if also_history {
                vec![Phase::Search, Phase::History]
            } else {
                vec![Phase::Search]
            }
        };
        match target.scope {
            Scope::Package { .. } => vec![Phase::Package],
            Scope::Guild { .. } => vec![Phase::Search],
            Scope::Private { .. } | Scope::Channel { .. } => match options.strategy {
                Strategy::History => vec![Phase::History],
                Strategy::Search => searched(options.thorough),
                Strategy::Auto => {
                    let private = matches!(target.scope, Scope::Private { .. });
                    if private && !self.filter.is_selective_for_search() {
                        vec![Phase::History]
                    } else {
                        searched(options.thorough)
                    }
                }
            },
        }
    }

    async fn sweep_target(&mut self, index: usize, target: &Target) -> Result<(), Halt> {
        let mut phases = self.plan(target);
        let (saved_phase, saved_cursor) = {
            let snapshot = self.tracker.snapshot.read();
            (
                snapshot.targets[index].phase as usize,
                snapshot.targets[index].cursor,
            )
        };
        let private = matches!(target.scope, Scope::Private { .. });

        let mut step = saved_phase.min(phases.len());
        let mut cursor = saved_cursor;
        while step < phases.len() {
            let phase = phases[step];
            let result = self
                .run_phase(index, target, phase, step as u32, cursor)
                .await;
            let can_fall_back =
                phase == Phase::Search && private && !phases.contains(&Phase::History);
            match result {
                Ok(left_behind) => {
                    if let Some(left) = left_behind {
                        if can_fall_back {
                            phases.push(Phase::History);
                        } else if !phases.contains(&Phase::History) {
                            self.tracker.note(format!(
                                "{}: Discord's search stopped with about {left} left. Sweep it again with Thorough on.",
                                target.label
                            ));
                        }
                    }
                }
                Err(Halt::Failed(failure))
                    if can_fall_back && failure.kind == FailureKind::Other =>
                {
                    self.tracker.note(format!(
                        "{}: search wasn't available, so full history was used.",
                        target.label
                    ));
                    phases.push(Phase::History);
                }
                Err(Halt::SkipTarget(_)) if can_fall_back => {
                    self.tracker.note(format!(
                        "{}: search wasn't available, so full history was used.",
                        target.label
                    ));
                    phases.push(Phase::History);
                }
                Err(other) => return Err(other),
            }
            step += 1;
            cursor = None;
            self.tracker.checkpoint(index, step as u32, None);
        }
        Ok(())
    }

    async fn scanner(
        &self,
        target: &Target,
        phase: Phase,
        cursor: Option<Snowflake>,
    ) -> Result<Scanner, Halt> {
        let speed = self.spec.options.speed;
        match phase {
            Phase::Search => {
                let (scope, channel, guild_scoped) = match target.scope {
                    Scope::Private { channel_id } => {
                        (SearchScope::Channel(channel_id), None, false)
                    }
                    Scope::Channel {
                        guild_id,
                        channel_id,
                    } => (SearchScope::Guild(guild_id), Some(channel_id), true),
                    Scope::Guild { guild_id } => (SearchScope::Guild(guild_id), None, true),
                    Scope::Package { .. } => {
                        return Err(Halt::SkipTarget(
                            "Search can't read a data package.".to_owned(),
                        ));
                    }
                };
                let mut query = self.filter.search_query(guild_scoped);
                query.channel_ids.extend(channel);
                let interval = speed.search_interval().mul_f64(self.pace_scale);
                Ok(Scanner::Search(SearchScanner::new(
                    self.client.clone(),
                    scope,
                    query,
                    cursor,
                    interval,
                )))
            }
            Phase::History => {
                let channel = match target.scope {
                    Scope::Private { channel_id } | Scope::Channel { channel_id, .. } => channel_id,
                    Scope::Guild { .. } | Scope::Package { .. } => {
                        return Err(Halt::SkipTarget(
                            "Full history needs a single conversation.".to_owned(),
                        ));
                    }
                };
                Ok(Scanner::History(HistoryScanner::new(
                    self.client.clone(),
                    channel,
                    cursor.or_else(|| self.filter.history_start()),
                    self.filter.history_floor(),
                    speed.history_interval().mul_f64(self.pace_scale),
                )))
            }
            Phase::Package => {
                let Scope::Package { channel_id } = target.scope else {
                    return Err(Halt::SkipTarget(
                        "This target isn't in a data package.".to_owned(),
                    ));
                };
                let index = self.package_index.as_ref().ok_or_else(|| {
                    Halt::Failed(JobFailure {
                        kind: FailureKind::Other,
                        message: "The data package is no longer imported.".to_owned(),
                    })
                })?;
                let channel = index
                    .channels
                    .iter()
                    .find(|channel| channel.id == channel_id)
                    .cloned()
                    .ok_or_else(|| {
                        Halt::SkipTarget(
                            "This conversation isn't in the imported package.".to_owned(),
                        )
                    })?;
                let path = index.path.clone();
                let owner = self.spec.account_id;
                let messages = tokio::task::spawn_blocking(move || {
                    package::read_channel(&path, &channel, owner)
                })
                .await
                .map_err(|error| Halt::SkipTarget(error.to_string()))?
                .map_err(|error| Halt::SkipTarget(error.to_string()))?;
                Ok(Scanner::Package(PackageScanner::new(messages, cursor)))
            }
        }
    }

    async fn run_phase(
        &mut self,
        index: usize,
        target: &Target,
        phase: Phase,
        step: u32,
        cursor: Option<Snowflake>,
    ) -> Result<Option<u64>, Halt> {
        let mut scanner = self.scanner(target, phase, cursor).await?;
        loop {
            self.control.proceed().await.map_err(|_| Halt::Stopped)?;
            let next = tokio::select! {
                biased;
                () = self.control.stopped() => return Err(Halt::Stopped),
                page = scanner.next_page() => page,
            };
            self.drain_throttles();
            let page = match next {
                Ok(Some(page)) => page,
                Ok(None) => return Ok(scanner.stalled_with()),
                Err(error) => return Err(classify_scan_error(error)),
            };

            self.tracker.begin_page(index, page.remaining);
            for message in page.messages {
                self.process(index, target, message).await?;
            }
            self.tracker.checkpoint(index, step, scanner.cursor());
            self.tracker.publish_soon();
            self.persist_if_due().await;
        }
    }

    async fn process(
        &mut self,
        index: usize,
        target: &Target,
        message: Message,
    ) -> Result<(), Halt> {
        self.control.proceed().await.map_err(|_| Halt::Stopped)?;
        if self
            .spec
            .options
            .limit
            .is_some_and(|limit| self.tracker.deleted_so_far() >= limit)
        {
            return Err(Halt::LimitReached);
        }

        self.tracker.examined();
        match self.filter.evaluate(&message) {
            Verdict::Delete => {}
            Verdict::Skip(SkipReason::NotYours | SkipReason::NoMatch) => return Ok(()),
            Verdict::Skip(reason) => {
                let note = match reason {
                    SkipReason::Pinned => "Pinned, so it was kept",
                    _ => "A system message, which can't be deleted",
                };
                self.tracker.record(
                    index,
                    &message,
                    &target.label,
                    FeedOutcome::Skipped,
                    Some(note.to_owned()),
                );
                return Ok(());
            }
        }
        if self.refused_messages.contains(&message.id) {
            return Ok(());
        }
        if self.denied_channels.contains(&message.channel_id) {
            let note = Some("Disweep can't access this channel".to_owned());
            self.tracker
                .record(index, &message, &target.label, FeedOutcome::Skipped, note);
            return Ok(());
        }

        if let Some(archive) = self.archive.as_mut() {
            match archive.record(&message, &target.label).await {
                Ok(warnings) => warnings
                    .into_iter()
                    .for_each(|warning| self.tracker.note(warning)),
                Err(error) => {
                    return Err(Halt::Failed(JobFailure {
                        kind: FailureKind::Archive,
                        message: error.to_string(),
                    }));
                }
            }
        }

        tokio::select! {
            biased;
            () = self.control.stopped() => return Err(Halt::Stopped),
            () = self.pacer.wait() => {}
        }
        let outcome = tokio::select! {
            biased;
            () = self.control.stopped() => return Err(Halt::Stopped),
            outcome = self.client.delete_message(message.channel_id, message.id) => outcome,
        };
        self.drain_throttles();
        self.tracker.set_pacing(self.pacer.interval());

        match outcome {
            Ok(DeleteOutcome::Deleted) => {
                self.pacer.on_success();
                self.unsaved_deletes += 1;
                self.tracker
                    .record(index, &message, &target.label, FeedOutcome::Deleted, None);
            }
            Ok(DeleteOutcome::AlreadyGone) => {
                self.pacer.on_success();
                self.tracker.record(
                    index,
                    &message,
                    &target.label,
                    FeedOutcome::AlreadyGone,
                    None,
                );
            }
            Ok(DeleteOutcome::Refused { code, reason }) => {
                self.refused_messages.insert(message.id);
                self.note_failure(index, target, &message, code, reason)
                    .await;
            }
            Ok(DeleteOutcome::ChannelDenied { code, reason }) => {
                self.denied_channels.insert(message.channel_id);
                self.note_failure(index, target, &message, code, reason)
                    .await;
            }
            Err(error) => match error {
                ApiError::Discord {
                    code,
                    message: reason,
                    ..
                } => {
                    self.refused_messages.insert(message.id);
                    self.note_failure(index, target, &message, code, reason)
                        .await;
                }
                other => return Err(Halt::Failed(failure_from(&other))),
            },
        }
        Ok(())
    }

    async fn note_failure(
        &mut self,
        index: usize,
        target: &Target,
        message: &Message,
        code: Option<u32>,
        reason: String,
    ) {
        self.tracker.record(
            index,
            message,
            &target.label,
            FeedOutcome::Failed,
            Some(reason.clone()),
        );
        let job_id = self.tracker.snapshot.read().id.clone();
        let failure = FailureRecord {
            channel_id: message.channel_id,
            message_id: message.id,
            conversation: target.label.clone(),
            code,
            reason,
            at: now_ms(),
        };
        if let Err(error) = self.store.add_failure(job_id, failure).await {
            tracing::warn!(%error, "couldn't record a failed delete");
        }
    }

    fn drain_throttles(&mut self) {
        let drained = std::mem::take(&mut *self.throttles.0.lock());
        for throttle in drained {
            self.pacer.on_throttled(throttle.wait);
        }
    }

    async fn persist_if_due(&mut self) {
        if self
            .last_persist
            .is_none_or(|last| last.elapsed() >= PERSIST_INTERVAL)
        {
            self.persist().await;
        }
    }

    async fn persist(&mut self) {
        self.last_persist = Some(Instant::now());
        let snapshot = self.tracker.snapshot.read().clone();
        let record = JobRecord {
            spec: self.spec.clone(),
            snapshot,
        };
        if let Err(error) = self.store.save_job(record).await {
            tracing::warn!(%error, "couldn't save job progress");
        }
        if self.unsaved_deletes > 0 {
            let count = std::mem::take(&mut self.unsaved_deletes);
            if let Err(error) = self.store.add_deleted(self.spec.account_id, count).await {
                tracing::warn!(%error, "couldn't update statistics");
            }
        }
    }
}

fn classify_scan_error(error: ApiError) -> Halt {
    match &error {
        ApiError::Discord {
            status: 403 | 404, ..
        } => Halt::SkipTarget("Disweep can't read this conversation.".to_owned()),
        ApiError::Discord { status: 400, .. } => Halt::SkipTarget(error.to_string()),
        _ => Halt::Failed(failure_from(&error)),
    }
}

pub(super) fn failure_from(error: &ApiError) -> JobFailure {
    let kind = match error {
        ApiError::Unauthorized | ApiError::VerificationRequired => FailureKind::Login,
        ApiError::Network(_) => FailureKind::Network,
        ApiError::Throttled | ApiError::InvalidRequestBudget => FailureKind::RateLimit,
        ApiError::IndexUnavailable | ApiError::Discord { .. } | ApiError::Decode(_) => {
            FailureKind::Other
        }
    };
    JobFailure {
        kind,
        message: error.to_string(),
    }
}
