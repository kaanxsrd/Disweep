# Sweeping

A sweep has three steps: choose what to delete, review exactly what would go, then confirm. Nothing changes until you type the confirmation.

## Before you start

- A sweep is permanent. Discord has no undo, and neither does Disweep.
- Automating a personal account isn't allowed by [Discord's Terms of Service](https://discord.com/terms), and Discord can suspend accounts for it. You accept that risk by using Disweep. Try a disposable account first if you're unsure.
- Run `disweep --demo` to learn the app with made-up data and no account.

## Choosing what to sweep

Pick any mix of DMs, group DMs, channels and whole servers. They run one after another as one job.

To include threads and forum posts, sweep the whole server rather than a single channel.

| Filter                   | What it does                                                                      |
| ------------------------ | --------------------------------------------------------------------------------- |
| When                     | Any time, older than, newer than, or between two dates. The end date is included  |
| Containing               | Only messages with this text, ignoring case                                       |
| Keep messages containing | Messages with any of these words are left alone. Separate them with commas        |
| With any of              | Only messages with a link, file, image, video, sound, embed, sticker or poll      |
| Include pinned messages  | Off by default, so pinned messages are kept                                       |
| Regular expression       | Only messages matching this pattern, ignoring case                                |
| From and to message ID   | A range between two messages, which are included. Copy IDs with Developer Mode on |
| Stop after               | Delete at most this many                                                          |

## How messages are found

**Automatic** is right almost every time.

| Method       | Used for                                  | Trade-off                                                    |
| ------------ | ----------------------------------------- | ------------------------------------------------------------ |
| Full history | DMs, unless a filter makes search cheaper | Exact and immune to search problems, but reads every message |
| Search       | Servers, and filtered DMs                 | Fast in busy places, but only finds what Discord has indexed |

**Thorough** runs a search and then reads the full history as well, catching anything search missed. Use it if a sweep leaves something behind in a server channel. It's slow in very busy channels.

If Discord's search isn't available for a DM, Disweep switches to full history by itself and says so.

## What is never deleted

- **Other people's messages.** Each message's author is checked immediately before it is deleted, even if Discord's search returned it.
- **Pinned messages**, unless you include them.
- **System messages**, such as call notices and "added to the group", which Discord doesn't allow to be deleted.

## While it runs

You can pause, resume or stop at any time. What has been deleted stays deleted.

Disweep waits between deletes and slows down further whenever Discord asks it to. **Careful**, **Balanced** and **Fast** in Advanced set how quickly it starts out. Discord's limits apply whichever you choose, so a large sweep takes a long time: expect roughly one message a second.

Disweep runs one sweep per account at a time. Others wait their turn.

## If a sweep is interrupted

Progress is saved as it goes. If the app closes or the computer restarts, the sweep appears in **Activity** as interrupted, and **Continue** picks it up from the last checkpoint.

## Saving a copy first

Turn on **Save a copy first** to keep a readable `transcript.html` and a complete `messages.jsonl` on your computer. Each message is written to disk before its delete request is sent, and if the copy can't be written the sweep stops rather than deleting something you asked to keep.

**Include attachments** also downloads files and images from Discord's servers.

Archives are saved in `Documents/Disweep Archives`. You can change the folder in Settings.

## Autosweep

A rule deletes your messages in chosen conversations once they pass a certain age, and checks on a schedule you set. Rules only run while Disweep is open or in your tray, so turn on **Start with my computer** in Settings if you want them to keep working.

Rules use the same safety checks as any other sweep.
