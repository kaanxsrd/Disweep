//! The bot's lifecycle: its token, its connection, and what the interface sees.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use disweep_core::api::{Application, AuthScheme, Discord, avatar_url};
use disweep_core::engine::Engine;
use disweep_core::error::ApiError;
use disweep_core::{Secret, Snowflake, now_ms};
use futures::StreamExt;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use twilight_gateway::{CloseFrame, ConfigBuilder, Intents, Message, Shard, ShardId, ShardState};

use crate::commands::{COMMANDS, CommandScope};
use crate::handler::{self, PendingActions};
use crate::interaction::Interaction;
use crate::setup;

const SETTING_ENABLED: &str = "bot.enabled";
const SETTING_DISABLED_COMMANDS: &str = "bot.disabledCommands";
const SETTING_REGISTERED: &str = "bot.registeredPayload";
const MAX_ACTIVITY: usize = 40;
const STATUS_BUFFER: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BotConnection {
    Off,
    Connecting,
    Online,
    Reconnecting,
    Failed,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BotApplication {
    pub id: Snowflake,
    pub name: String,
    pub bot_username: Option<String>,
    pub avatar_url: Option<String>,
    pub supports_user_install: bool,
    pub can_read_message_content: bool,
    pub has_conflicting_endpoint: bool,
    pub server_count: Option<u64>,
    pub user_install_count: Option<u64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallLinks {
    pub account: String,
    pub server: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandState {
    pub key: String,
    pub display: String,
    pub description: String,
    pub scope: CommandScope,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BotActivity {
    pub at: i64,
    pub who: String,
    pub command: String,
    pub summary: String,
}

/// Everything the Bot screen shows.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BotStatus {
    pub has_token: bool,
    pub enabled: bool,
    pub connection: BotConnection,
    pub simulated: bool,
    pub registered: bool,
    pub application: Option<BotApplication>,
    pub links: Option<InstallLinks>,
    pub foreign_commands: Vec<String>,
    pub problem: Option<String>,
    pub latency_ms: Option<u32>,
    pub commands: Vec<CommandState>,
    pub activity: Vec<BotActivity>,
}

#[derive(Debug, thiserror::Error)]
pub enum BotError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("Add a bot token first.")]
    NoToken,
    #[error("This application has other commands, which setup would replace: {}.", .0.join(", "))]
    ForeignCommands(Vec<String>),
    #[error(
        "This application sends commands to a web address, so they'd never reach Disweep. Clear Interactions Endpoint URL in the Developer Portal."
    )]
    EndpointConflict,
    #[error("Couldn't save the bot's settings: {0}")]
    Storage(String),
}

#[derive(Serialize, Deserialize)]
struct Registered {
    application: Snowflake,
    payload: String,
}

struct BotState {
    client: Option<Discord>,
    token: Option<Secret>,
    application: Option<Application>,
    enabled: bool,
    connection: BotConnection,
    registered: bool,
    foreign_commands: Vec<String>,
    problem: Option<String>,
    latency_ms: Option<u32>,
    enabled_commands: HashSet<String>,
    activity: VecDeque<BotActivity>,
    shutdown: Option<CancellationToken>,
}

pub struct BotInner {
    pub(crate) engine: Engine,
    pub(crate) pending: PendingActions,
    state: RwLock<BotState>,
    updates: broadcast::Sender<BotStatus>,
    gateway_url: RwLock<Option<String>>,
}

impl BotInner {
    pub(crate) fn client(&self) -> Option<Discord> {
        self.state.read().client.clone()
    }

    pub(crate) fn command_enabled(&self, key: &str) -> bool {
        self.state.read().enabled_commands.contains(key)
    }

    pub(crate) fn can_read_content(&self) -> bool {
        self.state
            .read()
            .application
            .as_ref()
            .is_some_and(setup::can_read_message_content)
    }

    pub(crate) fn record_activity(&self, who: String, command: String, summary: String) {
        {
            let mut state = self.state.write();
            state.activity.push_front(BotActivity {
                at: now_ms(),
                who,
                command,
                summary,
            });
            state.activity.truncate(MAX_ACTIVITY);
        }
        self.announce();
    }

