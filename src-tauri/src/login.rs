//! Signing in through Discord's own page, so nobody has to dig a token out of developer tools.
//!
//! The window is a private session with no access to Disweep's commands: remote pages have no
//! capability granted to them, so the only thing the page can do is navigate.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};
use ts_rs::TS;
use uuid::Uuid;

use crate::secrets::account_key;
use crate::state::AppState;
use disweep_core::Secret;
use disweep_core::engine::Account;

const WINDOW_LABEL: &str = "discord-login";
const HOOK: &str = include_str!("login_hook.js");
const CALLBACK_PREFIX: &str = "https://disweep-auth.invalid/";

#[derive(Clone, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum LoginEvent {
    Success { account: Account },
    Failure { message: String },
}

fn is_discord(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host == "discord.com" || host.ends_with(".discord.com"))
}

pub fn open(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return window.set_focus().map_err(|error| error.to_string());
    }

    let nonce = Uuid::new_v4().simple().to_string();
    let callback = format!("{CALLBACK_PREFIX}{nonce}");
    let handle = app.clone();
    let start = Url::parse("https://discord.com/login").map_err(|error| error.to_string())?;

    WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(start))
        .title("Sign in to Discord")
        .inner_size(500.0, 760.0)
        .center()
        .incognito(true)
        .initialization_script(HOOK.replace("__NONCE__", &nonce))
        .on_navigation(move |url| {
            if url.as_str().starts_with(&callback) {
                let token = url
                    .query_pairs()
                    .find(|(key, _)| key == "token")
                    .map(|(_, value)| value.into_owned());
                if let Some(token) = token {
                    finish(handle.clone(), token);
                }
                return false;
            }
            is_discord(url) || url.scheme() == "about"
        })
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn finish(app: AppHandle, token: String) {
    tauri::async_runtime::spawn(async move {
        let event = match complete(&app, token).await {
            Ok(account) => LoginEvent::Success { account },
            Err(message) => LoginEvent::Failure { message },
        };
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = window.close();
        }
        let _ = app.emit("login", event);
    });
}

async fn complete(app: &AppHandle, token: String) -> Result<Account, String> {
    let secret = Secret::from_pasted(&token).map_err(|error| error.to_string())?;
    let state = app.state::<AppState>();
    let account = state
        .engine
        .sign_in(&secret)
        .await
        .map_err(|error| error.to_string())?;
    state.secrets.save(&account_key(account.id), &secret)?;
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discords_own_pages_are_allowed() {
        for allowed in [
            "https://discord.com/login",
            "https://canary.discord.com/app",
        ] {
            assert!(is_discord(&Url::parse(allowed).unwrap()), "{allowed}");
        }
        for refused in [
            "http://discord.com/login",
            "https://discord.com.evil.example/login",
            "https://notdiscord.com/",
            "https://evil.example/?discord.com",
        ] {
            assert!(!is_discord(&Url::parse(refused).unwrap()), "{refused}");
        }
    }

    #[test]
    fn the_hook_has_a_slot_for_the_nonce_and_targets_an_unresolvable_host() {
        assert!(HOOK.contains("__NONCE__"));
        assert!(HOOK.contains(CALLBACK_PREFIX));
    }
}
