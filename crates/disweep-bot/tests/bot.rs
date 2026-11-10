#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use disweep_bot::{BotConnection, BotError, BotRuntime};
use disweep_core::api::Method;
use disweep_core::demo::{DEMO_BOT_TOKEN, DEMO_TOKEN, FakeDiscord};
use disweep_core::engine::Engine;
use disweep_core::store::Store;
use disweep_core::{Secret, Snowflake, now_ms};
use serde_json::{Value, json};

const ME: (u64, &str) = (1, "Kit");
const FRIEND: (u64, &str) = (2, "Ada");
const STRANGER: u64 = 99;
const DM: u64 = 50;
const GUILD: u64 = 60;
const CHANNEL: u64 = 61;
const APPLICATION: u64 = 700;

struct Harness {
    bot: BotRuntime,
    fake: Arc<FakeDiscord>,
    engine: Engine,
}

fn application(extra: Value) -> Value {
    let mut base = json!({
        "id": APPLICATION.to_string(),
        "name": "My Disweep",
        "bot": { "id": "701", "username": "my-disweep", "bot": true },
        "integration_types_config": { "0": {}, "1": {} },
        "flags": 0,
    });
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}

async fn harness() -> Harness {
    disweep_core::init_tls();
    let fake = Arc::new(FakeDiscord::new());
    fake.set_me(ME.0, ME.1);
    fake.add_dm(DM, FRIEND.0, FRIEND.1);
    fake.add_guild(GUILD, "Rust Harbor");
    fake.add_guild_channel(GUILD, CHANNEL, "general");
    fake.set_application(application(json!({})));

    let engine = Engine::new(
        Store::in_memory().unwrap(),
        fake.clone(),
        std::env::temp_dir(),
    );
    engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .unwrap();
    let bot = BotRuntime::new(engine.clone()).await.unwrap();
    bot.set_token(Secret::new(DEMO_BOT_TOKEN.to_owned()))
        .await
        .unwrap();
    Harness { bot, fake, engine }
}

fn id(n: u64) -> u64 {
    (1_000 + n) << 22
}

fn seed_dm(fake: &FakeDiscord) {
    for n in 0..6 {
        let author = if n % 2 == 0 { ME } else { FRIEND };
        fake.add_message(DM, id(n), author, &format!("message {n}"));
    }
}

fn base_interaction(kind: u8, invoker: u64, data: Value) -> Value {
    json!({
        "id": "9000",
        "application_id": APPLICATION.to_string(),
        "type": kind,
        "token": "interaction-token",
        "user": { "id": invoker.to_string(), "username": "someone", "global_name": "Someone" },
        "context": 2,
        "channel": { "id": DM.to_string(), "type": 1, "recipients": [{ "id": "2", "username": "ada", "global_name": "Ada" }] },
        "data": data,
    })
}

fn slash(invoker: u64, root: &str, sub: &str, options: Value) -> Value {
    base_interaction(
        2,
        invoker,
        json!({ "name": root, "type": 1, "options": [{ "name": sub, "type": 1, "options": options }] }),
    )
}

fn button(invoker: u64, custom_id: &str) -> Value {
    base_interaction(
        3,
        invoker,
        json!({ "custom_id": custom_id, "component_type": 2 }),
    )
}

fn guild_slash(
    root: &str,
    options: Value,
    invoker_permissions: u64,
    app_permissions: u64,
) -> Value {
    json!({
        "id": "9001",
        "application_id": APPLICATION.to_string(),
        "type": 2,
        "token": "interaction-token",
        "guild_id": GUILD.to_string(),
        "context": 0,
        "channel": { "id": CHANNEL.to_string(), "type": 0, "name": "general" },
        "member": {
            "user": { "id": "5", "username": "mod" },
            "permissions": invoker_permissions.to_string(),
        },
        "app_permissions": app_permissions.to_string(),
        "data": { "name": root, "type": 1, "options": options },
    })
}

fn last_reply(fake: &FakeDiscord) -> Value {
    fake.interaction_replies()
        .last()
        .expect("the bot should have replied")
        .1
        .clone()
}

fn view_of(body: &Value) -> Value {
    if body.get("data").is_some() {
        body["data"].clone()
    } else {
        body.clone()
    }
}

