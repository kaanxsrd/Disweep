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

pub fn init_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[must_use]
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
