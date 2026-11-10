use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;
use zip::ZipArchive;

use crate::Snowflake;
use crate::api::Message;

const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("Couldn't open that file: {0}")]
    Io(#[from] std::io::Error),
    #[error("That file isn't a valid zip archive.")]
    NotZip(#[from] zip::result::ZipError),
    #[error("No messages were found. Make sure this is the package.zip Discord sent you.")]
    NoMessages,
    #[error("Part of the package couldn't be read: {0}")]
    Malformed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PackageChannelKind {
    Direct,
    Group,
    Server,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PackageChannel {
    pub id: Snowflake,
    pub kind: PackageChannelKind,
    pub name: String,
    pub guild_id: Option<Snowflake>,
    pub guild_name: Option<String>,
    pub message_count: u64,
    pub first_message_at: Option<i64>,
    pub last_message_at: Option<i64>,
    #[ts(skip)]
    #[serde(default)]
    pub messages_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PackageIndex {
    #[ts(type = "string")]
    pub path: PathBuf,
    pub owner_id: Option<Snowflake>,
    pub owner_name: Option<String>,
    pub total_messages: u64,
    pub channels: Vec<PackageChannel>,
}

#[derive(Deserialize)]
struct ChannelFile {
    #[serde(default)]
    id: Option<Snowflake>,
    #[serde(rename = "type", default)]
    kind: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    guild: Option<GuildRef>,
    #[serde(default)]
    recipients: Vec<Value>,
}

#[derive(Deserialize)]
struct GuildRef {
    #[serde(default)]
    id: Option<Snowflake>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct UserFile {
    #[serde(default)]
    id: Option<Snowflake>,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(rename = "ID", alias = "id")]
    id: Snowflake,
    #[serde(rename = "Contents", alias = "contents", default)]
    contents: String,
    #[serde(rename = "Attachments", alias = "attachments", default)]
    attachments: String,
}

struct Row {
    id: Snowflake,
    contents: String,
    attachments: String,
}

pub fn read_index(path: &Path) -> Result<PackageIndex, PackageError> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(path)?))?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .map(|name| name.into_owned())
        .collect();

    let (owner_id, owner_name) = find_owner(&mut archive, &names);
    let display_names = find_display_names(&mut archive, &names);

    let mut channels = Vec::new();
    let mut total_messages = 0;
    for channel_path in names.iter().filter(|name| is_channel_file(name)) {
        let directory = &channel_path[..channel_path.len() - "channel.json".len()];
        let Some(messages_path) = find_messages_file(&names, directory) else {
            continue;
        };
        let Ok(text) = read_text(&mut archive, channel_path) else {
            continue;
        };
        let Ok(file) = serde_json::from_str::<ChannelFile>(&text) else {
            continue;
        };
        let Some(id) = file.id.or_else(|| channel_id_from_directory(directory)) else {
            continue;
        };

        let rows = read_rows(&mut archive, &messages_path)?;
        let message_count = rows.len() as u64;
        total_messages += message_count;
        let first_message_at = rows
            .iter()
            .map(|row| row.id)
            .min()
            .map(Snowflake::timestamp_ms);
        let last_message_at = rows
            .iter()
            .map(|row| row.id)
            .max()
            .map(Snowflake::timestamp_ms);

        let kind = classify(&file);
        let name = channel_name(&file, kind, display_names.get(&id.to_string()));
        channels.push(PackageChannel {
            id,
            kind,
            name,
            guild_id: file.guild.as_ref().and_then(|guild| guild.id),
            guild_name: file.guild.as_ref().and_then(|guild| guild.name.clone()),
            message_count,
            first_message_at,
            last_message_at,
            messages_path,
        });
    }

    if channels.is_empty() {
        return Err(PackageError::NoMessages);
    }
    channels.sort_by_key(|channel| std::cmp::Reverse(channel.last_message_at));
    Ok(PackageIndex {
        path: path.to_path_buf(),
        owner_id,
        owner_name,
        total_messages,
        channels,
    })
}

pub fn read_channel(
    path: &Path,
    channel: &PackageChannel,
    owner: Snowflake,
) -> Result<Vec<Message>, PackageError> {
    let mut archive = ZipArchive::new(BufReader::new(File::open(path)?))?;
    let rows = read_rows(&mut archive, &channel.messages_path)?;
    let mut messages: Vec<Message> = rows
        .into_iter()
        .map(|row| to_message(row, channel.id, owner))
        .collect::<Result<_, _>>()
        .map_err(|error: serde_json::Error| {
            PackageError::Malformed(format!("line {}", error.line()))
        })?;
    messages.sort_by_key(|message| std::cmp::Reverse(message.id));
    Ok(messages)
}

fn to_message(
    row: Row,
    channel: Snowflake,
    owner: Snowflake,
) -> Result<Message, serde_json::Error> {
    let attachments: Vec<Value> = row
        .attachments
        .split_whitespace()
        .map(|url| {
            let filename = url
                .split('?')
                .next()
                .unwrap_or(url)
                .rsplit('/')
                .next()
                .unwrap_or("file");
            json!({ "id": "0", "filename": filename, "url": url })
        })
        .collect();
    serde_json::from_value(json!({
        "id": row.id,
        "channel_id": channel,
        "author": { "id": owner, "username": "you" },
        "type": 0,
        "content": row.contents,
        "attachments": attachments,
    }))
}

fn is_channel_file(name: &str) -> bool {
    let Some(directory) = name.strip_suffix("channel.json") else {
        return false;
    };
    directory.ends_with('/') && channel_id_from_directory(directory).is_some()
}

fn channel_id_from_directory(directory: &str) -> Option<Snowflake> {
    let folder = directory.trim_end_matches('/').rsplit('/').next()?;
    folder
        .strip_prefix(['c', 'C'])
        .unwrap_or(folder)
        .parse()
        .ok()
}

fn find_messages_file(names: &[String], directory: &str) -> Option<String> {
    ["messages.json", "messages.csv"].iter().find_map(|file| {
        let wanted = format!("{directory}{file}");
        names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&wanted))
            .cloned()
    })
}

