//! The few command-line flags Disweep has, mainly for proving a build works.

use std::sync::Arc;

use disweep_core::Secret;
use disweep_core::api::Method;
use disweep_core::demo::{DEMO_TOKEN, FakeDiscord};
use disweep_core::engine::{Engine, JobOptions, JobOrigin, JobSpec, Scope, Target};
use disweep_core::filter::Filters;
use disweep_core::store::Store;

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Run against the built-in simulated Discord.
    pub demo: bool,
    /// Started at login, so stay in the tray.
    pub background: bool,
}

pub enum Action {
    Launch(Options),
    Exit(i32),
}

const HELP: &str = "Disweep

USAGE:
  disweep [--demo]

OPTIONS:
  --demo        Explore the app with a simulated Discord and no account
  --check       Test this installation without contacting Discord
  --version     Print the version
  --help        Print this help
";

pub fn parse() -> Action {
    let mut options = Options::default();
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--demo" => options.demo = true,
            "--background" => options.background = true,
            "--version" | "-V" => {
                attach_console();
                println!("Disweep {}", env!("CARGO_PKG_VERSION"));
                return Action::Exit(0);
            }
            "--help" | "-h" => {
                attach_console();
                print!("{HELP}");
                return Action::Exit(0);
            }
            "--check" => {
                attach_console();
                return Action::Exit(check());
            }
            other => {
                attach_console();
                eprintln!("Unknown option: {other}\n\n{HELP}");
                return Action::Exit(2);
            }
        }
    }
    Action::Launch(options)
}

/// Runs a complete sweep against the simulated Discord, proving the engine, database and TLS
/// provider all work in this build. Nothing here touches the network or the user's data.
fn check() -> i32 {
    match run_check() {
        Ok(()) => {
            println!("Disweep {}", env!("CARGO_PKG_VERSION"));
            println!("Installation OK");
            0
        }
        Err(problem) => {
            eprintln!("Installation problem: {problem}");
            1
        }
    }
}

fn run_check() -> Result<(), String> {
    disweep_core::init_tls();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let fake = Arc::new(FakeDiscord::new());
        fake.set_me(1, "Check");
        fake.add_dm(5, 2, "Someone");
        fake.add_message(5, 1 << 40, (1, "Check"), "yours");
        fake.add_message(5, (1 << 40) + 1, (2, "Someone"), "theirs");

        let store = Store::in_memory().map_err(|error| error.to_string())?;
        let engine = Engine::new(store, fake.clone(), std::env::temp_dir());
        engine.speed_up_for_demo(0.001);
        let account = engine
            .sign_in(&Secret::new(DEMO_TOKEN.to_owned()))
            .await
            .map_err(|error| error.to_string())?;
        let spec = JobSpec {
            account_id: account.id,
            targets: vec![Target {
                scope: Scope::Private {
                    channel_id: disweep_core::Snowflake(5),
                },
                label: "Check".to_owned(),
                expected: None,
            }],
            filters: Filters::default(),
            options: JobOptions::default(),
            origin: JobOrigin::App,
        };
        let job = engine
            .start(spec)
            .await
            .map_err(|error| error.to_string())?;

        for _ in 0..600 {
            if engine
                .snapshot(&job.id)
                .is_some_and(|snapshot| !snapshot.state.is_active())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let deleted = engine
            .snapshot(&job.id)
            .map_or(0, |snapshot| snapshot.counters.deleted);
        if deleted != 1 || fake.count_requests(&Method::DELETE) != 1 || fake.remaining(5).len() != 1
        {
            return Err(format!(
                "the test sweep deleted {deleted} messages instead of 1"
            ));
        }
        Ok(())
    })
}

/// A window app has no console, so borrow the terminal it was launched from.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole takes a plain constant, and failing just means there's no terminal.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
