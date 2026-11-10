# Security policy

Disweep handles Discord account tokens and permanently deletes messages. We take reports seriously.

## Reporting a vulnerability

Please **do not open a public issue**. Report privately through GitHub's [private vulnerability reporting](https://github.com/kaanxsrd/Disweep/security/advisories/new) for this repository.

Include:

- what is affected (screen, command or module, and version or commit),
- steps to reproduce or a proof of concept, using sanitized or synthetic data,
- the impact you expect.

Never include real tokens or private messages.

## What to expect

- Acknowledgement within 3 business days.
- An assessment and a planned fix date within 10 business days.
- Credit in the advisory and changelog when the fix ships, unless you prefer to stay anonymous.

Please give us a reasonable time to fix the issue before disclosing it publicly.

## Scope

In scope: this repository's code and its release builds.
Out of scope: Discord itself, account actions Discord takes against automated accounts, and findings that need a compromised machine or account.

## Verifying a download

Release builds are made by this repository's release workflow from the tagged source, and each release includes `checksums.txt`. Only run a file whose hash matches: a modified build could steal the token you give it.

## How credentials are handled

Treat an account token as a password. If one is exposed, change your Discord password, which revokes it. Removing a token from a file does not remove it from existing commits, forks, logs or screenshots.

- Account and bot tokens are stored only in your operating system's credential store (Windows Credential Manager or macOS Keychain). They are never written to Disweep's database, settings or logs, and they are never sent to the interface after you enter them.
- Tokens are only ever sent to `discord.com`.
- **Sign in with Discord** opens Discord's real login page in a private window that has no access to Disweep's commands and can only navigate within `discord.com`. Once you've signed in, a script in that window reads the session token from Discord's own requests and passes it to the app through a navigation that is cancelled before it leaves the computer.
- The interface runs under a strict content security policy and can only call Disweep's own commands. It has no direct access to files, the network or the credential store.

## Operational safety

- A sweep is permanent. Discord has no undo.
- Disweep deletes only messages written by the signed-in account, checked immediately before each delete. `/purge` is the deliberate exception: it uses the bot's own permissions in a server, and needs both the person running it and the bot to hold Manage Messages.
- The bot's personal commands only work for the account signed in to that copy of Disweep. Discord identifies who ran a command, and that identity is checked on every command and every confirmation button.
