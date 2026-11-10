use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Snowflake;
use crate::api::{Message, Transport};

const JSONL_NAME: &str = "messages.jsonl";
const TRANSCRIPT_NAME: &str = "transcript.html";
const ATTACHMENTS_DIRECTORY: &str = "attachments";
const MAX_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;

const ATTACHMENT_HOSTS: [&str; 2] = ["cdn.discordapp.com", "media.discordapp.net"];

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("Couldn't write the archive: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Serialize, Deserialize)]
struct ArchivedMessage {
    conversation: String,
    message: Value,
    #[serde(default)]
    saved_attachments: Vec<SavedAttachment>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedAttachment {
    filename: String,
    path: String,
}

pub struct Archive {
    directory: PathBuf,
    writer: BufWriter<File>,
    downloader: Option<Arc<dyn Transport>>,
}

impl Archive {
    pub fn open(
        directory: &Path,
        downloader: Option<Arc<dyn Transport>>,
    ) -> Result<Self, ArchiveError> {
        fs::create_dir_all(directory)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join(JSONL_NAME))?;
        Ok(Self {
            directory: directory.to_path_buf(),
            writer: BufWriter::new(file),
            downloader,
        })
    }

    pub async fn record(
        &mut self,
        message: &Message,
        conversation: &str,
    ) -> Result<Vec<String>, ArchiveError> {
        let mut warnings = Vec::new();
        let mut saved_attachments = Vec::new();

        if let Some(downloader) = self.downloader.clone() {
            for attachment in &message.attachments {
                if !is_discord_cdn(&attachment.url) {
                    warnings.push(format!(
                        "Skipped {}: not hosted by Discord",
                        attachment.filename
                    ));
                    continue;
                }
                match downloader
                    .download(&attachment.url, MAX_ATTACHMENT_BYTES)
                    .await
                {
                    Ok(bytes) => {
                        let name =
                            format!("{}_{}", message.id, safe_filename(&attachment.filename));
                        let folder = self.directory.join(ATTACHMENTS_DIRECTORY);
                        fs::create_dir_all(&folder)?;
                        fs::write(folder.join(&name), &bytes)?;
                        saved_attachments.push(SavedAttachment {
                            filename: attachment.filename.clone(),
                            path: format!("{ATTACHMENTS_DIRECTORY}/{name}"),
                        });
                    }
                    Err(error) => warnings.push(format!(
                        "Couldn't save {}: {}",
                        attachment.filename, error.0
                    )),
                }
            }
        }

        let record = ArchivedMessage {
            conversation: conversation.to_owned(),
            message: serde_json::to_value(message).unwrap_or(Value::Null),
            saved_attachments,
        };
        serde_json::to_writer(&mut self.writer, &record).map_err(std::io::Error::from)?;
        self.writer.write_all(b"\n")?;

        self.writer.flush()?;
        Ok(warnings)
    }

    pub fn finish(mut self) -> Result<PathBuf, ArchiveError> {
        self.writer.flush()?;
        write_transcript(&self.directory)?;
        Ok(self.directory)
    }
}

fn is_discord_cdn(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    ATTACHMENT_HOSTS.contains(&host)
}

fn safe_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    if cleaned.is_empty() {
        "file".to_owned()
    } else {
        cleaned
    }
}

#[must_use]
pub fn safe_directory_name(label: &str) -> String {
    let cleaned = safe_filename(label).replace('/', "_");
    cleaned
        .chars()
        .take(60)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn write_transcript(directory: &Path) -> Result<(), ArchiveError> {
    let reader = BufReader::new(File::open(directory.join(JSONL_NAME))?);
    let mut conversations: BTreeMap<String, Vec<ArchivedMessage>> = BTreeMap::new();
    for line in reader.lines() {
        if let Ok(record) = serde_json::from_str::<ArchivedMessage>(&line?) {
            conversations
                .entry(record.conversation.clone())
                .or_default()
                .push(record);
        }
    }

    let mut html = BufWriter::new(File::create(directory.join(TRANSCRIPT_NAME))?);
    html.write_all(TRANSCRIPT_HEAD.as_bytes())?;
    for (conversation, mut messages) in conversations {
        messages.sort_by_key(|record| message_id(&record.message));

        messages.dedup_by_key(|record| message_id(&record.message));
        write!(
            html,
            "<h2>{} <small>{}</small></h2>",
            escape(&conversation),
            messages.len()
        )?;
        for record in &messages {
            write_message(&mut html, record)?;
        }
    }
    html.write_all(b"</main></body></html>\n")?;
    html.flush()?;
    Ok(())
}

fn message_id(message: &Value) -> Snowflake {
    message
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse().ok())
        .unwrap_or_default()
}

