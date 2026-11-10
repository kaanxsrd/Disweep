//! Typed Discord endpoints on top of the transport and the rate limiter.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::time::sleep;

use super::models::{Application, Channel, Guild, Message, User};
use super::ratelimit::{RateLimiter, Throttle};
use super::transport::{ApiRequest, ApiResponse, Method, Transport};
use crate::error::{ApiError, ApiResult};
use crate::{Secret, Snowflake};

const MAX_TRANSIENT_RETRIES: u32 = 5;
const MAX_CONSECUTIVE_THROTTLES: u32 = 16;
const SEARCH_PAGE_SIZE: u8 = 25;

/// How the token is presented to Discord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthScheme {
    /// A person's session token, sent as is.
    User,
    /// An application's bot token.
    Bot,
}

/// Told about every wait Discord imposes, so the interface can explain a pause.
pub type Observer = Arc<dyn Fn(Throttle) + Send + Sync>;

/// A handle to Discord for one login. Clones share rate-limit state.
#[derive(Clone)]
pub struct Discord {
    inner: Arc<Inner>,
}

struct Inner {
    transport: Arc<dyn Transport>,
    authorization: Secret,
    limiter: RateLimiter,
    observer: RwLock<Option<Observer>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
    Guild(Snowflake),
    /// A DM or group DM, which have no guild to search in.
    Channel(Snowflake),
}

#[derive(Clone, Debug, Default)]
pub struct SearchQuery {
    pub author_id: Option<Snowflake>,
    pub channel_ids: Vec<Snowflake>,
    pub min_id: Option<Snowflake>,
    pub max_id: Option<Snowflake>,
    pub content: Option<String>,
    pub has: Vec<String>,
    pub pinned: Option<bool>,
    pub include_nsfw: bool,
}

#[derive(Clone, Debug)]
pub struct SearchPage {
    pub total_results: u64,
    pub hits: Vec<Message>,
    pub deep_indexing: bool,
}

#[derive(Clone, Debug)]
pub enum SearchOutcome {
    Ready(SearchPage),
    /// Discord answered 202: it is still building the index for this scope.
    NotIndexed {
        retry_after: Duration,
    },
}

/// What happened to one delete request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    /// Someone or something removed it first, which is what we wanted anyway.
    AlreadyGone,
    /// This message can't be deleted, but others in the channel still might be.
    Refused {
        code: Option<u32>,
        reason: String,
    },
    /// The channel itself is off limits, so nothing else in it can be deleted either.
    ChannelDenied {
        code: Option<u32>,
        reason: String,
    },
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: Option<u32>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct RawSearch {
    #[serde(default)]
    total_results: u64,
    #[serde(default)]
    messages: Vec<RawSearchEntry>,
    #[serde(default)]
    doing_deep_historical_index: bool,
}

/// Discord documents nested arrays but says the nesting is vestigial, so accept either.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawSearchEntry {
    Group(Vec<Message>),
    Single(Box<Message>),
}

#[derive(Deserialize)]
struct NotIndexedBody {
    #[serde(default)]
    retry_after: Option<f64>,
}

