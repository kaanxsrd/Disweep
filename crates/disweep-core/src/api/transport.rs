//! The single point where bytes leave the process.
//!
//! Everything above this trait is real production code in tests and in demo mode; only this layer
//! is swapped for the in-memory Discord in [`crate::demo`].

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
pub use reqwest::Method;
use serde_json::Value;

/// One Discord API call.
#[derive(Clone, Debug)]
pub struct ApiRequest {
    pub method: Method,
    /// Relative to the API base, such as `/channels/1/messages`.
    pub path: String,
    /// Repeated keys are how Discord takes array parameters.
    pub query: Vec<(String, String)>,
    pub body: Option<Value>,
    pub audit_reason: Option<String>,
}

impl ApiRequest {
    #[must_use]
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            body: None,
            audit_reason: None,
        }
    }

    #[must_use]
    pub fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_owned(), value.to_string()));
        self
    }

    #[must_use]
    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    #[must_use]
    pub fn audit_reason(mut self, reason: impl Into<String>) -> Self {
        self.audit_reason = Some(reason.into());
        self
    }
}

#[derive(Clone, Debug)]
pub struct ApiResponse {
    pub status: u16,
    /// Header names are lowercase.
    pub headers: HashMap<String, String>,
    pub body: Bytes,
}

impl ApiResponse {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// The request never completed.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Sends one API call. HTTP error statuses are normal responses, not `Err`.
    async fn execute(
        &self,
        request: &ApiRequest,
        authorization: &str,
    ) -> Result<ApiResponse, TransportError>;

    /// Downloads a public CDN file with no credentials attached.
    async fn download(&self, url: &str, max_bytes: u64) -> Result<Bytes, TransportError>;

    /// True for the in-memory Discord used by demo mode and tests.
    fn is_simulated(&self) -> bool {
        false
    }
}

/// The real network.
pub struct HttpTransport {
    api: reqwest::Client,
    cdn: reqwest::Client,
    base: String,
    user_agent: String,
    bot_user_agent: String,
}

impl HttpTransport {
    /// `bot_user_agent` is sent with bot tokens, where Discord requires a specific format.
    pub fn new(
        base: impl Into<String>,
        user_agent: &str,
        bot_user_agent: &str,
    ) -> Result<Self, TransportError> {
        let build = |timeout: u64| {
            reqwest::Client::builder()
                .user_agent(user_agent)
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(timeout))
                .build()
                .map_err(|error| TransportError(describe(&error)))
        };
        Ok(Self {
            api: build(45)?,
            cdn: build(600)?,
            base: base.into(),
            user_agent: user_agent.to_owned(),
            bot_user_agent: bot_user_agent.to_owned(),
        })
    }
}

/// Describes a failure without the URL, which can hold an interaction token.
fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the request timed out".to_owned()
    } else if error.is_connect() {
        "couldn't connect".to_owned()
    } else {
        reqwest_message(error)
    }
}

fn reqwest_message(error: &reqwest::Error) -> String {
    let mut text = String::from("connection problem");
    let without_url = format!("{}", NoUrl(error));
    if !without_url.is_empty() {
        text = without_url;
    }
    text
}

struct NoUrl<'a>(&'a reqwest::Error);