    fn status(&self) -> BotStatus {
        let state = self.state.read();
        let simulated = self.engine.transport().is_simulated();
        let application = state.application.as_ref().map(|app| BotApplication {
            id: app.id,
            name: app.name.clone(),
            bot_username: app.bot.as_ref().map(|bot| bot.username.clone()),
            avatar_url: if simulated {
                None
            } else {
                app.bot
                    .as_ref()
                    .map(|bot| avatar_url(bot.id, bot.avatar.as_deref()))
            },
            supports_user_install: setup::supports_user_install(app),
            can_read_message_content: setup::can_read_message_content(app),
            has_conflicting_endpoint: app
                .interactions_endpoint_url
                .as_deref()
                .is_some_and(|url| !url.is_empty()),
            server_count: app.approximate_guild_count,
            user_install_count: app.approximate_user_install_count,
        });
        BotStatus {
            has_token: state.token.is_some(),
            enabled: state.enabled,
            connection: state.connection,
            simulated,
            registered: state.registered,
            links: state
                .application
                .as_ref()
                .map(|app| setup::install_links(app.id)),
            application,
            foreign_commands: state.foreign_commands.clone(),
            problem: state.problem.clone(),
            latency_ms: state.latency_ms,
            commands: COMMANDS
                .iter()
                .map(|command| CommandState {
                    key: command.key.to_owned(),
                    display: command.display.to_owned(),
                    description: command.description.to_owned(),
                    scope: command.scope,
                    enabled: state.enabled_commands.contains(command.key),
                })
                .collect(),
            activity: state.activity.iter().cloned().collect(),
        }
    }

    fn announce(&self) {
        let _ = self.updates.send(self.status());
    }

    fn set_connection(&self, connection: BotConnection, problem: Option<String>) {
        {
            let mut state = self.state.write();
            state.connection = connection;
            if problem.is_some() || connection == BotConnection::Online {
                state.problem = problem;
            }
        }
        self.announce();
    }

    fn set_latency(&self, latency: Option<u32>) {
        let changed = {
            let mut state = self.state.write();
            let changed = state.latency_ms != latency;
            state.latency_ms = latency;
            changed
        };
        if changed {
            self.announce();
        }
    }
}

/// The running bot. Clones share one bot.
#[derive(Clone)]
pub struct BotRuntime {
    inner: Arc<BotInner>,
}

impl BotRuntime {
    /// Creates the bot with every command on, then applies any saved choices.
    pub async fn new(engine: Engine) -> Result<Self, BotError> {
        let storage = |error: disweep_core::store::StoreError| BotError::Storage(error.to_string());
        let store = engine.store().clone();
        let enabled = store
            .setting(SETTING_ENABLED.to_owned())
            .await
            .map_err(storage)?
            .as_deref()
            == Some("true");
        let disabled: HashSet<String> = store
            .setting(SETTING_DISABLED_COMMANDS.to_owned())
            .await
            .map_err(storage)?
            .and_then(|saved| serde_json::from_str(&saved).ok())
            .unwrap_or_default();

        let (updates, _) = broadcast::channel(STATUS_BUFFER);
        Ok(Self {
            inner: Arc::new(BotInner {
                engine,
                pending: PendingActions::default(),
                state: RwLock::new(BotState {
                    client: None,
                    token: None,
                    application: None,
                    enabled,
                    connection: BotConnection::Off,
                    registered: false,
                    foreign_commands: Vec::new(),
                    problem: None,
                    latency_ms: None,
                    enabled_commands: COMMANDS
                        .iter()
                        .map(|c| c.key.to_owned())
                        .filter(|key| !disabled.contains(key))
                        .collect(),
                    activity: VecDeque::new(),
                    shutdown: None,
                }),
                updates,
                gateway_url: RwLock::new(None),
            }),
        })
    }

    /// Points the gateway somewhere else, for tests.
    pub fn set_gateway_url(&self, url: String) {
        *self.inner.gateway_url.write() = Some(url);
    }

    /// Shows a problem on the Bot screen, such as a saved token Discord no longer accepts.
    pub fn report_problem(&self, problem: String) {
        self.inner.state.write().problem = Some(problem);
        self.inner.announce();
    }

