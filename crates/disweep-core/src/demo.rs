use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::Snowflake;
use crate::api::{ApiRequest, ApiResponse, Method, Transport, TransportError};
use crate::now_ms;

pub const DEMO_TOKEN: &str = "demo-account-token-not-a-real-one";
pub const DEMO_BOT_TOKEN: &str = "demo-bot-token-not-a-real-one-either";
pub const DEMO_USER_ID: Snowflake = Snowflake(100_000_000_000_000_001);
const DEMO_APPLICATION_ID: Snowflake = Snowflake(900_000_000_000_000_001);
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Default)]
pub struct Faults {
    pub latency: Duration,

    pub rate_limit_every: Option<u32>,
    pub rate_limit_seconds: f64,

    pub index_pending: u32,

    pub stale_search: bool,

    pub hidden_from_search: HashSet<u64>,

    pub empty_search_pages: u32,

    pub search_ignores_author: bool,

    pub refuse_delete: HashMap<u64, u32>,

    pub denied_channels: HashSet<u64>,

    pub dm_search_missing: bool,

    pub reject_token: bool,
}

#[derive(Clone)]
struct FakeMessage {
    id: u64,
    channel: u64,
    author: Value,
    author_id: u64,
    kind: u16,
    content: String,
    pinned: bool,
    attachments: Vec<Value>,
}

#[derive(Default)]
struct State {
    me: Option<Value>,
    guilds: Vec<Value>,
    guild_channels: HashMap<u64, Vec<Value>>,
    dms: Vec<Value>,
    channels: HashMap<u64, Value>,
    messages: BTreeMap<u64, BTreeMap<u64, FakeMessage>>,
    deleted_for_search: BTreeMap<u64, BTreeMap<u64, FakeMessage>>,
    requests: Vec<ApiRequest>,
    deletes_seen: u32,
    searches_seen: u32,
    next_id: u64,
    application: Option<Value>,
    commands: Vec<Value>,
    interaction_replies: Vec<(String, Value)>,
    sent_messages: Vec<(u64, Value)>,
    faults: Faults,
}

pub struct FakeDiscord {
    state: Mutex<State>,
    tokens: Vec<String>,
}

fn user(id: u64, name: &str) -> Value {
    json!({ "id": id.to_string(), "username": name.to_lowercase(), "global_name": name })
}

impl FakeDiscord {
    #[must_use]
    pub fn new() -> Self {
        let state = State {
            next_id: 1 << 40,
            ..State::default()
        };
        Self {
            state: Mutex::new(state),
            tokens: vec![
                DEMO_TOKEN.to_owned(),
                format!("Bot {DEMO_BOT_TOKEN}"),
                DEMO_BOT_TOKEN.to_owned(),
            ],
        }
    }

    pub fn set_faults(&self, faults: Faults) {
        self.state.lock().faults = faults;
    }

    pub fn set_me(&self, id: u64, name: &str) {
        self.state.lock().me = Some(user(id, name));
    }

    pub fn state_set_bot(&self) {
        self.state.lock().me = Some(json!({ "id": "1", "username": "some-bot", "bot": true }));
    }

    pub fn add_guild(&self, id: u64, name: &str) {
        self.state
            .lock()
            .guilds
            .push(json!({ "id": id.to_string(), "name": name }));
    }

    pub fn add_guild_channel(&self, guild: u64, id: u64, name: &str) {
        let channel = json!({ "id": id.to_string(), "type": 0, "guild_id": guild.to_string(), "name": name, "position": id });
        let mut state = self.state.lock();
        state
            .guild_channels
            .entry(guild)
            .or_default()
            .push(channel.clone());
        state.channels.insert(id, channel);
    }

    pub fn add_dm(&self, id: u64, with_id: u64, with_name: &str) {
        let channel =
            json!({ "id": id.to_string(), "type": 1, "recipients": [user(with_id, with_name)] });
        let mut state = self.state.lock();
        state.dms.push(channel.clone());
        state.channels.insert(id, channel);
    }

