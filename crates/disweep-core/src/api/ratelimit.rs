use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::time::{Instant, sleep_until};

use super::transport::ApiResponse;
use crate::error::{ApiError, ApiResult};

const INVALID_REQUEST_BUDGET: usize = 2_000;
const INVALID_REQUEST_WINDOW: Duration = Duration::from_secs(600);

const FALLBACK_RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Throttle {
    pub wait: Duration,
    pub global: bool,

    pub shared: bool,
}

#[derive(Default)]
struct Window {
    remaining: Option<u32>,
    resets_at: Option<Instant>,
}

pub(super) struct BucketPermit {
    window: OwnedMutexGuard<Window>,
    route: String,
    major: String,
}

#[derive(Default)]
pub struct RateLimiter {
    global_until: Mutex<Option<Instant>>,
    bucket_by_route: Mutex<HashMap<String, String>>,
    buckets: Mutex<HashMap<String, Arc<AsyncMutex<Window>>>>,
    invalid_requests: Mutex<VecDeque<Instant>>,
}

#[derive(Deserialize)]
struct RateLimitBody {
    #[serde(default)]
    retry_after: Option<f64>,
    #[serde(default)]
    global: bool,
}

impl RateLimiter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) async fn acquire(&self, method: &str, path: &str) -> ApiResult<BucketPermit> {
        self.ensure_within_budget()?;
        let (route, major) = route_key(method, path);
        let bucket = self.bucket(&route, &major);
        let mut window = bucket.lock_owned().await;

        loop {
            let global = *self.global_until.lock();
            let local = match (window.remaining, window.resets_at) {
                (Some(0), Some(resets_at)) => Some(resets_at),
                _ => None,
            };
            let Some(until) = global
                .into_iter()
                .chain(local)
                .max()
                .filter(|until| *until > Instant::now())
            else {
                break;
            };
            sleep_until(until).await;
        }

        if window
            .resets_at
            .is_some_and(|resets_at| resets_at <= Instant::now())
        {
            *window = Window::default();
        }
        Ok(BucketPermit {
            window,
            route,
            major,
        })
    }

    pub(super) fn observe(
        &self,
        mut permit: BucketPermit,
        response: &ApiResponse,
    ) -> Option<Throttle> {
        let now = Instant::now();

        if let Some(remaining) = header_number(response, "x-ratelimit-remaining") {
            permit.window.remaining = Some(remaining as u32);
        }
        if let Some(reset_after) = header_number(response, "x-ratelimit-reset-after") {
            permit.window.resets_at = Some(now + duration_from_seconds(reset_after));
        }
        if let Some(hash) = response.header("x-ratelimit-bucket") {
            self.adopt_bucket(&permit, hash);
        }

        if matches!(response.status, 401 | 403) {
            self.record_invalid(now);
        }
        if response.status != 429 {
            return None;
        }

        let body: Option<RateLimitBody> = serde_json::from_slice(&response.body).ok();
        let retry_after = body
            .as_ref()
            .and_then(|body| body.retry_after)
            .or_else(|| header_number(response, "retry-after"))
            .map_or(FALLBACK_RETRY, duration_from_seconds);
        let global =
            body.is_some_and(|body| body.global) || response.header("x-ratelimit-global").is_some();
        let shared = response.header("x-ratelimit-scope") == Some("shared");

        if global {
            *self.global_until.lock() = Some(now + retry_after);
        } else {
            permit.window.remaining = Some(0);
            permit.window.resets_at = Some(now + retry_after);
        }
        if !shared {
            self.record_invalid(now);
        }
        Some(Throttle {
            wait: retry_after,
            global,
            shared,
        })
    }

    fn adopt_bucket(&self, permit: &BucketPermit, hash: &str) {
        let bucket_id = format!("{hash}:{}", permit.major);
        let held = OwnedMutexGuard::mutex(&permit.window);
        let registered = self
            .buckets
            .lock()
            .entry(bucket_id)
            .or_insert_with(|| held.clone())
            .clone();

        if !Arc::ptr_eq(&registered, held)
            && let Ok(mut shared) = registered.try_lock()
        {
            shared.remaining = permit.window.remaining;
            shared.resets_at = permit.window.resets_at;
        }

        self.bucket_by_route
            .lock()
            .insert(permit.route.clone(), hash.to_owned());
    }

    fn bucket(&self, route: &str, major: &str) -> Arc<AsyncMutex<Window>> {
        let hash = self.bucket_by_route.lock().get(route).cloned();
        let bucket_id = hash.map_or_else(
            || format!("route:{route}:{major}"),
            |hash| format!("{hash}:{major}"),
        );
        self.buckets.lock().entry(bucket_id).or_default().clone()
    }

    fn record_invalid(&self, now: Instant) {
        let mut invalid = self.invalid_requests.lock();
        invalid.push_back(now);
        Self::forget_expired(&mut invalid, now);
    }

    fn ensure_within_budget(&self) -> ApiResult<()> {
        let mut invalid = self.invalid_requests.lock();
        Self::forget_expired(&mut invalid, Instant::now());
        if invalid.len() >= INVALID_REQUEST_BUDGET {
            return Err(ApiError::InvalidRequestBudget);
        }
        Ok(())
    }

    fn forget_expired(invalid: &mut VecDeque<Instant>, now: Instant) {
        while invalid
            .front()
            .is_some_and(|at| now.duration_since(*at) >= INVALID_REQUEST_WINDOW)
        {
            invalid.pop_front();
        }
    }
}

