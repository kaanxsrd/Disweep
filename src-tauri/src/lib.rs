//! The desktop app: windows, the tray, and the bridge between the interface and the engine.

mod cli;
mod commands;
mod login;
mod secrets;
mod state;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use disweep_bot::{BotError, BotRuntime};
use disweep_core::api::{API_BASE, HttpTransport, Transport};
use disweep_core::demo::FakeDiscord;
use disweep_core::engine::{Engine, EngineEvent, JobState};
use disweep_core::error::ApiError;
use disweep_core::store::Store;
use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::StateFlags;

use crate::cli::{Action, Options};
use crate::secrets::{BOT_KEY, Secrets, account_key};
use crate::state::AppState;

const USER_AGENT: &str = concat!(
    "Disweep/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/kaanxsrd/Disweep)"
);
const BOT_USER_AGENT: &str = concat!(
    "DiscordBot (https://github.com/kaanxsrd/Disweep, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);
const DEMO_PACE: f64 = 0.05;
const BOT_RESTORE_RETRY: Duration = Duration::from_secs(30);

/// Starts Disweep.
pub fn run() {
    let options = match cli::parse() {
        Action::Launch(options) => options,
        Action::Exit(code) => std::process::exit(code),
    };
    disweep_core::init_tls();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _directory| show_main_window(app),
        ))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // The window stays hidden until the interface has drawn.
                .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
                .with_denylist(&["discord-login"])
                .build(),
        )
        .setup(move |app| {
            setup(app, options)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && should_stay_in_tray(window.app_handle())
            {
                api.prevent_close();
                let _ = window.hide();
                tell_about_tray_once(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ui_ready,
            commands::app_info,
            commands::window_set_theme,
            commands::accounts_list,
            commands::account_add_token,
            commands::account_open_login,
            commands::account_remove,
            commands::demo_sign_in,
            commands::inventory,
            commands::guild_channels,
            commands::check_pattern,
            commands::preview_start,
            commands::preview_cancel,
            commands::job_start,
            commands::job_pause,
            commands::job_resume,
            commands::job_stop,
            commands::jobs_active,
            commands::jobs_history,
            commands::job_failures,
            commands::job_resume_finished,
            commands::job_forget,
            commands::stats,
            commands::rules_list,
            commands::rule_add,
            commands::rule_set_enabled,
            commands::rule_delete,
            commands::rule_run,
            commands::package_pick,
            commands::package_import,
            commands::package_get,
            commands::package_forget,
            commands::bot_status,
            commands::bot_set_token,
            commands::bot_clear_token,
            commands::bot_configure,
            commands::bot_set_enabled,
            commands::bot_set_command,
            commands::demo_bot_setup,
            commands::demo_bot_command,
            commands::settings_all,
            commands::setting_set,
            commands::autostart_get,
            commands::autostart_set,
            commands::archive_folder_pick,
            commands::open_link,
            commands::open_archive,
            commands::wipe_local_data,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("Disweep couldn't start: {error}");
        std::process::exit(1);
    }
}

fn setup(app: &mut App, options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let paths = app.path();
    let data_directory = paths.app_data_dir()?;
    let default_archive = paths
        .document_dir()
        .unwrap_or_else(|_| data_directory.clone())
        .join("Disweep Archives");

    let store = if options.demo {
        Store::in_memory()?
    } else {
        Store::open(&data_directory.join("disweep.db"))?
    };
    let transport: Arc<dyn Transport> = if options.demo {
        Arc::new(FakeDiscord::demo())
    } else {
        Arc::new(HttpTransport::new(API_BASE, USER_AGENT, BOT_USER_AGENT)?)
    };
    let engine = Engine::new(store, transport, default_archive);
    if options.demo {
        engine.speed_up_for_demo(DEMO_PACE);
    }

    let bot = tauri::async_runtime::block_on(BotRuntime::new(engine.clone()))?;
    app.manage(AppState {
        engine,
        bot,
        secrets: if options.demo {
            Secrets::Memory(Mutex::new(HashMap::new()))
        } else {
            Secrets::Keychain
        },
        demo: options.demo,
        start_hidden: options.background,
        previews: Mutex::new(HashMap::new()),
        told_about_tray: AtomicBool::new(false),
        restored: AtomicBool::new(false),
    });

    build_tray(app)?;
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        restore(&handle).await;
        forward_events(&handle);
    });
    Ok(())
}

