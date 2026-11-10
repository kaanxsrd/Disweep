//! The Disweep Discord bot.
//!
//! Discord gives bots no way to delete a person's messages in a DM, so the bot is a remote control
//! for the sweep engine. It is installed to the user's own account, which lets `/sweep` run inside
//! any conversation; the interaction tells this app which chat it was, and the engine deletes the
//! user's messages there with their signed-in account. Replies are ephemeral, so the other person
//! never sees any of it.
//!
//! Only the account signed in to this copy of Disweep can use the personal commands. `/purge` is
//! the exception: it uses the bot's own permissions in a server, for that server's moderators.

mod commands;
mod duration;
mod handler;
mod interaction;
mod runtime;
mod setup;
mod views;

pub use commands::{COMMANDS, CommandScope, registration_payload};
pub use duration::parse_duration;
pub use interaction::Interaction;
pub use runtime::{
    BotActivity, BotApplication, BotConnection, BotError, BotRuntime, BotStatus, CommandState,
    InstallLinks,
};
pub use setup::{install_links, required_guild_permissions};
