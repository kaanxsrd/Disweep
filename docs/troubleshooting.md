# Troubleshooting

## Windows protected your PC, or macOS won't open the app

Releases are not code-signed. On Windows, choose **More info** and then **Run anyway**. On macOS, clear the quarantine flag:

```sh
xattr -dr com.apple.quarantine /Applications/Disweep.app
```

Compare the file with `checksums.txt` from the same release first. If the hash doesn't match, delete it and don't run it.

## The Windows version won't start

Disweep uses Microsoft Edge WebView2, which is part of Windows 11 and nearly all Windows 10 systems. The installer adds it if it's missing. If you're using the portable `.exe`, install the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).

## Discord rejected this login

The token is out of date, usually because the account's password changed or the session was logged out. Sign in again from the account menu.

## Discord wants this account verified

Open Discord and complete the check it's showing, such as confirming an email or phone number. Then choose **Continue** on the sweep in **Activity**.

## A sweep is going slowly

That's expected. Discord limits how quickly messages can be deleted, roughly one a second, and older messages can be slower still. Disweep follows those limits exactly. **Waiting for Discord** means it was asked to pause and will carry on by itself.

## "Stopped to protect your connection"

Discord temporarily bans IP addresses that make too many refused requests. Disweep stops well before that point. Wait 10 minutes and choose **Continue** in **Activity**.

## Some messages were left behind

- In a server channel, sweep it again with **Thorough** on, which also reads the full history.
- Messages in an archived thread can't be deleted, and are listed as failed. Reopen the thread first.
- Open **Activity** and the sweep's details to see what failed and why.

## "Discord's search isn't ready"

Discord builds its search index on demand, and a server it hasn't indexed yet can take a while. Try again in a few minutes.

## The preview says "about"

Some filters, such as regular expressions, can only be checked on the messages themselves, so the count is estimated from a sample. The sweep itself checks every message.

## Slash commands don't appear

- Run setup again on the **Bot** screen and check it reports no problems.
- Make sure you used **Add to my account**.
- Discord can take a moment to update. Restarting Discord refreshes the list.

## The application did not respond

Disweep has to be running for commands to work. Check that it's open or in your tray, the Bot screen says **Online**, and you're signed in with the same account.

## The bot couldn't connect

The token may have been reset. On the Bot screen, remove it and add the current one.

## This application sends commands to a web address

The application has an **Interactions Endpoint URL**, and Discord sends commands there instead of to Disweep. Clear that field on the application's **General Information** page.

## This application has other commands

Setup replaces every command on an application. If you see a list of commands you want to keep, cancel and create a separate application for Disweep.

## Checking an installation

`disweep --check` runs a complete test sweep against a simulated Discord without touching the network or your data, and prints the version. Include its output in bug reports. Never include a token.
