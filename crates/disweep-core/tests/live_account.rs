#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use disweep_core::Secret;
use disweep_core::api::{API_BASE, HttpTransport};
use disweep_core::engine::{Engine, JobOptions, JobOrigin, JobSpec, Scope, Target};
use disweep_core::filter::Filters;
use disweep_core::store::Store;
use tokio_util::sync::CancellationToken;

const CONVERSATIONS_TO_PREVIEW: usize = 4;

const HISTORY_LIMIT: usize = 1_000;

fn token() -> Secret {
    let value = std::env::var("DISWEEP_LIVE_TOKEN")
        .expect("set DISWEEP_LIVE_TOKEN to a disposable account's token");
    Secret::from_pasted(&value).unwrap()
}

async fn engine() -> Engine {
    disweep_core::init_tls();
    let transport = HttpTransport::new(
        API_BASE,
        "Disweep-live-tests/1.0 (+https://github.com/kaanxsrd/Disweep)",
        "DiscordBot (https://github.com/kaanxsrd/Disweep, 1.0.0)",
    )
    .unwrap();
    Engine::new(
        Store::in_memory().unwrap(),
        Arc::new(transport),
        std::env::temp_dir(),
    )
}

fn spec(account: disweep_core::Snowflake, scope: Scope, label: &str) -> JobSpec {
    JobSpec {
        account_id: account,
        targets: vec![Target {
            scope,
            label: label.to_owned(),
            expected: None,
        }],
        filters: Filters::default(),
        options: JobOptions::default(),
        origin: JobOrigin::App,
    }
}

#[tokio::test]
#[ignore = "needs a real account's token"]
async fn signs_in_lists_and_previews_without_deleting() {
    let engine = engine().await;
    let account = engine.sign_in(&token()).await.unwrap();
    println!("signed in as @{}", account.username);

    let client = engine.client(account.id).unwrap();
    let inventory = engine.inventory(account.id).await.unwrap();
    println!(
        "{} servers, {} conversations",
        inventory.guilds.len(),
        inventory.conversations.len()
    );

    for conversation in inventory
        .conversations
        .iter()
        .take(CONVERSATIONS_TO_PREVIEW)
    {
        let scope = Scope::Private {
            channel_id: conversation.id,
        };
        let preview = engine
            .preview(
                format!("live-{}", conversation.id),
                spec(account.id, scope, &conversation.name),
                CancellationToken::new(),
            )
            .await;
        let preview = match preview {
            Ok(preview) => preview,
            Err(error) => panic!("previewing DM {} failed: {error}", conversation.name),
        };

        let (mut total, mut mine, mut before) = (0, 0, None);
        while total < HISTORY_LIMIT {
            let page = client.messages(conversation.id, before, 100).await.unwrap();
            total += page.len();
            mine += page
                .iter()
                .filter(|message| message.author.id == account.id && !message.pinned)
                .count();
            before = page.last().map(|message| message.id);
            if page.len() < 100 {
                break;
            }
        }
        println!(
            "DM {}: preview {} of yours ({}), history {mine} of yours in {total}, {:?}",
            conversation.name,
            preview.total,
            if preview.exact { "exact" } else { "estimated" },
            preview
                .targets
                .first()
                .and_then(|target| target.note.clone()),
        );
    }

    if let Some(guild) = inventory.guilds.first() {
        let channels = engine.guild_channels(account.id, guild.id).await.unwrap();
        println!("server {}: {} channels", guild.name, channels.len());
        let preview = engine
            .preview(
                format!("live-{}", guild.id),
                spec(account.id, Scope::Guild { guild_id: guild.id }, &guild.name),
                CancellationToken::new(),
            )
            .await;
        match preview {
            Ok(preview) => println!(
                "server {}: {} of yours ({}), {:?}",
                guild.name,
                preview.total,
                if preview.exact { "exact" } else { "estimated" },
                preview
                    .targets
                    .first()
                    .and_then(|target| target.note.clone()),
            ),
            Err(error) => panic!("previewing server {} failed: {error}", guild.name),
        }
    }
}