impl Discord {
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>, token: &Secret, scheme: AuthScheme) -> Self {
        let authorization = match scheme {
            AuthScheme::User => token.clone(),
            AuthScheme::Bot => Secret::new(format!("Bot {}", token.expose())),
        };
        Self {
            inner: Arc::new(Inner {
                transport,
                authorization,
                limiter: RateLimiter::new(),
                observer: RwLock::new(None),
            }),
        }
    }

    /// Sets the callback that hears about rate-limit waits.
    pub fn set_observer(&self, observer: Observer) {
        *self.inner.observer.write() = Some(observer);
    }

    #[must_use]
    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.inner.transport
    }

    /// Sends a request, absorbing rate limits and brief outages.
    async fn send(&self, request: ApiRequest) -> ApiResult<ApiResponse> {
        let inner = &self.inner;
        let method = request.method.as_str().to_owned();
        let mut transient_failures = 0;
        let mut throttles = 0;

        loop {
            let permit = inner.limiter.acquire(&method, &request.path).await?;
            let response = match inner
                .transport
                .execute(&request, inner.authorization.expose())
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    drop(permit);
                    transient_failures += 1;
                    if transient_failures > MAX_TRANSIENT_RETRIES {
                        return Err(ApiError::Network(error.0));
                    }
                    sleep(backoff(transient_failures)).await;
                    continue;
                }
            };

            if let Some(throttle) = inner.limiter.observe(permit, &response) {
                throttles += 1;
                if throttles > MAX_CONSECUTIVE_THROTTLES {
                    return Err(ApiError::Throttled);
                }
                if let Some(observer) = inner.observer.read().as_ref() {
                    observer(throttle);
                }
                // The next acquire sleeps for exactly as long as Discord asked.
                continue;
            }

            match response.status {
                401 => return Err(ApiError::Unauthorized),
                500..=599 => {
                    transient_failures += 1;
                    if transient_failures > MAX_TRANSIENT_RETRIES {
                        return Err(failure(&response));
                    }
                    sleep(backoff(transient_failures)).await;
                }
                _ => return Ok(response),
            }
        }
    }

    async fn call<T: DeserializeOwned>(&self, request: ApiRequest) -> ApiResult<T> {
        let response = self.send(request).await?;
        if !(200..300).contains(&response.status) {
            return Err(failure(&response));
        }
        decode(&response)
    }

    async fn call_empty(&self, request: ApiRequest) -> ApiResult<()> {
        let response = self.send(request).await?;
        if !(200..300).contains(&response.status) {
            return Err(failure(&response));
        }
        Ok(())
    }

    pub async fn current_user(&self) -> ApiResult<User> {
        self.call(ApiRequest::new(Method::GET, "/users/@me")).await
    }

    /// Every server the account is in.
    pub async fn guilds(&self) -> ApiResult<Vec<Guild>> {
        let mut guilds = Vec::new();
        let mut after: Option<Snowflake> = None;
        loop {
            let mut request = ApiRequest::new(Method::GET, "/users/@me/guilds").query("limit", 200);
            if let Some(after) = after {
                request = request.query("after", after);
            }
            let page: Vec<Guild> = self.call(request).await?;
            let finished = page.len() < 200;
            after = page.iter().map(|guild| guild.id).max();
            guilds.extend(page);
            if finished {
                return Ok(guilds);
            }
        }
    }

    pub async fn dm_channels(&self) -> ApiResult<Vec<Channel>> {
        self.call(ApiRequest::new(Method::GET, "/users/@me/channels"))
            .await
    }

    pub async fn guild_channels(&self, guild: Snowflake) -> ApiResult<Vec<Channel>> {
        self.call(ApiRequest::new(
            Method::GET,
            format!("/guilds/{guild}/channels"),
        ))
        .await
    }

    pub async fn channel(&self, channel: Snowflake) -> ApiResult<Channel> {
        self.call(ApiRequest::new(Method::GET, format!("/channels/{channel}")))
            .await
    }

    /// Finds or creates the DM with a person.
    pub async fn open_dm(&self, recipient: Snowflake) -> ApiResult<Channel> {
        self.call(
            ApiRequest::new(Method::POST, "/users/@me/channels")
                .json(json!({ "recipient_id": recipient })),
        )
        .await
    }

    /// One page of search results, newest first.
    pub async fn search(
        &self,
        scope: SearchScope,
        query: &SearchQuery,
    ) -> ApiResult<SearchOutcome> {
        let path = match scope {
            SearchScope::Guild(guild) => format!("/guilds/{guild}/messages/search"),
            SearchScope::Channel(channel) => format!("/channels/{channel}/messages/search"),
        };
        let mut request = ApiRequest::new(Method::GET, path)
            .query("limit", SEARCH_PAGE_SIZE)
            .query("sort_by", "timestamp")
            .query("sort_order", "desc");
        if let Some(author) = query.author_id {
            request = request.query("author_id", author);
        }
        for channel in &query.channel_ids {
            request = request.query("channel_id", channel);
        }
        if let Some(min_id) = query.min_id {
            request = request.query("min_id", min_id);
        }
        if let Some(max_id) = query.max_id {
            request = request.query("max_id", max_id);
        }
        if let Some(content) = &query.content {
            request = request.query("content", content);
        }
        for has in &query.has {
            request = request.query("has", has);
        }
        if let Some(pinned) = query.pinned {
            request = request.query("pinned", pinned);
        }
        if query.include_nsfw {
            request = request.query("include_nsfw", true);
        }

        let response = self.send(request).await?;
        if response.status == 202 {
            let body: Option<NotIndexedBody> = serde_json::from_slice(&response.body).ok();
            let seconds = body
                .and_then(|body| body.retry_after)
                .filter(|seconds| *seconds > 0.0)
                .unwrap_or(2.0);
            return Ok(SearchOutcome::NotIndexed {
                retry_after: Duration::from_secs_f64(seconds.min(120.0)),
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(failure(&response));
        }

        let raw: RawSearch = decode(&response)?;
        let hits = raw
            .messages
            .into_iter()
            .filter_map(|entry| match entry {
                RawSearchEntry::Single(message) => Some(*message),
                RawSearchEntry::Group(mut group) => {
                    match group.iter().position(|message| message.hit) {
                        Some(index) => Some(group.swap_remove(index)),
                        None => group.into_iter().next(),
                    }
                }
            })
            .collect();
        Ok(SearchOutcome::Ready(SearchPage {
            total_results: raw.total_results,
            hits,
            deep_indexing: raw.doing_deep_historical_index,
        }))
    }

    /// Channel history, newest first.
    pub async fn messages(
        &self,
        channel: Snowflake,
        before: Option<Snowflake>,
        limit: u8,
    ) -> ApiResult<Vec<Message>> {
        let mut request = ApiRequest::new(Method::GET, format!("/channels/{channel}/messages"))
            .query("limit", limit.clamp(1, 100));
        if let Some(before) = before {
            request = request.query("before", before);
        }
        self.call(request).await
    }

    /// Deletes one message and classifies how that went.
    pub async fn delete_message(
        &self,
        channel: Snowflake,
        message: Snowflake,
    ) -> ApiResult<DeleteOutcome> {
        let response = self
            .send(ApiRequest::new(
                Method::DELETE,
                format!("/channels/{channel}/messages/{message}"),
            ))
            .await?;
        if (200..300).contains(&response.status) {
            return Ok(DeleteOutcome::Deleted);
        }

        let body: Option<ErrorBody> = serde_json::from_slice(&response.body).ok();
        // Discord always answers in JSON. Anything else is its firewall turning this connection
        // away, and pressing on could turn a brief block into a ban.
        if response.status == 403 && body.is_none() {
            return Err(ApiError::Network(
                "Discord's firewall blocked this connection. Try again later.".to_owned(),
            ));
        }
        let code = body.as_ref().and_then(|body| body.code);
        let reason = body
            .and_then(|body| body.message)
            .unwrap_or_else(|| format!("HTTP {}", response.status));
        Ok(match code {
            Some(10008) => DeleteOutcome::AlreadyGone,
            Some(40002) => return Err(ApiError::VerificationRequired),
            // Unknown channel, missing access: asking again for this channel is pointless.
            Some(10003 | 50001) => DeleteOutcome::ChannelDenied { code, reason },
            _ if response.status == 404 => DeleteOutcome::AlreadyGone,
            _ => DeleteOutcome::Refused { code, reason },
        })
    }

    /// Deletes 2 to 100 messages at once. Bots only, and only for messages under two weeks old.
    pub async fn bulk_delete(
        &self,
        channel: Snowflake,
        messages: &[Snowflake],
        reason: &str,
    ) -> ApiResult<()> {
        self.call_empty(
            ApiRequest::new(
                Method::POST,
                format!("/channels/{channel}/messages/bulk-delete"),
            )
            .json(json!({ "messages": messages }))
            .audit_reason(reason),
        )
        .await
    }

    pub async fn delete_message_as_bot(
        &self,
        channel: Snowflake,
        message: Snowflake,
        reason: &str,
    ) -> ApiResult<()> {
        self.call_empty(
            ApiRequest::new(
                Method::DELETE,
                format!("/channels/{channel}/messages/{message}"),
            )
            .audit_reason(reason),
        )
        .await
    }

    pub async fn send_message(&self, channel: Snowflake, body: Value) -> ApiResult<()> {
        self.call_empty(
            ApiRequest::new(Method::POST, format!("/channels/{channel}/messages")).json(body),
        )
        .await
    }

    pub async fn application(&self) -> ApiResult<Application> {
        self.call(ApiRequest::new(Method::GET, "/applications/@me"))
            .await
    }

    pub async fn patch_application(&self, patch: Value) -> ApiResult<Application> {
        self.call(ApiRequest::new(Method::PATCH, "/applications/@me").json(patch))
            .await
    }

    pub async fn global_commands(&self, application: Snowflake) -> ApiResult<Vec<Value>> {
        self.call(ApiRequest::new(
            Method::GET,
            format!("/applications/{application}/commands"),
        ))
        .await
    }

    /// Replaces every global command on the application with this list.
    pub async fn overwrite_global_commands(
        &self,
        application: Snowflake,
        commands: Value,
    ) -> ApiResult<Vec<Value>> {
        self.call(
            ApiRequest::new(Method::PUT, format!("/applications/{application}/commands"))
                .json(commands),
        )
        .await
    }

    pub async fn interaction_callback(
        &self,
        interaction: Snowflake,
        token: &str,
        body: Value,
    ) -> ApiResult<()> {
        self.call_empty(
            ApiRequest::new(
                Method::POST,
                format!("/interactions/{interaction}/{token}/callback"),
            )
            .json(body),
        )
        .await
    }

    pub async fn edit_original_response(
        &self,
        application: Snowflake,
        token: &str,
        body: Value,
    ) -> ApiResult<()> {
        self.call_empty(
            ApiRequest::new(
                Method::PATCH,
                format!("/webhooks/{application}/{token}/messages/@original"),
            )
            .json(body),
        )
        .await
    }
}

fn backoff(failures: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << failures.min(6)))
}