    pub fn add_group(&self, id: u64, name: &str, people: &[(u64, &str)]) {
        let recipients: Vec<Value> = people.iter().map(|(id, name)| user(*id, name)).collect();
        let channel =
            json!({ "id": id.to_string(), "type": 3, "name": name, "recipients": recipients });
        let mut state = self.state.lock();
        state.dms.push(channel.clone());
        state.channels.insert(id, channel);
    }

    pub fn add_message(&self, channel: u64, id: u64, author: (u64, &str), content: &str) {
        self.insert(FakeMessage {
            id,
            channel,
            author: user(author.0, author.1),
            author_id: author.0,
            kind: 0,
            content: content.to_owned(),
            pinned: false,
            attachments: Vec::new(),
        });
    }

    pub fn add_pinned(&self, channel: u64, id: u64, author: (u64, &str), content: &str) {
        self.add_message(channel, id, author, content);
        if let Some(message) = self
            .state
            .lock()
            .messages
            .get_mut(&channel)
            .and_then(|m| m.get_mut(&id))
        {
            message.pinned = true;
        }
    }

    pub fn add_system(&self, channel: u64, id: u64, author: (u64, &str), kind: u16) {
        self.add_message(channel, id, author, "");
        if let Some(message) = self
            .state
            .lock()
            .messages
            .get_mut(&channel)
            .and_then(|m| m.get_mut(&id))
        {
            message.kind = kind;
        }
    }

    pub fn add_attachment(&self, channel: u64, id: u64, filename: &str, url: &str) {
        if let Some(message) = self
            .state
            .lock()
            .messages
            .get_mut(&channel)
            .and_then(|m| m.get_mut(&id))
        {
            message
                .attachments
                .push(json!({ "id": "1", "filename": filename, "url": url }));
        }
    }

    pub fn set_application(&self, application: Value) {
        self.state.lock().application = Some(application);
    }

    pub fn set_commands(&self, commands: Vec<Value>) {
        self.state.lock().commands = commands;
    }

    fn insert(&self, message: FakeMessage) {
        self.state
            .lock()
            .messages
            .entry(message.channel)
            .or_default()
            .insert(message.id, message);
    }

