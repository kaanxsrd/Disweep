# Contributing

Thanks for helping improve Disweep. This guide covers the local setup and the conventions the codebase follows.

## Setup

You need [Bun](https://bun.sh), [Rust](https://rustup.rs), and the [platform tools Tauri needs](https://tauri.app/start/prerequisites/): the Microsoft C++ Build Tools on Windows, or Xcode Command Line Tools on macOS.

```sh
git clone https://github.com/kaanxsrd/Disweep && cd Disweep
bun install
bun run demo        # the real app against a simulated Discord, with no account
```

On Windows 11 with Smart App Control on, Rust builds fail with `An Application Control policy has blocked this file`. Cargo compiles and runs small unsigned programs while building, and Smart App Control blocks them. Building needs it turned off, in **Windows Security** > **App & browser control** > **Smart App Control settings**.

## How the code fits together

- **`crates/disweep-core` is the engine** and knows nothing about windows. It talks to Discord through one `Transport` trait, which is the only thing swapped out in tests and demo mode.
- **`filter.rs` has the last word.** Every candidate passes `CompiledFilter::evaluate` right before deletion, whichever scanner found it. That check is what guarantees Disweep only deletes your own messages, so it must never be bypassed.
- **`preview.rs` only reads.** A preview must never be able to delete anything.
- **Scanners move a cursor backwards through message IDs** and never use a search `offset`. Offsets shift as messages are deleted and stop at 9,975.
- **`crates/disweep-bot` is a remote control.** Discord doesn't let bots delete a person's DM messages, so personal commands drive the engine with the signed-in account. Every command and button re-checks who is asking.
- **`src-tauri` is a thin shell.** The interface can only call the typed commands in `commands.rs`, and has no direct access to files, the network or the keychain.
- **Rust types are the source of truth for the interface.** `src/lib/bindings` is generated from them with `bun run bindings`. Don't edit those files by hand; CI fails if they drift.

## Before you open a pull request

```sh
bun run check
```

This must pass: Biome, TypeScript, the interface tests, rustfmt, Clippy and the Rust tests. CI runs the same checks on Windows and macOS.

Keep tests offline, and never add tokens or real Discord data to fixtures. Live testing must use a disposable account and never one you care about.

## Conventions

- **Commits** follow [Conventional Commits](https://www.conventionalcommits.org): `feat(engine): ...`, `fix(bot): ...`, `docs: ...`.
- **Safety:** preserve the preview, the typed confirmation, the author check before every delete and the owner check on every bot command. Include a regression test when fixing deletion logic.
- **Security:** never log tokens, interaction tokens, raw API responses or message content.
- **Rate limits:** never hard-code Discord's limits or skip a `Retry-After`. Adding delays to get past Discord's automated detection is out of scope.
- **Wording:** interface text is short, plain and describes what happens, not how it's implemented.
- **Changelog:** add user-facing changes under `[Unreleased]` in `CHANGELOG.md`. That section becomes the release notes.

## Releasing

One version number lives in the root `Cargo.toml`, under `[workspace.package]`.

1. Describe the changes under `[Unreleased]` in `CHANGELOG.md` and commit.
2. Once CI passes, cut the release and push it:

   ```sh
   bun scripts/release.ts minor    # or patch, major, or an explicit X.Y.Z
   git push --follow-tags
   ```

The script updates the version, dates the changelog section, and creates the commit and tag. `.github/workflows/release.yml` then runs the tests, builds and smoke-tests Windows and macOS on x64 and arm64, and publishes a GitHub Release with the installers, `checksums.txt` and the changelog notes. It refuses a tag that disagrees with the code or has no changelog entry.

Two optional repository secrets also update the package managers. Without them those steps are skipped:

| Secret               | Grants                                 |
| -------------------- | -------------------------------------- |
| `HOMEBREW_TAP_TOKEN` | Push access to `kaanxsrd/homebrew-tap` |
| `SCOOP_BUCKET_TOKEN` | Push access to `kaanxsrd/scoop-bucket` |