/// Brings back what was running or signed in last time.
async fn restore(app: &AppHandle) {
    let state = app.state::<AppState>();
    let engine = state.engine.clone();

    if let Err(error) = engine.recover().await {
        tracing::warn!(%error, "couldn't check for interrupted sweeps");
    }
    if let Ok(Some(folder)) = engine.store().setting("archiveFolder".to_owned()).await {
        engine.set_archive_root(PathBuf::from(folder));
    }

    match engine.store().accounts().await {
        Ok(accounts) => {
            for account in accounts {
                match state.secrets.load(&account_key(account.id)) {
                    Ok(Some(token)) => engine.attach(account, &token),
                    Ok(None) => {}
                    Err(error) => tracing::warn!(%error, "couldn't read a saved login"),
                }
            }
        }
        Err(error) => tracing::warn!(%error, "couldn't list saved accounts"),
    }
    state.restored.store(true, Ordering::SeqCst);
    let _ = app.emit("accounts-restored", ());
    engine.start_scheduler();

    if let Ok(Some(token)) = state.secrets.load(BOT_KEY) {
        let bot = state.bot.clone();
        tauri::async_runtime::spawn(async move {
            // The network may not be up yet when starting with the computer.
            loop {
                match bot.set_token(token.clone()).await {
                    Ok(_) => {
                        bot.resume_if_enabled();
                        return;
                    }
                    Err(BotError::Api(
                        ApiError::Network(_)
                        | ApiError::Discord {
                            status: 500..=599, ..
                        },
                    )) => {
                        tokio::time::sleep(BOT_RESTORE_RETRY).await;
                    }
                    Err(error) => {
                        bot.report_problem(format!("The saved bot token stopped working: {error}"));
                        return;
                    }
                }
            }
        });
    }
}

/// Passes engine and bot updates on to the interface.
fn forward_events(app: &AppHandle) {
    let state = app.state::<AppState>();

    let mut engine_events = state.engine.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut announced = HashSet::new();
        loop {
            match engine_events.recv().await {
                Ok(event) => {
                    notify_if_finished(&handle, &event, &mut announced).await;
                    let _ = handle.emit("engine", event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    let mut bot_updates = state.bot.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match bot_updates.recv().await {
                Ok(status) => {
                    let _ = handle.emit("bot", status);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

/// Tells the user about a finished sweep, if they aren't looking at the app.
async fn notify_if_finished(app: &AppHandle, event: &EngineEvent, announced: &mut HashSet<String>) {
    let EngineEvent::Job { job } = event else {
        return;
    };
    if job.state.is_active() || !announced.insert(job.id.clone()) {
        return;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    let store = app.state::<AppState>().engine.store().clone();
    let enabled = store
        .setting("notifications".to_owned())
        .await
        .ok()
        .flatten()
        .as_deref()
        != Some("false");
    if focused || !enabled {
        return;
    }
    let body = match job.state {
        JobState::Completed => format!(
            "Deleted {} messages in {}.",
            job.counters.deleted, job.label
        ),
        JobState::Failed => format!(
            "{} stopped: {}",
            job.label,
            job.failure
                .as_ref()
                .map_or("unknown problem", |f| f.message.as_str())
        ),
        _ => format!(
            "{} stopped after {} messages.",
            job.label, job.counters.deleted
        ),
    };
    let _ = app
        .notification()
        .builder()
        .title("Disweep")
        .body(body)
        .show();
}

fn build_tray(app: &App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Disweep", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop all sweeps", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Disweep", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &stop, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Disweep")
        .menu(&menu)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "stop" => app.state::<AppState>().engine.stop_all(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Closing the window shouldn't silently cancel a sweep or take the bot offline.
fn should_stay_in_tray(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let engine = state.engine.clone();
    let wanted = tauri::async_runtime::block_on(async move {
        engine
            .store()
            .setting("closeToTray".to_owned())
            .await
            .ok()
            .flatten()
            .as_deref()
            != Some("false")
    });
    if !wanted {
        return false;
    }
    let engine = state.engine.clone();
    let has_rule = tauri::async_runtime::block_on(async move {
        engine
            .rules()
            .await
            .map(|rules| rules.iter().any(|rule| rule.enabled))
            .unwrap_or(false)
    });
    !state.engine.active_jobs().is_empty() || state.bot.status().enabled || has_rule
}

fn tell_about_tray_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.told_about_tray.swap(true, Ordering::SeqCst) {
        let _ = app
            .notification()
            .builder()
            .title("Disweep is still running")
            .body("It's in your tray so sweeps, automatic rules and the bot can keep working.")
            .show();
    }
}