    #[must_use]
    pub fn remaining(&self, channel: u64) -> Vec<u64> {
        self.state
            .lock()
            .messages
            .get(&channel)
            .map(|messages| messages.keys().copied().collect())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn count_requests(&self, method: &Method) -> usize {
        self.state
            .lock()
            .requests
            .iter()
            .filter(|request| request.method == *method)
            .count()
    }

    #[must_use]
    pub fn requests(&self) -> Vec<ApiRequest> {
        self.state.lock().requests.clone()
    }

    #[must_use]
    pub fn interaction_replies(&self) -> Vec<(String, Value)> {
        self.state.lock().interaction_replies.clone()
    }

    #[must_use]
    pub fn sent_messages(&self) -> Vec<(u64, Value)> {
        self.state.lock().sent_messages.clone()
    }

    #[must_use]
    pub fn commands(&self) -> Vec<Value> {
        self.state.lock().commands.clone()
    }

    #[must_use]
    pub fn demo() -> Self {
        let fake = Self::new();
        fake.set_me(DEMO_USER_ID.0, "Kit");
        fake.set_faults(Faults {
            latency: Duration::from_millis(45),
            rate_limit_every: Some(70),
            rate_limit_seconds: 2.4,
            ..Faults::default()
        });
        fake.set_application(json!({
            "id": DEMO_APPLICATION_ID.to_string(),
            "name": "Disweep Demo",
            "bot": { "id": "900000000000000002", "username": "disweep-demo", "bot": true },
            "integration_types_config": { "0": {}, "1": {} },
            "approximate_guild_count": 2,
            "approximate_user_install_count": 1,
        }));

        fake.add_dm(201, 301, "Mira");
        fake.add_dm(202, 302, "Jonas");
        fake.add_dm(203, 303, "Sam Okafor");
        fake.add_dm(204, 304, "Noor");
        fake.add_dm(205, 305, "Lena");
        fake.add_group(
            206,
            "Weekend plans",
            &[(301, "Mira"), (303, "Sam Okafor"), (305, "Lena")],
        );

        fake.add_guild(401, "Rust Harbor");
        fake.add_guild_channel(401, 411, "general");
        fake.add_guild_channel(401, 412, "help");
        fake.add_guild_channel(401, 413, "showcase");
        fake.add_guild(402, "Midnight Arcade");
        fake.add_guild_channel(402, 421, "lobby");
        fake.add_guild_channel(402, 422, "speedruns");
        fake.add_guild(403, "Design Dept.");
        fake.add_guild_channel(403, 431, "critique");
        fake.add_guild_channel(403, 432, "resources");

        let conversations: [(u64, (u64, &str), u32); 12] = [
            (201, (301, "Mira"), 420),
            (202, (302, "Jonas"), 180),
            (203, (303, "Sam Okafor"), 960),
            (204, (304, "Noor"), 75),
            (205, (305, "Lena"), 240),
            (206, (301, "Mira"), 310),
            (411, (302, "Jonas"), 520),
            (412, (303, "Sam Okafor"), 140),
            (413, (305, "Lena"), 60),
            (421, (304, "Noor"), 380),
            (422, (302, "Jonas"), 95),
            (431, (301, "Mira"), 210),
        ];
        let now = now_ms();
        let mut seed: u64 = 0x5eed_cafe;
        for (channel, other, count) in conversations {
            fake.seed_channel(channel, other, count, now, &mut seed);
        }
        fake
    }

    fn seed_channel(&self, channel: u64, other: (u64, &str), count: u32, now: i64, seed: &mut u64) {
        const MINE: [&str; 16] = [
            "on my way, ten minutes out",
            "can you send me that link again? https://example.com/notes",
            "honestly I think we should ship it as is",
            "lol that's exactly what I said last week",
            "here's the password for the staging box, delete this after reading",
            "I'll take a look tonight",
            "did anyone back up the old config?",
            "moving this to Thursday, sorry",
            "that's my home address, don't share it",
            "check this out https://example.org/clip",
            "wait, which branch is that on",
            "good call, merged",
            "I can't make it on Saturday anymore",
            "brb, getting coffee",
            "sending the invoice now",
            "yeah I regret saying that",
        ];
        const THEIRS: [&str; 8] = [
            "sounds good to me",
            "haha nice",
            "sent you the file",
            "wait what happened",
            "ok see you then",
            "can you check the logs?",
            "thanks!",
            "I'll ping you tomorrow",
        ];

        let mut next = |bound: u64| {
            *seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (*seed >> 33) % bound
        };
        let me = self
            .state
            .lock()
            .me
            .clone()
            .unwrap_or_else(|| user(1, "me"));
        let my_id: u64 = me["id"]
            .as_str()
            .and_then(|id| id.parse().ok())
            .unwrap_or(1);

        for index in 0..count {
            let age_days = i64::from(index) * 1_100 / i64::from(count);
            let at = now - age_days * DAY_MS - (next(DAY_MS as u64) as i64);
            let id = Snowflake::from_timestamp_ms(at).0 + next(4096);
            let mine = next(100) < 62;
            let (author, author_id, text) = if mine {
                (me.clone(), my_id, MINE[next(MINE.len() as u64) as usize])
            } else {
                (
                    user(other.0, other.1),
                    other.0,
                    THEIRS[next(THEIRS.len() as u64) as usize],
                )
            };
            let mut attachments = Vec::new();
            if mine && next(100) < 8 {
                attachments.push(json!({
                    "id": "1", "filename": "screenshot.png", "content_type": "image/png",
                    "url": "https://cdn.discordapp.com/attachments/0/0/screenshot.png"
                }));
            }
            self.insert(FakeMessage {
                id,
                channel,
                author,
                author_id,
                kind: 0,
                content: text.to_owned(),
                pinned: mine && next(100) < 2,
                attachments,
            });
        }
    }

    fn respond(state: &mut State, request: &ApiRequest) -> ApiResponse {
        let segments: Vec<&str> = request
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let query = |key: &str| query_value(request, key);
        let id = |text: &str| text.parse::<u64>().unwrap_or(0);

        match (&request.method, segments.as_slice()) {
            (&Method::GET, ["users", "@me"]) => ok(state.me.clone().unwrap_or(Value::Null)),
            (&Method::GET, ["users", "@me", "guilds"]) => {
                let after = query("after").map_or(0, id);
                let limit = query("limit").map_or(200, id) as usize;
                let guilds: Vec<Value> = state
                    .guilds
                    .iter()
                    .filter(|guild| guild["id"].as_str().map_or(0, id) > after)
                    .take(limit)
                    .cloned()
                    .collect();
                ok(Value::Array(guilds))
            }
            (&Method::GET, ["users", "@me", "channels"]) => {
                let mut dms = state.dms.clone();
                for channel in &mut dms {
                    let channel_id = channel["id"].as_str().map_or(0, id);
                    if let Some(last) = state
                        .messages
                        .get(&channel_id)
                        .and_then(|m| m.keys().next_back())
                    {
                        channel["last_message_id"] = json!(last.to_string());
                    }
                }
                ok(Value::Array(dms))
            }
            (&Method::POST, ["users", "@me", "channels"]) => {
                let recipient = request
                    .body
                    .as_ref()
                    .and_then(|b| b["recipient_id"].as_str())
                    .map_or(0, id);
                let existing = state.dms.iter().find(|channel| {
                    channel["type"] == 1
                        && channel["recipients"][0]["id"].as_str().map_or(0, id) == recipient
                });
                match existing {
                    Some(channel) => ok(channel.clone()),
                    None => {
                        state.next_id += 1;
                        let channel = json!({
                            "id": state.next_id.to_string(), "type": 1,
                            "recipients": [user(recipient, "Someone")]
                        });
                        state.dms.push(channel.clone());
                        state.channels.insert(state.next_id, channel.clone());
                        ok(channel)
                    }
                }
            }
            (&Method::GET, ["guilds", guild, "channels"]) => ok(Value::Array(
                state
                    .guild_channels
                    .get(&id(guild))
                    .cloned()
                    .unwrap_or_default(),
            )),
            (&Method::GET, ["channels", channel]) => match state.channels.get(&id(channel)) {
                Some(found) => ok(found.clone()),
                None => error(404, 10003, "Unknown Channel"),
            },
            (&Method::GET, ["channels", channel, "messages"]) => {
                Self::history(state, id(channel), request)
            }
            (&Method::GET, ["channels", channel, "messages", "search"]) => {
                if state.faults.dm_search_missing {
                    return error(404, 0, "404: Not Found");
                }
                Self::search(state, Some(id(channel)), None, request)
            }
            (&Method::GET, ["guilds", guild, "messages", "search"]) => {
                Self::search(state, None, Some(id(guild)), request)
            }
            (&Method::DELETE, ["channels", channel, "messages", message]) => {
                Self::delete(state, id(channel), id(message))
            }
            (&Method::POST, ["channels", channel, "messages", "bulk-delete"]) => {
                let ids: Vec<u64> = request
                    .body
                    .as_ref()
                    .and_then(|b| b["messages"].as_array())
                    .map(|ids| ids.iter().filter_map(|v| v.as_str()).map(id).collect())
                    .unwrap_or_default();
                if !(2..=100).contains(&ids.len()) {
                    return error(400, 50016, "You must provide between 2 and 100 messages");
                }
                let oldest_allowed = Snowflake::from_timestamp_ms(now_ms() - 14 * DAY_MS).0;
                if ids.iter().any(|message| *message < oldest_allowed) {
                    return error(400, 50034, "A message provided was too old to bulk delete");
                }
                if let Some(messages) = state.messages.get_mut(&id(channel)) {
                    for message in ids {
                        messages.remove(&message);
                    }
                }
                empty(204)
            }
            (&Method::POST, ["channels", channel, "messages"]) => {
                state
                    .sent_messages
                    .push((id(channel), request.body.clone().unwrap_or(Value::Null)));
                ok(json!({ "id": "1" }))
            }
            (&Method::GET, ["applications", "@me"]) => match &state.application {
                Some(application) => ok(application.clone()),
                None => error(401, 0, "401: Unauthorized"),
            },
            (&Method::PATCH, ["applications", "@me"]) => {
                let patch = request.body.clone().unwrap_or_else(|| json!({}));
                let application = state.application.get_or_insert_with(|| json!({}));
                if let (Some(target), Some(source)) =
                    (application.as_object_mut(), patch.as_object())
                {
                    target.extend(source.clone());
                }
                ok(application.clone())
            }
            (&Method::GET, ["applications", _, "commands"]) => {
                ok(Value::Array(state.commands.clone()))
            }
            (&Method::PUT, ["applications", _, "commands"]) => {
                let commands = request
                    .body
                    .as_ref()
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                state.commands.clone_from(&commands);
                ok(Value::Array(commands))
            }
            (&Method::POST, ["interactions", ..])
            | (&Method::PATCH, ["webhooks", ..])
            | (&Method::POST, ["webhooks", ..]) => {
                state.interaction_replies.push((
                    request.path.clone(),
                    request.body.clone().unwrap_or(Value::Null),
                ));
                ok(json!({ "id": "1" }))
            }
            _ => error(404, 0, "404: Not Found"),
        }
    }

    fn history(state: &State, channel: u64, request: &ApiRequest) -> ApiResponse {
        if state.faults.denied_channels.contains(&channel) {
            return error(403, 50001, "Missing Access");
        }
        let before = query_value(request, "before")
            .and_then(|v| v.parse().ok())
            .unwrap_or(u64::MAX);
        let limit = query_value(request, "limit")
            .and_then(|v| v.parse().ok())
            .unwrap_or(50)
            .min(100);
        let page: Vec<Value> = state
            .messages
            .get(&channel)
            .into_iter()
            .flat_map(|messages| messages.range(..before).rev())
            .take(limit)
            .map(|(_, message)| to_json(message, false))
            .collect();
        ok(Value::Array(page))
    }

    fn search(
        state: &mut State,
        channel: Option<u64>,
        guild: Option<u64>,
        request: &ApiRequest,
    ) -> ApiResponse {
        state.searches_seen += 1;
        if state.searches_seen <= state.faults.index_pending {
            return ApiResponse {
                status: 202,
                headers: HashMap::new(),
                body: Bytes::from(
                    json!({ "code": 110_000, "retry_after": 1, "documents_indexed": 0 })
                        .to_string(),
                ),
            };
        }

        let values = |key: &str| -> Vec<&str> {
            request
                .query
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect()
        };
        let one = |key: &str| values(key).first().copied();
        let number = |key: &str| one(key).and_then(|v| v.parse::<u64>().ok());

        let scope: Vec<u64> = match (channel, guild) {
            (Some(channel), _) => vec![channel],
            (_, Some(guild)) => {
                let wanted: Vec<u64> = values("channel_id")
                    .iter()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                state
                    .guild_channels
                    .get(&guild)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["id"].as_str()?.parse().ok())
                    .filter(|id| wanted.is_empty() || wanted.contains(id))
                    .collect()
            }
            _ => Vec::new(),
        };

        let author = number("author_id");
        let min_id = number("min_id").unwrap_or(0);
        let max_id = number("max_id").unwrap_or(u64::MAX);
        let content = one("content").map(str::to_lowercase);
        let has = one("has");
        let pinned = one("pinned").map(|v| v == "true");

        let mut matches: Vec<FakeMessage> = Vec::new();
        for channel in scope {
            let live = state
                .messages
                .get(&channel)
                .into_iter()
                .flat_map(BTreeMap::values);
            let stale = state
                .faults
                .stale_search
                .then(|| state.deleted_for_search.get(&channel))
                .flatten();
            for message in live.chain(stale.into_iter().flat_map(BTreeMap::values)) {
                let matched = message.id > min_id
                    && message.id < max_id
                    && (state.faults.search_ignores_author
                        || author.is_none_or(|author| message.author_id == author))
                    && content
                        .as_deref()
                        .is_none_or(|text| message.content.to_lowercase().contains(text))
                    && pinned.is_none_or(|pinned| message.pinned == pinned)
                    && has.is_none_or(|kind| match kind {
                        "file" | "image" => !message.attachments.is_empty(),
                        "link" => message.content.contains("http"),
                        _ => false,
                    })
                    && !state.faults.hidden_from_search.contains(&message.id);
                if matched {
                    matches.push(message.clone());
                }
            }
        }
        matches.sort_by_key(|message| std::cmp::Reverse(message.id));
        let total = matches.len();

        if state.faults.empty_search_pages > 0 {
            state.faults.empty_search_pages -= 1;
            return ok(json!({ "total_results": total, "messages": [] }));
        }

        let offset = number("offset").unwrap_or(0) as usize;
        let limit = number("limit").unwrap_or(25).min(25) as usize;
        let hits: Vec<Value> = matches
            .iter()
            .skip(offset)
            .take(limit)
            .map(|message| json!([to_json(message, true)]))
            .collect();
        ok(
            json!({ "total_results": total, "messages": hits, "doing_deep_historical_index": false }),
        )
    }

