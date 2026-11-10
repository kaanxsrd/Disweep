//! Every command the bot offers, and how each is registered with Discord.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::{Value, json};
use ts_rs::TS;

use crate::setup::MANAGE_MESSAGES;

/// Who a command acts for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CommandScope {
    /// Installed to your account, works everywhere, and only you can run it.
    Personal,
    /// Installed to a server, for members who can manage messages.
    Server,
}

/// One command a user can switch on or off.
pub struct CommandSpec {
    pub key: &'static str,
    pub display: &'static str,
    pub description: &'static str,
    pub scope: CommandScope,
    /// The name Discord knows the top-level command by.
    pub root: &'static str,
}

const fn personal(
    key: &'static str,
    display: &'static str,
    description: &'static str,
    root: &'static str,
) -> CommandSpec {
    CommandSpec {
        key,
        display,
        description,
        scope: CommandScope::Personal,
        root,
    }
}

pub const CTX_NEWER: &str = "Sweep this and newer";
pub const CTX_OLDER: &str = "Sweep this and older";

pub const COMMANDS: &[CommandSpec] = &[
    personal(
        "sweep.here",
        "/sweep here",
        "Delete your messages in the chat you're in",
        "sweep",
    ),
    personal(
        "sweep.dm",
        "/sweep dm",
        "Delete your messages in your DM with someone",
        "sweep",
    ),
    personal(
        "sweep.status",
        "/sweep status",
        "See how the current sweep is going",
        "sweep",
    ),
    personal(
        "sweep.pause",
        "/sweep pause",
        "Pause the current sweep",
        "sweep",
    ),
    personal(
        "sweep.resume",
        "/sweep resume",
        "Continue a paused sweep",
        "sweep",
    ),
    personal(
        "sweep.stop",
        "/sweep stop",
        "Stop the current sweep",
        "sweep",
    ),
    personal(
        "sweep.stats",
        "/sweep stats",
        "See how much you've swept",
        "sweep",
    ),
    personal(
        "autosweep.set",
        "/autosweep set",
        "Keep a chat clean automatically",
        "autosweep",
    ),
    personal(
        "autosweep.off",
        "/autosweep off",
        "Stop keeping this chat clean",
        "autosweep",
    ),
    personal(
        "autosweep.list",
        "/autosweep list",
        "List your automatic sweeps",
        "autosweep",
    ),
    personal(
        "menu.newer",
        CTX_NEWER,
        "Right-click a message to delete it and your newer ones",
        CTX_NEWER,
    ),
    personal(
        "menu.older",
        CTX_OLDER,
        "Right-click a message to delete it and your older ones",
        CTX_OLDER,
    ),
    CommandSpec {
        key: "purge",
        display: "/purge",
        description: "Bulk delete anyone's messages in a server, for moderators",
        scope: CommandScope::Server,
        root: "purge",
    },
];

/// Every top-level name Disweep registers, for spotting commands that aren't Disweep's.
#[must_use]
pub fn all_root_names() -> HashSet<&'static str> {
    COMMANDS.iter().map(|command| command.root).collect()
}

fn text(name: &str, description: &str) -> Value {
    json!({ "type": 3, "name": name, "description": description })
}

fn user(name: &str, description: &str) -> Value {
    json!({ "type": 6, "name": name, "description": description })
}

fn has_option() -> Value {
    let choices: Vec<Value> = ["link", "file", "image", "video", "embed", "sticker", "poll"]
        .iter()
        .map(|kind| json!({ "name": kind, "value": kind }))
        .collect();
    json!({ "type": 3, "name": "has", "description": "Only messages with this", "choices": choices })
}

fn filter_options() -> Vec<Value> {
    vec![
        text("contains", "Only messages containing this text"),
        text(
            "older_than",
            "Only messages older than this, like 30d or 2h",
        ),
        text("newer_than", "Only messages newer than this, like 7d or 1h"),
        has_option(),
        json!({ "type": 4, "name": "limit", "description": "Delete at most this many", "min_value": 1, "max_value": 1_000_000 }),
        json!({ "type": 5, "name": "include_pinned", "description": "Also delete pinned messages" }),
    ]
}

fn subcommand(key: &str, options: Vec<Value>) -> (String, Value) {
    let spec = COMMANDS.iter().find(|command| command.key == key);
    let name = key.rsplit('.').next().unwrap_or(key).to_owned();
    let description = spec.map_or("", |spec| spec.description);
    let body = json!({ "type": 1, "name": name, "description": description, "options": options });
    (key.to_owned(), body)
}

/// Options on personal commands: installed to the user, usable in servers, the bot's DM and any
/// other DM or group DM.
fn personal_contexts() -> Value {
    json!({ "integration_types": [1], "contexts": [0, 1, 2] })
}

fn merged(mut base: Value, extra: &Value) -> Value {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    base
}

