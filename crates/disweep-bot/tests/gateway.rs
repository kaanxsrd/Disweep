#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use disweep_bot::{BotConnection, BotRuntime};
use disweep_core::Secret;
use disweep_core::demo::{DEMO_BOT_TOKEN, DEMO_TOKEN, FakeDiscord};
use disweep_core::engine::Engine;
use disweep_core::store::Store;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_websockets::{Message, ServerBuilder};

const OWNER: u64 = 1;
const APPLICATION: u64 = 700;

fn frame(value: Value) -> Message {
    Message::text(value.to_string())
}

async fn serve_one_session(
    listener: TcpListener,
    identify_seen: oneshot::Sender<Value>,
    interaction: Value,
) {
    let (stream, _) = listener.accept().await.unwrap();
    let (_, mut socket) = ServerBuilder::new().accept(stream).await.unwrap();

    socket
        .send(frame(
            json!({ "op": 10, "d": { "heartbeat_interval": 45_000 } }),
        ))
        .await
        .unwrap();

    let identify: Value = loop {
        let message = socket.next().await.unwrap().unwrap();
        let Some(text) = message.as_text() else {
            continue;
        };
        let value: Value = serde_json::from_str(text).unwrap();
        if value["op"] == 2 {
            break value;
        }
    };
    identify_seen.send(identify).unwrap();

    socket
        .send(frame(json!({
            "op": 0, "s": 1, "t": "READY",
            "d": {
                "v": 10,
                "user": { "id": "701", "username": "my-disweep", "discriminator": "0", "bot": true, "avatar": null },
                "guilds": [],
                "session_id": "session",
                "resume_gateway_url": "ws://127.0.0.1:1",
                "shard": [0, 1],
                "application": { "id": APPLICATION.to_string(), "flags": 0 },
            }
        })))
        .await
        .unwrap();
    socket
        .send(frame(
            json!({ "op": 0, "s": 2, "t": "INTERACTION_CREATE", "d": interaction }),
        ))
        .await
        .unwrap();

    while let Some(Ok(message)) = socket.next().await {
        let Some(text) = message.as_text() else {
            continue;
        };
        if serde_json::from_str::<Value>(text).is_ok_and(|value| value["op"] == 1) {
            let _ = socket.send(frame(json!({ "op": 11 }))).await;
        }
    }
}

async fn wait_for(mut condition: impl FnMut() -> bool, what: &str) {
    for _ in 0..500 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn connects_identifies_with_no_intents_and_answers_a_command() {
    disweep_core::init_tls();
    let fake = Arc::new(FakeDiscord::new());
    fake.set_me(OWNER, "Kit");
    fake.set_application(
        json!({ "id": APPLICATION.to_string(), "name": "My Disweep", "flags": 0 }),
    );
    let engine = Engine::new(
        Store::in_memory().unwrap(),
        fake.clone(),
        std::env::temp_dir(),
    );
    engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (identify_tx, identify_rx) = oneshot::channel();
    let interaction = json!({
        "id": "9000",
        "application_id": APPLICATION.to_string(),
        "type": 2,
        "token": "interaction-token",
        "context": 2,
        "user": { "id": OWNER.to_string(), "username": "kit" },
        "channel": { "id": "50", "type": 1 },
        "data": { "name": "sweep", "type": 1, "options": [{ "name": "status", "type": 1 }] },
    });
    tokio::spawn(serve_one_session(listener, identify_tx, interaction));

    let bot = BotRuntime::new(engine).await.unwrap();
    bot.set_gateway_url(format!("ws://{address}"));
    bot.set_token(Secret::new(DEMO_BOT_TOKEN.to_owned()))
        .await
        .unwrap();
    bot.set_enabled(true).await.unwrap();

    let identify = tokio::time::timeout(Duration::from_secs(10), identify_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(identify["d"]["intents"], 0, "the bot asks for no intents");
    assert_eq!(identify["d"]["token"], format!("Bot {DEMO_BOT_TOKEN}"));

    wait_for(
        || bot.status().connection == BotConnection::Online,
        "the bot to come online",
    )
    .await;
    wait_for(
        || !fake.interaction_replies().is_empty(),
        "the reply to the command",
    )
    .await;

    let (path, body) = fake.interaction_replies().remove(0);
    assert_eq!(path, "/interactions/9000/interaction-token/callback");
    assert_eq!(body["type"], 4);
    assert_eq!(body["data"]["flags"], 64, "replies are private");

    bot.set_enabled(false).await.unwrap();
    assert_eq!(bot.status().connection, BotConnection::Off);
}