fn description(body: &Value) -> String {
    view_of(body)["embeds"][0]["description"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn confirm_id(body: &Value) -> String {
    view_of(body)["components"][0]["components"][0]["custom_id"]
        .as_str()
        .expect("a confirm button")
        .to_owned()
}

fn cancel_id(body: &Value) -> String {
    view_of(body)["components"][0]["components"][1]["custom_id"]
        .as_str()
        .expect("a cancel button")
        .to_owned()
}

async fn wait_until_idle(engine: &Engine) {
    for _ in 0..100_000 {
        if engine.active_jobs().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the sweep never finished");
}

#[tokio::test(start_paused = true)]
async fn sweep_here_previews_then_deletes_only_after_confirming() {
    let h = harness().await;
    seed_dm(&h.fake);

    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();

    let replies = h.fake.interaction_replies();
    assert_eq!(
        replies[0].1["type"], 5,
        "deferred first, because previews can take a while"
    );
    assert_eq!(replies[0].1["data"]["flags"], 64, "and private");
    let preview = last_reply(&h.fake);
    assert!(
        description(&preview).contains("**about 3**"),
        "{}",
        description(&preview)
    );
    assert!(
        view_of(&preview).get("flags").is_none(),
        "edits can't change privacy, so they omit it"
    );
    assert_eq!(
        h.fake.count_requests(&Method::DELETE),
        0,
        "nothing goes until confirmed"
    );

    h.bot
        .simulate(button(ME.0, &confirm_id(&preview)))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;

    let survivors = h.fake.remaining(DM);
    assert_eq!(survivors.len(), 3);
    assert!(
        survivors.iter().all(|m| (m >> 22) % 2 == 1),
        "only your own messages are deleted"
    );
}

#[tokio::test(start_paused = true)]
async fn the_final_result_is_posted_to_the_reply() {
    let h = harness().await;
    seed_dm(&h.fake);
    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();
    h.bot
        .simulate(button(ME.0, &confirm_id(&last_reply(&h.fake))))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;
    tokio::time::sleep(Duration::from_secs(10)).await;

    let view = last_reply(&h.fake);
    assert_eq!(view["embeds"][0]["title"], "Sweep finished");
    assert!(description(&view).contains("Deleted **3**"));
}

#[tokio::test(start_paused = true)]
async fn strangers_cannot_use_personal_commands() {
    let h = harness().await;
    seed_dm(&h.fake);

    h.bot
        .simulate(slash(STRANGER, "sweep", "here", json!([])))
        .await
        .unwrap();

    assert!(description(&last_reply(&h.fake)).contains("belongs to someone else"));
    assert_eq!(h.fake.count_requests(&Method::DELETE), 0);
    assert!(h.engine.active_jobs().is_empty());
}

#[tokio::test(start_paused = true)]
async fn only_the_person_who_asked_can_confirm() {
    let h = harness().await;
    seed_dm(&h.fake);
    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);

    h.bot
        .simulate(button(STRANGER, &confirm_id(&preview)))
        .await
        .unwrap();

    assert_eq!(h.fake.count_requests(&Method::DELETE), 0);
    assert!(h.engine.active_jobs().is_empty());

    h.bot
        .simulate(button(ME.0, &confirm_id(&preview)))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;
    assert_eq!(h.fake.remaining(DM).len(), 3);
}

#[tokio::test(start_paused = true)]
async fn a_confirmation_only_works_once() {
    let h = harness().await;
    seed_dm(&h.fake);
    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);

    h.bot
        .simulate(button(ME.0, &confirm_id(&preview)))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;
    let deletes = h.fake.count_requests(&Method::DELETE);
    h.bot
        .simulate(button(ME.0, &confirm_id(&preview)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(30)).await;

    assert_eq!(h.fake.count_requests(&Method::DELETE), deletes);
}

#[tokio::test(start_paused = true)]
async fn cancel_deletes_nothing() {
    let h = harness().await;
    seed_dm(&h.fake);
    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);

    h.bot
        .simulate(button(ME.0, &cancel_id(&preview)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(30)).await;

    assert_eq!(
        last_reply(&h.fake)["data"]["embeds"][0]["title"],
        "Cancelled"
    );
    assert_eq!(h.fake.count_requests(&Method::DELETE), 0);
}

#[tokio::test(start_paused = true)]
async fn an_unknown_button_says_it_expired() {
    let h = harness().await;
    h.bot
        .simulate(button(ME.0, "ds:ok:does-not-exist"))
        .await
        .unwrap();
    assert_eq!(
        last_reply(&h.fake)["data"]["embeds"][0]["title"],
        "This expired"
    );
}

#[tokio::test(start_paused = true)]
async fn nothing_to_sweep_offers_no_button() {
    let h = harness().await;
    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();
    let view = last_reply(&h.fake);
    assert_eq!(view["embeds"][0]["title"], "Nothing to sweep");
    assert_eq!(view["components"], json!([]));
}

#[tokio::test(start_paused = true)]
async fn filters_reach_the_sweep() {
    let h = harness().await;
    h.fake.add_message(DM, id(1), ME, "my secret plan");
    h.fake.add_message(DM, id(2), ME, "lunch?");

    h.bot
        .simulate(slash(
            ME.0,
            "sweep",
            "here",
            json!([{ "name": "contains", "type": 3, "value": "secret" }]),
        ))
        .await
        .unwrap();
    h.bot
        .simulate(button(ME.0, &confirm_id(&last_reply(&h.fake))))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;

    assert_eq!(h.fake.remaining(DM), vec![id(2)]);
}

#[tokio::test(start_paused = true)]
async fn a_bad_duration_gets_a_helpful_answer() {
    let h = harness().await;
    h.bot
        .simulate(slash(
            ME.0,
            "sweep",
            "here",
            json!([{ "name": "older_than", "type": 3, "value": "soon" }]),
        ))
        .await
        .unwrap();
    assert!(description(&last_reply(&h.fake)).contains("soon"));
}

#[tokio::test(start_paused = true)]
async fn sweep_dm_finds_the_conversation_from_anywhere() {
    let h = harness().await;
    seed_dm(&h.fake);
    let mut payload = slash(
        ME.0,
        "sweep",
        "dm",
        json!([{ "name": "user", "type": 6, "value": FRIEND.0.to_string() }]),
    );
    payload["data"]["resolved"] =
        json!({ "users": { "2": { "id": "2", "username": "ada", "global_name": "Ada" } } });
    payload["channel"] = json!({ "id": "12345", "type": 0, "name": "elsewhere" });

    h.bot.simulate(payload).await.unwrap();

    let preview = last_reply(&h.fake);
    assert!(
        description(&preview).contains("**Ada**"),
        "{}",
        description(&preview)
    );
    h.bot
        .simulate(button(ME.0, &confirm_id(&preview)))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;
    assert_eq!(h.fake.remaining(DM).len(), 3);
}

#[tokio::test(start_paused = true)]
async fn the_message_menu_sweeps_from_the_picked_message() {
    let h = harness().await;
    for n in 0..5 {
        h.fake.add_message(DM, id(n), ME, "x");
    }
    let payload = base_interaction(
        2,
        ME.0,
        json!({ "name": "Sweep this and newer", "type": 3, "target_id": id(2).to_string() }),
    );

    h.bot.simulate(payload).await.unwrap();
    h.bot
        .simulate(button(ME.0, &confirm_id(&last_reply(&h.fake))))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;

    assert_eq!(h.fake.remaining(DM), vec![id(0), id(1)]);
}

#[tokio::test(start_paused = true)]
async fn status_pause_resume_and_stop_control_the_sweep() {
    let h = harness().await;
    for n in 0..200 {
        h.fake.add_message(DM, id(n), ME, "x");
    }
    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();
    h.bot
        .simulate(button(ME.0, &confirm_id(&last_reply(&h.fake))))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;

    h.bot
        .simulate(slash(ME.0, "sweep", "status", json!([])))
        .await
        .unwrap();
    assert_eq!(
        view_of(&last_reply(&h.fake))["embeds"][0]["title"],
        "Sweeping"
    );

    h.bot
        .simulate(slash(ME.0, "sweep", "pause", json!([])))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let paused_at = h.fake.count_requests(&Method::DELETE);
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(h.fake.count_requests(&Method::DELETE), paused_at);

    h.bot
        .simulate(slash(ME.0, "sweep", "resume", json!([])))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(h.fake.count_requests(&Method::DELETE) > paused_at);

    h.bot
        .simulate(slash(ME.0, "sweep", "stop", json!([])))
        .await
        .unwrap();
    wait_until_idle(&h.engine).await;
    assert!(!h.fake.remaining(DM).is_empty());
}

#[tokio::test(start_paused = true)]
async fn status_with_nothing_running_says_so() {
    let h = harness().await;
    h.bot
        .simulate(slash(ME.0, "sweep", "status", json!([])))
        .await
        .unwrap();
    assert!(description(&last_reply(&h.fake)).contains("Nothing is sweeping"));
}

#[tokio::test(start_paused = true)]
async fn a_turned_off_command_refuses_to_run() {
    let h = harness().await;
    seed_dm(&h.fake);
    h.bot
        .set_command_enabled("sweep.here", false)
        .await
        .unwrap();

    h.bot
        .simulate(slash(ME.0, "sweep", "here", json!([])))
        .await
        .unwrap();

    assert!(description(&last_reply(&h.fake)).contains("turned off"));
}

#[tokio::test(start_paused = true)]
async fn autosweep_asks_first_then_creates_a_rule() {
    let h = harness().await;
    h.bot
        .simulate(slash(
            ME.0,
            "autosweep",
            "set",
            json!([{ "name": "older_than", "type": 3, "value": "30d" }]),
        ))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);
    assert!(
        h.engine.rules().await.unwrap().is_empty(),
        "no rule until confirmed"
    );

    h.bot
        .simulate(button(ME.0, &confirm_id(&preview)))
        .await
        .unwrap();

    let rules = h.engine.rules().await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].older_than_secs, 30 * 86_400);
    assert_eq!(rules[0].every_secs, 86_400);

    h.bot
        .simulate(slash(ME.0, "autosweep", "list", json!([])))
        .await
        .unwrap();
    assert!(description(&last_reply(&h.fake)).contains("1mo"));

    h.bot
        .simulate(slash(ME.0, "autosweep", "off", json!([])))
        .await
        .unwrap();
    assert!(h.engine.rules().await.unwrap().is_empty());
}