    fn delete(state: &mut State, channel: u64, message: u64) -> ApiResponse {
        state.deletes_seen += 1;
        if state.faults.denied_channels.contains(&channel) {
            return error(403, 50001, "Missing Access");
        }
        if let Some(every) = state.faults.rate_limit_every
            && state.deletes_seen.is_multiple_of(every)
        {
            let seconds = state.faults.rate_limit_seconds;
            return ApiResponse {
                    status: 429,
                    headers: [("x-ratelimit-scope".to_owned(), "user".to_owned())].into_iter().collect(),
                    body: Bytes::from(json!({ "message": "You are being rate limited.", "retry_after": seconds, "global": false }).to_string()),
                };
        }
        if let Some(code) = state.faults.refuse_delete.get(&message) {
            return error(400, *code, "Discord refused this delete");
        }
        let Some(removed) = state
            .messages
            .get_mut(&channel)
            .and_then(|messages| messages.remove(&message))
        else {
            return error(404, 10008, "Unknown Message");
        };
        state
            .deleted_for_search
            .entry(channel)
            .or_default()
            .insert(message, removed);
        empty(204)
    }
}

impl Default for FakeDiscord {
    fn default() -> Self {
        Self::new()
    }
}

fn query_value<'a>(request: &'a ApiRequest, key: &str) -> Option<&'a str> {
    request
        .query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn to_json(message: &FakeMessage, hit: bool) -> Value {
    let timestamp = chrono::DateTime::from_timestamp_millis(Snowflake(message.id).timestamp_ms())
        .map(|moment| moment.to_rfc3339())
        .unwrap_or_default();
    let mut value = json!({
        "id": message.id.to_string(),
        "channel_id": message.channel.to_string(),
        "author": message.author,
        "type": message.kind,
        "content": message.content,
        "timestamp": timestamp,
        "pinned": message.pinned,
        "attachments": message.attachments,
    });
    if hit {
        value["hit"] = json!(true);
    }
    value
}