fn write_message(html: &mut impl Write, record: &ArchivedMessage) -> std::io::Result<()> {
    let message = &record.message;
    let time = chrono::DateTime::from_timestamp_millis(message_id(message).timestamp_ms())
        .map(|moment| moment.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default();
    let author = message
        .pointer("/author/global_name")
        .and_then(Value::as_str)
        .or_else(|| message.pointer("/author/username").and_then(Value::as_str))
        .unwrap_or("Unknown");
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();

    write!(
        html,
        "<article><header><b>{}</b><time>{}</time></header><p>{}</p>",
        escape(author),
        escape(&time),
        escape(content)
    )?;
    for attachment in &record.saved_attachments {
        write!(
            html,
            "<a href=\"{}\">{}</a>",
            escape(&attachment.path),
            escape(&attachment.filename)
        )?;
    }
    let names = message
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for attachment in names {
        let filename = attachment
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("file");
        if !record
            .saved_attachments
            .iter()
            .any(|saved| saved.filename == filename)
        {
            write!(html, "<span>{}</span>", escape(filename))?;
        }
    }
    html.write_all(b"</article>")
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

const TRANSCRIPT_HEAD: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src 'self'">
<title>Disweep archive</title>
<style>
:root{color-scheme:light dark;--bg:#fff;--fg:#14161c;--muted:#68707f;--line:#e4e7ee}
@media(prefers-color-scheme:dark){:root{--bg:#0b0d13;--fg:#e8eaf0;--muted:#8a93a5;--line:#1e222d}}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}
main{max-width:760px;margin:0 auto;padding:32px 20px 64px}
h1{font-size:20px;margin:0 0 4px}h2{font-size:16px;margin:36px 0 8px;padding-bottom:8px;border-bottom:1px solid var(--line)}
small{color:var(--muted);font-weight:400}
article{padding:10px 0;border-bottom:1px solid var(--line)}
header{display:flex;gap:10px;align-items:baseline}time{color:var(--muted);font-size:12px}
p{margin:2px 0 0;white-space:pre-wrap;overflow-wrap:anywhere}
a,span{display:inline-block;margin:6px 8px 0 0;font-size:13px}span{color:var(--muted)}
</style></head><body><main><h1>Disweep archive</h1><small>A copy saved before these messages were deleted.</small>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApiRequest, ApiResponse, TransportError};
    use async_trait::async_trait;
    use bytes::Bytes;
    use parking_lot::Mutex;
    use serde_json::json;

    fn message(content: &str, attachments: Value) -> Message {
        serde_json::from_value(json!({
            "id": "1175928847299117063", "channel_id": "9", "content": content,
            "author": {"id": "1", "username": "kit"}, "attachments": attachments
        }))
        .unwrap()
    }

    #[derive(Default)]
    struct Cdn {
        asked: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Transport for Cdn {
        async fn execute(&self, _: &ApiRequest, _: &str) -> Result<ApiResponse, TransportError> {
            unreachable!("archives only download files")
        }

        async fn download(&self, url: &str, _max_bytes: u64) -> Result<Bytes, TransportError> {
            self.asked.lock().push(url.to_owned());
            Ok(Bytes::from_static(b"file-bytes"))
        }
    }

    #[tokio::test]
    async fn writes_a_copy_and_an_escaped_transcript() {
        let directory = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(directory.path(), None).unwrap();
        archive
            .record(&message("<script>alert(1)</script>", json!([])), "Ada & Bo")
            .await
            .unwrap();
        archive.finish().unwrap();

        let jsonl = fs::read_to_string(directory.path().join(JSONL_NAME)).unwrap();
        assert!(jsonl.contains("alert(1)"));

        let html = fs::read_to_string(directory.path().join(TRANSCRIPT_NAME)).unwrap();
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("Ada &amp; Bo"));
        assert!(!html.contains("<script>"));
    }

    #[tokio::test]
    async fn the_copy_is_on_disk_before_record_returns() {
        let directory = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(directory.path(), None).unwrap();
        archive
            .record(&message("hello", json!([])), "c")
            .await
            .unwrap();

        let jsonl = fs::read_to_string(directory.path().join(JSONL_NAME)).unwrap();
        assert_eq!(jsonl.lines().count(), 1);
    }

    #[tokio::test]
    async fn a_page_saved_twice_appears_once_in_the_transcript() {
        let directory = tempfile::tempdir().unwrap();
        for _ in 0..2 {
            let mut archive = Archive::open(directory.path(), None).unwrap();
            archive
                .record(&message("only once", json!([])), "c")
                .await
                .unwrap();
            archive.finish().unwrap();
        }
        let html = fs::read_to_string(directory.path().join(TRANSCRIPT_NAME)).unwrap();
        assert_eq!(html.matches("only once").count(), 1);
    }

    #[tokio::test]
    async fn reopening_appends_rather_than_replacing() {
        let directory = tempfile::tempdir().unwrap();
        for text in ["first", "second"] {
            let mut archive = Archive::open(directory.path(), None).unwrap();
            archive
                .record(&message(text, json!([])), "c")
                .await
                .unwrap();
        }
        let jsonl = fs::read_to_string(directory.path().join(JSONL_NAME)).unwrap();
        assert_eq!(jsonl.lines().count(), 2);
    }

    #[tokio::test]
    async fn only_downloads_from_discord_and_keeps_names_inside_the_folder() {
        let directory = tempfile::tempdir().unwrap();
        let cdn = Arc::new(Cdn::default());
        let mut archive = Archive::open(directory.path(), Some(cdn.clone())).unwrap();
        let attachments = json!([
            {"id": "1", "filename": "../../evil.exe", "url": "https://cdn.discordapp.com/attachments/1/2/a.bin"},
            {"id": "2", "filename": "tracker.png", "url": "https://evil.example/x.png"},
            {"id": "3", "filename": "lookalike.png", "url": "https://cdn.discordapp.com.evil.example/x.png"},
        ]);

        let warnings = archive
            .record(&message("x", attachments), "c")
            .await
            .unwrap();

        assert_eq!(cdn.asked.lock().len(), 1);
        assert_eq!(warnings.len(), 2);
        let saved: Vec<_> = fs::read_dir(directory.path().join(ATTACHMENTS_DIRECTORY))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(saved, ["1175928847299117063_evil.exe"]);
    }

    #[test]
    fn filenames_are_made_safe() {
        assert_eq!(safe_filename("..\\..\\win.ini"), "win.ini");
        assert_eq!(safe_filename("a<b>:c?.txt"), "a_b__c_.txt");
        assert_eq!(safe_filename("..."), "file");
        assert_eq!(safe_filename("na\nme"), "na_me");
    }
}