fn find_owner<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    names: &[String],
) -> (Option<Snowflake>, Option<String>) {
    for name in names
        .iter()
        .filter(|name| name.to_ascii_lowercase().ends_with("/user.json"))
    {
        let Ok(text) = read_text(archive, name) else {
            continue;
        };
        if let Ok(user) = serde_json::from_str::<UserFile>(&text)
            && user.id.is_some()
        {
            return (user.id, user.username);
        }
    }
    (None, None)
}

fn find_display_names<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    names: &[String],
) -> HashMap<String, String> {
    let lookup = names
        .iter()
        .filter(|name| name.to_ascii_lowercase().ends_with("/index.json"));
    for name in lookup {
        let Ok(text) = read_text(archive, name) else {
            continue;
        };
        let Ok(map) = serde_json::from_str::<HashMap<String, Option<String>>>(&text) else {
            continue;
        };
        if map.keys().all(|key| key.parse::<Snowflake>().is_ok()) && !map.is_empty() {
            return map
                .into_iter()
                .filter_map(|(id, value)| Some((id, value?)))
                .collect();
        }
    }
    HashMap::new()
}

fn classify(file: &ChannelFile) -> PackageChannelKind {
    if file.guild.is_some() {
        return PackageChannelKind::Server;
    }
    match &file.kind {
        Some(Value::Number(number)) => match number.as_u64() {
            Some(1) => PackageChannelKind::Direct,
            Some(3) => PackageChannelKind::Group,
            _ => PackageChannelKind::Other,
        },
        Some(Value::String(text)) => match text.to_ascii_uppercase().as_str() {
            "DM" => PackageChannelKind::Direct,
            "GROUP_DM" | "GROUPDM" => PackageChannelKind::Group,
            _ => PackageChannelKind::Other,
        },
        _ if file.recipients.len() == 2 => PackageChannelKind::Direct,
        _ if file.recipients.len() > 2 => PackageChannelKind::Group,
        _ => PackageChannelKind::Other,
    }
}

fn channel_name(
    file: &ChannelFile,
    kind: PackageChannelKind,
    from_index: Option<&String>,
) -> String {
    if let Some(name) = file.name.as_deref().filter(|name| !name.is_empty()) {
        return if kind == PackageChannelKind::Server {
            format!("#{name}")
        } else {
            name.to_owned()
        };
    }
    if let Some(name) = from_index.filter(|name| !name.is_empty() && name.as_str() != "None") {
        return name
            .strip_prefix("Direct Message with ")
            .unwrap_or(name)
            .to_owned();
    }
    match kind {
        PackageChannelKind::Direct => "Closed DM".to_owned(),
        PackageChannelKind::Group => "Group DM".to_owned(),
        _ => "Unknown channel".to_owned(),
    }
}

fn read_text<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String, PackageError> {
    let entry = archive.by_name(name)?;
    let mut text = String::new();
    entry.take(MAX_ENTRY_BYTES).read_to_string(&mut text)?;
    Ok(text)
}

fn read_rows<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<Row>, PackageError> {
    let text = read_text(archive, name)?;
    let text = text.trim_start_matches('\u{feff}');
    if name.to_ascii_lowercase().ends_with(".json") {
        parse_json_rows(text)
    } else {
        parse_csv_rows(text)
    }
}

fn parse_json_rows(text: &str) -> Result<Vec<Row>, PackageError> {
    let raw: Vec<RawMessage> = serde_json::from_str(text)
        .map_err(|error| PackageError::Malformed(format!("line {}", error.line())))?;
    Ok(raw
        .into_iter()
        .map(|row| Row {
            id: row.id,
            contents: row.contents,
            attachments: row.attachments,
        })
        .collect())
}