/// The JSON for Discord's bulk overwrite, containing only what is enabled.
#[must_use]
pub fn registration_payload(enabled: &HashSet<String>) -> Value {
    let mut dm_options = vec![
        json!({ "type": 6, "name": "user", "description": "Whose DM to sweep", "required": true }),
    ];
    dm_options.extend(filter_options());

    let mut groups: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    let subcommands = [
        ("sweep", subcommand("sweep.here", filter_options())),
        ("sweep", subcommand("sweep.dm", dm_options)),
        ("sweep", subcommand("sweep.status", vec![])),
        ("sweep", subcommand("sweep.pause", vec![])),
        ("sweep", subcommand("sweep.resume", vec![])),
        ("sweep", subcommand("sweep.stop", vec![])),
        ("sweep", subcommand("sweep.stats", vec![])),
        (
            "autosweep",
            subcommand(
                "autosweep.set",
                vec![
                    json!({ "type": 3, "name": "older_than", "description": "Delete your messages older than this, like 30d", "required": true }),
                    text(
                        "every",
                        "How often to check, like 1d. Default is once a day",
                    ),
                ],
            ),
        ),
        ("autosweep", subcommand("autosweep.off", vec![])),
        ("autosweep", subcommand("autosweep.list", vec![])),
    ];
    for (root, (key, body)) in subcommands {
        if enabled.contains(&key) {
            groups.entry(root).or_default().push(body);
        }
    }

    let contexts = personal_contexts();
    let mut commands = Vec::new();
    if let Some(options) = groups.remove("sweep") {
        commands.push(merged(
            json!({ "name": "sweep", "type": 1, "description": "Delete your own messages", "options": options }),
            &contexts,
        ));
    }
    if let Some(options) = groups.remove("autosweep") {
        commands.push(merged(
            json!({ "name": "autosweep", "type": 1, "description": "Keep a chat clean automatically", "options": options }),
            &contexts,
        ));
    }
    for (key, name) in [("menu.newer", CTX_NEWER), ("menu.older", CTX_OLDER)] {
        if enabled.contains(key) {
            commands.push(merged(json!({ "name": name, "type": 3 }), &contexts));
        }
    }
    if enabled.contains("purge") {
        commands.push(json!({
            "name": "purge",
            "type": 1,
            "description": "Bulk delete messages in this channel",
            "integration_types": [0],
            "contexts": [0],
            "default_member_permissions": MANAGE_MESSAGES.to_string(),
            "options": [
                { "type": 4, "name": "count", "description": "How many messages to delete", "required": true, "min_value": 1, "max_value": 1000 },
                user("user", "Only this person's messages"),
                text("contains", "Only messages containing this text"),
                has_option(),
                { "type": 5, "name": "include_old", "description": "Also delete messages over two weeks old, which is much slower" },
            ],
        }));
    }
    Value::Array(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_enabled() -> HashSet<String> {
        COMMANDS
            .iter()
            .map(|command| command.key.to_owned())
            .collect()
    }

    fn payload(enabled: &HashSet<String>) -> Vec<Value> {
        registration_payload(enabled).as_array().unwrap().clone()
    }

    fn by_name<'a>(commands: &'a [Value], name: &str) -> &'a Value {
        commands
            .iter()
            .find(|command| command["name"] == name)
            .unwrap()
    }

    #[test]
    fn personal_commands_work_in_any_dm() {
        let commands = payload(&all_enabled());
        for name in ["sweep", "autosweep", CTX_NEWER, CTX_OLDER] {
            let command = by_name(&commands, name);
            assert_eq!(command["integration_types"], json!([1]), "{name}");
            assert_eq!(command["contexts"], json!([0, 1, 2]), "{name}");
        }
    }

    #[test]
    fn purge_is_limited_to_server_moderators() {
        let commands = payload(&all_enabled());
        let purge = by_name(&commands, "purge");
        assert_eq!(purge["integration_types"], json!([0]));
        assert_eq!(purge["contexts"], json!([0]));
        assert_eq!(purge["default_member_permissions"], "8192");
    }

    #[test]
    fn only_enabled_commands_are_registered() {
        let enabled: HashSet<String> = ["sweep.here".to_owned()].into_iter().collect();
        let commands = payload(&enabled);
        assert_eq!(commands.len(), 1);
        let subcommands = commands[0]["options"].as_array().unwrap();
        assert_eq!(subcommands.len(), 1);
        assert_eq!(subcommands[0]["name"], "here");
    }

    #[test]
    fn a_group_with_nothing_enabled_is_left_out() {
        assert!(payload(&HashSet::new()).is_empty());
    }

    #[test]
    fn meets_discords_naming_and_length_rules() {
        fn check(command: &Value) {
            let description = command["description"].as_str().unwrap_or("");
            assert!(
                description.chars().count() <= 100,
                "too long: {description}"
            );
            if let Some(options) = command["options"].as_array() {
                assert!(options.len() <= 25);
                let mut seen_optional = false;
                for option in options {
                    let name = option["name"].as_str().unwrap();
                    assert!(
                        name.len() <= 32
                            && name.chars().all(|c| c.is_ascii_lowercase()
                                || c.is_ascii_digit()
                                || c == '_'
                                || c == '-'),
                        "bad name {name}"
                    );
                    assert!(!option["description"].as_str().unwrap().is_empty());
                    let required = option["required"].as_bool().unwrap_or(false);
                    assert!(
                        !(required && seen_optional),
                        "required options come first: {name}"
                    );
                    seen_optional |= !required;
                    check(option);
                }
            }
        }
        for command in payload(&all_enabled()) {
            assert!(command["name"].as_str().unwrap().chars().count() <= 32);
            check(&command);
        }
    }

    #[test]
    fn every_listed_command_registers_something() {
        for spec in COMMANDS {
            let enabled: HashSet<String> = [spec.key.to_owned()].into_iter().collect();
            assert_eq!(
                payload(&enabled).len(),
                1,
                "{} produced no registration",
                spec.key
            );
        }
    }
}