    #[must_use]
    pub fn status(&self) -> BotStatus {
        self.inner.status()
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<BotStatus> {
        self.inner.updates.subscribe()
    }

    /// Checks a token and starts using it. Nothing on Discord changes yet.
    pub async fn set_token(&self, token: Secret) -> Result<BotStatus, BotError> {
        let client = Discord::new(self.inner.engine.transport(), &token, AuthScheme::Bot);
        let application = client.application().await?;
        let foreign_commands = setup::foreign_commands(&client, application.id).await?;
        self.apply_stored_registration_state(&application).await;
        {
            let mut state = self.inner.state.write();
            state.client = Some(client);
            state.token = Some(token);
            state.application = Some(application);
            state.foreign_commands = foreign_commands;
            state.problem = None;
        }
        self.inner.announce();
        Ok(self.status())
    }

    async fn apply_stored_registration_state(&self, application: &Application) {
        let saved = self
            .inner
            .engine
            .store()
            .setting(SETTING_REGISTERED.to_owned())
            .await
            .ok()
            .flatten();
        let registered = saved
            .and_then(|saved| serde_json::from_str::<Registered>(&saved).ok())
            .is_some_and(|registered| registered.application == application.id);
        self.inner.state.write().registered = registered;
    }

    /// Stops the bot and forgets its token.
    pub async fn clear_token(&self) -> Result<BotStatus, BotError> {
        self.stop();
        {
            let mut state = self.inner.state.write();
            state.client = None;
            state.token = None;
            state.application = None;
            state.registered = false;
            state.foreign_commands.clear();
            state.problem = None;
        }
        self.persist_enabled(false).await?;
        self.inner.state.write().enabled = false;
        self.inner.announce();
        Ok(self.status())
    }

    /// Sets the application up: account installs, message text, and the slash commands.
    pub async fn configure(&self, replace_foreign: bool) -> Result<BotStatus, BotError> {
        let (client, application) = {
            let state = self.inner.state.read();
            (
                state.client.clone().ok_or(BotError::NoToken)?,
                state.application.clone().ok_or(BotError::NoToken)?,
            )
        };
        if application
            .interactions_endpoint_url
            .as_deref()
            .is_some_and(|url| !url.is_empty())
        {
            return Err(BotError::EndpointConflict);
        }
        let foreign = setup::foreign_commands(&client, application.id).await?;
        if !foreign.is_empty() && !replace_foreign {
            self.inner
                .state
                .write()
                .foreign_commands
                .clone_from(&foreign);
            self.inner.announce();
            return Err(BotError::ForeignCommands(foreign));
        }

        let updated = setup::apply_install_settings(&client, &application).await;
        self.inner.state.write().application = Some(updated.clone());
        self.sync_commands(&client, updated.id, true).await?;
        self.inner.state.write().foreign_commands.clear();
        self.inner.announce();
        Ok(self.status())
    }

    /// Registers the enabled commands, skipping the call when nothing changed.
    async fn sync_commands(
        &self,
        client: &Discord,
        application: Snowflake,
        force: bool,
    ) -> Result<(), BotError> {
        let enabled = self.inner.state.read().enabled_commands.clone();
        let payload = crate::commands::registration_payload(&enabled).to_string();
        let store = self.inner.engine.store();
        let saved = store
            .setting(SETTING_REGISTERED.to_owned())
            .await
            .ok()
            .flatten();
        let unchanged = saved
            .and_then(|saved| serde_json::from_str::<Registered>(&saved).ok())
            .is_some_and(|registered| {
                registered.application == application && registered.payload == payload
            });
        if unchanged && !force {
            self.inner.state.write().registered = true;
            return Ok(());
        }

        setup::register(client, application, &enabled).await?;
        let record = serde_json::to_string(&Registered {
            application,
            payload,
        })
        .map_err(|error| BotError::Storage(error.to_string()))?;
        store
            .set_setting(SETTING_REGISTERED.to_owned(), record)
            .await
            .map_err(|error| BotError::Storage(error.to_string()))?;
        self.inner.state.write().registered = true;
        Ok(())
    }

    /// Switches one command on or off and updates Discord if the bot is set up.
    pub async fn set_command_enabled(
        &self,
        key: &str,
        enabled: bool,
    ) -> Result<BotStatus, BotError> {
        let disabled: Vec<String> = {
            let mut state = self.inner.state.write();
            if enabled {
                state.enabled_commands.insert(key.to_owned());
            } else {
                state.enabled_commands.remove(key);
            }
            COMMANDS
                .iter()
                .filter(|c| !state.enabled_commands.contains(c.key))
                .map(|c| c.key.to_owned())
                .collect()
        };
        let saved = serde_json::to_string(&disabled)
            .map_err(|error| BotError::Storage(error.to_string()))?;
        self.inner
            .engine
            .store()
            .set_setting(SETTING_DISABLED_COMMANDS.to_owned(), saved)
            .await
            .map_err(|error| BotError::Storage(error.to_string()))?;

        let (client, application, registered) = {
            let state = self.inner.state.read();
            (
                state.client.clone(),
                state.application.as_ref().map(|app| app.id),
                state.registered,
            )
        };
        if let (Some(client), Some(application), true) = (client, application, registered) {
            self.sync_commands(&client, application, false).await?;
        }
        self.inner.announce();
        Ok(self.status())
    }

    async fn persist_enabled(&self, enabled: bool) -> Result<(), BotError> {
        self.inner
            .engine
            .store()
            .set_setting(SETTING_ENABLED.to_owned(), enabled.to_string())
            .await
            .map_err(|error| BotError::Storage(error.to_string()))
    }

    /// Turns the bot on or off, remembering the choice for next launch.
    pub async fn set_enabled(&self, enabled: bool) -> Result<BotStatus, BotError> {
        if enabled && self.inner.state.read().token.is_none() {
            return Err(BotError::NoToken);
        }
        self.persist_enabled(enabled).await?;
        self.inner.state.write().enabled = enabled;
        if enabled {
            self.start()
        } else {
            self.stop()
        }
        self.inner.announce();
        Ok(self.status())
    }

    /// Connects if the bot should be on, as at launch once the token has been restored.
    pub fn resume_if_enabled(&self) {
        let should_start = {
            let state = self.inner.state.read();
            state.enabled && state.token.is_some()
        };
        if should_start {
            self.start();
        }
    }

    fn start(&self) {
        self.stop();
        let Some(token) = self.inner.state.read().token.clone() else {
            return;
        };

        if self.inner.engine.transport().is_simulated() && self.inner.gateway_url.read().is_none() {
            self.inner.set_connection(BotConnection::Online, None);
            self.inner.set_latency(Some(42));
            return;
        }

        let cancel = CancellationToken::new();
        self.inner.state.write().shutdown = Some(cancel.clone());
        self.inner.set_connection(BotConnection::Connecting, None);
        tokio::spawn(run_gateway(self.inner.clone(), token, cancel));
    }

    fn stop(&self) {
        let shutdown = {
            let mut state = self.inner.state.write();
            state.latency_ms = None;
            state.shutdown.take()
        };
        if let Some(cancel) = shutdown {
            cancel.cancel();
        }
        self.inner.set_connection(BotConnection::Off, None);
    }

    /// Runs a command without Discord, as demo mode's "try it" does.
    pub async fn simulate(&self, payload: serde_json::Value) -> Result<(), BotError> {
        let interaction: Interaction = serde_json::from_value(payload)
            .map_err(|error| BotError::Storage(error.to_string()))?;
        handler::handle(self.inner.clone(), interaction).await;
        Ok(())
    }
}

async fn run_gateway(inner: Arc<BotInner>, token: Secret, cancel: CancellationToken) {
    // Interactions are delivered with no intents at all, so the bot asks for none.
    let mut builder = ConfigBuilder::new(token.expose().to_owned(), Intents::empty());
    if let Some(url) = inner.gateway_url.read().clone() {
        builder = builder.proxy_url(url);
    }
    let mut shard = Shard::with_config(ShardId::ONE, builder.build());

    loop {
        let item = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                shard.close(CloseFrame::NORMAL);
                return;
            }
            item = shard.next() => item,
        };