const MANAGE: u64 = 1 << 13;
const READ_HISTORY: u64 = 1 << 16;

fn purge_options(count: u64) -> Value {
    json!([{ "name": "count", "type": 4, "value": count }])
}

fn seed_recent(fake: &FakeDiscord, count: u64) {
    let now = now_ms();
    for n in 0..count {
        let message = Snowflake::from_timestamp_ms(now - 60_000 - (n as i64) * 1000).0;
        fake.add_message(CHANNEL, message, FRIEND, "spam");
    }
}

#[tokio::test(start_paused = true)]
async fn purge_needs_the_moderators_permission() {
    let h = harness().await;
    seed_recent(&h.fake, 5);
    h.bot
        .simulate(guild_slash(
            "purge",
            purge_options(5),
            0,
            MANAGE | READ_HISTORY,
        ))
        .await
        .unwrap();
    assert!(description(&last_reply(&h.fake)).contains("Manage Messages"));
    assert_eq!(h.fake.remaining(CHANNEL).len(), 5);
}

#[tokio::test(start_paused = true)]
async fn purge_needs_the_bots_permission_too() {
    let h = harness().await;
    seed_recent(&h.fake, 5);
    h.bot
        .simulate(guild_slash("purge", purge_options(5), MANAGE, 0))
        .await
        .unwrap();
    assert!(description(&last_reply(&h.fake)).contains("I need"));
}

