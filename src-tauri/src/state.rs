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

    pub told_about_tray: AtomicBool,

    pub restored: AtomicBool,
}
