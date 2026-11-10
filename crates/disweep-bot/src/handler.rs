//! Turning interactions into engine calls and replies.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use disweep_core::Snowflake;
use disweep_core::api::{Channel, Discord, Message};
use disweep_core::engine::{
    Account, EngineError, EngineEvent, JobOptions, JobOrigin, JobSnapshot, JobSpec, RuleDraft,
    Scope, Target,
};
use disweep_core::error::{ApiError, ApiResult};
use disweep_core::filter::{Filters, HasKind};
use disweep_core::now_ms;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::commands::{CTX_NEWER, CTX_OLDER};
use crate::duration::parse_duration;
use crate::interaction::{CommandOption, Interaction, KIND_COMMAND, KIND_COMPONENT, OptionsExt};
use crate::runtime::BotInner;
use crate::views;

const CALLBACK_MESSAGE: u8 = 4;
const CALLBACK_DEFERRED: u8 = 5;
const CALLBACK_UPDATE: u8 = 7;
const EPHEMERAL: u64 = 1 << 6;

const PENDING_LIFETIME: Duration = Duration::from_secs(14 * 60);
const PROGRESS_EDIT_INTERVAL: Duration = Duration::from_secs(3);
/// Interaction tokens last 15 minutes. Updates stop shortly before that.
const PROGRESS_EDIT_WINDOW: Duration = Duration::from_secs(14 * 60);
const BULK_DELETE_MAX_AGE_MS: i64 = 14 * 86_400_000 - 3_600_000;
const PURGE_SCAN_LIMIT: usize = 5_000;
const DEFAULT_RULE_INTERVAL: Duration = Duration::from_secs(86_400);

/// A destructive action waiting for its confirmation button.
pub enum PendingAction {
    Sweep(Box<JobSpec>),
    Rule {
        draft: RuleDraft,
    },
    Purge {
        channel: Snowflake,
        messages: Vec<Snowflake>,
        old_messages: Vec<Snowflake>,
    },
}

pub struct Pending {
    owner: Snowflake,
    created: Instant,
    action: PendingAction,
}

#[derive(Default)]
pub struct PendingActions(Mutex<HashMap<String, Pending>>);

impl PendingActions {
    fn add(&self, owner: Snowflake, action: PendingAction) -> String {
        let id = Uuid::new_v4().simple().to_string();
        let mut pending = self.0.lock();
        pending.retain(|_, entry| entry.created.elapsed() < PENDING_LIFETIME);
        pending.insert(
            id.clone(),
            Pending {
                owner,
                created: Instant::now(),
                action,
            },
        );
        id
    }

    fn take(&self, id: &str, owner: Snowflake) -> Option<Pending> {
        let mut pending = self.0.lock();
        // Only the person who asked can answer, and only once.
        if pending.get(id)?.owner != owner {
            return None;
        }
        pending
            .remove(id)
            .filter(|entry| entry.created.elapsed() < PENDING_LIFETIME)
    }
}

/// Sends the replies to one interaction, in whichever form is currently valid.
struct Reply {
    client: Discord,
    application: Snowflake,
    interaction: Snowflake,
    token: String,
    acknowledged: bool,
}

impl Reply {
    fn new(client: Discord, interaction: &Interaction) -> Self {
        Self {
            client,
            application: interaction.application_id,
            interaction: interaction.id,
            token: interaction.token.clone(),
            acknowledged: false,
        }
    }

    /// Buys time for work that may take longer than Discord's three-second limit.
    async fn defer(&mut self) -> ApiResult<()> {
        let body = json!({ "type": CALLBACK_DEFERRED, "data": { "flags": EPHEMERAL } });
        self.client
            .interaction_callback(self.interaction, &self.token, body)
            .await?;
        self.acknowledged = true;
        Ok(())
    }

    async fn send(&mut self, view: Value) -> ApiResult<()> {
        if self.acknowledged {
            return self
                .client
                .edit_original_response(self.application, &self.token, for_edit(view))
                .await;
        }
        let body = json!({ "type": CALLBACK_MESSAGE, "data": view });
        self.client
            .interaction_callback(self.interaction, &self.token, body)
            .await?;
        self.acknowledged = true;
        Ok(())
    }