fn header_number(response: &ApiResponse, name: &str) -> Option<f64> {
    response
        .header(name)?
        .trim()
        .parse()
        .ok()
        .filter(|value: &f64| value.is_finite() && *value >= 0.0)
}

fn duration_from_seconds(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds.clamp(0.0, 3_600.0))
}

fn route_key(method: &str, path: &str) -> (String, String) {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let is_major = matches!(
        segments.first(),
        Some(&"channels" | &"guilds" | &"webhooks")
    );
    let major = if is_major {
        segments.get(1).copied().unwrap_or_default()
    } else {
        ""
    }
    .to_owned();

    let mut route = String::from(method);
    for (index, segment) in segments.iter().enumerate() {
        route.push('/');

        let is_id = segment.bytes().all(|byte| byte.is_ascii_digit());
        let is_token =
            index == 2 && matches!(segments.first(), Some(&"webhooks" | &"interactions"));
        if is_id || is_token {
            route.push_str(":id")
        } else {
            route.push_str(segment)
        }
    }
    (route, major)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn response(status: u16, headers: &[(&str, &str)], body: &str) -> ApiResponse {
        ApiResponse {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            body: Bytes::copy_from_slice(body.as_bytes()),
        }
    }

    #[test]
    fn route_keys_group_like_discord_buckets() {
        let (route, major) = route_key("DELETE", "/channels/123/messages/456");
        assert_eq!(route, "DELETE/channels/:id/messages/:id");
        assert_eq!(major, "123");
        assert_eq!(route_key("GET", "/users/@me/guilds").1, "");
    }

    #[test]
    fn tokens_never_enter_route_keys() {
        for path in [
            "/webhooks/1/sEcReT-ToKeN/messages/@original",
            "/interactions/1/sEcReT-ToKeN/callback",
        ] {
            assert!(!route_key("POST", path).0.contains("sEcReT"));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn waits_for_the_window_when_a_bucket_is_empty() {
        let limiter = RateLimiter::new();
        let permit = limiter
            .acquire("GET", "/channels/1/messages")
            .await
            .unwrap();
        limiter.observe(
            permit,
            &response(
                200,
                &[
                    ("x-ratelimit-bucket", "b"),
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset-after", "2.5"),
                ],
                "",
            ),
        );

        let started = Instant::now();
        let _permit = limiter
            .acquire("GET", "/channels/1/messages")
            .await
            .unwrap();
        assert_eq!(started.elapsed(), Duration::from_millis(2500));
    }

    #[tokio::test(start_paused = true)]
    async fn buckets_are_separate_per_channel() {
        let limiter = RateLimiter::new();
        let permit = limiter
            .acquire("GET", "/channels/1/messages")
            .await
            .unwrap();
        limiter.observe(
            permit,
            &response(
                200,
                &[
                    ("x-ratelimit-bucket", "b"),
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset-after", "60"),
                ],
                "",
            ),
        );

        let started = Instant::now();
        let _permit = limiter
            .acquire("GET", "/channels/2/messages")
            .await
            .unwrap();
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn the_same_channel_keeps_its_limit_after_the_bucket_is_named() {
        let limiter = RateLimiter::new();
        let headers = [
            ("x-ratelimit-bucket", "b"),
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset-after", "5"),
        ];
        let permit = limiter
            .acquire("DELETE", "/channels/1/messages/10")
            .await
            .unwrap();
        limiter.observe(permit, &response(204, &headers, ""));

        let started = Instant::now();
        let _permit = limiter
            .acquire("DELETE", "/channels/1/messages/11")
            .await
            .unwrap();
        assert_eq!(started.elapsed(), Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn a_global_limit_blocks_every_route() {
        let limiter = RateLimiter::new();
        let permit = limiter
            .acquire("GET", "/channels/1/messages")
            .await
            .unwrap();
        let throttle = limiter
            .observe(
                permit,
                &response(429, &[], r#"{"retry_after": 4.0, "global": true}"#),
            )
            .unwrap();
        assert!(throttle.global);

        let started = Instant::now();
        let _permit = limiter.acquire("GET", "/guilds/9/channels").await.unwrap();
        assert_eq!(started.elapsed(), Duration::from_secs(4));
    }

    #[tokio::test(start_paused = true)]
    async fn a_429_without_json_still_backs_off() {
        let limiter = RateLimiter::new();
        let permit = limiter.acquire("GET", "/x").await.unwrap();
        let throttle = limiter
            .observe(permit, &response(429, &[], "<html>cloudflare</html>"))
            .unwrap();
        assert_eq!(throttle.wait, FALLBACK_RETRY);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_before_the_invalid_request_ban() {
        let limiter = RateLimiter::new();
        for _ in 0..INVALID_REQUEST_BUDGET {
            let permit = limiter.acquire("GET", "/x").await.unwrap();
            limiter.observe(permit, &response(403, &[], "{}"));
        }
        assert!(matches!(
            limiter.acquire("GET", "/x").await,
            Err(ApiError::InvalidRequestBudget)
        ));

        tokio::time::advance(INVALID_REQUEST_WINDOW).await;
        assert!(limiter.acquire("GET", "/x").await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn shared_429s_do_not_count_against_the_budget() {
        let limiter = RateLimiter::new();
        for _ in 0..(INVALID_REQUEST_BUDGET + 5) {
            let permit = limiter.acquire("GET", "/x").await.unwrap();
            let throttle = limiter.observe(
                permit,
                &response(
                    429,
                    &[("x-ratelimit-scope", "shared")],
                    r#"{"retry_after": 0.001}"#,
                ),
            );
            assert!(throttle.unwrap().shared);
        }
        assert!(limiter.acquire("GET", "/x").await.is_ok());
    }
}