#[tokio::test(start_paused = true)]
async fn purge_deletes_in_bulk_after_confirming() {
    let h = harness().await;
    seed_recent(&h.fake, 150);

    h.bot
        .simulate(guild_slash(
            "purge",
            purge_options(120),
            MANAGE,
            MANAGE | READ_HISTORY,
        ))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);
    assert!(description(&preview).contains("**120**"));
    assert_eq!(
        h.fake.remaining(CHANNEL).len(),
        150,
        "nothing goes until confirmed"
    );

    let mut click = button(5, &confirm_id(&preview));
    click["user"] = Value::Null;
    click["member"] =
        json!({ "user": { "id": "5", "username": "mod" }, "permissions": MANAGE.to_string() });
    h.bot.simulate(click).await.unwrap();

    assert_eq!(h.fake.remaining(CHANNEL).len(), 30);
    let bulk = h
        .fake
        .requests()
        .into_iter()
        .filter(|r| r.path.ends_with("bulk-delete"))
        .count();
    assert_eq!(bulk, 2, "120 messages is two bulk requests");
    assert_eq!(last_reply(&h.fake)["embeds"][0]["title"], "Purge finished");
}

#[tokio::test(start_paused = true)]
async fn purge_leaves_old_messages_unless_asked() {
    let h = harness().await;
    let recent = Snowflake::from_timestamp_ms(now_ms() - 60_000).0;
    let also_recent = recent - (1 << 22);
    let old = id(1);
    h.fake.add_message(CHANNEL, recent, FRIEND, "new");
    h.fake.add_message(CHANNEL, also_recent, FRIEND, "new");
    h.fake.add_message(CHANNEL, old, FRIEND, "ancient");

    h.bot
        .simulate(guild_slash(
            "purge",
            purge_options(10),
            MANAGE,
            MANAGE | READ_HISTORY,
        ))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);
    assert!(description(&preview).contains("over two weeks old"));

    let mut click = button(5, &confirm_id(&preview));
    click["user"] = Value::Null;
    click["member"] = json!({ "user": { "id": "5", "username": "mod" } });
    h.bot.simulate(click).await.unwrap();

    assert_eq!(h.fake.remaining(CHANNEL), vec![old]);
}