    /// Replaces the message a button was on.
    async fn update(&mut self, view: Value) -> ApiResult<()> {
        let body = json!({ "type": CALLBACK_UPDATE, "data": for_edit(view) });
        self.client
            .interaction_callback(self.interaction, &self.token, body)
            .await?;
        self.acknowledged = true;
        Ok(())
    }
}

/// Whether a reply is private is fixed by its first response, so edits leave the flag out.
fn for_edit(mut view: Value) -> Value {
    if let Some(fields) = view.as_object_mut() {
        fields.remove("flags");
    }
    view
}

/// Something to tell the person, rather than a fault to log.
enum Refusal {
    Tell(String),
    Failed(String),
}

impl From<EngineError> for Refusal {
    fn from(error: EngineError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<ApiError> for Refusal {
    fn from(error: ApiError) -> Self {
        Self::Failed(error.to_string())
    }
}

type Outcome = Result<String, Refusal>;

/// Handles one interaction from start to finish.
pub async fn handle(inner: Arc<BotInner>, interaction: Interaction) {
    let Some(client) = inner.client() else { return };
    let mut reply = Reply::new(client, &interaction);

    let label = describe(&interaction);
    let result = match interaction.kind {
        KIND_COMMAND => run_command(&inner, &interaction, &mut reply).await,
        KIND_COMPONENT => run_component(&inner, &interaction, &mut reply).await,
        _ => return,
    };

    let who = interaction.invoker().map_or_else(
        || "Someone".to_owned(),
        |user| user.display_name().to_owned(),
    );
    let summary = match result {
        Ok(summary) => summary,
        Err(refusal) => {
            let (view, summary) = match refusal {
                Refusal::Tell(text) => (views::notice(&text), text),
                Refusal::Failed(text) => (views::problem(&text), text),
            };
            if let Err(error) = reply.send(view).await {
                tracing::warn!(%error, "couldn't reply to an interaction");
            }
            summary
        }
    };
    if !summary.is_empty() {
        inner.record_activity(who, label, summary);
    }
}

fn describe(interaction: &Interaction) -> String {
    let Some(data) = &interaction.data else {
        return "Unknown".to_owned();
    };
    if interaction.kind == KIND_COMPONENT {
        return "Button".to_owned();
    }
    match interaction.subcommand() {
        Some((sub, _)) => format!("/{} {sub}", data.name),
        None if data.kind == 1 => format!("/{}", data.name),
        None => data.name.clone(),
    }
}

/// The internal key of what was invoked, for checking whether it is switched on.
fn command_key(interaction: &Interaction) -> Option<String> {
    let data = interaction.data.as_ref()?;
    match data.name.as_str() {
        CTX_NEWER => Some("menu.newer".to_owned()),
        CTX_OLDER => Some("menu.older".to_owned()),
        "purge" => Some("purge".to_owned()),
        root => interaction
            .subcommand()
            .map(|(sub, _)| format!("{root}.{sub}")),
    }
}

async fn run_command(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
) -> Outcome {
    let key = command_key(interaction)
        .ok_or_else(|| Refusal::Tell("Disweep doesn't know that command.".to_owned()))?;
    if !inner.command_enabled(&key) {
        return Err(Refusal::Tell(
            "That command is turned off in Disweep.".to_owned(),
        ));
    }

    if key == "purge" {
        return run_purge(inner, interaction, reply).await;
    }

    let account = owner_account(inner, interaction)?;
    let options = interaction
        .subcommand()
        .map_or(&[][..], |(_, options)| options);
    match key.as_str() {
        "sweep.here" => sweep_here(inner, interaction, reply, &account, options).await,
        "sweep.dm" => sweep_dm(inner, interaction, reply, &account, options).await,
        "sweep.status" => status(inner, reply, &account).await,
        "sweep.pause" => control(inner, reply, &account, Control::Pause).await,
        "sweep.resume" => control(inner, reply, &account, Control::Resume).await,
        "sweep.stop" => control(inner, reply, &account, Control::Stop).await,
        "sweep.stats" => {
            let stats = inner.engine.stats(account.id).await?;
            reply.send(views::stats(&stats)).await?;
            Ok("Showed statistics".to_owned())
        }
        "autosweep.set" => autosweep_set(inner, interaction, reply, &account, options).await,
        "autosweep.off" => autosweep_off(inner, interaction, reply, &account).await,
        "autosweep.list" => {
            let rules: Vec<_> = inner
                .engine
                .rules()
                .await?
                .into_iter()
                .filter(|rule| rule.account_id == account.id)
                .collect();
            reply.send(views::rules(&rules)).await?;
            Ok("Listed automatic sweeps".to_owned())
        }
        "menu.newer" => sweep_from_message(inner, interaction, reply, &account, true).await,
        "menu.older" => sweep_from_message(inner, interaction, reply, &account, false).await,
        _ => Err(Refusal::Tell(
            "Disweep doesn't know that command.".to_owned(),
        )),
    }
}

/// The signed-in account of whoever ran this. Everyone else is turned away.
fn owner_account(inner: &BotInner, interaction: &Interaction) -> Result<Account, Refusal> {
    let accounts = inner.engine.accounts();
    if accounts.is_empty() {
        return Err(Refusal::Tell(
            "Sign in to Disweep on your computer first.".to_owned(),
        ));
    }
    let invoker = interaction.invoker().map(|user| user.id);
    accounts
        .into_iter()
        .find(|account| Some(account.id) == invoker)
        .ok_or_else(|| {
            Refusal::Tell(
                "This Disweep belongs to someone else, so it won't act for you.".to_owned(),
            )
        })
}

fn filters_from(options: &[CommandOption]) -> Result<Filters, Refusal> {
    let bad_time = |value: &str| {
        Refusal::Tell(format!(
            "I couldn't read \"{value}\" as a length of time. Try 30d, 2h or 1y."
        ))
    };
    let now = now_ms();
    let mut filters = Filters::default();
    if let Some(text) = options.string("older_than") {
        filters.before = Some(
            now - parse_duration(text)
                .ok_or_else(|| bad_time(text))?
                .as_millis() as i64,
        );
    }
    if let Some(text) = options.string("newer_than") {
        filters.after = Some(
            now - parse_duration(text)
                .ok_or_else(|| bad_time(text))?
                .as_millis() as i64,
        );
    }
    filters.contains = options.string("contains").map(str::to_owned);
    filters.include_pinned = options.boolean("include_pinned").unwrap_or(false);
    if let Some(kind) = options.string("has") {
        filters.has = vec![has_kind(kind)];
    }
    Ok(filters)
}

fn has_kind(value: &str) -> HasKind {
    match value {
        "file" => HasKind::File,
        "image" => HasKind::Image,
        "video" => HasKind::Video,
        "embed" => HasKind::Embed,
        "sticker" => HasKind::Sticker,
        "poll" => HasKind::Poll,
        _ => HasKind::Link,
    }
}

/// The chat this interaction happened in, as something a job can target.
fn current_target(interaction: &Interaction) -> Result<Target, Refusal> {
    let channel = interaction
        .channel_id()
        .ok_or_else(|| Refusal::Failed("Discord didn't say which chat this is.".to_owned()))?;
    let partial = interaction.channel.as_ref();
    let label = partial.and_then(|channel| {
        channel
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                let people: Vec<&str> = channel
                    .recipients
                    .iter()
                    .map(|user| user.display_name())
                    .collect();
                (!people.is_empty()).then(|| people.join(", "))
            })
    });
    let scope = match interaction.guild_id {
        Some(guild_id) => Scope::Channel {
            guild_id,
            channel_id: channel,
        },
        None => Scope::Private {
            channel_id: channel,
        },
    };
    let label = match (interaction.guild_id, label) {
        (Some(_), Some(name)) => format!("#{name}"),
        (_, Some(name)) => name,
        (Some(_), None) => "this channel".to_owned(),
        (None, None) => "this chat".to_owned(),
    };
    Ok(Target {
        scope,
        label,
        expected: None,
    })
}

