# Leftover

Where Disweep stands, and what to pick up next. Written 2026-11-10. Delete this file once it's all done.

## Where it stands

Working and tested on Windows x64:

- The engine, the bot and the full interface are written.
- The production build works: a 9.3 MB `disweep.exe`, a 3.5 MB installer and a 4.4 MB portable zip, built with `bun scripts/build.ts`.
- 217 Rust tests and 50 interface and script tests pass. They run against an in-memory Discord.
- `bun run test:e2e` drives the real built app through 18 flows in demo mode: sign-in, choosing, preview, typed confirmation, live progress, pause and resume, Activity, Autosweep, Bot and Settings.
- Checked against Discord's real servers with a fake token and no account: TLS, the REST client, the gateway handshake, and the rejected-token path. The sign-in window was also tested on the real login page, up to Discord rejecting the fake token.

## Not verified yet

Read these before trusting it with an account you care about.

- **It has never run against a real Discord account.** Search, delete and listing DMs have only been tested against the simulated Discord. `GET /channels/{id}/messages/search` and `GET /users/@me/channels` aren't in Discord's public docs, so they're the most likely to need adjusting.
- **Signing in has never completed with a real account.** The capture hook and the cancelled navigation work, but a full login hasn't been done, and saving to the keychain has never run for real.
- **The bot has never connected with a real application.** Automatic setup, command registration and running `/sweep` in a real DM are all untested. `PATCH /applications/@me` for User Install isn't fully documented, so setup may ask you to tick it in the Developer Portal.
- **macOS has never been built or run,** and neither has Windows on arm64. CI builds both on every push to `main`, so check the Actions tab first.
- **`bun run demo`** (dev mode with hot reload) hasn't been run. The production build with `--demo` has.
- **Automating a personal account is against Discord's Terms of Service.** Discord can suspend accounts for it.

## Known issue

`bun run test:e2e` failed once in about eight runs, at "sets up the bot and runs a command": the "Use the demo bot" button wasn't found. It wasn't reproduced in four more runs, and the cause isn't known. My guess is the scheduler starting the Autosweep rule the test just created. The test now saves a screenshot when a step fails, and prints where.

Also worth adding: an error boundary around the interface. Right now a render error would leave a blank window.

## Next steps, in order

1. **Check CI** in the Actions tab and fix anything macOS-specific. Private repos use paid Actions minutes, and macOS runners count 10x, so watch your allowance. Arm64 Windows runners may not be available on a private repo. If so, drop `windows-11-arm` from the matrix in both workflows.
2. **Try a throwaway Discord account.** Run a small DM sweep with **Save a copy first** on, and fix whatever the real API does differently.
3. **Try the bot.** Follow `docs/bot.md` with a new application and run `/sweep here` in a DM with a friend.
4. **Update the links if the repo name changes.** The docs, `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs` and `scripts/package-managers.ts` assume `kaanxsrd/Disweep`.
5. **Cut the first release.** The version is already 1.0.0, so `release.ts` will refuse to bump to it. For the first one, add `## [1.0.0] - YYYY-MM-DD` under `## [Unreleased]` in `CHANGELOG.md`, commit, then:

   ```sh
   git tag -a v1.0.0 -m "Disweep 1.0.0"
   git push --follow-tags
   ```

   After that, `bun scripts/release.ts patch` works as `CONTRIBUTING.md` describes.
6. **For Homebrew and Scoop,** add the `HOMEBREW_TAP_TOKEN` and `SCOOP_BUCKET_TOKEN` repository secrets. Without them those steps are skipped.

## Ideas not built

- Translations. CloneX has 42 languages, and Disweep is English only.
- Code signing and macOS notarization, so people don't get security warnings.
- A log file. Warnings currently go nowhere, which will make real-world bugs harder to report.
- In-app update checks.

## Picking it back up

On a new machine you need [Bun](https://bun.sh), [Rust](https://rustup.rs), and Microsoft C++ Build Tools on Windows or Xcode Command Line Tools on macOS.

```sh
bun install
bun run build:ui          # the Rust build embeds this
bun run check             # every linter and test
bun run tauri build --no-bundle
./target/release/disweep --demo
bun run test:e2e          # Windows only, needs the build above
```

Use `bun run <tool>` or `./node_modules/.bin/<tool>` for project tools, not `npx`. On Windows, `npx biome` and `npx tsc` download unrelated packages with those names.