fn ok(body: Value) -> ApiResponse {
    ApiResponse {
        status: 200,
        headers: HashMap::new(),
        body: Bytes::from(body.to_string()),
    }
}

fn empty(status: u16) -> ApiResponse {
    ApiResponse {
        status,
        headers: HashMap::new(),
        body: Bytes::new(),
    }
}

fn error(status: u16, code: u32, message: &str) -> ApiResponse {
    ApiResponse {
        status,
        headers: HashMap::new(),
        body: Bytes::from(json!({ "code": code, "message": message }).to_string()),
    }
}

#[async_trait]
impl Transport for FakeDiscord {
    async fn execute(
        &self,
        request: &ApiRequest,
        authorization: &str,
    ) -> Result<ApiResponse, TransportError> {
        let latency = self.state.lock().faults.latency;
        if !latency.is_zero() {
            tokio::time::sleep(latency).await;
        }
        let mut state = self.state.lock();
        state.requests.push(request.clone());
        if state.faults.reject_token || !self.tokens.iter().any(|token| token == authorization) {
            return Ok(error(401, 0, "401: Unauthorized"));
        }
        Ok(Self::respond(&mut state, request))
    }

    async fn download(&self, _url: &str, _max_bytes: u64) -> Result<Bytes, TransportError> {
        Ok(Bytes::from_static(b"simulated attachment"))
    }

    fn is_simulated(&self) -> bool {
        true
    }
}