fn spec_for(account: &Account, target: Target, filters: Filters, limit: Option<u64>) -> JobSpec {
    JobSpec {
        account_id: account.id,
        targets: vec![target],
        filters,
        options: JobOptions {
            limit,
            ..JobOptions::default()
        },
        origin: JobOrigin::Bot,
    }
}

async fn show_preview(
    inner: &Arc<BotInner>,
    reply: &mut Reply,
    account: &Account,
    mut spec: JobSpec,
) -> Outcome {
    let target_label = spec.targets[0].label.clone();
    let preview = inner
        .engine
        .preview(
            Uuid::new_v4().to_string(),
            spec.clone(),
            CancellationToken::new(),
        )
        .await?;
    if preview.total == 0 {
        reply.send(views::nothing_to_sweep(&target_label)).await?;
        return Ok(format!("Nothing to sweep in {target_label}"));
    }
    spec.targets[0].expected = Some(preview.total);
    let id = inner
        .pending
        .add(account.id, PendingAction::Sweep(Box::new(spec)));
    reply
        .send(views::sweep_preview(&id, &target_label, &preview))
        .await?;
    Ok(format!(
        "Previewed {} messages in {target_label}",
        preview.total
    ))
}

async fn sweep_here(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
    account: &Account,
    options: &[CommandOption],
) -> Outcome {
    let filters = filters_from(options)?;
    let target = current_target(interaction)?;
    reply.defer().await?;
    let limit = options.integer("limit").map(|limit| limit.max(1) as u64);
    show_preview(
        inner,
        reply,
        account,
        spec_for(account, target, filters, limit),
    )
    .await
}

