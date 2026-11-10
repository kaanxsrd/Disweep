//! Everything the interface can ask the app to do.
//!
//! The interface has no other privileges: files, links, dialogs and the keychain are all reached
//! through these functions, and each one validates what it's given.

use std::path::{Path, PathBuf};

use disweep_bot::BotStatus;
use disweep_core::Snowflake;
use disweep_core::demo::{DEMO_BOT_TOKEN, DEMO_TOKEN};
use disweep_core::engine::{
    Account, ChannelEntry, FailureRecord, Inventory, JobSnapshot, JobSpec, Preview, Rule,
    RuleDraft, Stats,
};
use disweep_core::package::PackageIndex;
use disweep_core::store::SettingEntry;
use disweep_core::{Secret, filter};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::login;
use crate::secrets::{BOT_KEY, account_key};
use crate::state::AppState;

type CommandResult<T> = Result<T, String>;

fn text<E: ToString>(error: E) -> String {
    error.to_string()
}

const ALLOWED_LINKS: [&str; 4] = [
    "https://discord.com/",
    "https://support.discord.com/",
    "https://github.com/kaanxsrd/Disweep",
    "https://docs.discord.com/",
];

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub demo: bool,
    pub platform: String,
    pub archive_folder: String,
    /// False until saved accounts have been loaded at startup.
    pub restored: bool,
}

/// Keeps the native title bar in step with the app's theme.
#[tauri::command]
pub fn window_set_theme(window: WebviewWindow, theme: String) -> CommandResult<()> {
    let value = match theme.as_str() {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        _ => None,
    };
    window.set_theme(value).map_err(text)
}

/// Shows the window once the interface has drawn, so it never flashes white.
#[tauri::command]
pub fn ui_ready(window: WebviewWindow, state: State<'_, AppState>) -> CommandResult<()> {
    if !state.start_hidden {
        window.show().map_err(text)?;
        window.set_focus().map_err(text)?;
    }
    Ok(())
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        demo: state.demo,
        platform: std::env::consts::OS.to_owned(),
        archive_folder: state.engine.archive_root().to_string_lossy().into_owned(),
        restored: state.restored.load(std::sync::atomic::Ordering::SeqCst),
    }
}

#[tauri::command]
pub fn accounts_list(state: State<'_, AppState>) -> Vec<Account> {
    state.engine.accounts()
}

#[tauri::command]
pub async fn account_add_token(
    state: State<'_, AppState>,
    token: String,
) -> CommandResult<Account> {
    let secret = Secret::from_pasted(&token).map_err(text)?;
    let account = state.engine.sign_in(&secret).await.map_err(text)?;
    state.secrets.save(&account_key(account.id), &secret)?;
    Ok(account)
}

/// This has to be async: on Windows, creating a window from a synchronous command stalls it.
#[tauri::command]
pub async fn account_open_login(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    if state.demo {
        return Err("The demo has no real sign-in.".to_owned());
    }
    login::open(&app)
}

#[tauri::command]
pub async fn account_remove(state: State<'_, AppState>, id: Snowflake) -> CommandResult<()> {
    state.engine.sign_out(id).await.map_err(text)?;
    state.secrets.delete(&account_key(id))
}

/// Signs in to the simulated Discord. Only exists in demo mode.
#[tauri::command]
pub async fn demo_sign_in(state: State<'_, AppState>) -> CommandResult<Account> {
    if !state.demo {
        return Err("This is only available in the demo.".to_owned());
    }
    let account = state
        .engine
        .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
        .await
        .map_err(text)?;
    state
        .engine
        .seed_demo_history(account.id)
        .await
        .map_err(text)?;
    Ok(account)
}

#[tauri::command]
pub async fn inventory(state: State<'_, AppState>, account: Snowflake) -> CommandResult<Inventory> {
    state.engine.inventory(account).await.map_err(text)
}

#[tauri::command]
pub async fn guild_channels(
    state: State<'_, AppState>,
    account: Snowflake,
    guild: Snowflake,
) -> CommandResult<Vec<ChannelEntry>> {
    state
        .engine
        .guild_channels(account, guild)
        .await
        .map_err(text)
}

/// Returns what is wrong with a regular expression, or nothing if it's fine.
#[tauri::command]
pub fn check_pattern(pattern: String) -> Option<String> {
    filter::validate_pattern(&pattern)
        .err()
        .map(|error| error.to_string())
}

#[tauri::command]
pub async fn preview_start(
    state: State<'_, AppState>,
    preview_id: String,
    spec: JobSpec,
) -> CommandResult<Preview> {
    let cancel = CancellationToken::new();
    state
        .previews
        .lock()
        .insert(preview_id.clone(), cancel.clone());
    let result = state.engine.preview(preview_id.clone(), spec, cancel).await;
    state.previews.lock().remove(&preview_id);
    result.map_err(text)
}

#[tauri::command]
pub fn preview_cancel(state: State<'_, AppState>, preview_id: String) {
    if let Some(cancel) = state.previews.lock().remove(&preview_id) {
        cancel.cancel();
    }
}