        match item {
            None => {
                inner.set_connection(BotConnection::Failed, Some(fatal_message(&shard)));
                return;
            }
            Some(Err(error)) => {
                tracing::warn!(%error, "the gateway connection had a problem");
                if !matches!(shard.state(), ShardState::Active) {
                    inner.set_connection(BotConnection::Reconnecting, None);
                }
            }
            Some(Ok(Message::Close(_))) => {
                let connection = if matches!(shard.state(), ShardState::FatallyClosed) {
                    BotConnection::Failed
                } else {
                    BotConnection::Reconnecting
                };
                let problem = (connection == BotConnection::Failed).then(|| fatal_message(&shard));
                inner.set_connection(connection, problem);
            }
            Some(Ok(Message::Text(text))) => {
                let latency = shard
                    .latency()
                    .recent()
                    .last()
                    .map(|latency| latency.as_millis() as u32);
                inner.set_latency(latency);
                dispatch(&inner, &text);
            }
        }
    }
}

fn fatal_message(shard: &Shard) -> String {
    match shard.state() {
        ShardState::FatallyClosed => {
            "Discord refused the bot's connection. Check the token, then turn the bot off and on."
                .to_owned()
        }
        _ => "The connection to Discord ended.".to_owned(),
    }
}

#[derive(Deserialize)]
struct GatewayFrame {
    #[serde(default)]
    op: u8,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    d: Option<serde_json::Value>,
}

fn dispatch(inner: &Arc<BotInner>, text: &str) {
    let Ok(frame) = serde_json::from_str::<GatewayFrame>(text) else {
        return;
    };
    if frame.op != 0 {
        return;
    }
    match frame.t.as_deref() {
        Some("READY" | "RESUMED") => inner.set_connection(BotConnection::Online, None),
        Some("INTERACTION_CREATE") => {
            let Some(payload) = frame.d else { return };
            match serde_json::from_value::<Interaction>(payload) {
                Ok(interaction) => {
                    tokio::spawn(handler::handle(inner.clone(), interaction));
                }
                // The error is only a position, since serde can quote what it failed on.
                Err(error) => tracing::warn!(
                    line = error.line(),
                    column = error.column(),
                    "couldn't read an interaction"
                ),
            }
        }
        _ => {}
    }
}