fn parse_csv_rows(text: &str) -> Result<Vec<Row>, PackageError> {
    let mut records = split_csv(text).into_iter();
    let Some(header) = records.next() else {
        return Ok(Vec::new());
    };
    let column = |wanted: &str| {
        header
            .iter()
            .position(|name| name.trim().eq_ignore_ascii_case(wanted))
    };
    let id_column = column("ID")
        .ok_or_else(|| PackageError::Malformed("the CSV has no ID column".to_owned()))?;
    let contents_column = column("Contents");
    let attachments_column = column("Attachments");

    let cell = |record: &[String], column: Option<usize>| {
        column
            .and_then(|i| record.get(i))
            .cloned()
            .unwrap_or_default()
    };
    Ok(records
        .filter_map(|record| {
            let id = record.get(id_column)?.trim().parse().ok()?;
            Some(Row {
                id,
                contents: cell(&record, contents_column),
                attachments: cell(&record, attachments_column),
            })
        })
        .collect())
}

fn split_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match (quoted, character) {
            (true, '"') if characters.peek() == Some(&'"') => {
                field.push('"');
                characters.next();
            }
            (true, '"') => quoted = false,
            (true, other) => field.push(other),
            (false, '"') => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            (false, other) => field.push(other),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn write_zip(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("package.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, body) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        (directory, path)
    }

    #[test]
    fn reads_a_modern_package_with_bare_integer_ids() {
        let (_guard, path) = write_zip(&[
            ("Account/user.json", r#"{"id": "42", "username": "kit"}"#),
            (
                "Messages/index.json",
                r#"{"100": "Direct Message with Ada", "200": "general in Rust Harbor"}"#,
            ),
            (
                "Messages/c100/channel.json",
                r#"{"id": "100", "type": "DM", "recipients": ["42", "7"]}"#,
            ),
            (
                "Messages/c100/messages.json",
                r#"[{"ID": 1175928847299117063, "Timestamp": "2024-07-10 16:45:23", "Contents": "hello", "Attachments": ""}]"#,
            ),
            (
                "Messages/c200/channel.json",
                r#"{"id": "200", "type": 0, "name": "general", "guild": {"id": "9", "name": "Rust Harbor"}}"#,
            ),
            (
                "Messages/c200/messages.json",
                r#"[{"ID": 5, "Contents": "a", "Attachments": "https://cdn.discordapp.com/attachments/1/2/pic.png?ex=1"}, {"ID": 6, "Contents": "b", "Attachments": ""}]"#,
            ),
        ]);

        let index = read_index(&path).unwrap();
        assert_eq!(index.owner_id, Some(Snowflake(42)));
        assert_eq!(index.total_messages, 3);

        let dm = index
            .channels
            .iter()
            .find(|c| c.id == Snowflake(100))
            .unwrap();
        assert_eq!(dm.kind, PackageChannelKind::Direct);
        assert_eq!(dm.name, "Ada");

        let server = index
            .channels
            .iter()
            .find(|c| c.id == Snowflake(200))
            .unwrap();
        assert_eq!(server.kind, PackageChannelKind::Server);
        assert_eq!(server.name, "#general");
        assert_eq!(server.guild_name.as_deref(), Some("Rust Harbor"));

        let messages = read_channel(&path, server, Snowflake(42)).unwrap();
        assert_eq!(messages.iter().map(|m| m.id.0).collect::<Vec<_>>(), [6, 5]);
        assert_eq!(messages[0].author.id, Snowflake(42));
        assert_eq!(messages[1].attachments[0].filename, "pic.png");
    }

    #[test]
    fn finds_translated_folders_and_old_csv_packages_without_the_c_prefix() {
        let (_guard, path) = write_zip(&[
            ("Berichten/123/channel.json", r#"{"id": "123", "type": 1}"#),
            (
                "Berichten/123/messages.csv",
                "ID,Timestamp,Contents,Attachments\n11,2020-01-01,\"one, with \"\"quotes\"\"\nand a line break\",\n12,2020-01-02,two,\n",
            ),
        ]);

        let index = read_index(&path).unwrap();
        assert_eq!(index.channels.len(), 1);

        let messages = read_channel(&path, &index.channels[0], Snowflake(1)).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[1].content,
            "one, with \"quotes\"\nand a line break"
        );
    }

    #[test]
    fn explains_when_a_zip_is_not_a_package() {
        let (_guard, path) = write_zip(&[("notes.txt", "hello")]);
        assert!(matches!(read_index(&path), Err(PackageError::NoMessages)));
    }

    #[test]
    fn explains_when_a_file_is_not_a_zip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("package.zip");
        std::fs::write(&path, "not a zip").unwrap();
        assert!(matches!(read_index(&path), Err(PackageError::NotZip(_))));
    }

    #[test]
    fn a_byte_order_mark_does_not_break_parsing() {
        let (_guard, path) = write_zip(&[
            ("m/c1/channel.json", r#"{"id": "1", "type": 1}"#),
            (
                "m/c1/messages.json",
                "\u{feff}[{\"ID\": 2, \"Contents\": \"x\"}]",
            ),
        ]);
        assert_eq!(read_index(&path).unwrap().total_messages, 1);
    }
}
