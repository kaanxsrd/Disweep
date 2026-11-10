//! Preparing a Discord application for use as a Disweep bot.

use std::collections::HashSet;

use disweep_core::Snowflake;
use disweep_core::api::{Application, Discord};
use disweep_core::error::ApiResult;
use serde_json::{Value, json};

use crate::commands::{all_root_names, registration_payload};
use crate::runtime::InstallLinks;

pub const MANAGE_MESSAGES: u64 = 1 << 13;
const VIEW_CHANNEL: u64 = 1 << 10;
const READ_MESSAGE_HISTORY: u64 = 1 << 16;
const FLAG_MESSAGE_CONTENT: u64 = 1 << 18;
const FLAG_MESSAGE_CONTENT_LIMITED: u64 = 1 << 19;

/// The permissions `/purge` needs when the bot joins a server.
#[must_use]
pub const fn required_guild_permissions() -> u64 {
    MANAGE_MESSAGES | VIEW_CHANNEL | READ_MESSAGE_HISTORY
}

/// Links that add the bot to an account or a server.
#[must_use]
pub fn install_links(application: Snowflake) -> InstallLinks {
    let base = format!("https://discord.com/oauth2/authorize?client_id={application}");
    InstallLinks {
        account: format!("{base}&integration_type=1&scope=applications.commands"),
        server: format!(
            "{base}&integration_type=0&scope=bot+applications.commands&permissions={}",
            required_guild_permissions()
        ),
    }
}

/// True when the application can be installed to a person's account.
#[must_use]
pub fn supports_user_install(application: &Application) -> bool {
    application
        .integration_types_config
        .as_ref()
        .is_some_and(|config| config.contains_key("1"))
}

/// True when Discord sends this application message text, which `/purge` filters depend on.
#[must_use]
pub fn can_read_message_content(application: &Application) -> bool {
    application
        .flags
        .is_some_and(|flags| flags & (FLAG_MESSAGE_CONTENT | FLAG_MESSAGE_CONTENT_LIMITED) != 0)
}

/// The names of registered commands that didn't come from Disweep.
pub async fn foreign_commands(client: &Discord, application: Snowflake) -> ApiResult<Vec<String>> {
    let ours = all_root_names();
    let registered = client.global_commands(application).await?;
    Ok(registered
        .iter()
        .filter_map(|command| command["name"].as_str())
        .filter(|name| !ours.contains(name))
        .map(str::to_owned)
        .collect())
}

/// Turns on account installs and message text, as far as Discord lets an API call do so.
///
/// Discord documents these fields but not whether every one takes effect, so a refusal isn't
/// fatal. The caller re-reads the application and reports what is actually in place.
pub async fn apply_install_settings(client: &Discord, application: &Application) -> Application {
    let install = json!({
        "integration_types_config": {
            "0": { "oauth2_install_params": {
                "scopes": ["bot", "applications.commands"],
                "permissions": required_guild_permissions().to_string(),
            } },
            "1": { "oauth2_install_params": { "scopes": ["applications.commands"], "permissions": "0" } },
        },
    });
    if let Err(error) = client.patch_application(install).await {
        tracing::warn!(%error, "Discord refused the install settings");
    }
    // Sent separately, so a refusal here can't also lose the install settings.
    if !can_read_message_content(application) {
        let flags = application.flags.unwrap_or(0) | FLAG_MESSAGE_CONTENT_LIMITED;
        if let Err(error) = client.patch_application(json!({ "flags": flags })).await {
            tracing::warn!(%error, "Discord refused the message content setting");
        }
    }
    client
        .application()
        .await
        .unwrap_or_else(|_| application.clone())
}

/// Replaces the application's commands with the enabled set.
pub async fn register(
    client: &Discord,
    application: Snowflake,
    enabled: &HashSet<String>,
) -> ApiResult<Vec<Value>> {
    client
        .overwrite_global_commands(application, registration_payload(enabled))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_links_match_discords_format() {
        let links = install_links(Snowflake(42));
        assert_eq!(
            links.account,
            "https://discord.com/oauth2/authorize?client_id=42&integration_type=1&scope=applications.commands"
        );
        assert!(links.server.contains("integration_type=0"));
        assert!(links.server.contains("scope=bot+applications.commands"));
        assert!(links.server.contains("permissions=74752"));
    }
}
