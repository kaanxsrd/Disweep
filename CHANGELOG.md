# Changelog

Every notable change to Disweep, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### 🚀 Highlights

- **A desktop app for Windows and macOS** that finds your Discord messages, shows exactly what would go, and deletes them only after a typed confirmation.
- **A Discord bot built in.** Add your own bot, then run `/sweep` inside any DM. Only you see its replies.
- **Sweeps that finish.** Disweep moves through message IDs instead of search offsets, so it doesn't stop early when Discord's search lags or runs past 10,000 results.

### ✨ Added

- Sweeping DMs, group DMs, single channels and whole servers, together in one job.
- Filters for date range, text, kept words, regular expressions, links, files, images, video, sound, embeds, stickers and polls, plus a range between two messages.
- A preview with a count, a time estimate and a sample, before anything can be deleted.
- Pause, resume and stop. Progress is saved, so a sweep can continue after a crash or restart.
- Discord data package import, which reaches closed DMs and servers you've left.
- Saving a readable copy of messages and their attachments before deleting them.
- Autosweep rules that keep a conversation clean automatically.
- Bot commands: `/sweep here`, `/sweep dm`, `/sweep status`, `/sweep pause`, `/sweep resume`, `/sweep stop`, `/sweep stats`, `/autosweep`, `/purge`, and message menu items that sweep from a chosen message.
- Sign-in through Discord's own page, or with a token. Logins are kept in the operating system's credential store.
- Several accounts at once, a tray icon, start with the computer, notifications, light and dark themes, and a streamer mode that blurs names and messages.
- A `--demo` mode that runs the real app against a simulated Discord, with no account.
- An offline test suite that runs the real engine and bot against an in-memory Discord, and CI on Windows and macOS.