async fn sweep_dm(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
    account: &Account,
    options: &[CommandOption],
) -> Outcome {
    let person = options
        .snowflake("user")
        .ok_or_else(|| Refusal::Tell("Choose who to sweep your DM with.".to_owned()))?;
    let filters = filters_from(options)?;
    reply.defer().await?;

    let client = inner
        .engine
        .client(account.id)
        .ok_or_else(|| Refusal::Failed("That account isn't signed in.".to_owned()))?;
    let channel: Channel = client.open_dm(person).await?;
    let name = interaction
        .data
        .as_ref()
        .and_then(|data| data.resolved.as_ref())
        .and_then(|resolved| resolved.users.get(&person.to_string()))
        .map_or_else(
            || channel.display_name(),
            |user| user.display_name().to_owned(),
        );
    let target = Target {
        scope: Scope::Private {
            channel_id: channel.id,
        },
        label: name,
        expected: None,
    };
    let limit = options.integer("limit").map(|limit| limit.max(1) as u64);
    show_preview(
        inner,
        reply,
        account,
        spec_for(account, target, filters, limit),
    )
    .await
}

async fn sweep_from_message(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
    account: &Account,
    newer: bool,
) -> Outcome {
    let message = interaction
        .data
        .as_ref()
        .and_then(|data| data.target_id)
        .ok_or_else(|| Refusal::Failed("Discord didn't say which message this is.".to_owned()))?;
    let target = current_target(interaction)?;
    reply.defer().await?;

    let mut filters = Filters::default();
    if newer {
        filters.after_message = Some(message);
    } else {
        filters.before_message = Some(message);
    }
    show_preview(
        inner,
        reply,
        account,
        spec_for(account, target, filters, None),
    )
    .await
}

async fn status(inner: &Arc<BotInner>, reply: &mut Reply, account: &Account) -> Outcome {
    match latest_active(inner, account) {
        Some(job) => {
            reply.send(views::job(&job)).await?;
            Ok(format!("Showed progress on {}", job.label))
        }
        None => Err(Refusal::Tell("Nothing is sweeping right now.".to_owned())),
    }
}

