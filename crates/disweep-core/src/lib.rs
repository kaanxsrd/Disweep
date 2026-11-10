//! The Disweep sweep engine.
//!
//! Everything that talks to Discord or touches persisted state lives here, with no dependency on
//! the desktop shell. That keeps the rules that matter (only your own messages, every limit
//! honoured, nothing deleted without a confirmed job) testable without a window or a network.

pub mod api;
pub mod archive;
pub mod demo;
pub mod engine;
pub mod error;
pub mod filter;
pub mod package;
pub mod scan;
pub mod secret;
pub mod snowflake;
pub mod store;

pub use error::ApiError;
pub use secret::Secret;
pub use snowflake::Snowflake;

/// Installs the process-wide TLS provider. Call once, before the first network request.
pub fn init_tls() {
    // A second install returns Err, which only means another caller got there first.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// The current time as milliseconds since the Unix epoch.
#[must_use]
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
