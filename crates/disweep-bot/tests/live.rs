//! Checks against Discord's real servers, using no account and a token that can't work.
//!
//! These prove the TLS setup works against the real thing, which a local server can't show.
//! They need a network, so they only run when asked for:
//!
//!     cargo test -p disweep-bot --test live -- --ignored

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use disweep_bot::{BotConnection, BotRuntime};
use disweep_core::Secret;
use disweep_core::api::{API_BASE, ApiRequest, HttpTransport, Method, Transport};
use disweep_core::demo::{DEMO_TOKEN, FakeDiscord};
use disweep_core::engine::Engine;
use disweep_core::store::Store;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "contacts Discord"]
async fn the_rest_client_can_reach_discord() {
    disweep_core::init_tls();
    let transport = HttpTransport::new(
        API_BASE,
        "Disweep-tests/1.0",
        "DiscordBot (https://github.com/kaanxsrd/Disweep, 1.0.0)",
    )
    .unwrap();

    let response = transport
        .execute(&ApiRequest::new(Method::GET, "/gateway"), "")
        .await
        .unwrap();

    assert_eq!(response.status, 200);
    let body: Value = serde_json::from_slice(&response.body).unwrap();
    assert!(body["url"].as_str().unwrap().starts_with("wss://"));
}

#[tokio::test]
#[ignore = "contacts Discord"]
async fn the_gateway_connects_and_reports_a_rejected_token() {
    disweep_core::init_tls();
    let fake = Arc::new(FakeDiscord::new());
    fake.set_me(1, "Kit");
    fake.set_application(json!({ "id": "700", "name": "Test", "flags": 0 }));
    let engine = Engine::new(Store::in_memory().unwrap(), fake, std::env::temp_dir());
    engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap();

    let bot = BotRuntime::new(engine).await.unwrap();
    // The simulated Discord accepts this token for REST; the real gateway will not.
    bot.set_gateway_url("wss://gateway.discord.gg".to_owned());
    bot.set_token(Secret::new(disweep_core::demo::DEMO_BOT_TOKEN.to_owned()))
        .await
        .unwrap();
    bot.set_enabled(true).await.unwrap();

    for _ in 0..600 {
        if bot.status().connection == BotConnection::Failed {
            let status = bot.status();
            assert!(status.problem.unwrap().contains("refused"));
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "expected Discord to reject the token, got {:?}",
        bot.status().connection
    );
}