fn latest_active(inner: &BotInner, account: &Account) -> Option<JobSnapshot> {
    inner
        .engine
        .active_jobs()
        .into_iter()
        .find(|job| job.account_id == account.id)
}

enum Control {
    Pause,
    Resume,
    Stop,
}

async fn control(
    inner: &Arc<BotInner>,
    reply: &mut Reply,
    account: &Account,
    action: Control,
) -> Outcome {
    let job = latest_active(inner, account)
        .ok_or_else(|| Refusal::Tell("Nothing is sweeping right now.".to_owned()))?;
    let (message, summary) = match action {
        Control::Pause => {
            inner.engine.pause(&job.id)?;
            ("Paused. Use `/sweep resume` to continue.", "Paused")
        }
        Control::Resume => {
            inner.engine.resume(&job.id)?;
            ("Sweeping again.", "Resumed")
        }
        Control::Stop => {
            inner.engine.stop(&job.id)?;
            (
                "Stopped. What was already deleted stays deleted.",
                "Stopped",
            )
        }
    };
    reply.send(views::notice(message)).await?;
    Ok(format!("{summary} {}", job.label))
}

async fn autosweep_set(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
    account: &Account,
    options: &[CommandOption],
) -> Outcome {
    let bad_time = |value: &str| {
        Refusal::Tell(format!(
            "I couldn't read \"{value}\" as a length of time. Try 30d, 2h or 1y."
        ))
    };
    let older_text = options.string("older_than").unwrap_or_default();
    let older_than = parse_duration(older_text).ok_or_else(|| bad_time(older_text))?;
    let every = match options.string("every") {
        Some(text) => parse_duration(text).ok_or_else(|| bad_time(text))?,
        None => DEFAULT_RULE_INTERVAL,
    };
    let target = current_target(interaction)?;
    let label = target.label.clone();
    let draft = RuleDraft {
        account_id: account.id,
        label: label.clone(),
        targets: vec![target],
        older_than_secs: older_than.as_secs(),
        every_secs: every.as_secs(),
        filters: Filters::default(),
    };
    let id = inner.pending.add(account.id, PendingAction::Rule { draft });
    let view = views::rule_preview(
        &id,
        &label,
        &views::duration_short(older_than.as_secs()),
        &views::duration_short(every.as_secs()),
    );
    reply.send(view).await?;
    Ok(format!("Offered an automatic sweep for {label}"))
}

async fn autosweep_off(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
    account: &Account,
) -> Outcome {
    let scope = current_target(interaction)?.scope;
    let mut removed = 0;
    for rule in inner.engine.rules().await? {
        if rule.account_id == account.id && rule.targets.iter().all(|target| target.scope == scope)
        {
            inner.engine.delete_rule(&rule.id).await?;
            removed += 1;
        }
    }
    if removed == 0 {
        return Err(Refusal::Tell(
            "This chat has no automatic sweep.".to_owned(),
        ));
    }
    reply
        .send(views::notice(
            "This chat will no longer be swept automatically.",
        ))
        .await?;
    Ok("Turned off an automatic sweep".to_owned())
}