impl std::fmt::Display for NoUrl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut source = std::error::Error::source(self.0);
        let mut deepest = None;
        while let Some(error) = source {
            deepest = Some(error);
            source = error.source();
        }
        match deepest {
            Some(error) => write!(f, "{error}"),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn execute(
        &self,
        request: &ApiRequest,
        authorization: &str,
    ) -> Result<ApiResponse, TransportError> {
        let url = format!("{}{}", self.base, request.path);
        let user_agent = if authorization.starts_with("Bot ") {
            &self.bot_user_agent
        } else {
            &self.user_agent
        };
        let mut outgoing = self
            .api
            .request(request.method.clone(), url)
            .header("Authorization", authorization)
            .header("User-Agent", user_agent);
        if !request.query.is_empty() {
            let mut parsed = outgoing
                .build()
                .map_err(|error| TransportError(describe(&error)))?;
            parsed
                .url_mut()
                .query_pairs_mut()
                .extend_pairs(request.query.iter());
            outgoing = reqwest::RequestBuilder::from_parts(self.api.clone(), parsed);
        }
        if let Some(body) = &request.body {
            outgoing = outgoing
                .header("Content-Type", "application/json")
                .body(serde_json::to_vec(body).map_err(|error| TransportError(error.to_string()))?);
        }
        if let Some(reason) = &request.audit_reason {
            outgoing = outgoing.header("X-Audit-Log-Reason", encode_audit_reason(reason));
        }
        let response = outgoing
            .send()
            .await
            .map_err(|error| TransportError(describe(&error)))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        let body = response
            .bytes()
            .await
            .map_err(|error| TransportError(describe(&error)))?;
        Ok(ApiResponse {
            status,
            headers,
            body,
        })
    }

    async fn download(&self, url: &str, max_bytes: u64) -> Result<Bytes, TransportError> {
        let response = self
            .cdn
            .get(url)
            .send()
            .await
            .map_err(|error| TransportError(describe(&error)))?;
        if !response.status().is_success() {
            return Err(TransportError(format!(
                "the CDN answered HTTP {}",
                response.status().as_u16()
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(TransportError(
                "the file is too large to archive".to_owned(),
            ));
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| TransportError(describe(&error)))?;
        if body.len() as u64 > max_bytes {
            return Err(TransportError(
                "the file is too large to archive".to_owned(),
            ));
        }
        Ok(body)
    }
}

/// Audit log reasons travel in a header, so anything outside plain ASCII is percent-encoded.
fn encode_audit_reason(reason: &str) -> String {
    let mut encoded = String::with_capacity(reason.len());
    for byte in reason.bytes().take(512) {
        if byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, header_regex, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn transport(server: &MockServer) -> HttpTransport {
        crate::init_tls();
        HttpTransport::new(
            server.uri(),
            "DisweepTests/1.0",
            "DiscordBot (https://example.com, 1.0)",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn bot_tokens_get_the_user_agent_discord_requires() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header_regex(
                "user-agent",
                r"^DiscordBot \(https://example\.com, 1\.0\)$",
            ))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        transport(&server)
            .execute(&ApiRequest::new(Method::GET, "/x"), "Bot abc")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn sends_authorization_query_and_lowercases_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/channels/5/messages"))
            .and(query_param("limit", "100"))
            .and(header("authorization", "abc"))
            .and(header("user-agent", "DisweepTests/1.0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-RateLimit-Bucket", "b1")
                    .set_body_string("[]"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let response = transport(&server)
            .execute(
                &ApiRequest::new(Method::GET, "/channels/5/messages").query("limit", 100),
                "abc",
            )
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.header("x-ratelimit-bucket"), Some("b1"));
    }

    #[tokio::test]
    async fn error_statuses_are_responses_not_failures() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let response = transport(&server)
            .execute(
                &ApiRequest::new(Method::DELETE, "/channels/5/messages/6"),
                "abc",
            )
            .await
            .unwrap();

        assert_eq!(response.status, 403);
    }

    #[tokio::test]
    async fn repeated_query_keys_are_preserved() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        transport(&server)
            .execute(
                &ApiRequest::new(Method::GET, "/x")
                    .query("has", "link")
                    .query("has", "file"),
                "abc",
            )
            .await
            .unwrap();

        let received = server.received_requests().await.unwrap();
        assert_eq!(received[0].url.query(), Some("has=link&has=file"));
    }

    #[tokio::test]
    async fn network_errors_do_not_mention_the_url() {
        crate::init_tls();
        // A port that was just released refuses connections.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let dead = HttpTransport::new(
            format!("http://{address}"),
            "DisweepTests/1.0",
            "DisweepTests/1.0",
        )
        .unwrap();

        let error = dead
            .execute(
                &ApiRequest::new(Method::POST, "/interactions/1/SECRET_TOKEN/callback"),
                "abc",
            )
            .await
            .unwrap_err();

        assert!(!error.0.contains("SECRET_TOKEN"), "leaked: {}", error.0);
    }

    #[tokio::test]
    async fn refuses_a_download_over_the_size_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 2048]))
            .mount(&server)
            .await;

        assert!(
            transport(&server)
                .download(&format!("{}/f.png", server.uri()), 1024)
                .await
                .is_err()
        );
        assert_eq!(
            transport(&server)
                .download(&format!("{}/f.png", server.uri()), 4096)
                .await
                .unwrap()
                .len(),
            2048
        );
    }

    #[test]
    fn audit_reasons_are_header_safe() {
        assert_eq!(
            encode_audit_reason("purge by Ada: ünï\n"),
            "purge by Ada%3A %C3%BCn%C3%AF%0A"
        );
    }
}