#[tokio::test(start_paused = true)]
async fn purge_filters_by_person() {
    let h = harness().await;
    let now = now_ms();
    h.fake.add_message(
        CHANNEL,
        Snowflake::from_timestamp_ms(now - 1000).0,
        FRIEND,
        "theirs",
    );
    h.fake.add_message(
        CHANNEL,
        Snowflake::from_timestamp_ms(now - 2000).0,
        ME,
        "mine",
    );

    let options = json!([{ "name": "count", "type": 4, "value": 10 }, { "name": "user", "type": 6, "value": FRIEND.0.to_string() }]);
    h.bot
        .simulate(guild_slash("purge", options, MANAGE, MANAGE | READ_HISTORY))
        .await
        .unwrap();
    let preview = last_reply(&h.fake);
    assert!(description(&preview).contains("**1**"));

    let mut click = button(5, &confirm_id(&preview));
    click["user"] = Value::Null;
    click["member"] = json!({ "user": { "id": "5", "username": "mod" } });
    h.bot.simulate(click).await.unwrap();

    assert_eq!(h.fake.remaining(CHANNEL).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn purge_explains_when_it_cannot_read_message_text() {
    let h = harness().await;
    seed_recent(&h.fake, 2);
    let options = json!([{ "name": "count", "type": 4, "value": 10 }, { "name": "contains", "type": 3, "value": "spam" }]);

    h.bot
        .simulate(guild_slash("purge", options, MANAGE, MANAGE | READ_HISTORY))
        .await
        .unwrap();

    assert!(description(&last_reply(&h.fake)).contains("Message Content Intent"));
    assert_eq!(h.fake.remaining(CHANNEL).len(), 2);
}

#[tokio::test]
async fn setup_turns_on_installs_and_registers_commands() {
    let h = harness().await;
    let status = h.bot.configure(false).await.unwrap();

    assert!(status.registered);
    assert!(status.application.unwrap().supports_user_install);
    assert!(status.links.unwrap().account.contains("integration_type=1"));

    let names: Vec<String> = h
        .fake
        .commands()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"sweep".to_owned()));
    assert!(names.contains(&"purge".to_owned()));
    assert!(names.contains(&"Sweep this and newer".to_owned()));

    let patches: Vec<_> = h
        .fake
        .requests()
        .into_iter()
        .filter(|r| r.method == Method::PATCH)
        .collect();
    assert_eq!(patches[0].path, "/applications/@me");
    let body = patches[0].body.as_ref().unwrap();
    assert_eq!(
        body["integration_types_config"]["1"]["oauth2_install_params"]["scopes"],
        json!(["applications.commands"])
    );
    assert!(
        body.get("flags").is_none(),
        "sent separately, so one refusal can't lose both"
    );
    assert_eq!(
        patches[1].body.as_ref().unwrap()["flags"],
        1 << 19,
        "the limited content flag is the one the API allows"
    );
}