async fn run_purge(inner: &Arc<BotInner>, interaction: &Interaction, reply: &mut Reply) -> Outcome {
    if !interaction.is_in_guild() {
        return Err(Refusal::Tell("`/purge` only works in servers.".to_owned()));
    }
    if !interaction.invoker_can_manage_messages() {
        return Err(Refusal::Tell(
            "You need the Manage Messages permission to use this.".to_owned(),
        ));
    }
    if !interaction.bot_can_purge() {
        return Err(Refusal::Tell(
            "I need the Manage Messages and Read Message History permissions in this channel."
                .to_owned(),
        ));
    }
    let channel = interaction
        .channel_id()
        .ok_or_else(|| Refusal::Failed("Discord didn't say which channel this is.".to_owned()))?;
    let options = interaction.top_level_options();
    let wanted = options.integer("count").unwrap_or(0).clamp(1, 1000) as usize;
    let author = options.snowflake("user");
    let contains = options.string("contains").map(str::to_lowercase);
    let has = options.string("has").map(has_kind);
    let include_old = options.boolean("include_old").unwrap_or(false);

    if (contains.is_some() || has.is_some()) && !inner.can_read_content() {
        return Err(Refusal::Tell(
            "To filter by content, turn on Message Content Intent for this application in the Discord Developer Portal.".to_owned(),
        ));
    }

    reply.defer().await?;
    let client = reply.client.clone();
    let cutoff = Snowflake::from_timestamp_ms(now_ms() - BULK_DELETE_MAX_AGE_MS);

    let mut recent = Vec::new();
    let mut old = Vec::new();
    let mut before = None;
    let mut examined = 0;
    'scan: while examined < PURGE_SCAN_LIMIT {
        let page = client.messages(channel, before, 100).await?;
        let Some(oldest) = page.iter().map(|message| message.id).min() else {
            break;
        };
        before = Some(oldest);
        examined += page.len();
        for message in page {
            if !purge_matches(&message, author, contains.as_deref(), has) {
                continue;
            }
            if recent.len() + old.len() >= wanted {
                break 'scan;
            }
            if message.id >= cutoff {
                recent.push(message.id)
            } else {
                old.push(message.id)
            }
        }
    }

    let too_old = old.len() as u64;
    let total = (recent.len() as u64) + if include_old { too_old } else { 0 };
    if total == 0 {
        reply
            .send(views::notice("No messages in this channel match."))
            .await?;
        return Ok("Nothing to purge".to_owned());
    }
    let invoker = interaction
        .invoker()
        .map(|user| user.id)
        .unwrap_or_default();
    let old_messages = if include_old { old } else { Vec::new() };
    let id = inner.pending.add(
        invoker,
        PendingAction::Purge {
            channel,
            messages: recent,
            old_messages,
        },
    );
    reply
        .send(views::purge_preview(&id, total, too_old, include_old))
        .await?;
    Ok(format!("Previewed a purge of {total} messages"))
}

fn purge_matches(
    message: &Message,
    author: Option<Snowflake>,
    contains: Option<&str>,
    has: Option<HasKind>,
) -> bool {
    if author.is_some_and(|author| message.author.id != author) {
        return false;
    }
    if contains.is_some_and(|text| !message.content.to_lowercase().contains(text)) {
        return false;
    }
    let Some(kind) = has else { return true };
    match kind {
        HasKind::Link => {
            message.content.contains("http://") || message.content.contains("https://")
        }
        HasKind::Embed => !message.embeds.is_empty(),
        HasKind::Sticker => !message.sticker_items.is_empty(),
        HasKind::Poll => message.poll.is_some(),
        HasKind::Image => message.attachments.iter().any(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|c| c.starts_with("image/"))
        }),
        HasKind::Video => message.attachments.iter().any(|a| {
            a.content_type
                .as_deref()
                .is_some_and(|c| c.starts_with("video/"))
        }),
        _ => !message.attachments.is_empty(),
    }
}

async fn run_component(
    inner: &Arc<BotInner>,
    interaction: &Interaction,
    reply: &mut Reply,
) -> Outcome {
    let custom_id = interaction
        .data
        .as_ref()
        .and_then(|data| data.custom_id.as_deref())
        .unwrap_or_default();
    let mut parts = custom_id.split(':');
    if parts.next() != Some("ds") {
        return Ok(String::new());
    }
    let verb = parts.next().unwrap_or_default();
    let id = parts.next().unwrap_or_default();
    let invoker = interaction
        .invoker()
        .map(|user| user.id)
        .unwrap_or_default();

    let Some(pending) = inner.pending.take(id, invoker) else {
        reply.update(views::expired()).await?;
        return Ok("A confirmation had expired".to_owned());
    };
    if verb != "ok" {
        reply.update(views::cancelled()).await?;
        return Ok("Cancelled".to_owned());
    }

    match pending.action {
        PendingAction::Sweep(spec) => {
            let job = inner.engine.start(*spec).await?;
            reply.update(views::job(&job)).await?;
            spawn_progress_updates(
                inner.clone(),
                reply.client.clone(),
                interaction.application_id,
                interaction.token.clone(),
                job.id.clone(),
            );
            Ok(format!("Started sweeping {}", job.label))
        }
        PendingAction::Rule { draft } => {
            let label = draft.label.clone();
            inner.engine.add_rule(draft).await?;
            reply
                .update(views::notice(
                    "Turned on. This chat will be kept clean for as long as Disweep is running.",
                ))
                .await?;
            Ok(format!("Turned on an automatic sweep for {label}"))
        }
        PendingAction::Purge {
            channel,
            messages,
            old_messages,
        } => {
            reply.update(views::notice("Purging…")).await?;
            let (deleted, failed) =
                execute_purge(&reply.client, channel, &messages, &old_messages, invoker).await;
            reply
                .client
                .edit_original_response(
                    interaction.application_id,
                    &interaction.token,
                    for_edit(views::purge_finished(deleted, failed)),
                )
                .await?;
            Ok(format!("Purged {deleted} messages"))
        }
    }
}