#[tauri::command]
pub async fn job_start(state: State<'_, AppState>, spec: JobSpec) -> CommandResult<JobSnapshot> {
    state.engine.start(spec).await.map_err(text)
}

#[tauri::command]
pub fn job_pause(state: State<'_, AppState>, id: String) -> CommandResult<JobSnapshot> {
    state.engine.pause(&id).map_err(text)
}

#[tauri::command]
pub fn job_resume(state: State<'_, AppState>, id: String) -> CommandResult<JobSnapshot> {
    state.engine.resume(&id).map_err(text)
}

#[tauri::command]
pub fn job_stop(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.engine.stop(&id).map_err(text)
}

#[tauri::command]
pub fn jobs_active(state: State<'_, AppState>) -> Vec<JobSnapshot> {
    state.engine.active_jobs()
}

#[tauri::command]
pub async fn jobs_history(
    state: State<'_, AppState>,
    limit: u32,
) -> CommandResult<Vec<JobSnapshot>> {
    state.engine.history(limit.min(500)).await.map_err(text)
}

#[tauri::command]
pub async fn job_failures(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Vec<FailureRecord>> {
    state.engine.failures(&id).await.map_err(text)
}

#[tauri::command]
pub async fn job_resume_finished(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<JobSnapshot> {
    state.engine.resume_finished(&id).await.map_err(text)
}

#[tauri::command]
pub async fn job_forget(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.engine.forget_job(&id).await.map_err(text)
}

#[tauri::command]
pub async fn stats(state: State<'_, AppState>, account: Snowflake) -> CommandResult<Stats> {
    state.engine.stats(account).await.map_err(text)
}

#[tauri::command]
pub async fn rules_list(state: State<'_, AppState>) -> CommandResult<Vec<Rule>> {
    state.engine.rules().await.map_err(text)
}

#[tauri::command]
pub async fn rule_add(state: State<'_, AppState>, draft: RuleDraft) -> CommandResult<Rule> {
    state.engine.add_rule(draft).await.map_err(text)
}

#[tauri::command]
pub async fn rule_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CommandResult<Rule> {
    state
        .engine
        .set_rule_enabled(&id, enabled)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn rule_delete(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.engine.delete_rule(&id).await.map_err(text)
}

#[tauri::command]
pub async fn rule_run(state: State<'_, AppState>, id: String) -> CommandResult<JobSnapshot> {
    state.engine.run_rule(&id).await.map_err(text)
}

/// Asks for the data package file.
#[tauri::command]
pub async fn package_pick(app: AppHandle) -> CommandResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose your Discord data package")
            .add_filter("Discord data package", &["zip"])
            .blocking_pick_file()
    })
    .await
    .map_err(text)?;
    Ok(picked
        .and_then(|file| file.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn package_import(
    state: State<'_, AppState>,
    account: Snowflake,
    path: String,
) -> CommandResult<PackageIndex> {
    state
        .engine
        .import_package(account, PathBuf::from(path))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn package_get(
    state: State<'_, AppState>,
    account: Snowflake,
) -> CommandResult<Option<PackageIndex>> {
    state.engine.package(account).await.map_err(text)
}

#[tauri::command]
pub async fn package_forget(state: State<'_, AppState>, account: Snowflake) -> CommandResult<()> {
    state.engine.forget_package(account).await.map_err(text)
}

#[tauri::command]
pub fn bot_status(state: State<'_, AppState>) -> BotStatus {
    state.bot.status()
}

#[tauri::command]
pub async fn bot_set_token(state: State<'_, AppState>, token: String) -> CommandResult<BotStatus> {
    let secret = Secret::from_pasted(&token).map_err(text)?;
    let status = state.bot.set_token(secret.clone()).await.map_err(text)?;
    state.secrets.save(BOT_KEY, &secret)?;
    Ok(status)
}

#[tauri::command]
pub async fn bot_clear_token(state: State<'_, AppState>) -> CommandResult<BotStatus> {
    let status = state.bot.clear_token().await.map_err(text)?;
    state.secrets.delete(BOT_KEY)?;
    Ok(status)
}

#[tauri::command]
pub async fn bot_configure(
    state: State<'_, AppState>,
    replace_foreign: bool,
) -> CommandResult<BotStatus> {
    state.bot.configure(replace_foreign).await.map_err(text)
}

#[tauri::command]
pub async fn bot_set_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<BotStatus> {
    state.bot.set_enabled(enabled).await.map_err(text)
}

#[tauri::command]
pub async fn bot_set_command(
    state: State<'_, AppState>,
    key: String,
    enabled: bool,
) -> CommandResult<BotStatus> {
    state
        .bot
        .set_command_enabled(&key, enabled)
        .await
        .map_err(text)
}

/// Gives the demo a bot to explore.
#[tauri::command]
pub async fn demo_bot_setup(state: State<'_, AppState>) -> CommandResult<BotStatus> {
    if !state.demo {
        return Err("This is only available in the demo.".to_owned());
    }
    state
        .bot
        .set_token(Secret::new(DEMO_BOT_TOKEN.to_owned()))
        .await
        .map_err(text)
}

/// Sends a pretend slash command through the real bot code. Only exists in demo mode.
#[tauri::command]
pub async fn demo_bot_command(state: State<'_, AppState>, subcommand: String) -> CommandResult<()> {
    if !state.demo {
        return Err("This is only available in the demo.".to_owned());
    }
    let account = state
        .engine
        .accounts()
        .into_iter()
        .next()
        .ok_or("Sign in to the demo first.")?;
    let application = state
        .bot
        .status()
        .application
        .ok_or("Set up the demo bot first.")?;
    let payload = json!({
        "id": "1",
        "application_id": application.id,
        "type": 2,
        "token": "demo",
        "context": 2,
        "user": { "id": account.id, "username": account.username, "global_name": account.display_name },
        "channel": { "id": "201", "type": 1, "recipients": [{ "id": "301", "username": "mira", "global_name": "Mira" }] },
        "data": { "name": "sweep", "type": 1, "options": [{ "name": subcommand, "type": 1, "options": [] }] },
    });
    state.bot.simulate(payload).await.map_err(text)
}

#[tauri::command]
pub async fn settings_all(state: State<'_, AppState>) -> CommandResult<Vec<SettingEntry>> {
    state.engine.store().settings().await.map_err(text)
}

#[tauri::command]
pub async fn setting_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> CommandResult<()> {
    // These belong to the engine and the bot, and are changed through their own commands.
    if key.starts_with("bot.") {
        return Err("That setting can't be changed directly.".to_owned());
    }
    state
        .engine
        .store()
        .set_setting(key, value)
        .await
        .map_err(text)
}

#[tauri::command]
pub fn autostart_get(app: AppHandle) -> CommandResult<bool> {
    app.autolaunch().is_enabled().map_err(text)
}

#[tauri::command]
pub fn autostart_set(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    if state.demo {
        return Err("The demo doesn't change how your computer starts.".to_owned());
    }
    if enabled {
        app.autolaunch().enable().map_err(text)
    } else {
        app.autolaunch().disable().map_err(text)
    }
}

/// Lets the user choose where archives go.
#[tauri::command]
pub async fn archive_folder_pick(app: AppHandle) -> CommandResult<Option<String>> {
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title("Choose where to save archives")
            .blocking_pick_folder()
    })
    .await
    .map_err(text)?;
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(None);
    };

    let state = app.state::<AppState>();
    state
        .engine
        .store()
        .set_setting(
            "archiveFolder".to_owned(),
            path.to_string_lossy().into_owned(),
        )
        .await
        .map_err(text)?;
    state.engine.set_archive_root(path.clone());
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Opens a link in the browser, if it goes somewhere Disweep has a reason to send people.
#[tauri::command]
pub fn open_link(app: AppHandle, url: String) -> CommandResult<()> {
    if !ALLOWED_LINKS.iter().any(|allowed| url.starts_with(allowed)) {
        return Err("Disweep won't open that link.".to_owned());
    }
    app.opener().open_url(url, None::<&str>).map_err(text)
}

fn is_inside(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// Opens an archive folder, and only an archive folder.
#[tauri::command]
pub async fn open_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<()> {
    let target = PathBuf::from(&path);
    let root = state.engine.archive_root();
    let is_job_archive = state
        .engine
        .history(500)
        .await
        .map_err(text)?
        .iter()
        .any(|job| job.archive_path.as_deref() == Some(path.as_str()));
    if !is_job_archive && !is_inside(&target, &root) {
        return Err("That isn't one of Disweep's archive folders.".to_owned());
    }
    app.opener().open_path(path, None::<&str>).map_err(text)
}

/// Stops everything and erases what Disweep has stored on this computer.
#[tauri::command]
pub async fn wipe_local_data(state: State<'_, AppState>) -> CommandResult<()> {
    state.engine.stop_all();
    for account in state.engine.accounts() {
        state.engine.sign_out(account.id).await.map_err(text)?;
        state.secrets.delete(&account_key(account.id))?;
    }
    state.bot.clear_token().await.map_err(text)?;
    state.secrets.delete(BOT_KEY)?;
    state.engine.store().wipe().await.map_err(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_limited_to_known_sites() {
        let allowed = |url: &str| ALLOWED_LINKS.iter().any(|prefix| url.starts_with(prefix));
        assert!(allowed("https://discord.com/developers/applications"));
        assert!(allowed("https://github.com/kaanxsrd/Disweep/issues"));
        assert!(!allowed("https://discord.com.evil.example/"));
        assert!(!allowed("https://github.com/someone-else/malware"));
        assert!(!allowed("file:///C:/Windows/System32/calc.exe"));
    }

    #[test]
    fn paths_must_really_be_inside_the_root() {
        let root = std::env::temp_dir().join(format!("disweep-path-test-{}", uuid::Uuid::new_v4()));
        let inner = root.join("job");
        std::fs::create_dir_all(&inner).unwrap();

        assert!(is_inside(&inner, &root));
        assert!(
            !is_inside(&root.join("job/../.."), &root),
            "traversal out of the root"
        );
        assert!(!is_inside(&root.join("does-not-exist"), &root));

        std::fs::remove_dir_all(root).unwrap();
    }
}
