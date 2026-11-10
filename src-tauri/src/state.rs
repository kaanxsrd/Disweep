//! What the running app holds on to.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use disweep_bot::BotRuntime;
use disweep_core::engine::Engine;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::secrets::Secrets;

pub struct AppState {
    pub engine: Engine,
    pub bot: BotRuntime,
    pub secrets: Secrets,
    pub demo: bool,
    pub start_hidden: bool,
    pub previews: Mutex<HashMap<String, CancellationToken>>,
    /// True once the app has explained that it keeps running in the tray.
    pub told_about_tray: AtomicBool,
    /// True once saved accounts are back, so the interface knows whether to ask for a sign-in.
    pub restored: AtomicBool,
}