fn decode<T: DeserializeOwned>(response: &ApiResponse) -> ApiResult<T> {
    // Serde's message can quote the body it failed on, so only the position is kept.
    serde_json::from_slice(&response.body).map_err(|error| {
        ApiError::Decode(format!("line {}, column {}", error.line(), error.column()))
    })
}

fn failure(response: &ApiResponse) -> ApiError {
    let body: Option<ErrorBody> = serde_json::from_slice(&response.body).ok();
    let code = body.as_ref().and_then(|body| body.code);
    if code == Some(40002) {
        return ApiError::VerificationRequired;
    }
    let message = body
        .and_then(|body| body.message)
        .unwrap_or_else(|| "Discord refused the request".to_owned());
    ApiError::Discord {
        status: response.status,
        code,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::transport::TransportError;
    use async_trait::async_trait;
    use bytes::Bytes;
    use parking_lot::Mutex;
    use std::collections::{HashMap, VecDeque};

    /// Answers with a script of responses and remembers what it was asked.
    #[derive(Default)]
    struct Scripted {
        script: Mutex<VecDeque<Result<ApiResponse, TransportError>>>,
        seen: Mutex<Vec<ApiRequest>>,
        authorizations: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(script: Vec<Result<ApiResponse, TransportError>>) -> Arc<Self> {
            Arc::new(Self {
                script: Mutex::new(script.into()),
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl Transport for Scripted {
        async fn execute(
            &self,
            request: &ApiRequest,
            authorization: &str,
        ) -> Result<ApiResponse, TransportError> {
            self.seen.lock().push(request.clone());
            self.authorizations.lock().push(authorization.to_owned());
            self.script
                .lock()
                .pop_front()
                .expect("the client sent more requests than expected")
        }

        async fn download(&self, _url: &str, _max_bytes: u64) -> Result<Bytes, TransportError> {
            Ok(Bytes::new())
        }
    }

    fn reply(status: u16, body: &str) -> Result<ApiResponse, TransportError> {
        Ok(ApiResponse {
            status,
            headers: HashMap::new(),
            body: Bytes::copy_from_slice(body.as_bytes()),
        })
    }

    fn client(script: Vec<Result<ApiResponse, TransportError>>) -> (Discord, Arc<Scripted>) {
        let transport = Scripted::new(script);
        (
            Discord::new(
                transport.clone(),
                &Secret::new("tok".to_owned()),
                AuthScheme::User,
            ),
            transport,
        )
    }

    const USER: &str = r#"{"id":"1","username":"kit"}"#;

    fn hit(id: u64) -> Value {
        json!({"id": id.to_string(), "channel_id": "9", "author": {"id": "1", "username": "kit"}, "content": "x", "hit": true})
    }

    #[tokio::test]
    async fn user_tokens_have_no_prefix_and_bot_tokens_do() {
        let transport = Scripted::new(vec![reply(200, USER), reply(200, USER)]);
        let token = Secret::new("tok".to_owned());
        Discord::new(transport.clone(), &token, AuthScheme::User)
            .current_user()
            .await
            .unwrap();
        Discord::new(transport.clone(), &token, AuthScheme::Bot)
            .current_user()
            .await
            .unwrap();
        assert_eq!(
            *transport.authorizations.lock(),
            vec!["tok".to_owned(), "Bot tok".to_owned()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retries_after_a_429_and_tells_the_observer() {
        let (discord, transport) = client(vec![
            reply(429, r#"{"retry_after": 3.0, "global": false}"#),
            reply(200, USER),
        ]);
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = heard.clone();
        discord.set_observer(Arc::new(move |throttle| sink.lock().push(throttle.wait)));

        let started = tokio::time::Instant::now();
        discord.current_user().await.unwrap();

        assert_eq!(started.elapsed(), Duration::from_secs(3));
        assert_eq!(transport.seen.lock().len(), 2);
        assert_eq!(*heard.lock(), vec![Duration::from_secs(3)]);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_outages_then_gives_up() {
        let recovering = vec![
            reply(502, ""),
            Err(TransportError("reset".to_owned())),
            reply(200, USER),
        ];
        client(recovering).0.current_user().await.unwrap();

        let down = (0..=MAX_TRANSIENT_RETRIES)
            .map(|_| reply(503, ""))
            .collect();
        assert!(matches!(
            client(down).0.current_user().await,
            Err(ApiError::Discord { status: 503, .. })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_too_many_consecutive_429s() {
        let script = (0..=MAX_CONSECUTIVE_THROTTLES)
            .map(|_| reply(429, r#"{"retry_after": 1}"#))
            .collect();
        assert!(matches!(
            client(script).0.current_user().await,
            Err(ApiError::Throttled)
        ));
    }

    #[tokio::test]
    async fn a_401_is_never_retried() {
        let (discord, transport) = client(vec![reply(
            401,
            r#"{"message":"401: Unauthorized","code":0}"#,
        )]);
        assert!(matches!(
            discord.current_user().await,
            Err(ApiError::Unauthorized)
        ));
        assert_eq!(transport.seen.lock().len(), 1);
    }

    #[tokio::test]
    async fn classifies_delete_outcomes() {
        let cases = [
            (204, "", DeleteOutcome::Deleted),
            (
                404,
                r#"{"code":10008,"message":"Unknown Message"}"#,
                DeleteOutcome::AlreadyGone,
            ),
            (404, "", DeleteOutcome::AlreadyGone),
            (
                403,
                r#"{"code":50001,"message":"Missing Access"}"#,
                DeleteOutcome::ChannelDenied {
                    code: Some(50001),
                    reason: "Missing Access".to_owned(),
                },
            ),
            (
                404,
                r#"{"code":10003,"message":"Unknown Channel"}"#,
                DeleteOutcome::ChannelDenied {
                    code: Some(10003),
                    reason: "Unknown Channel".to_owned(),
                },
            ),
            (
                400,
                r#"{"code":50083,"message":"Thread is archived"}"#,
                DeleteOutcome::Refused {
                    code: Some(50083),
                    reason: "Thread is archived".to_owned(),
                },
            ),
            (
                403,
                r#"{"code":50021,"message":"Cannot execute action on a system message"}"#,
                DeleteOutcome::Refused {
                    code: Some(50021),
                    reason: "Cannot execute action on a system message".to_owned(),
                },
            ),
        ];
        for (status, body, expected) in cases {
            let (discord, _) = client(vec![reply(status, body)]);
            assert_eq!(
                discord
                    .delete_message(Snowflake(1), Snowflake(2))
                    .await
                    .unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn a_firewall_block_stops_a_delete_instead_of_counting_as_refused() {
        let (discord, _) = client(vec![reply(403, "<html>Access denied</html>")]);
        assert!(matches!(
            discord.delete_message(Snowflake(1), Snowflake(2)).await,
            Err(ApiError::Network(_))
        ));
    }

    #[tokio::test]
    async fn verification_stops_a_delete() {
        let (discord, _) = client(vec![reply(403, r#"{"code":40002,"message":"verify"}"#)]);
        assert!(matches!(
            discord.delete_message(Snowflake(1), Snowflake(2)).await,
            Err(ApiError::VerificationRequired)
        ));
    }

    #[tokio::test]
    async fn search_reads_nested_and_flat_results_and_finds_the_hit() {
        let nested = json!({"total_results": 2, "messages": [[{"id":"7","channel_id":"9","author":{"id":"2","username":"o"}}, hit(8)]]});
        let flat = json!({"total_results": 1, "messages": [hit(5)]});
        for (body, expected) in [(nested, 8), (flat, 5)] {
            let (discord, _) = client(vec![reply(200, &body.to_string())]);
            let SearchOutcome::Ready(page) = discord
                .search(SearchScope::Channel(Snowflake(9)), &SearchQuery::default())
                .await
                .unwrap()
            else {
                panic!("expected results");
            };
            assert_eq!(page.hits[0].id, Snowflake(expected));
        }
    }

    #[tokio::test]
    async fn search_reports_an_index_still_building() {
        let (discord, _) = client(vec![reply(
            202,
            r#"{"code":110000,"retry_after":0,"documents_indexed":0}"#,
        )]);
        let outcome = discord
            .search(SearchScope::Guild(Snowflake(1)), &SearchQuery::default())
            .await
            .unwrap();
        // A retry_after of 0 means "shortly", per the docs.
        assert!(
            matches!(outcome, SearchOutcome::NotIndexed { retry_after } if retry_after == Duration::from_secs(2))
        );
    }

    #[tokio::test]
    async fn search_uses_repeated_keys_for_lists() {
        let (discord, transport) = client(vec![reply(200, r#"{"total_results":0,"messages":[]}"#)]);
        let query = SearchQuery {
            author_id: Some(Snowflake(1)),
            channel_ids: vec![Snowflake(2), Snowflake(3)],
            max_id: Some(Snowflake(9)),
            pinned: Some(false),
            ..Default::default()
        };
        discord
            .search(SearchScope::Guild(Snowflake(4)), &query)
            .await
            .unwrap();
        let seen = transport.seen.lock();
        assert_eq!(seen[0].path, "/guilds/4/messages/search");
        let channels: Vec<_> = seen[0]
            .query
            .iter()
            .filter(|(key, _)| key == "channel_id")
            .collect();
        assert_eq!(channels.len(), 2);
        assert!(
            seen[0]
                .query
                .contains(&("max_id".to_owned(), "9".to_owned()))
        );
        assert!(
            seen[0]
                .query
                .contains(&("pinned".to_owned(), "false".to_owned()))
        );
    }

    #[tokio::test]
    async fn guild_listing_follows_pagination() {
        let full: Vec<Value> = (1..=200)
            .map(|id| json!({"id": id.to_string(), "name": "g"}))
            .collect();
        let (discord, transport) = client(vec![
            reply(200, &Value::Array(full).to_string()),
            reply(200, r#"[{"id":"201","name":"g"}]"#),
        ]);
        assert_eq!(discord.guilds().await.unwrap().len(), 201);
        assert!(
            transport.seen.lock()[1]
                .query
                .contains(&("after".to_owned(), "200".to_owned()))
        );
    }

    #[tokio::test]
    async fn decode_errors_do_not_echo_the_body() {
        let (discord, _) = client(vec![reply(200, r#"{"username": "SENSITIVE"}"#)]);
        let error = discord.current_user().await.unwrap_err();
        assert!(!format!("{error:?}").contains("SENSITIVE"));
    }
}