#[tokio::test]
async fn setup_will_not_silently_replace_someone_elses_commands() {
    let h = harness().await;
    h.fake
        .set_commands(vec![json!({ "name": "ban", "type": 1 })]);

    let refused = h.bot.configure(false).await;
    assert!(
        matches!(refused, Err(BotError::ForeignCommands(ref names)) if names == &["ban".to_owned()])
    );
    assert_eq!(h.fake.count_requests(&Method::PUT), 0);
    assert_eq!(h.fake.commands().len(), 1);

    h.bot.configure(true).await.unwrap();
    assert!(h.fake.commands().iter().all(|c| c["name"] != "ban"));
}

#[tokio::test]
async fn setup_refuses_an_application_using_a_webhook_endpoint() {
    let h = harness().await;
    h.fake.set_application(application(
        json!({ "interactions_endpoint_url": "https://example.com/hook" }),
    ));
    h.bot
        .set_token(Secret::new(DEMO_BOT_TOKEN.to_owned()))
        .await
        .unwrap();

    assert!(matches!(
        h.bot.configure(false).await,
        Err(BotError::EndpointConflict)
    ));
    assert_eq!(h.fake.count_requests(&Method::PUT), 0);
}

#[tokio::test]
async fn toggling_a_command_updates_discord_only_after_setup() {
    let h = harness().await;
    h.bot.set_command_enabled("purge", false).await.unwrap();
    assert_eq!(
        h.fake.count_requests(&Method::PUT),
        0,
        "setup hasn't happened yet"
    );

    h.bot.configure(false).await.unwrap();
    assert!(h.fake.commands().iter().all(|c| c["name"] != "purge"));

    h.bot.set_command_enabled("purge", true).await.unwrap();
    assert!(h.fake.commands().iter().any(|c| c["name"] == "purge"));
}

#[tokio::test]
async fn command_choices_survive_a_restart() {
    let h = harness().await;
    h.bot.set_command_enabled("sweep.dm", false).await.unwrap();

    let restarted = BotRuntime::new(h.engine.clone()).await.unwrap();

    let dm = restarted
        .status()
        .commands
        .into_iter()
        .find(|c| c.key == "sweep.dm")
        .unwrap();
    assert!(!dm.enabled);
}

#[tokio::test]
async fn enabling_needs_a_token_and_is_remembered() {
    let h = harness().await;
    h.bot.clear_token().await.unwrap();
    assert!(matches!(
        h.bot.set_enabled(true).await,
        Err(BotError::NoToken)
    ));

    h.bot
        .set_token(Secret::new(DEMO_BOT_TOKEN.to_owned()))
        .await
        .unwrap();
    let status = h.bot.set_enabled(true).await.unwrap();
    assert_eq!(status.connection, BotConnection::Online);

    let restarted = BotRuntime::new(h.engine.clone()).await.unwrap();
    assert!(restarted.status().enabled);
}

#[tokio::test]
async fn a_bad_bot_token_is_refused() {
    let h = harness().await;
    assert!(
        h.bot
            .set_token(Secret::new("wrong-token-entirely-not-valid".to_owned()))
            .await
            .is_err()
    );
}

#[tokio::test(start_paused = true)]
async fn the_status_never_exposes_the_token() {
    let h = harness().await;
    h.bot
        .simulate(slash(ME.0, "sweep", "status", json!([])))
        .await
        .unwrap();
    let serialized = serde_json::to_string(&h.bot.status()).unwrap();
    assert!(!serialized.contains(DEMO_BOT_TOKEN));
    assert!(!serialized.contains("interaction-token"));
}

#[tokio::test(start_paused = true)]
async fn activity_is_recorded_for_the_screen() {
    let h = harness().await;
    h.bot
        .simulate(slash(ME.0, "sweep", "status", json!([])))
        .await
        .unwrap();
    let activity = h.bot.status().activity;
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].command, "/sweep status");
}
