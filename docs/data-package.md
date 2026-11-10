# Data packages

Discord can send you a data package listing every message you've ever sent. Importing it lets Disweep reach messages no scan can find: **closed DMs**, **servers you've left**, and anything Discord's search has lost track of.

## Getting your package

1. In Discord, open **User Settings**, then **Data & Privacy**.
2. Choose **Request Data**, include **Messages**, and confirm.
3. Discord emails you a link when it's ready, which can take several days.

## Importing it

Open **Data package**, choose the `package.zip`, and Disweep lists every conversation in it with its message count. Select what you want and continue to the usual review.

Disweep handles packages of any age, including older ones that use CSV files and packages with folder names translated into your account's language.

The package stays where you saved it. Disweep remembers where it is and reads it again when a sweep needs it, so don't move it while a sweep is using it.

## What to expect

- Messages are deleted directly by ID, so no searching is involved.
- Messages you've already deleted are counted as **already gone**, not as failures.
- In a server you've left, Discord refuses the delete, so the first refusal is reported as failed and the rest of that channel is skipped. Rejoin the server to delete them.
- A package only works with the account it belongs to. Disweep refuses one from a different account.
- Filters that depend on message details work on what the package records, which is the text and attachments. It doesn't record whether a message is pinned, so **pinned messages can't be kept** when sweeping from a package.