async fn execute_purge(
    client: &Discord,
    channel: Snowflake,
    recent: &[Snowflake],
    old: &[Snowflake],
    moderator: Snowflake,
) -> (u64, u64) {
    let reason = format!("Disweep purge requested by {moderator}");
    let (mut deleted, mut failed) = (0, 0);
    for chunk in recent.chunks(100) {
        // Discord's bulk endpoint needs at least two messages.
        if let [single] = chunk {
            match client
                .delete_message_as_bot(channel, *single, &reason)
                .await
            {
                Ok(()) => deleted += 1,
                Err(_) => failed += 1,
            }
            continue;
        }
        match client.bulk_delete(channel, chunk, &reason).await {
            Ok(()) => deleted += chunk.len() as u64,
            Err(error) => {
                tracing::warn!(%error, "a bulk delete failed");
                failed += chunk.len() as u64;
            }
        }
    }
    for message in old {
        match client
            .delete_message_as_bot(channel, *message, &reason)
            .await
        {
            Ok(()) => deleted += 1,
            Err(_) => failed += 1,
        }
    }
    (deleted, failed)
}

/// Keeps the reply's progress current until the sweep ends or Discord's token runs out.
fn spawn_progress_updates(
    inner: Arc<BotInner>,
    client: Discord,
    application: Snowflake,
    token: String,
    job_id: String,
) {
    tokio::spawn(async move {
        let mut events = inner.engine.subscribe();
        let started = Instant::now();
        let mut last_edit = Instant::now();

        // The job may have finished before this task subscribed.
        if let Some(snapshot) = inner.engine.snapshot(&job_id)
            && !snapshot.state.is_active()
        {
            let _ = client
                .edit_original_response(application, &token, for_edit(views::job(&snapshot)))
                .await;
            return;
        }

        loop {
            let event = match tokio::time::timeout(PROGRESS_EDIT_INTERVAL, events.recv()).await {
                Ok(Ok(event)) => Some(event),
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) | Err(_) => None,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return,
            };
            let finished = matches!(&event, Some(EngineEvent::Job { job }) if job.id == job_id && !job.state.is_active());
            if !finished && last_edit.elapsed() < PROGRESS_EDIT_INTERVAL {
                continue;
            }

            let Some(snapshot) = inner.engine.snapshot(&job_id) else {
                return;
            };
            if !snapshot.state.is_active() {
                if let Err(error) = client
                    .edit_original_response(application, &token, for_edit(views::job(&snapshot)))
                    .await
                {
                    tracing::warn!(%error, "couldn't post a sweep's final result");
                }
                return;
            }
            if started.elapsed() >= PROGRESS_EDIT_WINDOW {
                let _ = client
                    .edit_original_response(
                        application,
                        &token,
                        for_edit(views::still_running(&snapshot)),
                    )
                    .await;
                return;
            }
            let _ = client
                .edit_original_response(application, &token, for_edit(views::job(&snapshot)))
                .await;
            last_edit = Instant::now();
        }
    });
}
