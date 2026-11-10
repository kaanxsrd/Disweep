mod client;
mod models;
mod ratelimit;
mod transport;

pub use client::{
    AuthScheme, DeleteOutcome, Discord, Observer, SearchOutcome, SearchPage, SearchQuery,
    SearchScope,
};
pub use models::{
    Application, Attachment, Channel, Guild, Message, User, avatar_url, channel_icon_url,
    guild_icon_url, is_deletable_type,
};
pub use ratelimit::{RateLimiter, Throttle};
pub use transport::{ApiRequest, ApiResponse, HttpTransport, Method, Transport, TransportError};

pub const API_BASE: &str = "https://discord.com/api/v10";
